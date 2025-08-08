# Memo Token

X1

## Overview

This project consists of:
- A Solana program (smart contract) for token minting and burning
- Client utilities for token creation and management
- PDA-based mint authority for secure token minting

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
cargo run --bin test-memo-mint memo-800
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

## Security

The program uses a PDA as mint authority, which means:
- Only the program can mint tokens
- No private keys needed for mint authority
- Deterministic PDA address based on program ID

## Development Notes

- Program ID is stored in 
    `target/deploy/memo_mint-keypair.json`
    `target/deploy/memo_burn-keypair.json`
- Mint authority is a PDA derived from program ID
- Each user needs to create their token account before minting