// Rabby Wallet helper — wallet-specific bits only.

(function () {
    "use strict";

    const RabbyWalletHelper = {
        /** True when Rabby is the injected provider (or a coinjected one). */
        isRabbyInstalled() {
            return (
                typeof window.rabby !== "undefined" ||
                (typeof window.ethereum !== "undefined" && !!window.ethereum.isRabby)
            );
        },

        getRabbyProvider() {
            if (typeof window.rabby !== "undefined") return window.rabby;
            if (typeof window.ethereum !== "undefined" && window.ethereum.isRabby) {
                return window.ethereum;
            }
            return null;
        },

        async addNetwork(rpcUrl, chainId = "1337", networkName = "ToRPC Privacy Network") {
            const provider = this.getRabbyProvider();
            if (!provider) throw new Error("Rabby Wallet not found");
            const chainHex = `0x${parseInt(chainId, 10).toString(16)}`;
            try {
                await provider.request({
                    method: "wallet_addEthereumChain",
                    params: [{
                        chainId: chainHex,
                        chainName: networkName,
                        nativeCurrency: { name: "Ethereum", symbol: "ETH", decimals: 18 },
                        rpcUrls: [rpcUrl],
                        blockExplorerUrls: null,
                    }],
                });
                return true;
            } catch (error) {
                if (error && error.code === 4902) {
                    await provider.request({
                        method: "wallet_switchEthereumChain",
                        params: [{ chainId: chainHex }],
                    });
                    return true;
                }
                throw error;
            }
        },

        // Compatibility shims — use `ProxyDiscovery` directly in new code.
        queryProxyDiscovery: () => window.ProxyDiscovery.queryProxyDiscovery(),
        copyToClipboard: (t) => window.ProxyDiscovery.copyToClipboard(t),
    };

    window.RabbyWalletHelper = RabbyWalletHelper;
})();
