# Memo Token

X1

## Overview

This project consists of:
- A Solana program (smart contract) for token minting
- Client utilities for token creation and management
- PDA-based mint authority for secure token minting

## Prerequisites

- Rust and Cargo
- Solana CLI tools
- Anchor framework
- Node.js and npm

## Setup

1. Install dependencies:

```bash
cargo install anchor-cli
```

2. Build the program:

```bash
anchor build
```
```bash
anchor build --program-name memo-token
```
```bash
anchor build --program-name memo-social
```

## Deployment Steps

1. Deploy the program:

```bash
anchor deploy
```

```bash
anchor deploy --program-name memo-token
```

```bash
anchor deploy --program-name memo-social
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
cargo run --bin test-memo-mint memo-769
cargo run --bin test-memo-mint no-memo
cargo run --bin test-memo-mint short-memo
cargo run --bin test-memo-mint long-memo
```


## Architecture

- `lib.rs`: Main program logic with PDA-based mint authority
- `create-token.rs`: Token creation utility
- `init.rs`: Token account initialization
- `mint.rs`: Token minting client
```
cargo run --bin test-single-mint "$(printf 'a%.0s' {1..500})"
``` 
- `burn.rs`: Token burning client

##### default：add burn_history in memo and set it to "Y"
```
cargo run --bin test-single-burn
```
##### burn 420 tokens
```
cargo run --bin test-single-burn - 420
```

##### add burn_history in memo and set it to "N"
```
cargo run --bin test-single-burn 440000 1 "My message" N
```

##### not add burn_history in memo
```
cargo run --bin test-single-burn 440000 1 "My message" NONE
```

## Security

The program uses a PDA as mint authority, which means:
- Only the program can mint tokens
- No private keys needed for mint authority
- Deterministic PDA address based on program ID

## Development Notes

- Program ID is stored in `target/deploy/memo_token-keypair.json`
- Mint authority is a PDA derived from program ID
- Each user needs to create their token account before minting