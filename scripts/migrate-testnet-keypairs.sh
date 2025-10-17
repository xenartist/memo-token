#!/bin/bash
# Migrate existing keypairs from target/deploy/ to target/deploy/testnet/

set -e

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_info() { echo -e "${BLUE}ℹ️  $1${NC}"; }
print_success() { echo -e "${GREEN}✅ $1${NC}"; }
print_warning() { echo -e "${YELLOW}⚠️  $1${NC}"; }

# Get the project root directory (where the script's parent directory is)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

print_info "Migrating testnet keypairs..."
print_info "Project root: ${PROJECT_ROOT}"

# Change to project root
cd "${PROJECT_ROOT}"

# Create testnet directory
mkdir -p target/deploy/testnet

PROGRAMS=("memo_mint" "memo_burn" "memo_chat" "memo_profile" "memo_project")

for program in "${PROGRAMS[@]}"; do
    SOURCE="target/deploy/${program}-keypair.json"
    DEST="target/deploy/testnet/${program}-keypair.json"
    
    if [ -f "${SOURCE}" ]; then
        if [ -f "${DEST}" ]; then
            print_warning "${program}: testnet keypair already exists"
        else
            cp "${SOURCE}" "${DEST}"
            PUBKEY=$(solana-keygen pubkey "${DEST}")
            print_success "Migrated ${program}: ${PUBKEY}"
        fi
    else
        print_warning "${program}: source keypair not found at ${SOURCE}"
    fi
done

echo ""
print_info "Migration summary - Testnet Program IDs:"
for program in "${PROGRAMS[@]}"; do
    if [ -f "target/deploy/testnet/${program}-keypair.json" ]; then
        PUBKEY=$(solana-keygen pubkey "target/deploy/testnet/${program}-keypair.json")
        echo "  ${program} = \"${PUBKEY}\""
    fi
done

echo ""
print_warning "Next steps:"
echo "  1. Verify the keypairs are correct"
echo "  2. Backup target/deploy/testnet/ directory"
echo "  3. Update .gitignore if needed"
echo "  4. Test deployment: ./scripts/deploy-testnet.sh"