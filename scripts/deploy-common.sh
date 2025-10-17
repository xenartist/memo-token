#!/bin/bash
# Common deployment logic - works for both testnet and mainnet
# Compatible with bash 3.x (macOS default)

set -e

# Locate project root (one level up from scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Keypair storage location (outside project directory for security)
KEYPAIR_BASE_DIR="${HOME}/.config/solana/memo-token"

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

ALL_PROGRAMS=("memo_mint" "memo_burn" "memo_chat" "memo_profile" "memo_project")

print_info() { echo -e "${BLUE}ℹ️  $1${NC}"; }
print_success() { echo -e "${GREEN}✅ $1${NC}"; }
print_warning() { echo -e "${YELLOW}⚠️  $1${NC}"; }
print_error() { echo -e "${RED}❌ $1${NC}"; }

# Helper function to convert to uppercase (bash 3 compatible)
to_upper() {
    echo "$1" | tr '[:lower:]' '[:upper:]'
}

# Helper function to convert program name to dash format
get_program_name_dash() {
    case "$1" in
        memo_mint|memo-mint) echo "memo-mint" ;;
        memo_burn|memo-burn) echo "memo-burn" ;;
        memo_chat|memo-chat) echo "memo-chat" ;;
        memo_profile|memo-profile) echo "memo-profile" ;;
        memo_project|memo-project) echo "memo-project" ;;
        *) echo "$1" ;;
    esac
}

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

# Helper function to get expected testnet ID
get_expected_testnet_id() {
    case "$1" in
        memo_mint) echo "A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy" ;;
        memo_burn) echo "FEjJ9KKJETocmaStfsFteFrktPchDLAVNTMeTvndoxaP" ;;
        memo_chat) echo "54ky4LNnRsbYioDSBKNrc5hG8HoDyZ6yhf8TuncxTBRF" ;;
        memo_profile) echo "BwQTxuShrwJR15U6Utdfmfr4kZ18VT6FA1fcp58sT8US" ;;
        memo_project) echo "ENVapgjzzMjbRhLJ279yNsSgaQtDYYVgWq98j54yYnyx" ;;
        *) echo "" ;;
    esac
}

cleanup() {
    print_info "Cleaning up temporary changes..."
    cd "${PROJECT_ROOT}"
    git checkout -- Anchor.toml programs/*/src/lib.rs 2>/dev/null || true
    rm -f Anchor.toml.bak programs/*/src/lib.rs.bak
    print_success "Cleanup complete"
}

# Function to update program IDs and authority pubkeys
update_program_ids() {
    local ENV=$1
    shift
    local PROGRAMS=("$@")
    
    # New keypair locations
    local PROGRAM_KEYPAIR_DIR="${KEYPAIR_BASE_DIR}/${ENV}/program-keypairs"
    local AUTHORITY_KEYPAIR_DIR="${KEYPAIR_BASE_DIR}/${ENV}/authority-keypairs"
    
    print_info "Updating ${ENV} program IDs and authority pubkeys..."
    print_info "Program keypairs: ${PROGRAM_KEYPAIR_DIR}"
    print_info "Authority keypairs: ${AUTHORITY_KEYPAIR_DIR}"
    echo ""
    
    # Verify directories exist
    if [ ! -d "${PROGRAM_KEYPAIR_DIR}" ]; then
        print_error "Program keypair directory not found: ${PROGRAM_KEYPAIR_DIR}"
        print_info "Run: ./scripts/setup-keypairs.sh ${ENV}"
        exit 1
    fi
    
    if [ ! -d "${AUTHORITY_KEYPAIR_DIR}" ]; then
        print_error "Authority keypair directory not found: ${AUTHORITY_KEYPAIR_DIR}"
        print_info "Run: ./scripts/setup-keypairs.sh ${ENV}"
        exit 1
    fi
    
    # Read authority keypairs
    local MINT_AUTHORITY_KEYPAIR="${AUTHORITY_KEYPAIR_DIR}/memo_token-mint_keypair.json"
    local ADMIN_AUTHORITY_KEYPAIR="${AUTHORITY_KEYPAIR_DIR}/admin_keypair.json"
    
    # Verify authority keypairs exist
    if [ ! -f "${MINT_AUTHORITY_KEYPAIR}" ]; then
        print_error "Mint authority keypair not found: ${MINT_AUTHORITY_KEYPAIR}"
        exit 1
    fi
    
    if [ ! -f "${ADMIN_AUTHORITY_KEYPAIR}" ]; then
        print_error "Admin authority keypair not found: ${ADMIN_AUTHORITY_KEYPAIR}"
        exit 1
    fi
    
    # Get authority pubkeys
    local MINT_AUTHORITY_PUBKEY=$(solana-keygen pubkey "${MINT_AUTHORITY_KEYPAIR}")
    local ADMIN_AUTHORITY_PUBKEY=$(solana-keygen pubkey "${ADMIN_AUTHORITY_KEYPAIR}")
    
    print_info "Authority Pubkeys for ${ENV}:"
    echo "  Mint Authority: ${MINT_AUTHORITY_PUBKEY}"
    echo "  Admin Authority: ${ADMIN_AUTHORITY_PUBKEY}"
    echo ""
    
    # Display program IDs from keypairs
    print_info "Program IDs for ${ENV}:"
    for program in "${PROGRAMS[@]}"; do
        local keypair_file="${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json"
        if [ ! -f "${keypair_file}" ]; then
            print_error "Program keypair not found: ${keypair_file}"
            exit 1
        fi
        local program_id=$(solana-keygen pubkey "${keypair_file}")
        echo "  ${program}: ${program_id}"
    done
    echo ""
    
    # Change to project root for file operations
    cd "${PROJECT_ROOT}"
    
    if [ "${ENV}" = "mainnet" ]; then
        # Replace PLACEHOLDER_MAINNET_* in Anchor.toml
        print_info "Updating Anchor.toml..."
        for program in "${PROGRAMS[@]}"; do
            local program_id=$(solana-keygen pubkey "${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json")
            sed -i.bak "s|${program} = \"PLACEHOLDER_MAINNET\"|${program} = \"${program_id}\"|g" Anchor.toml
        done
        
        # Replace placeholders in source files
        print_info "Updating program source files..."
        for program in "${PROGRAMS[@]}"; do
            local program_id=$(solana-keygen pubkey "${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json")
            local program_dash=$(get_program_name_dash "${program}")
            
            # Replace program ID
            sed -i.bak "s|declare_id!(\"PLACEHOLDER_MAINNET\")|declare_id!(\"${program_id}\")|" "programs/${program_dash}/src/lib.rs"
            
            # Replace AUTHORIZED_MINT_PUBKEY
            sed -i.bak "s|pubkey!(\"PLACEHOLDER_MAINNET_MINT_AUTHORITY\")|pubkey!(\"${MINT_AUTHORITY_PUBKEY}\")|g" "programs/${program_dash}/src/lib.rs"
            
            # Replace AUTHORIZED_ADMIN_PUBKEY (only for chat and project)
            if [ "${program}" = "memo_chat" ] || [ "${program}" = "memo_project" ]; then
                sed -i.bak "s|pubkey!(\"PLACEHOLDER_MAINNET_ADMIN_AUTHORITY\")|pubkey!(\"${ADMIN_AUTHORITY_PUBKEY}\")|g" "programs/${program_dash}/src/lib.rs"
            fi
        done
    else
        # For testnet: verify IDs match
        print_info "Verifying testnet program IDs and authority pubkeys..."
        
        # Check program IDs
        local all_match=true
        for program in "${PROGRAMS[@]}"; do
            local program_id=$(solana-keygen pubkey "${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json")
            local expected_id=$(get_expected_testnet_id "${program}")
            
            if [ "${program_id}" != "${expected_id}" ]; then
                print_warning "${program}: Keypair ID doesn't match code!"
                echo "  Expected (in code): ${expected_id}"
                echo "  Actual (in keypair): ${program_id}"
                all_match=false
            fi
        done
        
        # Check authority pubkeys
        local EXPECTED_MINT_AUTHORITY="HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1"
        local EXPECTED_ADMIN_AUTHORITY="Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn"
        
        if [ "${MINT_AUTHORITY_PUBKEY}" != "${EXPECTED_MINT_AUTHORITY}" ]; then
            print_warning "Mint authority doesn't match!"
            echo "  Expected: ${EXPECTED_MINT_AUTHORITY}"
            echo "  Actual: ${MINT_AUTHORITY_PUBKEY}"
            all_match=false
        fi
        
        if [ "${ADMIN_AUTHORITY_PUBKEY}" != "${EXPECTED_ADMIN_AUTHORITY}" ]; then
            print_warning "Admin authority doesn't match!"
            echo "  Expected: ${EXPECTED_ADMIN_AUTHORITY}"
            echo "  Actual: ${ADMIN_AUTHORITY_PUBKEY}"
            all_match=false
        fi
        
        if [ "$all_match" = "false" ]; then
            echo ""
            print_error "Mismatch detected!"
            read -p "Continue and update code? (yes/no): " continue_mismatch
            if [ "$continue_mismatch" != "yes" ]; then
                exit 1
            fi
            
            # Update code to match keypairs
            print_info "Updating code to match keypair IDs..."
            for program in "${PROGRAMS[@]}"; do
                local program_id=$(solana-keygen pubkey "${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json")
                local expected_id=$(get_expected_testnet_id "${program}")
                local program_dash=$(get_program_name_dash "${program}")
                
                # Update program ID
                sed -i.bak "s|declare_id!(\"${expected_id}\")|declare_id!(\"${program_id}\")|" "programs/${program_dash}/src/lib.rs"
                
                # Update AUTHORIZED_MINT_PUBKEY
                sed -i.bak "s|pubkey!(\"${EXPECTED_MINT_AUTHORITY}\")|pubkey!(\"${MINT_AUTHORITY_PUBKEY}\")|g" "programs/${program_dash}/src/lib.rs"
                
                # Update AUTHORIZED_ADMIN_PUBKEY (only for chat and project)
                if [ "${program}" = "memo_chat" ] || [ "${program}" = "memo_project" ]; then
                    sed -i.bak "s|pubkey!(\"${EXPECTED_ADMIN_AUTHORITY}\")|pubkey!(\"${ADMIN_AUTHORITY_PUBKEY}\")|g" "programs/${program_dash}/src/lib.rs"
                fi
            done
        else
            print_success "All IDs and authority pubkeys match! No code changes needed."
        fi
    fi
    
    print_success "Program IDs and authority pubkeys updated!"
}

# Main deployment function
deploy_to_env() {
    local ENV=$1
    local CLUSTER=$2
    local WALLET=$3
    local FEATURE_FLAG=$4
    shift 4
    local SELECTED_PROGRAMS=("$@")
    
    # New keypair locations
    local PROGRAM_KEYPAIR_DIR="${KEYPAIR_BASE_DIR}/${ENV}/program-keypairs"
    
    # If no programs specified, deploy all
    if [ ${#SELECTED_PROGRAMS[@]} -eq 0 ]; then
        SELECTED_PROGRAMS=("${ALL_PROGRAMS[@]}")
        print_info "No specific programs specified, deploying all programs"
    else
        # Validate and normalize program names
        local VALIDATED_PROGRAMS=()
        for prog in "${SELECTED_PROGRAMS[@]}"; do
            local normalized=$(get_program_name_underscore "$prog")
            if is_valid_program "$normalized"; then
                VALIDATED_PROGRAMS+=("$normalized")
            else
                print_error "Invalid program name: $prog"
                echo "Valid programs: ${ALL_PROGRAMS[*]}"
                exit 1
            fi
        done
        SELECTED_PROGRAMS=("${VALIDATED_PROGRAMS[@]}")
        print_info "Deploying selected programs: ${SELECTED_PROGRAMS[*]}"
    fi
    
    echo ""
    echo "=========================================="
    echo "🚀 Deploying to $(to_upper "${ENV}")"
    echo "=========================================="
    echo ""
    
    # Change to project root
    cd "${PROJECT_ROOT}"
    
    trap cleanup EXIT
    
    # Check keypairs
    if [ ! -d "${PROGRAM_KEYPAIR_DIR}" ]; then
        print_error "${ENV} program keypairs not found!"
        print_info "Expected location: ${PROGRAM_KEYPAIR_DIR}"
        print_info "Run: ./scripts/setup-keypairs.sh ${ENV}"
        exit 1
    fi
    
    # Check git status
    if [ -n "$(git status --porcelain)" ]; then
        print_warning "You have uncommitted changes."
        read -p "Continue anyway? (yes/no): " continue_dirty
        if [ "$continue_dirty" != "yes" ]; then
            exit 0
        fi
    fi
    
    # Confirm deployment
    read -p "Deploy to $(to_upper "${ENV}")? (yes/no): " confirm
    if [ "$confirm" != "yes" ]; then
        print_info "Cancelled."
        exit 0
    fi
    
    # Step 1: Update IDs
    echo ""
    print_info "Step 1: Updating program IDs..."
    update_program_ids "${ENV}" "${SELECTED_PROGRAMS[@]}"
    
    # Step 2: Build
    echo ""
    print_info "Step 2: Building programs..."
    if [ -n "${FEATURE_FLAG}" ]; then
        print_info "Building with feature: ${FEATURE_FLAG}"
        for program in "${SELECTED_PROGRAMS[@]}"; do
            local program_dash=$(get_program_name_dash "${program}")
            print_info "Building ${program_dash}..."
            anchor build --features "${FEATURE_FLAG}" -p "${program_dash}"
        done
    else
        for program in "${SELECTED_PROGRAMS[@]}"; do
            local program_dash=$(get_program_name_dash "${program}")
            print_info "Building ${program_dash}..."
            anchor build -p "${program_dash}"
        done
    fi
    
    # Step 3: Deploy
    echo ""
    print_info "Step 3: Deploying to ${CLUSTER}..."
    
    export ANCHOR_PROVIDER_URL="${CLUSTER}"
    export ANCHOR_WALLET="${WALLET}"
    
    for program in "${SELECTED_PROGRAMS[@]}"; do
        echo ""
        print_info "Deploying ${program}..."
        
        local program_id=$(solana-keygen pubkey "${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json")
        
        anchor deploy \
            --provider.cluster "${CLUSTER}" \
            --program-name "${program}" \
            --program-keypair "${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json"
        
        print_success "${program} deployed: ${program_id}"
        
        if [ "${ENV}" = "mainnet" ]; then
            echo "   Explorer: https://explorer.solana.com/address/${program_id}"
        else
            echo "   Explorer: https://explorer.solana.com/address/${program_id}?cluster=custom&customUrl=${CLUSTER}"
        fi
    done
    
    # Summary
    echo ""
    echo "=========================================="
    print_success "Deployment Complete!"
    echo "=========================================="
    echo ""
    print_info "Deployed Program IDs ($(to_upper "${ENV}")):"
    for program in "${SELECTED_PROGRAMS[@]}"; do
        local program_id=$(solana-keygen pubkey "${PROGRAM_KEYPAIR_DIR}/${program}-keypair.json")
        echo "  ${program} = \"${program_id}\""
    done
    
    echo ""
    print_warning "Temporary changes will be cleaned up automatically."
}