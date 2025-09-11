// MetaMask Helper - Auto-detection for ToRPC Proxy

const MetaMaskHelper = {
    // Query the proxy discovery API
    async queryProxyDiscovery() {
        const discoveryUrl = 'http://localhost:8081/api/discovery';
        
        try {
            const controller = new AbortController();
            const timeoutId = setTimeout(() => controller.abort(), 2000);
            
            const response = await fetch(discoveryUrl, {
                method: 'GET',
                mode: 'cors',
                headers: {
                    'Accept': 'application/json',
                },
                signal: controller.signal
            });
            
            clearTimeout(timeoutId);
            
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            
            const data = await response.json();
            
            return {
                success: true,
                data: data,
                rpcUrl: data.suggested_rpc_url || `http://${data.proxy.listen_addr}`
            };
        } catch (error) {
            // Check if it's a timeout or connection error
            if (error.name === 'AbortError' || error.message.includes('Failed to fetch')) {
                return {
                    success: false,
                    error: 'ToRPC proxy not detected',
                    message: 'Please ensure the ToRPC proxy client is running.',
                    fallbackUrl: 'http://localhost:8545'
                };
            }
            
            return {
                success: false,
                error: error.message,
                fallbackUrl: 'http://localhost:8545'
            };
        }
    },

    // Check if MetaMask is installed
    isInstalled() {
        return typeof window.ethereum !== 'undefined' && window.ethereum.isMetaMask;
    },

    // Copy text to clipboard
    async copyToClipboard(text) {
        try {
            await navigator.clipboard.writeText(text);
            return true;
        } catch (error) {
            // Fallback for older browsers
            const textArea = document.createElement('textarea');
            textArea.value = text;
            textArea.style.position = 'fixed';
            textArea.style.left = '-999999px';
            document.body.appendChild(textArea);
            textArea.select();
            const success = document.execCommand('copy');
            document.body.removeChild(textArea);
            return success;
        }
    },

    // Get current chain ID
    async getCurrentChainId() {
        if (!this.isInstalled()) {
            throw new Error('MetaMask is not installed');
        }
        
        try {
            const chainId = await window.ethereum.request({ method: 'eth_chainId' });
            return chainId;
        } catch (error) {
            console.error('Error getting chain ID:', error);
            throw error;
        }
    },

    // Check if connected to mainnet
    async isMainnet() {
        const chainId = await this.getCurrentChainId();
        return chainId === '0x1'; // 0x1 is mainnet
    }
};

// Export for use in other scripts
window.MetaMaskHelper = MetaMaskHelper;