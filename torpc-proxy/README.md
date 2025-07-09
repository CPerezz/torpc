# ToRPC Proxy Client

A lightweight local proxy that enables crypto wallets to connect through Tor to privacy-preserving RPC endpoints. Available as both a command-line tool and a desktop application.

## Overview

ToRPC Proxy runs on your local machine and:
- Listens on `localhost:8545` (standard Ethereum RPC port)
- Forwards all wallet requests through Tor
- Routes to your configured `.onion` RPC endpoint
- Works with MetaMask, Trust Wallet, and other Web3 wallets
- Available as CLI tool or system tray GUI application

## Project Structure

This is a Cargo workspace with three crates:
- `torpc-proxy-core` - Core proxy logic and shared functionality
- `torpc-proxy-cli` - Command-line interface
- `torpc-proxy-gui` - Desktop GUI application (Tauri-based)

## Prerequisites

- Tor must be installed and running on your system
- Default Tor SOCKS5 proxy port: 9050

### Installing Tor

**macOS:**
```bash
brew install tor
brew services start tor
```

**Ubuntu/Debian:**
```bash
sudo apt install tor
sudo systemctl start tor
```

**Windows:**
Download Tor Expert Bundle from https://www.torproject.org/

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/torpc
cd torpc/torpc-proxy

# Build CLI
make build

# Build GUI
make build-gui

# Or build everything
cargo build --release --workspace
```

### Pre-built Binaries

Download from [Releases](https://github.com/yourusername/torpc/releases)

## Usage

### Command Line Interface

```bash
# Start proxy with onion endpoint
torpc-proxy start --onion your-endpoint.onion:8545

# Or use a config file
torpc-proxy start --config my-config.toml
```

### Desktop Application

```bash
# Run the GUI application
make run-gui

# The app runs in system tray with:
# - Start/Stop proxy control
# - Configuration settings
# - Status indicator
```

### Configuration File

Create `torpc-proxy.toml`:

```toml
# Port for wallet connections (default: 8545)
port = 8545

# Tor SOCKS5 proxy settings
tor_proxy_host = "127.0.0.1"
tor_proxy_port = 9050

# Your .onion RPC endpoint
onion_endpoint = "your-onion-address.onion:8545"

# Logging level (trace, debug, info, warn, error)
log_level = "info"
```

### Commands

```bash
# Start the proxy
torpc-proxy start

# Show default configuration
torpc-proxy config

# Test Tor connectivity
torpc-proxy test

# Show help
torpc-proxy --help
```

## Wallet Configuration

### MetaMask

1. Open MetaMask
2. Click network dropdown → "Add Network"
3. Enter:
   - Network Name: `Ethereum (Private)`
   - RPC URL: `http://localhost:8545`
   - Chain ID: `1`
   - Currency Symbol: `ETH`
4. Save and switch to the new network

### Trust Wallet

1. Go to Settings → Networks
2. Add Custom Network
3. Use same settings as MetaMask above

### Other Wallets

Any wallet that supports custom RPC endpoints can use:
- RPC URL: `http://localhost:8545`

## Security

- Proxy binds only to localhost (127.0.0.1)
- All traffic goes through Tor network
- No logging of sensitive data
- No RPC method filtering (server handles security)

## Troubleshooting

### "Failed to connect through Tor"
- Ensure Tor is running: `tor --version`
- Check Tor is listening on port 9050
- Try: `torpc-proxy test`

### "Connection refused" on localhost:8545
- Check if proxy is running
- Ensure no other service is using port 8545
- Try a different port: `torpc-proxy start --port 8546`

### Wallet won't connect
- Verify proxy is running
- Check wallet network configuration
- Ensure using `http://` not `https://`

## Performance

- Expect 200-500ms additional latency due to Tor
- First connection may be slower (Tor circuit building)
- Subsequent requests use established circuits

## Development

```bash
# Run CLI with debug logging
make run-debug

# Run GUI in development mode (with hot reload)
make dev-gui

# Run all tests
make test-all

# Format code
make fmt

# Run lints
make clippy

# Check code
make check

# See all available commands
make help
```

### Building from Source

The project uses a Makefile for common tasks:
- `make build` - Build CLI binary
- `make build-gui` - Build GUI application
- `make install` - Install CLI to ~/.cargo/bin/
- `make clean` - Clean build artifacts

## License

MIT License - see LICENSE file for details