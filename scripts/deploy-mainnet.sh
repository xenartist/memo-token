#!/bin/bash
# Deploy to mainnet
# Usage: ./scripts/deploy-mainnet.sh [program1] [program2] ...
# Example: ./scripts/deploy-mainnet.sh memo_burn
# Example: ./scripts/deploy-mainnet.sh memo-burn memo-mint

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/deploy-common.sh"

ENV="mainnet"
CLUSTER="https://rpc.mainnet.x1.xyz"
WALLET="${ANCHOR_WALLET:-~/.config/solana/mainnet-admin.json}"
FEATURE_FLAG="mainnet"  # Use mainnet feature

# Additional security check
echo ""
print_warning "MAINNET DEPLOYMENT SECURITY CHECK"
echo "  ✓ In secure isolated environment?"
echo "  ✓ Mainnet keypairs backed up?"
echo "  ✓ Tested on testnet first?"
echo ""
read -p "All checks passed? (yes/no): " security
if [ "$security" != "yes" ]; then
    print_error "Deployment cancelled."
    exit 0
fi

# Pass all arguments as programs to deploy
deploy_to_env "${ENV}" "${CLUSTER}" "${WALLET}" "${FEATURE_FLAG}" "$@"