// Tauri 2.x global API entry points (enabled by `app.withGlobalTauri: true`
// in tauri.conf.json). The 1.x layout (`window.__TAURI__.tauri.invoke`,
// `window.__TAURI__.window.appWindow`) was reorganized; `core` holds
// `invoke`, and the current window is fetched via `getCurrentWindow()`.
if (!window.__TAURI__) {
    console.error('Tauri API not available! Make sure you are running this in a Tauri desktop app.');
    alert('Error: This application must be run as a Tauri desktop app.');
}

const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const appWindow = getCurrentWindow();

// DOM elements
const statusDot = document.getElementById('statusDot');
const statusText = document.getElementById('statusText');
const configForm = document.getElementById('configForm');
const closeBtn = document.getElementById('closeBtn');
const message = document.getElementById('message');
const onionEndpointInput = document.getElementById('onionEndpoint');
const listenPortInput = document.getElementById('listenPort');
const torProxyInput = document.getElementById('torProxy');
const startBtn = document.getElementById('startBtn');
const stopBtn = document.getElementById('stopBtn');
const proxyInfo = document.getElementById('proxyInfo');
const proxyUrl = document.getElementById('proxyUrl');
const testBtn = document.getElementById('testBtn');
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
        showMessage('Failed to load configuration', 'error');
    }
}

async function saveConfig(event) {
    event.preventDefault();

    const onionEndpoint = onionEndpointInput.value.trim();
    const listenPort = parseInt(listenPortInput.value, 10);
    const torProxy = torProxyInput.value.trim();

    if (!onionEndpoint) {
        showMessage('Onion endpoint is required', 'error');
        return;
    }

    if (!onionEndpoint.includes('.onion')) {
        showMessage('Invalid onion endpoint format', 'error');
        return;
    }

    const torProxyParts = torProxy.split(':');
    if (torProxyParts.length < 2) {
        showMessage('Invalid Tor proxy format', 'error');
        return;
    }
    const torProxyPort = parseInt(torProxyParts[torProxyParts.length - 1], 10);
    const torProxyHost = torProxyParts.slice(0, -1).join(':') || torProxyParts[0];

    const config = {
        listen_addr: `127.0.0.1:${listenPort}`,
        tor_proxy: `${torProxyHost}:${torProxyPort}`,
        onion_endpoint: onionEndpoint
    };

    try {
        await invoke('update_config', { config });
        showMessage('Configuration saved successfully', 'success');

        const status = await invoke('get_status');
        if (status.state !== 'Running') {
            showMessage('Configuration saved. Starting proxy...', 'success');
            try {
                await invoke('start_proxy');
                showMessage('Proxy started successfully!', 'success');
            } catch (startError) {
                showMessage('Configuration saved but failed to start proxy: ' + startError, 'error');
            }
        } else {
            showMessage('Configuration updated and proxy restarted', 'success');
        }

        setTimeout(hideMessage, 3000);
    } catch (error) {
        console.error('Failed to save config:', error);
        showMessage('Failed to save configuration: ' + error, 'error');
    }
}

function showMessage(text, type) {
    message.textContent = text;
    message.className = `message ${type}`;
}

function hideMessage() {
    message.className = 'message';
    message.textContent = '';
}

async function startProxy() {
    try {
        const config = await invoke('get_config');
        if (!config.onion_endpoint || config.onion_endpoint.trim() === '') {
            showMessage('Please configure the onion endpoint first', 'error');
            return;
        }

        showMessage('Starting proxy...', 'success');
        await invoke('start_proxy');

        await updateStatus();
        hideMessage();
    } catch (error) {
        console.error('Failed to start proxy:', error);
        showMessage('Failed to start proxy: ' + error, 'error');
    }
}

async function stopProxy() {
    try {
        showMessage('Stopping proxy...', 'success');
        await invoke('stop_proxy');
        hideMessage();
    } catch (error) {
        console.error('Failed to stop proxy:', error);
        showMessage('Failed to stop proxy: ' + error, 'error');
    }
}

closeBtn.addEventListener('click', async () => {
    // Hides into the tray; the Rust `on_window_event` handler intercepts
    // CloseRequested and prevents the actual close.
    try {
        await appWindow.close();
    } catch (error) {
        console.error('Error closing window:', error);
        try {
            await appWindow.hide();
        } catch (hideError) {
            console.error('Error hiding window:', hideError);
        }
    }
});

configForm.addEventListener('submit', saveConfig);

async function testConnection() {
    const onionEndpoint = onionEndpointInput.value.trim();
    const torProxy = torProxyInput.value.trim();

    if (!onionEndpoint) {
        showMessage('Please enter an onion endpoint', 'error');
        return;
    }

    testBtn.disabled = true;
    testBtn.textContent = 'Testing...';
    showMessage('Testing connection to onion endpoint (this may take up to 30 seconds)...', 'success');

    try {
        const result = await invoke('test_connection', {
            onionEndpoint: onionEndpoint,
            torProxy: torProxy
        });
        showMessage(result, 'success');
    } catch (error) {
        console.error('Test connection error:', error);
        showMessage(error, 'error');
    } finally {
        testBtn.disabled = false;
        testBtn.textContent = 'Test Connection';
    }
}

startBtn.addEventListener('click', startProxy);
stopBtn.addEventListener('click', stopProxy);
testBtn.addEventListener('click', testConnection);

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
        showMessage('Failed to initialize: ' + error, 'error');
    }
});
