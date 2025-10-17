#!/bin/bash
# Generate keypairs for mainnet
# Testnet keypairs are migrated from existing files

set -e

# Locate project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_info() { echo -e "${BLUE}ℹ️  $1${NC}"; }
print_success() { echo -e "${GREEN}✅ $1${NC}"; }
print_warning() { echo -e "${YELLOW}⚠️  $1${NC}"; }

ENV=$1

if [ -z "${ENV}" ] || [ "${ENV}" != "mainnet" ]; then
    echo "Usage: $0 mainnet"
    echo ""
    echo "Note: Testnet keypairs should be migrated using:"
    echo "      ./scripts/migrate-testnet-keypairs.sh"
    exit 1
fi

# Change to project root
cd "${PROJECT_ROOT}"

print_info "Generating mainnet keypairs..."
print_info "Project root: ${PROJECT_ROOT}"

mkdir -p target/deploy/mainnet

PROGRAMS=("memo_mint" "memo_burn" "memo_chat" "memo_profile" "memo_project")

for program in "${PROGRAMS[@]}"; do
    if [ -f "target/deploy/mainnet/${program}-keypair.json" ]; then
        print_warning "Keypair for ${program} already exists, skipping..."
    else
        solana-keygen new --no-bip39-passphrase -o "target/deploy/mainnet/${program}-keypair.json"
        PUBKEY=$(solana-keygen pubkey "target/deploy/mainnet/${program}-keypair.json")
        print_success "Generated ${program}: ${PUBKEY}"
    fi
done

echo ""
print_info "Mainnet Program IDs:"
for program in "${PROGRAMS[@]}"; do
    PUBKEY=$(solana-keygen pubkey "target/deploy/mainnet/${program}-keypair.json")
    echo "  ${program} = \"${PUBKEY}\""
done

echo ""
print_warning "IMPORTANT:"
echo "  1. Backup these keypairs to secure encrypted storage"
echo "  2. Never commit target/deploy/mainnet/*.json to git"
echo "  3. Restrict access to these files"
echo "  4. Test deployment on testnet first"