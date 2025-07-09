# TorPC - Anonymous Ethereum RPC Access via Tor

<div align="center">

![TorPC](https://img.shields.io/badge/TorPC-Anonymous%20RPC-purple?style=for-the-badge)
![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![Tor](https://img.shields.io/badge/Tor-7E4798?style=for-the-badge&logo=Tor-Browser&logoColor=white)
![Ethereum](https://img.shields.io/badge/Ethereum-3C3C3D?style=for-the-badge&logo=Ethereum&logoColor=white)

**Anonymous Ethereum RPC access through Tor with MEV protection**

[Features](#features) • [Quick Start](#quick-start) • [Installation](#installation) • [Usage](#usage) • [Security](#security)

</div>

## 🌟 Overview

TorPC provides a privacy-preserving gateway to interact with Ethereum nodes through the Tor network. It acts as a reverse proxy that:

- **Routes all RPC calls through Tor** for complete anonymity
- **Protects against MEV** by routing transactions through Flashbots
- **Filters dangerous RPC methods** to prevent wallet exposure
- **Rate limits requests** per Tor circuit to prevent abuse
- **Provides a simple web interface** for testing

### Architecture

```
User → Tor Network → .onion address → TorPC Proxy → Geth Node
                                            ↓
                                     Flashbots Relay
```

## 📋 System Requirements

### Operating Systems
- ✅ **Linux** (Ubuntu 20.04+, Debian 11+, Fedora 35+)
- ✅ **macOS** (10.15 Catalina or later)
- ✅ **Windows** (via WSL2)

### Software Prerequisites
- **Rust** 1.70 or later
- **Geth** (go-ethereum) 1.10+
- **Tor** 0.4.5+
- **Foundry** (for testing) - latest version

### Hardware Requirements
- **RAM**: 4GB minimum (8GB recommended)
- **Storage**: 10GB free space
- **Network**: Stable internet connection

## 🚀 Quick Start

### One-Line Install (Linux/macOS)

```bash
curl -sSL https://raw.githubusercontent.com/yourusername/torpc/main/install.sh | bash
```

### Manual Quick Setup

```bash
# 1. Clone the repository
git clone https://github.com/yourusername/torpc.git
cd torpc

# 2. Check prerequisites
./scripts/check-prerequisites.sh

# 3. Build the project
cargo build --release

# 4. Start all services
./scripts/start-all-dev.sh

# This script automatically starts:
# - Geth development node
# - Tor hidden service  
# - TorPC proxy server
# No need to run 'cargo run' separately!

# 5. Get your .onion address
cat data/tor/torpc/hostname
```

Your anonymous RPC endpoint will be available at: `http://YOUR-ONION-ADDRESS.onion/rpc`

### Understanding start-all-dev.sh

The `start-all-dev.sh` script is a convenient way to start all services with one command. It:

1. **Starts Geth** in development mode with a pre-funded account
2. **Generates test blockchain data** (if needed) including:
   - Funded test accounts
   - Deployed smart contracts
   - Sample transactions
3. **Starts Tor** and waits for the .onion address generation
4. **Builds TorPC** (if needed) in release mode
5. **Starts TorPC** proxy server on port 8080

After running this script, all services are running in the background. You can:
- Access the web interface at `http://localhost:8080`
- View logs with the commands shown at the end of the script output
- Stop everything with `./scripts/stop-all.sh`

## 🛠️ Detailed Installation

### Step 1: Install Rust

```bash
# Install rustup (Rust installer)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add Rust to PATH
source $HOME/.cargo/env

# Verify installation
rustc --version
cargo --version
```

### Step 2: Install Geth

#### Option A: Using Package Manager

**Ubuntu/Debian:**
```bash
sudo add-apt-repository -y ppa:ethereum/ethereum
sudo apt-get update
sudo apt-get install ethereum
```

**macOS:**
```bash
brew tap ethereum/ethereum
brew install ethereum
```

**Fedora:**
```bash
sudo dnf install ethereum
```

#### Option B: Download Binary

```bash
# Download latest Geth
wget https://gethstore.blob.core.windows.net/builds/geth-linux-amd64-1.13.5-916d6a44.tar.gz

# Extract
tar -xvf geth-linux-amd64-*.tar.gz

# Move to PATH
sudo mv geth-linux-amd64-*/geth /usr/local/bin/

# Verify
geth version
```

### Step 3: Install Tor

**Ubuntu/Debian:**
```bash
sudo apt update
sudo apt install tor
```

**macOS:**
```bash
brew install tor
```

**Fedora:**
```bash
sudo dnf install tor
```

### Step 4: Install Foundry (for testing)

```bash
# Install Foundry
curl -L https://foundry.paradigm.xyz | bash

# Follow the instructions to add foundryup to your PATH, then:
foundryup

# Verify installation
cast --version
forge --version
```

## 🏃 Running TorPC

### Method 1: Using Helper Scripts (Recommended)

**Important:** The scripts require bash. Either make them executable or run with bash:

```bash
# Make scripts executable (one-time setup)
chmod +x scripts/*.sh

# Then run directly:
./scripts/start-all-dev.sh

# OR run with bash explicitly:
bash scripts/start-all-dev.sh

# Start with verbose logging
bash scripts/start-all-dev.sh --verbose

# Start with full debug mode
bash scripts/start-all-dev.sh --debug

# Or start services individually:
bash scripts/start-geth-dev.sh     # Start Geth in development mode
bash scripts/start-tor.sh          # Start Tor hidden service
cargo run --release                # Start TorPC proxy
```

**Note:** Do NOT use `sh scripts/start-all-dev.sh` as it will use the wrong shell interpreter.

### Method 2: Manual Setup

#### 1. Start Geth Development Node

```bash
# Create data directory
mkdir -p data/geth-dev

# Start Geth
geth --dev \
     --http \
     --http.addr 127.0.0.1 \
     --http.port 8545 \
     --http.api eth,net,web3,personal,miner,txpool,debug \
     --http.corsdomain "*" \
     --datadir ./data/geth-dev \
     --mine
```

#### 2. Configure and Start Tor

```bash
# Create Tor data directory
mkdir -p data/tor/torpc
chmod 700 data/tor/torpc

# Start Tor with our configuration
tor -f configs/torrc
```

#### 3. Start TorPC Proxy

> **Note**: If you used `./scripts/start-all-dev.sh`, TorPC is already running!
> The script automatically builds and starts TorPC for you.
> Only run the commands below if you're starting services manually.

```bash
# Build and run TorPC (only if not using start-all-dev.sh)
cargo build --release
./target/release/torpc

# Or with custom settings:
GETH_URL=http://127.0.0.1:8545 \
BIND_ADDR=127.0.0.1:8080 \
RUST_LOG=info \
./target/release/torpc
```

### Method 3: Using Docker

```bash
# Build Docker image
docker build -t torpc .

# Run with Docker Compose
docker-compose up -d

# View logs
docker-compose logs -f
```

### Method 4: Systemd Services (Production)

```bash
# Install service files
sudo cp services/*.service /etc/systemd/system/

# Enable services
sudo systemctl enable torpc-geth torpc-tor torpc-proxy

# Start all services
sudo systemctl start torpc-geth torpc-tor torpc-proxy

# Check status
sudo systemctl status torpc-*
```

## 🛑 Stopping Services

### Graceful Shutdown (Recommended)

```bash
# Stop all services gracefully (waits for processes to terminate)
./scripts/stop-all.sh

# Stop with custom timeout (default is 30 seconds)
./scripts/stop-all.sh --timeout 60
```

### Force Shutdown

```bash
# Immediately kill all processes (use if graceful shutdown fails)
./scripts/stop-all.sh --force
```

### Manual Process Management

```bash
# Check for running processes
pgrep -f "geth.*--dev"
pgrep -f "tor -f configs/torrc"
pgrep -f "target/release/torpc"

# Kill specific processes manually
kill -9 <PID>

# Check if ports are freed
lsof -ti:8545  # Geth RPC port
lsof -ti:8546  # Geth WebSocket port
```

The enhanced stop script will:
- ✅ Wait for processes to terminate gracefully
- ✅ Use force kill if graceful shutdown fails
- ✅ Verify all processes are actually terminated
- ✅ Check that ports are freed
- ✅ Report accurate success/failure status
- ✅ Handle multiple Geth instances properly

## 🧅 Understanding Tor Hidden Services

### How It Works

1. **Automatic .onion Address Generation**: When Tor starts, it automatically generates a unique .onion address
2. **Private Key Storage**: The private key is stored in `data/tor/torpc/`
3. **No DNS Required**: .onion addresses work without any DNS configuration
4. **End-to-End Encryption**: All traffic is encrypted through the Tor network

### Getting Your .onion Address

```bash
# After starting Tor, get your address:
cat data/tor/torpc/hostname

# Example output:
# 3g2upl4pq6kufc4m7v5jzvfncqvhv5bak6d2nprpz7hgbc6jvdnq5jqd.onion
```

### Custom Vanity Addresses (Optional)

Generate a custom .onion address starting with specific characters:

```bash
# Install mkp224o
git clone https://github.com/cathugger/mkp224o.git
cd mkp224o
./autogen.sh
./configure
make

# Generate address starting with "torpc"
./mkp224o torpc

# Copy generated keys to TorPC
cp -r generated_address/* ../data/tor/torpc/
```

### Testing Tor Connectivity

```bash
# Test using torsocks and curl
torsocks curl http://YOUR-ONION-ADDRESS.onion/

# Test RPC endpoint
torsocks curl -X POST http://YOUR-ONION-ADDRESS.onion/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

## 🧪 Testing & Verification

### 1. Generate Test Blockchain Data

```bash
# Run the test data generation script
./scripts/generate-test-data.sh

# Run with verbose output
./scripts/generate-test-data.sh --verbose

# Run with debug mode for troubleshooting
./scripts/generate-test-data.sh --debug

# This will:
# - Create test accounts with ETH
# - Deploy smart contracts
# - Generate 100+ blocks with transactions
# - Output test account details
```

### 2. Test Using Web Interface

1. Open your browser (regular or Tor Browser)
2. Navigate to `http://localhost:8080` or `http://YOUR-ONION-ADDRESS.onion`
3. Use the interactive interface to test RPC calls

### 3. Test Using Command Line

```bash
# Test local endpoint
curl -X POST http://localhost:8080/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Test through Tor
torsocks curl -X POST http://YOUR-ONION-ADDRESS.onion/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x742d35Cc6634C0532925a3b844Bc9e7595f7777","latest"],"id":1}'

# Test MEV protection endpoint
torsocks curl -X POST http://YOUR-ONION-ADDRESS.onion/rpc/flashbots \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_sendRawTransaction","params":["0xRAW_TX_HEX"],"id":1}'
```

### 4. Test Using Foundry Cast

```bash
# Set RPC URL
export ETH_RPC_URL=http://localhost:8080/rpc

# Get latest block
cast block-number

# Get account balance
cast balance 0x742d35Cc6634C0532925a3b844Bc9e7595f7777

# Send transaction (will route through Flashbots)
cast send --private-key $PRIVATE_KEY --value 0.1ether $TO_ADDRESS --rpc-url http://localhost:8080/rpc/flashbots
```

### 5. Run Integration Tests

```bash
# Run all tests
cargo test

# Run specific test suites
cargo test --test integration
cargo test whitelist
cargo test proxy

# Run with verbose output
RUST_LOG=debug cargo test -- --nocapture
```

## ⚙️ Configuration

### Environment Variables

```bash
# Geth connection
GETH_URL=http://127.0.0.1:8545        # Geth RPC endpoint
FLASHBOTS_URL=https://relay.flashbots.net  # Flashbots relay

# Server settings
BIND_ADDR=127.0.0.1:8080              # Proxy bind address
RUST_LOG=info                         # Log level (trace/debug/info/warn/error)

# Rate limiting
RATE_LIMIT_REQUESTS=60                # Max requests per window
RATE_LIMIT_WINDOW=60                  # Window duration in seconds

# Security settings
MAX_REQUEST_SIZE=524288               # Max request body size (default: 512KB)
REQUEST_TIMEOUT=30                    # Request timeout in seconds
STRICT_SECURITY_HEADERS=false         # Enable strict security (no CORS)

# Tor settings (if using SOCKS)
TOR_SOCKS_PROXY=127.0.0.1:9050        # Tor SOCKS proxy address
```

### Configuration Files

#### `configs/torrc` - Tor Configuration

```ini
# Hidden service settings
HiddenServiceDir ./data/tor/torpc/
HiddenServicePort 80 127.0.0.1:8080
HiddenServiceVersion 3

# Performance
HiddenServiceMaxStreams 100
HiddenServiceMaxStreamsCloseCircuit 1

# Security
SocksPort 0                           # Disable SOCKS if only using hidden service

# Logging
Log notice file ./data/tor/tor.log
DataDirectory ./data/tor
```

#### Rate Limiting Configuration

Edit `src/rate_limit.rs` to adjust rate limiting:

```rust
pub struct RateLimitConfig {
    pub max_requests: u32,            // Default: 60
    pub window_duration: Duration,    // Default: 60 seconds
}
```

## 📖 Usage Examples

### Basic RPC Calls

```bash
# Get current block number
curl -X POST http://localhost:8080/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Get account balance
curl -X POST http://localhost:8080/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getBalance",
    "params":["0x742d35Cc6634C0532925a3b844Bc9e7595f7777", "latest"],
    "id":1
  }'

# Estimate gas
curl -X POST http://localhost:8080/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_estimateGas",
    "params":[{
      "from": "0x742d35Cc6634C0532925a3b844Bc9e7595f7777",
      "to": "0x742d35Cc6634C0532925a3b844Bc9e7595f8888",
      "value": "0x9184e72a"
    }],
    "id":1
  }'
```

### MEV-Protected Transactions

```bash
# Send transaction through Flashbots
curl -X POST http://localhost:8080/rpc/flashbots \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_sendRawTransaction",
    "params":["0xf86c0185..."],  # Your signed transaction
    "id":1
  }'
```

### Batch Requests

```bash
# Send multiple requests in one call
curl -X POST http://localhost:8080/rpc \
  -H "Content-Type: application/json" \
  -d '[
    {"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1},
    {"jsonrpc":"2.0","method":"net_version","params":[],"id":2},
    {"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":3}
  ]'
```

## 🔒 Security Features & Configuration

### Security Headers

TorPC implements comprehensive security headers to protect against common web attacks:

```bash
# Security headers applied to all responses:
# - X-Content-Type-Options: nosniff (prevents MIME type sniffing)
# - X-Frame-Options: DENY (prevents clickjacking)
# - X-XSS-Protection: 0 (disables legacy XSS filter)
# - Referrer-Policy: no-referrer (prevents onion address leaks)
# - Content-Security-Policy: default-src 'none' (strict CSP for API)
# - Cache-Control: no-store, no-cache, must-revalidate
# - Server header sanitized to prevent fingerprinting
```

### Request Size Limits

Protection against DoS attacks through configurable request size limits:

```bash
# Default: 512KB (sufficient for most Ethereum RPC requests)
export MAX_REQUEST_SIZE=524288

# For batch operations: 2MB
export MAX_REQUEST_SIZE=2097152

# Request timeout (default: 30 seconds)
export REQUEST_TIMEOUT=30

# Strict security headers (disables CORS)
export STRICT_SECURITY_HEADERS=true
```

### CORS Configuration

By default, TorPC allows CORS for maximum compatibility:

```bash
# Standard mode (with CORS - default)
export STRICT_SECURITY_HEADERS=false

# Strict mode (no CORS - recommended for production)
export STRICT_SECURITY_HEADERS=true
```

## 🔒 Security Best Practices

### 1. Firewall Configuration

```bash
# Block external access to Geth and TorPC
sudo ufw deny 8545/tcp    # Block Geth
sudo ufw deny 8080/tcp    # Block TorPC direct access

# Allow only localhost connections
sudo iptables -A INPUT -p tcp --dport 8545 -s 127.0.0.1 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 8545 -j DROP
sudo iptables -A INPUT -p tcp --dport 8080 -s 127.0.0.1 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 8080 -j DROP
```

### 2. Tor Circuit Isolation

Each connection through Tor uses a separate circuit, ensuring:
- No correlation between different users
- Isolated rate limiting per circuit
- Enhanced privacy

### 3. Key Security

```bash
# Secure Tor keys
chmod 700 data/tor/torpc
chmod 600 data/tor/torpc/hs_ed25519_secret_key

# Backup your keys securely
cp -r data/tor/torpc /secure/backup/location/
```

### 4. Operational Security

- ✅ **DO**: Always use Tor Browser or torsocks for anonymous access
- ✅ **DO**: Regularly update all components
- ✅ **DO**: Monitor logs for suspicious activity
- ❌ **DON'T**: Share your .onion address publicly unless intended
- ❌ **DON'T**: Run on a VPS that requires KYC
- ❌ **DON'T**: Use the same .onion for multiple services

## 🔧 Troubleshooting

### Common Issues

#### Geth Won't Start

```bash
# Check if port is already in use
lsof -i :8545

# Kill existing process
kill -9 $(lsof -t -i:8545)

# Check Geth logs
tail -f data/geth-dev/geth.log
```

#### Tor Connection Failed

```bash
# Check Tor status
systemctl status tor

# Check Tor logs
tail -f data/tor/tor.log

# Verify torrc syntax
tor --verify-config -f configs/torrc

# Check permissions
ls -la data/tor/torpc/
```

#### TorPC Build Errors

```bash
# Update Rust
rustup update

# Clean and rebuild
cargo clean
cargo build --release

# Check dependencies
cargo tree
```

#### Rate Limiting Issues

```bash
# Increase rate limits
export RATE_LIMIT_REQUESTS=120
export RATE_LIMIT_WINDOW=60

# Or modify in code and rebuild
```

### Debug Mode

```bash
# Enable debug logging
RUST_LOG=debug ./target/release/torpc

# Enable Tor debug logs
echo "Log debug file ./data/tor/debug.log" >> configs/torrc
```

### Performance Issues

1. **Slow Responses**: 
   - Check Geth sync status: `cast block-number`
   - Verify Tor circuit health
   - Consider increasing rate limits

2. **High Memory Usage**:
   - Limit Geth cache: `--cache 1024`
   - Reduce Tor MaxStreams

3. **Connection Drops**:
   - Check firewall rules
   - Verify Tor is running
   - Monitor system resources

## 📚 Advanced Topics

### Custom RPC Method Whitelisting

Edit `src/whitelist.rs` to modify allowed methods:

```rust
const ALLOWED_METHODS: &[&str] = &[
    // Add your custom methods here
    "eth_blockNumber",
    "eth_getBalance",
    // ...
];
```

### Flashbots Integration

The `/rpc/flashbots` endpoint automatically routes transactions to prevent MEV:

1. `eth_sendRawTransaction` → Flashbots Relay
2. Other methods → Local Geth node

To add Flashbots authentication:
```rust
// In src/proxy.rs
headers.insert("X-Flashbots-Signature", signature);
```

### Monitoring & Metrics

```bash
# Add Prometheus metrics (coming soon)
cargo add prometheus

# Export metrics endpoint
METRICS_ADDR=127.0.0.1:9090
```

### Docker Deployment

```dockerfile
# Dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y tor geth
COPY --from=builder /app/target/release/torpc /usr/local/bin/
COPY configs /etc/torpc/
CMD ["torpc"]
```

```yaml
# docker-compose.yml
version: '3.8'
services:
  geth:
    image: ethereum/client-go:latest
    command: --dev --http --http.addr 0.0.0.0
    volumes:
      - geth-data:/data
  
  tor:
    image: osminogin/tor-simple
    volumes:
      - tor-data:/var/lib/tor
      - ./configs/torrc:/etc/tor/torrc
  
  torpc:
    build: .
    depends_on:
      - geth
      - tor
    environment:
      - GETH_URL=http://geth:8545
    ports:
      - "8080:8080"

volumes:
  geth-data:
  tor-data:
```

## 🤝 Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Setup

```bash
# Fork and clone
git clone https://github.com/yourusername/torpc.git
cd torpc

# Create feature branch
git checkout -b feature/your-feature

# Install dev dependencies
cargo install cargo-watch cargo-audit

# Run in watch mode
cargo watch -x run

# Run tests on change
cargo watch -x test
```

## 📜 License

This project is licensed under the MIT License - see [LICENSE](LICENSE) for details.

## 🙏 Acknowledgments

- [Tor Project](https://www.torproject.org/) for anonymous networking
- [Ethereum Foundation](https://ethereum.org/) for the blockchain
- [Flashbots](https://flashbots.net/) for MEV protection
- [Rust Community](https://www.rust-lang.org/) for the amazing ecosystem

---

<div align="center">

**Built with 🦀 Rust and 🧅 Tor for a more private Ethereum**

[Report Bug](https://github.com/yourusername/torpc/issues) • [Request Feature](https://github.com/yourusername/torpc/issues)

</div>