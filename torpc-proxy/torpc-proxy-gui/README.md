# ToRPC Proxy GUI

A desktop application for managing ToRPC proxy connections with a system tray interface. Built with Tauri for a native, lightweight experience.

## Overview

The ToRPC Proxy GUI provides:
- System tray application with status indicator
- Easy start/stop proxy control
- Visual configuration interface
- Auto-start on system boot (configurable)
- Real-time connection status
- Minimal resource usage

## Installation

### Prerequisites

- All [CLI prerequisites](../torpc-proxy-cli/README.md#prerequisites) (Tor, etc.)
- For development: Node.js 16+ and npm

Run the prerequisite check:
```bash
../scripts/check-prerequisites-gui.sh
```

### Pre-built Binaries

Download the installer for your platform from the releases page:
- **macOS**: `ToRPC-Proxy.dmg`
- **Windows**: `ToRPC-Proxy_x64.msi`
- **Linux**: `torpc-proxy-gui.AppImage` or `.deb`

### Build from Source

```bash
# From the workspace root
cd torpc-proxy
make build-gui

# Or with Tauri CLI (for development)
make install-tauri-cli  # First time only
make dev-gui           # Run in development mode
```

## First Run Walkthrough

### 1. Launch the Application

After installation, launch "ToRPC Proxy" from your applications menu. The app will:
- Start minimized to system tray
- Show a small icon in your system tray/menu bar
- Display connection status (red = stopped, green = running)

### 2. Open Settings

Right-click the system tray icon and select "Settings" to configure:

![Settings Window Placeholder]
- **Onion Endpoint**: Your .onion RPC endpoint (e.g., `abc123.onion:8545`)
- **Listen Port**: Local port for wallet connections (default: 8545)
- **Tor Proxy**: Tor SOCKS5 address (default: 127.0.0.1:9050)

### 3. Start the Proxy

Two ways to start:
1. Right-click tray icon → "Start Proxy"
2. Or use the Settings window after configuration

The icon will turn green when running successfully.

### 4. Connect Your Wallet

Configure your wallet exactly as with the CLI version:
- RPC URL: `http://localhost:8545`
- Chain ID: `1` (for mainnet)
- See [CLI wallet setup](../torpc-proxy-cli/README.md#complete-walkthrough-metamask--tor)

## Features

### System Tray Menu

Right-click the tray icon to access:
- **Start Proxy** - Start the proxy service
- **Stop Proxy** - Stop the proxy service
- **Settings** - Open configuration window
- **Quit** - Exit the application

### Status Indicators

The tray icon changes color to show status:
- 🔴 **Red** - Proxy stopped
- 🟡 **Yellow** - Starting/Stopping
- 🟢 **Green** - Proxy running
- ⚫ **Gray** - Error state

### Configuration Window

The settings window allows you to:
- Configure the onion endpoint
- Change the local listening port
- Modify Tor proxy settings
- Save configuration for next launch
- View current proxy status

### Auto-Start Setup

#### macOS

1. Open System Preferences → Users & Groups
2. Select your user → Login Items
3. Click "+" and add ToRPC Proxy
4. Or use the app's preference (coming soon)

#### Windows

1. The installer offers to enable auto-start
2. Or manually: Win+R → `shell:startup` → Copy shortcut

#### Linux

Depends on desktop environment:

**GNOME**:
```bash
cp ~/.local/share/applications/torpc-proxy.desktop ~/.config/autostart/
```

**KDE**:
System Settings → Startup and Shutdown → Autostart → Add Program

## Configuration

Configuration is stored in platform-specific locations:

- **macOS**: `~/Library/Application Support/com.torpc.proxy/config.json`
- **Windows**: `%APPDATA%\com.torpc.proxy\config.json`
- **Linux**: `~/.config/com.torpc.proxy/config.json`

Example configuration:
```json
{
  "onion_endpoint": "your-endpoint.onion:8545",
  "listen_port": 8545,
  "tor_proxy_host": "127.0.0.1",
  "tor_proxy_port": 9050,
  "auto_start": true,
  "start_minimized": true
}
```

## Troubleshooting

### Application won't start

1. Check prerequisites:
   ```bash
   ../scripts/check-prerequisites-gui.sh
   ```

2. Run from terminal to see errors:
   ```bash
   # macOS
   /Applications/ToRPC\ Proxy.app/Contents/MacOS/torpc-proxy-gui
   
   # Linux
   ./torpc-proxy-gui
   
   # Windows
   "C:\Program Files\ToRPC Proxy\torpc-proxy-gui.exe"
   ```

### Tray icon not appearing

**Linux**: Ensure you have a system tray extension:
- GNOME: Install "AppIndicator Support" extension
- KDE: Built-in support
- XFCE: Built-in support

**Windows**: Check notification area settings

### Can't save configuration

Ensure write permissions to config directory:
```bash
# Linux/macOS
mkdir -p ~/.config/com.torpc.proxy
chmod 755 ~/.config/com.torpc.proxy

# Windows (run as admin)
mkdir %APPDATA%\com.torpc.proxy
```

### High CPU/Memory usage

Normal usage:
- Memory: 20-50 MB
- CPU: <1% when idle, 2-5% when proxying

If higher, try:
1. Restart the application
2. Check for errors in logs
3. Disable hardware acceleration (if supported)

## Development

### Project Structure

```
torpc-proxy-gui/
├── src/
│   └── main.rs         # Tauri backend (Rust)
├── ui/
│   ├── index.html      # Main window HTML
│   ├── style.css       # Styling
│   └── app.js          # Frontend logic
├── icons/              # Application icons
├── tauri.conf.json     # Tauri configuration
└── Cargo.toml         # Rust dependencies
```

### Development Setup

```bash
# Install dependencies
make install-tauri-cli  # or: cargo install tauri-cli

# Run in development mode (hot reload)
cd torpc-proxy-gui
cargo tauri dev

# Build for production
cargo tauri build
```

### Customizing the UI

1. Edit files in the `ui/` directory
2. Modify `style.css` for appearance
3. Update `app.js` for behavior
4. Changes reload automatically in dev mode

### Building Installers

```bash
# Build for current platform
cargo tauri build

# Outputs in target/release/bundle/
# - dmg (macOS)
# - msi (Windows)
# - deb/AppImage (Linux)
```

Cross-compilation requires additional setup - see [Tauri docs](https://tauri.app/v1/guides/building/cross-platform).

## Platform-Specific Notes

### macOS

- Requires macOS 10.13 or later
- First run may show security warning - Open System Preferences → Security & Privacy to allow
- Tray icon appears in menu bar (top right)

### Windows

- Requires Windows 10 1803 or later
- May need to install WebView2 (installer handles this)
- Tray icon appears in notification area (bottom right)

### Linux

- Requires WebKitGTK 4.0
- System tray support varies by desktop environment
- AppImage is the most portable format

Install dependencies:
```bash
# Debian/Ubuntu
sudo apt install libwebkit2gtk-4.0-dev libappindicator3-dev

# Fedora
sudo dnf install webkit2gtk4.0-devel libappindicator-gtk3-devel

# Arch
sudo pacman -S webkit2gtk libappindicator-gtk3
```

## Keyboard Shortcuts

When the settings window is open:
- `Cmd/Ctrl + S` - Save configuration
- `Cmd/Ctrl + Q` - Quit application
- `Escape` - Close settings window

## Logs

Application logs are stored in:
- **macOS**: `~/Library/Logs/com.torpc.proxy/`
- **Windows**: `%LOCALAPPDATA%\com.torpc.proxy\logs\`
- **Linux**: `~/.local/share/com.torpc.proxy/logs/`

View logs:
```bash
# macOS
tail -f ~/Library/Logs/com.torpc.proxy/app.log

# Linux
tail -f ~/.local/share/com.torpc.proxy/logs/app.log

# Windows PowerShell
Get-Content "$env:LOCALAPPDATA\com.torpc.proxy\logs\app.log" -Wait
```

## Uninstallation

### macOS
1. Quit the application
2. Drag ToRPC Proxy.app to Trash
3. Remove config: `rm -rf ~/Library/Application\ Support/com.torpc.proxy`

### Windows
1. Use Add/Remove Programs
2. Or run the uninstaller from the install directory

### Linux
```bash
# AppImage - just delete the file
rm torpc-proxy-gui.AppImage

# Deb package
sudo apt remove torpc-proxy-gui

# Remove configuration
rm -rf ~/.config/com.torpc.proxy
```

## Security

- Configuration is stored in plain text (be cautious with onion endpoints)
- The GUI runs with your user privileges
- Communication with the proxy core uses local IPC only
- No telemetry or external connections (except through Tor)

## Support

- See the [main project README](../README.md) for general information
- GUI-specific issues: Check the issue tracker
- Tauri documentation: https://tauri.app/