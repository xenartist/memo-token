#!/bin/bash
# Deploy to testnet
# Usage: ./scripts/deploy-testnet.sh [program1] [program2] ...
# Example: ./scripts/deploy-testnet.sh memo_burn
# Example: ./scripts/deploy-testnet.sh memo-burn memo-mint

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/deploy-common.sh"

ENV="testnet"
CLUSTER="https://rpc.testnet.x1.xyz"
FEATURE_FLAG=""  # No feature flag for testnet (default)

# Environment validation checks
echo ""
print_warning "TESTNET ENVIRONMENT VALIDATION"
echo ""

# Check 1: Verify solana config is pointing to testnet
print_info "Check 1: Verifying Solana CLI configuration..."
SOLANA_CONFIG_URL=$(solana config get | grep "RPC URL" | awk '{print $3}')
echo "  Current Solana RPC URL: ${SOLANA_CONFIG_URL}"

if [[ ! "${SOLANA_CONFIG_URL}" =~ "testnet" ]]; then
    print_error "Solana CLI configuration is NOT testnet!"
    echo "  Current config: ${SOLANA_CONFIG_URL}"
    echo "  Expected: should contain 'testnet'"
    echo ""
    echo "Please run the following command to switch to testnet:"
    echo "  solana config set --url https://rpc.testnet.x1.xyz"
    echo ""
    exit 1
fi
print_success "Solana CLI configuration correct (testnet)"

# Check 2: Verify X1_RPC_URL environment variable if set
print_info "Check 2: Verifying X1_RPC_URL environment variable..."
if [ -n "${X1_RPC_URL}" ]; then
    echo "  X1_RPC_URL: ${X1_RPC_URL}"
    if [[ ! "${X1_RPC_URL}" =~ "testnet" ]]; then
        print_error "X1_RPC_URL environment variable is NOT testnet!"
        echo "  Current value: ${X1_RPC_URL}"
        echo "  Expected: should contain 'testnet'"
        echo ""
        echo "Please run the following command to set correct environment variable:"
        echo "  export X1_RPC_URL=https://rpc.testnet.x1.xyz"
        echo ""
        exit 1
    fi
    print_success "X1_RPC_URL environment variable correct (testnet)"
else
    print_warning "X1_RPC_URL environment variable not set (optional)"
fi

echo ""
print_success "Environment validation passed"
echo ""

# Pass all arguments as programs to deploy
deploy_to_env "${ENV}" "${CLUSTER}" "${FEATURE_FLAG}" "$@"