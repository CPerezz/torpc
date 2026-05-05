// MetaMask helper — wallet-specific bits only.

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
    };

    window.MetaMaskHelper = MetaMaskHelper;
})();
