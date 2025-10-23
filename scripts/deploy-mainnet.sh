#!/bin/bash
# Deploy to mainnet
# Usage: ./scripts/deploy-mainnet.sh [program1] [program2] ...
# Example: ./scripts/deploy-mainnet.sh memo_burn
# Example: ./scripts/deploy-mainnet.sh memo-burn memo-mint

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/deploy-common.sh"

ENV="mainnet"
FEATURE_FLAG="mainnet"  # Use mainnet feature

# Environment validation checks
echo ""
print_warning "MAINNET ENVIRONMENT VALIDATION"
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

# Verify it's a mainnet cluster
if [[ ! "${CLUSTER}" =~ "mainnet" ]]; then
    print_error "Anchor.toml cluster is NOT mainnet!"
    echo "  Current value: ${CLUSTER}"
    echo "  Expected: should contain 'mainnet'"
    echo ""
    echo "Please update Anchor.toml [provider] section:"
    echo "  cluster = \"https://rpc.mainnet.x1.xyz\""
    echo ""
    exit 1
fi
print_success "Anchor.toml configuration correct (mainnet)"

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
deploy_to_env "${ENV}" "${CLUSTER}" "${FEATURE_FLAG}" "$@"