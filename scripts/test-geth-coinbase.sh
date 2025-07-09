#!/bin/bash
# Quick test to verify Geth coinbase account is accessible

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

RPC_URL="${RPC_URL:-http://127.0.0.1:8545}"

echo "Testing Geth dev mode coinbase account..."
echo "RPC URL: $RPC_URL"

# Get coinbase
echo -e "\n${YELLOW}Getting coinbase account...${NC}"
COINBASE=$(cast rpc eth_coinbase --rpc-url $RPC_URL 2>/dev/null | tr -d '"')
if [ $? -ne 0 ] || [ -z "$COINBASE" ]; then
    echo -e "${RED}Error: Cannot get coinbase account. Is Geth running?${NC}"
    exit 1
fi
echo "Coinbase: $COINBASE"

# Check balance
echo -e "\n${YELLOW}Checking coinbase balance...${NC}"
BALANCE=$(cast balance $COINBASE --rpc-url $RPC_URL 2>/dev/null)
if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Cannot get balance${NC}"
    exit 1
fi
BALANCE_ETH=$(cast to-unit $BALANCE ether)
echo "Balance: $BALANCE_ETH ETH"

# Test sending transaction from coinbase
echo -e "\n${YELLOW}Testing transaction from coinbase...${NC}"
TEST_ADDR="0x742d35Cc6634C0532925a3b844Bc9e7595f8888"
TX=$(cast send --from $COINBASE --rpc-url $RPC_URL $TEST_ADDR --value 0.1ether 2>&1 | grep -E '0x[a-fA-F0-9]{64}' | head -1 | awk '{print $1}')
if [ $? -eq 0 ] && [ -n "$TX" ]; then
    echo -e "${GREEN}✓ Transaction sent successfully${NC}"
    echo "Transaction hash: $TX"
else
    echo -e "${RED}✗ Failed to send transaction${NC}"
    exit 1
fi

echo -e "\n${GREEN}✅ Coinbase account is working correctly!${NC}"