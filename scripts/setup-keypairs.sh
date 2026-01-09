#!/bin/bash
# Setup keypairs for deployment
# Program keypairs: target/deploy/ (Anchor default)
# Authority keypairs: ~/.config/solana/memo-token/authority/ (unified across environments)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Authority keypair storage location (unified path for all environments)
AUTHORITY_KEYPAIR_DIR="${HOME}/.config/solana/memo-token/authority"

# Program keypair location (project directory, Anchor default)
PROGRAM_KEYPAIR_DIR="${PROJECT_ROOT}/target/deploy"

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

if [ -z "${ENV}" ]; then
    echo "Usage: $0 <testnet|mainnet>"
    echo ""
    echo "This script sets up keypairs:"
    echo "  - Program keypairs: ${PROGRAM_KEYPAIR_DIR}"
    echo "  - Authority keypairs: ${AUTHORITY_KEYPAIR_DIR}"
    echo ""
    echo "Note: Authority keypairs use the same path for all environments."
    echo "      Different environments should use different servers/machines."
    exit 1
fi

if [ "${ENV}" != "testnet" ] && [ "${ENV}" != "mainnet" ]; then
    print_error "Invalid environment. Use 'testnet' or 'mainnet'"
    exit 1
fi

echo ""
echo "=========================================="
echo "Setting up keypairs for $(echo ${ENV} | tr '[:lower:]' '[:upper:]')"
echo "=========================================="
echo ""

# Create directories
mkdir -p "${PROGRAM_KEYPAIR_DIR}"
mkdir -p "${AUTHORITY_KEYPAIR_DIR}"

# Set restrictive permissions for authority keypairs
chmod 700 "${AUTHORITY_KEYPAIR_DIR}"

PROGRAMS=("memo_mint" "memo_burn" "memo_chat" "memo_profile" "memo_project" "memo_blog")

echo "========================================"
echo "PART 1: Program Keypairs"
echo "========================================"
echo ""
print_info "Location: ${PROGRAM_KEYPAIR_DIR}"
print_info "These determine your Program IDs"
echo ""

for program in "${PROGRAMS[@]}"; do
    KEYPAIR_FILE="${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json"
    
    if [ -f "${KEYPAIR_FILE}" ]; then
        print_success "${program}: keypair already exists"
        PUBKEY=$(solana-keygen pubkey "${KEYPAIR_FILE}")
        echo "  Program ID: ${PUBKEY}"
    else
        print_warning "${program}: keypair not found"
        echo ""
        echo "Options:"
        echo "  1) Copy existing keypair from backup"
        echo "  2) Generate NEW keypair"
        echo "  3) Skip for now"
        echo ""
        read -p "Choose option [1/2/3]: " choice
        
        case "$choice" in
            1)
                echo ""
                echo "Please copy your existing keypair to:"
                echo "  ${KEYPAIR_FILE}"
                read -p "Press Enter after copying the file..."
                
                if [ -f "${KEYPAIR_FILE}" ]; then
                    chmod 600 "${KEYPAIR_FILE}"
                    PUBKEY=$(solana-keygen pubkey "${KEYPAIR_FILE}")
                    print_success "${program}: keypair restored"
                    echo "  Program ID: ${PUBKEY}"
                else
                    print_error "File not found, skipping"
                fi
                ;;
            2)
                solana-keygen new --no-bip39-passphrase -o "${KEYPAIR_FILE}"
                chmod 600 "${KEYPAIR_FILE}"
                PUBKEY=$(solana-keygen pubkey "${KEYPAIR_FILE}")
                print_success "${program}: generated new keypair"
                echo "  Program ID: ${PUBKEY}"
                print_warning "Remember to update this Program ID in your code!"
                ;;
            3)
                print_info "${program}: skipped"
                ;;
            *)
                print_error "Invalid choice, skipping"
                ;;
        esac
    fi
done

echo ""
echo "========================================"
echo "PART 2: Authority Keypairs"
echo "========================================"
echo ""
print_info "Location: ${AUTHORITY_KEYPAIR_DIR}"
print_warning "Note: These keypairs are shared across environments via physical isolation"
echo ""

# Deploy Admin keypair (combined deploy + admin authority)
DEPLOY_ADMIN_KEYPAIR="${AUTHORITY_KEYPAIR_DIR}/deploy_admin-keypair.json"
echo "1. Deploy & Admin Authority"
if [ -f "${DEPLOY_ADMIN_KEYPAIR}" ]; then
    print_success "Deploy/Admin keypair already exists"
    ADMIN_PUBKEY=$(solana-keygen pubkey "${DEPLOY_ADMIN_KEYPAIR}")
    echo "  Pubkey: ${ADMIN_PUBKEY}"
    echo "  Usage: Deployment operations + Admin authority in contracts"
else
    print_warning "Deploy/Admin keypair not found"
    echo ""
    echo "This keypair is used for:"
    echo "  - Paying for contract deployment"
    echo "  - Admin operations in memo_chat and memo_project contracts"
    echo ""
    echo "Options:"
    echo "  1) Copy existing keypair from backup"
    echo "  2) Generate NEW keypair"
    echo "  3) Skip for now"
    echo ""
    read -p "Choose option [1/2/3]: " choice
    
    case "$choice" in
        1)
            echo ""
            echo "Please copy your existing keypair to:"
            echo "  ${DEPLOY_ADMIN_KEYPAIR}"
            read -p "Press Enter after copying the file..."
            
            if [ -f "${DEPLOY_ADMIN_KEYPAIR}" ]; then
                chmod 600 "${DEPLOY_ADMIN_KEYPAIR}"
                ADMIN_PUBKEY=$(solana-keygen pubkey "${DEPLOY_ADMIN_KEYPAIR}")
                print_success "Deploy/Admin keypair restored"
                echo "  Pubkey: ${ADMIN_PUBKEY}"
            else
                print_error "File not found, skipping"
            fi
            ;;
        2)
            solana-keygen new --no-bip39-passphrase -o "${DEPLOY_ADMIN_KEYPAIR}"
            chmod 600 "${DEPLOY_ADMIN_KEYPAIR}"
            ADMIN_PUBKEY=$(solana-keygen pubkey "${DEPLOY_ADMIN_KEYPAIR}")
            print_success "Deploy/Admin keypair generated"
            echo "  Pubkey: ${ADMIN_PUBKEY}"
            print_warning "Remember to update AUTHORIZED_ADMIN_PUBKEY in your code!"
            ;;
        3)
            print_info "Deploy/Admin keypair: skipped"
            ;;
        *)
            print_error "Invalid choice, skipping"
            ;;
    esac
fi

echo ""

# Mint authority
MINT_KEYPAIR="${AUTHORITY_KEYPAIR_DIR}/memo_token_mint-keypair.json"
echo "2. Mint Authority"
if [ -f "${MINT_KEYPAIR}" ]; then
    print_success "Mint authority keypair already exists"
    MINT_PUBKEY=$(solana-keygen pubkey "${MINT_KEYPAIR}")
    echo "  Pubkey: ${MINT_PUBKEY}"
    echo "  Usage: MEMO token minting authority"
else
    print_warning "Mint authority keypair not found"
    echo ""
    echo "This keypair is used for:"
    echo "  - Minting MEMO tokens"
    echo ""
    echo "Options:"
    echo "  1) Copy existing keypair from backup"
    echo "  2) Generate NEW keypair"
    echo "  3) Skip for now"
    echo ""
    read -p "Choose option [1/2/3]: " choice
    
    case "$choice" in
        1)
            echo ""
            echo "Please copy your existing keypair to:"
            echo "  ${MINT_KEYPAIR}"
            read -p "Press Enter after copying the file..."
            
            if [ -f "${MINT_KEYPAIR}" ]; then
                chmod 600 "${MINT_KEYPAIR}"
                MINT_PUBKEY=$(solana-keygen pubkey "${MINT_KEYPAIR}")
                print_success "Mint authority keypair restored"
                echo "  Pubkey: ${MINT_PUBKEY}"
            else
                print_error "File not found, skipping"
            fi
            ;;
        2)
            solana-keygen new --no-bip39-passphrase -o "${MINT_KEYPAIR}"
            chmod 600 "${MINT_KEYPAIR}"
            MINT_PUBKEY=$(solana-keygen pubkey "${MINT_KEYPAIR}")
            print_success "Mint authority keypair generated"
            echo "  Pubkey: ${MINT_PUBKEY}"
            print_warning "Remember to update AUTHORIZED_MINT_PUBKEY in your code!"
            ;;
        3)
            print_info "Mint authority keypair: skipped"
            ;;
        *)
            print_error "Invalid choice, skipping"
            ;;
    esac
fi

echo ""
echo "=========================================="
print_success "Setup complete for ${ENV}"
echo "=========================================="
echo ""
print_info "Keypair locations:"
echo "  Program keypairs: ${PROGRAM_KEYPAIR_DIR}"
echo "  Authority keypairs: ${AUTHORITY_KEYPAIR_DIR}"
echo ""

print_warning "IMPORTANT: Update your code with the actual pubkeys"
echo ""
echo "Next steps:"
echo "  1. Update program IDs in programs/*/src/lib.rs"
echo "  2. Update program IDs in Anchor.toml"
echo "  3. Update AUTHORIZED_MINT_PUBKEY in program code"
echo "  4. Update AUTHORIZED_ADMIN_PUBKEY in program code (memo_chat, memo_project)"
echo "  5. Commit changes to git"
echo "  6. Run deployment: ./scripts/deploy-${ENV}.sh"
echo ""

print_warning "SECURITY REMINDERS:"
echo "  ✓ Authority keypairs: ${AUTHORITY_KEYPAIR_DIR} (unified location)"
echo "  ✓ Program keypairs: ${PROGRAM_KEYPAIR_DIR} (Anchor default)"
echo "  ✓ Directory permissions set to 700 (owner only)"
echo "  ✓ File permissions set to 600 (owner read/write only)"
echo "  ✓ Authority keypairs are NOT tracked by git"
echo "  ✓ Use separate servers/machines for different environments"
echo "  ✓ Backup all keypairs to secure encrypted storage"
echo ""

if [ "${ENV}" = "mainnet" ]; then
    echo "  🔐 MAINNET SECURITY:"
    echo "  ✓ Ensure this is running on a SECURE, ISOLATED production server"
    echo "  ✓ Never use the same machine for testnet and mainnet"
    echo "  ✓ Mainnet Program IDs should be hardcoded in source"
    echo "  ✓ Authority pubkeys should be hardcoded in source"
    echo "  ✓ Deployment script will VERIFY (not replace) these values"
    echo "  ✓ Consider hardware wallet or multisig for authorities"
    echo "  ✓ Store backups in encrypted cold storage"
    echo "  ✓ Never share these keypairs"
    echo ""
fi