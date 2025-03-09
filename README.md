# Memo Token

A Solana program for creating and minting tokens using a Program Derived Address (PDA) as the mint authority.

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

## Deployment Steps

1. Deploy the program:

```bash
anchor deploy
```

2. Create the token (one-time operation by deployer):

```bash
cargo run --bin create_token
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

1. Create token account (first-time users):

```bash
cargo run --bin init
```

2. Mint tokens:

```bash
cargo run --bin mint
```

## Architecture

- `lib.rs`: Main program logic with PDA-based mint authority
- `create-token.rs`: Token creation utility
- `init.rs`: Token account initialization
- `mint.rs`: Token minting client
```
cargo run --bin mint 400000 "$(printf 'a%.0s' {1..700})"
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