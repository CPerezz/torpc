// MetaMask helper — wallet-specific bits only.
// Discovery and clipboard now live in `proxy-discovery.js` to avoid the
// near-identical 50-line copy in every wallet helper.

(function () {
    "use strict";

    const MetaMaskHelper = {
        /** True when the MetaMask extension is the active provider. */
        isInstalled() {
            return typeof window.ethereum !== "undefined" && !!window.ethereum.isMetaMask;
        },

        /** Resolves to the current `eth_chainId` hex string. */
        async getCurrentChainId() {
            if (!this.isInstalled()) throw new Error("MetaMask is not installed");
            return await window.ethereum.request({ method: "eth_chainId" });
        },

        async isMainnet() {
            return (await this.getCurrentChainId()) === "0x1";
        },

        // Compatibility shims for code that still calls these on the helper
        // directly. New callers should use `ProxyDiscovery.*` instead.
        queryProxyDiscovery: () => window.ProxyDiscovery.queryProxyDiscovery(),
        copyToClipboard: (t) => window.ProxyDiscovery.copyToClipboard(t),
    };

    window.MetaMaskHelper = MetaMaskHelper;
})();
