// Trust Wallet helper — deep-link generation and platform detection.
// Trust Wallet doesn't expose a programmatic `addEthereumChain` flow on the
// browser side, so the integration relies on a universal/deep link plus
// manual setup instructions (rendered by `app.js`).

(function () {
    "use strict";

    const TrustWalletHelper = {
        /**
         * Build the universal link that opens Trust Wallet's "Add custom
         * network" view with the supplied parameters pre-filled. Both mobile
         * and desktop browsers can navigate to this URL.
         */
        generateDeepLink(rpcUrl, chainId = "1") {
            const params = new URLSearchParams({
                network: "ToRPC Privacy Network",
                rpcUrl,
                chainId,
                symbol: "ETH",
            });
            return `https://link.trustwallet.com/add_network?${params.toString()}`;
        },

        /** Serialize a WalletConnect-shaped network configuration descriptor. */
        generateWalletConnectURI(rpcUrl, chainId = "1") {
            return JSON.stringify({
                name: "ToRPC Privacy Network",
                rpcUrl,
                chainId: parseInt(chainId, 10),
                nativeCurrency: { name: "Ethereum", symbol: "ETH", decimals: 18 },
            });
        },

        /** True inside Trust Wallet's in-app browser. */
        isTrustWalletBrowser() {
            return typeof window.ethereum !== "undefined" && window.ethereum.isTrust === true;
        },

        isMobileDevice() {
            return /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(
                navigator.userAgent
            );
        },

        openTrustWallet(deepLink) {
            if (this.isMobileDevice()) {
                window.location.href = deepLink;
            } else {
                window.open(deepLink, "_blank");
            }
        },

        // Compatibility shims — use `ProxyDiscovery` directly in new code.
        queryProxyDiscovery: () => window.ProxyDiscovery.queryProxyDiscovery(),
        copyToClipboard: (t) => window.ProxyDiscovery.copyToClipboard(t),
    };

    window.TrustWalletHelper = TrustWalletHelper;
})();
