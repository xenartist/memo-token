#!/bin/bash
# Common deployment logic - works for both testnet and mainnet
# Compatible with bash 3.x (macOS default)
#
# SECURITY: This script only VERIFIES configurations, never modifies source code

set -e

# Locate project root (one level up from scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Keypair locations: one tree per environment, managed by scripts/keys.sh. Keypairs are stored
# GPG-encrypted (<name>-keypair.json.gpg) with their public key (<name>-keypair.pub); while the
# environment is unlocked, <name>-keypair.json links to the decrypted file on a RAM disk.
KEYS_BASE="${HOME}/.config/solana/memo-token"
TARGET_DEPLOY_DIR="${PROJECT_ROOT}/target/deploy"

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

ALL_PROGRAMS=("memo_mint" "memo_burn" "memo_chat" "memo_profile" "memo_project" "memo_blog" "memo_forum")

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
        memo_blog|memo-blog) echo "memo-blog" ;;
        memo_forum|memo-forum) echo "memo-forum" ;;
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
        memo-blog|memo_blog) echo "memo_blog" ;;
        memo-forum|memo_forum) echo "memo_forum" ;;
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

# Keypair paths for an environment (testnet/mainnet); the file exists only while unlocked
authority_keypair_path() {
    echo "${KEYS_BASE}/$2/authority/$1-keypair.json"
}

program_keypair_path() {
    local program=$(get_program_name_underscore "$1")
    echo "${KEYS_BASE}/$2/programs/${program}-keypair.json"
}

# Public key of a stored keypair (works while locked); empty if the keypair is not stored
stored_pubkey() {
    local pub="${1%.json}.pub"
    if [ -f "${pub}" ]; then
        cat "${pub}"
    elif [ -f "$1" ]; then
        solana-keygen pubkey "$1"
    fi
}

# On-chain state of a program: prints "deployed <upgrade authority>", "absent", or "error <message>"
get_program_state() {
    local program_id=$1
    local cluster=$2
    local keypair=$3 # any keypair: the CLI wants a signer even for this read-only query
    local output
    if output=$(solana program show "${program_id}" -u "${cluster}" -k "${keypair}" 2>&1); then
        echo "deployed $(echo "${output}" | awk '/^Authority:/{print $2}')"
    elif echo "${output}" | grep -q "Unable to find the account"; then
        echo "absent"
    else
        echo "error $(echo "${output}" | head -n 1)"
    fi
}

# Upgrade the on-chain IDL, or create it if missing. The IDL instructions run inside the program,
# which cannot be invoked in the slot it was upgraded in ("Program is not deployed"), so retry with a delay.
sync_idl() {
    local program_id=$1
    local idl_file=$2
    local cluster=$3
    local attempt
    for attempt in 1 2 3 4 5; do
        sleep 3
        if anchor idl upgrade "${program_id}" --filepath "${idl_file}" \
                --provider.cluster "${cluster}" --provider.wallet "${DEPLOY_WALLET}" >/dev/null 2>&1 \
            || anchor idl init "${program_id}" --filepath "${idl_file}" \
                --provider.cluster "${cluster}" --provider.wallet "${DEPLOY_WALLET}" >/dev/null 2>&1; then
            return 0
        fi
        print_info "IDL update not possible yet (attempt ${attempt}/5), retrying..."
    done
    return 1
}

# Helper function to extract program ID from source code
extract_program_id_from_code() {
    local program=$1
    local env=$2
    local program_dash=$(get_program_name_dash "${program}")
    local source_file="${PROJECT_ROOT}/programs/${program_dash}/src/lib.rs"
    
    if [ ! -f "${source_file}" ]; then
        echo ""
        return
    fi
    
    local result=""
    if [ "${env}" = "mainnet" ]; then
        # Look for pattern: #[cfg(feature = "mainnet")] followed by declare_id!
        result=$(grep -A 1 '#\[cfg(feature = "mainnet")\]' "${source_file}" | \
                 grep 'declare_id!' | \
                 sed -E 's/.*declare_id!\("([^"]+)"\).*/\1/' | \
                 head -n 1)
    else
        # Look for pattern: #[cfg(not(feature = "mainnet"))] followed by declare_id!
        result=$(grep -A 1 '#\[cfg(not(feature = "mainnet"))\]' "${source_file}" | \
                 grep 'declare_id!' | \
                 sed -E 's/.*declare_id!\("([^"]+)"\).*/\1/' | \
                 head -n 1)
    fi
    
    echo "${result}"
}

# Helper function to extract authority pubkey from source code
extract_authority_from_code() {
    local program=$1
    local authority_type=$2  # "MINT" or "ADMIN"
    local env=$3
    local program_dash=$(get_program_name_dash "${program}")
    local source_file="${PROJECT_ROOT}/programs/${program_dash}/src/lib.rs"
    
    if [ ! -f "${source_file}" ]; then
        echo ""
        return
    fi
    
    local const_name="AUTHORIZED_${authority_type}_PUBKEY"
    local result=""
    
    if [ "${env}" = "mainnet" ]; then
        # Look for pattern: #[cfg(feature = "mainnet")] followed by const AUTHORIZED_*_PUBKEY
        result=$(grep -A 1 '#\[cfg(feature = "mainnet")\]' "${source_file}" | \
                 grep "${const_name}" | \
                 grep 'pubkey!' | \
                 sed -E 's/.*pubkey!\("([^"]+)"\).*/\1/' | \
                 head -n 1)
    else
        # Look for pattern: #[cfg(not(feature = "mainnet"))] followed by const AUTHORIZED_*_PUBKEY
        result=$(grep -A 1 '#\[cfg(not(feature = "mainnet"))\]' "${source_file}" | \
                 grep "${const_name}" | \
                 grep 'pubkey!' | \
                 sed -E 's/.*pubkey!\("([^"]+)"\).*/\1/' | \
                 head -n 1)
    fi
    
    echo "${result}"
}

# Function to verify program IDs and authority pubkeys
verify_configuration() {
    local ENV=$1
    shift
    local PROGRAMS=("$@")
    
    print_info "Verifying ${ENV} configuration..."
    print_info "Keypairs: ${KEYS_BASE}/${ENV} (program keypairs are needed only for first deploys)"
    echo ""
    
    local DEPLOY_ADMIN_KEYPAIR=$(authority_keypair_path deploy_admin "${ENV}")
    local MINT_AUTHORITY_KEYPAIR=$(authority_keypair_path memo_token_mint "${ENV}")
    
    # The deploy admin signs the deployment, so it must be unlocked; the mint keypair only needs its public key
    if [ ! -f "${DEPLOY_ADMIN_KEYPAIR}" ]; then
        if [ -f "${DEPLOY_ADMIN_KEYPAIR}.gpg" ]; then
            print_error "${ENV} keys are locked. Run in your terminal: ./scripts/keys.sh unlock ${ENV}"
        else
            print_error "Deploy/Admin keypair not found: ${DEPLOY_ADMIN_KEYPAIR}.gpg"
            print_info "Run: ./scripts/setup-keypairs.sh ${ENV}"
        fi
        exit 1
    fi
    
    local MINT_AUTHORITY_PUBKEY=$(stored_pubkey "${MINT_AUTHORITY_KEYPAIR}")
    if [ -z "${MINT_AUTHORITY_PUBKEY}" ]; then
        print_error "Mint authority keypair not found: ${MINT_AUTHORITY_KEYPAIR}.gpg"
        print_info "Run: ./scripts/setup-keypairs.sh ${ENV}"
        exit 1
    fi
    local ADMIN_AUTHORITY_PUBKEY=$(solana-keygen pubkey "${DEPLOY_ADMIN_KEYPAIR}")
    
    print_info "Authority Pubkeys (from keypair files):"
    echo "  Deploy/Admin Authority: ${ADMIN_AUTHORITY_PUBKEY}"
    echo "  Mint Authority: ${MINT_AUTHORITY_PUBKEY}"
    echo ""
    
    # Verify each program
    print_info "Verifying Program Configurations:"
    echo ""
    local all_match=true
    local mismatch_details=""
    
    for program in "${PROGRAMS[@]}"; do
        local program_dash=$(get_program_name_dash "${program}")
        echo "Checking ${program}..."
        
        # 1. Program ID from source code (the source of truth)
        local code_program_id=$(extract_program_id_from_code "${program}" "${ENV}")
        local keypair_file=$(program_keypair_path "${program}" "${ENV}")
        
        echo -n "  Program ID: "
        if [ -z "${code_program_id}" ]; then
            print_warning "Could not extract from code"
            mismatch_details="${mismatch_details}\n${program}: Could not verify program ID in code"
            all_match=false
        else
            echo "${code_program_id}"
            
            # 2. If the program keypair is stored, it must match the code
            local keypair_program_id=$(stored_pubkey "${keypair_file}")
            if [ -n "${keypair_program_id}" ]; then
                echo -n "  Program keypair: "
                if [ "${keypair_program_id}" != "${code_program_id}" ]; then
                    print_error "MISMATCH"
                    echo "    Keypair:  ${keypair_program_id} (${keypair_file})"
                    echo "    Code:     ${code_program_id}"
                    mismatch_details="${mismatch_details}\n${program} Program ID:"
                    mismatch_details="${mismatch_details}\n  Keypair: ${keypair_program_id}"
                    mismatch_details="${mismatch_details}\n  In Code: ${code_program_id}"
                    all_match=false
                else
                    print_success "matches"
                fi
            fi
            
            # 3. Deployed programs are upgraded by program ID with the upgrade authority;
            #    only a first deploy needs the program keypair
            local state=$(get_program_state "${code_program_id}" "${CLUSTER}" "${DEPLOY_ADMIN_KEYPAIR}")
            echo -n "  On-chain: "
            case "${state}" in
                deployed\ *)
                    local onchain_authority="${state#deployed }"
                    if [ "${onchain_authority}" = "${ADMIN_AUTHORITY_PUBKEY}" ]; then
                        print_success "deployed, upgrade authority matches (upgrade by program ID)"
                    else
                        print_error "deployed, but upgrade authority is ${onchain_authority:-none (immutable)}"
                        mismatch_details="${mismatch_details}\n${program}: on-chain upgrade authority ${onchain_authority:-none} is not ${ADMIN_AUTHORITY_PUBKEY}"
                        all_match=false
                    fi
                    ;;
                absent)
                    if [ -f "${keypair_file}" ]; then
                        print_success "not deployed yet (first deploy with ${keypair_file})"
                    elif [ -f "${keypair_file}.gpg" ]; then
                        print_error "not deployed yet, and its program keypair is locked"
                        mismatch_details="${mismatch_details}\n${program}: first deploy needs the program keypair (./scripts/keys.sh unlock ${ENV})"
                        all_match=false
                    else
                        print_error "not deployed yet, and no program keypair at ${keypair_file}"
                        mismatch_details="${mismatch_details}\n${program}: first deploy needs ${keypair_file} (run ./scripts/setup-keypairs.sh ${ENV})"
                        all_match=false
                    fi
                    ;;
                *)
                    print_error "could not query ${CLUSTER}: ${state#error }"
                    mismatch_details="${mismatch_details}\n${program}: could not query on-chain state"
                    all_match=false
                    ;;
            esac
        fi
        
        # 5. Verify mint authority (all programs have this)
        local code_mint_authority=$(extract_authority_from_code "${program}" "MINT" "${ENV}")
        echo -n "  Mint Authority: "
        if [ -z "${code_mint_authority}" ]; then
            print_warning "Could not extract from code"
            echo "    Keypair: ${MINT_AUTHORITY_PUBKEY}"
            echo "    Code: Unable to parse"
            mismatch_details="${mismatch_details}\n${program}: Could not verify AUTHORIZED_MINT_PUBKEY in code"
            all_match=false
        elif [ "${MINT_AUTHORITY_PUBKEY}" != "${code_mint_authority}" ]; then
            print_error "MISMATCH"
            echo "    Keypair:  ${MINT_AUTHORITY_PUBKEY}"
            echo "    Code:     ${code_mint_authority}"
            mismatch_details="${mismatch_details}\n${program} AUTHORIZED_MINT_PUBKEY:"
            mismatch_details="${mismatch_details}\n  Expected: ${MINT_AUTHORITY_PUBKEY}"
            mismatch_details="${mismatch_details}\n  In Code:  ${code_mint_authority}"
            all_match=false
        else
            print_success "${MINT_AUTHORITY_PUBKEY}"
        fi
        
        # 6. Verify admin authority (only for memo_mint, memo_burn, memo_chat, memo_project, and memo_forum)
        # Note: memo_blog no longer requires admin authority (global counter removed)
        if [ "${program}" = "memo_mint" ] || [ "${program}" = "memo_burn" ] || [ "${program}" = "memo_chat" ] || [ "${program}" = "memo_project" ] || [ "${program}" = "memo_forum" ]; then
            local code_admin_authority=$(extract_authority_from_code "${program}" "ADMIN" "${ENV}")
            echo -n "  Admin Authority: "
            if [ -z "${code_admin_authority}" ]; then
                print_warning "Could not extract from code"
                echo "    Keypair: ${ADMIN_AUTHORITY_PUBKEY}"
                echo "    Code: Unable to parse"
                mismatch_details="${mismatch_details}\n${program}: Could not verify AUTHORIZED_ADMIN_PUBKEY in code"
                all_match=false
            elif [ "${ADMIN_AUTHORITY_PUBKEY}" != "${code_admin_authority}" ]; then
                print_error "MISMATCH"
                echo "    Keypair:  ${ADMIN_AUTHORITY_PUBKEY}"
                echo "    Code:     ${code_admin_authority}"
                mismatch_details="${mismatch_details}\n${program} AUTHORIZED_ADMIN_PUBKEY:"
                mismatch_details="${mismatch_details}\n  Expected: ${ADMIN_AUTHORITY_PUBKEY}"
                mismatch_details="${mismatch_details}\n  In Code:  ${code_admin_authority}"
                all_match=false
            else
                print_success "${ADMIN_AUTHORITY_PUBKEY}"
            fi
        fi
        
        echo ""
    done
    
    # Final verdict
    if [ "$all_match" = "false" ]; then
        echo "=========================================="
        print_error "VERIFICATION FAILED"
        echo "=========================================="
        echo ""
        echo "Mismatches found:"
        echo -e "${mismatch_details}"
        echo ""
        print_error "Deployment aborted: Configuration does not match keypairs"
        echo ""
        echo "To fix:"
        echo "  1. Review the mismatches above"
        echo "  2. Update your source code to match the keypair pubkeys:"
        echo "     - programs/*/src/lib.rs (declare_id! and AUTHORIZED_*_PUBKEY)"
        echo "     - Anchor.toml (programs.${ENV} section)"
        echo "  3. Commit the changes to git"
        echo "  4. Re-run this deployment script"
        echo ""
        exit 1
    else
        echo "=========================================="
        print_success "VERIFICATION PASSED"
        echo "=========================================="
        echo ""
        print_success "All program IDs and authority pubkeys match!"
        echo ""
    fi
}

# Main deployment function
deploy_to_env() {
    local ENV=$1
    local CLUSTER=$2
    local FEATURE_FLAG=$3
    shift 3
    local SELECTED_PROGRAMS=("$@")
    
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
        # Deploy in canonical order (memo_burn before the apps that CPI into it)
        SELECTED_PROGRAMS=()
        for canonical in "${ALL_PROGRAMS[@]}"; do
            for prog in "${VALIDATED_PROGRAMS[@]}"; do
                if [ "$prog" = "$canonical" ]; then
                    SELECTED_PROGRAMS+=("$canonical")
                    break
                fi
            done
        done
        print_info "Deploying selected programs: ${SELECTED_PROGRAMS[*]}"
    fi
    
    echo ""
    echo "=========================================="
    echo "🚀 Deploying to $(to_upper "${ENV}")"
    echo "=========================================="
    echo ""
    
    # Change to project root
    cd "${PROJECT_ROOT}"
    
    # Check git status
    if [ -n "$(git status --porcelain)" ]; then
        print_warning "You have uncommitted changes."
        read -p "Continue anyway? (yes/no): " continue_dirty
        if [ "$continue_dirty" != "yes" ]; then
            print_info "Deployment cancelled."
            exit 0
        fi
    fi
    
    # Confirm deployment
    echo ""
    print_warning "Ready to deploy to $(to_upper "${ENV}")"
    echo "  Cluster: ${CLUSTER}"
    echo "  Programs: ${SELECTED_PROGRAMS[*]}"
    echo ""
    read -p "Proceed with deployment? (yes/no): " confirm
    if [ "$confirm" != "yes" ]; then
        print_info "Deployment cancelled."
        exit 0
    fi
    
    # Step 1: Verify configuration
    echo ""
    print_info "Step 1: Verifying configuration..."
    verify_configuration "${ENV}" "${SELECTED_PROGRAMS[@]}"
    
    # Step 2: Build
    echo ""
    print_info "Step 2: Building programs..."
    # Drop links from the old keypair layout: cargo build-sbf would write a new keypair through
    # a dangling link. A missing keypair is fine: build-sbf generates an unused random one, and
    # first deploys pass the stored program keypair explicitly.
    for keypair_link in "${TARGET_DEPLOY_DIR}"/*-keypair.json; do
        if [ -L "${keypair_link}" ]; then
            rm "${keypair_link}"
        fi
    done
    if [ -n "${FEATURE_FLAG}" ]; then
        print_info "Building with feature: ${FEATURE_FLAG}"
        for program in "${SELECTED_PROGRAMS[@]}"; do
            local program_dash=$(get_program_name_dash "${program}")
            print_info "Building ${program_dash}..."
            anchor build -p "${program_dash}" -- --features "${FEATURE_FLAG}"
        done
    else
        for program in "${SELECTED_PROGRAMS[@]}"; do
            local program_dash=$(get_program_name_dash "${program}")
            print_info "Building ${program_dash}..."
            anchor build -p "${program_dash}"
        done
    fi
    
    print_success "Build complete"
    
    # Step 3: Deploy
    echo ""
    print_info "Step 3: Deploying to ${CLUSTER}..."
    
    export ANCHOR_PROVIDER_URL="${CLUSTER}"
    # Every anchor command signs with this environment's deploy admin, whatever Anchor.toml's wallet is
    DEPLOY_WALLET="$(authority_keypair_path deploy_admin "${ENV}")"
    # Admin clients read their keypair from this environment's key tree
    export MEMO_PROGRAM_ENV="${ENV}"
    
    for program in "${SELECTED_PROGRAMS[@]}"; do
        echo ""
        print_info "Deploying ${program}..."
        
        local program_id=$(extract_program_id_from_code "${program}" "${ENV}")
        local state=$(get_program_state "${program_id}" "${CLUSTER}" "${DEPLOY_WALLET}")
        
        case "${state}" in
            deployed\ *)
                # Upgrade by program ID: needs only the upgrade authority (the Anchor.toml wallet)
                anchor upgrade "${TARGET_DEPLOY_DIR}/${program}.so" \
                    --program-id "${program_id}" \
                    --provider.cluster "${CLUSTER}" \
                    --provider.wallet "${DEPLOY_WALLET}"
                print_success "${program} upgraded: ${program_id}"
                
                # Keep the on-chain IDL in sync (create it if it does not exist yet)
                local idl_file="${PROJECT_ROOT}/target/idl/${program}.json"
                if sync_idl "${program_id}" "${idl_file}" "${CLUSTER}"; then
                    print_success "${program} IDL updated"
                else
                    print_warning "${program} IDL update failed; the program is upgraded. Retry:"
                    echo "   anchor idl upgrade ${program_id} --filepath target/idl/${program}.json --provider.cluster ${CLUSTER} --provider.wallet ${DEPLOY_WALLET}"
                fi
                ;;
            absent)
                # First deploy: the program keypair creates the program at its address (uploads the IDL too)
                anchor deploy \
                    --provider.cluster "${CLUSTER}" \
                    --provider.wallet "${DEPLOY_WALLET}" \
                    --program-name "${program}" \
                    --program-keypair "$(program_keypair_path "${program}" "${ENV}")"
                print_success "${program} deployed: ${program_id}"
                ;;
            *)
                print_error "Could not query ${program} on ${CLUSTER}: ${state#error }"
                exit 1
                ;;
        esac

        # memo-mint requires the mint state account (one mint per transaction); every mint fails until it exists.
        # The client is idempotent: it does nothing if the account is already initialized.
        if [ "${program}" = "memo_mint" ]; then
            print_info "Initializing memo-mint mint state..."
            if ! cargo run --quiet --manifest-path "${PROJECT_ROOT}/clients/Cargo.toml" \
                    --bin admin-init-mint-state -- "${program_id}" "${CLUSTER}"; then
                print_error "Mint state initialization failed. Mints will fail until it succeeds."
                print_info "Retry: MEMO_PROGRAM_ENV=${ENV} cargo run --manifest-path clients/Cargo.toml --bin admin-init-mint-state -- ${program_id} ${CLUSTER}"
                exit 1
            fi
            print_success "Mint state ready"
        fi

        # memo-burn requires the global burn stats account; every burn fails until it exists.
        # The client is idempotent: it does nothing if the account is already initialized.
        if [ "${program}" = "memo_burn" ]; then
            print_info "Initializing memo-burn global burn stats..."
            if ! cargo run --quiet --manifest-path "${PROJECT_ROOT}/clients/Cargo.toml" \
                    --bin admin-init-global-burn-stats -- "${program_id}" "${CLUSTER}"; then
                print_error "Global burn stats initialization failed. Burns will fail until it succeeds."
                print_info "Retry: MEMO_PROGRAM_ENV=${ENV} cargo run --manifest-path clients/Cargo.toml --bin admin-init-global-burn-stats -- ${program_id} ${CLUSTER}"
                exit 1
            fi
            print_success "Global burn stats ready"
        fi
        
        # Determine explorer URL based on actual deployment cluster (not ENV)
        if [[ "${CLUSTER}" =~ "mainnet" ]]; then
            echo "   Explorer: https://explorer.mainnet.x1.xyz/address/${program_id}"
        else
            echo "   Explorer: https://explorer.testnet.x1.xyz/address/${program_id}"
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
        local program_id=$(extract_program_id_from_code "${program}" "${ENV}")
        echo "  ${program} = \"${program_id}\""
    done
    echo ""
    
    if [ "${ENV}" = "mainnet" ]; then
        echo ""
        print_warning "POST-DEPLOYMENT SECURITY CHECKLIST:"
        echo "  ☐ Verify contracts on blockchain explorer"
        echo "  ☐ Test all critical functions"
        echo "  ☐ Consider transferring upgrade authority to multisig"
        echo "  ☐ Backup all keypairs to cold storage"
        echo "  ☐ Document this deployment"
        echo ""
    fi
}