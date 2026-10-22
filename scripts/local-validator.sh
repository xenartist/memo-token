#!/usr/bin/env bash
#
# Local validator for threshold / large-amount testing
#
# Runs solana-test-validator with the programs from `anchor build` (testnet IDs) and state
# copied from testnet, with the MEMO supply and the test wallet's balance set to any value.
# Nothing is deployed and no keys need to be unlocked.
#
# Usage:
#   ./scripts/local-validator.sh start [--supply <tokens>] [--balance <tokens>] [--no-build]
#                                      [--no-core-state] [--so-dir <dir>] [--refresh-external]
#   ./scripts/local-validator.sh stop
#   ./scripts/local-validator.sh status
#
# Then run the smoke tests against it:
#   ./scripts/run-smoke-tests.sh --local memo-chat
# or any client with:
#   MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json cargo run --bin <bin>
#
# --supply   MEMO supply in tokens (default: testnet's current supply), e.g. 10000000.000001
# --balance  test wallet's MEMO balance in tokens (default: 1000000)
# --no-build skip `anchor build` (use the existing target/deploy/*.so)
# --no-core-state  leave out MintState and GlobalBurnStats (a fresh deployment before the admin
#            initializes them; used by the regression suite to test the admin-only init)
# --so-dir   load <name>.so from this directory instead of target/deploy when present
#            (e.g. a mutated build for mutation testing); implies --no-build
# --refresh-external  dump the external programs (Metaplex Core, Token-2022, SPL Token, ATA) from
#            X1 mainnet again instead of using the copies in target/local-validator/external
#
# The external programs are X1 mainnet's own builds, not the ones bundled with the test
# validator. Each start prints their sha256 and warns when one differs from the hash pinned
# below (the version the regression suites were last checked against).

set -e

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
DIR="$ROOT/target/local-validator"
LEDGER="$DIR/ledger"
ACCOUNTS="$DIR/accounts"
WALLET="$DIR/wallet.json"
PID_FILE="$DIR/validator.pid"
LOG="$DIR/validator.log"
RPC_URL="http://127.0.0.1:8899"
AIRDROP_SOL=1000
MAINNET_RPC_URL="https://rpc.mainnet.x1.xyz"
# External programs, dumped from X1 mainnet into target/local-validator/external on first start:
# <file name> <program id> <pinned sha256 of the dump>
EXTERNAL_PROGRAMS="mpl_core CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d e471b0c92e1f70eab2e7e6ee9e39968b6506d3abf4ea86c6a8a1d876308f098c
spl_token_2022 TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb d32c6364ed5d2a0ad68a8f4b04baff9b24d2b7e60df1bcdd1894071d07d69de6
spl_token TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA 8190d3f7ceb6cb7a7a8d8924bff89f9f611e15ce1f806f2b6237f3311a98f697
spl_associated_token_account ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL 9ba8d4dfec9ca6ba8480f7f4edcc1a90184271db44e238ea0f09bba47ad2d8a9"
# Test-only programs (clients/regression/test-programs, built by run-regression-tests.sh into
# target/test-programs): <file name> <program id>
TEST_PROGRAMS="transfer_hook_test HCwbzCbQRg4YH2pDkwa9hDDGqu64axLquG6mAKSdnbhr
home_test_app EZzeE6EZ7oD3Qsr5F1Wuu5TVHgbzDYL4LiZCxaSPR6c2
home_web_test 6uknhjicEvg7B7oAvZ3y7TWojo8YfvKicn7HFVDPtiip"

# Programs from Anchor.toml [programs.testnet]: <name> <program id>
PROGRAMS=$(awk '/^\[programs.testnet\]/{f=1;next} /^\[/{f=0} f && /=/{gsub(/[" ]/,""); split($0,a,"="); print a[1], a[2]}' Anchor.toml)

is_running() {
    [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null
}

stop_validator() {
    if is_running; then
        kill "$(cat "$PID_FILE")"
        for _ in $(seq 1 30); do
            is_running || break
            sleep 0.5
        done
        echo "Local validator stopped"
    else
        echo "Local validator is not running"
    fi
    rm -f "$PID_FILE"
}

status() {
    if is_running; then
        echo "Local validator running (pid $(cat "$PID_FILE")), RPC $RPC_URL"
        echo "Wallet: $(solana-keygen pubkey "$WALLET")"
    else
        echo "Local validator is not running"
    fi
}

start() {
    local supply="" balance="1000000" build=true core_state=true so_dir="" refresh=false
    while [[ $# -gt 0 ]]; do
        case $1 in
            --supply) supply="$2"; shift 2 ;;
            --balance) balance="$2"; shift 2 ;;
            --no-build) build=false; shift ;;
            --no-core-state) core_state=false; shift ;;
            --so-dir) so_dir="$(cd "$2" && pwd)"; build=false; shift 2 ;;
            --refresh-external) refresh=true; shift ;;
            *) echo "Unknown option: $1"; exit 1 ;;
        esac
    done

    command -v solana-test-validator >/dev/null || { echo "solana-test-validator not found"; exit 1; }
    is_running && stop_validator

    mkdir -p "$DIR"
    rm -rf "$LEDGER" "$ACCOUNTS"

    # The .so files must carry the testnet program IDs (the default build, no mainnet feature)
    if [ "$build" = true ]; then
        echo "Building programs (testnet IDs)..."
        anchor build
    fi

    if [ ! -f "$WALLET" ]; then
        solana-keygen new --no-bip39-passphrase --silent --force -o "$WALLET" >/dev/null
    fi
    local wallet_pubkey
    wallet_pubkey=$(solana-keygen pubkey "$WALLET")

    echo "Preparing accounts..."
    local prepare_args=(--out-dir "$ACCOUNTS" --wallet "$wallet_pubkey" --balance "$balance")
    [ -n "$supply" ] && prepare_args+=(--supply "$supply")
    (cd clients && MEMO_PROGRAM_ENV=testnet cargo run --quiet --bin prepare-local-validator -- "${prepare_args[@]}")

    if [ "$core_state" = false ]; then
        local name seed pda
        for name_seed in "memo_mint:mint_state" "memo_burn:global_burn_stats"; do
            name="${name_seed%%:*}"
            seed="${name_seed#*:}"
            pda=$(solana find-program-derived-address "$(awk -v n="$name" '$1 == n {print $2}' <<< "$PROGRAMS")" "string:$seed" | awk '{print $1}')
            rm -f "$ACCOUNTS/$pda.json"
            echo "Left out $seed ($pda)"
        done
    fi

    local validator_args=(--reset --quiet --ledger "$LEDGER" --account-dir "$ACCOUNTS")
    while read -r name id; do
        local so="$ROOT/target/deploy/$name.so"
        if [ -n "$so_dir" ] && [ -f "$so_dir/$name.so" ]; then
            so="$so_dir/$name.so"
            echo "Using $so for $name"
        fi
        [ -f "$so" ] || { echo "Missing $so (run without --no-build)"; exit 1; }
        validator_args+=(--bpf-program "$id" "$so")
    done <<< "$PROGRAMS"

    # External programs, X1 mainnet's builds (public, read-only); user programs given with
    # --bpf-program replace the test validator's bundled SPL programs
    mkdir -p "$DIR/external"
    local file id pinned ext_so hash
    while read -r file id pinned; do
        ext_so="$DIR/external/$file.so"
        if [ "$refresh" = true ] || [ ! -f "$ext_so" ]; then
            echo "Dumping $file ($id) from X1 mainnet..."
            solana program dump -u "$MAINNET_RPC_URL" "$id" "$ext_so" >/dev/null
        fi
        hash=$(shasum -a 256 "$ext_so" | cut -c1-64)
        if [ "$hash" = "$pinned" ]; then
            echo "External $file: sha256 $hash (pinned)"
        else
            echo "WARNING: external $file: sha256 $hash differs from the pinned $pinned"
            echo "         (X1 upgraded it, or the dump is stale: re-run the suites, then update the pin)"
        fi
        validator_args+=(--bpf-program "$id" "$ext_so")
    done <<< "$EXTERNAL_PROGRAMS"
    # Test-only programs (regression-memo-home, the home:// extension prototype), when built
    while read -r file id; do
        if [ -f "$ROOT/target/test-programs/$file.so" ]; then
            validator_args+=(--bpf-program "$id" "$ROOT/target/test-programs/$file.so")
        fi
    done <<< "$TEST_PROGRAMS"

    echo "Starting solana-test-validator..."
    nohup solana-test-validator "${validator_args[@]}" > "$LOG" 2>&1 &
    echo $! > "$PID_FILE"

    for _ in $(seq 1 60); do
        if solana cluster-version -u "$RPC_URL" >/dev/null 2>&1; then
            break
        fi
        if ! is_running; then
            echo "Validator exited, see $LOG"
            tail -20 "$LOG"
            exit 1
        fi
        sleep 1
    done
    solana cluster-version -u "$RPC_URL" >/dev/null 2>&1 || { echo "Validator did not start, see $LOG"; exit 1; }

    solana airdrop "$AIRDROP_SOL" "$wallet_pubkey" -u "$RPC_URL" >/dev/null
    # The clients use finalized commitment by default, so wait until the airdrop is finalized
    for _ in $(seq 1 60); do
        [ "$(solana balance "$wallet_pubkey" -u "$RPC_URL" --commitment finalized --lamports | awk '{print $1}')" != "0" ] && break
        sleep 1
    done

    # Every app burns through memo-burn, which needs the wallet's burn stats account
    (cd clients && MEMO_PROGRAM_ENV=testnet MEMO_RPC_URL="$RPC_URL" MEMO_WALLET="$WALLET" \
        cargo run --quiet --bin init-user-global-burn-stats >/dev/null 2>&1) || true
    local burn_program stats_pda
    burn_program=$(awk '$1 == "memo_burn" {print $2}' <<< "$PROGRAMS")
    stats_pda=$(solana find-program-derived-address "$burn_program" string:user_global_burn_stats pubkey:"$wallet_pubkey" | awk '{print $1}')
    solana account "$stats_pda" -u "$RPC_URL" >/dev/null 2>&1 \
        || { echo "Failed to initialize the wallet's burn stats ($stats_pda)"; exit 1; }

    echo ""
    echo "Local validator ready: $RPC_URL (pid $(cat "$PID_FILE"))"
    echo "Wallet: $wallet_pubkey ($WALLET)"
    echo ""
    echo "Run smoke tests:  ./scripts/run-smoke-tests.sh --local [tests...]"
    echo "Other clients:    MEMO_RPC_URL=$RPC_URL MEMO_WALLET=$WALLET cargo run --bin <bin>"
    echo "Stop:             ./scripts/local-validator.sh stop"
}

case "${1:-}" in
    start) shift; start "$@" ;;
    stop) stop_validator ;;
    status) status ;;
    *)
        sed -n '3,34p' "$0" | sed 's/^# \{0,1\}//'
        exit 1
        ;;
esac
