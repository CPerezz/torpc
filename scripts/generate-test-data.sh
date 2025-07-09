#!/bin/bash
# Script to generate test blockchain data using Foundry cast

# Parse command line arguments
DEBUG_MODE=false
VERBOSE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--debug)
            DEBUG_MODE=true
            VERBOSE=true
            shift
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        --rpc-url)
            RPC_URL="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "OPTIONS:"
            echo "  -d, --debug     Enable debug mode with verbose logging"
            echo "  -v, --verbose   Enable verbose output"
            echo "  --rpc-url URL   RPC endpoint URL (default: http://127.0.0.1:8545)"
            echo "  -h, --help      Show this help message"
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

# Default RPC URL
RPC_URL="${RPC_URL:-http://127.0.0.1:8545}"

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

echo "📊 Generating test blockchain data..."
if [ "$DEBUG_MODE" = true ]; then
    echo -e "${BLUE}[DEBUG MODE ENABLED]${NC}"
fi
if [ "$VERBOSE" = true ]; then
    echo -e "${YELLOW}[VERBOSE MODE ENABLED]${NC}"
fi
echo "RPC URL: $RPC_URL"

debug_log "Checking for required tools..."
# Check if cast is installed
if ! command -v cast &> /dev/null; then
    echo -e "${RED}Error: Foundry cast is not installed${NC}"
    echo "Install it from: https://book.getfoundry.sh/getting-started/installation"
    debug_log "PATH: $PATH"
    debug_log "Available commands: $(compgen -c | grep -E '^(forge|cast|anvil)' || echo 'None found')"
    exit 1
fi
debug_log "✓ Found cast: $(which cast)"
debug_log "Cast version: $(cast --version)"

# Check if bc is installed for number comparisons
if ! command -v bc &> /dev/null; then
    echo -e "${RED}Error: bc is not installed${NC}"
    echo "Please install bc for number comparisons"
    echo "  Ubuntu/Debian: sudo apt-get install bc"
    echo "  macOS: brew install bc"
    echo "  Fedora: sudo dnf install bc"
    exit 1
fi

# Setting up test accounts
echo -e "\n${YELLOW}Setting up test accounts...${NC}"

# Get the developer account from Geth
echo "Getting Geth developer account..."

# Method 1: Try eth_coinbase (might not be available)
COINBASE=$(cast rpc eth_coinbase --rpc-url $RPC_URL 2>/dev/null | tr -d '"')

# Method 2: If eth_coinbase fails, try eth_accounts (should return the dev account)
if [ -z "$COINBASE" ] || [ "$COINBASE" = "null" ]; then
    debug_log "eth_coinbase not available, trying eth_accounts..."
    ACCOUNTS_RESPONSE=$(cast rpc eth_accounts --rpc-url $RPC_URL 2>/dev/null)
    # Extract first account from the array response
    COINBASE=$(echo "$ACCOUNTS_RESPONSE" | grep -oE '0x[a-fA-F0-9]{40}' | head -1)
fi

# Method 3: If still no account, extract from Geth logs
if [ -z "$COINBASE" ] || [ "$COINBASE" = "null" ]; then
    debug_log "RPC methods failed, extracting from Geth logs..."
    if [ -f "data/geth-dev/geth.log" ]; then
        COINBASE=$(grep "Using developer account" data/geth-dev/geth.log | tail -1 | grep -oE '0x[a-fA-F0-9]{40}')
    fi
fi

if [ -z "$COINBASE" ]; then
    echo -e "${RED}Error: Cannot determine Geth developer account${NC}"
    echo "Please check that Geth is running in dev mode"
    exit 1
fi

echo "Geth developer account: $COINBASE"

# Define the Anvil/Hardhat account that we'll use for everything
PK_MAIN="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
ADDR_MAIN=$(cast wallet address $PK_MAIN)

# Force mine a block to ensure the developer account is funded
echo "Ensuring developer account is funded..."
# Try to mine a block using evm_mine
MINE_RESPONSE=$(curl -s -X POST -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"evm_mine","params":[],"id":1}' \
    $RPC_URL)
debug_log "Mine response: $MINE_RESPONSE"

# Wait a moment for the block to be processed
sleep 2

# Additional test accounts (these will receive funds from the main account)
PK2="0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
PK3="0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"

ADDR2=$(cast wallet address $PK2)
ADDR3=$(cast wallet address $PK3)

echo "Geth Developer Account (coinbase): $COINBASE"
echo "Main Account (Anvil/Hardhat): $ADDR_MAIN" 
echo "Test Account 2: $ADDR2"
echo "Test Account 3: $ADDR3"

# First, wait for the Geth developer account to be funded
echo "Waiting for Geth developer account to be funded..."
MIN_BALANCE_ETH="100"
MAX_WAIT=30
WAIT_COUNT=0

while [ $WAIT_COUNT -lt $MAX_WAIT ]; do
    # Check balance of the COINBASE account (not ADDR_MAIN yet)
    DEV_BALANCE=$(cast balance $COINBASE --rpc-url $RPC_URL 2>/dev/null)
    if [ -z "$DEV_BALANCE" ]; then
        DEV_BALANCE="0"
    fi
    
    # Handle potentially large balances by dividing in bc
    DEV_BALANCE_ETH=$(echo "scale=6; $DEV_BALANCE / 1000000000000000000" | bc 2>/dev/null || echo "0")
    BALANCE_ETH_NUM=$(echo "$DEV_BALANCE_ETH" | sed 's/[^0-9.]//g')
    
    # Ensure we have a valid number
    if [ -z "$BALANCE_ETH_NUM" ]; then
        BALANCE_ETH_NUM="0"
    fi
    
    # Check if we have enough balance
    if [ $(echo "$BALANCE_ETH_NUM >= $MIN_BALANCE_ETH" | bc -l) -eq 1 ]; then
        echo -e "${GREEN}✓ Geth developer account funded: $DEV_BALANCE_ETH ETH${NC}"
        break
    fi
    
    # If not enough, try to trigger mining
    if [ $((WAIT_COUNT % 5)) -eq 0 ]; then
        echo "Current balance: $DEV_BALANCE_ETH ETH - triggering block mining..."
        # Send a dummy transaction to trigger mining in dev mode
        curl -s -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_sendTransaction\",\"params\":[{\"from\":\"$COINBASE\",\"to\":\"$COINBASE\",\"value\":\"0x0\"}],\"id\":1}" \
            $RPC_URL >/dev/null 2>&1
        
        # Also try evm_mine
        curl -s -X POST -H "Content-Type: application/json" \
            --data '{"jsonrpc":"2.0","method":"evm_mine","params":[],"id":1}' \
            $RPC_URL >/dev/null 2>&1
    else
        echo -n "."
    fi
    
    sleep 1
    WAIT_COUNT=$((WAIT_COUNT + 1))
done

# Final check
if [ $(echo "$BALANCE_ETH_NUM < $MIN_BALANCE_ETH" | bc -l) -eq 1 ]; then
    echo -e "\n${RED}Error: Geth developer account not funded after ${MAX_WAIT} seconds${NC}"
    echo "Final balance: $DEV_BALANCE_ETH ETH"
    echo "This may happen if Geth is not in dev mode or is misconfigured"
    exit 1
fi

# Now transfer all funds from Geth developer account to Anvil account
echo -e "\n${YELLOW}Transferring funds from Geth developer account to Anvil account...${NC}"

# Get the exact balance in wei
DEV_BALANCE_WEI=$(cast balance $COINBASE --rpc-url $RPC_URL)

# Check if this is a very large balance (common in dev mode)
# If balance is greater than 10^30 wei (1 trillion ETH), just transfer 1000 ETH
LARGE_BALANCE_THRESHOLD="1000000000000000000000000000000" # 10^30 wei
if [ $(echo "$DEV_BALANCE_WEI > $LARGE_BALANCE_THRESHOLD" | bc) -eq 1 ]; then
    echo "Developer account has very large balance, transferring 1000 ETH..."
    # Transfer 1000 ETH instead of trying to transfer the maximum
    TRANSFER_AMOUNT="1000000000000000000000" # 1000 ETH in wei
else
    # Calculate amount to transfer (leave a small amount for gas)
    # Subtract 0.1 ETH for gas costs
    GAS_RESERVE="100000000000000000" # 0.1 ETH in wei
    
    # Use bc to handle large numbers and ensure we don't go negative
    TRANSFER_AMOUNT=$(echo "$DEV_BALANCE_WEI - $GAS_RESERVE" | bc)
    if [ $(echo "$TRANSFER_AMOUNT < 0" | bc) -eq 1 ]; then
        echo -e "${RED}Error: Developer account has less than 0.1 ETH${NC}"
        exit 1
    fi
fi

# Convert to hex using bc, which can handle arbitrarily large numbers
TRANSFER_AMOUNT_HEX=$(echo "obase=16; $TRANSFER_AMOUNT" | bc)
TRANSFER_AMOUNT_HEX="0x$TRANSFER_AMOUNT_HEX"

TRANSFER_ETH=$(echo "scale=6; $TRANSFER_AMOUNT / 1000000000000000000" | bc)
echo "Transferring $TRANSFER_ETH ETH to Anvil account..."

# Use eth_sendTransaction since the coinbase is unlocked
TRANSFER_TX_RESPONSE=$(curl -s -X POST -H "Content-Type: application/json" \
    --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_sendTransaction\",\"params\":[{\"from\":\"$COINBASE\",\"to\":\"$ADDR_MAIN\",\"value\":\"$TRANSFER_AMOUNT_HEX\"}],\"id\":1}" \
    $RPC_URL)

debug_log "Transfer response: $TRANSFER_TX_RESPONSE"

TRANSFER_TX=$(echo $TRANSFER_TX_RESPONSE | grep -o '"result":"[^"]*"' | cut -d'"' -f4)
if [ -z "$TRANSFER_TX" ] || [ "$TRANSFER_TX" = "null" ]; then
    echo -e "${RED}Error: Failed to transfer funds from Geth developer account${NC}"
    echo "Response: $TRANSFER_TX_RESPONSE"
    exit 1
fi

echo "Transfer transaction: $TRANSFER_TX"

# Helper function to wait for transaction confirmation
wait_for_tx() {
    local tx_hash=$1
    local timeout=${2:-30}
    
    if [ -z "$tx_hash" ] || [ "$tx_hash" = "null" ]; then
        echo -e " ${RED}✗ Invalid transaction hash${NC}"
        return 1
    fi
    
    echo -n "  Waiting for confirmation (timeout: ${timeout}s)"
    for i in $(seq 1 $timeout); do
        if cast receipt $tx_hash --rpc-url $RPC_URL &>/dev/null; then
            echo -e " ${GREEN}✓${NC}"
            return 0
        fi
        echo -n "."
        sleep 1
    done
    echo -e " ${RED}✗ Transaction timeout${NC}"
    echo "  Transaction hash: $tx_hash"
    return 1
}

# Wait for the transfer transaction to confirm
wait_for_tx "$TRANSFER_TX" 60

# Verify the Anvil account now has funds
MAIN_BALANCE=$(cast balance $ADDR_MAIN --rpc-url $RPC_URL)
MAIN_BALANCE_ETH=$(echo "scale=6; $MAIN_BALANCE / 1000000000000000000" | bc)
echo -e "${GREEN}✓ Main account (Anvil) balance: $MAIN_BALANCE_ETH ETH${NC}"

# Helper function to safely execute cast commands with retries
safe_cast_send() {
    local max_retries=3
    local retry_count=0
    local cmd="$*"
    
    while [ $retry_count -lt $max_retries ]; do
        local result=$(eval "$cmd" 2>&1)
        local exit_code=$?
        
        if [ $exit_code -eq 0 ] && [ -n "$result" ] && [ "$result" != "null" ]; then
            echo "$result"
            return 0
        fi
        
        retry_count=$((retry_count + 1))
        if [ $retry_count -lt $max_retries ]; then
            echo -e "  ${YELLOW}Retry $retry_count/$((max_retries-1))...${NC}" >&2
            sleep 2
        fi
    done
    
    echo -e "${RED}Error: Command failed after $max_retries attempts${NC}" >&2
    echo "Command: $cmd" >&2
    echo "Last output: $result" >&2
    return 1
}

# Fund test accounts from coinbase account
echo -e "\n${YELLOW}Funding test accounts...${NC}"

# Now we use the Anvil account with its private key for all operations
echo "Funding account 2..."

# Use cast send with private key
echo "  Sending 10 ETH to account 2..."
TX1=$(cast send --private-key $PK_MAIN --rpc-url $RPC_URL $ADDR2 --value 10ether --json 2>/dev/null | jq -r '.transactionHash // empty' || echo "")
if [ -z "$TX1" ]; then
    echo -e "${RED}Error: Failed to fund account 2${NC}"
    exit 1
fi
wait_for_tx "$TX1"

echo "Funding account 3..."

# Use cast send with private key
echo "  Sending 10 ETH to account 3..."
TX2=$(cast send --private-key $PK_MAIN --rpc-url $RPC_URL $ADDR3 --value 10ether --json 2>/dev/null | jq -r '.transactionHash // empty' || echo "")
if [ -z "$TX2" ]; then
    echo -e "${RED}Error: Failed to fund account 3${NC}"
    exit 1
fi
wait_for_tx "$TX2"

# Generate various transaction types
echo -e "\n${YELLOW}Generating ETH transfers...${NC}"
for i in {1..10}; do
    echo "Transfer batch $i..."
    # From main account to account 2
    cast send --private-key $PK_MAIN --rpc-url $RPC_URL $ADDR2 --value 0.1ether >/dev/null 2>&1 || true
    # From account 2 to account 3 (now has funds)
    cast send --private-key $PK2 --rpc-url $RPC_URL $ADDR3 --value 0.05ether >/dev/null 2>&1 || true
    # From account 3 back to main account
    cast send --private-key $PK3 --rpc-url $RPC_URL $ADDR_MAIN --value 0.02ether >/dev/null 2>&1 || true
done

# Deploy a simple storage contract
echo -e "\n${YELLOW}Deploying test contracts...${NC}"
# Simple storage contract bytecode (stores a number)
STORAGE_BYTECODE="0x608060405234801561001057600080fd5b50610150806100206000396000f3fe608060405234801561001057600080fd5b50600436106100365760003560e01c80632e64cec11461003b5780636057361d14610059575b600080fd5b610043610075565b60405161005091906100d9565b60405180910390f35b610073600480360381019061006e919061009d565b61007e565b005b60008054905090565b8060008190555050565b60008135905061009781610103565b92915050565b6000602082840312156100b3576100b26100fe565b5b60006100c184828501610088565b91505092915050565b6100d3816100f4565b82525050565b60006020820190506100ee60008301846100ca565b92915050565b6000819050919050565b600080fd5b61010c816100f4565b811461011757600080fd5b5056fea26469706673582212209c159de4d17f668719b1dd48a912e339b9e3f6c3e73e178c60e09e5c21a3ad3264736f6c63430008130033"

echo "Deploying SimpleStorage contract..."
DEPLOY_TX=$(cast send --private-key $PK_MAIN --rpc-url $RPC_URL --create $STORAGE_BYTECODE --json 2>/dev/null | jq -r '.transactionHash // empty' || echo "")
if [ -z "$DEPLOY_TX" ]; then
    echo -e "${RED}Error: Failed to deploy SimpleStorage contract${NC}"
    exit 1
fi
echo "Transaction hash: $DEPLOY_TX"
wait_for_tx "$DEPLOY_TX" 60  # Longer timeout for contract deployment

# Get contract address from receipt
RECEIPT=$(cast receipt $DEPLOY_TX --rpc-url $RPC_URL 2>&1)
debug_log "Receipt: $RECEIPT"

# Extract contract address - it's in the contractAddress field
STORAGE_ADDR=$(echo "$RECEIPT" | grep -i "contractAddress" | grep -oE '0x[a-fA-F0-9]{40}' | head -1)
if [ -z "$STORAGE_ADDR" ] || [ "$STORAGE_ADDR" = "null" ]; then
    echo -e "${RED}Error: Failed to get SimpleStorage contract address${NC}"
    echo "Receipt: $RECEIPT"
    exit 1
fi
echo "SimpleStorage deployed at: $STORAGE_ADDR"

# Interact with the contract (store and retrieve)
echo -e "\n${YELLOW}Interacting with contracts...${NC}"
# Store value 42
echo "Storing value 42..."
cast send --private-key $PK_MAIN --rpc-url $RPC_URL $STORAGE_ADDR "store(uint256)" 42 >/dev/null
# Store value 100
echo "Storing value 100..."
cast send --private-key $PK2 --rpc-url $RPC_URL $STORAGE_ADDR "store(uint256)" 100 >/dev/null

# Generate some failed transactions
echo -e "\n${YELLOW}Generating failed transactions...${NC}"
# Try to send more ETH than account 2 has (should fail)
cast send --private-key $PK2 --rpc-url $RPC_URL $ADDR3 --value 100ether 2>/dev/null || echo "Failed transaction (expected - insufficient funds)"

# Generate transactions with different gas prices
echo -e "\n${YELLOW}Generating transactions with various gas prices...${NC}"
for i in {1..5}; do
    GAS_PRICE=$((1000000000 * $i * 2)) # 2-10 gwei
    echo "Sending with gas price: $GAS_PRICE wei ($((i*2)) gwei)"
    cast send --private-key $PK_MAIN --rpc-url $RPC_URL $ADDR2 --value 0.01ether --gas-price $GAS_PRICE >/dev/null
done

# Deploy an ERC20 token
echo -e "\n${YELLOW}Deploying ERC20 token...${NC}"

# Check if we have compiled TestToken bytecode
if [ -f "dev/contracts/TestToken.bytecode" ]; then
    echo "Using compiled TestToken bytecode..."
    ERC20_BYTECODE=$(cat dev/contracts/TestToken.bytecode)
else
    echo "Compiling TestToken contract..."
    # Compile the contract if not already compiled
    (cd dev && ./compile-contracts.sh)
    if [ -f "dev/contracts/TestToken.bytecode" ]; then
        ERC20_BYTECODE=$(cat dev/contracts/TestToken.bytecode)
    else
        echo -e "${RED}Error: Failed to compile TestToken contract${NC}"
        exit 1
    fi
fi

echo "Deploying TestToken..."
TOKEN_DEPLOY_TX=$(cast send --private-key $PK_MAIN --rpc-url $RPC_URL --create $ERC20_BYTECODE --json 2>/dev/null | jq -r '.transactionHash // empty' || echo "")
if [ -z "$TOKEN_DEPLOY_TX" ]; then
    echo -e "${RED}Error: Failed to deploy TestToken contract${NC}"
    exit 1
fi
wait_for_tx "$TOKEN_DEPLOY_TX" 60

# Get contract address from receipt
TOKEN_RECEIPT=$(cast receipt $TOKEN_DEPLOY_TX --rpc-url $RPC_URL 2>&1)
debug_log "Token receipt: $TOKEN_RECEIPT"

# Extract contract address - it's in the contractAddress field
TOKEN_ADDR=$(echo "$TOKEN_RECEIPT" | grep -i "contractAddress" | grep -oE '0x[a-fA-F0-9]{40}' | head -1)
if [ -z "$TOKEN_ADDR" ] || [ "$TOKEN_ADDR" = "null" ]; then
    echo -e "${RED}Error: Failed to get TestToken contract address${NC}"
    echo "Receipt: $TOKEN_RECEIPT"
    exit 1
fi
echo "TestToken deployed at: $TOKEN_ADDR"

# Transfer some tokens (this simplified contract just tracks balances in storage)
echo -e "\n${YELLOW}Generating ERC20 transfers...${NC}"
# Give some "tokens" to test accounts (this is a simple balance tracking contract)
echo "Setting initial token balances..."
cast send --private-key $PK_MAIN --rpc-url $RPC_URL $TOKEN_ADDR "transfer(address,uint256)" $ADDR2 1000 >/dev/null
cast send --private-key $PK_MAIN --rpc-url $RPC_URL $TOKEN_ADDR "transfer(address,uint256)" $ADDR3 2000 >/dev/null

# Mine additional blocks to reach ~100 blocks
echo -e "\n${YELLOW}Mining additional blocks...${NC}"
CURRENT_BLOCK=$(cast block-number --rpc-url $RPC_URL)
TARGET_BLOCK=100
BLOCKS_TO_MINE=$((TARGET_BLOCK - CURRENT_BLOCK))

if [ $BLOCKS_TO_MINE -gt 0 ]; then
    echo "Current block: $CURRENT_BLOCK, mining $BLOCKS_TO_MINE more blocks..."
    for i in $(seq 1 $BLOCKS_TO_MINE); do
        # Send a dummy transaction to trigger block mining in dev mode
        cast send --private-key $PK_MAIN --rpc-url $RPC_URL $ADDR_MAIN --value 0.0001ether >/dev/null 2>&1
        if [ $((i % 10)) -eq 0 ]; then
            echo "Mined $i blocks..."
        fi
    done
fi

# Final statistics
echo -e "\n${GREEN}✅ Test data generation complete!${NC}"
FINAL_BLOCK=$(cast block-number --rpc-url $RPC_URL)
echo "Final block number: $FINAL_BLOCK"
echo "Total transactions generated: ~50+"

echo -e "\n${YELLOW}Test accounts:${NC}"
echo "Main Account (Geth coinbase): $ADDR_MAIN"
echo "Test Account 2: $ADDR2 (Private key: $PK2)"
echo "Test Account 3: $ADDR3 (Private key: $PK3)"
echo ""
echo "SimpleStorage contract: $STORAGE_ADDR"
echo "TestToken contract: $TOKEN_ADDR"

echo -e "\n${GREEN}You can now test the RPC proxy with these accounts and contracts!${NC}"