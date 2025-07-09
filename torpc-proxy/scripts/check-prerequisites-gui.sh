#!/bin/bash
# Prerequisite check script for ToRPC Proxy GUI
# Checks system requirements for running the Tauri-based desktop application

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
    OS_VERSION=$(sw_vers -productVersion)
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
    # Detect desktop environment
    DESKTOP=$XDG_CURRENT_DESKTOP
    if [ -z "$DESKTOP" ]; then
        DESKTOP="unknown"
    fi
elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
    OS="windows"
fi

echo "🖥️  ToRPC Proxy GUI - Prerequisite Check"
echo "========================================"
echo ""
echo "Detected OS: $OS"
if [ "$OS" == "linux" ]; then
    echo "Linux distribution: $DISTRO"
    echo "Desktop environment: $DESKTOP"
elif [ "$OS" == "macos" ]; then
    echo "macOS version: $OS_VERSION"
fi
echo ""

# Track if all prerequisites are met
ALL_GOOD=true

# Function to check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# First run CLI prerequisites check
echo "📋 Running CLI prerequisites check first..."
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$SCRIPT_DIR/check-prerequisites-cli.sh" ]; then
    bash "$SCRIPT_DIR/check-prerequisites-cli.sh"
    if [ $? -ne 0 ]; then
        ALL_GOOD=false
    fi
else
    echo -e "${YELLOW}⚠${NC} CLI prerequisites script not found"
fi
echo ""

# Check Node.js and npm (for development)
echo "📦 Checking Node.js/npm (for development)..."
if command_exists node; then
    NODE_VERSION=$(node --version)
    echo -e "${GREEN}✓${NC} Node.js is installed ($NODE_VERSION)"
    
    # Check version is 16+
    NODE_MAJOR=$(echo $NODE_VERSION | cut -d'.' -f1 | sed 's/v//')
    if [ "$NODE_MAJOR" -ge 16 ]; then
        echo -e "${GREEN}✓${NC} Node.js version is sufficient"
    else
        echo -e "${YELLOW}⚠${NC} Node.js version is old. Version 16+ recommended."
    fi
    
    if command_exists npm; then
        NPM_VERSION=$(npm --version)
        echo -e "${GREEN}✓${NC} npm is installed ($NPM_VERSION)"
    else
        echo -e "${YELLOW}⚠${NC} npm not found"
    fi
else
    echo -e "${YELLOW}⚠${NC} Node.js is not installed (only needed for development)"
    echo "   To install: https://nodejs.org/"
fi
echo ""

# Check Tauri CLI (for development)
echo "🦀 Checking Tauri CLI (for development)..."
if command_exists cargo-tauri; then
    TAURI_VERSION=$(cargo-tauri --version | cut -d' ' -f2)
    echo -e "${GREEN}✓${NC} Tauri CLI is installed ($TAURI_VERSION)"
else
    echo -e "${YELLOW}⚠${NC} Tauri CLI not installed (only needed for development)"
    echo "   To install: cargo install tauri-cli"
fi
echo ""

# Check system requirements based on OS
echo "💻 Checking GUI system requirements..."

case "$OS" in
    "macos")
        # Check macOS version (10.13+ required)
        MAJOR=$(echo $OS_VERSION | cut -d'.' -f1)
        MINOR=$(echo $OS_VERSION | cut -d'.' -f2)
        if [ "$MAJOR" -ge 11 ] || ([ "$MAJOR" -eq 10 ] && [ "$MINOR" -ge 13 ]); then
            echo -e "${GREEN}✓${NC} macOS version is supported"
        else
            echo -e "${RED}✗${NC} macOS 10.13 or later required"
            ALL_GOOD=false
        fi
        ;;
        
    "linux")
        # Check for WebKitGTK
        echo -n "   Checking for WebKitGTK... "
        if pkg-config --exists webkit2gtk-4.0 2>/dev/null; then
            echo -e "${GREEN}installed${NC}"
        else
            echo -e "${RED}not found${NC}"
            echo ""
            echo "   WebKitGTK is required for the GUI. Install with:"
            case "$DISTRO" in
                "debian")
                    echo "   sudo apt install libwebkit2gtk-4.0-dev"
                    ;;
                "redhat")
                    echo "   sudo dnf install webkit2gtk4.0-devel"
                    ;;
                "arch")
                    echo "   sudo pacman -S webkit2gtk"
                    ;;
            esac
            ALL_GOOD=false
        fi
        
        # Check for system tray support
        echo -n "   Checking for system tray support... "
        if pkg-config --exists libappindicator3-0.1 2>/dev/null || pkg-config --exists ayatana-appindicator3-0.1 2>/dev/null; then
            echo -e "${GREEN}installed${NC}"
        else
            echo -e "${YELLOW}not found${NC}"
            echo ""
            echo "   System tray support recommended. Install with:"
            case "$DISTRO" in
                "debian")
                    echo "   sudo apt install libappindicator3-dev"
                    ;;
                "redhat")
                    echo "   sudo dnf install libappindicator-gtk3-devel"
                    ;;
                "arch")
                    echo "   sudo pacman -S libappindicator-gtk3"
                    ;;
            esac
        fi
        
        # Check desktop environment compatibility
        case "$DESKTOP" in
            *GNOME*)
                echo -e "   ${YELLOW}⚠${NC} GNOME detected - install AppIndicator extension for tray support"
                echo "   https://extensions.gnome.org/extension/615/appindicator-support/"
                ;;
            *KDE*|*XFCE*|*MATE*|*Cinnamon*)
                echo -e "   ${GREEN}✓${NC} Desktop environment has native tray support"
                ;;
            *)
                echo -e "   ${YELLOW}⚠${NC} Unknown desktop environment - tray support may vary"
                ;;
        esac
        ;;
        
    "windows")
        # Check for WebView2
        echo -n "   Checking for WebView2... "
        if [ -d "$LOCALAPPDATA/Microsoft/Edge/Application" ]; then
            echo -e "${GREEN}likely installed${NC}"
        else
            echo -e "${YELLOW}possibly missing${NC}"
            echo "   WebView2 is required. It's usually installed with Windows 10/11."
            echo "   If missing, the installer will download it."
        fi
        ;;
esac
echo ""

# Check display (important for headless systems)
if [ "$OS" == "linux" ]; then
    echo "🖥️  Checking display server..."
    if [ -n "$DISPLAY" ] || [ -n "$WAYLAND_DISPLAY" ]; then
        echo -e "${GREEN}✓${NC} Display server is available"
        if [ -n "$WAYLAND_DISPLAY" ]; then
            echo "   Running on Wayland"
        else
            echo "   Running on X11"
        fi
    else
        echo -e "${RED}✗${NC} No display server found"
        echo "   GUI applications require a graphical environment"
        ALL_GOOD=false
    fi
    echo ""
fi

# Check for pre-built binary
echo "📦 Checking for pre-built binary..."
if [ -f "../target/release/torpc-proxy-gui" ]; then
    echo -e "${GREEN}✓${NC} GUI binary found at: ../target/release/torpc-proxy-gui"
    
    # Check if it's executable
    if [ -x "../target/release/torpc-proxy-gui" ]; then
        echo -e "${GREEN}✓${NC} Binary is executable"
    else
        echo -e "${YELLOW}⚠${NC} Binary is not executable"
        echo "   Run: chmod +x ../target/release/torpc-proxy-gui"
    fi
else
    echo -e "${YELLOW}⚠${NC} No pre-built binary found"
    echo "   Build with: make build-gui"
fi
echo ""

# Summary
echo "📊 Summary"
echo "=========="
if [ "$ALL_GOOD" = true ]; then
    echo -e "${GREEN}✓${NC} All required prerequisites are installed!"
    echo ""
    echo "You can now run the ToRPC Proxy GUI:"
    echo "  cd $(dirname $0)/.."
    echo "  make run-gui"
    echo ""
    echo "For development:"
    echo "  make dev-gui"
else
    echo -e "${RED}✗${NC} Some prerequisites are missing."
    echo "Please install the missing components listed above."
fi
echo ""

# Platform-specific installation helper
if [ "$ALL_GOOD" = false ] && [ "$OS" == "linux" ]; then
    echo "Would you like to install missing Linux dependencies? (y/n)"
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]]; then
        echo ""
        echo "🚀 Installing dependencies..."
        
        case "$DISTRO" in
            "debian")
                sudo apt update
                sudo apt install -y libwebkit2gtk-4.0-dev libappindicator3-dev
                ;;
            "redhat")
                sudo dnf install -y webkit2gtk4.0-devel libappindicator-gtk3-devel
                ;;
            "arch")
                sudo pacman -S --noconfirm webkit2gtk libappindicator-gtk3
                ;;
            *)
                echo "Please install WebKitGTK and AppIndicator manually for your distribution"
                ;;
        esac
        
        echo ""
        echo "Installation complete! Please run this script again to verify."
    fi
fi

# Development setup helper
echo "🛠️  Development Setup"
echo "==================="
echo "To set up for GUI development:"
echo "1. Install Node.js 16+ from https://nodejs.org/"
echo "2. Install Tauri CLI: cargo install tauri-cli"
echo "3. Run development server: make dev-gui"
echo ""

exit 0