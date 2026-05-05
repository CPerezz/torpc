//! Middleware-stack-composition tests.
//!
//! Covers interactions between the security middlewares — specifically the
//! JSON-RPC timeout layer, the security headers, and the body-size limit —
//! that are individually unit-tested elsewhere but whose composition is
//! easy to break (e.g. a layer ordering change that drops headers from
//! timeout-rejected responses).
//!
//! All tests run as part of `make test` and don't require running services.

use axum::{extract::DefaultBodyLimit, middleware, routing::post, Router};
use axum_test::TestServer;
use serde_json::json;
use std::time::Duration;
use torpc::security::{
    json_rpc_timeout_middleware, security_headers_middleware, STATIC_CSP,
};

async fn slow_handler() -> &'static str {
    tokio::time::sleep(Duration::from_millis(500)).await;
    "should never be reached"
}

async fn echo_handler(axum::Json(body): axum::Json<serde_json::Value>) -> axum::Json<serde_json::Value> {
    axum::Json(body)
}

fn build_router(timeout: Duration, body_limit: usize) -> Router {
    let csp = axum::http::HeaderValue::from_static(STATIC_CSP);

    Router::new()
        .route("/slow", post(slow_handler))
        .route("/echo", post(echo_handler))
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(middleware::from_fn_with_state(timeout, json_rpc_timeout_middleware))
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            csp,
        ))
}

/// JSON-RPC timeouts must keep the security headers attached. A bad layer
/// ordering would leave the 504 response naked, which is fingerprinting-
/// adjacent on a Tor-exposed endpoint.
#[tokio::test]
async fn timeout_response_carries_security_headers() {
    let server = TestServer::new(build_router(Duration::from_millis(50), 1024 * 1024)).unwrap();
    let response = server.post("/slow").await;

    assert_eq!(response.status_code(), 504);
    assert_eq!(response.header("content-type"), "application/json");
    assert_eq!(response.header("x-content-type-options"), "nosniff");
    assert_eq!(response.header("x-frame-options"), "DENY");
    assert!(response
        .header("content-security-policy")
        .to_str()
        .unwrap()
        .contains("default-src 'self'"));

    let body: serde_json::Value = response.json();
    assert_eq!(body["error"]["code"], -32001);
    assert_eq!(body["error"]["message"], "upstream timeout");
}

/// Body-limit rejections must also keep security headers — same reasoning
/// as above. A 413 leaking out without `Cache-Control: no-store` would
/// surrender response caching control to intermediaries.
#[tokio::test]
async fn body_limit_response_carries_security_headers() {
    let server = TestServer::new(build_router(Duration::from_secs(5), 64)).unwrap();
    let response = server
        .post("/echo")
        .json(&json!({ "data": "x".repeat(2_000) }))
        .await;

    assert_eq!(response.status_code(), 413);
    assert_eq!(response.header("x-content-type-options"), "nosniff");
    assert_eq!(response.header("cache-control"), "no-store, no-cache, must-revalidate");
}

/// Sanity: a normal request below the body limit and within the timeout
/// passes through and still gets headers applied.
#[tokio::test]
async fn happy_path_response_has_security_headers() {
    let server = TestServer::new(build_router(Duration::from_secs(5), 1024 * 1024)).unwrap();
    let response = server
        .post("/echo")
        .json(&json!({"jsonrpc": "2.0", "method": "echo", "id": 1}))
        .await;

    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("x-frame-options"), "DENY");
    let body: serde_json::Value = response.json();
    assert_eq!(body["method"], "echo");
}
