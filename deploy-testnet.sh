#!/bin/bash

# make sure keypair file exists
if [ ! -f "target/deploy/memo_burn-keypair.json" ]; then
    echo "Error: memo_burn-keypair.json not found!"
    echo "Please generate or restore the keypair file first."
    exit 1
fi

# verify program id matches
EXPECTED_ID="FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP"
ACTUAL_ID=$(solana-keygen pubkey target/deploy/memo_burn-keypair.json)

if [ "$EXPECTED_ID" != "$ACTUAL_ID" ]; then
    echo "Error: Program ID mismatch!"
    echo "Expected: $EXPECTED_ID"
    echo "Actual: $ACTUAL_ID"
    exit 1
fi

echo "✅ Program ID verified: $ACTUAL_ID"

# build and deploy
anchor build
anchor deploy --program-name memo_burn