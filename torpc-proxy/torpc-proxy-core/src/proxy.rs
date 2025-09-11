use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Method};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_socks::tcp::Socks5Stream;
use tracing::{debug, error, info, trace};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
        // Start discovery server
        let _discovery_handle = self.start_discovery_server();
        
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
    
    /// Start the discovery HTTP server on port 8081
    fn start_discovery_server(&self) -> tokio::task::JoinHandle<()> {
        let config = Arc::clone(&self.config);
        let discovery_port = std::env::var("TORPC_DISCOVERY_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8081);
        
        tokio::spawn(async move {
            let discovery_addr: SocketAddr = ([127, 0, 0, 1], discovery_port).into();
            
            let listener = match TcpListener::bind(discovery_addr).await {
                Ok(listener) => {
                    info!("Discovery API listening on http://{}", discovery_addr);
                    listener
                }
                Err(e) => {
                    error!("Failed to start discovery server on port {}: {}", discovery_port, e);
                    return;
                }
            };
            
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!("Discovery server accept error: {}", e);
                        continue;
                    }
                };
                
                let config = Arc::clone(&config);
                tokio::spawn(async move {
                    if let Err(e) = handle_discovery_request(stream, config).await {
                        debug!("Error handling discovery request: {}", e);
                    }
                });
            }
        })
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

    info!("Proxying {} request to {}", method, uri);
    debug!("Request headers: {:?}", headers);

    // Collect the request body
    let body_bytes = req.collect().await?.to_bytes();

    // Connect through Tor
    debug!("Attempting to connect through Tor");
    debug!("Tor SOCKS5 proxy: {}", config.tor_proxy);
    debug!("Target onion endpoint: {}", config.onion_endpoint);
    trace!("Request body size: {} bytes", body_bytes.len());
    
    let tor_stream =
        match Socks5Stream::connect(config.tor_proxy, config.onion_endpoint.as_str()).await {
            Ok(stream) => {
                info!("Successfully connected to {} through Tor", config.onion_endpoint);
                stream
            }
            Err(e) => {
                error!(
                    "Failed to connect through Tor proxy {} to {}: {}",
                    config.tor_proxy, config.onion_endpoint, e
                );
                
                // Provide more helpful error messages based on the error type
                let error_msg = if e.to_string().contains("Connection refused") {
                    format!(
                        "Connection refused: The onion service at {} may not be running or the address is incorrect. \
                        Tor proxy at {} is working correctly.",
                        config.onion_endpoint, config.tor_proxy
                    )
                } else {
                    format!(
                        "Failed to connect through Tor: {} (proxy: {}, target: {})",
                        e, config.tor_proxy, config.onion_endpoint
                    )
                };
                
                return Ok(Response::builder()
                    .status(502)
                    .body(Full::new(Bytes::from(error_msg)))
                    .unwrap());
            }
        };

    let mut stream = tor_stream.into_inner();

    // Build HTTP request
    // Always forward to /rpc path for RPC requests
    let path = "/rpc";
    info!("Forwarding request to path: {} on {}", path, config.onion_endpoint);

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
    debug!("Sending HTTP request ({} bytes)", request.len());
    stream.write_all(&request).await?;
    stream.flush().await?;
    info!("Request sent, waiting for response...");

    // Read response
    let mut response = Vec::new();
    let mut buffer = [0u8; 8192];
    
    // First, read headers until we find \r\n\r\n
    let mut headers_complete = false;
    let mut content_length: Option<usize> = None;
    
    while !headers_complete {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            stream.read(&mut buffer)
        ).await {
            Ok(Ok(0)) => {
                error!("Connection closed while reading headers");
                return Ok(Response::builder()
                    .status(502)
                    .body(Full::new(Bytes::from("Connection closed by server")))
                    .unwrap());
            }
            Ok(Ok(n)) => {
                response.extend_from_slice(&buffer[..n]);
                
                // Check if we have complete headers
                if let Some(header_end) = response.windows(4).position(|w| w == b"\r\n\r\n") {
                    headers_complete = true;
                    
                    // Parse Content-Length if present
                    let headers_bytes = &response[..header_end];
                    if let Ok(headers_str) = std::str::from_utf8(headers_bytes) {
                        for line in headers_str.lines() {
                            if line.to_lowercase().starts_with("content-length:") {
                                if let Some(len_str) = line.split(':').nth(1) {
                                    content_length = len_str.trim().parse().ok();
                                }
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                error!("Error reading headers: {}", e);
                return Ok(Response::builder()
                    .status(502)
                    .body(Full::new(Bytes::from("Error reading response headers")))
                    .unwrap());
            }
            Err(_) => {
                error!("Timeout reading response headers");
                return Ok(Response::builder()
                    .status(504)
                    .body(Full::new(Bytes::from("Gateway timeout")))
                    .unwrap());
            }
        }
    }
    
    // Now read the body if there is one
    if let Some(header_end) = response.windows(4).position(|w| w == b"\r\n\r\n") {
        let body_start = header_end + 4;
        let current_body_size = response.len() - body_start;
        
        // If we have Content-Length, read exactly that many bytes
        if let Some(expected_length) = content_length {
            while current_body_size + (response.len() - body_start) < expected_length {
                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(10),
                    stream.read(&mut buffer)
                ).await {
                    Ok(Ok(0)) => break, // EOF
                    Ok(Ok(n)) => response.extend_from_slice(&buffer[..n]),
                    Ok(Err(e)) => {
                        error!("Error reading body: {}", e);
                        break;
                    }
                    Err(_) => {
                        error!("Timeout reading response body");
                        break;
                    }
                }
            }
        }
    }

    // Parse HTTP response
    if response.is_empty() {
        error!("Empty response from onion service");
        return Ok(Response::builder()
            .status(502)
            .body(Full::new(Bytes::from("Empty response from onion service")))
            .unwrap());
    }

    // Find the end of headers
    let header_end = response.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("Invalid HTTP response: no header terminator"))?;

    let headers_bytes = &response[..header_end];
    let body_start = header_end + 4;
    let body = &response[body_start..];

    // Parse status line
    let headers_str = std::str::from_utf8(headers_bytes)?;
    let mut lines = headers_str.lines();
    let status_line = lines.next().ok_or_else(|| anyhow::anyhow!("No status line"))?;
    
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);

    info!("Response status: {} (body size: {} bytes)", status_code, body.len());
    if body.len() < 1000 {
        debug!("Response body: {}", String::from_utf8_lossy(body));
    }

    Ok(Response::builder()
        .status(status_code)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.to_vec())))
        .unwrap())
}

/// Handle discovery API requests
async fn handle_discovery_request(
    stream: tokio::net::TcpStream,
    config: Arc<ProxyConfig>,
) -> Result<()> {
    let io = TokioIo::new(stream);
    
    let service = service_fn(|req: Request<Incoming>| {
        let config = Arc::clone(&config);
        async move {
            // Only handle GET /api/discovery
            if req.method() == Method::GET && req.uri().path() == "/api/discovery" {
                let response_body = serde_json::json!({
                    "status": "running",
                    "proxy": {
                        "listen_addr": config.listen_addr.to_string(),
                        "rpc_endpoint": "",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "suggested_rpc_url": format!("http://{}", config.listen_addr),
                });
                
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Access-Control-Allow-Methods", "GET, OPTIONS")
                    .header("Access-Control-Allow-Headers", "Content-Type")
                    .header("Content-Security-Policy", "default-src 'self'; connect-src 'self' http://localhost:* ws://localhost:* wss://localhost:*")
                    .body(Full::new(Bytes::from(response_body.to_string())))
                    .unwrap();
                
                Ok::<_, anyhow::Error>(response)
            } else if req.method() == Method::OPTIONS {
                // Handle CORS preflight
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Access-Control-Allow-Methods", "GET, OPTIONS")
                    .header("Access-Control-Allow-Headers", "Content-Type")
                    .header("Content-Security-Policy", "default-src 'self'; connect-src 'self' http://localhost:* ws://localhost:* wss://localhost:*")
                    .body(Full::new(Bytes::new()))
                    .unwrap();
                
                Ok(response)
            } else {
                let response = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("Content-Security-Policy", "default-src 'self'; connect-src 'self' http://localhost:* ws://localhost:* wss://localhost:*")
                    .body(Full::new(Bytes::from("Not Found")))
                    .unwrap();
                
                Ok(response)
            }
        }
    });
    
    let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
        .serve_connection(io, service)
        .await;
    
    Ok(())
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
