#!/bin/bash
# Start Tor with our configuration

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}🧅 Starting Tor hidden service...${NC}"

# Check if Tor is already running
if pgrep -f "tor -f configs/torrc" > /dev/null; then
    echo -e "${GREEN}✓ Tor is already running${NC}"
    
    if [ -f "data/tor/torpc/hostname" ]; then
        ONION_ADDRESS=$(cat data/tor/torpc/hostname)
        echo -e "Onion address: ${YELLOW}$ONION_ADDRESS${NC}"
    fi
    exit 0
fi

# Create directories if needed
mkdir -p data/tor/torpc
chmod 700 data/tor/torpc

# Start Tor
echo "Starting Tor daemon..."
tor -f configs/torrc &
TOR_PID=$!

# Wait for hostname file
echo -n "Waiting for .onion address generation"
ATTEMPTS=0
while [ ! -f "data/tor/torpc/hostname" ] && [ $ATTEMPTS -lt 60 ]; do
    echo -n "."
    sleep 1
    ((ATTEMPTS++))
done

if [ -f "data/tor/torpc/hostname" ]; then
    echo -e " ${GREEN}✓${NC}"
    ONION_ADDRESS=$(cat data/tor/torpc/hostname)
    echo -e "\n${GREEN}✓ Tor hidden service started successfully!${NC}"
    echo -e "PID: $TOR_PID"
    echo -e "Onion address: ${YELLOW}$ONION_ADDRESS${NC}"
    echo -e "\nThe service forwards:"
    echo -e "  http://$ONION_ADDRESS → http://127.0.0.1:8080"
    echo -e "\nTo test:"
    echo -e "  torsocks curl http://$ONION_ADDRESS/"
else
    echo -e " ${RED}✗${NC}"
    echo -e "${RED}Failed to start Tor hidden service${NC}"
    echo "Check logs: tail -f data/tor/tor.log"
    exit 1
fi