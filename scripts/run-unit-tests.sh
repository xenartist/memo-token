#!/usr/bin/env bash
#
# Unit Test Runner for Memo Token Project
#
# Usage:
#   ./scripts/run-unit-tests.sh                    # Run all contracts' unit tests
#   ./scripts/run-unit-tests.sh memo-mint          # Run single contract
#   ./scripts/run-unit-tests.sh memo_mint          # Alternative naming
#   ./scripts/run-unit-tests.sh memo-mint memo-chat # Run multiple contracts
#   ./scripts/run-unit-tests.sh --verbose memo-mint # Run with verbose output
#   ./scripts/run-unit-tests.sh --nocapture memo-mint # Show println! output
#   ./scripts/run-unit-tests.sh --help             # Show help

set -e

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color
BOLD='\033[1m'

# Available contracts in the project
AVAILABLE_CONTRACTS=(
    "memo-burn"
    "memo-chat"
    "memo-mint"
    "memo-profile"
    "memo-project"
    "memo-blog"
)

# Test options
VERBOSE=false
NOCAPTURE=false
TEST_THREADS=0
EXTRA_ARGS=""

# Function to normalize contract name (convert underscore to hyphen)
normalize_contract_name() {
    echo "$1" | tr '_' '-'
}

# Function to check if a contract exists
contract_exists() {
    local contract="$1"
    for available in "${AVAILABLE_CONTRACTS[@]}"; do
        if [ "$available" = "$contract" ]; then
            return 0
        fi
    done
    return 1
}

# Function to print colored message
print_message() {
    local color=$1
    local message=$2
    echo -e "${color}${message}${NC}"
}

# Function to print section header
print_header() {
    local message=$1
    echo ""
    echo -e "${CYAN}${BOLD}========================================${NC}"
    echo -e "${CYAN}${BOLD}  $message${NC}"
    echo -e "${CYAN}${BOLD}========================================${NC}"
    echo ""
}

# Function to show help
show_help() {
    cat << EOF
${BOLD}Unit Test Runner for Memo Token Project${NC}

${BOLD}USAGE:${NC}
    ./scripts/run-unit-tests.sh [OPTIONS] [CONTRACTS...]

${BOLD}OPTIONS:${NC}
    --help              Show this help message
    --verbose, -v       Show verbose test output
    --nocapture         Show println! and stdout output from tests
    --test-threads N    Number of test threads (0 = all cores, 1 = sequential)
    --list, -l          List all available contracts

${BOLD}CONTRACTS:${NC}
    memo-mint           Run tests for memo-mint contract
    memo_mint           Alternative naming (underscore)
    memo-chat           Run tests for memo-chat contract
    memo-burn           Run tests for memo-burn contract
    memo-profile        Run tests for memo-profile contract
    memo-project        Run tests for memo-project contract
    memo-blog           Run tests for memo-blog contract

${BOLD}EXAMPLES:${NC}
    # Run all contracts' unit tests
    ./scripts/run-unit-tests.sh

    # Run single contract
    ./scripts/run-unit-tests.sh memo-mint

    # Run multiple contracts
    ./scripts/run-unit-tests.sh memo-mint memo-chat memo-burn

    # Run with verbose output
    ./scripts/run-unit-tests.sh --verbose memo-mint

    # Run with stdout capture disabled (see println! output)
    ./scripts/run-unit-tests.sh --nocapture memo-mint

    # Run tests sequentially
    ./scripts/run-unit-tests.sh --test-threads 1 memo-mint

    # List available contracts
    ./scripts/run-unit-tests.sh --list

${BOLD}AVAILABLE CONTRACTS:${NC}
EOF
    for contract in "${AVAILABLE_CONTRACTS[@]}"; do
        echo "    - $contract"
    done
    echo ""
}

# Function to list available contracts
list_contracts() {
    print_header "Available Contracts"
    for contract in "${AVAILABLE_CONTRACTS[@]}"; do
        if [ -d "programs/$contract" ]; then
            if [ -f "programs/$contract/src/tests.rs" ] || [ -d "programs/$contract/tests" ]; then
                echo -e "  ${GREEN}✓${NC} $contract ${CYAN}(has tests)${NC}"
            else
                echo -e "  ${YELLOW}○${NC} $contract ${YELLOW}(no test files found)${NC}"
            fi
        else
            echo -e "  ${RED}✗${NC} $contract ${RED}(not found)${NC}"
        fi
    done
    echo ""
}

# Function to run tests for a specific contract
run_contract_tests() {
    local contract=$1
    local contract_normalized=$(normalize_contract_name "$contract")
    
    # Check if contract exists
    if ! contract_exists "$contract_normalized"; then
        print_message "$RED" "❌ Error: Contract '$contract' not found"
        echo "   Available contracts: ${AVAILABLE_CONTRACTS[*]}"
        return 1
    fi
    
    print_header "Testing: $contract_normalized"
    
    # Check if tests exist
    if [ ! -f "programs/$contract_normalized/src/tests.rs" ] && [ ! -d "programs/$contract_normalized/tests" ]; then
        print_message "$YELLOW" "⚠️  Warning: No test files found for $contract_normalized"
        echo "   Expected: programs/$contract_normalized/src/tests.rs or programs/$contract_normalized/tests/"
        return 0
    fi
    
    # Build test command
    local cmd="cargo test -p $contract_normalized --lib"
    
    # Add extra arguments
    if [ -n "$EXTRA_ARGS" ]; then
        cmd="$cmd -- $EXTRA_ARGS"
    fi
    
    # Show command being run
    if [ "$VERBOSE" = true ]; then
        print_message "$CYAN" "Running: $cmd"
        echo ""
    fi
    
    # Run the tests
    if eval "$cmd"; then
        print_message "$GREEN" "✅ $contract_normalized tests passed"
        return 0
    else
        print_message "$RED" "❌ $contract_normalized tests failed"
        return 1
    fi
}

# Parse command line arguments
CONTRACTS_TO_TEST=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            show_help
            exit 0
            ;;
        --list|-l)
            list_contracts
            exit 0
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --nocapture)
            NOCAPTURE=true
            EXTRA_ARGS="$EXTRA_ARGS --nocapture"
            shift
            ;;
        --test-threads)
            TEST_THREADS="$2"
            EXTRA_ARGS="$EXTRA_ARGS --test-threads $TEST_THREADS"
            shift 2
            ;;
        --*)
            print_message "$RED" "Unknown option: $1"
            echo "Use --help to see available options"
            exit 1
            ;;
        *)
            CONTRACTS_TO_TEST+=("$1")
            shift
            ;;
    esac
done

# Change to project root
cd "$(dirname "$0")/.."

# Print banner
print_message "$BOLD" "🧪 Memo Token Unit Test Runner"
echo ""

# If no contracts specified, run all
if [ ${#CONTRACTS_TO_TEST[@]} -eq 0 ]; then
    print_message "$BLUE" "No contracts specified. Running all contracts..."
    CONTRACTS_TO_TEST=("${AVAILABLE_CONTRACTS[@]}")
fi

# Run tests for each contract
FAILED_CONTRACTS=()
PASSED_CONTRACTS=()
SKIPPED_CONTRACTS=()

for contract in "${CONTRACTS_TO_TEST[@]}"; do
    if run_contract_tests "$contract"; then
        PASSED_CONTRACTS+=("$contract")
    else
        # Check if it was a skip or fail
        if [ ! -f "programs/$(normalize_contract_name "$contract")/src/tests.rs" ] && \
           [ ! -d "programs/$(normalize_contract_name "$contract")/tests" ]; then
            SKIPPED_CONTRACTS+=("$contract")
        else
            FAILED_CONTRACTS+=("$contract")
        fi
    fi
    echo ""
done

# Print summary
print_header "Test Summary"

if [ ${#PASSED_CONTRACTS[@]} -gt 0 ]; then
    print_message "$GREEN" "✅ Passed (${#PASSED_CONTRACTS[@]}):"
    for contract in "${PASSED_CONTRACTS[@]}"; do
        echo "   - $contract"
    done
    echo ""
fi

if [ ${#SKIPPED_CONTRACTS[@]} -gt 0 ]; then
    print_message "$YELLOW" "⚠️  Skipped (${#SKIPPED_CONTRACTS[@]}):"
    for contract in "${SKIPPED_CONTRACTS[@]}"; do
        echo "   - $contract (no tests found)"
    done
    echo ""
fi

if [ ${#FAILED_CONTRACTS[@]} -gt 0 ]; then
    print_message "$RED" "❌ Failed (${#FAILED_CONTRACTS[@]}):"
    for contract in "${FAILED_CONTRACTS[@]}"; do
        echo "   - $contract"
    done
    echo ""
    exit 1
fi

# All tests passed
if [ ${#PASSED_CONTRACTS[@]} -gt 0 ]; then
    print_message "$GREEN" "🎉 All tests passed!"
else
    print_message "$YELLOW" "⚠️  No tests were run"
fi

exit 0

