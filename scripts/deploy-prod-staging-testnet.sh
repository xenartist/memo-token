#!/bin/bash
# Deploy to testnet with MAINNET configuration for production staging validation
# Usage: ./scripts/deploy-prod-staging-testnet.sh [program1] [program2] ...
# Example: ./scripts/deploy-prod-staging-testnet.sh memo_burn
# Example: ./scripts/deploy-prod-staging-testnet.sh memo-burn memo-mint
#
# This script allows testing mainnet production configurations on testnet before
# deploying to actual mainnet. It uses mainnet feature flags but deploys to testnet RPC.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/deploy-common.sh"

ENV="mainnet"  # Use mainnet configuration for validation
CLUSTER="https://rpc.testnet.x1.xyz"
FEATURE_FLAG="mainnet"  # Use mainnet feature flag

# Environment validation checks
echo ""
print_warning "PRODUCTION STAGING ENVIRONMENT VALIDATION"
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

# Additional information check
echo "=========================================="
print_warning "PRODUCTION STAGING EXPLANATION"
echo "=========================================="
echo ""
echo "  🎯 Purpose: Validate mainnet production configuration on testnet"
echo ""
echo "  📋 Configuration Details:"
echo "     - Deployment Environment: Testnet"
echo "     - Configuration Environment: Mainnet"
echo "     - Feature Flag: mainnet"
echo "     - RPC Endpoint: ${CLUSTER}"
echo ""
echo "  ⚠️  Important Notes:"
echo "     ✓ Uses mainnet Program IDs and Authority Keys"
echo "     ✓ Deploys to testnet for validation"
echo "     ✓ Must pass validation before deploying to mainnet"
echo "     ✓ Ensure mainnet addresses are correctly configured in programs/*/src/lib.rs"
echo ""

read -p "Understand the above and continue? (yes/no): " understand
if [ "$understand" != "yes" ]; then
    print_info "Deployment cancelled."
    exit 0
fi

echo ""
print_warning "FINAL CONFIRMATION CHECKLIST"
echo "  ✓ Correctly configured mainnet Program IDs in programs/*/src/lib.rs?"
echo "  ✓ Correctly configured mainnet Authority Keys in programs/*/src/lib.rs?"
echo "  ✓ Ready to test all critical functions on testnet?"
echo "  ✓ Will deploy to actual mainnet after testing passes?"
echo ""
read -p "All checks confirmed? (yes/no): " final_check
if [ "$final_check" != "yes" ]; then
    print_error "Deployment cancelled."
    exit 0
fi

# Pass all arguments as programs to deploy
deploy_to_env "${ENV}" "${CLUSTER}" "${FEATURE_FLAG}" "$@"

# Post-deployment reminder
echo ""
print_warning "TESTNET DEPLOYMENT COMPLETE - NEXT STEPS"
echo "  ☐ Test all critical functions on testnet"
echo "  ☐ Verify Program IDs and Authority Keys are correct"
echo "  ☐ Test all permission controls and security mechanisms"
echo "  ☐ Confirm all functionality meets mainnet requirements"
echo "  ☐ After testing passes, deploy to mainnet using deploy-mainnet.sh"
echo ""