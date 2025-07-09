#!/bin/bash
# Compile contracts using Foundry's forge

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

echo "Compiling TestToken.sol..."

# Compile the contract using forge
forge build

# Check if compilation was successful
if [ $? -ne 0 ]; then
    echo "Error: Contract compilation failed"
    exit 1
fi

# The compiled output is in out/TestToken.sol/TestToken.json
JSON_FILE="out/TestToken.sol/TestToken.json"

if [ ! -f "$JSON_FILE" ]; then
    echo "Error: Cannot find compiled TestToken.json at $JSON_FILE"
    exit 1
fi

echo "Found compiled contract at: $JSON_FILE"

# Extract bytecode
BYTECODE=$(cat "$JSON_FILE" | jq -r '.bytecode.object')

if [ -z "$BYTECODE" ] || [ "$BYTECODE" = "null" ]; then
    echo "Error: Failed to extract bytecode from compiled contract"
    exit 1
fi

echo "Bytecode extracted: ${BYTECODE:0:66}..."

# Save to file for use by generate-test-data.sh
echo "$BYTECODE" > contracts/TestToken.bytecode

echo "Contract compiled successfully!"
echo "Bytecode saved to: contracts/TestToken.bytecode"