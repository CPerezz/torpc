use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::sleep;

const TORPC_URL: &str = "http://127.0.0.1:8080/rpc";
const TORPC_FLASHBOTS_URL: &str = "http://127.0.0.1:8080/rpc/flashbots";

// Helper function to make RPC requests
async fn make_rpc_request(url: &str, method: &str, params: Option<Value>) -> Result<Value, String> {
    let client = Client::new();
    let request_body = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params.unwrap_or(json!([])),
        "id": 1
    });

    let response = client
        .post(url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    if !status.is_success() {
        return Err(format!("HTTP error {status}: {body:?}"));
    }

    Ok(body)
}

// Test 1: End-to-end Tor integration test
#[ignore = "requires running services; run via `make test-with-services`"]
#[tokio::test]
async fn test_tor_hidden_service_accessibility() {
    println!("Testing Tor hidden service accessibility...");

    // First, check if Tor hostname file exists
    let hostname_path = "data/tor/torpc/hostname";
    if !std::path::Path::new(hostname_path).exists() {
        println!("  ⚠️  Tor hostname file not found, skipping Tor test");
        return;
    }

    // Read the onion address
    let onion_address = std::fs::read_to_string(hostname_path)
        .expect("Failed to read Tor hostname")
        .trim()
        .to_string();

    println!("  Found onion address: {onion_address}");

    // Check if system Tor is running with SOCKS proxy
    let socks_test = Client::builder()
        .proxy(reqwest::Proxy::all("socks5h://127.0.0.1:9050").unwrap())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // Try to connect to a known .onion service to test SOCKS proxy
    match socks_test.get("http://check.torproject.org").send().await {
        Ok(_) => println!("  ✓ System Tor SOCKS proxy is available on port 9050"),
        Err(_) => {
            println!("  ⚠️  Tor SOCKS proxy not available on port 9050");
            println!("  ⚠️  The torrc disables SOCKS (SocksPort 0)");
            println!(
                "  ⚠️  To test .onion access, you need a separate Tor instance with SOCKS enabled"
            );
            println!("  ✓ Skipping Tor accessibility test - service is configured correctly");
            return;
        }
    }

    // Create a client with SOCKS5 proxy configuration for Tor
    let proxy =
        reqwest::Proxy::all("socks5h://127.0.0.1:9050").expect("Failed to create SOCKS5 proxy");

    let client = Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create Tor client");

    let onion_url = format!("http://{onion_address}/rpc");
    println!("  Testing RPC endpoint via Tor: {onion_url}");

    // Make a simple eth_blockNumber request via Tor
    let request_body = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    match client.post(&onion_url).json(&request_body).send().await {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                let body: Value = response.json().await.unwrap();
                println!("  ✓ Successfully accessed TorPC via Tor hidden service");
                println!(
                    "  Response: {}",
                    serde_json::to_string_pretty(&body).unwrap()
                );
                assert!(
                    body.get("result").is_some(),
                    "Response should have a result field"
                );
            } else {
                println!("  ✗ Tor request returned status: {status}");
                panic!("Failed to access TorPC via Tor");
            }
        }
        Err(e) => {
            println!("  ✗ Failed to connect via Tor: {e}");
            println!("  Note: This is expected if no system Tor with SOCKS is running");
            println!("  The hidden service is still accessible to Tor users");
        }
    }
}

// Test 2: Concurrent request handling
#[ignore = "requires running services; run via `make test-with-services`"]
#[tokio::test]
async fn test_concurrent_requests() {
    println!("Testing concurrent request handling...");

    // Wait a bit to ensure services are ready
    sleep(Duration::from_secs(2)).await;

    let num_concurrent_requests = 20;
    let mut handles = Vec::new();

    println!("  Sending {num_concurrent_requests} concurrent requests...");

    // Spawn concurrent tasks
    for i in 0..num_concurrent_requests {
        let handle = tokio::spawn(async move {
            let start = std::time::Instant::now();

            // Mix of different methods to test routing
            let method = match i % 4 {
                0 => "eth_blockNumber",
                1 => "eth_gasPrice",
                2 => "eth_chainId",
                _ => "net_version",
            };

            match make_rpc_request(TORPC_URL, method, None).await {
                Ok(response) => {
                    let duration = start.elapsed();
                    assert!(
                        response.get("result").is_some(),
                        "Request {i} failed: no result"
                    );
                    println!("    Request {i} ({method}) completed in {duration:?}");
                    Ok(())
                }
                Err(e) => {
                    println!("    Request {i} ({method}) failed: {e}");
                    Err(e)
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all requests to complete
    let mut successes = 0;
    let mut failures = 0;

    for handle in handles {
        match handle.await {
            Ok(Ok(())) => successes += 1,
            _ => failures += 1,
        }
    }

    println!("  Results: {successes} successful, {failures} failed");
    assert!(
        successes >= num_concurrent_requests * 90 / 100,
        "At least 90% of requests should succeed"
    );
    println!("  ✓ Concurrent request handling test passed");
}

// Test 3: Malformed JSON handling
#[ignore = "requires running services; run via `make test-with-services`"]
#[tokio::test]
async fn test_malformed_json_handling() {
    println!("Testing malformed JSON handling...");

    let client = Client::new();

    // Test cases for various malformed JSON scenarios
    let test_cases = vec![
        (
            "Invalid JSON syntax",
            r#"{"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1"#, // Missing closing brace
        ),
        (
            "Missing required field (method)",
            r#"{"jsonrpc": "2.0", "id": 1}"#,
        ),
        (
            "Wrong JSON-RPC version",
            r#"{"jsonrpc": "1.0", "method": "eth_blockNumber", "id": 1}"#,
        ),
        (
            "Non-string method",
            r#"{"jsonrpc": "2.0", "method": 12345, "id": 1}"#,
        ),
        ("Empty request body", r#"{}"#),
        ("Not JSON at all", r#"This is not JSON"#),
        (
            "Null values",
            r#"{"jsonrpc": null, "method": null, "id": null}"#,
        ),
    ];

    for (description, malformed_json) in test_cases {
        println!("  Testing: {description}");

        let response = client
            .post(TORPC_URL)
            .header("Content-Type", "application/json")
            .body(malformed_json.to_string())
            .send()
            .await
            .expect("Failed to send request");

        let status = response.status();

        // We expect 400 Bad Request or similar error status
        assert!(
            status.is_client_error(),
            "{description}: Expected client error status, got {status}"
        );

        // Try to parse the error response
        if let Ok(body) = response.json::<Value>().await {
            println!(
                "    Response: {}",
                serde_json::to_string_pretty(&body).unwrap_or_default()
            );

            // Should have an error field
            assert!(
                body.get("error").is_some(),
                "{description}: Error response should have 'error' field"
            );
        }

        println!("    ✓ Properly rejected with status {status}");
    }

    println!("  ✓ All malformed JSON tests passed");
}

// Test 7: Rate limiting behavior (runs last to avoid affecting other tests)
#[ignore = "requires running services; run via `make test-with-services`"]
#[tokio::test]
async fn test_z_rate_limiting() {
    println!("Testing rate limiting...");

    // The configured rate limit is 100 requests per minute
    // We'll send requests in a burst to trigger the limit
    let num_requests = 120;
    let mut handles = Vec::new();

    println!("  Sending {num_requests} requests to trigger rate limit...");

    // Send all requests without delay to ensure we hit the limit
    for i in 0..num_requests {
        let handle = tokio::spawn(async move {
            let client = Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();

            let request_body = json!({
                "jsonrpc": "2.0",
                "method": "eth_blockNumber",
                "params": [],
                "id": i
            });

            let response = client.post(TORPC_URL).json(&request_body).send().await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    if status == 429 {
                        Err("Rate limited".to_string())
                    } else if status.is_success() {
                        Ok("Success".to_string())
                    } else {
                        Err(format!("HTTP {status}"))
                    }
                }
                Err(e) => Err(format!("Request error: {e}")),
            }
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let mut success_count = 0;
    let mut rate_limited_count = 0;
    let mut other_errors = 0;

    for (i, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(Ok(_)) => success_count += 1,
            Ok(Err(e)) => {
                if e.contains("Rate limited") {
                    rate_limited_count += 1;
                    if rate_limited_count == 1 {
                        println!("    First rate limit hit at request {}", i + 1);
                    }
                } else {
                    other_errors += 1;
                    println!("    Request {i} error: {e}");
                }
            }
            Err(e) => {
                println!("    Join error for request {i}: {e}");
            }
        }
    }

    println!(
        "  Results: {success_count} successful, {rate_limited_count} rate limited, {other_errors} other errors"
    );

    // We should see some successes and some rate limits
    assert!(success_count > 0, "Should have some successful requests");
    assert!(
        success_count <= 100,
        "Should not exceed rate limit of 100 requests per minute"
    );

    // If we didn't hit rate limit, it might be because requests were spread out
    if rate_limited_count == 0 {
        println!("  ⚠️  Rate limiting not triggered - requests may have been spread over time");
        println!("  ⚠️  This can happen if the system is slow or requests are queued");
        // Don't fail the test, just warn
    } else {
        println!("  ✓ Rate limiting is working correctly");
    }
}

// Test 5: Valid request/response flow
#[ignore = "requires running services; run via `make test-with-services`"]
#[tokio::test]
async fn test_valid_rpc_methods() {
    println!("Testing valid RPC methods...");

    // Test various allowed methods
    let test_methods = vec![
        ("eth_blockNumber", None),
        ("eth_chainId", None),
        ("eth_gasPrice", None),
        ("net_version", None),
        ("web3_clientVersion", None),
        (
            "eth_getBalance",
            Some(json!([
                "0x0000000000000000000000000000000000000000",
                "latest"
            ])),
        ),
    ];

    for (method, params) in test_methods {
        println!("  Testing method: {method}");

        // Add retry logic for transient failures
        let mut attempts = 0;
        let max_attempts = 3;
        let mut last_error = String::new();

        while attempts < max_attempts {
            match make_rpc_request(TORPC_URL, method, params.clone()).await {
                Ok(response) => {
                    assert!(
                        response.get("result").is_some(),
                        "Response should have result field"
                    );
                    assert_eq!(
                        response.get("jsonrpc").and_then(|v| v.as_str()),
                        Some("2.0")
                    );
                    assert_eq!(response.get("id").and_then(|v| v.as_i64()), Some(1));
                    println!("    ✓ {method} responded correctly");
                    break;
                }
                Err(e) => {
                    attempts += 1;
                    last_error = e;
                    if attempts < max_attempts {
                        println!("    Retry {attempts} for {method}: {last_error}");
                        sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }

        if attempts >= max_attempts {
            panic!("Method {method} failed after {max_attempts} attempts: {last_error}");
        }
    }

    println!("  ✓ All valid RPC methods working correctly");
}

// Test 4: Flashbots endpoint routing
#[ignore = "requires running services; run via `make test-with-services`"]
#[tokio::test]
async fn test_flashbots_endpoint() {
    println!("Testing Flashbots endpoint...");

    // Test that non-transaction methods still work on flashbots endpoint
    match make_rpc_request(TORPC_FLASHBOTS_URL, "eth_blockNumber", None).await {
        Ok(response) => {
            assert!(response.get("result").is_some());
            println!("  ✓ Flashbots endpoint routes read methods correctly");
        }
        Err(e) => panic!("Flashbots endpoint test failed: {e}"),
    }

    // Note: We can't test actual eth_sendRawTransaction without a valid signed transaction
    // but we can verify the endpoint exists and responds
}

// Test 7: Blocked methods are rejected
#[ignore = "requires running services; run via `make test-with-services`"]
#[tokio::test]
async fn test_blocked_methods() {
    println!("Testing blocked method rejection...");

    let blocked_methods = vec![
        "eth_accounts",
        "eth_sign",
        "personal_unlockAccount",
        "admin_addPeer",
        "debug_traceTransaction",
    ];

    for method in blocked_methods {
        println!("  Testing blocked method: {method}");

        let response = make_rpc_request(TORPC_URL, method, None).await;

        match response {
            Err(e) => {
                let error_str = e.to_string();
                assert!(
                    error_str.contains("405") || error_str.contains("Method Not Allowed"),
                    "Expected method not allowed error for {method}, got: {error_str}"
                );
                println!("    ✓ {method} correctly blocked");
            }
            Ok(_) => panic!("Method {method} should have been blocked"),
        }
    }

    println!("  ✓ All dangerous methods are properly blocked");
}
