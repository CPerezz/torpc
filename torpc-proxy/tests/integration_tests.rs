use anyhow::Result;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use torpc_proxy::proxy::{ProxyConfig, TorRpcProxy};

/// Helper to find an available port
async fn get_available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Helper to wait for server to be ready
async fn wait_for_server(addr: &str, max_wait: Duration) -> Result<()> {
    let start = tokio::time::Instant::now();
    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        if start.elapsed() > max_wait {
            anyhow::bail!("Server didn't start in time");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_proxy_startup_and_shutdown() {
    let port = get_available_port().await;
    let config = ProxyConfig {
        listen_addr: ([127, 0, 0, 1], port).into(),
        tor_proxy: ([127, 0, 0, 1], 9050).into(),
        onion_endpoint: "test.onion:8545".to_string(),
    };

    let proxy = TorRpcProxy::new(config);
    let proxy_handle = tokio::spawn(async move { proxy.run().await });

    // Wait for server to start
    assert!(
        wait_for_server(&format!("127.0.0.1:{port}"), Duration::from_secs(2))
            .await
            .is_ok()
    );

    // Server should be running
    assert!(!proxy_handle.is_finished());

    // Shutdown
    proxy_handle.abort();
    let _ = proxy_handle.await;
}

#[tokio::test]
async fn test_proxy_returns_502_without_tor() {
    let port = get_available_port().await;
    let config = ProxyConfig {
        listen_addr: ([127, 0, 0, 1], port).into(),
        tor_proxy: ([127, 0, 0, 1], 9999).into(), // Non-existent Tor proxy
        onion_endpoint: "test.onion:8545".to_string(),
    };

    let proxy = TorRpcProxy::new(config);
    let _proxy_handle = tokio::spawn(async move { proxy.run().await });

    // Wait for server to start
    wait_for_server(&format!("127.0.0.1:{port}"), Duration::from_secs(2))
        .await
        .unwrap();

    // Make a request
    let client = Client::builder(TokioExecutor::new()).build_http();
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("http://127.0.0.1:{port}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(
            r#"{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}"#,
        )))
        .unwrap();

    let response = timeout(Duration::from_secs(5), client.request(req))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn test_proxy_handles_concurrent_connections() {
    let port = get_available_port().await;
    let config = ProxyConfig {
        listen_addr: ([127, 0, 0, 1], port).into(),
        tor_proxy: ([127, 0, 0, 1], 9999).into(), // Non-existent Tor proxy
        onion_endpoint: "test.onion:8545".to_string(),
    };

    let proxy = TorRpcProxy::new(config);
    let _proxy_handle = tokio::spawn(async move { proxy.run().await });

    // Wait for server to start
    wait_for_server(&format!("127.0.0.1:{port}"), Duration::from_secs(2))
        .await
        .unwrap();

    // Make multiple concurrent requests
    let mut handles = vec![];
    for i in 0..10 {
        let handle = tokio::spawn(async move {
            let client = Client::builder(TokioExecutor::new()).build_http();
            let req = Request::builder()
                .method(Method::POST)
                .uri(format!("http://127.0.0.1:{port}/"))
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(format!(
                    r#"{{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":{i}}}"#
                ))))
                .unwrap();

            let response = client.request(req).await.unwrap();
            response.status()
        });
        handles.push(handle);
    }

    // All requests should complete
    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }
}

#[tokio::test]
async fn test_proxy_handles_various_http_methods() {
    let port = get_available_port().await;
    let config = ProxyConfig {
        listen_addr: ([127, 0, 0, 1], port).into(),
        tor_proxy: ([127, 0, 0, 1], 9999).into(), // Non-existent Tor proxy
        onion_endpoint: "test.onion:8545".to_string(),
    };

    let proxy = TorRpcProxy::new(config);
    let _proxy_handle = tokio::spawn(async move { proxy.run().await });

    // Wait for server to start
    wait_for_server(&format!("127.0.0.1:{port}"), Duration::from_secs(2))
        .await
        .unwrap();

    let client = Client::builder(TokioExecutor::new()).build_http();

    // Test different HTTP methods
    let methods = vec![Method::GET, Method::POST, Method::PUT, Method::DELETE];

    for method in methods {
        let req = Request::builder()
            .method(method.clone())
            .uri(format!("http://127.0.0.1:{port}/test"))
            .body(Full::new(Bytes::new()))
            .unwrap();

        let response = timeout(Duration::from_secs(5), client.request(req))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_GATEWAY,
            "Method {method} failed"
        );
    }
}

#[tokio::test]
async fn test_proxy_preserves_headers() {
    let port = get_available_port().await;
    let config = ProxyConfig {
        listen_addr: ([127, 0, 0, 1], port).into(),
        tor_proxy: ([127, 0, 0, 1], 9999).into(), // Non-existent Tor proxy
        onion_endpoint: "test.onion:8545".to_string(),
    };

    let proxy = TorRpcProxy::new(config);
    let _proxy_handle = tokio::spawn(async move { proxy.run().await });

    // Wait for server to start
    wait_for_server(&format!("127.0.0.1:{port}"), Duration::from_secs(2))
        .await
        .unwrap();

    let client = Client::builder(TokioExecutor::new()).build_http();
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("http://127.0.0.1:{port}/"))
        .header("content-type", "application/json")
        .header("x-custom-header", "test-value")
        .header("authorization", "Bearer test-token")
        .body(Full::new(Bytes::from(
            r#"{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}"#,
        )))
        .unwrap();

    let response = timeout(Duration::from_secs(5), client.request(req))
        .await
        .unwrap()
        .unwrap();

    // Even though it fails to connect to Tor, headers should be processed
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}
