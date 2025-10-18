#!/bin/bash
# Deploy to mainnet
# Usage: ./scripts/deploy-mainnet.sh [program1] [program2] ...
# Example: ./scripts/deploy-mainnet.sh memo_burn
# Example: ./scripts/deploy-mainnet.sh memo-burn memo-mint

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/deploy-common.sh"

ENV="mainnet"
CLUSTER="https://rpc.mainnet.x1.xyz"
WALLET="${ANCHOR_WALLET:-${HOME}/.config/solana/memo-token/authority/deploy_admin-keypair.json}"
FEATURE_FLAG="mainnet"  # Use mainnet feature

# Additional security check
echo ""
print_warning "MAINNET DEPLOYMENT SECURITY CHECK"
echo "  ✓ Running on secure, isolated PRODUCTION server?"
echo "  ✓ This is NOT the same machine as testnet?"
echo "  ✓ Mainnet keypairs backed up to cold storage?"
echo "  ✓ Tested thoroughly on testnet first?"
echo "  ✓ Team notified about deployment?"
echo ""
read -p "All checks passed? (yes/no): " security
if [ "$security" != "yes" ]; then
    print_error "Deployment cancelled."
    exit 0
fi

# Pass all arguments as programs to deploy
deploy_to_env "${ENV}" "${CLUSTER}" "${WALLET}" "${FEATURE_FLAG}" "$@"