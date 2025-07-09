#!/bin/bash
# Script to test RPC connectivity through Tor

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "🧅 Testing RPC connectivity through Tor..."

# Check if torsocks is installed
if ! command -v torsocks &> /dev/null; then
    echo -e "${YELLOW}Warning: torsocks not installed${NC}"
    echo "Install it to test through Tor network:"
    echo "  brew install torsocks  # macOS"
    echo "  sudo apt install torsocks  # Debian/Ubuntu"
    echo ""
fi

# Check if hostname file exists
HOSTNAME_FILE="data/tor/torpc/hostname"
if [ ! -f "$HOSTNAME_FILE" ]; then
    echo -e "${RED}Error: Tor hostname not found${NC}"
    echo "Make sure Tor is running: ./scripts/start-tor.sh"
    exit 1
fi

ONION_URL=$(cat "$HOSTNAME_FILE")
echo "Testing onion address: $ONION_URL"

# Test 1: Direct local connection
echo -e "\n${YELLOW}Test 1: Direct local connection${NC}"
curl -s -X POST http://127.0.0.1:8080/rpc \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
    | jq . || echo -e "${RED}Failed${NC}"

# Test 2: Through Tor (if torsocks is available)
if command -v torsocks &> /dev/null; then
    echo -e "\n${YELLOW}Test 2: Connection through Tor${NC}"
    torsocks curl -s -X POST "http://$ONION_URL/rpc" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
        | jq . || echo -e "${RED}Failed${NC}"
    
    echo -e "\n${YELLOW}Test 3: Blocked method through Tor${NC}"
    torsocks curl -s -X POST "http://$ONION_URL/rpc" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_accounts","params":[],"id":1}' \
        | jq . || echo -e "${RED}Failed${NC}"
else
    echo -e "\n${YELLOW}Skipping Tor tests (torsocks not installed)${NC}"
fi

# Test with Python if available (alternative to torsocks)
if command -v python3 &> /dev/null && ! command -v torsocks &> /dev/null; then
    echo -e "\n${YELLOW}Testing with Python SOCKS support${NC}"
    cat << 'EOF' > /tmp/test_tor_rpc.py
import json
import requests

# You need: pip install requests[socks]
try:
    import socks
    
    proxies = {
        'http': 'socks5h://127.0.0.1:9050',
        'https': 'socks5h://127.0.0.1:9050'
    }
    
    with open('data/tor/torpc/hostname', 'r') as f:
        onion_url = f.read().strip()
    
    response = requests.post(
        f'http://{onion_url}/rpc',
        json={"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1},
        proxies=proxies,
        timeout=30
    )
    
    print(json.dumps(response.json(), indent=2))
    
except ImportError:
    print("Install requests with SOCKS support: pip install requests[socks]")
except Exception as e:
    print(f"Error: {e}")
EOF
    
    python3 /tmp/test_tor_rpc.py
    rm -f /tmp/test_tor_rpc.py
fi

echo -e "\n${GREEN}Testing complete!${NC}"