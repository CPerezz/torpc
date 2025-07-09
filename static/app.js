// TorPC Frontend JavaScript

// Elements
const methodSelect = document.getElementById('method');
const paramsSection = document.getElementById('params-section');
const paramsInput = document.getElementById('params');
const testBtn = document.getElementById('test-btn');
const requestDisplay = document.getElementById('request-display');
const responseDisplay = document.getElementById('response-display');
const statusElement = document.getElementById('status');
const torInfo = document.getElementById('tor-info');

// Check connection status on load
window.addEventListener('DOMContentLoaded', () => {
    checkStatus();
    setupMethodSelector();
});

// Check RPC status
async function checkStatus() {
    try {
        const response = await fetch('/rpc', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                jsonrpc: '2.0',
                method: 'net_version',
                params: [],
                id: 1
            })
        });

        if (response.ok) {
            statusElement.textContent = 'Online';
            statusElement.className = 'status-online';
        } else {
            statusElement.textContent = 'Offline';
            statusElement.className = 'status-offline';
        }
    } catch (error) {
        statusElement.textContent = 'Offline';
        statusElement.className = 'status-offline';
    }

    // Check for Tor info (this would normally come from a status endpoint)
    // For now, we'll just show instructions
    torInfo.textContent = `To use through Tor:
1. Run: ./scripts/start-tor.sh
2. Check: data/tor/torpc/hostname for your .onion address
3. Connect via: torsocks curl http://your-address.onion/rpc`;
}

// Setup method selector
function setupMethodSelector() {
    methodSelect.addEventListener('change', () => {
        const method = methodSelect.value;
        
        // Show params input for methods that need it
        if (method === 'eth_getBalance') {
            paramsSection.style.display = 'block';
            paramsInput.placeholder = '["0x742d35Cc6634C0532925a3b844Bc9e7595f7777", "latest"]';
        } else {
            paramsSection.style.display = 'none';
            paramsInput.value = '';
        }
    });
}

// Send test request
testBtn.addEventListener('click', async () => {
    const method = methodSelect.value;
    const endpoint = document.querySelector('input[name="endpoint"]:checked').value;
    
    // Build params
    let params = [];
    if (paramsInput.value) {
        try {
            params = JSON.parse(paramsInput.value);
        } catch (e) {
            alert('Invalid JSON in parameters');
            return;
        }
    }
    
    // Build request
    const request = {
        jsonrpc: '2.0',
        method: method,
        params: params,
        id: Date.now()
    };
    
    // Display request
    requestDisplay.textContent = JSON.stringify(request, null, 2);
    responseDisplay.textContent = 'Sending...';
    
    try {
        const response = await fetch(endpoint, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(request)
        });
        
        const data = await response.json();
        
        // Display response with syntax highlighting
        responseDisplay.textContent = JSON.stringify(data, null, 2);
        
        // Color code based on success/error
        if (data.error) {
            responseDisplay.style.borderColor = '#e74c3c';
        } else {
            responseDisplay.style.borderColor = '#27ae60';
        }
        
    } catch (error) {
        responseDisplay.textContent = `Error: ${error.message}`;
        responseDisplay.style.borderColor = '#e74c3c';
    }
});

// Copy functionality for code blocks
document.querySelectorAll('.code-block').forEach(block => {
    block.addEventListener('click', function() {
        if (this.textContent && this.textContent !== 'Sending...') {
            navigator.clipboard.writeText(this.textContent).then(() => {
                // Visual feedback
                const original = this.style.borderColor;
                this.style.borderColor = '#27ae60';
                setTimeout(() => {
                    this.style.borderColor = original;
                }, 500);
            });
        }
    });
});

// Auto-update RPC URL if not on localhost
if (window.location.hostname !== 'localhost' && window.location.hostname !== '127.0.0.1') {
    document.getElementById('full-rpc-url').textContent = 
        `${window.location.protocol}//${window.location.host}/rpc`;
}