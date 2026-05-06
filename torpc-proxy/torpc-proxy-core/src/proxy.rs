//! Client-side ToRPC proxy.
//!
//! Wallets POST RPC requests to a local listener; this proxy SOCKS5-tunnels
//! them through Tor to a configured `.onion` endpoint. The forwarding path
//! used to be hand-rolled (manual `\r\n\r\n`-sniffing on raw bytes), which
//! truncated chunked-encoding responses, dropped the request URI, and had a
//! latent header-smuggling vector. It is now built on `hyper::client::conn`
//! over the SOCKS5-tunneled stream so HTTP/1.1 framing is handled correctly.
//!
//! The discovery server (separate listener used by the wallet UI to detect a
//! running proxy) is now opt-in via `TORPC_DISCOVERY_ENABLE=true` and
//! token-gated when on. Previously it was always on, returned
//! `Access-Control-Allow-Origin: *`, and let any drive-by website fingerprint
//! torpc users. See `discovery.rs`-shaped notes inline below.

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{HeaderName, HeaderValue, AUTHORIZATION, CONTENT_LENGTH, HOST, USER_AGENT};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rand::RngCore;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_socks::tcp::Socks5Stream;
use tracing::{debug, error, info, warn};

/// Hard cap on the response body the proxy will buffer in memory. Onion
/// services serving JSON-RPC don't return more than a few hundred KB; 4 MiB
/// is a generous ceiling that prevents a malicious upstream from exhausting
/// memory on the wallet host.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// How long to wait for a single onion request to complete end-to-end. Tor
/// circuit setup can be 5-10s under load; keep this generous but bounded.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

/// Hop-by-hop headers that must not be forwarded on either leg, per RFC 7230.
/// `host` is rewritten explicitly to point at the onion endpoint so the
/// server's vhost matching works.
fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

/// Headers a wallet may attach that would fingerprint the user when forwarded
/// over Tor. The whole point of routing through Tor is unlinkability; sending
/// `User-Agent: MetaMask/12.3 ...` plus `Accept-Language: en-US,fr;q=0.9` plus
/// `Sec-CH-UA: "Chromium";v="135"` defeats it on a single request.
///
/// Authorization and Cookie are stripped to avoid leaking wallet-host secrets
/// to the operator of the onion. RPC over a raw .onion shouldn't need either.
fn is_wallet_fingerprint(name: &HeaderName) -> bool {
    let n = name.as_str().to_ascii_lowercase();
    matches!(
        n.as_str(),
        "user-agent"
            | "accept-language"
            | "origin"
            | "referer"
            | "cookie"
            | "authorization"
            | "dnt"
            | "x-forwarded-for"
            | "x-real-ip"
    ) || n.starts_with("sec-")
        || n.starts_with("x-wallet-")
}

/// Synthetic User-Agent attached to forwarded requests so the upstream sees a
/// consistent, non-identifying client. Versioned so an operator who upgrades
/// the proxy can correlate logs without per-user per-wallet variance.
fn forwarded_user_agent() -> HeaderValue {
    HeaderValue::from_static(concat!("torpc-proxy/", env!("CARGO_PKG_VERSION")))
}

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

    /// Start the proxy server. Spawns a discovery server too if (and only if)
    /// `TORPC_DISCOVERY_ENABLE=true`. The discovery server returns a small
    /// JSON descriptor that the wallet GUI/CLI uses to detect a running proxy
    /// — see `start_discovery_server`.
    pub async fn run(&self) -> Result<()> {
        let _discovery = self.start_discovery_server();

        let listener = TcpListener::bind(self.config.listen_addr)
            .await
            .with_context(|| format!("Failed to bind {}", self.config.listen_addr))?;

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

    /// Start the discovery HTTP server iff `TORPC_DISCOVERY_ENABLE=true`. The
    /// server is an attack surface (it tells callers `we're running torpc on
    /// port X`), so it's now opt-in and requires a per-launch random token in
    /// `X-Torpc-Token` to respond. Without the env flag, the function returns
    /// a no-op task immediately. The token is logged and persisted to
    /// `${XDG_RUNTIME_DIR:-/tmp}/torpc-discovery.token` (mode 0600) so
    /// trusted local clients (the GUI) can read it.
    fn start_discovery_server(&self) -> tokio::task::JoinHandle<()> {
        if std::env::var("TORPC_DISCOVERY_ENABLE").as_deref() != Ok("true") {
            info!("Discovery server disabled (set TORPC_DISCOVERY_ENABLE=true to enable)");
            return tokio::spawn(async {});
        }

        let config = Arc::clone(&self.config);
        let discovery_port = std::env::var("TORPC_DISCOVERY_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8081u16);

        let token = generate_discovery_token();
        match persist_discovery_token(&token) {
            Ok(path) => info!(
                "Discovery server enabled on 127.0.0.1:{} (token persisted to {})",
                discovery_port,
                path.display()
            ),
            Err(e) => warn!(
                "Discovery enabled on 127.0.0.1:{} but failed to persist token: {} (token still printed below)",
                discovery_port, e
            ),
        }
        // Token is already persisted at mode 0600 to ${XDG_RUNTIME_DIR:-/tmp}/
        // torpc-discovery.token; printing it at INFO duplicates that info into
        // journal/syslog where any local user with log access can grab it.
        // Trusted local clients should read the file. Operators wanting to
        // see the token at startup can `RUST_LOG=debug`.
        debug!("Discovery token (X-Torpc-Token): {}", token);
        let token = Arc::new(token);

        tokio::spawn(async move {
            let discovery_addr: SocketAddr = ([127, 0, 0, 1], discovery_port).into();
            let listener = match TcpListener::bind(discovery_addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind discovery on {}: {}", discovery_addr, e);
                    return;
                }
            };

            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Discovery accept error: {}", e);
                        continue;
                    }
                };
                let config = Arc::clone(&config);
                let token = Arc::clone(&token);
                tokio::spawn(async move {
                    if let Err(e) = handle_discovery_request(stream, config, token).await {
                        debug!("Discovery request error: {}", e);
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
    let service = service_fn(move |req| {
        let config = Arc::clone(&config);
        async move { proxy_request(req, config).await }
    });

    if let Err(e) = hyper::server::conn::http1::Builder::new()
        .serve_connection(io, service)
        .await
    {
        debug!("Failed to serve connection: {}", e);
    }
    Ok(())
}

/// Forward a single HTTP request through Tor. The original method, URI, and
/// headers (minus hop-by-hop and host) are preserved so multiple endpoints
/// (`/rpc`, `/rpc/flashbots`, `/health`, …) on the upstream all work — the
/// previous version hardcoded the path to `/rpc`, which made the flashbots
/// endpoint unreachable from any wallet client.
async fn proxy_request(
    req: Request<Incoming>,
    config: Arc<ProxyConfig>,
) -> Result<Response<Full<Bytes>>> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    info!("Proxying {} {} via Tor", method, uri);

    // Wrap the whole thing in a timeout so a stalled circuit can't hang
    // a wallet indefinitely. The timeout returns 504 to the wallet rather
    // than a half-completed response.
    match tokio::time::timeout(REQUEST_TIMEOUT, do_proxy(req, config)).await {
        Ok(result) => result,
        Err(_) => {
            warn!("Tor request timed out after {:?}", REQUEST_TIMEOUT);
            Ok(simple_response(
                StatusCode::GATEWAY_TIMEOUT,
                "Upstream onion request timed out",
            ))
        }
    }
}

async fn do_proxy(
    req: Request<Incoming>,
    config: Arc<ProxyConfig>,
) -> Result<Response<Full<Bytes>>> {
    let (parts, body) = req.into_parts();

    // Connect through the local Tor SOCKS5 proxy to the onion endpoint.
    let stream = match Socks5Stream::connect(config.tor_proxy, config.onion_endpoint.as_str()).await
    {
        Ok(s) => s,
        Err(e) => {
            error!(
                "Tor SOCKS connect failed: proxy={} onion={} error={}",
                config.tor_proxy, config.onion_endpoint, e
            );
            return Ok(simple_response(
                StatusCode::BAD_GATEWAY,
                "Failed to connect to onion service via Tor",
            ));
        }
    };

    let io = TokioIo::new(stream.into_inner());
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .context("hyper handshake over Tor stream failed")?;
    // Drive the connection in the background — Tor server may push
    // chunked / keep-alive responses that need active reads.
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            debug!("Tor-tunneled hyper connection ended: {}", e);
        }
    });

    // Build the upstream request, preserving the original URI and headers
    // (minus hop-by-hop and the wallet's `host`, which we override to the
    // onion endpoint so the upstream's vhost routing matches).
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/")
        .to_string();
    let mut builder = Request::builder()
        .method(parts.method.clone())
        .uri(path_and_query);

    for (name, value) in parts.headers.iter() {
        if name == HOST || is_hop_by_hop(name) || is_wallet_fingerprint(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder = builder.header(HOST, &config.onion_endpoint);
    // Always set a synthetic User-Agent. We've stripped the wallet's own UA
    // above; without this the upstream gets no UA at all (subtly fingerprintable
    // — most clients send *something*).
    builder = builder.header(USER_AGENT, forwarded_user_agent());

    // Read the request body up to the cap.
    let body_bytes = match Limited::new(body, MAX_RESPONSE_BYTES).collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => {
            warn!("Request body too large or unreadable: {}", e);
            return Ok(simple_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body exceeds proxy limit",
            ));
        }
    };
    if !body_bytes.is_empty() {
        builder = builder.header(CONTENT_LENGTH, body_bytes.len());
    }

    let upstream_req = builder
        .body(Full::new(body_bytes))
        .context("building upstream request")?;

    // Send and collect.
    let upstream_resp = sender
        .send_request(upstream_req)
        .await
        .context("send_request through Tor failed")?;

    let (resp_parts, resp_body) = upstream_resp.into_parts();
    let resp_bytes = match Limited::new(resp_body, MAX_RESPONSE_BYTES).collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => {
            warn!("Response body exceeded {} bytes: {}", MAX_RESPONSE_BYTES, e);
            return Ok(simple_response(
                StatusCode::BAD_GATEWAY,
                "Onion response exceeded maximum size",
            ));
        }
    };
    info!(
        "Onion replied {} ({} bytes)",
        resp_parts.status,
        resp_bytes.len()
    );

    // Mirror the upstream response back to the wallet, preserving status and
    // non-hop headers. We deliberately drop `Authorization` from the upstream
    // response (it shouldn't be set by an onion service, but if it were we
    // would leak it back to the wallet host's process tree).
    let mut out = Response::builder().status(resp_parts.status);
    for (name, value) in resp_parts.headers.iter() {
        if is_hop_by_hop(name) || name == AUTHORIZATION {
            continue;
        }
        out = out.header(name, value);
    }
    out.body(Full::new(resp_bytes))
        .context("building outbound response")
}

fn simple_response(status: StatusCode, msg: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(msg.to_string())))
        .expect("static response builder must succeed")
}

// -----------------------------------------------------------------------------
// Discovery server (opt-in, token-gated)
// -----------------------------------------------------------------------------

fn generate_discovery_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Persist the token to a per-runtime-dir file with mode 0600 so a trusted
/// local GUI client can read it. Returns the chosen path.
fn persist_discovery_token(token: &str) -> Result<PathBuf> {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let path = dir.join("torpc-discovery.token");
    std::fs::write(&path, token).context("writing discovery token")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms).context("setting token permissions")?;
    }

    Ok(path)
}

/// Handle a discovery API request. Behaviour:
/// - `OPTIONS /api/discovery` → 204 with no CORS wildcard (tight allow-list).
/// - `GET /api/discovery` with valid `X-Torpc-Token` → JSON descriptor.
/// - Anything else → 404 / 401 / 405.
async fn handle_discovery_request(
    stream: tokio::net::TcpStream,
    config: Arc<ProxyConfig>,
    token: Arc<String>,
) -> Result<()> {
    let io = TokioIo::new(stream);
    let service = service_fn(move |req: Request<Incoming>| {
        let config = Arc::clone(&config);
        let token = Arc::clone(&token);
        async move { discovery_handler(req, config, token).await }
    });

    let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
        .serve_connection(io, service)
        .await;
    Ok(())
}

async fn discovery_handler(
    req: Request<Incoming>,
    config: Arc<ProxyConfig>,
    expected_token: Arc<String>,
) -> Result<Response<Full<Bytes>>, anyhow::Error> {
    // Consistent CSP for every discovery response — no localhost-port
    // wildcards, since those let a browser enumerate ports via timing.
    let csp = HeaderValue::from_static("default-src 'none'; connect-src 'self'");

    if req.method() == Method::OPTIONS {
        // Preflight — refuse the wildcard origin the old code allowed. The
        // GUI/CLI doesn't need CORS at all (they're not browsers).
        return Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header("content-security-policy", csp)
            .body(Full::new(Bytes::new()))
            .unwrap());
    }

    if req.method() != Method::GET || req.uri().path() != "/api/discovery" {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-security-policy", csp)
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap());
    }

    // Token check — require a literal match on `X-Torpc-Token`. Constant-time
    // compare to defeat the (admittedly small) timing oracle of `==` on
    // strings of equal length.
    let provided = req
        .headers()
        .get("x-torpc-token")
        .and_then(|v| v.to_str().ok());
    if !provided
        .map(|p| constant_time_eq(p.as_bytes(), expected_token.as_bytes()))
        .unwrap_or(false)
    {
        warn!("discovery: rejected request without valid X-Torpc-Token");
        return Ok(Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("content-security-policy", csp)
            .body(Full::new(Bytes::from("Unauthorized")))
            .unwrap());
    }

    let body = serde_json::json!({
        "status": "running",
        "proxy": {
            "listen_addr": config.listen_addr.to_string(),
            "version": env!("CARGO_PKG_VERSION"),
        },
        "suggested_rpc_url": format!("http://{}", config.listen_addr),
    });
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("content-security-policy", csp)
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap())
}

/// Constant-time byte comparison. Same length is short-circuited (the length
/// itself is not a useful side-channel here since the token is fixed length).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_config() {
        let config = ProxyConfig {
            listen_addr: ([127, 0, 0, 1], 8545).into(),
            tor_proxy: ([127, 0, 0, 1], 9050).into(),
            onion_endpoint: "test.onion:8545".to_string(),
        };
        assert_eq!(config.listen_addr.port(), 8545);
        assert_eq!(config.tor_proxy.port(), 9050);
    }

    #[test]
    fn test_token_is_64_hex_chars() {
        let t = generate_discovery_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_token_uniqueness() {
        let a = generate_discovery_token();
        let b = generate_discovery_token();
        // Birthday probability for 256 random bits is negligible.
        assert_ne!(a, b);
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn test_hop_by_hop_classification() {
        for h in &["connection", "keep-alive", "transfer-encoding", "upgrade"] {
            let name: HeaderName = h.parse().unwrap();
            assert!(is_hop_by_hop(&name), "{h} should be hop-by-hop");
        }
        for h in &["content-type", "x-flashbots-signature"] {
            let name: HeaderName = h.parse().unwrap();
            assert!(!is_hop_by_hop(&name), "{h} should be end-to-end");
        }
    }

    #[test]
    fn wallet_fingerprint_headers_are_stripped() {
        // Anything that would identify the wallet, the user, or the local
        // browser fingerprint to the operator of the .onion.
        for h in &[
            "user-agent",
            "User-Agent",
            "accept-language",
            "origin",
            "referer",
            "cookie",
            "authorization",
            "dnt",
            "x-forwarded-for",
            "x-real-ip",
            "sec-fetch-site",
            "sec-fetch-mode",
            "sec-ch-ua",
            "x-wallet-name",
        ] {
            let name: HeaderName = h.to_lowercase().parse().unwrap();
            assert!(
                is_wallet_fingerprint(&name),
                "{h} must be classified as wallet-fingerprint and stripped"
            );
        }
    }

    #[test]
    fn benign_headers_are_forwarded() {
        // These must NOT be stripped — they're either required for the JSON-RPC
        // protocol (`content-type`) or actively load-bearing (`x-flashbots-*`).
        for h in &[
            "content-type",
            "content-length",
            "accept",
            "x-flashbots-signature",
        ] {
            let name: HeaderName = h.parse().unwrap();
            assert!(
                !is_wallet_fingerprint(&name),
                "{h} must not be classified as wallet-fingerprint"
            );
        }
    }

    #[test]
    fn forwarded_user_agent_is_synthetic_and_versioned() {
        let ua = forwarded_user_agent();
        let s = ua.to_str().unwrap();
        assert!(s.starts_with("torpc-proxy/"));
        // Must not include any wallet/vendor identifier.
        for forbidden in &["MetaMask", "Coinbase", "Mozilla", "Chrome", "Tauri"] {
            assert!(
                !s.contains(forbidden),
                "synthetic UA should not contain {forbidden}: {s}"
            );
        }
    }
}
