#!/bin/bash
# Prerequisite check script for ToRPC Proxy CLI
# Checks system requirements and helps install missing dependencies

set -e

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Detect OS
OS="unknown"
if [[ "$OSTYPE" == "darwin"* ]]; then
    OS="macos"
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    OS="linux"
    # Detect specific distro
    if [ -f /etc/debian_version ]; then
        DISTRO="debian"
    elif [ -f /etc/redhat-release ]; then
        DISTRO="redhat"  
    elif [ -f /etc/arch-release ]; then
        DISTRO="arch"
    else
        DISTRO="unknown"
    fi
elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
    OS="windows"
fi

echo "🔍 ToRPC Proxy CLI - Prerequisite Check"
echo "========================================"
echo ""
echo "Detected OS: $OS"
if [ "$OS" == "linux" ]; then
    echo "Linux distribution: $DISTRO"
fi
echo ""

# Track if all prerequisites are met
ALL_GOOD=true

# Function to check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Function to get version of a command
get_version() {
    if command_exists "$1"; then
        case "$1" in
            "cargo")
                cargo --version | cut -d' ' -f2
                ;;
            "tor")
                tor --version | head -n1 | cut -d' ' -f3
                ;;
            *)
                echo "unknown"
                ;;
        esac
    else
        echo "not installed"
    fi
}

# Check Rust/Cargo
echo "🦀 Checking Rust installation..."
if command_exists cargo; then
    CARGO_VERSION=$(get_version cargo)
    echo -e "${GREEN}✓${NC} Cargo is installed (version: $CARGO_VERSION)"
    
    # Check Rust version is recent enough
    RUST_VERSION=$(rustc --version | cut -d' ' -f2)
    RUST_MAJOR=$(echo $RUST_VERSION | cut -d'.' -f1)
    RUST_MINOR=$(echo $RUST_VERSION | cut -d'.' -f2)
    
    if [ "$RUST_MAJOR" -ge 1 ] && [ "$RUST_MINOR" -ge 70 ]; then
        echo -e "${GREEN}✓${NC} Rust version is sufficient ($RUST_VERSION)"
    else
        echo -e "${YELLOW}⚠${NC} Rust version is old ($RUST_VERSION). Consider updating."
        echo "   Run: rustup update"
    fi
else
    echo -e "${RED}✗${NC} Cargo is not installed"
    echo ""
    echo "   To install Rust:"
    echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo ""
    ALL_GOOD=false
fi
echo ""

# Check Tor
echo "🌐 Checking Tor installation..."
if command_exists tor; then
    TOR_VERSION=$(get_version tor)
    echo -e "${GREEN}✓${NC} Tor is installed (version: $TOR_VERSION)"
    
    # Check if Tor is running
    if pgrep -x "tor" > /dev/null; then
        echo -e "${GREEN}✓${NC} Tor is currently running"
        
        # Try to verify Tor connectivity
        echo -n "   Testing Tor connectivity... "
        if curl -s --socks5 127.0.0.1:9050 https://check.torproject.org/api/ip | grep -q "IsTor.*true"; then
            echo -e "${GREEN}working${NC}"
        else
            echo -e "${YELLOW}unable to verify${NC}"
        fi
    else
        echo -e "${YELLOW}⚠${NC} Tor is not running"
        echo ""
        echo "   To start Tor:"
        case "$OS" in
            "macos")
                echo "   brew services start tor"
                ;;
            "linux")
                echo "   sudo systemctl start tor"
                ;;
            "windows")
                echo "   Start Tor from the Start Menu or Services"
                ;;
        esac
        echo ""
    fi
else
    echo -e "${RED}✗${NC} Tor is not installed"
    echo ""
    echo "   To install Tor:"
    case "$OS" in
        "macos")
            echo "   brew install tor"
            ;;
        "linux")
            case "$DISTRO" in
                "debian")
                    echo "   sudo apt update && sudo apt install tor"
                    ;;
                "redhat")
                    echo "   sudo yum install tor"
                    ;;
                "arch")
                    echo "   sudo pacman -S tor"
                    ;;
                *)
                    echo "   Check your distribution's package manager for 'tor'"
                    ;;
            esac
            ;;
        "windows")
            echo "   Download from: https://www.torproject.org/download/"
            ;;
    esac
    echo ""
    ALL_GOOD=false
fi
echo ""

# Check port availability
echo "🔌 Checking port availability..."
DEFAULT_PORT=8545

# Function to check if port is in use
port_in_use() {
    if [ "$OS" == "macos" ] || [ "$OS" == "linux" ]; then
        lsof -i :$1 >/dev/null 2>&1
    elif [ "$OS" == "windows" ]; then
        netstat -an | grep -q ":$1.*LISTENING"
    fi
}

if port_in_use $DEFAULT_PORT; then
    echo -e "${YELLOW}⚠${NC} Port $DEFAULT_PORT is already in use"
    echo -n "   Used by: "
    if [ "$OS" == "macos" ] || [ "$OS" == "linux" ]; then
        lsof -i :$DEFAULT_PORT | grep LISTEN | awk '{print $1}' | head -n1
    else
        echo "another process"
    fi
    echo ""
    echo "   You can use a different port with: --port <PORT>"
else
    echo -e "${GREEN}✓${NC} Port $DEFAULT_PORT is available"
fi
echo ""

# Check system resources
echo "💻 Checking system resources..."
if [ "$OS" == "macos" ]; then
    MEM_TOTAL=$(sysctl -n hw.memsize | awk '{print int($1/1024/1024/1024)}')
    DISK_FREE=$(df -h / | tail -1 | awk '{print $4}')
elif [ "$OS" == "linux" ]; then
    MEM_TOTAL=$(free -g | grep Mem | awk '{print $2}')
    DISK_FREE=$(df -h / | tail -1 | awk '{print $4}')
else
    MEM_TOTAL="unknown"
    DISK_FREE="unknown"
fi

if [ "$MEM_TOTAL" != "unknown" ]; then
    echo "   Total memory: ${MEM_TOTAL}GB"
    if [ "$MEM_TOTAL" -ge 2 ]; then
        echo -e "   ${GREEN}✓${NC} Sufficient memory"
    else
        echo -e "   ${YELLOW}⚠${NC} Low memory - may affect performance"
    fi
fi

if [ "$DISK_FREE" != "unknown" ]; then
    echo "   Free disk space: $DISK_FREE"
fi
echo ""

# Check for build tools (optional but recommended)
echo "🔧 Checking optional build tools..."
if [ "$OS" == "macos" ]; then
    if command_exists xcode-select; then
        if xcode-select -p >/dev/null 2>&1; then
            echo -e "${GREEN}✓${NC} Xcode Command Line Tools installed"
        else
            echo -e "${YELLOW}⚠${NC} Xcode Command Line Tools not installed"
            echo "   Install with: xcode-select --install"
        fi
    fi
elif [ "$OS" == "linux" ]; then
    if command_exists gcc; then
        echo -e "${GREEN}✓${NC} GCC is installed"
    else
        echo -e "${YELLOW}⚠${NC} GCC not installed (needed for some Rust crates)"
        echo "   Install with: sudo apt install build-essential"
    fi
fi
echo ""

# Summary
echo "📊 Summary"
echo "=========="
if [ "$ALL_GOOD" = true ]; then
    echo -e "${GREEN}✓${NC} All required prerequisites are installed!"
    echo ""
    echo "You can now build and run the ToRPC Proxy CLI:"
    echo "  cd $(dirname $0)/.."
    echo "  make build"
    echo "  ./target/release/torpc-proxy --help"
else
    echo -e "${RED}✗${NC} Some prerequisites are missing."
    echo "Please install the missing components listed above."
fi
echo ""

# Offer to install missing dependencies
if [ "$ALL_GOOD" = false ]; then
    echo "Would you like to attempt automatic installation of missing components? (y/n)"
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]]; then
        echo ""
        echo "🚀 Attempting automatic installation..."
        
        # Install Rust if missing
        if ! command_exists cargo; then
            echo "Installing Rust..."
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            source $HOME/.cargo/env
        fi
        
        # Install Tor if missing
        if ! command_exists tor; then
            echo "Installing Tor..."
            case "$OS" in
                "macos")
                    if command_exists brew; then
                        brew install tor
                    else
                        echo "Homebrew not found. Please install from https://brew.sh"
                    fi
                    ;;
                "linux")
                    case "$DISTRO" in
                        "debian")
                            sudo apt update && sudo apt install -y tor
                            ;;
                        "redhat")
                            sudo yum install -y tor
                            ;;
                        "arch")
                            sudo pacman -S --noconfirm tor
                            ;;
                    esac
                    ;;
            esac
        fi
        
        echo ""
        echo "Installation complete! Please run this script again to verify."
    fi
fi

exit 0