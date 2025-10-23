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
FEATURE_FLAG="mainnet"  # Use mainnet feature flag

# Environment validation checks
echo ""
print_warning "PRODUCTION STAGING ENVIRONMENT VALIDATION"
echo ""

# Read cluster from Anchor.toml (single source of truth)
print_info "Reading configuration from Anchor.toml..."
CLUSTER=$(grep "^cluster" Anchor.toml | awk -F'"' '{print $2}')

if [ -z "${CLUSTER}" ]; then
    print_error "Could not read cluster from Anchor.toml"
    echo "Please ensure Anchor.toml has [provider] section with cluster defined"
    exit 1
fi

echo "  Cluster: ${CLUSTER}"

# Verify it's a testnet cluster (for prod-staging)
if [[ ! "${CLUSTER}" =~ "testnet" ]]; then
    print_error "Anchor.toml cluster is NOT testnet!"
    echo "  Current value: ${CLUSTER}"
    echo "  Expected: should contain 'testnet'"
    echo ""
    echo "Note: For prod-staging, Anchor.toml should use testnet cluster"
    echo "Please update Anchor.toml [provider] section:"
    echo "  cluster = \"https://rpc.testnet.x1.xyz\""
    echo ""
    exit 1
fi
print_success "Anchor.toml configuration correct (testnet for prod-staging)"

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