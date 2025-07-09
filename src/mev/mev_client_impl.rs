//! MEV relay client implementation
//! 
//! This module provides the client for interacting with MEV relay services
//! like Flashbots to submit bundles with MEV protection.

use std::sync::Arc;
use std::time::Duration;
use reqwest::{Client, header::{HeaderMap, HeaderValue}};
use serde_json;
use tracing::{debug, info, warn, error};

use crate::error::{ProxyError, ProxyResult};
use super::mev_auth::{FlashbotsSigner, AuthError};
use super::mev_retry::{retry_with_backoff, CircuitBreaker, CircuitBreakerConfig, RetryConfig};
use super::mev_types::{Bundle, BundleResponse, SendBundleParams};
use crate::rpc_types::{JsonRpcRequest, JsonRpcResponse};

/// MEV relay client configuration
#[derive(Clone)]
pub struct MevConfig {
    /// Relay endpoint URL
    pub relay_url: String,
    /// Private key for signing requests (hex encoded)
    pub signing_key: String,
    /// Request timeout
    pub request_timeout: Duration,
    /// How many blocks ahead to target
    pub blocks_ahead: u64,
}

/// MEV relay client
/// 
/// Handles communication with MEV relays like Flashbots, including:
/// - Request signing using EIP-191
/// - Bundle submission and transformation
/// - Retry logic with circuit breaker
/// - Response parsing and error handling
/// 
/// # Architecture
/// 
/// The client acts as a protective layer between users and MEV relays,
/// automatically handling authentication and retries.
pub struct MevRelayClient {
    /// HTTP client for relay communication
    client: Client,
    /// Request signer
    signer: FlashbotsSigner,
    /// Relay configuration
    config: MevConfig,
    /// Circuit breaker for fault tolerance
    circuit_breaker: CircuitBreaker,
    /// Retry configuration
    retry_config: RetryConfig,
}

impl MevRelayClient {
    /// Create a new MEV relay client
    /// 
    /// # Arguments
    /// 
    /// * `config` - Client configuration including relay URL and signing key
    /// 
    /// # Returns
    /// 
    /// A configured client ready to submit bundles
    pub fn new(config: MevConfig) -> Result<Self, AuthError> {
        let signer = FlashbotsSigner::new(&config.signing_key)?;
        
        let client = Client::builder()
            .timeout(config.request_timeout)
            .build()
            .map_err(|e| AuthError::InvalidKey(format!("Failed to create HTTP client: {}", e)))?;
            
        let circuit_breaker = CircuitBreaker::new(CircuitBreakerConfig::default());
        let retry_config = RetryConfig::default();
        
        info!("MEV client initialized for relay: {}", config.relay_url);
        info!("Signing with address: {}", signer.address());
        
        Ok(Self {
            client,
            signer,
            config,
            circuit_breaker,
            retry_config,
        })
    }
    
    /// Handle eth_sendRawTransaction by converting to a bundle
    /// 
    /// # Arguments
    /// 
    /// * `request` - The original sendRawTransaction request
    /// * `current_block` - Current block number for targeting
    /// 
    /// # Returns
    /// 
    /// Bundle hash if successful
    pub async fn handle_send_raw_transaction(
        &self,
        request: &JsonRpcRequest,
        current_block: u64,
    ) -> ProxyResult<String> {
        // Extract the raw transaction from params
        let tx_hex = request.params
            .as_ref()
            .and_then(|p| p.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProxyError::InvalidRequest(
                "Missing transaction data in params".to_string()
            ))?;
            
        // Create a bundle with single transaction
        let target_block = current_block + self.config.blocks_ahead;
        let bundle = Bundle {
            txs: vec![tx_hex.to_string()],
            block_number: format!("0x{:x}", target_block),
            min_timestamp: None,
            max_timestamp: None,
        };
        
        debug!("Converting raw transaction to bundle targeting block {}", target_block);
        self.submit_bundle(bundle).await
    }
    
    /// Handle eth_sendBundle request
    /// 
    /// # Arguments
    /// 
    /// * `request` - The sendBundle request
    /// 
    /// # Returns
    /// 
    /// Bundle hash if successful
    pub async fn handle_send_bundle(&self, request: &JsonRpcRequest) -> ProxyResult<String> {
        // Parse bundle parameters
        let params: SendBundleParams = request.params
            .as_ref()
            .and_then(|p| p.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .ok_or_else(|| ProxyError::InvalidRequest(
                "Invalid bundle parameters".to_string()
            ))?;
            
        let bundle = Bundle {
            txs: params.txs,
            block_number: params.block_number,
            min_timestamp: params.min_timestamp,
            max_timestamp: params.max_timestamp,
        };
        
        self.submit_bundle(bundle).await
    }
    
    /// Submit a bundle to the MEV relay
    /// 
    /// # Arguments
    /// 
    /// * `bundle` - The bundle to submit
    /// 
    /// # Returns
    /// 
    /// Bundle hash if successful
    async fn submit_bundle(&self, bundle: Bundle) -> ProxyResult<String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_sendBundle".to_string(),
            params: Some(serde_json::json!([bundle])),
            id: Some(serde_json::json!(1)),
        };
        
        let body = serde_json::to_string(&request)
            .map_err(|e| ProxyError::InternalError(format!("Failed to serialize request: {}", e)))?;
            
        // Sign the request
        let signature = self.signer.sign_request(&body)
            .map_err(|e| ProxyError::InternalError(format!("Failed to sign request: {}", e)))?;
            
        debug!("Submitting bundle with signature: {}", signature);
        
        // Submit with retry logic
        let response = retry_with_backoff(
            &self.retry_config,
            Some(&self.circuit_breaker),
            || async {
                self.send_signed_request(&body, &signature).await
            }
        ).await?;
        
        // Parse response
        let bundle_response: BundleResponse = response.result
            .and_then(|v| serde_json::from_value(v).ok())
            .ok_or_else(|| ProxyError::InternalError(
                "Invalid bundle response from relay".to_string()
            ))?;
            
        info!("Bundle submitted successfully: {}", bundle_response.bundle_hash);
        Ok(bundle_response.bundle_hash)
    }
    
    /// Send a signed request to the relay
    async fn send_signed_request(
        &self,
        body: &str,
        signature: &str,
    ) -> ProxyResult<JsonRpcResponse> {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        headers.insert(
            "X-Flashbots-Signature",
            HeaderValue::from_str(signature)
                .map_err(|e| ProxyError::InternalError(format!("Invalid signature header: {}", e)))?
        );
        
        let response = self.client
            .post(&self.config.relay_url)
            .headers(headers)
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| {
                error!("Failed to send request to MEV relay: {}", e);
                ProxyError::UpstreamError(format!("MEV relay connection failed: {}", e))
            })?;
            
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("MEV relay returned error status {}: {}", status, body);
            return Err(ProxyError::UpstreamError(format!(
                "MEV relay returned status {}: {}",
                status, body
            )));
        }
        
        let json_response: JsonRpcResponse = response.json().await
            .map_err(|e| {
                error!("Failed to parse MEV relay response: {}", e);
                ProxyError::UpstreamError(format!("Failed to parse response: {}", e))
            })?;
            
        if let Some(error) = &json_response.error {
            warn!("MEV relay returned error: {:?}", error);
            return Err(ProxyError::UpstreamError(format!(
                "MEV relay error: {}",
                error.message
            )));
        }
        
        Ok(json_response)
    }
}

/// Create MEV client from configuration
pub fn create_mev_client(config: MevConfig) -> Result<Arc<MevRelayClient>, ProxyError> {
    MevRelayClient::new(config)
        .map(Arc::new)
        .map_err(|e| ProxyError::InternalError(format!("Failed to create MEV client: {}", e)))
}