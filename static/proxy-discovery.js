// Shared discovery + clipboard helpers used by every wallet integration.
//
// Phase 4 in the daemon now default-disables the discovery server and
// token-gates it when on. The web UI cannot read the per-launch token
// (filesystem-protected, mode 0600), so this client passes whatever
// `window.TorpcDiscoveryToken` is set to (intended for a future server-side
// inject) and falls back to plain `fetch` for the GUI/CLI flows that don't
// need cross-origin permission.

(function (root) {
    "use strict";

    // Default fallback proxy URL when discovery fails. Aligned to
    // `torpc-proxy-core/src/config.rs` default of port 8545. Previously the
    // helpers disagreed (MetaMask: 8545, the rest: 9000), so a user running
    // the default config saw inconsistent fallbacks per wallet.
    var DEFAULT_FALLBACK_URL = "http://localhost:8545";

    // Discovery endpoint defaults — overridden by `window.TorpcConfig` if a
    // future Phase 7 follow-up serves `/config.js` from the daemon.
    function discoveryUrl() {
        return (root.TorpcConfig && root.TorpcConfig.discoveryUrl)
            || "http://localhost:8081/api/discovery";
    }
    function discoveryTimeoutMs() {
        return (root.TorpcConfig && root.TorpcConfig.discoveryTimeoutMs) || 2000;
    }
    function discoveryToken() {
        return root.TorpcDiscoveryToken || "";
    }

    /**
     * Query the local proxy's discovery endpoint. Returns one of:
     *  - { success: true,  rpcUrl: "..." }
     *  - { success: false, error: "...", fallbackUrl: DEFAULT_FALLBACK_URL }
     */
    async function queryProxyDiscovery() {
        var controller = new AbortController();
        var timeoutId = setTimeout(function () { controller.abort(); }, discoveryTimeoutMs());

        try {
            var headers = { "Accept": "application/json" };
            var token = discoveryToken();
            if (token) headers["X-Torpc-Token"] = token;

            var response = await fetch(discoveryUrl(), {
                method: "GET",
                mode: "cors",
                headers: headers,
                signal: controller.signal,
            });
            clearTimeout(timeoutId);

            if (response.status === 401) {
                return {
                    success: false,
                    error: "Discovery server requires X-Torpc-Token (set window.TorpcDiscoveryToken first)",
                    fallbackUrl: DEFAULT_FALLBACK_URL,
                };
            }
            if (!response.ok) {
                return {
                    success: false,
                    error: "HTTP " + response.status,
                    fallbackUrl: DEFAULT_FALLBACK_URL,
                };
            }

            var data = await response.json();
            var rpc = data.suggested_rpc_url
                || (data.proxy && "http://" + data.proxy.listen_addr)
                || DEFAULT_FALLBACK_URL;

            return { success: true, data: data, rpcUrl: rpc };
        } catch (e) {
            clearTimeout(timeoutId);
            var msg = (e && e.name === "AbortError")
                ? "Discovery timeout"
                : "ToRPC proxy not detected";
            return { success: false, error: msg, fallbackUrl: DEFAULT_FALLBACK_URL };
        }
    }

    /** Copy a string to the clipboard, with a fallback for non-secure contexts. */
    async function copyToClipboard(text) {
        try {
            await navigator.clipboard.writeText(text);
            return true;
        } catch (_) {
            var ta = document.createElement("textarea");
            ta.value = text;
            ta.style.position = "fixed";
            ta.style.left = "-9999px";
            document.body.appendChild(ta);
            ta.select();
            var ok = document.execCommand("copy");
            document.body.removeChild(ta);
            return ok;
        }
    }

    root.ProxyDiscovery = {
        queryProxyDiscovery: queryProxyDiscovery,
        copyToClipboard: copyToClipboard,
        DEFAULT_FALLBACK_URL: DEFAULT_FALLBACK_URL,
    };
})(typeof window !== "undefined" ? window : this);
