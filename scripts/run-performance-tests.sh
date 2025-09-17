#!/bin/bash

# DBGIF Performance Testing Script
# Run comprehensive performance tests for DBGIF server

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default values
SERVER_ADDR="127.0.0.1:5555"
DEVICE_PORT="5557"
TEST_TYPE="all"
VERBOSE=""
SKIP_BUILD=""

# Print usage
usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -s, --server ADDR    DBGIF server address (default: $SERVER_ADDR)"
    echo "  -d, --device PORT    Device server port (default: $DEVICE_PORT)"
    echo "  -t, --test TYPE      Test type: all, throughput, latency, connections, benchmark"
    echo "  -v, --verbose        Enable verbose logging"
    echo "  -n, --no-build       Skip building the test binary"
    echo "  -h, --help           Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                           # Run all performance tests"
    echo "  $0 -t throughput             # Run only throughput test"
    echo "  $0 -t benchmark -v           # Run benchmark suite with verbose output"
    echo "  $0 -s 192.168.1.100:5555    # Test against remote server"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -s|--server)
            SERVER_ADDR="$2"
            shift 2
            ;;
        -d|--device)
            DEVICE_PORT="$2"
            shift 2
            ;;
        -t|--test)
            TEST_TYPE="$2"
            shift 2
            ;;
        -v|--verbose)
            VERBOSE="--verbose"
            shift
            ;;
        -n|--no-build)
            SKIP_BUILD="1"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            usage
            exit 1
            ;;
    esac
done

# Function to print colored headers
print_header() {
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════${NC}"
}

# Function to check if server is running
check_server() {
    echo -e "${YELLOW}Checking if DBGIF server is running at $SERVER_ADDR...${NC}"

    if nc -z -w 2 ${SERVER_ADDR%:*} ${SERVER_ADDR#*:} 2>/dev/null; then
        echo -e "${GREEN}✓ Server is reachable${NC}"
        return 0
    else
        echo -e "${RED}✗ Server is not reachable at $SERVER_ADDR${NC}"
        echo -e "${YELLOW}Starting DBGIF server...${NC}"

        # Try to start the server
        cargo run --bin dbgif-server -- --port ${SERVER_ADDR#*:} &
        SERVER_PID=$!

        # Wait for server to start
        sleep 2

        if nc -z -w 2 ${SERVER_ADDR%:*} ${SERVER_ADDR#*:} 2>/dev/null; then
            echo -e "${GREEN}✓ Server started (PID: $SERVER_PID)${NC}"
            return 0
        else
            echo -e "${RED}✗ Failed to start server${NC}"
            return 1
        fi
    fi
}

# Function to cleanup on exit
cleanup() {
    if [ ! -z "$SERVER_PID" ]; then
        echo -e "${YELLOW}Stopping server (PID: $SERVER_PID)...${NC}"
        kill $SERVER_PID 2>/dev/null || true
    fi

    if [ ! -z "$DEVICE_PID" ]; then
        echo -e "${YELLOW}Stopping device server (PID: $DEVICE_PID)...${NC}"
        kill $DEVICE_PID 2>/dev/null || true
    fi
}

# Set trap for cleanup
trap cleanup EXIT

# Build if needed
if [ -z "$SKIP_BUILD" ]; then
    print_header "Building Performance Test Binary"
    cargo build --package dbgif-integrated-test --example performance_test --release
    echo -e "${GREEN}✓ Build completed${NC}"
fi

# Check server
print_header "Server Status"
check_server

# Run tests based on type
PERF_BIN="./target/release/examples/performance_test"

case $TEST_TYPE in
    all)
        print_header "Running All Performance Tests"
        $PERF_BIN --server $SERVER_ADDR $VERBOSE all
        ;;

    throughput)
        print_header "Running Throughput Test"
        $PERF_BIN --server $SERVER_ADDR $VERBOSE throughput --data-size 8192 --duration 15
        ;;

    latency)
        print_header "Running Latency Test"
        $PERF_BIN --server $SERVER_ADDR $VERBOSE latency --iterations 200
        ;;

    connections)
        print_header "Running Connection Limit Test"
        $PERF_BIN --server $SERVER_ADDR $VERBOSE connections --max 50
        ;;

    benchmark)
        print_header "Running Benchmark Suite"
        $PERF_BIN --server $SERVER_ADDR $VERBOSE benchmark
        ;;

    *)
        echo -e "${RED}Unknown test type: $TEST_TYPE${NC}"
        echo "Valid types: all, throughput, latency, connections, benchmark"
        exit 1
        ;;
esac

# Print summary
print_header "Test Complete"
echo -e "${GREEN}✓ All tests finished successfully${NC}"
echo ""
echo "To run specific tests:"
echo "  $0 -t throughput    # Test data transfer speed"
echo "  $0 -t latency       # Test round-trip time"
echo "  $0 -t connections   # Test concurrent connections"
echo "  $0 -t benchmark     # Run standardized benchmark"