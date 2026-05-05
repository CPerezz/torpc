use axum::{extract::State, Json};
use reqwest::Client;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::{
    error::{ProxyError, ProxyResult},
    mev::retry::CircuitBreaker,
    rate_limit::{RateLimitConfig, RateLimiter},
    rpc_types::{JsonRpcRequest, JsonRpcResponse},
    security::SecurityMetrics,
    whitelist::is_method_allowed,
};

/// Methods that submit transactions and therefore deserve a strict secondary
/// rate-limit on top of the per-port bucket. These are the only RPC paths
/// where abuse has financial consequences (gas spent, MEV bundles consumed),
/// so we cap them separately at a much smaller request volume.
pub const WRITE_METHODS: &[&str] = &["eth_sendRawTransaction", "eth_sendBundle"];

/// Default budget for the write-method limiter (per *method*, globally —
/// not per source). This is a fail-safe: even if 1 000 ports each pass the
/// per-port read budget, only `WRITE_METHOD_DEFAULT_REQUESTS` write calls
/// total succeed in any one window. Operators tune via env.
pub const WRITE_METHOD_DEFAULT_REQUESTS: u32 = 10;
pub const WRITE_METHOD_DEFAULT_WINDOW_SECS: u64 = 60;

/// Shared state for the JSON-RPC proxy. Cloned per-request via `Arc` (the
/// `Client` carries its own `Arc` internally so cloning is cheap), so any
/// fields added here should be cheaply cloneable or themselves `Arc`-wrapped.
///
/// Note: the daemon used to also carry a `HealthCache` here — a cached
/// upstream-Geth probe used by `/health`. That was deleted alongside the
/// component-state JSON in `/health` itself: the topology this daemon runs
/// in (Tor hidden service, no LB) doesn't have a consumer that benefits
/// from per-component health distinctions. Operators wanting Geth liveness
/// either curl Geth directly or read the `geth_circuit` field exposed by
/// `/metrics`.
#[derive(Clone)]
pub struct ProxyState {
    pub geth_client: Client,
    pub geth_url: String,
    pub flashbots_url: String,
    /// Live-incremented security counters surfaced by `/metrics`.
    pub metrics: Arc<SecurityMetrics>,
    /// Wall-clock anchor for `/health` uptime reporting.
    pub start_time: Instant,
    /// Circuit breaker around upstream Geth. Without this, when Geth is down
    /// every request waits the full 30s reqwest timeout, blocking handler
    /// threads and creating a thundering-herd retry storm. The breaker
    /// fast-fails after 5 consecutive failures and recovers after 30s.
    pub geth_circuit: Arc<CircuitBreaker>,
    /// Strict secondary rate-limit for transaction-submitting methods.
    /// Keyed by method name (so each write method has its own bucket); much
    /// stricter than the per-port limit because the consequences of abuse
    /// are financial. See `WRITE_METHODS` for the gated set.
    pub write_method_limiter: Arc<RateLimiter>,
}

impl ProxyState {
    /// Build a fresh ProxyState. Returns an error rather than panicking on
    /// HTTP-client construction failure, which surfaces in stripped containers
    /// where TLS roots aren't bundled — previously the daemon would panic-loop
    /// inside systemd until the operator noticed.
    pub fn new(geth_url: String, flashbots_url: String) -> Result<Self, ProxyError> {
        let geth_client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProxyError::InternalError(format!("HTTP client init failed: {}", e)))?;

        let write_method_limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            max_requests: WRITE_METHOD_DEFAULT_REQUESTS,
            window_duration: Duration::from_secs(WRITE_METHOD_DEFAULT_WINDOW_SECS),
        }));

        Ok(Self {
            geth_client,
            geth_url,
            flashbots_url,
            metrics: Arc::new(SecurityMetrics::new()),
            start_time: Instant::now(),
            geth_circuit: Arc::new(CircuitBreaker::new()),
            write_method_limiter,
        })
    }

    /// Construct a `ProxyState` with an explicit write-method limiter
    /// configuration. Used by `main.rs` to honour `WRITE_RATE_LIMIT_*` env
    /// vars; tests stick with `new()` and the documented defaults.
    pub fn new_with_write_limit(
        geth_url: String,
        flashbots_url: String,
        max_requests: u32,
        window: Duration,
    ) -> Result<Self, ProxyError> {
        let mut state = Self::new(geth_url, flashbots_url)?;
        state.write_method_limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            max_requests,
            window_duration: window,
        }));
        Ok(state)
    }

    /// Returns `Err(RateLimitExceeded)` when `method` is on the write
    /// allow-list and the per-method bucket is exhausted. Increments the
    /// `rate_limit_hits` metric on rejection so `/metrics` reports it.
    pub async fn check_write_method_rate_limit(&self, method: &str) -> ProxyResult<()> {
        if !WRITE_METHODS.contains(&method) {
            return Ok(());
        }
        if self.write_method_limiter.check_rate_limit(method).await {
            return Ok(());
        }
        warn!(
            "write-method rate limit exceeded for {} (global budget)",
            method
        );
        self.metrics.increment_rate_limit_hits();
        Err(ProxyError::RateLimitExceeded)
    }
}

/// Handle RPC requests to the standard endpoint
pub async fn handle_rpc(
    State(state): State<Arc<ProxyState>>,
    Json(request): Json<JsonRpcRequest>,
) -> ProxyResult<Json<JsonRpcResponse>> {
    debug!("Received RPC request: method={}", request.method);
    
    // Validate request
    request.validate()
        .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?;
    
    // Check if method is allowed
    if !is_method_allowed(&request.method) {
        state.metrics.increment_invalid_methods();
        state.metrics.increment_blocked_requests();
        // Structured warn — replaces the prior `SecurityEvent` envelope.
        // SIEM-style ingestion can pick the `event_type` and `method` fields
        // out of the JSON tracing output without a typed enum.
        warn!(
            event_type = "blocked_method",
            method = %request.method,
            "blocked disallowed JSON-RPC method"
        );
        return Err(ProxyError::MethodNotAllowed(request.method.clone()));
    }

    state.check_write_method_rate_limit(&request.method).await?;

    let response = proxy_to_geth(&state, request).await?;

    Ok(Json(response))
}

/// Handle RPC requests to the Flashbots endpoint
pub async fn handle_flashbots(
    State(state): State<Arc<ProxyState>>,
    Json(request): Json<JsonRpcRequest>,
) -> ProxyResult<Json<JsonRpcResponse>> {
    debug!("Received Flashbots RPC request: method={}", request.method);
    
    // Validate request
    request.validate()
        .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?;
    
    // Check if method is allowed
    if !is_method_allowed(&request.method) {
        state.metrics.increment_invalid_methods();
        state.metrics.increment_blocked_requests();
        // Structured warn — replaces the prior `SecurityEvent` envelope.
        // SIEM-style ingestion can pick the `event_type` and `method` fields
        // out of the JSON tracing output without a typed enum.
        warn!(
            event_type = "blocked_method",
            method = %request.method,
            "blocked disallowed JSON-RPC method"
        );
        return Err(ProxyError::MethodNotAllowed(request.method.clone()));
    }

    state.check_write_method_rate_limit(&request.method).await?;

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

/// Forward request to the upstream Geth node, gated by the per-state circuit
/// breaker. When Geth is unhealthy the breaker fail-fasts subsequent requests
/// rather than letting them all wait the full reqwest timeout — which would
/// otherwise tie up handler threads and synchronously stall the rate limiter.
pub async fn proxy_to_geth(
    state: &ProxyState,
    request: JsonRpcRequest,
) -> ProxyResult<JsonRpcResponse> {
    if !state.geth_circuit.can_proceed().await {
        warn!("upstream-Geth circuit is open; failing fast");
        return Err(ProxyError::UpstreamError(
            "Upstream Ethereum node temporarily unavailable (circuit breaker open)".to_string(),
        ));
    }

    let response_result = state
        .geth_client
        .post(&state.geth_url)
        .json(&request)
        .send()
        .await;

    let response = match response_result {
        Ok(resp) => resp,
        Err(e) => {
            error!("Failed to send request to Geth: {}", e);
            state.geth_circuit.record_failure().await;
            return Err(ProxyError::UpstreamError(format!(
                "Geth connection failed: {}",
                e
            )));
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Geth returned error status {}: {}", status, body);
        state.geth_circuit.record_failure().await;
        return Err(ProxyError::UpstreamError(format!(
            "Geth returned status {}: {}",
            status, body
        )));
    }

    let json_response: JsonRpcResponse = match response.json().await {
        Ok(j) => j,
        Err(e) => {
            error!("Failed to parse Geth response: {}", e);
            state.geth_circuit.record_failure().await;
            return Err(ProxyError::UpstreamError(format!(
                "Failed to parse response: {}",
                e
            )));
        }
    };

    state.geth_circuit.record_success().await;
    Ok(json_response)
}

/// Forward request to Flashbots relay.
///
/// Without `FLASHBOTS_SIGNING_KEY` configured, bundle submissions cannot be
/// authenticated to a real relay. Returning a fake `bundleHash` would mask the
/// misconfiguration and have wallets wait on a hash that will never confirm,
/// so we surface a JSON-RPC error instead. Non-bundle requests are still
/// forwarded to local Geth so wallets can test the path.
async fn proxy_to_flashbots(
    state: &ProxyState,
    request: JsonRpcRequest,
) -> ProxyResult<JsonRpcResponse> {
    if request.method == "eth_sendBundle" {
        warn!("eth_sendBundle received but MEV signing key not configured");
        return Ok(JsonRpcResponse::error(
            request.id,
            -32004,
            "MEV protection not configured: set FLASHBOTS_SIGNING_KEY to enable bundle submission".to_string(),
            None,
        ));
    }

    debug!("Forwarding non-bundle Flashbots request to local Geth");
    proxy_to_geth(state, request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serde_json::json;

    fn create_test_state(server_url: String) -> ProxyState {
        ProxyState::new(server_url.clone(), format!("{}/flashbots", server_url))
            .expect("ProxyState::new must succeed in tests")
    }

    /// Per-method limiter must allow non-write methods unconditionally and
    /// reject write methods only after the configured budget is consumed.
    #[tokio::test]
    async fn test_check_write_method_rate_limit_allows_reads() {
        let state = ProxyState::new_with_write_limit(
            "http://invalid".to_string(),
            "http://invalid".to_string(),
            1, // brutal: 1 write per window
            Duration::from_secs(60),
        )
        .unwrap();

        // Reads are unlimited (the per-port limiter handles them, not this).
        for _ in 0..50 {
            assert!(
                state
                    .check_write_method_rate_limit("eth_blockNumber")
                    .await
                    .is_ok()
            );
        }
    }

    #[tokio::test]
    async fn test_check_write_method_rate_limit_blocks_write_burst() {
        let state = ProxyState::new_with_write_limit(
            "http://invalid".to_string(),
            "http://invalid".to_string(),
            2,
            Duration::from_secs(60),
        )
        .unwrap();

        // Per-method counters are independent: each write method has its
        // own bucket of 2 before tripping.
        for method in &["eth_sendRawTransaction", "eth_sendBundle"] {
            assert!(state.check_write_method_rate_limit(method).await.is_ok());
            assert!(state.check_write_method_rate_limit(method).await.is_ok());
            let err = state
                .check_write_method_rate_limit(method)
                .await
                .expect_err("3rd call should be rate-limited");
            assert!(matches!(err, ProxyError::RateLimitExceeded));
        }

        // Metric increments fire on every rejection — `/metrics` should
        // report `rate_limit_hits == 2` (one per method).
        use std::sync::atomic::Ordering::Relaxed;
        assert_eq!(state.metrics.rate_limit_hits.load(Relaxed), 2);
    }

    #[tokio::test]
    async fn test_handle_rpc_valid_request() {
        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/")
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

    /// When MEV is not configured, `handle_flashbots` falls back to local Geth
    /// for `eth_sendRawTransaction`. The mock must therefore intercept the path
    /// that `proxy_to_geth` actually posts to (the bare `geth_url`, not the
    /// `flashbots_url`). This test was previously orphaned — `make test` runs
    /// integration suites only, not `cargo test --lib` — and silently broken.
    #[tokio::test]
    async fn test_handle_flashbots_raw_tx_falls_back_to_geth() {
        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/")
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
    async fn test_handle_flashbots_send_bundle_without_mev_returns_error() {
        let server = Server::new_async().await;
        let state = Arc::new(create_test_state(server.url()));

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_sendBundle".to_string(),
            params: Some(json!([{ "txs": ["0x"], "blockNumber": "0x1" }])),
            id: Some(json!(7)),
        };

        let response = handle_flashbots(State(state), Json(request)).await.unwrap().0;
        let err = response.error.expect("expected JSON-RPC error when MEV not configured");
        assert_eq!(err.code, -32004);
        assert!(err.message.contains("MEV protection not configured"));
        assert_eq!(response.id, Some(json!(7)));
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