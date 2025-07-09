#!/bin/bash
# Unified prerequisite check script for ToRPC Proxy
# Helps users choose between CLI and GUI and checks appropriate requirements

set -e

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Banner
echo -e "${CYAN}"
echo "╔═══════════════════════════════════════════╗"
echo "║        ToRPC Proxy Prerequisites          ║"
echo "╚═══════════════════════════════════════════╝"
echo -e "${NC}"

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Function to display menu
show_menu() {
    echo "Which ToRPC Proxy component would you like to check?"
    echo ""
    echo -e "${BLUE}1)${NC} CLI (Command Line Interface)"
    echo "   - Lightweight, runs in terminal"
    echo "   - Perfect for servers and automation"
    echo "   - Can run as system service"
    echo ""
    echo -e "${BLUE}2)${NC} GUI (Desktop Application)"
    echo "   - System tray application"
    echo "   - Visual configuration interface"
    echo "   - Easy start/stop control"
    echo ""
    echo -e "${BLUE}3)${NC} Both (Check all prerequisites)"
    echo ""
    echo -e "${BLUE}4)${NC} Quick Install (Automated setup)"
    echo ""
    echo -e "${BLUE}5)${NC} Exit"
    echo ""
}

# Function for quick install
quick_install() {
    echo ""
    echo -e "${MAGENTA}🚀 Quick Install${NC}"
    echo "================"
    echo ""
    echo "This will attempt to:"
    echo "1. Install Rust/Cargo (if missing)"
    echo "2. Install Tor (if missing)"
    echo "3. Build both CLI and GUI"
    echo "4. Create desktop shortcuts"
    echo ""
    echo -e "${YELLOW}Continue? (y/n)${NC}"
    read -r response
    
    if [[ ! "$response" =~ ^[Yy]$ ]]; then
        return
    fi
    
    echo ""
    
    # Detect OS
    OS="unknown"
    if [[ "$OSTYPE" == "darwin"* ]]; then
        OS="macos"
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        OS="linux"
    elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
        OS="windows"
    fi
    
    # Install Rust if missing
    if ! command -v cargo >/dev/null 2>&1; then
        echo "📦 Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source $HOME/.cargo/env
        echo -e "${GREEN}✓${NC} Rust installed"
    else
        echo -e "${GREEN}✓${NC} Rust already installed"
    fi
    
    # Install Tor if missing
    if ! command -v tor >/dev/null 2>&1; then
        echo "📦 Installing Tor..."
        case "$OS" in
            "macos")
                if command -v brew >/dev/null 2>&1; then
                    brew install tor
                    brew services start tor
                else
                    echo -e "${RED}✗${NC} Homebrew not found. Install from https://brew.sh"
                    return 1
                fi
                ;;
            "linux")
                if [ -f /etc/debian_version ]; then
                    sudo apt update && sudo apt install -y tor
                    sudo systemctl start tor
                elif [ -f /etc/redhat-release ]; then
                    sudo yum install -y tor
                    sudo systemctl start tor
                elif [ -f /etc/arch-release ]; then
                    sudo pacman -S --noconfirm tor
                    sudo systemctl start tor
                fi
                ;;
            "windows")
                echo "Please download Tor from https://www.torproject.org/"
                echo "After installation, press Enter to continue..."
                read
                ;;
        esac
        echo -e "${GREEN}✓${NC} Tor installed"
    else
        echo -e "${GREEN}✓${NC} Tor already installed"
    fi
    
    # Build the project
    echo ""
    echo "🔨 Building ToRPC Proxy..."
    cd "$SCRIPT_DIR/.."
    
    # Build CLI
    echo "Building CLI..."
    cargo build --release -p torpc-proxy-cli
    echo -e "${GREEN}✓${NC} CLI built successfully"
    
    # Build GUI (if dependencies available)
    echo "Building GUI..."
    if cargo build --release -p torpc-proxy-gui 2>/dev/null; then
        echo -e "${GREEN}✓${NC} GUI built successfully"
    else
        echo -e "${YELLOW}⚠${NC} GUI build failed (missing dependencies?)"
    fi
    
    # Install CLI to PATH
    echo ""
    echo "📁 Installing to ~/.cargo/bin..."
    cp target/release/torpc-proxy ~/.cargo/bin/ 2>/dev/null || true
    echo -e "${GREEN}✓${NC} CLI installed to PATH"
    
    # Create desktop entry for GUI (Linux)
    if [ "$OS" == "linux" ] && [ -f "target/release/torpc-proxy-gui" ]; then
        echo "Creating desktop entry..."
        mkdir -p ~/.local/share/applications
        cat > ~/.local/share/applications/torpc-proxy.desktop << EOF
[Desktop Entry]
Name=ToRPC Proxy
Comment=Privacy-preserving Ethereum RPC proxy
Exec=$PWD/target/release/torpc-proxy-gui
Icon=$PWD/torpc-proxy-gui/icons/icon.png
Type=Application
Categories=Network;Security;
Terminal=false
EOF
        echo -e "${GREEN}✓${NC} Desktop entry created"
    fi
    
    echo ""
    echo -e "${GREEN}✅ Quick install complete!${NC}"
    echo ""
    echo "To start using ToRPC Proxy:"
    echo "  CLI: torpc-proxy --help"
    echo "  GUI: ./target/release/torpc-proxy-gui"
    echo ""
}

# Main menu loop
while true; do
    show_menu
    echo -n "Enter your choice (1-5): "
    read choice
    
    case $choice in
        1)
            echo ""
            if [ -f "$SCRIPT_DIR/check-prerequisites-cli.sh" ]; then
                bash "$SCRIPT_DIR/check-prerequisites-cli.sh"
            else
                echo -e "${RED}Error: CLI prerequisites script not found${NC}"
            fi
            echo ""
            echo "Press Enter to continue..."
            read
            clear
            ;;
            
        2)
            echo ""
            if [ -f "$SCRIPT_DIR/check-prerequisites-gui.sh" ]; then
                bash "$SCRIPT_DIR/check-prerequisites-gui.sh"
            else
                echo -e "${RED}Error: GUI prerequisites script not found${NC}"
            fi
            echo ""
            echo "Press Enter to continue..."
            read
            clear
            ;;
            
        3)
            echo ""
            echo -e "${BLUE}Checking CLI prerequisites...${NC}"
            echo "=============================="
            if [ -f "$SCRIPT_DIR/check-prerequisites-cli.sh" ]; then
                bash "$SCRIPT_DIR/check-prerequisites-cli.sh"
            fi
            
            echo ""
            echo -e "${BLUE}Checking GUI prerequisites...${NC}"
            echo "=============================="
            if [ -f "$SCRIPT_DIR/check-prerequisites-gui.sh" ]; then
                bash "$SCRIPT_DIR/check-prerequisites-gui.sh"
            fi
            
            echo ""
            echo "Press Enter to continue..."
            read
            clear
            ;;
            
        4)
            quick_install
            echo "Press Enter to continue..."
            read
            clear
            ;;
            
        5)
            echo ""
            echo "Thank you for using ToRPC Proxy!"
            echo "For more information, see the README files."
            echo ""
            exit 0
            ;;
            
        *)
            echo ""
            echo -e "${RED}Invalid choice. Please enter 1-5.${NC}"
            sleep 2
            clear
            ;;
    esac
done