//! End-to-end integration tests covering the security stack on `/rpc`.
//!
//! Exercises the full middleware stack against a mockito Geth, verifying:
//! - Disallowed methods are blocked with a `405 Method Not Allowed` and
//!   security headers are still applied.
//! - The body-size limit rejects oversized requests with `413`.
//! - Invalid JSON-RPC versions are rejected with `400`.
//! - Live `/health` and `/metrics` endpoints reflect actual state.
//! - Per-method write-rate-limiting kicks in after the configured budget.
//!
//! These tests don't require a running Geth/Tor — they spin up mockito to
//! stand in for upstream Ethereum.

use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    middleware,
    routing::{get, post},
    Router,
};
use axum_test::TestServer;
use mockito::Server;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use torpc::{
    mev::mev_handler::MevProxyState,
    proxy::{handle_rpc, ProxyState},
    security::{health_check, security_headers_middleware, security_metrics, STATIC_CSP},
};

/// Builds a router that mirrors `main.rs`'s wiring of all security
/// concerns, against a `ProxyState` pointing at the supplied Geth URL.
/// Returns the router; tests own the lifetime of the underlying mockito.
fn build_router(geth_url: String, max_body_size: usize, write_limit: u32) -> Router {
    let base_state = Arc::new(
        ProxyState::new_with_write_limit(
            geth_url,
            "unused".to_string(),
            write_limit,
            Duration::from_secs(60),
        )
        .expect("ProxyState::new_with_write_limit must succeed in tests"),
    );

    let mev_state = Arc::new(MevProxyState {
        base_state: base_state.clone(),
        mev_client: None,
    });

    let csp = axum::http::HeaderValue::from_static(STATIC_CSP);

    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(security_metrics))
        .route(
            "/rpc",
            post({
                let base = base_state.clone();
                move |axum::extract::State(_): axum::extract::State<Arc<MevProxyState>>, req| {
                    let state = base.clone();
                    async move { handle_rpc(axum::extract::State(state), req).await }
                }
            }),
        )
        .with_state(mev_state)
        .layer(DefaultBodyLimit::max(max_body_size))
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            csp,
        ))
}

fn mock_geth_block_number(server: &mut mockito::ServerGuard, value: &str) -> mockito::Mock {
    let body = format!(r#"{{"jsonrpc":"2.0","result":"{value}","id":1}}"#);
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create()
}

#[tokio::test]
async fn rpc_forwards_allowed_methods_to_geth_and_attaches_security_headers() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x42");

    let server = TestServer::new(build_router(geth.url(), 1024 * 1024, 100)).unwrap();
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "id": 1,
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::OK);
    let body: serde_json::Value = response.json();
    assert_eq!(body["result"], "0x42");
    assert_eq!(response.header("x-content-type-options"), "nosniff");
    assert_eq!(response.header("x-frame-options"), "DENY");
}

#[tokio::test]
async fn disallowed_method_returns_405_with_security_headers_intact() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = TestServer::new(build_router(geth.url(), 1024 * 1024, 100)).unwrap();
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_accounts",
            "id": 1,
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.header("x-content-type-options"), "nosniff");
}

#[tokio::test]
async fn invalid_jsonrpc_version_is_rejected_with_400() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = TestServer::new(build_router(geth.url(), 1024 * 1024, 100)).unwrap();
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "1.0",
            "method": "eth_blockNumber",
            "id": 1,
        }))
        .await;
    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn body_size_limit_rejects_oversized_requests() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    // Tiny limit so the test stays fast.
    let server = TestServer::new(build_router(geth.url(), 256, 100)).unwrap();
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": ["x".repeat(2_000)],
            "id": 1,
        }))
        .await;
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn live_metrics_reflect_blocked_method_counter() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = TestServer::new(build_router(geth.url(), 1024 * 1024, 100)).unwrap();

    // Trigger a blocked method to bump `invalid_methods` and `blocked_requests_total`.
    let _ = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_accounts",
            "id": 1,
        }))
        .await;

    let metrics: serde_json::Value = server.get("/metrics").await.json();
    let m = &metrics["security_metrics"];
    assert_eq!(m["invalid_methods"], 1);
    assert_eq!(m["blocked_requests_total"], 1);
}

#[tokio::test]
async fn write_method_rate_limit_returns_jsonrpc_error_after_burst() {
    let mut geth = Server::new_async().await;
    // Mock both `eth_blockNumber` (used internally) and `eth_sendRawTransaction`.
    let _m = geth
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","result":"0xdead","id":1}"#)
        .expect_at_least(1)
        .create();

    // Limit the strict-write bucket to 2; the third write must be rejected.
    let server = TestServer::new(build_router(geth.url(), 1024 * 1024, 2)).unwrap();
    let send = || async {
        server
            .post("/rpc")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_sendRawTransaction",
                "params": ["0xdeadbeef"],
                "id": 1,
            }))
            .await
    };

    assert_eq!(send().await.status_code(), StatusCode::OK);
    assert_eq!(send().await.status_code(), StatusCode::OK);
    let third = send().await;
    // `RateLimitExceeded` maps to `429 TOO_MANY_REQUESTS` in the error
    // `IntoResponse` impl.
    assert_eq!(third.status_code(), StatusCode::TOO_MANY_REQUESTS);

    // And the metric must reflect it.
    let metrics: serde_json::Value = server.get("/metrics").await.json();
    assert_eq!(metrics["security_metrics"]["rate_limit_hits"], 1);
}

#[tokio::test]
async fn metrics_endpoint_reports_circuit_state() {
    // After Phase-Option-C the component-state info migrated from /health
    // to /metrics. Verify operators still get the geth/mev circuit
    // summary they need for routing decisions.
    let server = TestServer::new(build_router(
        "http://127.0.0.1:1".to_string(),
        1024 * 1024,
        100,
    ))
    .unwrap();
    let body: serde_json::Value = server.get("/metrics").await.json();
    assert!(body["circuits"]["geth"].is_string());
    assert_eq!(body["circuits"]["mev_relay"], "disabled");
    assert_eq!(
        body["service"].as_str(),
        None,
        "service field belongs in /health, not /metrics"
    );
}
