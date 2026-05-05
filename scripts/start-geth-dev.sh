#!/bin/bash
# Start Geth in development mode for ToRPC integration tests.
#
# This script previously enabled the `admin`, `debug`, and `miner` RPC APIs
# with a wide-open CORS policy (`--http.corsdomain "*"` / `--http.vhosts "*"`).
# That meant any local process — or any browser-side JS that bypassed the
# torpc whitelist by hitting Geth directly on 8545 — could change the dev
# coinbase, run debug tracing, or seize the miner. Tightened scope below.

set -euo pipefail

mkdir -p data/geth-dev

echo "Starting Geth in development mode (12s block period, restricted RPC)..."
geth \
    --dev \
    --http \
    --http.addr 127.0.0.1 \
    --http.port 8545 \
    --http.api eth,net,web3 \
    --http.corsdomain "http://localhost:8080" \
    --http.vhosts "localhost,127.0.0.1" \
    --ws \
    --ws.addr 127.0.0.1 \
    --ws.port 8546 \
    --ws.api eth,net,web3 \
    --ws.origins "http://localhost:8080" \
    --datadir ./data/geth-dev \
    --dev.period 12 \
    --nodiscover \
    --maxpeers 0 \
    --verbosity 3 \
    > data/geth-dev/geth.log 2>&1
