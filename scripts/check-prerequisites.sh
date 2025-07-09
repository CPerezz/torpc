#!/bin/bash
# Script to check if all prerequisites are installed

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "🔍 Checking TorPC Prerequisites..."
echo "=================================="

# Track if all prerequisites are met
ALL_GOOD=true

# Check Rust
echo -e "\n📦 Checking Rust..."
if command -v rustc &> /dev/null && command -v cargo &> /dev/null; then
    RUST_VERSION=$(rustc --version | cut -d' ' -f2)
    echo -e "${GREEN}✓ Rust is installed (version $RUST_VERSION)${NC}"
    
    # Check minimum version (1.70)
    MIN_VERSION="1.70.0"
    if [ "$(printf '%s\n' "$MIN_VERSION" "$RUST_VERSION" | sort -V | head -n1)" = "$MIN_VERSION" ]; then
        echo -e "${GREEN}  Version meets minimum requirement (1.70+)${NC}"
    else
        echo -e "${YELLOW}  Warning: Rust version is older than recommended 1.70${NC}"
    fi
else
    echo -e "${RED}✗ Rust is not installed${NC}"
    echo "  Install from: https://rustup.rs"
    ALL_GOOD=false
fi

# Check Geth
echo -e "\n⚡ Checking Geth..."
if command -v geth &> /dev/null; then
    GETH_VERSION=$(geth version | grep "Version:" | cut -d' ' -f2)
    echo -e "${GREEN}✓ Geth is installed (version $GETH_VERSION)${NC}"
else
    echo -e "${RED}✗ Geth is not installed${NC}"
    echo "  Install instructions:"
    if [[ "$OSTYPE" == "darwin"* ]]; then
        echo "    brew tap ethereum/ethereum && brew install ethereum"
    elif [[ -f /etc/debian_version ]]; then
        echo "    sudo add-apt-repository -y ppa:ethereum/ethereum"
        echo "    sudo apt-get update && sudo apt-get install ethereum"
    else
        echo "    Visit: https://geth.ethereum.org/downloads"
    fi
    ALL_GOOD=false
fi

# Check Tor
echo -e "\n🧅 Checking Tor..."
if command -v tor &> /dev/null; then
    TOR_VERSION=$(tor --version | head -n1)
    echo -e "${GREEN}✓ Tor is installed${NC}"
    echo "  $TOR_VERSION"
else
    echo -e "${RED}✗ Tor is not installed${NC}"
    echo "  Install instructions:"
    if [[ "$OSTYPE" == "darwin"* ]]; then
        echo "    brew install tor"
    elif [[ -f /etc/debian_version ]]; then
        echo "    sudo apt update && sudo apt install tor"
    else
        echo "    Visit: https://www.torproject.org"
    fi
    ALL_GOOD=false
fi

# Check Foundry (optional but recommended)
echo -e "\n🔨 Checking Foundry (optional)..."
if command -v cast &> /dev/null && command -v forge &> /dev/null; then
    echo -e "${GREEN}✓ Foundry is installed${NC}"
    echo "  $(cast --version)"
else
    echo -e "${YELLOW}⚠ Foundry is not installed (optional but recommended for testing)${NC}"
    echo "  Install from: https://book.getfoundry.sh/getting-started/installation"
fi

# Check Git
echo -e "\n📝 Checking Git..."
if command -v git &> /dev/null; then
    GIT_VERSION=$(git --version)
    echo -e "${GREEN}✓ Git is installed${NC}"
    echo "  $GIT_VERSION"
else
    echo -e "${RED}✗ Git is not installed${NC}"
    ALL_GOOD=false
fi

# Check system resources
echo -e "\n💻 Checking System Resources..."
if [[ "$OSTYPE" == "darwin"* ]]; then
    TOTAL_MEM=$(sysctl -n hw.memsize | awk '{print $1/1024/1024/1024}')
    FREE_DISK=$(df -h . | awk 'NR==2 {print $4}')
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    TOTAL_MEM=$(free -g | awk 'NR==2{print $2}')
    FREE_DISK=$(df -h . | awk 'NR==2 {print $4}')
fi

echo -e "  Total RAM: ${TOTAL_MEM}GB (4GB minimum, 8GB recommended)"
echo -e "  Free disk space: ${FREE_DISK} (10GB recommended)"

# Summary
echo -e "\n=================================="
if [ "$ALL_GOOD" = true ]; then
    echo -e "${GREEN}✅ All required prerequisites are installed!${NC}"
    echo -e "\nYou can now proceed with building TorPC:"
    echo -e "  ${YELLOW}cargo build --release${NC}"
else
    echo -e "${RED}❌ Some prerequisites are missing.${NC}"
    echo -e "Please install the missing components before proceeding."
fi

echo ""