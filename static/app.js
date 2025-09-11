// TorPC Frontend JavaScript

// Elements
const methodSelect = document.getElementById('method');
const paramsSection = document.getElementById('params-section');
const paramsInput = document.getElementById('params');
const testBtn = document.getElementById('test-btn');
const requestDisplay = document.getElementById('request-display');
const responseDisplay = document.getElementById('response-display');
const statusElement = document.getElementById('status');
const torInfo = document.getElementById('tor-info');

// Check connection status on load
window.addEventListener('DOMContentLoaded', () => {
    checkStatus();
    setupMethodSelector();
});

// Check RPC status
async function checkStatus() {
    try {
        const response = await fetch('/rpc', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                jsonrpc: '2.0',
                method: 'net_version',
                params: [],
                id: 1
            })
        });

        if (response.ok) {
            statusElement.textContent = 'Online';
            statusElement.className = 'status-online';
        } else {
            statusElement.textContent = 'Offline';
            statusElement.className = 'status-offline';
        }
    } catch (error) {
        statusElement.textContent = 'Offline';
        statusElement.className = 'status-offline';
    }

    // Check for Tor info (this would normally come from a status endpoint)
    // For now, we'll just show instructions
    torInfo.textContent = `To use through Tor:
1. Run: ./scripts/start-tor.sh
2. Check: data/tor/torpc/hostname for your .onion address
3. Connect via: torsocks curl http://your-address.onion/rpc`;
}

// Setup method selector
function setupMethodSelector() {
    methodSelect.addEventListener('change', () => {
        const method = methodSelect.value;
        
        // Show params input for methods that need it
        if (method === 'eth_getBalance') {
            paramsSection.style.display = 'block';
            paramsInput.placeholder = '["0x742d35Cc6634C0532925a3b844Bc9e7595f7777", "latest"]';
        } else {
            paramsSection.style.display = 'none';
            paramsInput.value = '';
        }
    });
}

// Send test request
testBtn.addEventListener('click', async () => {
    const method = methodSelect.value;
    const endpoint = document.querySelector('input[name="endpoint"]:checked').value;
    
    // Build params
    let params = [];
    if (paramsInput.value) {
        try {
            params = JSON.parse(paramsInput.value);
        } catch (e) {
            alert('Invalid JSON in parameters');
            return;
        }
    }
    
    // Build request
    const request = {
        jsonrpc: '2.0',
        method: method,
        params: params,
        id: Date.now()
    };
    
    // Display request
    requestDisplay.textContent = JSON.stringify(request, null, 2);
    responseDisplay.textContent = 'Sending...';
    
    try {
        const response = await fetch(endpoint, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(request)
        });
        
        const data = await response.json();
        
        // Display response with syntax highlighting
        responseDisplay.textContent = JSON.stringify(data, null, 2);
        
        // Color code based on success/error
        if (data.error) {
            responseDisplay.style.borderColor = '#e74c3c';
        } else {
            responseDisplay.style.borderColor = '#27ae60';
        }
        
    } catch (error) {
        responseDisplay.textContent = `Error: ${error.message}`;
        responseDisplay.style.borderColor = '#e74c3c';
    }
});

// Copy functionality for code blocks
document.querySelectorAll('.code-block').forEach(block => {
    block.addEventListener('click', function() {
        if (this.textContent && this.textContent !== 'Sending...') {
            navigator.clipboard.writeText(this.textContent).then(() => {
                // Visual feedback
                const original = this.style.borderColor;
                this.style.borderColor = '#27ae60';
                setTimeout(() => {
                    this.style.borderColor = original;
                }, 500);
            });
        }
    });
});

// Auto-update RPC URL if not on localhost
if (window.location.hostname !== 'localhost' && window.location.hostname !== '127.0.0.1') {
    document.getElementById('full-rpc-url').textContent = 
        `${window.location.protocol}//${window.location.host}/rpc`;
}

// MetaMask Integration
const addToMetaMaskBtn = document.getElementById('add-to-metamask');
const metamaskStatus = document.getElementById('metamask-status');
const statusText = document.getElementById('status-text');
const rpcInfo = document.getElementById('rpc-info');
const rpcUrlInput = document.getElementById('rpc-url');
const copyBtn = document.getElementById('copy-btn');
const instructions = document.getElementById('instructions');

// Trust Wallet Integration
const addToTrustWalletBtn = document.getElementById('add-to-trustwallet');
const trustwalletStatus = document.getElementById('trustwallet-status');
const trustStatusText = document.getElementById('trust-status-text');
const trustRpcInfo = document.getElementById('trust-rpc-info');
const trustRpcUrlInput = document.getElementById('trust-rpc-url');
const trustCopyBtn = document.getElementById('trust-copy-btn');
const trustInstructions = document.getElementById('trust-instructions');
const deepLinkBtn = document.getElementById('trust-deeplink-btn');

// Coinbase Wallet Integration
const addToCoinbaseBtn = document.getElementById('add-to-coinbase');
const coinbaseStatus = document.getElementById('coinbase-status');
const coinbaseStatusText = document.getElementById('coinbase-status-text');
const coinbaseRpcInfo = document.getElementById('coinbase-rpc-info');
const coinbaseRpcUrlInput = document.getElementById('coinbase-rpc-url');
const coinbaseCopyBtn = document.getElementById('coinbase-copy-btn');
const coinbaseInstructions = document.getElementById('coinbase-instructions');
const coinbaseAddNetworkBtn = document.getElementById('coinbase-add-network-btn');

// Rainbow Wallet Integration
const addToRainbowBtn = document.getElementById('add-to-rainbow');
const rainbowStatus = document.getElementById('rainbow-status');
const rainbowStatusText = document.getElementById('rainbow-status-text');
const rainbowRpcInfo = document.getElementById('rainbow-rpc-info');
const rainbowRpcUrlInput = document.getElementById('rainbow-rpc-url');
const rainbowCopyBtn = document.getElementById('rainbow-copy-btn');
const rainbowInstructions = document.getElementById('rainbow-instructions');

// Rabby Wallet Integration
const addToRabbyBtn = document.getElementById('add-to-rabby');
const rabbyStatus = document.getElementById('rabby-status');
const rabbyStatusText = document.getElementById('rabby-status-text');
const rabbyRpcInfo = document.getElementById('rabby-rpc-info');
const rabbyRpcUrlInput = document.getElementById('rabby-rpc-url');
const rabbyCopyBtn = document.getElementById('rabby-copy-btn');
const rabbyInstructions = document.getElementById('rabby-instructions');
const rabbyAddNetworkBtn = document.getElementById('rabby-add-network-btn');

// Handle Add to MetaMask button click
addToMetaMaskBtn.addEventListener('click', async () => {
    // Check if MetaMask is installed
    if (!MetaMaskHelper.isInstalled()) {
        statusText.textContent = 'MetaMask is not installed. Please install MetaMask first.';
        metamaskStatus.style.display = 'block';
        return;
    }
    
    // Show detection UI
    metamaskStatus.style.display = 'block';
    statusText.innerHTML = '<span class="spinner"></span> Detecting ToRPC proxy client...';
    
    try {
        // Query the discovery API
        const discovery = await MetaMaskHelper.queryProxyDiscovery();
        
        if (discovery.success) {
            // Success - show the detected RPC URL
            statusText.textContent = '✓ ToRPC proxy detected!';
            rpcUrlInput.value = discovery.rpcUrl;
            rpcInfo.style.display = 'block';
            instructions.style.display = 'block';
            
            // Auto-select the text for easy copying
            rpcUrlInput.select();
        } else {
            // Failed to detect - show manual entry
            statusText.textContent = discovery.message || 'ToRPC proxy not detected. Enter RPC URL manually:';
            rpcUrlInput.value = discovery.fallbackUrl || 'http://localhost:8545';
            rpcInfo.style.display = 'block';
            instructions.style.display = 'block';
        }
    } catch (error) {
        // Error - show manual entry
        statusText.textContent = 'Error detecting proxy. Enter RPC URL manually:';
        rpcUrlInput.value = 'http://localhost:8545';
        rpcInfo.style.display = 'block';
        instructions.style.display = 'block';
        console.error('Discovery error:', error);
    }
});

// Handle copy button
copyBtn.addEventListener('click', async () => {
    const success = await MetaMaskHelper.copyToClipboard(rpcUrlInput.value);
    
    if (success) {
        const originalText = copyBtn.textContent;
        copyBtn.textContent = '✓ Copied!';
        copyBtn.classList.add('copied');
        
        setTimeout(() => {
            copyBtn.textContent = originalText;
            copyBtn.classList.remove('copied');
        }, 2000);
    }
});

// Auto-select text when clicking on the RPC URL input
rpcUrlInput.addEventListener('click', () => {
    rpcUrlInput.select();
});

// Handle Add to Trust Wallet button click
if (addToTrustWalletBtn) {
    addToTrustWalletBtn.addEventListener('click', async () => {
        // Show detection UI
        trustwalletStatus.style.display = 'block';
        trustStatusText.innerHTML = '<span class="spinner"></span> Detecting ToRPC proxy client...';
        
        try {
            // Query the discovery API
            const discovery = await TrustWalletHelper.queryProxyDiscovery();
            
            if (discovery.success) {
                // Success - show the detected RPC URL
                trustStatusText.textContent = '✓ ToRPC proxy detected!';
                trustRpcUrlInput.value = discovery.rpcUrl;
                trustRpcInfo.style.display = 'block';
                trustInstructions.style.display = 'block';
                
                
                // Setup deep link button
                const deepLink = TrustWalletHelper.generateDeepLink(discovery.rpcUrl);
                deepLinkBtn.href = deepLink;
                deepLinkBtn.style.display = 'inline-block';
                
                // Auto-select the text for easy copying
                trustRpcUrlInput.select();
            } else {
                // Failed to detect proxy
                trustStatusText.textContent = discovery.error || 'Failed to detect ToRPC proxy client';
                
                // Show manual configuration
                trustRpcUrlInput.value = discovery.fallbackUrl || 'http://localhost:9000';
                trustRpcInfo.style.display = 'block';
                trustInstructions.style.display = 'block';
            }
        } catch (error) {
            // Error during detection
            trustStatusText.textContent = 'Error detecting proxy. Please configure manually.';
            trustRpcUrlInput.value = 'http://localhost:9000';
            trustRpcInfo.style.display = 'block';
            trustInstructions.style.display = 'block';
            console.error('Discovery error:', error);
        }
    });
}

// Handle Trust Wallet copy button
if (trustCopyBtn) {
    trustCopyBtn.addEventListener('click', async () => {
        const success = await TrustWalletHelper.copyToClipboard(trustRpcUrlInput.value);
        
        if (success) {
            const originalText = trustCopyBtn.textContent;
            trustCopyBtn.textContent = '✓ Copied!';
            trustCopyBtn.classList.add('copied');
            
            setTimeout(() => {
                trustCopyBtn.textContent = originalText;
                trustCopyBtn.classList.remove('copied');
            }, 2000);
        }
    });
}

// Auto-select text when clicking on the Trust Wallet RPC URL input
if (trustRpcUrlInput) {
    trustRpcUrlInput.addEventListener('click', () => {
        trustRpcUrlInput.select();
    });
}

// Handle Add to Coinbase Wallet button click
if (addToCoinbaseBtn) {
    addToCoinbaseBtn.addEventListener('click', async () => {
        // Show detection UI
        coinbaseStatus.style.display = 'block';
        coinbaseStatusText.innerHTML = '<span class="spinner"></span> Detecting ToRPC proxy client...';
        
        try {
            // Query the discovery API
            const discovery = await CoinbaseWalletHelper.queryProxyDiscovery();
            const hasCoinbaseWallet = CoinbaseWalletHelper.isCoinbaseWalletInstalled();
            
            if (discovery.success) {
                // Success - show the detected RPC URL
                coinbaseStatusText.textContent = '✓ ToRPC proxy detected!';
                coinbaseRpcUrlInput.value = discovery.rpcUrl;
                coinbaseRpcInfo.style.display = 'block';
                coinbaseInstructions.style.display = 'block';
                
                // Show appropriate buttons
                if (hasCoinbaseWallet) {
                    coinbaseAddNetworkBtn.style.display = 'inline-block';
                    coinbaseAddNetworkBtn.dataset.rpcUrl = discovery.rpcUrl;
                }
                
                // Auto-select the text for easy copying
                coinbaseRpcUrlInput.select();
            } else {
                // Failed to detect proxy
                coinbaseStatusText.textContent = discovery.error || 'Failed to detect ToRPC proxy client';
                
                // Show manual configuration
                const fallbackUrl = discovery.fallbackUrl || 'http://localhost:9000';
                coinbaseRpcUrlInput.value = fallbackUrl;
                coinbaseRpcInfo.style.display = 'block';
                coinbaseInstructions.style.display = 'block';
                
                // Show buttons with fallback URL
                if (hasCoinbaseWallet) {
                    coinbaseAddNetworkBtn.style.display = 'inline-block';
                    coinbaseAddNetworkBtn.dataset.rpcUrl = fallbackUrl;
                }
            }
        } catch (error) {
            // Error during detection
            coinbaseStatusText.textContent = 'Error detecting proxy. Please configure manually.';
            const fallbackUrl = 'http://localhost:9000';
            coinbaseRpcUrlInput.value = fallbackUrl;
            coinbaseRpcInfo.style.display = 'block';
            coinbaseInstructions.style.display = 'block';
            
            // Show buttons with fallback URL
            if (CoinbaseWalletHelper.isCoinbaseWalletInstalled()) {
                coinbaseAddNetworkBtn.style.display = 'inline-block';
                coinbaseAddNetworkBtn.dataset.rpcUrl = fallbackUrl;
            }
            
            console.error('Discovery error:', error);
        }
    });
}

// Handle Coinbase Wallet copy button
if (coinbaseCopyBtn) {
    coinbaseCopyBtn.addEventListener('click', async () => {
        const success = await CoinbaseWalletHelper.copyToClipboard(coinbaseRpcUrlInput.value);
        
        if (success) {
            const originalText = coinbaseCopyBtn.textContent;
            coinbaseCopyBtn.textContent = '✓ Copied!';
            coinbaseCopyBtn.classList.add('copied');
            
            setTimeout(() => {
                coinbaseCopyBtn.textContent = originalText;
                coinbaseCopyBtn.classList.remove('copied');
            }, 2000);
        }
    });
}

// Handle Add Network button for Coinbase Wallet extension
if (coinbaseAddNetworkBtn) {
    coinbaseAddNetworkBtn.addEventListener('click', async () => {
        const rpcUrl = coinbaseAddNetworkBtn.dataset.rpcUrl;
        
        try {
            coinbaseAddNetworkBtn.disabled = true;
            coinbaseAddNetworkBtn.textContent = 'Adding network...';
            
            await CoinbaseWalletHelper.addNetwork(rpcUrl);
            
            coinbaseAddNetworkBtn.textContent = '✓ Network added!';
            coinbaseAddNetworkBtn.classList.add('success');
        } catch (error) {
            console.error('Error adding network:', error);
            coinbaseAddNetworkBtn.textContent = 'Failed to add network';
            coinbaseAddNetworkBtn.classList.add('error');
            
            // Reset button after 3 seconds
            setTimeout(() => {
                coinbaseAddNetworkBtn.disabled = false;
                coinbaseAddNetworkBtn.textContent = 'Add to Coinbase Wallet Extension';
                coinbaseAddNetworkBtn.classList.remove('error', 'success');
            }, 3000);
        }
    });
}

// Auto-select text when clicking on the Coinbase RPC URL input
if (coinbaseRpcUrlInput) {
    coinbaseRpcUrlInput.addEventListener('click', () => {
        coinbaseRpcUrlInput.select();
    });
}

// Handle Add to Rainbow button click
if (addToRainbowBtn) {
    addToRainbowBtn.addEventListener('click', async () => {
        // Show detection UI
        rainbowStatus.style.display = 'block';
        rainbowStatusText.innerHTML = '<span class="spinner"></span> Detecting ToRPC proxy client...';
        
        try {
            // Query the discovery API
            const discovery = await RainbowWalletHelper.queryProxyDiscovery();
            
            if (discovery.success) {
                // Success - show the detected RPC URL
                rainbowStatusText.textContent = '✓ ToRPC proxy detected!';
                rainbowRpcUrlInput.value = discovery.rpcUrl;
                rainbowRpcInfo.style.display = 'block';
                rainbowInstructions.style.display = 'block';
                
                // Auto-select the text for easy copying
                rainbowRpcUrlInput.select();
            } else {
                // Failed to detect proxy
                rainbowStatusText.textContent = discovery.error || 'Failed to detect ToRPC proxy client';
                
                // Show manual configuration
                rainbowRpcUrlInput.value = discovery.fallbackUrl || 'http://localhost:9000';
                rainbowRpcInfo.style.display = 'block';
                rainbowInstructions.style.display = 'block';
            }
        } catch (error) {
            // Error during detection
            rainbowStatusText.textContent = 'Error detecting proxy. Please configure manually.';
            rainbowRpcUrlInput.value = 'http://localhost:9000';
            rainbowRpcInfo.style.display = 'block';
            rainbowInstructions.style.display = 'block';
            console.error('Discovery error:', error);
        }
    });
}

// Handle Rainbow copy button
if (rainbowCopyBtn) {
    rainbowCopyBtn.addEventListener('click', async () => {
        const success = await RainbowWalletHelper.copyToClipboard(rainbowRpcUrlInput.value);
        
        if (success) {
            const originalText = rainbowCopyBtn.textContent;
            rainbowCopyBtn.textContent = '✓ Copied!';
            rainbowCopyBtn.classList.add('copied');
            
            setTimeout(() => {
                rainbowCopyBtn.textContent = originalText;
                rainbowCopyBtn.classList.remove('copied');
            }, 2000);
        }
    });
}

// Auto-select text when clicking on the Rainbow RPC URL input
if (rainbowRpcUrlInput) {
    rainbowRpcUrlInput.addEventListener('click', () => {
        rainbowRpcUrlInput.select();
    });
}

// Handle Add to Rabby button click
if (addToRabbyBtn) {
    addToRabbyBtn.addEventListener('click', async () => {
        // Check if Rabby is installed
        if (!RabbyWalletHelper.isRabbyInstalled()) {
            rabbyStatusText.textContent = 'Rabby Wallet is not installed. Please install Rabby Wallet first.';
            rabbyStatus.style.display = 'block';
            return;
        }
        
        // Show detection UI
        rabbyStatus.style.display = 'block';
        rabbyStatusText.innerHTML = '<span class="spinner"></span> Detecting ToRPC proxy client...';
        
        try {
            // Query the discovery API
            const discovery = await RabbyWalletHelper.queryProxyDiscovery();
            
            if (discovery.success) {
                // Success - show the detected RPC URL
                rabbyStatusText.textContent = '✓ ToRPC proxy detected!';
                rabbyRpcUrlInput.value = discovery.rpcUrl;
                rabbyRpcInfo.style.display = 'block';
                rabbyInstructions.style.display = 'block';
                
                // Show add network button
                rabbyAddNetworkBtn.style.display = 'inline-block';
                rabbyAddNetworkBtn.dataset.rpcUrl = discovery.rpcUrl;
                
                // Auto-select the text for easy copying
                rabbyRpcUrlInput.select();
            } else {
                // Failed to detect proxy
                rabbyStatusText.textContent = discovery.error || 'Failed to detect ToRPC proxy client';
                
                // Show manual configuration
                const fallbackUrl = discovery.fallbackUrl || 'http://localhost:9000';
                rabbyRpcUrlInput.value = fallbackUrl;
                rabbyRpcInfo.style.display = 'block';
                rabbyInstructions.style.display = 'block';
                
                // Show add network button with fallback URL
                rabbyAddNetworkBtn.style.display = 'inline-block';
                rabbyAddNetworkBtn.dataset.rpcUrl = fallbackUrl;
            }
        } catch (error) {
            // Error during detection
            rabbyStatusText.textContent = 'Error detecting proxy. Please configure manually.';
            const fallbackUrl = 'http://localhost:9000';
            rabbyRpcUrlInput.value = fallbackUrl;
            rabbyRpcInfo.style.display = 'block';
            rabbyInstructions.style.display = 'block';
            
            // Show add network button with fallback URL
            rabbyAddNetworkBtn.style.display = 'inline-block';
            rabbyAddNetworkBtn.dataset.rpcUrl = fallbackUrl;
            
            console.error('Discovery error:', error);
        }
    });
}

// Handle Rabby copy button
if (rabbyCopyBtn) {
    rabbyCopyBtn.addEventListener('click', async () => {
        const success = await RabbyWalletHelper.copyToClipboard(rabbyRpcUrlInput.value);
        
        if (success) {
            const originalText = rabbyCopyBtn.textContent;
            rabbyCopyBtn.textContent = '✓ Copied!';
            rabbyCopyBtn.classList.add('copied');
            
            setTimeout(() => {
                rabbyCopyBtn.textContent = originalText;
                rabbyCopyBtn.classList.remove('copied');
            }, 2000);
        }
    });
}

// Handle Add Network button for Rabby Wallet
if (rabbyAddNetworkBtn) {
    rabbyAddNetworkBtn.addEventListener('click', async () => {
        const rpcUrl = rabbyAddNetworkBtn.dataset.rpcUrl;
        
        try {
            rabbyAddNetworkBtn.disabled = true;
            rabbyAddNetworkBtn.textContent = 'Adding network...';
            
            await RabbyWalletHelper.addNetwork(rpcUrl);
            
            rabbyAddNetworkBtn.textContent = '✓ Network added!';
            rabbyAddNetworkBtn.classList.add('success');
        } catch (error) {
            console.error('Error adding network:', error);
            rabbyAddNetworkBtn.textContent = 'Failed to add network';
            rabbyAddNetworkBtn.classList.add('error');
            
            // Reset button after 3 seconds
            setTimeout(() => {
                rabbyAddNetworkBtn.disabled = false;
                rabbyAddNetworkBtn.textContent = 'Add to Rabby Wallet';
                rabbyAddNetworkBtn.classList.remove('error', 'success');
            }, 3000);
        }
    });
}

// Auto-select text when clicking on the Rabby RPC URL input
if (rabbyRpcUrlInput) {
    rabbyRpcUrlInput.addEventListener('click', () => {
        rabbyRpcUrlInput.select();
    });
}