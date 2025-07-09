use axum::{
    http::StatusCode,
    middleware,
    routing::post,
    Router,
};
use axum_test::TestServer;
use serde_json::json;
use std::sync::Arc;
use torpc::{
    mev_handler::{handle_flashbots_with_mev, MevProxyState},
    proxy::{handle_rpc, ProxyState},
    security::{build_security_layers, security_headers_middleware, SecurityConfig},
};

async fn create_test_app() -> Router {
    let base_state = Arc::new(ProxyState::new(
        "http://localhost:8545".to_string(),
        "http://localhost:8545/flashbots".to_string(),
    ));
    
    let mev_state = Arc::new(MevProxyState {
        base_state: base_state.clone(),
        mev_client: None,
    });
    
    let security_config = SecurityConfig::default();
    
    Router::new()
        .route("/rpc", post({
            let base_state = base_state.clone();
            move |axum::extract::State(_): axum::extract::State<Arc<MevProxyState>>, req| {
                let state = base_state.clone();
                async move {
                    handle_rpc(axum::extract::State(state), req).await
                }
            }
        }))
        .with_state(mev_state)
        .layer(build_security_layers(security_config))
        .layer(middleware::from_fn(security_headers_middleware))
}

#[tokio::test]
async fn test_security_headers_on_success_response() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "id": 1
        }))
        .await;
    
    // Verify security headers are present
    assert_eq!(response.header("X-Content-Type-Options"), "nosniff");
    assert_eq!(response.header("X-Frame-Options"), "DENY");
    assert_eq!(response.header("X-XSS-Protection"), "0");
    assert_eq!(response.header("Referrer-Policy"), "no-referrer");
    assert_eq!(response.header("Cache-Control"), "no-store, no-cache, must-revalidate, private");
    assert_eq!(response.header("Pragma"), "no-cache");
    assert!(response.header("Content-Security-Policy").to_str().unwrap().contains("default-src 'none'"));
    
    // Verify server header is overridden
    assert_eq!(response.header("Server"), "torpc");
}

#[tokio::test]
async fn test_security_headers_on_error_response() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // Send invalid request to trigger error
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "1.0", // Invalid version
            "method": "eth_blockNumber",
            "id": 1
        }))
        .await;
    
    // Verify security headers are present even on error
    assert_eq!(response.header("X-Content-Type-Options"), "nosniff");
    assert_eq!(response.header("X-Frame-Options"), "DENY");
    assert_eq!(response.header("Referrer-Policy"), "no-referrer");
}

#[tokio::test]
async fn test_request_size_limit() {
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    // Create a large payload (over 512KB default limit)
    let large_data = "x".repeat(600 * 1024); // 600KB
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "data": large_data
        }],
        "id": 1
    });
    
    let response = server
        .post("/rpc")
        .json(&payload)
        .await;
    
    // Should be rejected due to size
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_cors_headers_present() {
    std::env::set_var("STRICT_SECURITY_HEADERS", "false");
    
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "id": 1
        }))
        .await;
    
    // Verify CORS headers when not in strict mode
    assert_eq!(response.header("Access-Control-Allow-Origin"), "*");
    assert_eq!(response.header("Access-Control-Allow-Methods"), "POST, OPTIONS");
    assert_eq!(response.header("Access-Control-Allow-Headers"), "Content-Type, Authorization");
    
    std::env::remove_var("STRICT_SECURITY_HEADERS");
}

#[tokio::test]
async fn test_strict_headers_no_cors() {
    std::env::set_var("STRICT_SECURITY_HEADERS", "true");
    
    let app = create_test_app().await;
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "id": 1
        }))
        .await;
    
    // Verify no CORS headers in strict mode
    assert!(response.header("Access-Control-Allow-Origin").is_empty());
    
    // But security headers should still be present
    assert_eq!(response.header("X-Frame-Options"), "DENY");
    assert_eq!(response.header("Referrer-Policy"), "no-referrer");
    
    std::env::remove_var("STRICT_SECURITY_HEADERS");
}