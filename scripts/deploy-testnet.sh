#!/bin/bash
# Deploy to testnet
# Usage: ./scripts/deploy-testnet.sh [program1] [program2] ...
# Example: ./scripts/deploy-testnet.sh memo_burn
# Example: ./scripts/deploy-testnet.sh memo-burn memo-mint

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/deploy-common.sh"

ENV="testnet"
CLUSTER="https://rpc.testnet.x1.xyz"
WALLET="${ANCHOR_WALLET:-~/.config/solana/id.json}"
FEATURE_FLAG=""  # No feature flag for testnet (default)

# Pass all arguments as programs to deploy
deploy_to_env "${ENV}" "${CLUSTER}" "${WALLET}" "${FEATURE_FLAG}" "$@"