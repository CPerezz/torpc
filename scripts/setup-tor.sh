#!/bin/bash
# Script to help set up Tor for TorPC

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "🧅 Setting up Tor for TorPC..."

# Check if Tor is installed
if ! command -v tor &> /dev/null; then
    echo -e "${YELLOW}Tor is not installed. Please install it using:${NC}"
    echo ""
    
    # Detect OS and provide installation instructions
    if [[ "$OSTYPE" == "darwin"* ]]; then
        echo "  brew install tor"
    elif [[ -f /etc/debian_version ]]; then
        echo "  sudo apt-get update && sudo apt-get install tor"
    elif [[ -f /etc/redhat-release ]]; then
        echo "  sudo yum install tor"
    else
        echo "  Please install Tor using your system's package manager"
    fi
    
    echo ""
    echo "After installing Tor, run this script again."
    exit 1
fi

echo -e "${GREEN}✓ Tor is installed${NC}"
tor --version

# Create necessary directories
echo -e "\n${YELLOW}Creating Tor data directories...${NC}"
mkdir -p data/tor/torpc
chmod 700 data/tor/torpc

# Check if torrc exists
if [ ! -f configs/torrc ]; then
    echo -e "${RED}Error: configs/torrc not found${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Tor configuration found${NC}"

# Create a systemd service file (optional)
echo -e "\n${YELLOW}Creating systemd service file (optional)...${NC}"
cat > torpc-tor.service << 'EOF'
[Unit]
Description=Tor hidden service for TorPC
After=network.target

[Service]
Type=simple
User=$USER
WorkingDirectory=$PWD
ExecStart=/usr/bin/tor -f $PWD/configs/torrc
ExecReload=/bin/kill -HUP $MAINPID
KillSignal=SIGINT
TimeoutSec=60
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

# Replace variables in service file
sed -i.bak "s|\$USER|$USER|g" torpc-tor.service
sed -i.bak "s|\$PWD|$PWD|g" torpc-tor.service
rm torpc-tor.service.bak

echo -e "${GREEN}✓ Created torpc-tor.service${NC}"
echo ""
echo "To install as a systemd service (optional):"
echo "  sudo cp torpc-tor.service /etc/systemd/system/"
echo "  sudo systemctl daemon-reload"
echo "  sudo systemctl enable torpc-tor"
echo "  sudo systemctl start torpc-tor"

# Create start script
echo -e "\n${YELLOW}Creating start-tor.sh script...${NC}"
cat > scripts/start-tor.sh << 'EOF'
#!/bin/bash
# Start Tor with our configuration

echo "Starting Tor hidden service..."
tor -f configs/torrc
EOF

chmod +x scripts/start-tor.sh

echo -e "${GREEN}✓ Created scripts/start-tor.sh${NC}"

echo -e "\n${GREEN}Setup complete!${NC}"
echo ""
echo "To start Tor:"
echo "  ./scripts/start-tor.sh"
echo ""
echo "After starting Tor, your .onion address will be in:"
echo "  data/tor/torpc/hostname"
echo ""
echo "The service will forward:"
echo "  http://your-address.onion → http://127.0.0.1:8080"