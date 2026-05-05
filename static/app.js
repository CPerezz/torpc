// ToRPC frontend.
//
// Phase-7 follow-up: the per-wallet event handlers used to be ~620 lines of
// near-identical copy-paste (one block per wallet). They're now collapsed
// into a single `WALLETS` config array consumed by `wireWallet()`. Adding
// a new wallet is now ~15 lines instead of ~120.

(function () {
    "use strict";

    // ---------------------------------------------------------------------
    // Shared "Test RPC Connection" panel — works the same for every backend.
    // ---------------------------------------------------------------------

    const methodSelect = document.getElementById("method");
    const paramsSection = document.getElementById("params-section");
    const paramsInput = document.getElementById("params");
    const testBtn = document.getElementById("test-btn");
    const requestDisplay = document.getElementById("request-display");
    const responseDisplay = document.getElementById("response-display");
    const statusElement = document.getElementById("status");
    const torInfo = document.getElementById("tor-info");

    window.addEventListener("DOMContentLoaded", () => {
        checkStatus();
        setupMethodSelector();
        WALLETS.forEach(wireWallet);
    });

    async function checkStatus() {
        try {
            const response = await fetch("/rpc", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    jsonrpc: "2.0",
                    method: "net_version",
                    params: [],
                    id: 1,
                }),
            });
            statusElement.textContent = response.ok ? "Online" : "Offline";
            statusElement.className = response.ok ? "status-online" : "status-offline";
        } catch (_) {
            statusElement.textContent = "Offline";
            statusElement.className = "status-offline";
        }
        if (torInfo) {
            torInfo.textContent =
                "To use through Tor:\n" +
                "1. Run: ./scripts/start-tor.sh\n" +
                "2. Check: data/tor/torpc/hostname for your .onion address\n" +
                "3. Connect via: torsocks curl http://your-address.onion/rpc";
        }
    }

    function setupMethodSelector() {
        if (!methodSelect) return;
        methodSelect.addEventListener("change", () => {
            if (methodSelect.value === "eth_getBalance") {
                paramsSection.style.display = "block";
                paramsInput.placeholder =
                    '["0x742d35Cc6634C0532925a3b844Bc9e7595f7777", "latest"]';
            } else {
                paramsSection.style.display = "none";
                paramsInput.value = "";
            }
        });
    }

    if (testBtn) {
        testBtn.addEventListener("click", async () => {
            const method = methodSelect.value;
            const endpoint = document.querySelector('input[name="endpoint"]:checked').value;
            let params = [];
            if (paramsInput.value) {
                try { params = JSON.parse(paramsInput.value); }
                catch (e) { alert("Invalid JSON in parameters"); return; }
            }
            const request = { jsonrpc: "2.0", method, params, id: Date.now() };
            requestDisplay.textContent = JSON.stringify(request, null, 2);
            responseDisplay.textContent = "Sending...";
            try {
                const response = await fetch(endpoint, {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify(request),
                });
                const data = await response.json();
                responseDisplay.textContent = JSON.stringify(data, null, 2);
                responseDisplay.style.borderColor = data.error ? "#e74c3c" : "#27ae60";
            } catch (error) {
                responseDisplay.textContent = `Error: ${error.message}`;
                responseDisplay.style.borderColor = "#e74c3c";
            }
        });
    }

    document.querySelectorAll(".code-block").forEach((block) => {
        block.addEventListener("click", function () {
            if (this.textContent && this.textContent !== "Sending...") {
                navigator.clipboard.writeText(this.textContent).then(() => {
                    const original = this.style.borderColor;
                    this.style.borderColor = "#27ae60";
                    setTimeout(() => { this.style.borderColor = original; }, 500);
                });
            }
        });
    });

    if (window.location.hostname !== "localhost" &&
        window.location.hostname !== "127.0.0.1") {
        const fullRpcUrl = document.getElementById("full-rpc-url");
        if (fullRpcUrl) {
            fullRpcUrl.textContent =
                `${window.location.protocol}//${window.location.host}/rpc`;
        }
    }

    // ---------------------------------------------------------------------
    // Wallet adapter pattern.
    //
    // Each entry describes the DOM elements a wallet section uses and the
    // wallet-specific extras to run on success/failure. `wireWallet`
    // hooks up the standard discovery → display → copy flow; per-wallet
    // hooks deal with deep links, "Add network" buttons, and so on.
    // ---------------------------------------------------------------------

    const FALLBACK_URL = (window.ProxyDiscovery && window.ProxyDiscovery.DEFAULT_FALLBACK_URL)
        || "http://localhost:8545";

    const WALLETS = [
        {
            id: "metamask",
            els: {
                addBtn: "add-to-metamask",
                statusContainer: "metamask-status",
                statusText: "status-text",
                rpcInfo: "rpc-info",
                rpcInput: "rpc-url",
                copyBtn: "copy-btn",
                instructions: "instructions",
            },
            isInstalled: () => window.MetaMaskHelper && window.MetaMaskHelper.isInstalled(),
            notInstalledMessage: "MetaMask is not installed. Please install MetaMask first.",
        },
        {
            id: "trustwallet",
            els: {
                addBtn: "add-to-trustwallet",
                statusContainer: "trustwallet-status",
                statusText: "trust-status-text",
                rpcInfo: "trust-rpc-info",
                rpcInput: "trust-rpc-url",
                copyBtn: "trust-copy-btn",
                instructions: "trust-instructions",
                deepLinkBtn: "trust-deeplink-btn",
            },
            onShown: (rpcUrl, els) => {
                if (els.deepLinkBtn && window.TrustWalletHelper) {
                    els.deepLinkBtn.href = window.TrustWalletHelper.generateDeepLink(rpcUrl);
                    els.deepLinkBtn.style.display = "inline-block";
                }
            },
        },
        {
            id: "coinbase",
            els: {
                addBtn: "add-to-coinbase",
                statusContainer: "coinbase-status",
                statusText: "coinbase-status-text",
                rpcInfo: "coinbase-rpc-info",
                rpcInput: "coinbase-rpc-url",
                copyBtn: "coinbase-copy-btn",
                instructions: "coinbase-instructions",
                addNetworkBtn: "coinbase-add-network-btn",
            },
            // Coinbase exposes a programmatic addNetwork only when the
            // extension is present; show the "Add network" button in either
            // case (success or fallback) so the UI is consistent.
            onShown: (rpcUrl, els) => {
                if (els.addNetworkBtn && window.CoinbaseWalletHelper?.isCoinbaseWalletInstalled()) {
                    els.addNetworkBtn.style.display = "inline-block";
                    els.addNetworkBtn.dataset.rpcUrl = rpcUrl;
                }
            },
            wireExtras: (els) => {
                if (!els.addNetworkBtn || !window.CoinbaseWalletHelper) return;
                els.addNetworkBtn.addEventListener("click", () =>
                    runAddNetwork(
                        els.addNetworkBtn,
                        (url) => window.CoinbaseWalletHelper.addNetwork(url),
                        "Add to Coinbase Wallet Extension"
                    )
                );
            },
        },
        {
            id: "rainbow",
            els: {
                addBtn: "add-to-rainbow",
                statusContainer: "rainbow-status",
                statusText: "rainbow-status-text",
                rpcInfo: "rainbow-rpc-info",
                rpcInput: "rainbow-rpc-url",
                copyBtn: "rainbow-copy-btn",
                instructions: "rainbow-instructions",
            },
        },
        {
            id: "rabby",
            els: {
                addBtn: "add-to-rabby",
                statusContainer: "rabby-status",
                statusText: "rabby-status-text",
                rpcInfo: "rabby-rpc-info",
                rpcInput: "rabby-rpc-url",
                copyBtn: "rabby-copy-btn",
                instructions: "rabby-instructions",
                addNetworkBtn: "rabby-add-network-btn",
            },
            isInstalled: () => window.RabbyWalletHelper && window.RabbyWalletHelper.isRabbyInstalled(),
            notInstalledMessage: "Rabby Wallet is not installed. Please install Rabby Wallet first.",
            onShown: (rpcUrl, els) => {
                if (els.addNetworkBtn) {
                    els.addNetworkBtn.style.display = "inline-block";
                    els.addNetworkBtn.dataset.rpcUrl = rpcUrl;
                }
            },
            wireExtras: (els) => {
                if (!els.addNetworkBtn || !window.RabbyWalletHelper) return;
                els.addNetworkBtn.addEventListener("click", () =>
                    runAddNetwork(
                        els.addNetworkBtn,
                        (url) => window.RabbyWalletHelper.addNetwork(url),
                        "Add to Rabby Wallet"
                    )
                );
            },
        },
    ];

    function resolveEls(map) {
        const out = {};
        for (const [logical, domId] of Object.entries(map)) {
            out[logical] = document.getElementById(domId);
        }
        return out;
    }

    function wireWallet(cfg) {
        const els = resolveEls(cfg.els);
        if (!els.addBtn) return; // section absent from this page

        els.addBtn.addEventListener("click", () => handleAdd(cfg, els));

        if (els.copyBtn && els.rpcInput) {
            els.copyBtn.addEventListener("click", async () => {
                const ok = await window.ProxyDiscovery.copyToClipboard(els.rpcInput.value);
                if (ok) flashCopied(els.copyBtn);
            });
        }

        if (els.rpcInput) {
            els.rpcInput.addEventListener("click", () => els.rpcInput.select());
        }

        if (typeof cfg.wireExtras === "function") cfg.wireExtras(els);
    }

    async function handleAdd(cfg, els) {
        if (typeof cfg.isInstalled === "function" && !cfg.isInstalled()) {
            if (els.statusText) els.statusText.textContent = cfg.notInstalledMessage || "";
            if (els.statusContainer) els.statusContainer.style.display = "block";
            return;
        }

        if (els.statusContainer) els.statusContainer.style.display = "block";
        if (els.statusText) {
            els.statusText.innerHTML =
                '<span class="spinner"></span> Detecting ToRPC proxy client...';
        }

        let discovery;
        try {
            discovery = await window.ProxyDiscovery.queryProxyDiscovery();
        } catch (e) {
            console.error("[" + cfg.id + "] discovery error:", e);
            discovery = { success: false, error: "Error detecting proxy", fallbackUrl: FALLBACK_URL };
        }

        const rpcUrl = discovery.success
            ? discovery.rpcUrl
            : (discovery.fallbackUrl || FALLBACK_URL);

        if (els.statusText) {
            els.statusText.textContent = discovery.success
                ? "✓ ToRPC proxy detected!"
                : (discovery.error || "Failed to detect ToRPC proxy client");
        }
        if (els.rpcInput) els.rpcInput.value = rpcUrl;
        if (els.rpcInfo) els.rpcInfo.style.display = "block";
        if (els.instructions) els.instructions.style.display = "block";

        if (typeof cfg.onShown === "function") cfg.onShown(rpcUrl, els);
        if (els.rpcInput) els.rpcInput.select();
    }

    function flashCopied(btn) {
        const original = btn.textContent;
        btn.textContent = "✓ Copied!";
        btn.classList.add("copied");
        setTimeout(() => {
            btn.textContent = original;
            btn.classList.remove("copied");
        }, 2000);
    }

    /**
     * Runs an "Add network" action with the visible state machine
     * (disabling the button, showing a transient success/failure label,
     * then reverting). Used by Coinbase and Rabby — both expose the same
     * `wallet_addEthereumChain` flow under a slightly different helper API.
     */
    async function runAddNetwork(btn, addFn, restoreText) {
        const url = btn.dataset.rpcUrl;
        try {
            btn.disabled = true;
            btn.textContent = "Adding network...";
            await addFn(url);
            btn.textContent = "✓ Network added!";
            btn.classList.add("success");
        } catch (e) {
            console.error("addNetwork error:", e);
            btn.textContent = "Failed to add network";
            btn.classList.add("error");
            setTimeout(() => {
                btn.disabled = false;
                btn.textContent = restoreText;
                btn.classList.remove("error", "success");
            }, 3000);
        }
    }
})();
