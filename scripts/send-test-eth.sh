#!/bin/bash
# Script to send ETH from the dev account to a specified address

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Target address (provided by user)
TO_ADDRESS="0x00fD23bC9Acdc9eCF848B29d98953B79B163a331"
AMOUNT="1"  # 1 ETH

# RPC URL (using the proxy)
RPC_URL="http://localhost:8080/rpc"

echo -e "${BLUE}💸 Sending $AMOUNT ETH to $TO_ADDRESS${NC}"
echo "=============================="

# Get the developer account
echo "Getting Geth developer account..."

# Try eth_accounts first
ACCOUNTS_RESPONSE=$(cast rpc eth_accounts --rpc-url $RPC_URL 2>/dev/null)
FROM_ADDRESS=$(echo "$ACCOUNTS_RESPONSE" | grep -oE '0x[a-fA-F0-9]{40}' | head -1)

if [ -z "$FROM_ADDRESS" ]; then
    # Try eth_coinbase
    FROM_ADDRESS=$(cast rpc eth_coinbase --rpc-url $RPC_URL 2>/dev/null | tr -d '"')
fi

if [ -z "$FROM_ADDRESS" ] || [ "$FROM_ADDRESS" = "null" ]; then
    echo -e "${RED}Error: Cannot determine developer account${NC}"
    echo "Make sure Geth is running in dev mode and the proxy is working"
    exit 1
fi

echo "From address (dev account): $FROM_ADDRESS"

# Check balance
echo -e "\n${YELLOW}Checking balance...${NC}"
BALANCE=$(cast balance $FROM_ADDRESS --rpc-url $RPC_URL)
BALANCE_ETH=$(cast to-unit $BALANCE ether)
echo "Current balance: $BALANCE_ETH ETH"

# Send the transaction
echo -e "\n${YELLOW}Sending transaction...${NC}"
echo "Amount: $AMOUNT ETH"
echo "To: $TO_ADDRESS"

# Use cast send to send the transaction
TX_HASH=$(cast send $TO_ADDRESS --value "${AMOUNT}ether" --from $FROM_ADDRESS --rpc-url $RPC_URL 2>&1)

if [ $? -eq 0 ]; then
    # Extract transaction hash from the output
    TX_HASH_CLEAN=$(echo "$TX_HASH" | grep -oE '0x[a-fA-F0-9]{64}' | head -1)
    
    if [ -n "$TX_HASH_CLEAN" ]; then
        echo -e "${GREEN}✓ Transaction sent successfully!${NC}"
        echo "Transaction hash: $TX_HASH_CLEAN"
        
        # Wait for confirmation
        echo -e "\n${YELLOW}Waiting for confirmation...${NC}"
        sleep 2
        
        # Get transaction receipt
        RECEIPT=$(cast receipt $TX_HASH_CLEAN --rpc-url $RPC_URL 2>/dev/null)
        
        if [ -n "$RECEIPT" ]; then
            echo -e "${GREEN}✓ Transaction confirmed!${NC}"
            
            # Check new balance of recipient
            NEW_BALANCE=$(cast balance $TO_ADDRESS --rpc-url $RPC_URL)
            NEW_BALANCE_ETH=$(cast to-unit $NEW_BALANCE ether)
            echo -e "\nRecipient's new balance: $NEW_BALANCE_ETH ETH"
        else
            echo "Transaction is pending..."
        fi
    else
        echo -e "${RED}Error: Could not extract transaction hash${NC}"
        echo "Output: $TX_HASH"
    fi
else
    echo -e "${RED}Error: Failed to send transaction${NC}"
    echo "Output: $TX_HASH"
    echo ""
    echo "Troubleshooting:"
    echo "1. Make sure Geth is running: ./scripts/start-geth-dev.sh"
    echo "2. Make sure the proxy is running: ./scripts/start-all-dev.sh"
    echo "3. Check that MetaMask is connected to: http://localhost:8080/rpc"
fi