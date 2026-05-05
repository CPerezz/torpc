//! MEV request handler module
//! 
//! This module provides MEV-aware request handling that integrates
//! with the proxy module without circular dependency issues.

use std::sync::Arc;
use axum::{extract::State, Json};
use tracing::{info, warn};

use crate::{
    error::{ProxyError, ProxyResult},
    proxy::{ProxyState, proxy_to_geth},
    rpc_types::{JsonRpcRequest, JsonRpcResponse},
};
use super::client::MevRelayClient;

/// MEV-aware state wrapper
pub struct MevProxyState {
    pub base_state: Arc<ProxyState>,
    pub mev_client: Option<Arc<MevRelayClient>>,
}

/// Handle Flashbots requests with MEV protection
pub async fn handle_flashbots_with_mev(
    State(state): State<Arc<MevProxyState>>,
    Json(request): Json<JsonRpcRequest>,
) -> ProxyResult<Json<JsonRpcResponse>> {
    // Apply the strict per-method limiter before doing any upstream work.
    // Without this, an attacker could exhaust their per-port read budget on
    // cheap calls and then flip to flooding `eth_sendBundle` against the
    // relay; the per-method bucket caps that path globally.
    state
        .base_state
        .check_write_method_rate_limit(&request.method)
        .await?;

    if let Some(mev_client) = &state.mev_client {
        match request.method.as_str() {
            "eth_sendRawTransaction" => {
                // Get current block number from Geth
                let block_req = JsonRpcRequest {
                    jsonrpc: "2.0".to_string(),
                    method: "eth_blockNumber".to_string(),
                    params: None,
                    id: Some(serde_json::json!(1)),
                };
                
                let block_resp = proxy_to_geth(&state.base_state, block_req).await?;
                let block_hex = block_resp.result
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ProxyError::InternalError("Failed to get block number".to_string()))?;
                
                let current_block = u64::from_str_radix(block_hex.trim_start_matches("0x"), 16)
                    .map_err(|e| ProxyError::InternalError(format!("Invalid block number: {}", e)))?;
                
                // Submit via MEV client
                info!("Submitting transaction via MEV relay");
                let bundle_hash = mev_client.handle_send_raw_transaction(&request, current_block).await?;
                
                // Return bundle hash as if it were a transaction hash
                Ok(Json(JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: Some(serde_json::json!(bundle_hash)),
                    error: None,
                    id: request.id,
                }))
            }
            "eth_sendBundle" => {
                // Direct bundle submission
                info!("Submitting bundle via MEV relay");
                let bundle_hash = mev_client.handle_send_bundle(&request).await?;
                
                Ok(Json(JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: Some(serde_json::json!({
                        "bundleHash": bundle_hash
                    })),
                    error: None,
                    id: request.id,
                }))
            }
            _ => {
                // Other methods go to regular Geth
                let response = proxy_to_geth(&state.base_state, request).await?;
                Ok(Json(response))
            }
        }
    } else {
        // Fallback to regular proxy handler
        warn!("MEV client not configured, using standard handler");
        crate::proxy::handle_flashbots(State(state.base_state.clone()), Json(request)).await
    }
}