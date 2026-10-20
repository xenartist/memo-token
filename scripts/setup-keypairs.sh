#!/bin/bash
# Set up the keypairs of one network, stored encrypted by scripts/keys.sh:
#   ~/.config/solana/memo-token/<testnet|mainnet>/authority/  deploy_admin, memo_token_mint
#   ~/.config/solana/memo-token/<testnet|mainnet>/programs/   one keypair per program
# For each missing keypair: import it from a backup file, generate a new one, or skip.
# Compatible with bash 3.x (macOS default)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KEYS="${SCRIPT_DIR}/keys.sh"
KEYS_BASE="${HOME}/.config/solana/memo-token"

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

print_info() { echo -e "${BLUE}ℹ️  $1${NC}"; }
print_success() { echo -e "${GREEN}✅ $1${NC}"; }
print_warning() { echo -e "${YELLOW}⚠️  $1${NC}"; }
print_error() { echo -e "${RED}❌ $1${NC}"; }

ENV=$1
if [ "${ENV}" != "testnet" ] && [ "${ENV}" != "mainnet" ]; then
    echo "Usage: $0 <testnet|mainnet>"
    exit 1
fi

PROGRAMS="memo_mint memo_burn memo_home memo_chat memo_profile memo_project memo_blog memo_forum"

# Plaintext keypairs from the old layout should be migrated, not re-imported one by one
if ls "${KEYS_BASE}/authority"/*.json "${KEYS_BASE}/programs"/*/*.json >/dev/null 2>&1; then
    print_warning "Plaintext keypairs from the old layout found. Run '${KEYS} migrate' first."
    exit 1
fi

# $1 = category, $2 = name, $3 = what it is for
setup_key() {
    local category=$1 name=$2 purpose=$3
    local pub="${KEYS_BASE}/${ENV}/${category}/${name}-keypair.pub"
    if [ -f "${pub%.pub}.json.gpg" ]; then
        print_success "${name}: $(cat "${pub}")"
        return
    fi
    echo ""
    print_warning "${name} not found (${purpose})"
    echo "  1) Import from a backup file"
    echo "  2) Generate a NEW keypair"
    echo "  3) Skip"
    local choice path
    read -r -p "Choose option [1/2/3]: " choice
    case "${choice}" in
        1)
            read -r -p "Path to the backup keypair file: " path
            path="${path/#\~/${HOME}}"
            "${KEYS}" import "${ENV}" "${category}" "${path}" "${name}"
            ;;
        2)
            "${KEYS}" new "${ENV}" "${category}" "${name}"
            if [ "${category}" = "programs" ]; then
                print_warning "New program ID: update declare_id! (${ENV}) and Anchor.toml [programs.${ENV}]"
            else
                print_warning "New ${name}: update AUTHORIZED_*_PUBKEY (${ENV}) in the programs"
            fi
            ;;
        *)
            print_info "${name}: skipped"
            ;;
    esac
}

echo ""
echo "=========================================="
echo "Keypairs for $(echo "${ENV}" | tr '[:lower:]' '[:upper:]'): ${KEYS_BASE}/${ENV}"
echo "=========================================="
echo ""
print_info "Authority keypairs"
setup_key authority deploy_admin "upgrade authority of all programs + admin of their global state"
setup_key authority memo_token_mint "MEMO token mint address keypair (AUTHORIZED_MINT_PUBKEY)"

echo ""
print_info "Program keypairs (needed only for a program's first deploy)"
for program in ${PROGRAMS}; do
    setup_key programs "${program}" "program ID"
done

echo ""
print_success "Setup complete for ${ENV}"
print_info "Keys are stored encrypted. Before deploying: ${KEYS} unlock ${ENV}"
print_info "Back up ${KEYS_BASE}/${ENV} to offline storage."
