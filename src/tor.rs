use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use tracing::{info, warn};

use reqwest::{Client, Proxy, Url};
use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use crate::{
    error::ProxyError,
    rpc_types::{JsonRpcRequest, JsonRpcResponse},
};

/// Tor service configuration and utilities
pub struct TorService {
    pub hostname_path: String,
    pub config_path: String,
}

impl TorService {
    pub fn new() -> Self {
        Self {
            hostname_path: "./data/tor/torpc/hostname".to_string(),
            config_path: "./configs/torrc".to_string(),
        }
    }
    
    /// Check if Tor is properly configured
    pub fn check_configuration(&self) -> Result<()> {
        // Check if torrc exists
        if !Path::new(&self.config_path).exists() {
            anyhow::bail!("Tor configuration file not found at: {}", self.config_path);
        }
        
        // Check if data directory exists
        let data_dir = Path::new("./data/tor/torpc");
        if !data_dir.exists() {
            warn!("Tor data directory doesn't exist, creating it...");
            fs::create_dir_all(data_dir)
                .context("Failed to create Tor data directory")?;
            
            // Set proper permissions (700)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = fs::Permissions::from_mode(0o700);
                fs::set_permissions(data_dir, permissions)
                    .context("Failed to set Tor directory permissions")?;
            }
        }
        
        info!("Tor configuration verified");
        Ok(())
    }
    
    /// Get the .onion hostname if available
    pub fn get_hostname(&self) -> Result<Option<String>> {
        let hostname_path = Path::new(&self.hostname_path);
        
        if !hostname_path.exists() {
            info!("Hostname file not found - Tor service may not be running yet");
            return Ok(None);
        }
        
        let hostname = fs::read_to_string(hostname_path)
            .context("Failed to read hostname file")?
            .trim()
            .to_string();
            
        if hostname.is_empty() {
            return Ok(None);
        }
        
        // Validate it looks like a .onion address
        if !hostname.ends_with(".onion") {
            anyhow::bail!("Invalid hostname format: {}", hostname);
        }
        
        Ok(Some(hostname))
    }
    
    /// Get the full onion URL for a given path
    pub fn get_onion_url(&self, path: &str) -> Result<Option<String>> {
        match self.get_hostname()? {
            Some(hostname) => {
                let url = format!("http://{}{}", hostname, path);
                Ok(Some(url))
            }
            None => Ok(None),
        }
    }
    
    /// Check if Tor appears to be running by looking for the hostname file
    pub fn is_running(&self) -> bool {
        Path::new(&self.hostname_path).exists()
    }
}

impl Default for TorService {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TorJsonRpcClient {
    client: Client,
    endpoint: Url,
}

impl TorJsonRpcClient {
    fn new(
        endpoint: String,
        tor_proxy: String,
        dial_timeout: Duration,
        keep_alive: Duration,
        request_timeout: Duration,
        idle_conn_timeout: Duration,
        max_idle_conns: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let endpoint = Url::parse(format!("http://{}", endpoint).as_str())?;
        let proxy = Proxy::http(format!("socks5h://{}", tor_proxy))?;

        let client = Client::builder()
            .proxy(proxy)
            .timeout(request_timeout)
            .connect_timeout(dial_timeout)
            .pool_idle_timeout(idle_conn_timeout)
            .pool_max_idle_per_host(max_idle_conns)
            .tcp_keepalive(Some(keep_alive))
            .build()?;

        Ok(Self { client, endpoint })
    }
    async fn send_request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, ProxyError> {
        let response = self
            .client
            .post(self.endpoint.as_str())
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let rpc_response: JsonRpcResponse = response.json().await?;

        if let Some(error) = rpc_response.error {
            return Err(ProxyError::UpstreamError(format!(
                "JSON-RPC request through TOR returned error: {}",
                error.message
            )));
        }

        return Ok(rpc_response);
    }
}

pub struct Onion {
    peers: Vec<TorJsonRpcClient>,
    next_peer: AtomicU32,
}

#[derive(Debug)]
pub struct OnionConfig {
    pub tor_proxy: String,
    pub addresses: Vec<String>,
    pub dial_timeout: Duration,
    pub keep_alive: Duration,
    pub request_timeout: Duration,
    pub idle_conn_timeout: Duration,
    pub max_idle_conns: usize,
}

impl OnionConfig {
    pub fn default() -> Self {
        Self {
            tor_proxy: "127.0.0.1:9050".to_string(),
            addresses: vec![],
            dial_timeout: Duration::from_secs(60),
            keep_alive: Duration::from_secs(60),
            request_timeout: Duration::from_secs(60),
            idle_conn_timeout: Duration::from_secs(60),
            max_idle_conns: 0usize,
        }
    }

    pub fn with_peers(self, addresses: Vec<String>) -> Self {
        Self {
            tor_proxy: self.tor_proxy,
            addresses,
            dial_timeout: self.dial_timeout,
            keep_alive: self.keep_alive,
            request_timeout: self.request_timeout,
            idle_conn_timeout: self.idle_conn_timeout,
            max_idle_conns: self.max_idle_conns,
        }
    }
}

impl Onion {
    pub fn try_new(config: OnionConfig) -> Result<Self, String> {
        let OnionConfig {
            addresses,
            tor_proxy,
            dial_timeout,
            keep_alive,
            request_timeout,
            idle_conn_timeout,
            max_idle_conns,
        } = config;
        let peers: Vec<TorJsonRpcClient> = addresses
            .into_iter()
            .filter_map(|addr| {
                TorJsonRpcClient::new(
                    addr,
                    tor_proxy.clone(),
                    dial_timeout,
                    keep_alive,
                    request_timeout,
                    idle_conn_timeout,
                    max_idle_conns,
                )
                .ok()
            })
            .collect();

        if peers.is_empty() {
            // TODO: forward last error
            return Err("no valid peers!".to_string());
        }

        Ok(Self {
            peers,
            next_peer: AtomicU32::new(0),
        })
    }
    pub async fn send_request(
        &self,
        request: &JsonRpcRequest,
        retries: usize,
    ) -> Result<JsonRpcResponse, ProxyError> {
        for attempt in 0..retries {
            // plain and simple RR (for now)
            let peer_id = self.next_peer.fetch_add(1, Ordering::SeqCst) as usize % self.peers.len();
            let peer = &self.peers[peer_id];
            
            let result = peer.send_request(request).await;

            match result {
                Ok(hash) => {
                    return Ok(hash);
                }
                Err(err) => {
                    warn!("Failed to send tx to {} ({})", peer.endpoint, err);
                }
            }

            if attempt + 1 < retries {
                tokio::time::sleep(Duration::from_secs(attempt as u64 + 1)).await;
            }
        }

        return Err(ProxyError::UpstreamError(format!(
            "failed to send tx through TOR after {} retries",
            retries
        )));
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;
    
    #[test]
    fn test_tor_service_new() {
        let tor = TorService::new();
        assert_eq!(tor.hostname_path, "./data/tor/torpc/hostname");
        assert_eq!(tor.config_path, "./configs/torrc");
    }
    
    #[test]
    fn test_get_hostname_missing_file() {
        let tor = TorService {
            hostname_path: "/nonexistent/path/hostname".to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        let result = tor.get_hostname().unwrap();
        assert!(result.is_none());
    }
    
    #[test]
    fn test_get_hostname_valid() {
        let temp_dir = TempDir::new().unwrap();
        let hostname_path = temp_dir.path().join("hostname");
        
        fs::write(&hostname_path, "test3xamplee2onion.onion\n").unwrap();
        
        let tor = TorService {
            hostname_path: hostname_path.to_str().unwrap().to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        let result = tor.get_hostname().unwrap();
        assert_eq!(result, Some("test3xamplee2onion.onion".to_string()));
    }
    
    #[test]
    fn test_get_hostname_invalid_format() {
        let temp_dir = TempDir::new().unwrap();
        let hostname_path = temp_dir.path().join("hostname");
        
        fs::write(&hostname_path, "not-an-onion-address").unwrap();
        
        let tor = TorService {
            hostname_path: hostname_path.to_str().unwrap().to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        let result = tor.get_hostname();
        assert!(result.is_err());
    }
    
    #[test]
    fn test_get_onion_url() {
        let temp_dir = TempDir::new().unwrap();
        let hostname_path = temp_dir.path().join("hostname");
        
        fs::write(&hostname_path, "test3xamplee2onion.onion\n").unwrap();
        
        let tor = TorService {
            hostname_path: hostname_path.to_str().unwrap().to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        let url = tor.get_onion_url("/rpc").unwrap();
        assert_eq!(url, Some("http://test3xamplee2onion.onion/rpc".to_string()));
    }
    
    #[test]
    fn test_is_running() {
        let temp_dir = TempDir::new().unwrap();
        let hostname_path = temp_dir.path().join("hostname");
        
        let tor = TorService {
            hostname_path: hostname_path.to_str().unwrap().to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        // Not running initially
        assert!(!tor.is_running());
        
        // Create hostname file
        fs::write(&hostname_path, "test.onion").unwrap();
        
        // Should appear as running
        assert!(tor.is_running());
    }
}