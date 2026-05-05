//! MEV relay client for submitting bundles to Flashbots
//! 
//! This module provides the main client interface for MEV protection,
//! handling bundle submission, authentication, and retry logic.

use std::sync::Arc;
use std::time::Duration;
use reqwest::{Client, StatusCode};
use serde_json::Value;
use tracing::{debug, info};

use crate::error::ProxyError;
use crate::rpc_types::JsonRpcRequest;
use super::auth::FlashbotsAuthenticator;
use super::retry::{CircuitBreaker, RetryPolicy};
use super::types::{Bundle, MevConfig, SendBundleRequest};

/// MEV relay client for submitting bundles to Flashbots
/// 
/// Handles connection pooling, authentication, and retry logic for
/// reliable bundle submission to MEV relays.
/// 
/// # For Proxy Operators
/// Configure via environment variables:
/// - `FLASHBOTS_RELAY_URL`: Relay endpoint (defaults to mainnet)
/// - `FLASHBOTS_SIGNING_KEY`: Private key for authentication
/// 
/// # Example
/// ```rust
/// let config = MevConfig {
///     relay_url: "https://relay-sepolia.flashbots.net".to_string(),
///     signing_key: "your-private-key".to_string(),
///     ..Default::default()
/// };
/// 
/// let client = MevRelayClient::new(config)?;
/// let bundle_hash = client.send_bundle(bundle).await?;
/// ```
pub struct MevRelayClient {
    /// HTTP client with connection pooling
    http_client: Client,
    /// Configuration
    config: MevConfig,
    /// Authenticator for signing requests
    authenticator: Arc<FlashbotsAuthenticator>,
    /// Circuit breaker for fault tolerance
    circuit_breaker: Arc<CircuitBreaker>,
    /// Retry policy
    retry_policy: RetryPolicy,
}

impl MevRelayClient {
    /// Create a new MEV relay client
    /// 
    /// # Arguments
    /// * `config` - MEV relay configuration
    /// 
    /// # Returns
    /// * `Ok(MevRelayClient)` - Configured client
    /// * `Err(ProxyError)` - If configuration is invalid
    pub fn new(config: MevConfig) -> Result<Self, ProxyError> {
        // Create HTTP client with connection pooling
        let http_client = Client::builder()
            .timeout(config.request_timeout)
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(10)
            .build()
            .map_err(|e| ProxyError::InternalError(format!("Failed to create HTTP client: {}", e)))?;
        
        // Create authenticator
        let authenticator = FlashbotsAuthenticator::new(&config.signing_key)
            .map_err(|e| ProxyError::InternalError(format!("Invalid signing key: {}", e)))?;
        
        info!(
            "MEV client initialized with relay: {} and signer: {}",
            config.relay_url,
            authenticator.address()
        );
        
        Ok(Self {
            http_client,
            config,
            authenticator: Arc::new(authenticator),
            circuit_breaker: Arc::new(CircuitBreaker::new()),
            retry_policy: RetryPolicy::default(),
        })
    }
    
    /// Transform a raw transaction into a bundle for the next block
    /// 
    /// # For Library Developers
    /// Converts eth_sendRawTransaction format to eth_sendBundle format
    /// targeting the next block for inclusion.
    /// 
    /// # Arguments
    /// * `raw_tx` - Signed transaction in hex format
    /// * `current_block` - Current block number
    /// 
    /// # Returns
    /// Bundle configured for next block submission
    pub fn create_bundle_from_tx(&self, raw_tx: String, current_block: u64) -> Bundle {
        let target_block = current_block + self.config.blocks_ahead;
        
        Bundle {
            txs: vec![raw_tx],
            block_number: format!("0x{:x}", target_block),
            min_timestamp: None,
            max_timestamp: None,
        }
    }
    
    /// Submit a bundle to the MEV relay
    /// 
    /// Handles authentication, retry logic, and circuit breaking.
    /// 
    /// # Arguments
    /// * `bundle` - Bundle to submit
    /// 
    /// # Returns
    /// * `Ok(bundle_hash)` - Unique identifier for tracking
    /// * `Err(ProxyError)` - If submission fails
    pub async fn send_bundle(&self, bundle: Bundle) -> Result<String, ProxyError> {
        // Check circuit breaker
        if !self.circuit_breaker.can_proceed().await {
            return Err(ProxyError::UpstreamError(
                "MEV relay circuit breaker is open - too many failures".to_string()
            ));
        }
        
        // Create request
        let request = SendBundleRequest::new(bundle.clone(), 1);
        let body = serde_json::to_string(&request)
            .map_err(|e| ProxyError::InternalError(format!("Failed to serialize bundle: {}", e)))?;
        
        // Attempt with retries
        let mut last_error = None;
        
        for attempt in 0..self.retry_policy.max_attempts {
            if attempt > 0 {
                let delay = self.retry_policy.calculate_delay(attempt - 1);
                debug!("Retrying bundle submission after {:?} (attempt {})", delay, attempt + 1);
                tokio::time::sleep(delay).await;
            }
            
            match self.send_bundle_attempt(&body).await {
                Ok(bundle_hash) => {
                    self.circuit_breaker.record_success().await;
                    // Bundle hash is a correlatable identifier for a real
                    // user's transaction. INFO-level logs flow into syslog /
                    // log shippers by default; downgrade so operators have to
                    // opt in via RUST_LOG=debug.
                    debug!("Bundle submitted successfully: {}", bundle_hash);
                    return Ok(bundle_hash);
                }
                Err(e) => {
                    last_error = Some(e);
                    
                    // Check if error is retryable
                    if let Some(ref error) = last_error {
                        if !self.retry_policy.should_retry(&error.to_string()) {
                            self.circuit_breaker.record_failure().await;
                            return Err(error.clone());
                        }
                    }
                }
            }
        }
        
        // All retries exhausted
        self.circuit_breaker.record_failure().await;
        Err(last_error.unwrap_or_else(|| 
            ProxyError::UpstreamError("Bundle submission failed after all retries".to_string())
        ))
    }
    
    /// Single attempt to send a bundle
    async fn send_bundle_attempt(&self, body: &str) -> Result<String, ProxyError> {
        // Sign the request
        let signature = self.authenticator.sign_request(body)
            .map_err(|e| ProxyError::InternalError(format!("Failed to sign request: {}", e)))?;
        
        debug!("Sending bundle to {} with signature", self.config.relay_url);
        
        // Send request
        let response = self.http_client
            .post(&self.config.relay_url)
            .header("Content-Type", "application/json")
            .header("X-Flashbots-Signature", signature)
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| ProxyError::UpstreamError(format!("Network error: {}", e)))?;
        
        let status = response.status();
        let response_text = response.text().await
            .unwrap_or_else(|_| "Failed to read response body".to_string());
        
        debug!("Relay response: {} - {}", status, response_text);
        
        // Parse response
        if status.is_success() {
            self.parse_bundle_response(&response_text)
        } else {
            Err(self.map_relay_error(status, &response_text))
        }
    }
    
    /// Parse successful bundle response
    fn parse_bundle_response(&self, response_text: &str) -> Result<String, ProxyError> {
        let response: Value = serde_json::from_str(response_text)
            .map_err(|e| ProxyError::UpstreamError(
                format!("Invalid JSON response from relay: {}", e)
            ))?;
        
        // Handle JSON-RPC response format
        if let Some(error) = response.get("error") {
            let error_msg = error.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            
            return Err(ProxyError::UpstreamError(
                format!("Relay error: {}", error_msg)
            ));
        }
        
        // Extract bundle hash from result
        let bundle_hash = response
            .get("result")
            .and_then(|r| r.get("bundleHash"))
            .and_then(|h| h.as_str())
            .ok_or_else(|| ProxyError::UpstreamError(
                "Missing bundleHash in response".to_string()
            ))?;
        
        Ok(bundle_hash.to_string())
    }
    
    /// Map relay HTTP errors to ProxyError
    fn map_relay_error(&self, status: StatusCode, body: &str) -> ProxyError {
        match status {
            StatusCode::UNAUTHORIZED => ProxyError::InternalError(
                "Flashbots authentication failed - check signing key".to_string()
            ),
            StatusCode::BAD_REQUEST => {
                // Try to extract error message
                if let Ok(json) = serde_json::from_str::<Value>(body) {
                    if let Some(error_msg) = json.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str()) {
                        return ProxyError::InvalidRequest(
                            format!("Bundle validation failed: {}", error_msg)
                        );
                    }
                }
                ProxyError::InvalidRequest("Invalid bundle format".to_string())
            }
            StatusCode::TOO_MANY_REQUESTS => ProxyError::UpstreamError(
                "MEV relay rate limit exceeded".to_string()
            ),
            _ => ProxyError::UpstreamError(
                format!("MEV relay error {}: {}", status, body)
            ),
        }
    }
    
    /// Handle eth_sendRawTransaction by converting to bundle
    /// 
    /// # For Library Developers  
    /// This is the main entry point from the proxy for MEV protection.
    /// Transforms a regular transaction into a Flashbots bundle.
    pub async fn handle_send_raw_transaction(
        &self,
        request: &JsonRpcRequest,
        current_block: u64,
    ) -> Result<String, ProxyError> {
        // Extract raw transaction from params
        let raw_tx = request.params
            .as_ref()
            .and_then(|p| p.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProxyError::InvalidRequest(
                "Missing transaction data in params".to_string()
            ))?;
        
        // Create and submit bundle
        let bundle = self.create_bundle_from_tx(raw_tx.to_string(), current_block);
        info!("Converting transaction to bundle for block {}", bundle.block_number);
        
        self.send_bundle(bundle).await
    }
    
    /// Handle eth_sendBundle directly
    /// 
    /// # For Library Developers
    /// Processes pre-formatted bundle submissions from advanced users.
    /// Non-blocking summary of the relay circuit-breaker state, used by
    /// `/health` to surface MEV-relay availability without contending on
    /// the breaker's internal mutex.
    pub fn circuit_state_summary(&self) -> &'static str {
        self.circuit_breaker.state_summary()
    }

    pub async fn handle_send_bundle(&self, request: &JsonRpcRequest) -> Result<String, ProxyError> {
        // Extract bundle from params
        let bundle_json = request.params
            .as_ref()
            .and_then(|p| p.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| ProxyError::InvalidRequest(
                "Missing bundle in params".to_string()
            ))?;

        let bundle: Bundle = serde_json::from_value(bundle_json.clone())
            .map_err(|e| ProxyError::InvalidRequest(
                format!("Invalid bundle format: {}", e)
            ))?;

        self.send_bundle(bundle).await
    }
}

/// Construct a shared MEV relay client from configuration.
pub fn create_mev_client(config: MevConfig) -> Result<Arc<MevRelayClient>, ProxyError> {
    MevRelayClient::new(config).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn test_config() -> MevConfig {
        MevConfig {
            relay_url: "https://relay-sepolia.flashbots.net".to_string(),
            signing_key: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            request_timeout: Duration::from_secs(5),
            blocks_ahead: 1,
        }
    }
    
    #[test]
    fn test_bundle_creation() {
        let config = test_config();
        let client = MevRelayClient::new(config).unwrap();
        
        let raw_tx = "0xf86b...".to_string();
        let bundle = client.create_bundle_from_tx(raw_tx.clone(), 1000);
        
        assert_eq!(bundle.txs.len(), 1);
        assert_eq!(bundle.txs[0], raw_tx);
        assert_eq!(bundle.block_number, "0x3e9"); // 1001 in hex
        assert!(bundle.min_timestamp.is_none());
        assert!(bundle.max_timestamp.is_none());
    }
    
    #[test]
    fn test_parse_bundle_response() {
        let config = test_config();
        let client = MevRelayClient::new(config).unwrap();
        
        // Success response
        let response = r#"{"jsonrpc":"2.0","result":{"bundleHash":"0xabc123"},"id":1}"#;
        let result = client.parse_bundle_response(response);
        assert_eq!(result.unwrap(), "0xabc123");
        
        // Error response
        let error_response = r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Bundle failed"},"id":1}"#;
        let result = client.parse_bundle_response(error_response);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Bundle failed"));
    }
    
    #[test]
    fn test_map_relay_errors() {
        let config = test_config();
        let client = MevRelayClient::new(config).unwrap();
        
        // Unauthorized
        let error = client.map_relay_error(StatusCode::UNAUTHORIZED, "");
        assert!(matches!(error, ProxyError::InternalError(_)));
        assert!(error.to_string().contains("authentication"));
        
        // Bad request with JSON error
        let body = r#"{"error":{"message":"Invalid bundle"}}"#;
        let error = client.map_relay_error(StatusCode::BAD_REQUEST, body);
        assert!(matches!(error, ProxyError::InvalidRequest(_)));
        assert!(error.to_string().contains("Invalid bundle"));
        
        // Rate limit
        let error = client.map_relay_error(StatusCode::TOO_MANY_REQUESTS, "");
        assert!(matches!(error, ProxyError::UpstreamError(_)));
        assert!(error.to_string().contains("rate limit"));
    }
}