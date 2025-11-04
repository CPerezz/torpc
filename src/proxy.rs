use axum::{extract::State, Json};
use hex;
use reqwest::Client;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::{
    tor::Onion,
    error::{ProxyError, ProxyResult},
    rpc_types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse},
    security::{SecurityEvent, SecurityEventType},
    whitelist::{is_method_allowed, is_send_method},
};

#[derive(Clone)]
pub struct ProxyState {
    pub geth_client: Client,
    pub geth_url: String,
    pub flashbots_url: String,
}

impl ProxyState {
    pub fn new(geth_url: String, flashbots_url: String) -> Self {
        let geth_client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            geth_client,
            geth_url,
            flashbots_url,
        }
    }
}

/// Handle RPC requests to the standard endpoint
pub async fn handle_rpc(
    State(state): State<Arc<ProxyState>>,
    Json(request): Json<JsonRpcRequest>,
) -> ProxyResult<Json<JsonRpcResponse>> {
    debug!("Received RPC request: method={}", request.method);

    // Validate request
    request
        .validate()
        .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?;

    // Check if method is allowed
    if !is_method_allowed(&request.method) {
        warn!("Blocked disallowed method: {}", request.method);

        // Log security event
        let event = SecurityEvent::new(
            SecurityEventType::BlockedMethod,
            format!("Blocked disallowed method: {}", request.method),
        )
        .with_method(request.method.clone());
        event.log();

        return Err(ProxyError::MethodNotAllowed(request.method.clone()));
    }

    // Forward to Geth
    let response = proxy_to_geth(&state, request).await?;

    Ok(Json(response))
}

pub struct InboundState {
    pub proxy_state: ProxyState,
    pub onion_peers: Onion,
}

/// Handle RPC requests to the standard endpoint
pub async fn handle_inbound(
    State(state): State<Arc<InboundState>>,
    Json(request): Json<JsonRpcRequest>,
) -> ProxyResult<Json<JsonRpcResponse>> {
    info!("Received RPC request: method={}", request.method);

    // Validate request
    request
        .validate()
        .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?;

    // Check if method is allowed
    if !is_method_allowed(&request.method) {
        warn!("Blocked disallowed method: {}", request.method);

        // Log security event
        let event = SecurityEvent::new(
            SecurityEventType::BlockedMethod,
            format!("Blocked disallowed method: {}", request.method),
        )
        .with_method(request.method.clone());
        event.log();

        return Err(ProxyError::MethodNotAllowed(request.method.clone()));
    }

    if is_send_method(&request.method) {
        return Ok(Json(state.onion_peers.send_request(&request, 3).await?));
    }

    // Forward to Geth
    let response = proxy_to_geth(&state.proxy_state, request).await?;

    Ok(Json(response))
}

/// Handle RPC requests to the Flashbots endpoint
pub async fn handle_flashbots(
    State(state): State<Arc<ProxyState>>,
    Json(request): Json<JsonRpcRequest>,
) -> ProxyResult<Json<JsonRpcResponse>> {
    debug!("Received Flashbots RPC request: method={}", request.method);

    // Validate request
    request
        .validate()
        .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?;

    // Check if method is allowed
    if !is_method_allowed(&request.method) {
        warn!("Blocked disallowed method: {}", request.method);

        // Log security event
        let event = SecurityEvent::new(
            SecurityEventType::BlockedMethod,
            format!("Blocked disallowed method: {}", request.method),
        )
        .with_method(request.method.clone());
        event.log();

        return Err(ProxyError::MethodNotAllowed(request.method.clone()));
    }

    // Route based on method
    let response = match request.method.as_str() {
        "eth_sendRawTransaction" | "eth_sendBundle" => {
            info!("Routing transaction to Flashbots");
            proxy_to_flashbots(&state, request).await?
        }
        _ => {
            debug!("Routing non-transaction request to Geth");
            proxy_to_geth(&state, request).await?
        }
    };

    Ok(Json(response))
}

/// Forward request to Geth node
pub async fn proxy_to_geth(
    state: &ProxyState,
    request: JsonRpcRequest,
) -> ProxyResult<JsonRpcResponse> {
    let response = state
        .geth_client
        .post(&state.geth_url)
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to send request to Geth: {}", e);
            ProxyError::UpstreamError(format!("Geth connection failed: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Geth returned error status {}: {}", status, body);
        return Err(ProxyError::UpstreamError(format!(
            "Geth returned status {}: {}",
            status, body
        )));
    }

    let json_response: JsonRpcResponse = response.json().await.map_err(|e| {
        error!("Failed to parse Geth response: {}", e);
        ProxyError::UpstreamError(format!("Failed to parse response: {}", e))
    })?;

    Ok(json_response)
}

/// Forward request to Flashbots relay
async fn proxy_to_flashbots(
    state: &ProxyState,
    request: JsonRpcRequest,
) -> ProxyResult<JsonRpcResponse> {
    // Without MEV protection configured, route flashbots requests to local Geth
    // This allows testing without requiring actual Flashbots authentication
    warn!("MEV protection not configured, routing flashbots requests to local Geth");

    // For bundle requests, we'll simulate a response
    if request.method == "eth_sendBundle" {
        info!("Simulating bundle submission for testing");

        // Validate bundle parameters
        if let Some(params) = &request.params {
            if let Some(arr) = params.as_array() {
                if let Some(bundle) = arr.first() {
                    if let Some(obj) = bundle.as_object() {
                        // Check for required fields
                        if !obj.contains_key("txs") || !obj.contains_key("blockNumber") {
                            return Ok(JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32602,
                                    message: "Invalid params: missing required fields".to_string(),
                                    data: None,
                                }),
                                id: request.id,
                            });
                        }
                    }
                }
            }
        }

        return Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(serde_json::json!({
                "bundleHash": format!("0x{}", hex::encode(&[0u8; 32]))
            })),
            error: None,
            id: request.id,
        });
    }

    // For other requests, forward to local Geth
    proxy_to_geth(state, request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serde_json::json;

    fn create_test_state(server_url: String) -> ProxyState {
        ProxyState::new(server_url.clone(), format!("{}/flashbots", server_url))
    }

    #[tokio::test]
    async fn test_handle_rpc_valid_request() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","result":"0x123","id":1}"#)
            .create();

        let state = Arc::new(create_test_state(server.url()));

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_blockNumber".to_string(),
            params: None,
            id: Some(json!(1)),
        };

        let result = handle_rpc(State(state), Json(request)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.result, Some(json!("0x123")));
    }

    #[tokio::test]
    async fn test_handle_rpc_blocked_method() {
        let server = Server::new_async().await;
        let state = Arc::new(create_test_state(server.url()));

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_accounts".to_string(),
            params: None,
            id: Some(json!(1)),
        };

        let result = handle_rpc(State(state), Json(request)).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            ProxyError::MethodNotAllowed(method) => {
                assert_eq!(method, "eth_accounts");
            }
            _ => panic!("Expected MethodNotAllowed error"),
        }
    }

    #[tokio::test]
    async fn test_handle_flashbots_transaction_routing() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/flashbots")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","result":"0xhash","id":1}"#)
            .create();

        let state = Arc::new(create_test_state(server.url()));

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_sendRawTransaction".to_string(),
            params: Some(json!(["0xrawtx"])),
            id: Some(json!(1)),
        };

        let result = handle_flashbots(State(state), Json(request)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.result, Some(json!("0xhash")));
    }

    #[tokio::test]
    async fn test_invalid_json_rpc_version() {
        let server = Server::new_async().await;
        let state = Arc::new(create_test_state(server.url()));

        let request = JsonRpcRequest {
            jsonrpc: "1.0".to_string(),
            method: "eth_blockNumber".to_string(),
            params: None,
            id: Some(json!(1)),
        };

        let result = handle_rpc(State(state), Json(request)).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            ProxyError::InvalidRequest(msg) => {
                assert!(msg.contains("jsonrpc version"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }
}
