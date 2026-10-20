#!/usr/bin/env bash
#
# On-chain regression test runner for memo-mint, memo-burn and memo-home
#
# Runs the regression suites (clients/regression/src/) against a local solana-test-validator
# started by ./scripts/local-validator.sh. Nothing is sent to testnet or mainnet.
#
#   Phase 1: validator with a supply of 18.4T tokens held by the test wallet
#            regression-memo-curve (walks the supply down to ~5M), then
#            regression-memo-mint, regression-memo-burn and regression-memo-home
#            (5M supply when the curve suite is not selected)
#   Phase 2: validator without MintState / GlobalBurnStats (fresh deployment)
#            regression-admin-init
#
# Usage:
#   ./scripts/run-regression-tests.sh                 # build programs, run every suite
#   ./scripts/run-regression-tests.sh --no-build      # use the existing target/deploy/*.so
#   ./scripts/run-regression-tests.sh memo-burn       # only some suites (curve, mint, burn, home, admin-init)
#   ./scripts/run-regression-tests.sh --so-dir <dir>  # load <name>.so from <dir> (e.g. mutation testing)
#   ./scripts/run-regression-tests.sh --keep          # leave the last validator running
#   ./scripts/run-regression-tests.sh --help
#
# Each suite asserts exact outcomes (error codes, amounts, statistics) and exits non-zero on any
# failure; this script exits non-zero if any suite fails.

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'
BOLD='\033[1m'

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
RPC_URL="http://127.0.0.1:8899"
WALLET="$ROOT/target/local-validator/wallet.json"
LOG_DIR="$ROOT/target/regression-logs"

ALL_SUITES=(curve mint burn home admin-init)
# The curve suite needs a supply above 10T held by the wallet; mint/burn alone run at 5M
CURVE_ARGS=(--supply 18400000000000 --balance 18400000000000)
DEFAULT_ARGS=(--supply 5000000 --balance 1000000)

show_help() {
    sed -n '3,24p' "$0" | sed 's/^# \{0,1\}//'
}

print_header() {
    echo ""
    echo -e "${CYAN}${BOLD}========================================${NC}"
    echo -e "${CYAN}${BOLD}  $1${NC}"
    echo -e "${CYAN}${BOLD}========================================${NC}"
    echo ""
}

BUILD=true
KEEP=false
SO_DIR=""
SUITES=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h) show_help; exit 0 ;;
        --no-build) BUILD=false; shift ;;
        --keep) KEEP=true; shift ;;
        --so-dir) SO_DIR="$(cd "$2" && pwd)"; BUILD=false; shift 2 ;;
        --*) echo "Unknown option: $1"; exit 1 ;;
        *)
            s="${1#memo-}"
            [[ " ${ALL_SUITES[*]} " == *" $s "* ]] || { echo "Unknown suite: $1 (available: ${ALL_SUITES[*]})"; exit 1; }
            SUITES+=("$s"); shift ;;
    esac
done
[ ${#SUITES[@]} -eq 0 ] && SUITES=("${ALL_SUITES[@]}")

wants() { [[ " ${SUITES[*]} " == *" $1 "* ]]; }

bin_name() {
    case $1 in
        admin-init) echo "regression-admin-init" ;;
        *) echo "regression-memo-$1" ;;
    esac
}

start_validator() {
    local args=(--no-build "$@")
    [ -n "$SO_DIR" ] && args+=(--so-dir "$SO_DIR")
    ./scripts/local-validator.sh start "${args[@]}"
}

stop_validator() {
    ./scripts/local-validator.sh stop >/dev/null 2>&1 || true
}

PASSED=()
FAILED=()

run_suite() {
    local suite=$1 bin log
    bin=$(bin_name "$suite")
    log="$LOG_DIR/$bin.log"
    print_header "Running $bin"
    if (cd clients && MEMO_PROGRAM_ENV=testnet MEMO_RPC_URL="$RPC_URL" MEMO_WALLET="$WALLET" \
            cargo run --quiet --bin "$bin") 2>&1 | tee "$log"; then
        PASSED+=("$suite")
    else
        FAILED+=("$suite")
    fi
}

mkdir -p "$LOG_DIR"
trap '[ "$KEEP" = true ] || stop_validator' EXIT

if [ "$BUILD" = true ]; then
    print_header "Building programs (testnet IDs)"
    anchor build
fi
# Test-only programs for the home suite (clients/regression/test-programs): a Token-2022 transfer
# hook and an app program that calls memo-home by CPI
if wants home && { [ "$BUILD" = true ] || [ ! -f target/test-programs/home_test_app.so ] \
        || [ ! -f target/test-programs/transfer_hook_test.so ]; }; then
    print_header "Building test programs"
    for p in transfer-hook home-app; do
        cargo build-sbf --manifest-path "clients/regression/test-programs/$p/Cargo.toml" --sbf-out-dir target/test-programs
    done
fi
print_header "Building regression clients"
(cd clients && cargo build --quiet --bin regression-memo-curve --bin regression-memo-mint \
    --bin regression-memo-burn --bin regression-memo-home --bin regression-admin-init --bin prepare-local-validator \
    --bin init-user-global-burn-stats)

set -o pipefail
if wants curve || wants mint || wants burn || wants home; then
    if wants curve; then
        print_header "Phase 1: local validator, supply 18.4T"
        start_validator "${CURVE_ARGS[@]}"
    else
        print_header "Phase 1: local validator, supply 5M"
        start_validator "${DEFAULT_ARGS[@]}"
    fi
    for s in curve mint burn home; do
        if wants "$s"; then run_suite "$s"; fi
    done
    stop_validator
fi

if wants admin-init; then
    print_header "Phase 2: local validator without MintState / GlobalBurnStats"
    start_validator --no-core-state
    run_suite admin-init
fi

print_header "Regression Test Summary"
for s in "${PASSED[@]}"; do echo -e "  ${GREEN}PASS${NC} $(bin_name "$s")"; done
for s in "${FAILED[@]}"; do echo -e "  ${RED}FAIL${NC} $(bin_name "$s") (log: target/regression-logs/$(bin_name "$s").log)"; done
echo ""
if [ ${#FAILED[@]} -gt 0 ]; then
    echo -e "${RED}${BOLD}${#FAILED[@]} suite(s) failed${NC}"
    exit 1
fi
echo -e "${GREEN}${BOLD}All ${#PASSED[@]} suite(s) passed${NC}"
