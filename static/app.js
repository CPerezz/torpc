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
        // Each setup function early-returns when its DOM isn't present, so a
        // single app.js drives both `index_operator.html` (test panel +
        // self-test wallet sections) and `index_user.html` (wallet picker
        // gated by a localhost client probe).
        checkStatus();
        setupMethodSelector();
        setupClientProbe();
        setupWalletPicker();
        WALLETS.forEach(wireWallet);
    });

    async function checkStatus() {
        if (!statusElement) return; // operator template only
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

    // ---------------------------------------------------------------------
    // User template: probe the local TorPC client and gate Step 2.
    //
    // The user template (`index_user.html`) is served when the visitor
    // reaches the daemon via .onion. They don't have a co-located proxy at
    // localhost:8545 unless they've installed the client. Probing it tells
    // them whether they're ready for Step 2 without waiting for a wallet
    // request to silently fail.
    //
    // Tor Browser at the Safest security level disables JS — for those
    // users the `<noscript>` block in the template shows manual setup
    // instructions instead of this probe.
    // ---------------------------------------------------------------------
    async function setupClientProbe() {
        const statusEl = document.getElementById("client-status");
        const step2 = document.getElementById("step-2");
        if (!statusEl || !step2) return; // operator template

        const reachable = await probeLocalClient();
        if (reachable) {
            statusEl.classList.remove("callout-pending");
            statusEl.classList.add("callout-success");
            statusEl.innerHTML =
                "<span aria-hidden=\"true\">✓</span> Local client detected. " +
                "Continue to Step 2 below.";
            step2.hidden = false;
        } else {
            statusEl.classList.remove("callout-pending");
            statusEl.classList.add("callout-warn");
            statusEl.innerHTML =
                "<span aria-hidden=\"true\">⚠</span> No client at " +
                "<code>127.0.0.1:8545</code>. Install above, run it, then " +
                "<a href=\"\" onclick=\"location.reload(); return false;\">retry</a>.";
        }
    }

    /**
     * Returns true iff a TorPC client is responding on localhost:8545. Sends
     * a dummy `net_version` JSON-RPC request and accepts any 2xx with a
     * JSON-shaped body — we don't care what the upstream chain is, only
     * that *something* is bridging to a real RPC.
     *
     * Times out at 1.5s so the page renders fast even when localhost rejects
     * the connection on first try (some firewalls are slow to RST).
     */
    async function probeLocalClient() {
        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), 1500);
        try {
            const resp = await fetch("http://127.0.0.1:8545", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    jsonrpc: "2.0",
                    method: "net_version",
                    params: [],
                    id: 1,
                }),
                signal: controller.signal,
                // No credentials, no cache; pure probe.
                credentials: "omit",
                cache: "no-store",
            });
            clearTimeout(timeout);
            if (!resp.ok) return false;
            const data = await resp.json();
            return data && (data.result !== undefined || data.error !== undefined);
        } catch (_) {
            clearTimeout(timeout);
            return false;
        }
    }

    // ---------------------------------------------------------------------
    // User template: wallet picker.
    //
    // Step 2 of the user template hides all wallet flows by default and
    // reveals one when a `.wallet-card` is clicked. The flows themselves
    // use the same DOM IDs as the operator template's self-test sections,
    // so `WALLETS.forEach(wireWallet)` wires both up identically.
    // ---------------------------------------------------------------------
    function setupWalletPicker() {
        const cards = document.querySelectorAll(".wallet-card");
        if (cards.length === 0) return; // operator template

        cards.forEach((card) => {
            card.addEventListener("click", () => {
                const id = card.dataset.wallet;
                cards.forEach((c) =>
                    c.classList.toggle("active", c === card)
                );
                document.querySelectorAll(".wallet-flow").forEach((flow) => {
                    flow.hidden = flow.dataset.wallet !== id;
                });
            });
        });
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

    // Hardcoded RPC URL the wallet sections show to the user. Used to be
    // the result of an HTTP discovery call to the local proxy on
    // localhost:8081, but that endpoint is default-disabled, the static
    // CSP no longer permits the cross-origin fetch, and the helpers were
    // already falling through to this same literal whenever discovery
    // failed (which was always). The discovery indirection was deleted.
    const RPC_URL = "http://localhost:8545";

    /**
     * Copy a string to the clipboard, with a fallback for non-secure
     * contexts (e.g. plain `http://onion-host` over Tor without TLS,
     * where `navigator.clipboard` is gated).
     */
    async function copyToClipboard(text) {
        try {
            await navigator.clipboard.writeText(text);
            return true;
        } catch (_) {
            const ta = document.createElement("textarea");
            ta.value = text;
            ta.style.position = "fixed";
            ta.style.left = "-9999px";
            document.body.appendChild(ta);
            ta.select();
            const ok = document.execCommand("copy");
            document.body.removeChild(ta);
            return ok;
        }
    }

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
        // Rainbow had a section in the previous index.html but no helper
        // file, no installed-detection, and no addNetwork flow — clicking
        // the button just showed the same RPC URL as MetaMask. Removed.
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
                const ok = await copyToClipboard(els.rpcInput.value);
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

        // No discovery call — show the configured RPC URL directly. Users
        // running the proxy on a non-default port edit RPC_URL above.
        if (els.statusContainer) els.statusContainer.style.display = "block";
        if (els.statusText) {
            els.statusText.textContent = "Use the RPC URL below in your wallet:";
        }
        const rpcUrl = RPC_URL;
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
