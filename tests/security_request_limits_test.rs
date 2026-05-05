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
use tokio::time::sleep;
use torpc::security::{
    json_rpc_timeout_middleware, security_headers_middleware, SecurityConfig,
};

// Test handler that echoes back the request
async fn echo_handler(Json(payload): Json<Value>) -> Json<Value> {
    Json(payload)
}

// Test handler that simulates slow processing
async fn slow_handler(Json(payload): Json<Value>) -> Json<Value> {
    sleep(Duration::from_secs(35)).await; // Longer than default 30s timeout
    Json(payload)
}

// Test handler that processes request and sleeps for specified duration
async fn configurable_delay_handler(Json(payload): Json<Value>) -> Json<Value> {
    if let Some(delay_ms) = payload.get("delay_ms").and_then(|v| v.as_u64()) {
        sleep(Duration::from_millis(delay_ms)).await;
    }
    Json(payload)
}

fn create_test_router_with_limits(max_body_size: usize, timeout_secs: u64) -> Router {
    let security_config = SecurityConfig {
        max_body_size,
        request_timeout: Duration::from_secs(timeout_secs),
        strict_headers: true,
    };

    // Phase-Option-C swap: the deprecated `build_security_layers` returned a
    // bare `tower-http::TimeoutLayer` (empty 408). The new
    // `json_rpc_timeout_middleware` produces a JSON-RPC `-32001` body on
    // timeout; same timeout knob, structured error.
    Router::new()
        .route("/echo", post(echo_handler))
        .route("/slow", post(slow_handler))
        .route("/delay", post(configurable_delay_handler))
        .layer(middleware::from_fn_with_state(
            security_config.request_timeout,
            json_rpc_timeout_middleware,
        ))
        .layer(DefaultBodyLimit::max(security_config.max_body_size))
        .layer(middleware::from_fn(security_headers_middleware))
}

fn create_test_router_with_default_limits() -> Router {
    create_test_router_with_limits(1024 * 1024, 30) // 1MB, 30 seconds
}

#[tokio::test]
async fn test_request_under_size_limit() {
    let app = create_test_router_with_default_limits();
    let server = TestServer::new(app).unwrap();
    
    let small_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "id": 1,
        "data": "x".repeat(100) // 100 bytes
    });
    
    let response = server
        .post("/echo")
        .json(&small_payload)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    let response_json: Value = response.json();
    assert_eq!(response_json["data"], "x".repeat(100));
}

#[tokio::test]
async fn test_request_at_size_limit() {
    // Create router with 1KB limit for precise testing
    let app = create_test_router_with_limits(1024, 30);
    let server = TestServer::new(app).unwrap();
    
    // Create payload that's exactly at the limit (accounting for JSON overhead)
    let data_size = 900; // Leave room for JSON structure
    let payload = json!({
        "data": "x".repeat(data_size)
    });
    
    let response = server
        .post("/echo")
        .json(&payload)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
}

#[tokio::test]
async fn test_request_over_size_limit() {
    // Create router with 1KB limit
    let app = create_test_router_with_limits(1024, 30);
    let server = TestServer::new(app).unwrap();
    
    // Create payload larger than 1KB
    let large_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_call", 
        "params": [{
            "data": "x".repeat(2048) // 2KB of data
        }],
        "id": 1
    });
    
    let response = server
        .post("/echo")
        .json(&large_payload)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_very_large_request() {
    let app = create_test_router_with_default_limits();
    let server = TestServer::new(app).unwrap();
    
    // Create a 2MB payload (larger than default 1MB limit)
    let very_large_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "data": "x".repeat(2 * 1024 * 1024) // 2MB
        }],
        "id": 1
    });
    
    let response = server
        .post("/echo")
        .json(&very_large_payload)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_request_timeout_under_limit() {
    // Create router with 5 second timeout
    let app = create_test_router_with_limits(1024 * 1024, 5);
    let server = TestServer::new(app).unwrap();
    
    let payload = json!({
        "delay_ms": 2000, // 2 seconds - under limit
        "message": "test"
    });
    
    let response = server
        .post("/delay")
        .json(&payload)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    let response_json: Value = response.json();
    assert_eq!(response_json["message"], "test");
}

#[tokio::test]
async fn test_request_timeout_over_limit() {
    // Create router with 2 second timeout
    let app = create_test_router_with_limits(1024 * 1024, 2);
    let server = TestServer::new(app).unwrap();
    
    let payload = json!({
        "delay_ms": 5000, // 5 seconds - over 2 second limit
        "message": "should timeout"
    });
    
    let response = server
        .post("/delay")
        .json(&payload)
        .await;

    // The new JSON-RPC timeout middleware returns 504 (gateway timeout)
    // with a `-32001` body — the prior bare `tower-http::TimeoutLayer`
    // returned an empty 408. Wallets parse the body now; check both.
    assert_eq!(response.status_code(), StatusCode::GATEWAY_TIMEOUT);
    let body: Value = response.json();
    assert_eq!(body["error"]["code"], -32001);
}

#[tokio::test]
async fn test_multiple_size_limits() {
    // Test different size limits
    let test_cases = vec![
        (512, "x".repeat(256), StatusCode::OK),           // Under limit
        (512, "x".repeat(1024), StatusCode::PAYLOAD_TOO_LARGE), // Over limit
        (2048, "x".repeat(1024), StatusCode::OK),         // Under larger limit
        (2048, "x".repeat(4096), StatusCode::PAYLOAD_TOO_LARGE), // Over larger limit
    ];
    
    for (limit, data, expected_status) in test_cases {
        let app = create_test_router_with_limits(limit, 30);
        let server = TestServer::new(app).unwrap();
        
        let payload = json!({
            "data": data
        });
        
        let response = server
            .post("/echo")
            .json(&payload)
            .await;
        
        assert_eq!(
            response.status_code(), 
            expected_status,
            "Failed for limit {} with data size {}", 
            limit, 
            data.len()
        );
    }
}

#[tokio::test]
async fn test_multiple_timeout_limits() {
    // Test different timeout limits. New middleware returns 504, not 408.
    let test_cases = vec![
        (5, 2000, StatusCode::OK),                    // 2s delay with 5s limit
        (5, 8000, StatusCode::GATEWAY_TIMEOUT),       // 8s delay with 5s limit
        (10, 5000, StatusCode::OK),                   // 5s delay with 10s limit
        (1, 2000, StatusCode::GATEWAY_TIMEOUT),       // 2s delay with 1s limit
    ];
    
    for (timeout_secs, delay_ms, expected_status) in test_cases {
        let app = create_test_router_with_limits(1024 * 1024, timeout_secs);
        let server = TestServer::new(app).unwrap();
        
        let payload = json!({
            "delay_ms": delay_ms,
            "test": "timeout"
        });
        
        let response = server
            .post("/delay")
            .json(&payload)
            .await;
        
        assert_eq!(
            response.status_code(), 
            expected_status,
            "Failed for timeout {}s with delay {}ms", 
            timeout_secs, 
            delay_ms
        );
    }
}

#[tokio::test]
async fn test_security_headers_on_size_limit_error() {
    let app = create_test_router_with_limits(512, 30);
    let server = TestServer::new(app).unwrap();
    
    let large_payload = json!({
        "data": "x".repeat(1024) // Over 512 byte limit
    });
    
    let response = server
        .post("/echo")
        .json(&large_payload)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
    
    // Verify security headers are present even on error
    assert_eq!(response.header("X-Content-Type-Options"), "nosniff");
    assert_eq!(response.header("X-Frame-Options"), "DENY");
    assert_eq!(response.header("X-XSS-Protection"), "0");
}

#[tokio::test]
async fn test_security_headers_on_timeout_error() {
    let app = create_test_router_with_limits(1024 * 1024, 1);
    let server = TestServer::new(app).unwrap();
    
    let payload = json!({
        "delay_ms": 3000, // 3 seconds with 1 second timeout
        "data": "timeout test"
    });
    
    let response = server
        .post("/delay")
        .json(&payload)
        .await;

    assert_eq!(response.status_code(), StatusCode::GATEWAY_TIMEOUT);

    // Verify security headers are present even on timeout
    assert_eq!(response.header("X-Content-Type-Options"), "nosniff");
    assert_eq!(response.header("X-Frame-Options"), "DENY");
    assert_eq!(response.header("X-XSS-Protection"), "0");
}

#[tokio::test]
async fn test_json_rpc_structure_size_limits() {
    let app = create_test_router_with_limits(1024, 30);
    let server = TestServer::new(app).unwrap();
    
    // Test JSON-RPC request that exceeds size limit
    let large_rpc_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [
            {
                "to": "0x1234567890123456789012345678901234567890",
                "data": format!("0x{}", "ff".repeat(800)) // Large hex data
            },
            "latest"
        ],
        "id": 1
    });
    
    let response = server
        .post("/echo")
        .json(&large_rpc_request)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_empty_request_handling() {
    let app = create_test_router_with_default_limits();
    let server = TestServer::new(app).unwrap();
    
    let empty_payload = json!({});
    
    let response = server
        .post("/echo")
        .json(&empty_payload)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    let response_json: Value = response.json();
    assert_eq!(response_json, json!({}));
}

#[tokio::test]
async fn test_security_config_from_environment() {
    // Test that SecurityConfig respects environment variables
    std::env::set_var("MAX_BODY_SIZE", "2048");
    std::env::set_var("REQUEST_TIMEOUT", "5");
    
    let config = SecurityConfig::from_env();
    assert_eq!(config.max_body_size, 2048);
    assert_eq!(config.request_timeout.as_secs(), 5);
    
    // Test router with environment config
    let app = create_test_router_with_limits(config.max_body_size, config.request_timeout.as_secs());
    let server = TestServer::new(app).unwrap();
    
    let payload = json!({
        "data": "x".repeat(1500) // Between 1024 (default) and 2048 (env)
    });
    
    let response = server
        .post("/echo")
        .json(&payload)
        .await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    
    // Clean up environment
    std::env::remove_var("MAX_BODY_SIZE");
    std::env::remove_var("REQUEST_TIMEOUT");
}

#[tokio::test]
async fn test_sequential_requests_size_limits() {
    let app = create_test_router_with_limits(1024, 30);
    let server = TestServer::new(app).unwrap();
    
    // Test multiple requests sequentially - some valid, some oversized
    let test_cases = vec![
        (json!({"data": "x".repeat(500)}), StatusCode::OK),
        (json!({"data": "x".repeat(2000)}), StatusCode::PAYLOAD_TOO_LARGE),
        (json!({"data": "x".repeat(800)}), StatusCode::OK),
        (json!({"data": "x".repeat(1500)}), StatusCode::PAYLOAD_TOO_LARGE),
    ];
    
    for (payload, expected_status) in test_cases {
        let response = server
            .post("/echo")
            .json(&payload)
            .await;
        
        assert_eq!(
            response.status_code(), 
            expected_status,
            "Failed for payload size {}", 
            payload.to_string().len()
        );
    }
}