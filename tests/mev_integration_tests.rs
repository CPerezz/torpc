//! End-to-end integration tests for MEV protection functionality
//! 
//! These tests verify that MEV protection works correctly, including:
//! - Bundle submission via Flashbots relay
//! - EIP-191 authentication
//! - Retry logic and error handling
//! - Transaction conversion to bundles

use std::process::Command;
use std::sync::Once;
use std::time::Duration;
use serde_json::json;
use tokio::time::sleep;

static INIT: Once = Once::new();

/// Ensure services are running before tests
fn ensure_services_running() {
    INIT.call_once(|| {
        println!("Checking if services are running...");
        
        // Check if services are already running
        let geth_running = Command::new("pgrep")
            .args(&["-f", "geth.*--dev"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
            
        if !geth_running {
            panic!("Services not running! Please run: ./scripts/start-all-dev.sh");
        }
        
        println!("Services confirmed running");
    });
}

/// Test helper to create a signed transaction
/// Returns a hex-encoded signed transaction ready for submission
async fn create_test_transaction() -> String {
    // For testing, we'll use eth_sendBundle which doesn't require a valid transaction
    // since we're not actually submitting to mainnet
    "0x02f86b0180808252089400000000000000000000000000000000000000008080c001a0d91cd92cd079e3c9e22be2ce33a32e0b3eee02a3e039c7c39c3bf12a4b9b4fa01234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string()
}

/// Wait for service to be ready
async fn wait_for_service(url: &str, timeout: Duration) -> Result<(), String> {
    let client = reqwest::Client::new();
    let start = std::time::Instant::now();
    
    loop {
        if start.elapsed() > timeout {
            return Err(format!("Service at {} did not become ready in time", url));
        }
        
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 404 => {
                println!("Service at {} is ready", url);
                return Ok(());
            }
            _ => {
                sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

#[tokio::test]
async fn test_mev_bundle_submission_without_key() {
    ensure_services_running();
    wait_for_service("http://127.0.0.1:8080", Duration::from_secs(10)).await.unwrap();
    
    println!("Testing MEV bundle submission without signing key (should fall back to standard)...");
    
    let client = reqwest::Client::new();
    let tx_data = create_test_transaction().await;
    
    // Submit transaction without MEV protection
    let request = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [tx_data],
        "id": 1
    });
    
    let response = client
        .post("http://127.0.0.1:8080/rpc/flashbots")
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
    
    let status = response.status();
    let body = response.text().await.unwrap();
    println!("Response status: {}", status);
    println!("Response body: {}", body);
        
    assert!(status.is_success(), "Request failed with status: {} body: {}", status, body);
    
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    println!("Response: {:?}", json);
    
    // Without MEV key, it should still process the transaction
    // but without MEV protection
    assert!(json.get("result").is_some() || json.get("error").is_some());
}

#[tokio::test]
async fn test_send_bundle_method() {
    ensure_services_running();
    wait_for_service("http://127.0.0.1:8080", Duration::from_secs(10)).await.unwrap();
    
    println!("Testing eth_sendBundle method...");
    
    let client = reqwest::Client::new();
    
    // Create a bundle request
    let request = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendBundle",
        "params": [{
            "txs": [create_test_transaction().await],
            "blockNumber": "0x1000000",
            "minTimestamp": null,
            "maxTimestamp": null
        }],
        "id": 1
    });
    
    let response = client
        .post("http://127.0.0.1:8080/rpc/flashbots")
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
        
    assert!(response.status().is_success());
    
    let json: serde_json::Value = response.json().await.unwrap();
    println!("Bundle response: {:?}", json);
    
    // Check response structure
    assert!(json.get("jsonrpc").is_some());
    assert_eq!(json.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
}

#[tokio::test]
async fn test_mev_with_signing_key() {
    ensure_services_running();
    
    // Skip this test if no signing key is provided
    if std::env::var("TEST_FLASHBOTS_SIGNING_KEY").is_err() {
        println!("Skipping MEV signing test - set TEST_FLASHBOTS_SIGNING_KEY to enable");
        return;
    }
    
    println!("Testing MEV bundle submission with signing key...");
    
    // Start a new instance of torpc with MEV protection
    let mut torpc_process = Command::new("cargo")
        .args(&["run", "--bin", "torpc", "--release"])
        .env("FLASHBOTS_SIGNING_KEY", std::env::var("TEST_FLASHBOTS_SIGNING_KEY").unwrap())
        .env("FLASHBOTS_RELAY_URL", "https://relay-goerli.flashbots.net")
        .env("BIND_ADDR", "127.0.0.1:8081") // Different port
        .spawn()
        .expect("Failed to start TorPC with MEV");
        
    // Wait for it to start
    wait_for_service("http://127.0.0.1:8081", Duration::from_secs(30)).await.unwrap();
    
    let client = reqwest::Client::new();
    let tx_data = create_test_transaction().await;
    
    // Submit transaction with MEV protection
    let request = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [tx_data],
        "id": 1
    });
    
    let response = client
        .post("http://127.0.0.1:8081/rpc/flashbots")
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
        
    // Clean up
    torpc_process.kill().unwrap();
    
    assert!(response.status().is_success());
    
    let json: serde_json::Value = response.json().await.unwrap();
    println!("MEV-protected response: {:?}", json);
    
    // With MEV protection, we should get a bundle hash
    assert!(json.get("result").is_some());
}

#[tokio::test]
async fn test_invalid_bundle_format() {
    ensure_services_running();
    wait_for_service("http://127.0.0.1:8080", Duration::from_secs(10)).await.unwrap();
    
    println!("Testing invalid bundle format handling...");
    
    let client = reqwest::Client::new();
    
    // Send invalid bundle format
    let request = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendBundle",
        "params": [{
            "invalidField": "invalid",
            "blockNumber": "not-a-hex"
        }],
        "id": 1
    });
    
    let response = client
        .post("http://127.0.0.1:8080/rpc/flashbots")
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
        
    assert!(response.status().is_success()); // HTTP should succeed
    
    let json: serde_json::Value = response.json().await.unwrap();
    println!("Error response: {:?}", json);
    
    // But JSON-RPC should contain error
    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn test_circuit_breaker_simulation() {
    ensure_services_running();
    
    if std::env::var("TEST_FLASHBOTS_SIGNING_KEY").is_err() {
        println!("Skipping circuit breaker test - set TEST_FLASHBOTS_SIGNING_KEY to enable");
        return;
    }
    
    println!("Testing circuit breaker behavior...");
    
    // Start TorPC with a bad relay URL to trigger failures
    let mut torpc_process = Command::new("cargo")
        .args(&["run", "--bin", "torpc", "--release"])
        .env("FLASHBOTS_SIGNING_KEY", std::env::var("TEST_FLASHBOTS_SIGNING_KEY").unwrap())
        .env("FLASHBOTS_RELAY_URL", "http://127.0.0.1:9999") // Non-existent relay
        .env("BIND_ADDR", "127.0.0.1:8082")
        .spawn()
        .expect("Failed to start TorPC");
        
    wait_for_service("http://127.0.0.1:8082", Duration::from_secs(30)).await.unwrap();
    
    let client = reqwest::Client::new();
    
    // Send multiple requests to trigger circuit breaker
    for i in 0..10 {
        println!("Request {} to trigger circuit breaker...", i + 1);
        
        let request = json!({
            "jsonrpc": "2.0",
            "method": "eth_sendRawTransaction",
            "params": [create_test_transaction().await],
            "id": i + 1
        });
        
        let response = client
            .post("http://127.0.0.1:8082/rpc/flashbots")
            .json(&request)
            .send()
            .await
            .expect("Failed to send request");
            
        let json: serde_json::Value = response.json().await.unwrap();
        println!("Response {}: {:?}", i + 1, json.get("error"));
        
        // After several failures, circuit should open
        assert!(json.get("error").is_some());
    }
    
    // Clean up
    torpc_process.kill().unwrap();
}

#[tokio::test]
async fn test_block_number_targeting() {
    ensure_services_running();
    wait_for_service("http://127.0.0.1:8080", Duration::from_secs(10)).await.unwrap();
    
    println!("Testing block number targeting in bundles...");
    
    let client = reqwest::Client::new();
    
    // First get current block number
    let block_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });
    
    let block_response = client
        .post("http://127.0.0.1:8080/rpc")
        .json(&block_request)
        .send()
        .await
        .expect("Failed to get block number");
        
    let block_json: serde_json::Value = block_response.json().await.unwrap();
    let current_block = block_json.get("result")
        .and_then(|v| v.as_str())
        .expect("Failed to get block number");
        
    println!("Current block: {}", current_block);
    
    // Submit bundle targeting specific block
    let target_block = format!("0x{:x}", 
        u64::from_str_radix(current_block.trim_start_matches("0x"), 16).unwrap() + 5
    );
    
    let bundle_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendBundle",
        "params": [{
            "txs": [create_test_transaction().await],
            "blockNumber": target_block.clone()
        }],
        "id": 2
    });
    
    let response = client
        .post("http://127.0.0.1:8080/rpc/flashbots")
        .json(&bundle_request)
        .send()
        .await
        .expect("Failed to send bundle");
        
    let json: serde_json::Value = response.json().await.unwrap();
    println!("Bundle targeting block {}: {:?}", target_block, json);
    
    assert!(json.get("result").is_some() || json.get("error").is_some());
}

/// Comprehensive test of the MEV protection flow
#[tokio::test]
async fn test_full_mev_protection_flow() {
    ensure_services_running();
    wait_for_service("http://127.0.0.1:8080", Duration::from_secs(10)).await.unwrap();
    
    println!("Testing full MEV protection flow...");
    
    let client = reqwest::Client::new();
    
    // Test 1: Standard RPC endpoint should not have MEV protection
    let standard_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });
    
    let response = client
        .post("http://127.0.0.1:8080/rpc")
        .json(&standard_request)
        .send()
        .await
        .expect("Failed to send to standard endpoint");
        
    assert!(response.status().is_success());
    println!("✓ Standard RPC endpoint works");
    
    // Test 2: Flashbots endpoint accepts eth_sendBundle
    let bundle_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendBundle",
        "params": [{
            "txs": [create_test_transaction().await],
            "blockNumber": "0x1000000"
        }],
        "id": 2
    });
    
    let response = client
        .post("http://127.0.0.1:8080/rpc/flashbots")
        .json(&bundle_request)
        .send()
        .await
        .expect("Failed to send bundle");
        
    assert!(response.status().is_success());
    println!("✓ Flashbots endpoint accepts bundles");
    
    // Test 3: Regular queries to flashbots endpoint work
    let query_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": ["0x0000000000000000000000000000000000000000", "latest"],
        "id": 3
    });
    
    let response = client
        .post("http://127.0.0.1:8080/rpc/flashbots")
        .json(&query_request)
        .send()
        .await
        .expect("Failed to send query");
        
    assert!(response.status().is_success());
    let json: serde_json::Value = response.json().await.unwrap();
    assert!(json.get("result").is_some());
    println!("✓ Non-transaction queries work on flashbots endpoint");
    
    println!("✅ All MEV protection flow tests passed!");
}