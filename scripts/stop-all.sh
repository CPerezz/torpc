#!/bin/bash
# Script to stop all TorPC services

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
DEFAULT_TIMEOUT=30
FORCE_MODE=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --force)
            FORCE_MODE=true
            shift
            ;;
        --timeout)
            DEFAULT_TIMEOUT="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [--force] [--timeout SECONDS]"
            echo "  --force     Use SIGKILL immediately instead of graceful shutdown"
            echo "  --timeout   Wait time in seconds for graceful shutdown (default: 30)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

echo -e "${BLUE}🛑 Stopping TorPC Services...${NC}"
echo "=============================="

# Track overall success
OVERALL_SUCCESS=true

# Enhanced function to terminate a process with proper waiting
terminate_process_with_wait() {
    local pid=$1
    local name=$2
    local timeout=${3:-$DEFAULT_TIMEOUT}
    
    if ! kill -0 "$pid" 2>/dev/null; then
        return 0  # Already dead
    fi
    
    if [ "$FORCE_MODE" = true ]; then
        echo -n "  Force killing $name ($pid)..."
        kill -9 "$pid"
        sleep 2
        if ! kill -0 "$pid" 2>/dev/null; then
            echo -e " ${GREEN}✓${NC}"
            return 0
        else
            echo -e " ${RED}✗${NC}"
            return 1
        fi
    fi
    
    # Send SIGTERM for graceful shutdown
    echo -n "  Stopping $name ($pid)..."
    kill "$pid" 2>/dev/null
    
    # Wait for graceful termination
    for i in $(seq 1 $timeout); do
        if ! kill -0 "$pid" 2>/dev/null; then
            echo -e " ${GREEN}✓${NC} (graceful)"
            return 0
        fi
        if [ $((i % 5)) -eq 0 ]; then
            echo -n "."
        fi
        sleep 1
    done
    
    # Force kill if still running
    echo -n " timeout, force killing..."
    kill -9 "$pid" 2>/dev/null
    sleep 2
    
    if ! kill -0 "$pid" 2>/dev/null; then
        echo -e " ${GREEN}✓${NC} (forced)"
        return 0
    else
        echo -e " ${RED}✗${NC} (failed)"
        return 1
    fi
}

# Enhanced function to stop services by pattern
stop_service_by_pattern() {
    local service_name=$1
    local process_pattern=$2
    local success=true
    
    # Find all matching processes
    local pids=($(pgrep -f "$process_pattern"))
    
    if [ ${#pids[@]} -eq 0 ]; then
        echo -e "  ${YELLOW}$service_name not running${NC}"
        return 0
    fi
    
    echo -e "  Found ${#pids[@]} $service_name process(es)"
    
    # Terminate each process
    for pid in "${pids[@]}"; do
        if ! terminate_process_with_wait "$pid" "$service_name"; then
            success=false
            OVERALL_SUCCESS=false
        fi
    done
    
    return $([ "$success" = true ] && echo 0 || echo 1)
}

# Function to stop a service (handles both PID and pattern)
stop_service() {
    local service_name=$1
    local pid=$2
    local process_pattern=$3
    
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        # Use specific PID if available and valid
        if ! terminate_process_with_wait "$pid" "$service_name"; then
            OVERALL_SUCCESS=false
        fi
    else
        # Fall back to pattern matching
        stop_service_by_pattern "$service_name" "$process_pattern"
    fi
}

# Read PIDs from file if it exists
if [ -f ".torpc.pids" ]; then
    source .torpc.pids
fi

# Stop TorPC
echo -e "\n${YELLOW}1. Stopping TorPC proxy...${NC}"
stop_service "TorPC" "$TORPC_PID" "target/release/torpc"

# Stop Tor
echo -e "\n${YELLOW}2. Stopping Tor...${NC}"
stop_service "Tor" "$TOR_PID" "tor -f configs/torrc"

# Stop Geth
echo -e "\n${YELLOW}3. Stopping Geth...${NC}"
stop_service "Geth" "$GETH_PID" "geth.*--dev"

# Clean up any leftover Geth processes
echo -e "\n${YELLOW}3.1 Cleaning up Geth child processes...${NC}"
sleep 2  # Give Geth time to clean up its children
LEFTOVER_GETH_PIDS=$(pidof geth 2>/dev/null || true)
if [ -n "$LEFTOVER_GETH_PIDS" ]; then
    echo "  Found leftover Geth processes: $LEFTOVER_GETH_PIDS"
    for pid in $LEFTOVER_GETH_PIDS; do
        echo -n "  Force killing Geth child process $pid..."
        kill -9 $pid 2>/dev/null && echo -e " ${GREEN}✓${NC}" || echo -e " ${RED}✗${NC}"
    done
else
    echo -e "  ${GREEN}✓ No leftover Geth processes found${NC}"
fi

# Clean up PID file
rm -f .torpc.pids

# Verify ports are freed (especially for Geth)
verify_ports_freed() {
    local ports_in_use=false
    
    echo -e "\n${YELLOW}4. Verifying ports are freed...${NC}"
    
    if command -v lsof >/dev/null 2>&1; then
        # Check Geth ports
        if lsof -ti:8545 >/dev/null 2>&1; then
            echo -e "  ${RED}⚠ Port 8545 still in use${NC}"
            ports_in_use=true
        fi
        if lsof -ti:8546 >/dev/null 2>&1; then
            echo -e "  ${RED}⚠ Port 8546 still in use${NC}"
            ports_in_use=true
        fi
        
        if [ "$ports_in_use" = false ]; then
            echo -e "  ${GREEN}✓ All ports freed${NC}"
        fi
    else
        echo -e "  ${YELLOW}lsof not available, skipping port check${NC}"
    fi
    
    return $([ "$ports_in_use" = true ] && echo 1 || echo 0)
}

# Final verification of all processes
final_process_check() {
    echo -e "\n${YELLOW}5. Final process verification...${NC}"
    local remaining_processes=false
    
    # Check for any remaining TorPC processes
    local torpc_pids=($(pgrep -f "target/release/torpc"))
    if [ ${#torpc_pids[@]} -gt 0 ]; then
        echo -e "  ${RED}⚠ TorPC still running (PIDs: ${torpc_pids[*]})${NC}"
        remaining_processes=true
    fi
    
    # Check for any remaining Tor processes
    local tor_pids=($(pgrep -f "tor -f configs/torrc"))
    if [ ${#tor_pids[@]} -gt 0 ]; then
        echo -e "  ${RED}⚠ Tor still running (PIDs: ${tor_pids[*]})${NC}"
        remaining_processes=true
    fi
    
    # Check for any remaining Geth processes
    local geth_pids=($(pgrep -f "geth.*--dev"))
    if [ ${#geth_pids[@]} -gt 0 ]; then
        echo -e "  ${RED}⚠ Geth still running (PIDs: ${geth_pids[*]})${NC}"
        remaining_processes=true
    fi
    
    if [ "$remaining_processes" = false ]; then
        echo -e "  ${GREEN}✓ All processes terminated${NC}"
        return 0
    else
        echo -e "  ${RED}✗ Some processes still running${NC}"
        echo -e "  ${YELLOW}Tip: Use '$0 --force' for immediate termination${NC}"
        OVERALL_SUCCESS=false
        return 1
    fi
}

# Run verifications
verify_ports_freed
final_process_check

echo ""
echo "=============================="
if [ "$OVERALL_SUCCESS" = true ]; then
    echo -e "${GREEN}✅ Shutdown complete - All services stopped${NC}"
    EXIT_CODE=0
else
    echo -e "${RED}❌ Shutdown incomplete - Some services still running${NC}"
    EXIT_CODE=1
fi
echo ""

# Ask about log cleanup only if shutdown was successful
if [ "$OVERALL_SUCCESS" = true ]; then
    read -p "Do you want to clean up log files? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo "Cleaning up logs..."
        rm -f data/torpc.log
        rm -f data/tor/tor.log
        rm -f data/tor/tor.stdout.log
        echo -e "${GREEN}✓ Logs cleaned${NC}"
    fi
else
    echo -e "${YELLOW}Skipping log cleanup due to incomplete shutdown${NC}"
    echo -e "${YELLOW}You may want to check logs for debugging:${NC}"
    echo -e "  ${YELLOW}tail -f data/geth-dev/geth.log${NC}"
    echo -e "  ${YELLOW}tail -f data/tor/tor.log${NC}"
    echo -e "  ${YELLOW}tail -f data/torpc.log${NC}"
fi

echo ""

# Exit with appropriate code
exit $EXIT_CODE