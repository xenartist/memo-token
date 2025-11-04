#!/usr/bin/env bash
#
# Smoke Test Runner for Memo Token Project
#
# Usage:
#   ./scripts/run-smoke-tests.sh                      # Run all smoke tests
#   ./scripts/run-smoke-tests.sh memo-mint            # Run single smoke test
#   ./scripts/run-smoke-tests.sh memo-mint memo-burn  # Run multiple smoke tests
#   ./scripts/run-smoke-tests.sh --help               # Show help

set -e

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color
BOLD='\033[1m'

# Available smoke tests (these should match the [[bin]] names in Cargo.toml)
AVAILABLE_SMOKE_TESTS=(
    "memo-mint"
)

# Function to normalize test name (convert underscore to hyphen)
normalize_test_name() {
    echo "$1" | tr '_' '-'
}

# Function to check if a smoke test exists
test_exists() {
    local test="$1"
    for available in "${AVAILABLE_SMOKE_TESTS[@]}"; do
        if [ "$available" = "$test" ]; then
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
${BOLD}Smoke Test Runner for Memo Token Project${NC}

${BOLD}USAGE:${NC}
    ./scripts/run-smoke-tests.sh [OPTIONS] [TESTS...]

${BOLD}OPTIONS:${NC}
    --help, -h          Show this help message
    --list, -l          List all available smoke tests
    --verbose, -v       Show verbose output

${BOLD}TESTS:${NC}
    memo-mint           Run memo-mint smoke test
    memo_mint           Alternative naming (underscore)

${BOLD}EXAMPLES:${NC}
    # Run all smoke tests
    ./scripts/run-smoke-tests.sh

    # Run single smoke test
    ./scripts/run-smoke-tests.sh memo-mint

    # Run multiple smoke tests
    ./scripts/run-smoke-tests.sh memo-mint memo-burn

    # List available tests
    ./scripts/run-smoke-tests.sh --list

${BOLD}ABOUT SMOKE TESTS:${NC}
    Smoke tests are quick, single-execution tests that validate core
    functionality across different environments.
    
    Characteristics:
      - Single transaction execution (no loops)
      - Exact compute unit calculation (no buffer)
      - Environment-agnostic configuration
      - Fast execution (< 10 seconds per test)
      - Basic functionality validation

${BOLD}ENVIRONMENT DETECTION:${NC}
    Environment is determined by cluster + program_env combination:
    
    testnet:      cluster=testnet  + program_env=testnet
                  (Development: testnet cluster, testnet programs)
    
    prod-staging: cluster=testnet  + program_env=mainnet
                  (Pre-production: testnet cluster, mainnet programs)
    
    mainnet:      cluster=mainnet  + program_env=mainnet
                  (Production: mainnet cluster, mainnet programs)
    
    Configuration is read from Anchor.toml [provider] section

${BOLD}AVAILABLE TESTS:${NC}
EOF
    for test in "${AVAILABLE_SMOKE_TESTS[@]}"; do
        echo "    - $test"
    done
    echo ""
}

# Function to list available smoke tests
list_tests() {
    print_header "Available Smoke Tests"
    for test in "${AVAILABLE_SMOKE_TESTS[@]}"; do
        local bin_name="smoke-test-${test}"
        local bin_path="clients/smoke-test/src/smoke-test-${test}.rs"
        
        if [ -f "$bin_path" ]; then
            echo -e "  ${GREEN}✓${NC} $test ${CYAN}($bin_name)${NC}"
        else
            echo -e "  ${RED}✗${NC} $test ${RED}(not found: $bin_path)${NC}"
        fi
    done
    echo ""
}

# Function to run a smoke test
run_smoke_test() {
    local test=$1
    local test_normalized=$(normalize_test_name "$test")
    
    # Check if test exists
    if ! test_exists "$test_normalized"; then
        print_message "$RED" "❌ Error: Smoke test '$test' not found"
        echo "   Available tests: ${AVAILABLE_SMOKE_TESTS[*]}"
        return 1
    fi
    
    print_header "Running Smoke Test: $test_normalized"
    
    local bin_name="smoke-test-${test_normalized}"
    local bin_path="clients/smoke-test/src/smoke-test-${test_normalized}.rs"
    
    # Check if test file exists
    if [ ! -f "$bin_path" ]; then
        print_message "$YELLOW" "⚠️  Warning: Test file not found: $bin_path"
        return 1
    fi
    
    # Show command being run
    if [ "$VERBOSE" = true ]; then
        print_message "$CYAN" "Running: cargo run --bin $bin_name"
        echo ""
    fi
    
    # Run the smoke test
    if cargo run --bin "$bin_name"; then
        print_message "$GREEN" "✅ $test_normalized smoke test passed"
        return 0
    else
        print_message "$RED" "❌ $test_normalized smoke test failed"
        return 1
    fi
}

# Parse command line arguments
VERBOSE=false
TESTS_TO_RUN=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            show_help
            exit 0
            ;;
        --list|-l)
            list_tests
            exit 0
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --*)
            print_message "$RED" "Unknown option: $1"
            echo "Use --help to see available options"
            exit 1
            ;;
        *)
            TESTS_TO_RUN+=("$1")
            shift
            ;;
    esac
done

# Change to project root
cd "$(dirname "$0")/.."

# Print banner
print_message "$BOLD" "🔬 Memo Token Smoke Test Runner"
echo ""

# Get current environment configuration from Anchor.toml
PROGRAM_ENV=$(grep 'program_env' Anchor.toml | awk -F'"' '{print $2}')
RPC_URL=$(grep -A 5 '\[provider\]' Anchor.toml | grep 'cluster' | awk -F'"' '{print $2}')

# Detect cluster type from RPC URL
CLUSTER_TYPE="unknown"
if [[ "$RPC_URL" == *"testnet"* ]] || [[ "$RPC_URL" == *"test"* ]]; then
    CLUSTER_TYPE="testnet"
elif [[ "$RPC_URL" == *"devnet"* ]]; then
    CLUSTER_TYPE="devnet"
elif [[ "$RPC_URL" == *"localhost"* ]] || [[ "$RPC_URL" == *"127.0.0.1"* ]]; then
    CLUSTER_TYPE="localnet"
elif [[ "$RPC_URL" == *"mainnet"* ]]; then
    CLUSTER_TYPE="mainnet"
fi

# Determine environment based on cluster + program_env combination
# Logic:
#   cluster: testnet  + program_env: testnet  = testnet (development)
#   cluster: testnet  + program_env: mainnet  = prod-staging (pre-production testing)
#   cluster: mainnet  + program_env: mainnet  = mainnet (production)
if [ "$CLUSTER_TYPE" = "testnet" ] && [ "$PROGRAM_ENV" = "testnet" ]; then
    ENV_NAME="testnet"
    ENV_DESC="Development (testnet cluster, testnet programs)"
elif [ "$CLUSTER_TYPE" = "testnet" ] && [ "$PROGRAM_ENV" = "mainnet" ]; then
    ENV_NAME="prod-staging"
    ENV_DESC="Production Staging (testnet cluster, mainnet programs)"
elif [ "$CLUSTER_TYPE" = "mainnet" ] && [ "$PROGRAM_ENV" = "mainnet" ]; then
    ENV_NAME="mainnet"
    ENV_DESC="Production (mainnet cluster, mainnet programs)"
elif [ "$CLUSTER_TYPE" = "devnet" ]; then
    ENV_NAME="devnet"
    ENV_DESC="Devnet (devnet cluster)"
elif [ "$CLUSTER_TYPE" = "localnet" ]; then
    ENV_NAME="localnet"
    ENV_DESC="Local (localhost cluster)"
else
    ENV_NAME="unknown"
    ENV_DESC="Unknown configuration (cluster: $CLUSTER_TYPE, program_env: $PROGRAM_ENV)"
fi

print_message "$BLUE" "Environment: $ENV_NAME"
print_message "$BLUE" "Description: $ENV_DESC"
print_message "$BLUE" "RPC Cluster: $CLUSTER_TYPE"
print_message "$BLUE" "RPC URL: $RPC_URL"
print_message "$BLUE" "Program Env: $PROGRAM_ENV"
echo ""

# If no tests specified, run all
if [ ${#TESTS_TO_RUN[@]} -eq 0 ]; then
    print_message "$BLUE" "No tests specified. Running all smoke tests..."
    TESTS_TO_RUN=("${AVAILABLE_SMOKE_TESTS[@]}")
fi

# Run smoke tests
FAILED_TESTS=()
PASSED_TESTS=()
SKIPPED_TESTS=()

for test in "${TESTS_TO_RUN[@]}"; do
    if run_smoke_test "$test"; then
        PASSED_TESTS+=("$test")
    else
        local test_normalized=$(normalize_test_name "$test")
        local bin_path="clients/smoke-test/src/smoke-test-${test_normalized}.rs"
        
        if [ ! -f "$bin_path" ]; then
            SKIPPED_TESTS+=("$test")
        else
            FAILED_TESTS+=("$test")
        fi
    fi
    echo ""
done

# Print summary
print_header "Smoke Test Summary"

if [ ${#PASSED_TESTS[@]} -gt 0 ]; then
    print_message "$GREEN" "✅ Passed (${#PASSED_TESTS[@]}):"
    for test in "${PASSED_TESTS[@]}"; do
        echo "   - $test"
    done
    echo ""
fi

if [ ${#SKIPPED_TESTS[@]} -gt 0 ]; then
    print_message "$YELLOW" "⚠️  Skipped (${#SKIPPED_TESTS[@]}):"
    for test in "${SKIPPED_TESTS[@]}"; do
        echo "   - $test (test file not found)"
    done
    echo ""
fi

if [ ${#FAILED_TESTS[@]} -gt 0 ]; then
    print_message "$RED" "❌ Failed (${#FAILED_TESTS[@]}):"
    for test in "${FAILED_TESTS[@]}"; do
        echo "   - $test"
    done
    echo ""
    
    print_message "$RED" "Some smoke tests failed. Please check the output above."
    exit 1
fi

# All tests passed
if [ ${#PASSED_TESTS[@]} -gt 0 ]; then
    print_message "$GREEN" "🎉 All smoke tests passed!"
    echo ""
    print_message "$CYAN" "Environment validated: $ENV_NAME"
else
    print_message "$YELLOW" "⚠️  No smoke tests were run"
fi

exit 0

