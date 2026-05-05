// Coinbase Wallet helper — wallet-specific bits only.

(function () {
    "use strict";

    const CoinbaseWalletHelper = {
        /** True if any injected provider self-identifies as Coinbase Wallet. */
        isCoinbaseWalletInstalled() {
            if (typeof window.ethereum === "undefined") return false;
            return (
                window.ethereum.isCoinbaseWallet === true ||
                window.ethereum.selectedProvider?.isCoinbaseWallet === true ||
                (window.ethereum.providers || []).some((p) => p.isCoinbaseWallet === true)
            );
        },

        /** Returns the Coinbase Wallet provider instance, or null. */
        getCoinbaseWalletProvider() {
            if (typeof window.ethereum === "undefined") return null;
            const providers = window.ethereum.providers || [];
            const fromList = providers.find((p) => p.isCoinbaseWallet === true);
            if (fromList) return fromList;
            if (window.ethereum.selectedProvider?.isCoinbaseWallet === true) {
                return window.ethereum.selectedProvider;
            }
            if (window.ethereum.isCoinbaseWallet === true) return window.ethereum;
            return null;
        },

        /**
         * Add (or switch to) a custom EVM network in Coinbase Wallet via the
         * standard `wallet_addEthereumChain` flow.
         */
        // Default chainId is mainnet (1). The earlier default of 1337 (Geth
        // `--dev`) silently created a dev-chain entry that pointed at a
        // mainnet RPC URL, so every signature mismatched the chain the wallet
        // thought it was on. Operators running torpc against a non-mainnet
        // network must pass an explicit chainId.
        async addNetwork(rpcUrl, chainId = "1", networkName = "ToRPC Privacy Network") {
            const provider = this.getCoinbaseWalletProvider();
            if (!provider) throw new Error("Coinbase Wallet not found");
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
    };

    window.CoinbaseWalletHelper = CoinbaseWalletHelper;
})();
