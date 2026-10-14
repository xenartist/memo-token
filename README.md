# Memo Token

X1

## Overview

This project consists of:
- A Solana program (smart contract) for token minting and burning
- Client utilities for token creation and management
- PDA-based mint authority for secure token minting

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

**Emission curve**: the reward per mint depends on the circulating supply and steps down at every round number.

| Supply reaches | Reward per mint |
|---|---|
| below 20M | 1 MEMO |
| 20M, 30M, ..., 90M | 0.9, 0.8, ..., 0.2 |
| 100M | 0.1 |
| 200M, ..., 900M | 0.09, ..., 0.02 |
| 1B | 0.01 |
| each further 10x | 10x less, down to 1 lamport at the cap |

Circulating supply is capped at 10 trillion MEMO. Burns lower the circulating supply, so minting never stops permanently. See `MEMO-MINT-AUDIT.md` for details.

## Burn Statistics

memo-burn keeps per-user stats (`UserGlobalBurnStats`, seeds `[b"user_global_burn_stats", user]`) and one global account (`GlobalBurnStats`, seeds `[b"global_burn_stats"]`) with the total burned (u128, in units of 10^-6 MEMO), the number of burn instructions, and the last burn time. Both counters stop at their maximum instead of overflowing. Every `process_burn`, direct or through an app, must pass the global account as its last account.

The global account is created once by the admin with `cargo run --bin admin-init-global-burn-stats` (the deploy scripts do this right after deploying memo-burn). Burns fail until it exists, so memo-burn must be deployed and initialized before the apps.

## Prerequisites

- Rust and Cargo
- Solana CLI tools
- Anchor framework

## Setup

1. Install all dependencies:

```bash
curl --proto '=https' --tlsv1.2 -sSfL https://raw.githubusercontent.com/solana-developers/solana-install/main/install.sh | bash
```

2. Build the program:

```bash
anchor build
```
```bash
anchor build --program-name memo-mint
```
```bash
anchor build --program-name memo-burn
```

## Deployment Steps

1. Deploy the program:

```bash
anchor deploy
```

```bash
anchor deploy --program-name memo-mint
```

```bash
anchor deploy --program-name memo-burn
```

2. Create the token (one-time operation by deployer):

```bash
chmod +x clients/src/admin-create-memo-token.sh
clients/src/admin-create-memo-token.sh
```

```bash
cargo run --bin admin-transfer-memo-token-mint-authority <token_mint_address> <program_id>
```

Save the output addresses:
- Program ID
- Mint address
- Mint authority (PDA)

3. Update the following files with the new addresses:
- `programs/memo-token/src/lib.rs`: Program ID
- `clients/src/init.rs`: Program ID and Mint address
- `clients/src/mint.rs`: Program ID and Mint address

## User Operations

1. Mint token:

```bash
cargo run --bin test-memo-mint valid-memo
cargo run --bin test-memo-mint memo-69
cargo run --bin test-memo-mint memo-750
cargo run --bin test-memo-mint no-memo
cargo run --bin test-memo-mint short-memo
cargo run --bin test-memo-mint long-memo
cargo run --bin test-memo-mint custom-length 420
```

2. Burn token:

```bash
cargo run --bin test-memo-burn 1 valid-memo
cargo run --bin test-memo-burn 1 memo-69
cargo run --bin test-memo-burn 1 memo-800
cargo run --bin test-memo-burn 1 no-memo
cargo run --bin test-memo-burn 1 short-memo
cargo run --bin test-memo-burn 1 long-memo
cargo run --bin test-memo-burn 1 custom-length 420
```

3. Chat Group

```bash
cargo run --bin test-memo-chat-create-group -- custom 1 "solXEN" "solXEN chat group" "avatar.png" "solXEN,X1,Solana" 60

cargo run --bin test-memo-chat-create-group -- valid-basic

cargo run --bin test-memo-chat-create-group -- invalid-category

cargo run --bin test-memo-chat-create-group -- long-name
```

```bash
cargo run --bin test-memo-chat-send-memo -- simple-text 0

cargo run --bin test-memo-chat-send-memo -- json-memo 1

cargo run --bin test-memo-chat-send-memo -- too-long 0

cargo run --bin test-memo-chat-send-memo -- custom 0 "Hello, world."

# show help
cargo run --bin test-memo-chat-send-memo --
```

```bash
# basic burn test (default message)
cargo run --bin test-memo-chat-burn-for-group -- valid-basic 0

# empty message
cargo run --bin test-memo-chat-burn-for-group -- valid-empty-message 0

# long message
cargo run --bin test-memo-chat-burn-for-group -- valid-long-message 0

# custom token burns and custom message
cargo run --bin test-memo-chat-burn-for-group -- custom 0 5000 "Supporting group development"

# too long message (should be failed)
cargo run --bin test-memo-chat-burn-for-group -- too-long-message 0
```

## Security

The program uses a PDA as mint authority, which means:
- Only the program can mint tokens
- No private keys needed for mint authority
- Deterministic PDA address based on program ID

## Development Notes

- Program keypairs are stored in `~/.config/solana/memo-token/programs/<testnet|mainnet>/` (outside `target/`, which `anchor clean` deletes) and symlinked into `target/deploy/` by `scripts/setup-keypairs.sh`. They are only needed for a program's first deploy; the deploy scripts upgrade deployed programs by program ID with the upgrade authority.
- Mint authority is a PDA derived from program ID
- Each user needs to create their token account before minting