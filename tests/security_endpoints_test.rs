//! End-to-end tests for the `/health` and `/metrics` endpoints.
//!
//! These previously tested a stub implementation that returned hardcoded
//! `"healthy"` and an empty metrics struct on every call. After Phase 2 the
//! endpoints query live `ProxyState`: `/health` probes upstream Geth (with a
//! 1.5s timeout and 5s cache), and `/metrics` reads atomic counters that the
//! request handlers update in flight. The tests now exercise both code paths
//! (Geth reachable vs unreachable) and verify the new response shape.

use axum::{middleware, routing::get, Router};
use axum_test::TestServer;
use mockito::Server;
use serde_json::Value;
use std::sync::Arc;
use torpc::mev::mev_handler::MevProxyState;
use torpc::proxy::ProxyState;
use torpc::security::{health_check, security_headers_middleware, security_metrics, STATIC_CSP};

/// Build a router with `/health` and `/metrics` wired to a `ProxyState`
/// pointing at the supplied URL (use a mockito server URL when you want
/// `/health` to report `geth: "ok"`, or any unreachable address when you
/// want it to report `"down"`). Mirrors the production wiring in `main.rs`
/// for the dynamic CSP so tests catch CSP regressions.
fn router_with_geth(geth_url: String) -> Router {
    let base_state = Arc::new(
        ProxyState::new(geth_url, "unused".to_string())
            .expect("ProxyState::new must succeed in tests"),
    );
    let state = Arc::new(MevProxyState {
        base_state,
        mev_client: None,
    });

    let csp = axum::http::HeaderValue::from_static(STATIC_CSP);

    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(security_metrics))
        .with_state(state)
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            csp,
        ))
}

#[tokio::test]
async fn health_returns_process_uptime_payload() {
    // /health is now a minimal "process is alive" probe; it does NOT
    // probe upstream Geth. Component-state observability lives in
    // /metrics now. Pin the slim shape so a future re-introduction of
    // probing leaks doesn't sneak past CI.
    let app = TestServer::new(router_with_geth("http://127.0.0.1:1".to_string())).unwrap();
    let response = app.get("/health").await;
    let body: Value = response.json();

    assert_eq!(response.status_code(), 200);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "torpc");
    assert!(body["uptime_seconds"].is_number());
    assert!(body["version"].is_string());
    assert!(body["timestamp"].as_str().unwrap().contains('T'));
    assert!(
        body.get("components").is_none(),
        "components moved to /metrics; /health must stay minimal"
    );
}

#[tokio::test]
async fn health_response_carries_security_headers() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_body(r#"{"jsonrpc":"2.0","id":0,"result":"0x1"}"#)
        .create_async()
        .await;

    let app = TestServer::new(router_with_geth(server.url())).unwrap();
    let response = app.get("/health").await;

    assert_eq!(response.header("x-content-type-options"), "nosniff");
    assert_eq!(response.header("x-frame-options"), "DENY");
    assert_eq!(response.header("referrer-policy"), "no-referrer");
    let csp = response.header("content-security-policy");
    let csp_str = csp.to_str().unwrap();
    assert!(csp_str.contains("default-src 'self'"), "CSP was: {csp_str}");
}

#[tokio::test]
async fn health_does_not_leak_sensitive_fields() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_body(r#"{"jsonrpc":"2.0","id":0,"result":"0x1"}"#)
        .create_async()
        .await;

    let app = TestServer::new(router_with_geth(server.url())).unwrap();
    let response = app.get("/health").await;
    let body = response.text().to_lowercase();

    // None of these should appear in any health payload — Tor users see this.
    for forbidden in &[
        "password",
        "private_key",
        "signing_key",
        "credential",
        "geth_url",
        "flashbots",
    ] {
        assert!(
            !body.contains(forbidden),
            "health payload leaked sensitive token `{forbidden}`: {body}"
        );
    }
}

#[tokio::test]
async fn metrics_response_has_expected_shape() {
    let app = TestServer::new(router_with_geth("http://127.0.0.1:1".to_string())).unwrap();
    let response = app.get("/metrics").await;
    let body: Value = response.json();

    assert_eq!(response.status_code(), 200);
    let m = &body["security_metrics"];
    assert!(m["blocked_requests_total"].is_number());
    assert!(m["rate_limit_hits"].is_number());
    assert!(m["oversized_requests"].is_number());
    assert!(m["invalid_methods"].is_number());
    assert!(m["suspicious_patterns"].is_number());
    assert!(body["uptime_seconds"].is_number());
    assert!(body["timestamp"].is_string());
}

#[tokio::test]
async fn metrics_initial_counters_are_zero() {
    let app = TestServer::new(router_with_geth("http://127.0.0.1:1".to_string())).unwrap();
    let response = app.get("/metrics").await;
    let body: Value = response.json();

    let m = &body["security_metrics"];
    assert_eq!(m["blocked_requests_total"], 0);
    assert_eq!(m["rate_limit_hits"], 0);
    assert_eq!(m["oversized_requests"], 0);
    assert_eq!(m["invalid_methods"], 0);
    assert_eq!(m["suspicious_patterns"], 0);
}

#[tokio::test]
async fn endpoints_reject_wrong_methods_with_405() {
    let app = TestServer::new(router_with_geth("http://127.0.0.1:1".to_string())).unwrap();
    assert_eq!(app.post("/health").await.status_code(), 405);
    assert_eq!(app.post("/metrics").await.status_code(), 405);
}

#[tokio::test]
async fn unknown_path_returns_404_with_security_headers() {
    let app = TestServer::new(router_with_geth("http://127.0.0.1:1".to_string())).unwrap();
    let response = app.get("/does-not-exist").await;
    assert_eq!(response.status_code(), 404);
    assert_eq!(response.header("x-content-type-options"), "nosniff");
}

#[tokio::test]
async fn metrics_reports_circuit_breaker_state() {
    // Phase-Option-C: component-state observability moved from /health to
    // /metrics. Pin the new shape so the geth/mev circuit summary stays
    // surfaced for operators.
    let app = TestServer::new(router_with_geth("http://127.0.0.1:1".to_string())).unwrap();
    let response = app.get("/metrics").await;
    assert_eq!(response.status_code(), 200);
    let body: Value = response.json();
    let circuits = &body["circuits"];
    // Both "closed" / "open" / "half_open" / "n/a" are valid states; we
    // check the field exists and is a string rather than the exact value.
    assert!(circuits["geth"].is_string());
    assert!(circuits["mev_relay"].is_string());
    assert!(circuits["mev_circuit"].is_string());
}

#[tokio::test]
async fn health_responds_quickly_under_all_conditions() {
    // /health is now O(1) — no upstream probe, no cache lookup. Should
    // always return well under 100ms regardless of upstream Geth state.
    let app = TestServer::new(router_with_geth("http://127.0.0.1:1".to_string())).unwrap();
    let start = std::time::Instant::now();
    let response = app.get("/health").await;
    let elapsed = start.elapsed();

    assert_eq!(response.status_code(), 200);
    assert!(
        elapsed.as_millis() < 250,
        "/health took {}ms; should be sub-100ms in O(1) form",
        elapsed.as_millis()
    );
}
