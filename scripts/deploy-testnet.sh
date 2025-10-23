#!/bin/bash
# Deploy to testnet
# Usage: ./scripts/deploy-testnet.sh [program1] [program2] ...
# Example: ./scripts/deploy-testnet.sh memo_burn
# Example: ./scripts/deploy-testnet.sh memo-burn memo-mint

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/deploy-common.sh"

ENV="testnet"
FEATURE_FLAG=""  # No feature flag for testnet (default)

# Environment validation checks
echo ""
print_warning "TESTNET ENVIRONMENT VALIDATION"
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

# Verify it's a testnet cluster
if [[ ! "${CLUSTER}" =~ "testnet" ]]; then
    print_error "Anchor.toml cluster is NOT testnet!"
    echo "  Current value: ${CLUSTER}"
    echo "  Expected: should contain 'testnet'"
    echo ""
    echo "Please update Anchor.toml [provider] section:"
    echo "  cluster = \"https://rpc.testnet.x1.xyz\""
    echo ""
    exit 1
fi
print_success "Anchor.toml configuration correct (testnet)"

echo ""
print_success "Environment validation passed"
echo ""

# Pass all arguments as programs to deploy
deploy_to_env "${ENV}" "${CLUSTER}" "${FEATURE_FLAG}" "$@"