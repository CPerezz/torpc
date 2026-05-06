pub mod config;
pub mod proxy;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{error, info};

pub use config::Config;
pub use proxy::{ProxyConfig, TorRpcProxy};

/// Status of the proxy.
///
/// Serializes to a tagged JSON shape so the GUI can pattern-match without
/// regexing against `Debug` output. Examples:
///
/// ```json
/// { "state": "Stopped" }
/// { "state": "Running" }
/// { "state": "Error", "message": "Tor not reachable" }
/// ```
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", content = "message")]
pub enum ProxyStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}

/// Controller for managing the proxy lifecycle
pub struct ProxyController {
    config: Arc<RwLock<ProxyConfig>>,
    status: Arc<RwLock<ProxyStatus>>,
    handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl ProxyController {
    /// Create a new proxy controller
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            status: Arc::new(RwLock::new(ProxyStatus::Stopped)),
            handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the proxy
    pub async fn start(&self) -> Result<()> {
        let mut status = self.status.write().await;
        let mut handle = self.handle.write().await;

        // Check if already running
        if matches!(*status, ProxyStatus::Running | ProxyStatus::Starting) {
            return Ok(());
        }

        *status = ProxyStatus::Starting;
        drop(status); // Release the lock

        let config = self.config.read().await.clone();
        let proxy = TorRpcProxy::new(config);
        let status_clone = Arc::clone(&self.status);

        let task_handle = tokio::spawn(async move {
            info!("Starting proxy...");

            // Update status to running
            {
                let mut status = status_clone.write().await;
                *status = ProxyStatus::Running;
            }

            // Run the proxy
            if let Err(e) = proxy.run().await {
                error!("Proxy error: {}", e);
                let mut status = status_clone.write().await;
                *status = ProxyStatus::Error(e.to_string());
            } else {
                let mut status = status_clone.write().await;
                *status = ProxyStatus::Stopped;
            }
        });

        *handle = Some(task_handle);
        Ok(())
    }

    /// Stop the proxy
    pub async fn stop(&self) -> Result<()> {
        let mut status = self.status.write().await;
        let mut handle = self.handle.write().await;

        if let Some(h) = handle.take() {
            *status = ProxyStatus::Stopping;
            drop(status); // Release the lock

            info!("Stopping proxy...");
            h.abort();
            let _ = h.await; // Wait for it to finish

            let mut status = self.status.write().await;
            *status = ProxyStatus::Stopped;
        }

        Ok(())
    }

    /// Check if the proxy is running
    pub async fn is_running(&self) -> bool {
        let status = self.status.read().await;
        matches!(*status, ProxyStatus::Running)
    }

    /// Get the current status
    pub async fn get_status(&self) -> ProxyStatus {
        self.status.read().await.clone()
    }

    /// Update the configuration
    pub async fn update_config(&self, new_config: ProxyConfig) -> Result<()> {
        // Stop if running
        if self.is_running().await {
            self.stop().await?;

            // Update config
            let mut config = self.config.write().await;
            *config = new_config;
            drop(config);

            // Restart
            self.start().await?;
        } else {
            // Just update config
            let mut config = self.config.write().await;
            *config = new_config;
        }

        Ok(())
    }

    /// Get a copy of the current configuration
    pub async fn get_config(&self) -> ProxyConfig {
        self.config.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_proxy_controller_lifecycle() {
        let config = ProxyConfig {
            listen_addr: ([127, 0, 0, 1], 0).into(),
            tor_proxy: ([127, 0, 0, 1], 9050).into(),
            onion_endpoint: "test.onion:8545".to_string(),
        };

        let controller = ProxyController::new(config);

        // Initially should be stopped
        assert_eq!(controller.get_status().await, ProxyStatus::Stopped);
        assert!(!controller.is_running().await);

        // Start the proxy
        controller.start().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Should be running
        assert!(controller.is_running().await);

        // Stop the proxy
        controller.stop().await.unwrap();

        // Should be stopped
        assert_eq!(controller.get_status().await, ProxyStatus::Stopped);
        assert!(!controller.is_running().await);
    }
}
