#!/bin/bash
set -e  # Exit on error

# Default values
TOKEN_NAME="MEMO Engraves Memories Onchain"
TOKEN_SYMBOL="MEMO"
TOKEN_URI="https://raw.githubusercontent.com/xenartist/memo-token/refs/heads/dev/metadata/memo_token-metadata.json"
DECIMALS=6
MINT_KEYPAIR_PATH="$HOME/.config/solana/memo-token/authority/memo_token_mint-keypair.json"

# Parse command line arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --token-name)
      TOKEN_NAME="$2"
      shift 2
      ;;
    --token-symbol)
      TOKEN_SYMBOL="$2"
      shift 2
      ;;
    --token-uri)
      TOKEN_URI="$2"
      shift 2
      ;;
    --decimals)
      DECIMALS="$2"
      shift 2
      ;;
    --mint-keypair)
      MINT_KEYPAIR_PATH="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1"
      exit 1
      ;;
  esac
done

echo "Creating Token-2022 token with the following details:"
echo "Name: $TOKEN_NAME"
echo "Symbol: $TOKEN_SYMBOL"
echo "URI: $TOKEN_URI"
echo "Decimals: $DECIMALS"
echo "Mint keypair path: $MINT_KEYPAIR_PATH"

# Expand the mint keypair path
MINT_KEYPAIR_PATH="${MINT_KEYPAIR_PATH/#\~/$HOME}"

# Check if the mint keypair file exists
if [ -f "$MINT_KEYPAIR_PATH" ]; then
  echo "Using existing mint keypair file at $MINT_KEYPAIR_PATH"
else
  echo "Mint keypair file not found. Creating a new keypair with a vanity address..."
  
  # Check if user wants to create a vanity address
  read -p "Do you want to create a vanity address that starts with specific characters? (y/n): " CREATE_VANITY
  
  if [[ "$CREATE_VANITY" =~ ^[Yy]$ ]]; then
    read -p "Enter prefix (1-5 characters recommended): " PREFIX
    read -p "Enter number of matches to find: " COUNT
    
    echo "Finding a keypair that starts with '$PREFIX'... (this may take a while)"
    solana-keygen grind --starts-with "$PREFIX:$COUNT" --outfile "$MINT_KEYPAIR_PATH"
  else
    echo "Generating a random keypair..."
    solana-keygen new --no-passphrase --outfile "$MINT_KEYPAIR_PATH"
  fi
fi

# Read mint public key from the keypair file
MINT_PUBKEY=$(solana-keygen pubkey "$MINT_KEYPAIR_PATH")
echo "Mint public key: $MINT_PUBKEY"

# Step 1: Create the token mint with enabled metadata
echo -e "\nStep 1: Creating Token-2022 token..."
spl-token create-token \
  --program-id TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb \
  --enable-metadata \
  --decimals "$DECIMALS" \
  "$MINT_KEYPAIR_PATH"

# Step 2: Initialize metadata
echo -e "\nStep 2: Initializing metadata..."
spl-token initialize-metadata \
  "$MINT_PUBKEY" \
  "$TOKEN_NAME" \
  "$TOKEN_SYMBOL" \
  "$TOKEN_URI"

echo -e "\nToken created and metadata initialized successfully!"
echo "Token Info:"
echo "Token-2022 ID: TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
echo "Mint address: $MINT_PUBKEY"
echo "Token Name: $TOKEN_NAME"
echo "Token Symbol: $TOKEN_SYMBOL"
echo "Token Metadata URI: $TOKEN_URI"
echo "Decimals: $DECIMALS"
echo -e "\nNow run the transfer-mint-authority Rust program to transfer mint authority to your contract."