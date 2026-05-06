// Tauri 2.x global API entry points (enabled by `app.withGlobalTauri: true`
// in tauri.conf.json). The 1.x layout (`window.__TAURI__.tauri.invoke`,
// `window.__TAURI__.window.appWindow`) was reorganized; `core` holds
// `invoke`. The window handle is fetched on-demand by the (currently
// nonexistent) callers that need it; the OS close-button is what hides
// the window into the tray today.
if (!window.__TAURI__) {
    console.error('Tauri API not available! Make sure you are running this in a Tauri desktop app.');
    alert('Error: This application must be run as a Tauri desktop app.');
}

const { invoke } = window.__TAURI__.core;

// DOM elements
const statusDot = document.getElementById('statusDot');
const statusText = document.getElementById('statusText');
const configForm = document.getElementById('configForm');
const toastContainer = document.getElementById('toastContainer');
const onionEndpointInput = document.getElementById('onionEndpoint');
const listenPortInput = document.getElementById('listenPort');
const torProxyInput = document.getElementById('torProxy');
const startBtn = document.getElementById('startBtn');
const stopBtn = document.getElementById('stopBtn');
const proxyInfo = document.getElementById('proxyInfo');
const proxyUrl = document.getElementById('proxyUrl');
const walletRpcUrl = document.getElementById('walletRpcUrl');

// Update status display.
//
// `status` is now structured (per the serde-tagged enum on the Rust side):
//   { "state": "Running" } / { "state": "Error", "message": "..." }
// Pre-PR-1 the GUI did `status.includes('Running')` against
// `format!("{:?}")` output — fragile, fragile, fragile. Now we pattern-
// match on `status.state`.
async function updateStatus() {
    try {
        const status = await invoke('get_status');
        const config = await invoke('get_config');

        statusDot.classList.remove('running', 'stopped', 'starting', 'stopping');

        switch (status.state) {
            case 'Running': {
                statusDot.classList.add('running');
                statusText.textContent = 'Proxy is running';
                startBtn.style.display = 'none';
                stopBtn.style.display = 'block';
                proxyInfo.style.display = 'block';
                const port = config.listen_addr.split(':').pop();
                const url = `http://localhost:${port}`;
                proxyUrl.textContent = url;
                walletRpcUrl.textContent = url;
                break;
            }
            case 'Stopped': {
                statusDot.classList.add('stopped');
                statusText.textContent = 'Proxy is stopped';
                startBtn.style.display = 'block';
                stopBtn.style.display = 'none';
                proxyInfo.style.display = 'none';
                break;
            }
            case 'Starting': {
                statusDot.classList.add('starting');
                statusText.textContent = 'Starting proxy...';
                startBtn.style.display = 'none';
                stopBtn.style.display = 'none';
                proxyInfo.style.display = 'none';
                break;
            }
            case 'Stopping': {
                statusDot.classList.add('stopping');
                statusText.textContent = 'Stopping proxy...';
                startBtn.style.display = 'none';
                stopBtn.style.display = 'none';
                proxyInfo.style.display = 'none';
                break;
            }
            case 'Error': {
                statusDot.classList.add('stopped');
                statusText.textContent = `Error: ${status.message ?? 'Unknown error'}`;
                startBtn.style.display = 'block';
                stopBtn.style.display = 'none';
                proxyInfo.style.display = 'none';
                break;
            }
            default: {
                console.warn('Unknown ProxyStatus state:', status);
                statusText.textContent = 'Unknown';
                statusDot.classList.add('stopped');
            }
        }
    } catch (error) {
        console.error('Failed to get status:', error);
        statusText.textContent = 'Unknown';
        statusDot.classList.add('stopped');
    }
}

async function loadConfig() {
    try {
        const config = await invoke('get_config');

        const listenPort = config.listen_addr.split(':').pop();
        listenPortInput.value = listenPort;

        const torProxyParts = config.tor_proxy.split(':');
        if (torProxyParts.length >= 2) {
            const torProxyPort = torProxyParts[torProxyParts.length - 1];
            const torProxyHost = torProxyParts.slice(0, -1).join(':');
            torProxyInput.value = `${torProxyHost}:${torProxyPort}`;
        }

        // Empty string is the new default — let the HTML `placeholder`
        // attr suggest the format rather than seeding a misleading
        // "placeholder.onion:80" the proxy will then chase.
        onionEndpointInput.value = config.onion_endpoint || '';
    } catch (error) {
        console.error('Failed to load config:', error);
        showToast('Failed to load configuration', 'error');
    }
}

// Tor v3 onion: 56-char base32 (a-z, 2-7) + ".onion", optional ":port".
// Pre-fix this used `.includes('.onion')`, which accepted "example.onion"
// and any other string containing those eight characters. The proxy then
// happily started its TCP listener (status went green) but every wallet
// request failed at the SOCKS5 layer with "invalid address format" — a
// confusing first-run experience for new operators.
const ONION_REGEX = /^[a-z2-7]{56}\.onion(:\d{1,5})?$/i;

async function saveConfig(event) {
    event.preventDefault();

    const rawOnion = onionEndpointInput.value.trim();
    const listenPort = parseInt(listenPortInput.value, 10);
    const torProxy = torProxyInput.value.trim();

    if (!rawOnion) {
        showToast('Onion endpoint is required', 'error');
        return;
    }

    if (!ONION_REGEX.test(rawOnion)) {
        showToast(
            'Invalid onion endpoint. Expected 56 base32 characters (a-z, 2-7) ' +
            'followed by ".onion" and an optional ":port" (e.g. ' +
            'pxglb4naobvtpmnnvmsqmqgmfougct7en7xfqdevao6nrkmgmgrzopid.onion:80).',
            'error',
            { duration: 8000 } // longer because the message is dense
        );
        return;
    }

    // Auto-append :80 if no port specified — Tor's default for HTTP-over-onion.
    // tokio_socks rejects address-without-port at SOCKS5 connect time, so we
    // can't rely on the proxy to deal with this for us.
    const onionEndpoint = rawOnion.includes(':') ? rawOnion : `${rawOnion}:80`;

    const torProxyParts = torProxy.split(':');
    if (torProxyParts.length < 2) {
        showToast('Invalid Tor proxy format', 'error');
        return;
    }
    const torProxyPort = parseInt(torProxyParts[torProxyParts.length - 1], 10);
    const torProxyHost = torProxyParts.slice(0, -1).join(':') || torProxyParts[0];

    const config = {
        listen_addr: `127.0.0.1:${listenPort}`,
        tor_proxy: `${torProxyHost}:${torProxyPort}`,
        onion_endpoint: onionEndpoint
    };

    // Save persists + applies the new config to the controller. It does NOT
    // start the proxy. The user clicks "Start Proxy" explicitly so the
    // "Running" status reflects an intentional action rather than a save
    // side-effect — and so they can run Test Connection first if they want
    // to verify the endpoint is reachable before going live.
    try {
        await invoke('update_config', { config });
        showToast(
            'Configuration saved. Click "Test Connection" to verify, then "Start Proxy" to begin.',
            'success',
            { duration: 6000 }
        );
    } catch (error) {
        console.error('Failed to save config:', error);
        showToast('Failed to save configuration: ' + error, 'error');
    }
}

// -----------------------------------------------------------------------
// Toasts
//
// Routine feedback (save success, validation errors, test results,
// start/stop) goes to a transient floating notification at the top-right.
// `showToast(text, type, { duration })` returns `{ dismiss }` so callers
// can pop a "in progress…" toast with `duration: 0` and dismiss it
// explicitly when the operation completes.
// -----------------------------------------------------------------------

const TOAST_ICON = { success: '✓', error: '✕', info: 'ℹ' };
const DEFAULT_TOAST_DURATION_MS = 4000;
const TOAST_LEAVE_ANIMATION_MS = 220; // matches `.toast` transition timing

function showToast(text, type = 'info', { duration = DEFAULT_TOAST_DURATION_MS } = {}) {
    const toast = document.createElement('div');
    toast.className = `toast toast-${type}`;
    toast.setAttribute('role', type === 'error' ? 'alert' : 'status');

    const icon = document.createElement('span');
    icon.className = 'toast-icon';
    icon.setAttribute('aria-hidden', 'true');
    icon.textContent = TOAST_ICON[type] ?? 'ℹ';

    const body = document.createElement('span');
    body.className = 'toast-body';
    body.textContent = text;

    toast.append(icon, body);
    toastContainer.appendChild(toast);

    // Force the entry transition by toggling the visible class on the next
    // animation frame — without this delay the browser may collapse the
    // initial styles + class change into a single frame.
    requestAnimationFrame(() => toast.classList.add('toast-visible'));

    let dismissed = false;
    const dismiss = () => {
        if (dismissed) return;
        dismissed = true;
        toast.classList.remove('toast-visible');
        toast.classList.add('toast-leaving');
        setTimeout(() => toast.remove(), TOAST_LEAVE_ANIMATION_MS);
    };

    let dismissTimer;
    if (duration > 0) {
        dismissTimer = setTimeout(dismiss, duration);
    }

    toast.addEventListener('click', () => {
        if (dismissTimer) clearTimeout(dismissTimer);
        dismiss();
    });

    return { dismiss };
}

// Error-modal helpers. Reserved for failures the user MUST acknowledge —
// the inline `.message` banner is easy to miss at the bottom of the form,
// so verification failures and other "your proxy is not running because
// X" errors get a dim-screen modal with a one-time shake animation.
const errorModal = document.getElementById('errorModal');
const errorModalTitle = document.getElementById('errorModalTitle');
const errorModalBody = document.getElementById('errorModalBody');
const errorModalClose = document.getElementById('errorModalClose');

function showErrorModal(title, body) {
    errorModalTitle.textContent = title;
    errorModalBody.textContent = body;
    errorModal.hidden = false;
    // Re-trigger the shake animation if the modal was already showing.
    // (Strictly: removeProperty + reflow + re-add. Cheap.)
    const card = errorModal.querySelector('.modal-card');
    if (card) {
        card.style.animation = 'none';
        // Force reflow so the next assignment restarts the keyframes.
        // eslint-disable-next-line no-unused-expressions
        void card.offsetWidth;
        card.style.animation = '';
    }
    errorModalClose.focus();
}

function hideErrorModal() {
    errorModal.hidden = true;
}

errorModalClose.addEventListener('click', hideErrorModal);

// Click outside the card → dismiss. (We intentionally don't dismiss on a
// click *inside* the card so the user doesn't lose the message by accident
// while reading it.)
errorModal.addEventListener('click', (event) => {
    if (event.target === errorModal) hideErrorModal();
});

// Esc key → dismiss. Standard accessibility expectation for modal dialogs.
document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !errorModal.hidden) {
        hideErrorModal();
    }
});

// Pre-flight verify before actually flipping the proxy to Running.
//
// Without this, "Start Proxy" used to mean "bind a TCP listener", not "the
// onion is reachable and speaks JSON-RPC". The proxy would happily go
// green on a non-existent .onion (or a real-looking one that's offline),
// and the failure would only surface when the wallet sent its first
// request. That's a confusing first-run.
//
// Verification runs the same probe as the Test Connection button:
// SOCKS5 to the configured Tor proxy, then a `web3_clientVersion`
// JSON-RPC over the tunnel. The listener only comes up if both pass.
async function startProxy() {
    const restoreButton = () => {
        startBtn.disabled = false;
        startBtn.textContent = 'Start Proxy';
    };
    try {
        const config = await invoke('get_config');
        if (!config.onion_endpoint || config.onion_endpoint.trim() === '') {
            showToast('Please configure the onion endpoint first', 'error');
            return;
        }

        startBtn.disabled = true;
        startBtn.textContent = 'Verifying…';
        const verifyingToast = showToast(
            'Verifying connection to the onion endpoint (this can take up to 30 seconds)…',
            'info',
            { duration: 0 } // sticks until we dismiss it explicitly
        );

        try {
            await invoke('test_connection', {
                onionEndpoint: config.onion_endpoint,
                torProxy: config.tor_proxy
            });
        } catch (testError) {
            // Verification failure is must-acknowledge — surface in the
            // modal, not a corner toast.
            verifyingToast.dismiss();
            showErrorModal('Cannot start proxy', String(testError));
            restoreButton();
            return;
        }

        verifyingToast.dismiss();
        startBtn.textContent = 'Starting…';
        const startingToast = showToast('Verification passed. Starting proxy…', 'info', { duration: 0 });
        await invoke('start_proxy');

        await updateStatus();
        startingToast.dismiss();
        showToast('Proxy is running.', 'success');
        restoreButton();
    } catch (error) {
        console.error('Failed to start proxy:', error);
        showToast('Failed to start proxy: ' + error, 'error');
        restoreButton();
    }
}

async function stopProxy() {
    const stoppingToast = showToast('Stopping proxy…', 'info', { duration: 0 });
    try {
        await invoke('stop_proxy');
        stoppingToast.dismiss();
        showToast('Proxy stopped.', 'success');
    } catch (error) {
        console.error('Failed to stop proxy:', error);
        stoppingToast.dismiss();
        showToast('Failed to stop proxy: ' + error, 'error');
    }
}

// The standalone "Test Connection" button was removed: Start Proxy now
// runs the same probe automatically and surfaces failures via the modal.
// Test/close handlers + the `testConnection` function are gone — the
// `test_connection` Rust command is still alive and called by startProxy.

configForm.addEventListener('submit', saveConfig);
startBtn.addEventListener('click', startProxy);
stopBtn.addEventListener('click', stopProxy);

document.addEventListener('DOMContentLoaded', async () => {
    try {
        await loadConfig();
        await updateStatus();

        // Poll status every 2s. Cheap (it's a local Tauri IPC call against
        // an Arc<Mutex<>>); refines fast enough that the user sees state
        // changes promptly.
        setInterval(updateStatus, 2000);
    } catch (error) {
        console.error('Failed to initialize GUI:', error);
        showToast('Failed to initialize: ' + error, 'error');
    }
});
