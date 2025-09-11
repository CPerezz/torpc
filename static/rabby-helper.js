// Rabby Wallet Helper - Integration for ToRPC Proxy

const RabbyWalletHelper = {
    // Query the proxy discovery API (same as other wallets)
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
            if (error.name === 'AbortError' || error.message.includes('Failed to fetch')) {
                return {
                    success: false,
                    error: 'ToRPC proxy not detected',
                    message: 'Please ensure the ToRPC proxy client is running.',
                    fallbackUrl: 'http://localhost:9000'
                };
            }
            
            return {
                success: false,
                error: error.message,
                fallbackUrl: 'http://localhost:9000'
            };
        }
    },

    // Check if Rabby extension is installed
    isRabbyInstalled() {
        // Rabby injects itself as window.rabby
        return typeof window.rabby !== 'undefined';
    },

    // Get Rabby provider
    getRabbyProvider() {
        if (typeof window.rabby !== 'undefined') {
            return window.rabby;
        }
        
        // Check if ethereum provider is Rabby
        if (typeof window.ethereum !== 'undefined' && window.ethereum.isRabby) {
            return window.ethereum;
        }
        
        return null;
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

    // Add custom network to Rabby
    async addNetwork(rpcUrl, chainId = '1337', networkName = 'ToRPC Privacy Network') {
        const provider = this.getRabbyProvider();
        if (!provider) {
            throw new Error('Rabby Wallet not found');
        }

        try {
            // Rabby uses the same wallet_addEthereumChain method
            await provider.request({
                method: 'wallet_addEthereumChain',
                params: [{
                    chainId: `0x${parseInt(chainId).toString(16)}`,
                    chainName: networkName,
                    nativeCurrency: {
                        name: 'Ethereum',
                        symbol: 'ETH',
                        decimals: 18
                    },
                    rpcUrls: [rpcUrl],
                    blockExplorerUrls: null
                }]
            });
            return true;
        } catch (error) {
            // If error is because chain already exists, try to switch to it
            if (error.code === 4902) {
                try {
                    await provider.request({
                        method: 'wallet_switchEthereumChain',
                        params: [{ chainId: `0x${parseInt(chainId).toString(16)}` }]
                    });
                    return true;
                } catch (switchError) {
                    console.error('Error switching chain:', switchError);
                    throw switchError;
                }
            }
            throw error;
        }
    }
};

// Export for use in other scripts
window.RabbyWalletHelper = RabbyWalletHelper;