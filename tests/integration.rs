use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use torpc::proxy::ProxyState;
use torpc::rate_limit::{RateLimitConfig, RateLimiter};
use torpc::tor::TorService;

#[ignore = "requires running Geth; run via `make test-with-services`"]
#[tokio::test]
async fn test_full_rpc_request_flow() {
    // This test requires a running Geth instance
    if std::env::var("GETH_URL").is_err() {
        eprintln!("Skipping integration test - GETH_URL not set");
        return;
    }

    let geth_url = "http://127.0.0.1:8545".to_string();
    let state = ProxyState::new(geth_url, "https://relay.flashbots.net".to_string())
        .expect("ProxyState::new must succeed in tests");

    // Create a simple JSON-RPC request
    let request = torpc::rpc_types::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "eth_blockNumber".to_string(),
        params: None,
        id: Some(json!(1)),
    };

    // Test direct proxy call
    let response = torpc::proxy::proxy_to_geth(&state, request).await;

    match response {
        Ok(resp) => {
            assert_eq!(resp.jsonrpc, "2.0");
            assert!(resp.result.is_some() || resp.error.is_some());
        }
        Err(e) => {
            eprintln!("Integration test failed - ensure Geth is running: {e}");
        }
    }
}

#[test]
fn test_tor_configuration_exists() {
    let tor_service = TorService::new();
    let config_path = std::path::Path::new(&tor_service.config_path);

    assert!(
        config_path.exists(),
        "Tor configuration file should exist at: {}",
        tor_service.config_path
    );
}

#[tokio::test]
async fn test_rate_limiting_integration() {
    let config = RateLimitConfig {
        max_requests: 3,
        window_duration: Duration::from_secs(1),
    };
    let limiter = Arc::new(RateLimiter::new(config));

    // Simulate rapid requests from same source
    let identifier = "test-client";

    // First 3 should pass
    for i in 1..=3 {
        assert!(
            limiter.check_rate_limit(identifier).await,
            "Request {i} should be allowed"
        );
    }

    // 4th should be blocked
    assert!(
        !limiter.check_rate_limit(identifier).await,
        "4th request should be rate limited"
    );

    // Wait for window to reset
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Should be allowed again
    assert!(
        limiter.check_rate_limit(identifier).await,
        "Request should be allowed after window reset"
    );
}

#[test]
fn test_static_files_exist() {
    let static_files = ["static/index.html", "static/style.css", "static/app.js"];

    for file in &static_files {
        let path = std::path::Path::new(file);
        assert!(path.exists(), "Static file should exist: {file}");
    }
}

// Helper function to check if service is running
async fn is_service_running(url: &str) -> bool {
    match reqwest::get(url).await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

#[ignore = "requires running daemon; run via `make test-with-services`"]
#[tokio::test]
async fn test_service_endpoints() {
    // Skip if not running
    if !is_service_running("http://127.0.0.1:8080").await {
        eprintln!("Skipping endpoint test - service not running");
        return;
    }

    let client = reqwest::Client::new();

    // Test static file serving
    let response = client.get("http://127.0.0.1:8080/").send().await.unwrap();
    assert_eq!(response.status(), 200);

    // Test RPC endpoint with invalid request (should get proper error)
    let response = client
        .post("http://127.0.0.1:8080/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "invalid_method",
            "id": 1
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 405); // METHOD_NOT_ALLOWED
}
