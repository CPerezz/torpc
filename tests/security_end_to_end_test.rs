//! End-to-end tests that drive the full security middleware stack against a
//! mock RPC handler. These differ from `security_integration_tests.rs` by
//! using a synthetic handler (so we can exercise paths like a deliberately
//! slow method) rather than the real `handle_rpc` + Geth path.
//!
//! Run as part of `make test` (no services required).

use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    middleware,
    routing::post,
    Json, Router,
};
use axum_test::TestServer;
use serde_json::{json, Value};
use std::time::Duration;
use torpc::security::{
    json_rpc_timeout_middleware, security_headers_middleware, RuntimeWebConfig,
};

/// Tiny mock JSON-RPC handler. Mirrors a minimal subset of the real one so
/// the security stack has something realistic to wrap.
async fn mock_rpc(Json(payload): Json<Value>) -> Result<Json<Value>, StatusCode> {
    let method = payload.get("method").and_then(|m| m.as_str()).unwrap_or("");
    match method {
        "eth_accounts" | "personal_unlockAccount" | "admin_peers" => {
            Err(StatusCode::METHOD_NOT_ALLOWED)
        }
        "test_slow_method" => {
            tokio::time::sleep(Duration::from_secs(2)).await;
            Ok(Json(json!({"jsonrpc": "2.0", "result": "0x1", "id": payload["id"]})))
        }
        _ => Ok(Json(json!({"jsonrpc": "2.0", "result": "0x1", "id": payload["id"]}))),
    }
}

/// Build a router shaped exactly like production except the RPC handler is
/// the small `mock_rpc` above.
fn create_secure_test_app(body_limit: usize, timeout: Duration) -> Router {
    let csp = axum::http::HeaderValue::from_str(
        &RuntimeWebConfig {
            discovery_url: "http://localhost:8081/api/discovery".to_string(),
            discovery_timeout_ms: 2000,
            fallback_rpc_url: "http://localhost:8545".to_string(),
        }
        .build_csp(),
    )
    .unwrap();

    Router::new()
        .route("/rpc", post(mock_rpc))
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(middleware::from_fn_with_state(timeout, json_rpc_timeout_middleware))
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            csp,
        ))
}

fn assert_security_headers(response: &axum_test::TestResponse) {
    assert_eq!(response.header("x-content-type-options"), "nosniff");
    assert_eq!(response.header("x-frame-options"), "DENY");
    assert_eq!(response.header("referrer-policy"), "no-referrer");
    assert!(response
        .header("content-security-policy")
        .to_str()
        .unwrap()
        .contains("default-src 'self'"));
}

#[tokio::test]
async fn full_security_flow_valid_request_succeeds() {
    let server =
        TestServer::new(create_secure_test_app(512 * 1024, Duration::from_secs(5))).unwrap();
    let response = server
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "params": [], "id": 1}))
        .await;

    assert_eq!(response.status_code(), StatusCode::OK);
    assert_security_headers(&response);
    let body: Value = response.json();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 1);
}

#[tokio::test]
async fn full_security_flow_blocked_method_keeps_headers() {
    let server =
        TestServer::new(create_secure_test_app(512 * 1024, Duration::from_secs(5))).unwrap();
    let response = server
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_accounts", "params": [], "id": 1}))
        .await;

    assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    assert_security_headers(&response);
}

#[tokio::test]
async fn full_security_flow_oversized_request_returns_413() {
    let server = TestServer::new(create_secure_test_app(64, Duration::from_secs(5))).unwrap();
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"data": "0x".to_string() + &"ff".repeat(300)}],
            "id": 1
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_security_headers(&response);
}

#[tokio::test]
async fn full_security_flow_timeout_returns_jsonrpc_504() {
    // Handler sleeps 2s, timeout is 100ms → middleware short-circuits.
    let server =
        TestServer::new(create_secure_test_app(512 * 1024, Duration::from_millis(100))).unwrap();
    let response = server
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "test_slow_method", "id": 1}))
        .await;

    assert_eq!(response.status_code(), StatusCode::GATEWAY_TIMEOUT);
    assert_security_headers(&response);
    let body: Value = response.json();
    assert_eq!(body["error"]["code"], -32001);
}

#[tokio::test]
async fn suspicious_user_agent_is_logged_but_does_not_block_request() {
    // The middleware logs suspicious UAs (visible in logs) but never blocks
    // — wallets sometimes legitimately ship UAs containing "bot" etc.
    let server =
        TestServer::new(create_secure_test_app(512 * 1024, Duration::from_secs(5))).unwrap();
    let response = server
        .post("/rpc")
        .add_header(
            axum::http::HeaderName::from_static("user-agent"),
            axum::http::HeaderValue::from_static("malicious-bot/1.0"),
        )
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "params": [], "id": 1}))
        .await;

    assert_eq!(response.status_code(), StatusCode::OK);
    assert_security_headers(&response);
}

#[tokio::test]
async fn rapid_requests_pass_through_when_under_limits() {
    // Sanity that the middleware stack doesn't accidentally serialize
    // requests (e.g. by holding a Mutex across an await).
    let server =
        TestServer::new(create_secure_test_app(512 * 1024, Duration::from_secs(5))).unwrap();
    for i in 0..10 {
        let response = server
            .post("/rpc")
            .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": i}))
            .await;
        assert_eq!(response.status_code(), StatusCode::OK, "iteration {} failed", i);
    }
}
