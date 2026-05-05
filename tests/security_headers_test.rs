//! Header coverage tests.
//!
//! Verifies the always-on `security_headers_middleware` and the dynamic CSP
//! layer that lives next to it in production. These were previously gated
//! by `#[ignore]` because the assertions referenced `default-src 'none'`,
//! which the daemon never emitted; the actual policy was — and is —
//! `default-src 'self'` (so the static frontend can load its own JS/CSS).
//!
//! All tests in this file run as part of `make test`. They don't need a
//! running daemon.

use axum::{
    http::StatusCode,
    middleware,
    routing::{get, post},
    Router,
};
use axum_test::TestServer;
use serde_json::json;
use torpc::security::{security_headers_middleware, STATIC_CSP};

async fn ok_text() -> &'static str {
    "test response"
}

async fn ok_json() -> axum::Json<serde_json::Value> {
    axum::Json(json!({"status": "ok"}))
}

async fn error_handler() -> Result<&'static str, StatusCode> {
    Err(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Builds a router that mirrors `main.rs`'s wiring of the headers middleware
/// + static CSP layer. Tests assert against this exact stack.
fn create_test_router() -> Router {
    let csp = axum::http::HeaderValue::from_static(STATIC_CSP);

    Router::new()
        .route("/text", get(ok_text))
        .route("/json", get(ok_json))
        .route("/error", get(error_handler))
        .route("/post", post(ok_text))
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            csp,
        ))
}

/// Asserts every header `add_security_headers` is contractually committed to
/// emitting. Anti-flake: list the exact expected values rather than just
/// `is_some()`, so a regression in the value (e.g. `DENY` → `SAMEORIGIN`)
/// is caught.
fn assert_security_headers(response: &axum_test::TestResponse) {
    assert_eq!(response.header("x-content-type-options"), "nosniff");
    assert_eq!(response.header("x-frame-options"), "DENY");
    assert_eq!(response.header("x-xss-protection"), "0");
    assert_eq!(response.header("referrer-policy"), "no-referrer");
    assert_eq!(
        response.header("cache-control"),
        "no-store, no-cache, must-revalidate"
    );
    assert_eq!(response.header("pragma"), "no-cache");
    assert_eq!(response.header("expires"), "0");
    assert_eq!(response.header("x-service"), "TorPC");

    let csp = response
        .header("content-security-policy")
        .to_str()
        .expect("CSP header must be UTF-8")
        .to_string();
    assert!(
        csp.contains("default-src 'self'"),
        "CSP should be `'self'`-based: {}",
        csp
    );
    assert!(
        csp.contains("frame-ancestors 'none'"),
        "CSP must keep frame-ancestors locked down: {}",
        csp
    );
    // After RuntimeWebConfig deletion, CSP no longer includes the
    // discovery URL — `connect-src 'self'` covers same-origin /rpc,
    // and the discovery server is itself default-disabled.
    assert!(
        csp.contains("connect-src 'self'"),
        "CSP must allow same-origin connect: {}",
        csp
    );
    assert!(
        !csp.contains("http://localhost:8081"),
        "CSP must NOT hardcode the discovery URL after the RuntimeWebConfig removal: {}",
        csp
    );
}

#[tokio::test]
async fn headers_present_on_get_text() {
    let server = TestServer::new(create_test_router()).unwrap();
    let response = server.get("/text").await;
    assert_eq!(response.status_code(), StatusCode::OK);
    assert_security_headers(&response);
}

#[tokio::test]
async fn headers_present_on_get_json() {
    let server = TestServer::new(create_test_router()).unwrap();
    let response = server.get("/json").await;
    assert_eq!(response.status_code(), StatusCode::OK);
    assert_security_headers(&response);
    let body: serde_json::Value = response.json();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn headers_present_on_post() {
    let server = TestServer::new(create_test_router()).unwrap();
    let response = server.post("/post").await;
    assert_eq!(response.status_code(), StatusCode::OK);
    assert_security_headers(&response);
}

#[tokio::test]
async fn headers_present_on_handler_error() {
    let server = TestServer::new(create_test_router()).unwrap();
    let response = server.get("/error").await;
    assert_eq!(response.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_security_headers(&response);
}

#[tokio::test]
async fn headers_present_on_404() {
    let server = TestServer::new(create_test_router()).unwrap();
    let response = server.get("/no-such-route").await;
    assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
    assert_security_headers(&response);
}

#[tokio::test]
async fn headers_present_on_405() {
    // GET-only routes return 405 on POST; security headers still apply.
    let server = TestServer::new(create_test_router()).unwrap();
    let response = server.post("/text").await;
    assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    assert_security_headers(&response);
}

#[tokio::test]
async fn server_header_is_stripped() {
    // Defensive: `add_security_headers` removes any `Server` header. We
    // never set one, but verify it's absent regardless.
    let server = TestServer::new(create_test_router()).unwrap();
    let response = server.get("/text").await;
    assert!(
        response.maybe_header("server").is_none(),
        "Server header should be stripped to avoid fingerprinting"
    );
}

#[tokio::test]
async fn csp_disallows_remote_scripts_and_inline_default() {
    // Belt-and-suspenders: the CSP must NOT contain `'unsafe-eval'`,
    // `*` source lists, or remote script-src origins.
    let server = TestServer::new(create_test_router()).unwrap();
    let response = server.get("/text").await;
    let csp = response.header("content-security-policy").to_str().unwrap().to_string();

    assert!(!csp.contains("'unsafe-eval'"), "CSP leaks unsafe-eval: {}", csp);
    assert!(!csp.contains("script-src 'self' *"), "CSP wildcards scripts: {}", csp);
    assert!(
        csp.contains("script-src 'self'"),
        "CSP must restrict scripts to self: {}",
        csp
    );
    assert!(
        csp.contains("img-src 'self' data:"),
        "CSP must allow data: images for inline icons: {}",
        csp
    );
}

#[tokio::test]
async fn headers_persist_across_repeated_requests() {
    // Regression guard: the middleware must apply on every response, not
    // just the first one served by an axum-test TestServer.
    let server = TestServer::new(create_test_router()).unwrap();
    for _ in 0..5 {
        let response = server.get("/text").await;
        assert_security_headers(&response);
    }
}
