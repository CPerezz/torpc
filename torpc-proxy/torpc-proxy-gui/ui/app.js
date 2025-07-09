// Check if Tauri API is available
if (!window.__TAURI__) {
    console.error('Tauri API not available! Make sure you are running this in a Tauri window.');
    alert('Error: This application must be run as a Tauri desktop app.');
}

const { invoke } = window.__TAURI__.tauri;
const { appWindow } = window.__TAURI__.window;

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

// Update status display
async function updateStatus() {
    try {
        const status = await invoke('get_status');
        const config = await invoke('get_config');
        
        // Remove all status classes
        statusDot.classList.remove('running', 'stopped', 'starting', 'stopping');
        
        if (status.includes('Running')) {
            statusDot.classList.add('running');
            statusText.textContent = 'Proxy is running';
            startBtn.style.display = 'none';
            stopBtn.style.display = 'block';
            proxyInfo.style.display = 'block';
            
            // Update proxy URL with actual port
            const port = config.listen_addr.split(':').pop();
            const url = `http://localhost:${port}`;
            proxyUrl.textContent = url;
            walletRpcUrl.textContent = url;
        } else if (status.includes('Stopped')) {
            statusDot.classList.add('stopped');
            statusText.textContent = 'Proxy is stopped';
            startBtn.style.display = 'block';
            stopBtn.style.display = 'none';
            proxyInfo.style.display = 'none';
        } else if (status.includes('Starting')) {
            statusDot.classList.add('starting');
            statusText.textContent = 'Starting proxy...';
            startBtn.style.display = 'none';
            stopBtn.style.display = 'none';
            proxyInfo.style.display = 'none';
        } else if (status.includes('Stopping')) {
            statusDot.classList.add('stopping');
            statusText.textContent = 'Stopping proxy...';
            startBtn.style.display = 'none';
            stopBtn.style.display = 'none';
            proxyInfo.style.display = 'none';
        } else if (status.includes('Error')) {
            statusDot.classList.add('stopped');
            const errorMatch = status.match(/Error\(["'](.+?)["']\)/);
            const errorMsg = errorMatch ? errorMatch[1] : 'Unknown error';
            statusText.textContent = `Error: ${errorMsg}`;
            startBtn.style.display = 'block';
            stopBtn.style.display = 'none';
            proxyInfo.style.display = 'none';
        }
    } catch (error) {
        console.error('Failed to get status:', error);
        statusText.textContent = 'Unknown';
        statusDot.classList.add('stopped');
    }
}

// Load current configuration
async function loadConfig() {
    try {
        const config = await invoke('get_config');
        
        // Parse listen address
        const listenPort = config.listen_addr.split(':').pop();
        listenPortInput.value = listenPort;
        
        // Parse tor proxy address
        const torProxyParts = config.tor_proxy.split(':');
        if (torProxyParts.length >= 2) {
            const torProxyPort = torProxyParts[torProxyParts.length - 1];
            const torProxyHost = torProxyParts.slice(0, -1).join(':');
            torProxyInput.value = `${torProxyHost}:${torProxyPort}`;
        }
        
        // Set onion endpoint
        onionEndpointInput.value = config.onion_endpoint || '';
    } catch (error) {
        console.error('Failed to load config:', error);
        showMessage('Failed to load configuration', 'error');
    }
}

// Save configuration
async function saveConfig(event) {
    event.preventDefault();
    
    const onionEndpoint = onionEndpointInput.value.trim();
    const listenPort = parseInt(listenPortInput.value);
    const torProxy = torProxyInput.value.trim();
    
    // Validate inputs
    if (!onionEndpoint) {
        showMessage('Onion endpoint is required', 'error');
        return;
    }
    
    if (!onionEndpoint.includes('.onion')) {
        showMessage('Invalid onion endpoint format', 'error');
        return;
    }
    
    // Parse tor proxy address
    const torProxyParts = torProxy.split(':');
    if (torProxyParts.length < 2) {
        showMessage('Invalid Tor proxy format', 'error');
        return;
    }
    
    const torProxyPort = parseInt(torProxyParts[torProxyParts.length - 1]);
    const torProxyHost = torProxyParts.slice(0, -1).join(':') || torProxyParts[0];
    
    // Create config object
    const config = {
        listen_addr: `127.0.0.1:${listenPort}`,
        tor_proxy: `${torProxyHost}:${torProxyPort}`,
        onion_endpoint: onionEndpoint
    };
    
    try {
        await invoke('update_config', { config });
        showMessage('Configuration saved successfully', 'success');
        
        // Get current status
        const status = await invoke('get_status');
        
        // If proxy is not running, offer to start it
        if (!status.includes('Running')) {
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
        
        setTimeout(() => {
            hideMessage();
        }, 3000);
    } catch (error) {
        console.error('Failed to save config:', error);
        showMessage('Failed to save configuration: ' + error, 'error');
    }
}

// Show message
function showMessage(text, type) {
    message.textContent = text;
    message.className = `message ${type}`;
}

// Hide message
function hideMessage() {
    message.className = 'message';
    message.textContent = '';
}

// Start proxy
async function startProxy() {
    console.log('Start proxy button clicked');
    try {
        // Check if onion endpoint is configured
        const config = await invoke('get_config');
        console.log('Current config:', config);
        
        if (!config.onion_endpoint || config.onion_endpoint.trim() === '') {
            showMessage('Please configure the onion endpoint first', 'error');
            return;
        }
        
        showMessage('Starting proxy...', 'success');
        console.log('Invoking start_proxy command...');
        await invoke('start_proxy');
        console.log('start_proxy command completed');
        
        // Force status update
        await updateStatus();
        hideMessage();
    } catch (error) {
        console.error('Failed to start proxy:', error);
        showMessage('Failed to start proxy: ' + error, 'error');
    }
}

// Stop proxy
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

// Close window
closeBtn.addEventListener('click', async () => {
    try {
        // Since we're now handling close events in Rust to hide the window,
        // we can simply close the window and it will be hidden instead
        await appWindow.close();
    } catch (error) {
        console.error('Error closing window:', error);
        // Fallback: try to hide the window directly
        try {
            await appWindow.hide();
        } catch (hideError) {
            console.error('Error hiding window:', hideError);
        }
    }
});

// Form submission
configForm.addEventListener('submit', saveConfig);

// Test connection
async function testConnection() {
    console.log('Test connection button clicked');
    const onionEndpoint = onionEndpointInput.value.trim();
    const torProxy = torProxyInput.value.trim();
    
    console.log('Testing connection to:', onionEndpoint, 'via', torProxy);
    
    if (!onionEndpoint) {
        showMessage('Please enter an onion endpoint', 'error');
        return;
    }
    
    // Disable button during test
    testBtn.disabled = true;
    testBtn.textContent = 'Testing...';
    showMessage('Testing connection to onion endpoint (this may take up to 30 seconds)...', 'success');
    
    try {
        console.log('Invoking test_connection command...');
        const result = await invoke('test_connection', { 
            onionEndpoint: onionEndpoint, 
            torProxy: torProxy 
        });
        console.log('Test result:', result);
        showMessage(result, 'success');
    } catch (error) {
        console.error('Test connection error:', error);
        showMessage(error, 'error');
    } finally {
        testBtn.disabled = false;
        testBtn.textContent = 'Test Connection';
    }
}

// Start/Stop button handlers
startBtn.addEventListener('click', startProxy);
stopBtn.addEventListener('click', stopProxy);
testBtn.addEventListener('click', testConnection);

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
    console.log('DOM loaded, initializing ToRPC Proxy GUI...');
    console.log('Tauri API available:', !!window.__TAURI__);
    
    try {
        await loadConfig();
        await updateStatus();
        
        // Update status periodically
        setInterval(updateStatus, 2000);
        
        console.log('GUI initialization complete');
    } catch (error) {
        console.error('Failed to initialize GUI:', error);
        showMessage('Failed to initialize: ' + error, 'error');
    }
});