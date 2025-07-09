use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_socks::tcp::Socks5Stream;
use tracing::{debug, error, info};

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub listen_addr: SocketAddr,
    pub tor_proxy: SocketAddr,
    pub onion_endpoint: String,
}

pub struct TorRpcProxy {
    config: Arc<ProxyConfig>,
}

impl TorRpcProxy {
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Start the proxy server
    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(self.config.listen_addr)
            .await
            .with_context(|| format!("Failed to bind to address {}", self.config.listen_addr))?;

        info!(
            "ToRPC proxy listening on http://{}",
            self.config.listen_addr
        );
        info!("Forwarding to {} via Tor", self.config.onion_endpoint);

        loop {
            let (stream, addr) = listener.accept().await?;
            let config = Arc::clone(&self.config);

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, addr, config).await {
                    error!("Error handling connection from {}: {}", addr, e);
                }
            });
        }
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    config: Arc<ProxyConfig>,
) -> Result<()> {
    debug!("New connection from {}", addr);

    let io = TokioIo::new(stream);

    // Create service function with config
    let service = service_fn(move |req| {
        let config = Arc::clone(&config);
        async move { proxy_request(req, config).await }
    });

    // Serve the connection
    if let Err(e) = hyper::server::conn::http1::Builder::new()
        .serve_connection(io, service)
        .await
    {
        error!("Failed to serve connection: {}", e);
    }

    Ok(())
}

async fn proxy_request(
    req: Request<Incoming>,
    config: Arc<ProxyConfig>,
) -> Result<Response<Full<Bytes>>> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();

    debug!("Proxying {} request to {}", method, uri);

    // Collect the request body
    let body_bytes = req.collect().await?.to_bytes();

    // Connect through Tor
    let tor_stream =
        match Socks5Stream::connect(config.tor_proxy, config.onion_endpoint.as_str()).await {
            Ok(stream) => stream,
            Err(e) => {
                error!("Failed to connect through Tor: {}", e);
                return Ok(Response::builder()
                    .status(502)
                    .body(Full::new(Bytes::from("Failed to connect through Tor")))
                    .unwrap());
            }
        };

    let mut stream = tor_stream.into_inner();

    // Build HTTP request
    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

    let mut request = BytesMut::new();
    request.extend_from_slice(format!("{method} {path} HTTP/1.1\r\n").as_bytes());
    request.extend_from_slice(format!("Host: {}\r\n", config.onion_endpoint).as_bytes());

    // Copy headers
    for (name, value) in headers.iter() {
        if name != "host" {
            request.extend_from_slice(format!("{name}: {}\r\n", value.to_str()?).as_bytes());
        }
    }

    // Add content length if we have a body
    if !body_bytes.is_empty() {
        request.extend_from_slice(format!("Content-Length: {}\r\n", body_bytes.len()).as_bytes());
    }

    request.extend_from_slice(b"\r\n");
    request.extend_from_slice(&body_bytes);

    // Send request
    stream.write_all(&request).await?;
    stream.flush().await?;

    // Read response
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;

    // Parse response (simplified - just return as-is)
    Ok(Response::builder()
        .status(200)
        .body(Full::new(Bytes::from(response)))
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;
    use tokio::time::{timeout, Duration};

    #[test]
    fn test_proxy_config() {
        let config = ProxyConfig {
            listen_addr: ([127, 0, 0, 1], 8545).into(),
            tor_proxy: ([127, 0, 0, 1], 9050).into(),
            onion_endpoint: "test.onion:8545".to_string(),
        };

        assert_eq!(config.listen_addr.port(), 8545);
        assert_eq!(config.tor_proxy.port(), 9050);
        assert_eq!(config.onion_endpoint, "test.onion:8545");
    }

    #[test]
    fn test_proxy_creation() {
        let config = ProxyConfig {
            listen_addr: ([127, 0, 0, 1], 8545).into(),
            tor_proxy: ([127, 0, 0, 1], 9050).into(),
            onion_endpoint: "test.onion:8545".to_string(),
        };

        let proxy = TorRpcProxy::new(config.clone());
        assert_eq!(proxy.config.listen_addr, config.listen_addr);
    }

    #[tokio::test]
    async fn test_proxy_bind_failure() {
        // Try to bind to a privileged port that should fail
        let config = ProxyConfig {
            listen_addr: ([127, 0, 0, 1], 1).into(),
            tor_proxy: ([127, 0, 0, 1], 9050).into(),
            onion_endpoint: "test.onion:8545".to_string(),
        };

        let proxy = TorRpcProxy::new(config);
        let result = timeout(Duration::from_secs(1), proxy.run()).await;

        assert!(result.is_ok()); // Timeout is ok
        let inner_result = result.unwrap();
        assert!(inner_result.is_err()); // Should fail to bind
    }

    #[tokio::test]
    async fn test_proxy_accepts_connections() {
        let config = ProxyConfig {
            listen_addr: ([127, 0, 0, 1], 0).into(), // Use port 0 for auto-assignment
            tor_proxy: ([127, 0, 0, 1], 9050).into(),
            onion_endpoint: "test.onion:8545".to_string(),
        };

        // Create a listener to get the actual port
        let listener = TcpListener::bind(config.listen_addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut config = config;
        config.listen_addr = actual_addr;

        let proxy = TorRpcProxy::new(config);
        let proxy_handle = tokio::spawn(async move { proxy.run().await });

        // Give the proxy time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Try to connect
        let connect_result = timeout(Duration::from_secs(1), TcpStream::connect(actual_addr)).await;

        assert!(connect_result.is_ok());

        // Clean up
        proxy_handle.abort();
    }
}
