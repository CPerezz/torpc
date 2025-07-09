use axum::{
    extract::DefaultBodyLimit,
    http::{HeaderName, HeaderValue, StatusCode},
    middleware,
    routing::{get, post},
    Json, Router,
};
use axum_test::TestServer;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use torpc::{
    proxy::{handle_rpc, ProxyState},
    mev_handler::MevProxyState,
    security::{
        health_check, security_metrics, SecurityConfig, 
        build_security_layers, monitor_request_patterns, 
        security_headers_middleware
    },
};

// Mock handler for testing security behavior
async fn mock_rpc_handler(Json(payload): Json<Value>) -> Result<Json<Value>, StatusCode> {
    // Check for test method that should be blocked
    if let Some(method) = payload.get("method").and_then(|m| m.as_str()) {
        match method {
            "eth_accounts" | "personal_unlockAccount" | "admin_peers" => {
                return Err(StatusCode::METHOD_NOT_ALLOWED);
            }
            "test_slow_method" => {
                sleep(Duration::from_secs(2)).await;
            }
            _ => {}
        }
    }
    
    // Echo back the request for valid methods
    Ok(Json(json!({
        "jsonrpc": "2.0",
        "result": "0x1",
        "id": payload.get("id")
    })))
}

// Create a full security-enabled test application
fn create_secure_test_app() -> Router {
    let base_state = Arc::new(ProxyState::new(
        "http://localhost:8545".to_string(),
        "http://localhost:8545".to_string(),
    ));
    
    let mev_state = Arc::new(MevProxyState {
        base_state: base_state.clone(),
        mev_client: None,
    });
    
    let security_config = SecurityConfig {
        max_body_size: 1024 * 512, // 512KB for testing
        request_timeout: Duration::from_secs(5),
        strict_headers: true,
    };
    
    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(security_metrics))
        .route("/rpc", post(mock_rpc_handler))
        .with_state(mev_state)
        .layer(build_security_layers(security_config.clone()))
        .layer(DefaultBodyLimit::max(security_config.max_body_size))
        .layer(middleware::from_fn(monitor_request_patterns))
        .layer(middleware::from_fn(security_headers_middleware))
}

#[tokio::test]
async fn test_complete_security_flow_valid_request() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    let valid_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });
    
    let response = server
        .post("/rpc")
        .json(&valid_request)
        .await;
    
    // Should succeed with all security measures in place
    assert_eq!(response.status_code(), StatusCode::OK);
    
    // Verify security headers are present
    verify_all_security_headers(&response);
    
    // Verify response structure
    let response_json: Value = response.json();
    assert_eq!(response_json["jsonrpc"], "2.0");
    assert_eq!(response_json["id"], 1);
}

#[tokio::test]
async fn test_complete_security_flow_blocked_method() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    let blocked_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_accounts", // This should be blocked
        "params": [],
        "id": 1
    });
    
    let response = server
        .post("/rpc")
        .json(&blocked_request)
        .await;
    
    // Should be blocked
    assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    
    // Security headers should still be present
    verify_all_security_headers(&response);
}

#[tokio::test]
async fn test_complete_security_flow_oversized_request() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    // Create request larger than 512KB limit
    let large_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "data": "0x".to_string() + &"ff".repeat(300_000) // ~600KB
        }],
        "id": 1
    });
    
    let response = server
        .post("/rpc")
        .json(&large_request)
        .await;
    
    // Should be rejected due to size
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
    
    // Security headers should still be present
    verify_all_security_headers(&response);
}

#[tokio::test]
async fn test_complete_security_flow_timeout() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    let slow_request = json!({
        "jsonrpc": "2.0",
        "method": "test_slow_method", // This method delays for 2 seconds
        "params": [],
        "id": 1
    });
    
    let response = server
        .post("/rpc")
        .json(&slow_request)
        .await;
    
    // Should complete successfully (2s delay < 5s timeout)
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_all_security_headers(&response);
}

#[tokio::test]
async fn test_security_with_suspicious_user_agent() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    let valid_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });
    
    let response = server
        .post("/rpc")
        .add_header(
            HeaderName::from_static("user-agent"),
            HeaderValue::from_static("malicious-bot/1.0")
        )
        .json(&valid_request)
        .await;
    
    // Request should still succeed (logging happens but doesn't block)
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_all_security_headers(&response);
    
    // In a real implementation, this would be logged as suspicious
}

#[tokio::test]
async fn test_health_check_in_secure_environment() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_all_security_headers(&response);
    
    let health_data: Value = response.json();
    assert_eq!(health_data["status"], "healthy");
    assert_eq!(health_data["service"], "torpc");
}

#[tokio::test]
async fn test_metrics_in_secure_environment() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/metrics").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_all_security_headers(&response);
    
    let metrics_data: Value = response.json();
    assert!(metrics_data["security_metrics"].is_object());
    assert!(metrics_data["timestamp"].is_string());
}

#[tokio::test]
async fn test_multiple_security_violations() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    // Test sequence: valid -> blocked method -> oversized -> valid
    let test_cases = vec![
        (
            json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}),
            StatusCode::OK
        ),
        (
            json!({"jsonrpc": "2.0", "method": "personal_unlockAccount", "id": 2}),
            StatusCode::METHOD_NOT_ALLOWED
        ),
        (
            json!({
                "jsonrpc": "2.0", 
                "method": "eth_call", 
                "params": [{"data": "0x".to_string() + &"ff".repeat(300_000)}],
                "id": 3
            }),
            StatusCode::PAYLOAD_TOO_LARGE
        ),
        (
            json!({"jsonrpc": "2.0", "method": "eth_getBalance", "id": 4}),
            StatusCode::OK
        ),
    ];
    
    for (request, expected_status) in test_cases {
        let response = server
            .post("/rpc")
            .json(&request)
            .await;
        
        assert_eq!(
            response.status_code(), 
            expected_status,
            "Failed for request: {}", 
            request
        );
        verify_all_security_headers(&response);
    }
}

#[tokio::test]
async fn test_json_rpc_validation_with_security() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    // Test invalid JSON-RPC requests
    let invalid_requests = vec![
        json!({"method": "eth_blockNumber"}), // Missing jsonrpc and id
        json!({"jsonrpc": "1.0", "method": "eth_blockNumber", "id": 1}), // Invalid version
        json!({"jsonrpc": "2.0", "id": 1}), // Missing method
    ];
    
    for invalid_request in invalid_requests {
        let response = server
            .post("/rpc")
            .json(&invalid_request)
            .await;
        
        // Should handle gracefully with security headers
        verify_all_security_headers(&response);
    }
}

#[tokio::test]
async fn test_security_across_different_endpoints() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    // Test all endpoints have consistent security
    let endpoints = vec![
        ("/health", "GET"),
        ("/metrics", "GET"),
    ];
    
    for (path, method) in endpoints {
        let response = match method {
            "GET" => server.get(path).await,
            _ => unreachable!(),
        };
        
        assert_eq!(response.status_code(), StatusCode::OK);
        verify_all_security_headers(&response);
    }
    
    // Test POST endpoint
    let rpc_response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "id": 1
        }))
        .await;
    
    assert_eq!(rpc_response.status_code(), StatusCode::OK);
    verify_all_security_headers(&rpc_response);
}

#[tokio::test]
async fn test_security_error_responses() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    // Test that error responses also have security measures
    let error_cases = vec![
        ("/nonexistent", StatusCode::NOT_FOUND),
    ];
    
    for (path, expected_status) in error_cases {
        let response = server.get(path).await;
        
        assert_eq!(response.status_code(), expected_status);
        verify_all_security_headers(&response);
    }
    
    // Test wrong HTTP method
    let response = server.post("/health").await;
    assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    verify_all_security_headers(&response);
}

#[tokio::test]
async fn test_security_with_empty_requests() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    // Test empty JSON
    let response = server
        .post("/rpc")
        .json(&json!({}))
        .await;
    
    verify_all_security_headers(&response);
    
    // Test minimal valid request
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "id": null
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_all_security_headers(&response);
}

#[tokio::test]
async fn test_security_performance_under_load() {
    let app = create_secure_test_app();
    let server = TestServer::new(app).unwrap();
    
    let valid_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "id": 1
    });
    
    // Make multiple requests to ensure security doesn't degrade performance significantly
    for i in 0..10 {
        let start = std::time::Instant::now();
        
        let response = server
            .post("/rpc")
            .json(&valid_request)
            .await;
        
        let duration = start.elapsed();
        
        assert_eq!(
            response.status_code(), 
            StatusCode::OK,
            "Request {} failed", 
            i
        );
        verify_all_security_headers(&response);
        
        // Security overhead should be minimal
        assert!(
            duration.as_millis() < 1000, 
            "Request {} took too long: {}ms", 
            i, 
            duration.as_millis()
        );
    }
}

#[tokio::test]
async fn test_comprehensive_security_configuration() {
    // Test that security configuration is properly applied
    let base_state = Arc::new(ProxyState::new(
        "http://localhost:8545".to_string(),
        "http://localhost:8545".to_string(),
    ));
    
    let mev_state = Arc::new(MevProxyState {
        base_state: base_state.clone(),
        mev_client: None,
    });
    
    // Custom security configuration
    let custom_config = SecurityConfig {
        max_body_size: 1024, // 1KB
        request_timeout: Duration::from_secs(1), // 1 second
        strict_headers: true,
    };
    
    let app = Router::new()
        .route("/rpc", post(mock_rpc_handler))
        .with_state(mev_state)
        .layer(build_security_layers(custom_config.clone()))
        .layer(DefaultBodyLimit::max(custom_config.max_body_size))
        .layer(middleware::from_fn(monitor_request_patterns))
        .layer(middleware::from_fn(security_headers_middleware));
    
    let server = TestServer::new(app).unwrap();
    
    // Test size limit with custom config
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"data": "x".repeat(2000)}], // 2KB > 1KB limit
            "id": 1
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
    verify_all_security_headers(&response);
}

// Helper function to verify all security headers are properly set
fn verify_all_security_headers(response: &axum_test::TestResponse) {
    // Content security headers
    assert_eq!(response.header("X-Content-Type-Options"), "nosniff");
    assert_eq!(response.header("X-Frame-Options"), "DENY");
    assert_eq!(response.header("X-XSS-Protection"), "0");
    assert_eq!(response.header("Referrer-Policy"), "no-referrer");
    
    // Content Security Policy
    let csp = response.header("Content-Security-Policy");
    let csp_str = csp.to_str().unwrap();
    assert!(csp_str.contains("default-src 'none'"));
    assert!(csp_str.contains("frame-ancestors 'none'"));
    
    // Cache control headers
    let cache_control = response.header("Cache-Control");
    let cache_str = cache_control.to_str().unwrap();
    assert!(cache_str.contains("no-store"));
    assert!(cache_str.contains("no-cache"));
    assert!(cache_str.contains("must-revalidate"));
    
    assert_eq!(response.header("Pragma"), "no-cache");
    assert_eq!(response.header("Expires"), "0");
    
    // Service identification
    assert_eq!(response.header("X-Service"), "TorPC");
    
    // Server header should be removed
    let server_header = response.headers().get("Server");
    assert!(server_header.is_none(), "Server header should be removed");
}