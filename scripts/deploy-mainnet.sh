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

# Environment validation checks
echo ""
print_warning "MAINNET ENVIRONMENT VALIDATION"
echo ""

# Check 1: Verify solana config is pointing to mainnet
print_info "Check 1: Verifying Solana CLI configuration..."
SOLANA_CONFIG_URL=$(solana config get | grep "RPC URL" | awk '{print $3}')
echo "  Current Solana RPC URL: ${SOLANA_CONFIG_URL}"

if [[ ! "${SOLANA_CONFIG_URL}" =~ "mainnet" ]]; then
    print_error "Solana CLI configuration is NOT mainnet!"
    echo "  Current config: ${SOLANA_CONFIG_URL}"
    echo "  Expected: should contain 'mainnet'"
    echo ""
    echo "Please run the following command to switch to mainnet:"
    echo "  solana config set --url https://rpc.mainnet.x1.xyz"
    echo ""
    exit 1
fi
print_success "Solana CLI configuration correct (mainnet)"

# Check 2: Verify X1_RPC_URL environment variable if set
print_info "Check 2: Verifying X1_RPC_URL environment variable..."
if [ -n "${X1_RPC_URL}" ]; then
    echo "  X1_RPC_URL: ${X1_RPC_URL}"
    if [[ ! "${X1_RPC_URL}" =~ "mainnet" ]]; then
        print_error "X1_RPC_URL environment variable is NOT mainnet!"
        echo "  Current value: ${X1_RPC_URL}"
        echo "  Expected: should contain 'mainnet'"
        echo ""
        echo "Please run the following command to set correct environment variable:"
        echo "  export X1_RPC_URL=https://rpc.mainnet.x1.xyz"
        echo ""
        exit 1
    fi
    print_success "X1_RPC_URL environment variable correct (mainnet)"
else
    print_warning "X1_RPC_URL environment variable not set (optional)"
fi

echo ""
print_success "Environment validation passed"
echo ""

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