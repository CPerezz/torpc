# ToRPC Proxy CLI

A command-line interface for running a local HTTP-to-SOCKS5 proxy that routes Ethereum RPC requests through Tor to .onion endpoints.

## Overview

The ToRPC Proxy CLI allows you to:
- Run a local proxy server that wallets can connect to
- Route all RPC traffic through Tor for privacy
- Connect to .onion Ethereum nodes without exposing your IP
- Run as a system service for always-on connectivity

## Installation

### From Source

```bash
# From the workspace root
cd torpc-proxy
make build

# Or build just the CLI
cargo build --release -p torpc-proxy-cli

# Binary location: ./target/release/torpc-proxy
```

### Install to System

```bash
make install
# Installs to ~/.cargo/bin/torpc-proxy
```

## Prerequisites

- **Tor**: Must be installed and running
  - macOS: `brew install tor && brew services start tor`
  - Linux: `sudo apt install tor && sudo systemctl start tor`
  - Windows: Download from [torproject.org](https://www.torproject.org/)

Run the prerequisite check:
```bash
../scripts/check-prerequisites-cli.sh
```

## Quick Start

### 1. Basic Usage

```bash
# Start proxy with an onion endpoint
torpc-proxy start --onion your-endpoint.onion:8545

# Start with custom local port
torpc-proxy start --onion your-endpoint.onion:8545 --port 8546

# Use a configuration file
torpc-proxy start --config torpc-proxy.toml
```

### 2. Test Tor Connectivity

```bash
# Verify Tor is working
torpc-proxy test
```

### 3. Show Configuration

```bash
# Display current configuration
torpc-proxy config
```

## Configuration File

Create `torpc-proxy.toml` in your working directory:

```toml
# Local port for wallet connections
port = 8545

# Tor SOCKS5 proxy
tor_proxy_host = "127.0.0.1"
tor_proxy_port = 9050

# Your .onion RPC endpoint
onion_endpoint = "your-onion-address.onion:8545"

# Logging level: trace, debug, info, warn, error
log_level = "info"
```

## Complete Walkthrough: MetaMask + Tor

### Step 1: Start Tor

```bash
# macOS
brew services start tor

# Linux
sudo systemctl start tor

# Verify Tor is running
curl --socks5 127.0.0.1:9050 https://check.torproject.org/api/ip
```

### Step 2: Configure and Start Proxy

```bash
# Create configuration
cat > torpc-proxy.toml << EOF
port = 8545
tor_proxy_host = "127.0.0.1"
tor_proxy_port = 9050
onion_endpoint = "your-actual-onion.onion:8545"
log_level = "info"
EOF

# Start the proxy
torpc-proxy start --config torpc-proxy.toml
```

### Step 3: Configure MetaMask

1. Open MetaMask
2. Click the network dropdown
3. Select "Add Network" → "Add a network manually"
4. Enter:
   - **Network Name**: `Ethereum (Tor)`
   - **RPC URL**: `http://localhost:8545`
   - **Chain ID**: `1` (for mainnet)
   - **Currency Symbol**: `ETH`
5. Save and switch to the new network

### Step 4: Verify Connection

```bash
# Check proxy logs
tail -f proxy.log

# Test with curl
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

## Command Reference

### `start` - Start the proxy server

```bash
torpc-proxy start [OPTIONS]

Options:
  -p, --port <PORT>           Local port to listen on [default: 8545]
  -o, --onion <ENDPOINT>      Onion endpoint (overrides config)
  -c, --config <FILE>         Config file path [default: torpc-proxy.toml]
      --tor-host <HOST>       Tor SOCKS5 host [default: 127.0.0.1]
      --tor-port <PORT>       Tor SOCKS5 port [default: 9050]
  -h, --help                  Print help
```

### `test` - Test Tor connectivity

```bash
torpc-proxy test [OPTIONS]

Options:
      --tor-host <HOST>       Tor SOCKS5 host [default: 127.0.0.1]
      --tor-port <PORT>       Tor SOCKS5 port [default: 9050]
  -h, --help                  Print help
```

### `config` - Show configuration

```bash
torpc-proxy config

# Shows the default configuration that would be used
```

## Running as a System Service

### Linux (systemd)

Create `/etc/systemd/system/torpc-proxy.service`:

```ini
[Unit]
Description=ToRPC Proxy Service
After=network.target tor.service
Requires=tor.service

[Service]
Type=simple
User=your-username
WorkingDirectory=/home/your-username
ExecStart=/home/your-username/.cargo/bin/torpc-proxy start --config /home/your-username/torpc-proxy.toml
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl enable torpc-proxy
sudo systemctl start torpc-proxy
sudo systemctl status torpc-proxy
```

### macOS (launchd)

Create `~/Library/LaunchAgents/com.torpc.proxy.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.torpc.proxy</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/your-username/.cargo/bin/torpc-proxy</string>
        <string>start</string>
        <string>--config</string>
        <string>/Users/your-username/torpc-proxy.toml</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/torpc-proxy.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/torpc-proxy.error.log</string>
</dict>
</plist>
```

Load and start:
```bash
launchctl load ~/Library/LaunchAgents/com.torpc.proxy.plist
launchctl start com.torpc.proxy
```

## Logging

Control log verbosity with the `RUST_LOG` environment variable:

```bash
# Debug logging
RUST_LOG=debug torpc-proxy start

# Trace logging (very verbose)
RUST_LOG=trace torpc-proxy start

# Only errors
RUST_LOG=error torpc-proxy start
```

Logs are written to:
- Console (stderr)
- `proxy.log` file in the current directory

## Troubleshooting

### "Failed to connect through Tor"

1. Check Tor is running:
   ```bash
   # Linux/macOS
   pgrep -x tor
   
   # Test Tor
   curl --socks5 127.0.0.1:9050 https://check.torproject.org/
   ```

2. Verify Tor port:
   ```bash
   # Default is 9050, but check your torrc
   grep "SocksPort" /etc/tor/torrc
   ```

### "Address already in use"

Another service is using port 8545:
```bash
# Find what's using the port
lsof -i :8545  # macOS/Linux
netstat -ano | findstr :8545  # Windows

# Use a different port
torpc-proxy start --port 8546
```

### "Connection refused" from wallet

1. Ensure proxy is running:
   ```bash
   ps aux | grep torpc-proxy
   ```

2. Check the listen address:
   ```bash
   # Should show listening on 127.0.0.1:8545
   netstat -an | grep 8545
   ```

3. Test with curl:
   ```bash
   curl -v http://localhost:8545
   ```

### Wallet transactions failing

1. Check proxy logs for errors
2. Verify the onion endpoint is correct
3. Ensure Tor has established a circuit (may take 30-60 seconds on first start)
4. Try restarting both Tor and the proxy

## Security Considerations

- The proxy only listens on localhost (127.0.0.1) by default
- No RPC methods are filtered - the remote node handles access control
- All traffic between the proxy and the .onion node is encrypted by Tor
- Local traffic (wallet to proxy) is unencrypted HTTP

## Performance

- Expect 200-500ms additional latency due to Tor routing
- First request may be slower while Tor builds circuits
- Consider running as a system service for better performance
- The proxy maintains persistent connections when possible

## Development

```bash
# Run with debug logging
RUST_LOG=debug cargo run -- start

# Run tests
cargo test

# Format code
cargo fmt

# Lint
cargo clippy
```

## Support

- Check the [main project README](../README.md) for more information
- Report issues at the project's issue tracker
- Tor documentation: https://www.torproject.org/docs/