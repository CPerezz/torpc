use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::json;
use std::sync::Arc;
use torpc::{
    proxy::{ProxyState, handle_rpc},
    security::{
        SecurityConfig, monitor_request_patterns,
        security_headers_middleware, health_check, security_metrics
    },
    mev_handler::MevProxyState,
};
use axum::{
    routing::{get, post},
    Router,
    middleware,
    extract::DefaultBodyLimit,
};

async fn create_test_app_with_security() -> Router {
    let base_state = Arc::new(ProxyState::new(
        "http://localhost:8545".to_string(),
        "http://localhost:8545".to_string(),
    ));
    
    let mev_state = Arc::new(MevProxyState {
        base_state: base_state.clone(),
        mev_client: None,
    });
    
    let security_config = SecurityConfig::default();
    
    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(security_metrics))
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
        .layer(DefaultBodyLimit::max(security_config.max_body_size))
        .layer(middleware::from_fn(monitor_request_patterns))
        .layer(middleware::from_fn(security_headers_middleware))
}

#[tokio::test]
async fn test_security_headers_present() {
    let app = create_test_app_with_security().await;
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "id": 1
        }))
        .await;
    
    let headers = response.headers();
    
    // Check all security headers are present
    assert_eq!(headers.get("X-Content-Type-Options").unwrap(), "nosniff");
    assert_eq!(headers.get("X-Frame-Options").unwrap(), "DENY");
    assert_eq!(headers.get("X-XSS-Protection").unwrap(), "0");
    assert_eq!(headers.get("Referrer-Policy").unwrap(), "no-referrer");
    assert_eq!(headers.get("Content-Security-Policy").unwrap(), "default-src 'none'; frame-ancestors 'none'");
    assert_eq!(headers.get("Cache-Control").unwrap(), "no-store, no-cache, must-revalidate");
    assert_eq!(headers.get("Pragma").unwrap(), "no-cache");
    assert_eq!(headers.get("Expires").unwrap(), "0");
    assert_eq!(headers.get("X-Service").unwrap(), "TorPC");
    
    // Server header should be removed
    assert!(headers.get("Server").is_none());
}

#[tokio::test]
async fn test_blocked_method_security_logging() {
    let app = create_test_app_with_security().await;
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_accounts",  // This method should be blocked
            "id": 1
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    
    // Verify response still has security headers even on error
    let headers = response.headers();
    assert_eq!(headers.get("X-Content-Type-Options").unwrap(), "nosniff");
    assert_eq!(headers.get("X-Frame-Options").unwrap(), "DENY");
}

#[tokio::test]
async fn test_health_check_endpoint() {
    let app = create_test_app_with_security().await;
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    
    let json_response: serde_json::Value = response.json();
    assert_eq!(json_response["status"], "healthy");
    assert_eq!(json_response["service"], "torpc");
    assert!(json_response["timestamp"].is_string());
    assert_eq!(json_response["components"]["proxy"], "ok");
    assert_eq!(json_response["components"]["handlers"], "ok");
}

#[tokio::test]
async fn test_security_metrics_endpoint() {
    let app = create_test_app_with_security().await;
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/metrics").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    
    let json_response: serde_json::Value = response.json();
    assert!(json_response["security_metrics"].is_object());
    assert!(json_response["timestamp"].is_string());
    
    // Check that metrics structure exists
    let metrics = &json_response["security_metrics"];
    assert!(metrics["blocked_requests_total"].is_number());
    assert!(metrics["rate_limit_hits"].is_number());
    assert!(metrics["oversized_requests"].is_number());
    assert!(metrics["invalid_methods"].is_number());
    assert!(metrics["suspicious_patterns"].is_number());
}

#[tokio::test]
async fn test_request_size_limit() {
    let app = create_test_app_with_security().await;
    let server = TestServer::new(app).unwrap();
    
    // Create a large JSON payload that exceeds the 1MB default limit
    let large_data = "x".repeat(2 * 1024 * 1024); // 2MB string
    
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [large_data],
            "id": 1
        }))
        .await;
    
    // Should be rejected due to size limit
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_invalid_json_rpc_request() {
    let app = create_test_app_with_security().await;
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "1.0",  // Invalid version
            "method": "eth_blockNumber",
            "id": 1
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    
    // Verify security headers are still present on error responses
    let headers = response.headers();
    assert_eq!(headers.get("X-Content-Type-Options").unwrap(), "nosniff");
}

#[tokio::test] 
async fn test_security_config_from_env() {
    // Test default configuration
    let config = SecurityConfig::default();
    assert_eq!(config.max_body_size, 1024 * 1024); // 1MB
    assert_eq!(config.request_timeout.as_secs(), 30);
    assert_eq!(config.strict_headers, true);
    
    // Test environment variable override
    std::env::set_var("MAX_BODY_SIZE", "2097152"); // 2MB
    std::env::set_var("REQUEST_TIMEOUT", "60");
    std::env::set_var("STRICT_HEADERS", "false");
    
    let env_config = SecurityConfig::from_env();
    assert_eq!(env_config.max_body_size, 2097152);
    assert_eq!(env_config.request_timeout.as_secs(), 60);
    assert_eq!(env_config.strict_headers, false);
    
    // Clean up
    std::env::remove_var("MAX_BODY_SIZE");
    std::env::remove_var("REQUEST_TIMEOUT");
    std::env::remove_var("STRICT_HEADERS");
}