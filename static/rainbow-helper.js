// Rainbow Wallet Helper - Integration for ToRPC Proxy

const RainbowWalletHelper = {
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
    }
};

// Export for use in other scripts
window.RainbowWalletHelper = RainbowWalletHelper;