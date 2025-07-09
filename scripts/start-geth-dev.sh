#!/bin/bash
# Script to start Geth in development mode

# Create data directory if it doesn't exist
mkdir -p data/geth-dev

# Start Geth in dev mode with instant mining
echo "Starting Geth in development mode..."
geth \
    --dev \
    --http \
    --http.addr 127.0.0.1 \
    --http.port 8545 \
    --http.api eth,net,web3,miner,txpool,debug,admin \
    --http.corsdomain "*" \
    --http.vhosts "*" \
    --ws \
    --ws.addr 127.0.0.1 \
    --ws.port 8546 \
    --ws.api eth,net,web3,miner,txpool,debug,admin \
    --datadir ./data/geth-dev \
    --dev.period 1 \
    --nodiscover \
    --maxpeers 0 \
    --verbosity 3 \
    > data/geth-dev/geth.log 2>&1