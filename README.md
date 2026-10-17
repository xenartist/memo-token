# Memo Token

X1

## Overview

This project consists of:
- Two core programs on X1 (Solana-compatible): **memo-mint** (memo-based minting along a supply curve, PDA mint authority) and **memo-burn** (memo-based burning with per-user and global burn statistics)
- Five app programs built on them: memo-chat, memo-profile, memo-project, memo-blog and memo-forum
- Client tools for testing, administration and deployment checks (`clients/`)

MEMO is a Token-2022 token with 6 decimals. Every mint or burn must carry an SPL Memo instruction at index 0 of the transaction.

## Minting MEMO

Anyone can mint MEMO (Token-2022, 6 decimals) by sending a transaction to the memo-mint program.

**Transaction structure**:

```
[0]    SPL Memo, at least 69 bytes (required, must be the first instruction;
       no contract maximum, the transaction size limit applies)
[1..]  memo_mint::process_mint  (mint to yourself)
       or memo_mint::process_mint_to(recipient)  (mint to someone else's MEMO token account)
       -- exactly one mint per transaction --
[last] ComputeBudget set_compute_unit_limit (optional, never at index 0)
```

A transaction can mint only once, directly or through another program; a second mint fails with `OneMintPerTransaction`. Both mint instructions take the `MintState` PDA (seeds `[b"mint_state"]`) as their last account. X1 charges fees by the compute-unit limit you request, so simulate first and set the exact limit.

**Emission curve**: the reward per mint depends on the circulating supply before the mint. It is 1 MEMO up to and including 10M, then steps down by a tenth of the decade base just above every round number (a mint at exactly a round number still pays the previous step), so each 10x of supply pays 10x less.

| Supply before the mint | Reward per mint |
|---|---|
| up to and including 10M | 1 MEMO |
| 10M → 100M | 1 → 0.1: 0.9 above 10M, 0.8 above 20M, ..., 0.1 above 90M |
| 100M → 1B | 0.1 → 0.01: 0.09 above 100M, ..., 0.01 above 900M |
| 1B → 10B | 0.01 → 0.001: 0.009 above 1B, ..., 0.001 above 9B |
| each further 10x | 10x less, down to 0.000001 MEMO (1 lamport) above 9T, up to the 10T cap |

Circulating supply is capped at 10 trillion MEMO. Burns lower the circulating supply, so minting never stops permanently. See `MEMO-MINT-AUDIT.md` for details.

## Burn Statistics

memo-burn keeps per-user stats (`UserGlobalBurnStats`, seeds `[b"user_global_burn_stats", user]`) and one global account (`GlobalBurnStats`, seeds `[b"global_burn_stats"]`) with the total burned through memo-burn since the account was initialized (u128, in units of 10^-6 MEMO; on mainnet since October 15, 2026), the number of burn instructions, and the last burn time. Both counters stop at their maximum instead of overflowing. Every `process_burn`, direct or through an app, must pass the global account as its last account. The global account is not the historical total and not the total supply reduction; see Integration Notes.

The global account is created once by the admin with `cargo run --bin admin-init-global-burn-stats` (the deploy scripts do this right after deploying memo-burn). Burns fail until it exists, so memo-burn must be deployed and initialized before the apps.

## Integration Notes

Practical rules for wallets, clients, indexers and programs built on memo-mint and memo-burn. Details and evidence are in `MEMO-CORE-AUDIT-2026-10-16.md`.

### Transaction layout

- The SPL Memo instruction (Memo v3, `MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr`) must be the **top-level instruction at index 0**. The mint, burn or app instruction must be at index 1 or later. Programs that call memo-mint or memo-burn via CPI are bound by the same rule: the memo is always read from the transaction's top-level instruction 0.
- Compute budget instructions (`SetComputeUnitLimit`, `SetComputeUnitPrice`) must be at **index 1 or later**. A wallet or SDK that prepends them at index 0 makes the transaction fail with `MemoRequired`.
- **Durable nonce transactions are not supported.** The runtime only treats a transaction as a nonce transaction when `AdvanceNonceAccount` is at index 0, which conflicts with the memo rule. Offline-signing or custody flows that rely on durable nonces cannot mint or burn.

### Memo length

- On chain, memo-mint and memo-burn check the raw memo instruction bytes against a minimum of 69 bytes (`MemoTooShort` below that); there is no maximum other than the transaction size.
- In the project's design, 69 is the minimum the official client enforces on the **user's input before Borsh/Base64 encoding** (users type at least 69 characters). The encoded memo is then always far longer than 69.
- memo-burn and the apps expect standard, padded Base64 of Borsh data. Padded Base64 lengths are multiples of 4, so the shortest memo memo-burn can decode is 72 characters: a 69–71-character memo fails with `InvalidMemoFormat` (6001), not `MemoTooShort` (6010). This only matters if you build memos by hand.

### Minting

- **One mint per transaction.** A second mint in the same transaction fails with `OneMintPerTransaction` (6008), whether it comes from another top-level instruction or from a CPI, including the app instructions that mint (`send_memo_to_group`, `mint_for_blog`, `mint_for_post`).
- The reward is computed on chain from the current supply (see the curve above); callers cannot choose it.
- Re-sending the same instructions (for example a retry with a new blockhash or another fee payer) that lands in the same slot as the first transaction fails with 6008 and still pays the fee. Put a nonce or timestamp in the memo to make every mint transaction unique.
- Minting to any token account of the MEMO mint works, including accounts with CPI Guard enabled and multisig-owned accounts. `process_mint_to`'s `recipient` is only a consistency check against the token account owner, not an authorization.
- While circulating supply is at the 10T cap, every mint fails with `SupplyLimitReached` (6005).

### Burning

- **memo-burn minimum: 1 unit (0.000001 MEMO).** Any amount in units is allowed (no whole-token rule, no integer multiple), up to 1T MEMO per `process_burn` instruction. The memo's `burn_amount` must equal the instruction's `amount` exactly. Each user must call `initialize_user_global_burn_stats` once before their first burn. Note: until the mainnet memo-burn upgrade, mainnet still accepts whole tokens only (at least 1 MEMO); see `MEMO-BURN-AUDIT.md`.
- **App minimums scale with the current mint reward**: `minimum = multiplier × reward at the current supply` (1 MEMO per mint today, so the multiplier is the minimum in MEMO while supply is at or below 10M):

  | Operation | Multiplier |
  |---|---|
  | chat `create_chat_group` | 42,069 |
  | chat `burn_tokens_for_group` | 1 |
  | project `create_project`, `update_project` | 42,069 |
  | project `burn_for_project` | 420 |
  | profile `create_profile`, `update_profile` | 420 |
  | blog `create_blog`, `update_blog`, `burn_for_blog` | 1 |
  | forum `create_post`, `burn_for_post` | 1 |

- **Thresholds can step up.** The reward, and with it every app minimum, normally falls as supply grows, but a mint or a burn (by anyone, even a tiny one) can move supply across a round number and change the minimum by one step; just below 90M, 900M, 9B, ... the step is 2x. Fetch the current supply and compute the minimum right before signing (or add a margin), and on `BurnAmountTooSmall` re-fetch and retry.
- **CPI Guard**: memo-burn always burns through a CPI to Token-2022, so a token account with CPI Guard enabled cannot burn through memo-burn, whether called directly or through an app (Token-2022 error 43, `CpiGuardBurnBlocked`). Ask the user to disable CPI Guard in a separate top-level transaction first. Token accounts owned by a multisig cannot burn through memo-burn either.
- **Self-delegation**: if an owner has approved itself as delegate, its burns are limited to the delegated amount and larger burns fail with Token-2022 error 1 (`InsufficientFunds`) despite a sufficient balance. Check `delegate == owner` and ask the user to revoke.

### Indexing and statistics

- **The burn amount is bound to each `process_burn` instruction, not to the transaction.** One memo can back several burns of the same amount (top-level or via CPI, even by different signers) and the transaction's one mint. Never count burns or mints by counting memos or reading memo text; use the per-instruction logs (`Burned N units from ...`, `Minted N units to ...`), the Token-2022 inner instructions, or balance and stats deltas, and attribute app burns by the calling program and app account changes.
- **`GlobalBurnStats` is a lower bound of supply reduction**: it counts only burns through memo-burn since it was initialized. Holders can burn directly through Token-2022 without a memo, which reduces supply without updating any stats. Use `mint.supply` for circulating supply.

## Prerequisites

- Rust and Cargo
- Solana CLI (tested with 2.2.20) — includes `solana-test-validator` for local testing
- Anchor CLI 0.32.1
- GnuPG (deployment only: keypairs are stored GPG-encrypted)

Install the Solana toolchain:

```bash
curl --proto '=https' --tlsv1.2 -sSfL https://raw.githubusercontent.com/solana-developers/solana-install/main/install.sh | bash
```

## Setup

The workspace contains 7 programs under `programs/` (memo-mint, memo-burn, memo-chat, memo-profile, memo-project, memo-blog, memo-forum) and one client crate under `clients/`. Program IDs, token mints, RPC and wallet for both networks are configured in `Anchor.toml`, which the clients read as the single source of truth.

Build:

```bash
anchor build                                      # all programs, testnet IDs (default)
anchor build -p memo-mint                         # one program
anchor build -p memo-mint -- --features mainnet   # mainnet IDs and authorities
```

Each program selects its IDs with the `mainnet` cargo feature; without it the testnet IDs are used.

## Testing

```bash
./scripts/run-unit-tests.sh                 # unit tests of all programs (or: ./scripts/run-unit-tests.sh memo-burn)
./scripts/run-regression-tests.sh           # on-chain regression suite for memo-mint / memo-burn (local validator)
./scripts/run-smoke-tests.sh --local        # smoke tests of all 7 programs against a local validator
./scripts/run-smoke-tests.sh                # smoke tests against the cluster in Anchor.toml (spends test MEMO)
```

The local tests need no keys and send nothing to a public cluster. `scripts/local-validator.sh` starts `solana-test-validator` with the programs from `target/deploy/` (testnet IDs) and state copied from X1 testnet, and lets you set the MEMO supply and the test wallet's balance:

```bash
./scripts/local-validator.sh start --supply 10000000.000001 --balance 1000000
MEMO_RPC_URL=http://127.0.0.1:8899 MEMO_WALLET=target/local-validator/wallet.json cargo run --bin <client>
./scripts/local-validator.sh stop
```

Run the unit and regression tests before deploying any change to memo-mint or memo-burn.

## Deployment Steps

The programs are already live on X1 testnet and mainnet, so a release is an **upgrade** of the existing programs by program ID. Do not use `anchor deploy` directly; use the scripts, which build, check the keys and authorities, upgrade in a safe order, initialize state accounts and sync the IDL. See [PROD-DEPLOYMENT.md](PROD-DEPLOYMENT.md) for the full guide, including first-time deployment and token creation.

1. Unlock the keys of the target network (in your own terminal; asks for that network's passphrase):

   ```bash
   ./scripts/keys.sh unlock testnet     # or: mainnet
   ```

2. Deploy:

   ```bash
   ./scripts/deploy-testnet.sh                    # testnet, testnet IDs (runs unattended)
   ./scripts/deploy-prod-staging-testnet.sh       # testnet, mainnet build — rehearsal before mainnet
   ./scripts/deploy-mainnet.sh                    # mainnet (asks for confirmation)
   ```

   With no arguments all 7 programs are upgraded in order: `memo_mint` → `memo_burn` → `memo_chat` → `memo_profile` → `memo_project` → `memo_blog` → `memo_forum`. You can pass program names to upgrade a subset; they are still deployed in that order. **Never upgrade only the apps:** they depend on the behavior of the current memo-mint / memo-burn (for example fractional burn amounts), so upgrade memo-burn and memo-mint first or in the same run. The script creates the `mint_state` and `global_burn_stats` accounts after upgrading memo-mint / memo-burn if they do not exist yet.

3. Verify (read-only, simulations only):

   ```bash
   cargo run --bin verify-deployment -- <WALLET_WITH_MEMO>
   ./scripts/run-smoke-tests.sh                   # testnet / prod staging only; on mainnet do one real mint and one small burn
   ```

4. Lock the keys:

   ```bash
   ./scripts/keys.sh lock
   ```

Keypairs are stored GPG-encrypted in `~/.config/solana/memo-token/<testnet|mainnet>/{authority,programs}/`. `scripts/setup-keypairs.sh <testnet|mainnet>` imports or generates missing keypairs; program keypairs are only needed for a program's first deployment.

## Client Tools

The `clients` crate contains test and admin tools for every program (`clients/<program>/src/`). They use the network, program IDs and wallet from `Anchor.toml`; set `MEMO_RPC_URL` and `MEMO_WALLET` to point them at another RPC or wallet (for example the local validator). Run them from the repository root. Each tool prints its usage when run without arguments.

Before the first burn, a wallet needs its burn statistics account (once per wallet):

```bash
cargo run --bin init-user-global-burn-stats
cargo run --bin check-memo-burn-user-global-burn-stats
```

1. Mint (`test-memo-mint <scenario>`):

```bash
cargo run --bin test-memo-mint valid-memo            # memo of at least 69 bytes, should succeed
cargo run --bin test-memo-mint memo-69               # exactly 69 bytes, should succeed
cargo run --bin test-memo-mint memo-750              # 750 bytes, should succeed
cargo run --bin test-memo-mint custom-length 420     # custom memo length
cargo run --bin test-memo-mint no-memo               # should fail
cargo run --bin test-memo-mint short-memo            # < 69 bytes, should fail
cargo run --bin test-memo-mint long-memo             # exceeds the 1232-byte transaction size, should fail
```

2. Burn (`test-memo-burn <amount> <scenario>`; amount in tokens, up to 6 decimals, minimum 0.000001):

```bash
cargo run --bin test-memo-burn 1 valid-memo           # should succeed
cargo run --bin test-memo-burn 0.000001 valid-memo    # minimum burn (1 unit), should succeed
cargo run --bin test-memo-burn 2 custom-length 666    # custom memo length
cargo run --bin test-memo-burn 1 no-memo              # should fail
cargo run --bin test-memo-burn 1 short-memo           # should fail
cargo run --bin test-memo-burn 1 long-memo            # exceeds the transaction size, should fail
```

3. Chat groups. Creating a group burns at least 42,069 × the current mint reward (42,069 MEMO while the reward is 1 MEMO); the predefined test cases fetch the current minimum automatically.

```bash
cargo run --bin test-memo-chat-create-group -- valid-basic
cargo run --bin test-memo-chat-create-group -- custom 42069 "solXEN" "solXEN chat group" "avatar.png" "solXEN,X1,Solana" 60
cargo run --bin test-memo-chat-create-group -- long-name           # should fail
cargo run --bin test-memo-chat-create-group -- invalid-category    # should fail
```

Send a message to group `<group_id>` (mints the current reward to the sender):

```bash
cargo run --bin test-memo-chat-send-memo -- simple-text 0
cargo run --bin test-memo-chat-send-memo -- unicode-message 0
cargo run --bin test-memo-chat-send-memo -- custom 0 "Hello, world."
cargo run --bin test-memo-chat-send-memo -- too-long-message 0     # should fail
cargo run --bin test-memo-chat-send-memo                           # show all test cases
```

Burn for a group (at least 1 × the current mint reward):

```bash
cargo run --bin test-memo-chat-burn-for-group -- valid-basic 0
cargo run --bin test-memo-chat-burn-for-group -- valid-empty-message 0
cargo run --bin test-memo-chat-burn-for-group -- valid-long-message 0
cargo run --bin test-memo-chat-burn-for-group -- custom 0 5000 "Supporting group development"
cargo run --bin test-memo-chat-burn-for-group -- too-long-message 0     # should fail
```

4. Other apps: `test-memo-profile-*`, `test-memo-project-*`, `test-memo-blog-*` and `test-memo-forum-*` (see `clients/Cargo.toml` for the full list), and the `smoke-test-*` tools, which run a complete scenario per program.

## Security

The MEMO mint authority is a PDA of memo-mint (seed `mint_authority`), and the token has no freeze authority, which means:
- Only memo-mint can mint tokens, following the supply curve and at most once per transaction
- No private key exists for the mint authority
- The PDA address is deterministic, derived from the memo-mint program ID

The programs are currently upgradeable by the deploy admin key. The upgrade authority of memo-mint and memo-burn will be revoked (making them immutable) once they have run stably for a while. See [MEMO-CORE-AUDIT-2026-10-16.md](MEMO-CORE-AUDIT-2026-10-16.md) for the latest security audit of the core programs.

## Development Notes

- All keypairs are stored GPG-encrypted, one tree per network: `~/.config/solana/memo-token/<testnet|mainnet>/{authority,programs}/` (outside `target/`, which `anchor clean` deletes). `scripts/keys.sh unlock <network>` decrypts them into a RAM disk for a work session and `scripts/keys.sh lock` removes them; `scripts/setup-keypairs.sh` imports or generates missing keypairs. Program keypairs are only needed for a program's first deploy; the deploy scripts upgrade deployed programs by program ID with the upgrade authority.
- Mint authority is a PDA derived from program ID
- Each user needs to create their token account before minting