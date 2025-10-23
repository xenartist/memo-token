#!/bin/bash
# Close (delete) deployed programs from the blockchain
# This will refund the rent to the upgrade authority
# Usage: ./scripts/close-programs.sh [program1] [program2] ...
# Example: ./scripts/close-programs.sh memo_mint memo_burn

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_info() { echo -e "${BLUE}ℹ️  $1${NC}"; }
print_success() { echo -e "${GREEN}✅ $1${NC}"; }
print_warning() { echo -e "${YELLOW}⚠️  $1${NC}"; }
print_error() { echo -e "${RED}❌ $1${NC}"; }

ALL_PROGRAMS=("memo_mint" "memo_burn" "memo_chat" "memo_profile" "memo_project")

# Helper function to convert program name to underscore format
get_program_name_underscore() {
    case "$1" in
        memo-mint|memo_mint) echo "memo_mint" ;;
        memo-burn|memo_burn) echo "memo_burn" ;;
        memo-chat|memo_chat) echo "memo_chat" ;;
        memo-profile|memo_profile) echo "memo_profile" ;;
        memo-project|memo_project) echo "memo_project" ;;
        *) echo "$1" ;;
    esac
}

# Helper function to validate program name
is_valid_program() {
    local program=$(get_program_name_underscore "$1")
    for valid_program in "${ALL_PROGRAMS[@]}"; do
        if [ "$valid_program" = "$program" ]; then
            return 0
        fi
    done
    return 1
}

# Parse arguments
SELECTED_PROGRAMS=("$@")

if [ ${#SELECTED_PROGRAMS[@]} -eq 0 ]; then
    print_error "No programs specified"
    echo ""
    echo "Usage: ./scripts/close-programs.sh [program1] [program2] ..."
    echo "Example: ./scripts/close-programs.sh memo_mint memo_burn"
    echo ""
    echo "Available programs: ${ALL_PROGRAMS[*]}"
    exit 1
fi

# Validate and normalize program names
VALIDATED_PROGRAMS=()
for prog in "${SELECTED_PROGRAMS[@]}"; do
    normalized=$(get_program_name_underscore "$prog")
    if is_valid_program "$normalized"; then
        VALIDATED_PROGRAMS+=("$normalized")
    else
        print_error "Invalid program name: $prog"
        echo "Valid programs: ${ALL_PROGRAMS[*]}"
        exit 1
    fi
done

echo ""
echo "=========================================="
print_warning "CLOSE PROGRAMS"
echo "=========================================="
echo ""

# Get current RPC URL
CURRENT_RPC=$(solana config get | grep "RPC URL" | awk '{print $3}')
print_info "Current RPC: ${CURRENT_RPC}"

# Determine which upgrade authority to use
print_info "Checking upgrade authorities..."
echo ""

# Check upgrade authority for each program
for program in "${VALIDATED_PROGRAMS[@]}"; do
    PROGRAM_ID=$(solana-keygen pubkey "${PROJECT_ROOT}/target/deploy/${program}-keypair.json")
    echo "Program: ${program}"
    echo "  Program ID: ${PROGRAM_ID}"
    
    # Get upgrade authority
    UPGRADE_AUTH=$(solana program show ${PROGRAM_ID} 2>/dev/null | grep "Authority" | awk '{print $2}')
    
    if [ -z "${UPGRADE_AUTH}" ]; then
        print_warning "  Program not found on chain (already closed or never deployed)"
    else
        echo "  Upgrade Authority: ${UPGRADE_AUTH}"
    fi
    echo ""
done

# Determine which keypair to use for closing
print_warning "Which keypair should be used as the upgrade authority to close these programs?"
echo ""
echo "1) ~/.config/solana/id.json (default Solana CLI wallet)"
echo "2) ~/.config/solana/memo-token/authority/deploy_admin-keypair.json (new deploy admin)"
echo ""
read -p "Enter choice (1 or 2): " KEYPAIR_CHOICE

case $KEYPAIR_CHOICE in
    1)
        UPGRADE_AUTHORITY_KEYPAIR="${HOME}/.config/solana/id.json"
        ;;
    2)
        UPGRADE_AUTHORITY_KEYPAIR="${HOME}/.config/solana/memo-token/authority/deploy_admin-keypair.json"
        ;;
    *)
        print_error "Invalid choice"
        exit 1
        ;;
esac

if [ ! -f "${UPGRADE_AUTHORITY_KEYPAIR}" ]; then
    print_error "Keypair file not found: ${UPGRADE_AUTHORITY_KEYPAIR}"
    exit 1
fi

UPGRADE_AUTHORITY_PUBKEY=$(solana-keygen pubkey "${UPGRADE_AUTHORITY_KEYPAIR}")
print_info "Using upgrade authority: ${UPGRADE_AUTHORITY_PUBKEY}"
echo ""

# Confirm closing
print_warning "WARNING: This will CLOSE (delete) the following programs from the blockchain:"
for program in "${VALIDATED_PROGRAMS[@]}"; do
    PROGRAM_ID=$(solana-keygen pubkey "${PROJECT_ROOT}/target/deploy/${program}-keypair.json")
    echo "  - ${program} (${PROGRAM_ID})"
done
echo ""
print_warning "The rent will be refunded to: ${UPGRADE_AUTHORITY_PUBKEY}"
echo ""
read -p "Are you sure you want to close these programs? (yes/no): " confirm

if [ "$confirm" != "yes" ]; then
    print_info "Operation cancelled"
    exit 0
fi

# Close each program
echo ""
print_info "Closing programs..."
echo ""

for program in "${VALIDATED_PROGRAMS[@]}"; do
    PROGRAM_ID=$(solana-keygen pubkey "${PROJECT_ROOT}/target/deploy/${program}-keypair.json")
    
    echo "Closing ${program}..."
    
    if solana program close ${PROGRAM_ID} \
        --authority "${UPGRADE_AUTHORITY_KEYPAIR}" \
        --bypass-warning 2>&1; then
        print_success "${program} closed successfully"
    else
        print_error "Failed to close ${program}"
        echo "  This might mean:"
        echo "  - Program doesn't exist on chain"
        echo "  - Wrong authority"
        echo "  - Insufficient balance for transaction fees"
    fi
    echo ""
done

echo ""
echo "=========================================="
print_success "CLOSE OPERATION COMPLETE"
echo "=========================================="
echo ""
print_info "Next steps:"
echo "  1. Verify programs are closed using: solana program show <PROGRAM_ID>"
echo "  2. Re-deploy with correct configuration: ./scripts/deploy-testnet.sh"
echo ""

