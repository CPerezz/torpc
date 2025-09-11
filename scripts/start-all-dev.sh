#!/bin/bash
# Script to start all TorPC services in development mode (uses Geth --dev)

# Parse command line arguments
DEBUG_MODE=false
VERBOSE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--debug)
            DEBUG_MODE=true
            shift
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "OPTIONS:"
            echo "  -d, --debug     Enable debug mode with verbose logging"
            echo "  -v, --verbose   Enable verbose output"
            echo "  -h, --help      Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                Start services normally"
            echo "  $0 --debug       Start with debug logging"
            echo "  $0 --verbose     Start with verbose output"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Debug function
debug_log() {
    if [ "$DEBUG_MODE" = true ] || [ "$VERBOSE" = true ]; then
        echo -e "${BLUE}[DEBUG]${NC} $1"
    fi
}

# Verbose function
verbose_log() {
    if [ "$VERBOSE" = true ]; then
        echo -e "${YELLOW}[VERBOSE]${NC} $1"
    fi
}

echo -e "${BLUE}🚀 Starting TorPC Services (Development Mode)...${NC}"
if [ "$DEBUG_MODE" = true ]; then
    echo -e "${BLUE}[DEBUG MODE ENABLED]${NC}"
fi
if [ "$VERBOSE" = true ]; then
    echo -e "${YELLOW}[VERBOSE MODE ENABLED]${NC}"
fi
echo "=============================="

debug_log "Checking current directory for Cargo.toml"
# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    echo -e "${RED}Error: Not in TorPC directory. Please run from project root.${NC}"
    debug_log "Current directory: $(pwd)"
    debug_log "Directory contents: $(ls -la)"
    exit 1
fi
debug_log "✓ Found Cargo.toml in $(pwd)"

# Function to check if a process is running
is_running() {
    pgrep -f "$1" > /dev/null
}

# Function to wait for a service to be ready
wait_for_service() {
    local service_name=$1
    local check_command=$2
    local max_attempts=30
    local attempt=1
    
    printf "  Waiting for $service_name to be ready (this may take up to 1 minute)"
    while [ $attempt -le $max_attempts ]; do
        if eval "$check_command" &> /dev/null; then
            echo -e " ${GREEN}✓${NC}"
            return 0
        fi
        printf "."
        sleep 1
        ((attempt++))
    done
    echo -e " ${RED}✗${NC}"
    return 1
}

# Start Geth
echo -e "\n${YELLOW}1. Starting Geth development node...${NC}"
debug_log "Checking if Geth is already running with pattern 'geth.*--dev'"
if is_running "geth.*--dev"; then
    echo -e "  ${GREEN}✓ Geth is already running${NC}"
    debug_log "Found existing Geth process: $(pgrep -f 'geth.*--dev')"
else
    echo "  Starting Geth..."
    verbose_log "Executing ./scripts/start-geth-dev.sh in background"
    
    if [ "$DEBUG_MODE" = true ]; then
        # In debug mode, show Geth output briefly
        debug_log "Starting Geth with debug output..."
        # Note: timeout command not available on macOS by default
        ./scripts/start-geth-dev.sh &
    else
        ./scripts/start-geth-dev.sh &
    fi
    
    GETH_PID=$!
    debug_log "Geth process started with PID: $GETH_PID"
    
    # Wait for Geth to be ready
    debug_log "Waiting for Geth RPC to become available..."
    wait_for_service "Geth" "curl -s -X POST -H 'Content-Type: application/json' --data '{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"method\\\":\\\"eth_blockNumber\\\",\\\"params\\\":[],\\\"id\\\":1}' http://127.0.0.1:8545"
    
    if [ $? -eq 0 ]; then
        echo -e "  ${GREEN}✓ Geth started successfully (PID: $GETH_PID)${NC}"
        debug_log "Geth RPC is responding on port 8545"
        
        # Give Geth a moment to fully initialize the developer account
        echo "  Waiting for Geth to initialize developer account..."
        sleep 3
    else
        echo -e "  ${RED}✗ Failed to start Geth${NC}"
        debug_log "Geth startup failed or timed out"
        if [ "$DEBUG_MODE" = true ]; then
            debug_log "Last 10 lines of Geth log:"
            tail -10 data/geth-dev/geth.log 2>/dev/null || debug_log "No Geth log found"
        fi
        exit 1
    fi
fi

# Generate test data if needed
echo -e "\n${YELLOW}2. Checking blockchain data...${NC}"
BLOCK_NUMBER=$(curl -s -X POST -H "Content-Type: application/json" --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' http://127.0.0.1:8545 | grep -o '"result":"[^"]*"' | cut -d'"' -f4 | xargs printf "%d\n")

debug_log "Current block number: $BLOCK_NUMBER"
if [ "$BLOCK_NUMBER" -lt 50 ]; then
    echo "  Block number is $BLOCK_NUMBER, generating test data..."
    verbose_log "Executing generate-test-data.sh script"
    
    # Pass debug/verbose flags to the generate-test-data script
    if [ "$DEBUG_MODE" = true ]; then
        debug_log "Running generate-test-data.sh in debug mode"
        ./scripts/generate-test-data.sh --debug
    elif [ "$VERBOSE" = true ]; then
        debug_log "Running generate-test-data.sh in verbose mode"  
        ./scripts/generate-test-data.sh --verbose
    else
        ./scripts/generate-test-data.sh
    fi
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}✗ Failed to generate test data${NC}"
        debug_log "generate-test-data.sh exited with code $?"
        exit 1
    fi
else
    echo -e "  ${GREEN}✓ Blockchain has $BLOCK_NUMBER blocks${NC}"
    debug_log "Sufficient blockchain data already exists"
fi

# Start Tor
echo -e "\n${YELLOW}3. Starting Tor hidden service...${NC}"
if is_running "tor -f configs/torrc"; then
    echo -e "  ${GREEN}✓ Tor is already running${NC}"
else
    echo "  Starting Tor..."
    mkdir -p data/tor/torpc
    chmod 700 data/tor/torpc
    
    # Start Tor in the background
    nohup tor -f configs/torrc > data/tor/tor.stdout.log 2>&1 &
    TOR_PID=$!
    
    # Wait for Tor to create the hostname file
    echo -n "  Waiting for Tor to generate .onion address"
    ATTEMPTS=0
    while [ ! -f "data/tor/torpc/hostname" ] && [ $ATTEMPTS -lt 60 ]; do
        printf "."
        sleep 1
        ((ATTEMPTS++))
    done
    
    if [ -f "data/tor/torpc/hostname" ]; then
        echo -e " ${GREEN}✓${NC}"
        ONION_ADDRESS=$(cat data/tor/torpc/hostname)
        echo -e "  ${GREEN}✓ Tor started successfully (PID: $TOR_PID)${NC}"
        echo -e "  ${BLUE}🧅 Onion address: $ONION_ADDRESS${NC}"
    else
        echo -e " ${RED}✗${NC}"
        echo -e "  ${RED}✗ Failed to start Tor${NC}"
        echo "  Check logs: tail -f data/tor/tor.log"
        exit 1
    fi
fi

# Build TorPC if needed
echo -e "\n${YELLOW}4. Building TorPC...${NC}"
if [ ! -f "target/release/torpc" ] || [ "src/main.rs" -nt "target/release/torpc" ]; then
    echo "  Building TorPC in release mode..."
    cargo build --release
    if [ $? -eq 0 ]; then
        echo -e "  ${GREEN}✓ Build successful${NC}"
    else
        echo -e "  ${RED}✗ Build failed${NC}"
        exit 1
    fi
else
    echo -e "  ${GREEN}✓ TorPC is already built${NC}"
fi

# Start TorPC
echo -e "\n${YELLOW}5. Starting TorPC proxy...${NC}"
if is_running "target/release/torpc"; then
    echo -e "  ${GREEN}✓ TorPC is already running${NC}"
else
    echo "  Starting TorPC..."
    # Ensure log directory exists
    mkdir -p data
    # For development, use local Geth as flashbots endpoint to avoid authentication issues
    RUST_LOG=info FLASHBOTS_URL="http://127.0.0.1:8545" nohup ./target/release/torpc > data/torpc.log 2>&1 &
    TORPC_PID=$!
    
    # Wait for TorPC to be ready
    wait_for_service "TorPC" "curl -s http://127.0.0.1:8080/"
    
    if [ $? -eq 0 ]; then
        echo -e "  ${GREEN}✓ TorPC started successfully (PID: $TORPC_PID)${NC}"
    else
        echo -e "  ${RED}✗ Failed to start TorPC${NC}"
        echo "  Check logs: tail -f data/torpc.log"
        exit 1
    fi
fi

# Summary
echo -e "\n=============================="
echo -e "${GREEN}✅ All services are running!${NC}"
echo ""
echo -e "${BLUE}📍 Access Points:${NC}"
echo -e "  Local web interface: ${YELLOW}http://localhost:8080${NC}"
echo -e "  Local RPC endpoint:  ${YELLOW}http://localhost:8080/rpc${NC}"

if [ -f "data/tor/torpc/hostname" ]; then
    ONION_ADDRESS=$(cat data/tor/torpc/hostname)
    echo -e "  Tor web interface:   ${YELLOW}http://$ONION_ADDRESS${NC}"
    echo -e "  Tor RPC endpoint:    ${YELLOW}http://$ONION_ADDRESS/rpc${NC}"
fi

echo ""
echo -e "${BLUE}📝 Logs:${NC}"
echo -e "  Geth:  ${YELLOW}tail -f data/geth-dev/geth.log${NC}"
echo -e "  Tor:   ${YELLOW}tail -f data/tor/tor.log${NC}"
echo -e "  TorPC: ${YELLOW}tail -f data/torpc.log${NC}"

echo ""
echo -e "${BLUE}🛑 To stop all services:${NC}"
echo -e "  ${YELLOW}./scripts/stop-all.sh${NC} (graceful shutdown)"
echo -e "  ${YELLOW}./scripts/stop-all.sh --force${NC} (immediate termination)"
echo -e "  ${YELLOW}./scripts/stop-all.sh --timeout 60${NC} (custom timeout)"

# Create PID file for easy cleanup
echo "GETH_PID=${GETH_PID:-$(pgrep -f "geth.*--dev")}" > .torpc.pids
echo "TOR_PID=${TOR_PID:-$(pgrep -f "tor -f configs/torrc")}" >> .torpc.pids
echo "TORPC_PID=${TORPC_PID:-$(pgrep -f "target/release/torpc")}" >> .torpc.pids

echo ""