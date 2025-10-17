#!/bin/bash
# Setup keypairs for testnet or mainnet in secure location
# Stores keypairs in ~/.config/solana/memo-token/{env}/

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Keypair storage location
KEYPAIR_BASE_DIR="${HOME}/.config/solana/memo-token"

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
    echo "This script sets up keypairs in: ${KEYPAIR_BASE_DIR}/{env}/"
    exit 1
fi

if [ "${ENV}" != "testnet" ] && [ "${ENV}" != "mainnet" ]; then
    print_error "Invalid environment. Use 'testnet' or 'mainnet'"
    exit 1
fi

print_info "Setting up ${ENV} keypairs in secure location..."
print_info "Base directory: ${KEYPAIR_BASE_DIR}/${ENV}"
echo ""

# Create directories
PROGRAM_KEYPAIR_DIR="${KEYPAIR_BASE_DIR}/${ENV}/program-keypairs"
AUTHORITY_KEYPAIR_DIR="${KEYPAIR_BASE_DIR}/${ENV}/authority-keypairs"

mkdir -p "${PROGRAM_KEYPAIR_DIR}"
mkdir -p "${AUTHORITY_KEYPAIR_DIR}"

# Set restrictive permissions
chmod 700 "${KEYPAIR_BASE_DIR}"
chmod 700 "${KEYPAIR_BASE_DIR}/${ENV}"
chmod 700 "${PROGRAM_KEYPAIR_DIR}"
chmod 700 "${AUTHORITY_KEYPAIR_DIR}"

PROGRAMS=("memo_mint" "memo_burn" "memo_chat" "memo_profile" "memo_project")

# Array to store newly generated program IDs for testnet
declare -a NEW_PROGRAM_IDS

# Setup program keypairs
print_info "Setting up program keypairs..."
for program in "${PROGRAMS[@]}"; do
    KEYPAIR_FILE="${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json"
    
    if [ -f "${KEYPAIR_FILE}" ]; then
        print_success "${program}: keypair already exists"
        PUBKEY=$(solana-keygen pubkey "${KEYPAIR_FILE}")
        echo "  Pubkey: ${PUBKEY}"
    else
        if [ "${ENV}" = "mainnet" ]; then
            # For mainnet, generate new keypair
            solana-keygen new --no-bip39-passphrase -o "${KEYPAIR_FILE}"
            chmod 600 "${KEYPAIR_FILE}"
            PUBKEY=$(solana-keygen pubkey "${KEYPAIR_FILE}")
            print_success "${program}: generated new keypair"
            echo "  Pubkey: ${PUBKEY}"
        else
            # For testnet, give user options
            print_warning "${program}: keypair not found"
            echo ""
            echo "Options:"
            echo "  1) Copy existing keypair from backup"
            echo "  2) Generate NEW keypair (requires updating code)"
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
                        echo "  Pubkey: ${PUBKEY}"
                    else
                        print_error "File not found, skipping"
                    fi
                    ;;
                2)
                    solana-keygen new --no-bip39-passphrase -o "${KEYPAIR_FILE}"
                    chmod 600 "${KEYPAIR_FILE}"
                    PUBKEY=$(solana-keygen pubkey "${KEYPAIR_FILE}")
                    print_success "${program}: generated new keypair"
                    echo "  Pubkey: ${PUBKEY}"
                    
                    # Store for later reminder
                    NEW_PROGRAM_IDS+=("${program}=${PUBKEY}")
                    ;;
                3)
                    print_info "${program}: skipped"
                    ;;
                *)
                    print_error "Invalid choice, skipping"
                    ;;
            esac
        fi
    fi
done

echo ""

# Setup authority keypairs
print_info "Setting up authority keypairs..."

# Mint authority
MINT_KEYPAIR="${AUTHORITY_KEYPAIR_DIR}/memo_token-mint_keypair.json"
if [ -f "${MINT_KEYPAIR}" ]; then
    print_success "Mint authority: keypair already exists"
    MINT_PUBKEY=$(solana-keygen pubkey "${MINT_KEYPAIR}")
    echo "  Pubkey: ${MINT_PUBKEY}"
else
    if [ "${ENV}" = "mainnet" ]; then
        # For mainnet, generate new keypair
        solana-keygen new --no-bip39-passphrase -o "${MINT_KEYPAIR}"
        chmod 600 "${MINT_KEYPAIR}"
        MINT_PUBKEY=$(solana-keygen pubkey "${MINT_KEYPAIR}")
        print_success "Mint authority: generated new keypair"
        echo "  Pubkey: ${MINT_PUBKEY}"
    else
        # For testnet, give user options
        print_warning "Mint authority: keypair not found"
        echo ""
        echo "Options:"
        echo "  1) Copy existing keypair from backup"
        echo "  2) Generate NEW keypair (requires updating code)"
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
                    print_success "Mint authority: keypair restored"
                    echo "  Pubkey: ${MINT_PUBKEY}"
                else
                    print_error "File not found, skipping"
                fi
                ;;
            2)
                solana-keygen new --no-bip39-passphrase -o "${MINT_KEYPAIR}"
                chmod 600 "${MINT_KEYPAIR}"
                MINT_PUBKEY=$(solana-keygen pubkey "${MINT_KEYPAIR}")
                print_success "Mint authority: generated new keypair"
                echo "  Pubkey: ${MINT_PUBKEY}"
                
                NEW_PROGRAM_IDS+=("AUTHORIZED_MINT_PUBKEY=${MINT_PUBKEY}")
                ;;
            3)
                print_info "Mint authority: skipped"
                ;;
            *)
                print_error "Invalid choice, skipping"
                ;;
        esac
    fi
fi

# Admin authority
ADMIN_KEYPAIR="${AUTHORITY_KEYPAIR_DIR}/admin_keypair.json"
if [ -f "${ADMIN_KEYPAIR}" ]; then
    print_success "Admin authority: keypair already exists"
    ADMIN_PUBKEY=$(solana-keygen pubkey "${ADMIN_KEYPAIR}")
    echo "  Pubkey: ${ADMIN_PUBKEY}"
else
    if [ "${ENV}" = "mainnet" ]; then
        # For mainnet, generate new keypair
        solana-keygen new --no-bip39-passphrase -o "${ADMIN_KEYPAIR}"
        chmod 600 "${ADMIN_KEYPAIR}"
        ADMIN_PUBKEY=$(solana-keygen pubkey "${ADMIN_KEYPAIR}")
        print_success "Admin authority: generated new keypair"
        echo "  Pubkey: ${ADMIN_PUBKEY}"
    else
        # For testnet, give user options
        print_warning "Admin authority: keypair not found"
        echo ""
        echo "Options:"
        echo "  1) Copy existing keypair from backup"
        echo "  2) Generate NEW keypair (requires updating code)"
        echo "  3) Skip for now"
        echo ""
        read -p "Choose option [1/2/3]: " choice
        
        case "$choice" in
            1)
                echo ""
                echo "Please copy your existing keypair to:"
                echo "  ${ADMIN_KEYPAIR}"
                read -p "Press Enter after copying the file..."
                
                if [ -f "${ADMIN_KEYPAIR}" ]; then
                    chmod 600 "${ADMIN_KEYPAIR}"
                    ADMIN_PUBKEY=$(solana-keygen pubkey "${ADMIN_KEYPAIR}")
                    print_success "Admin authority: keypair restored"
                    echo "  Pubkey: ${ADMIN_PUBKEY}"
                else
                    print_error "File not found, skipping"
                fi
                ;;
            2)
                solana-keygen new --no-bip39-passphrase -o "${ADMIN_KEYPAIR}"
                chmod 600 "${ADMIN_KEYPAIR}"
                ADMIN_PUBKEY=$(solana-keygen pubkey "${ADMIN_KEYPAIR}")
                print_success "Admin authority: generated new keypair"
                echo "  Pubkey: ${ADMIN_PUBKEY}"
                
                NEW_PROGRAM_IDS+=("AUTHORIZED_ADMIN_PUBKEY=${ADMIN_PUBKEY}")
                ;;
            3)
                print_info "Admin authority: skipped"
                ;;
            *)
                print_error "Invalid choice, skipping"
                ;;
        esac
    fi
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

# If new program IDs were generated for testnet, remind user to update code
if [ "${ENV}" = "testnet" ] && [ ${#NEW_PROGRAM_IDS[@]} -gt 0 ]; then
    echo ""
    print_warning "⚠️  IMPORTANT: NEW KEYPAIRS GENERATED FOR TESTNET"
    echo ""
    echo "You need to manually update the following in your code:"
    echo ""
    
    for entry in "${NEW_PROGRAM_IDS[@]}"; do
        name="${entry%%=*}"
        pubkey="${entry##*=}"
        
        # Check if it's a program or authority
        if [[ "$name" == *"PUBKEY"* ]]; then
            # It's an authority pubkey
            echo "In programs that use ${name}:"
            echo "  ${name}: ${pubkey}"
        else
            # It's a program ID
            program_dash="${name//_/-}"
            echo "In programs/${program_dash}/src/lib.rs:"
            echo "  declare_id!(\"${pubkey}\");"
        fi
    done
    
    echo ""
    echo "After updating, commit the changes:"
    echo "  git add programs/*/src/lib.rs"
    echo "  git commit -m \"Update testnet program IDs\""
    echo ""
fi

print_warning "SECURITY REMINDERS:"
echo "  ✓ Keypairs are stored outside the project directory"
echo "  ✓ Directory permissions set to 700 (owner only)"
echo "  ✓ File permissions set to 600 (owner read/write only)"
echo "  ✓ These directories are NOT tracked by git"
echo "  ✓ Backup these keypairs to secure encrypted storage"
if [ "${ENV}" = "mainnet" ]; then
    echo ""
    echo "  🔐 MAINNET SECURITY:"
    echo "  ✓ Consider using hardware wallet for maximum security"
    echo "  ✓ Store backup in encrypted cold storage"
    echo "  ✓ Never share these keypairs"
fi