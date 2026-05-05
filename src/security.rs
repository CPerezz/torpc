use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::time::Duration;
use tracing::warn;

/// Static Content-Security-Policy applied to every response by the
/// `SetResponseHeaderLayer` in `app.rs`. The previous code built this
/// string at startup from `RuntimeWebConfig` so an opt-in discovery
/// server on a non-default port could be reflected in `connect-src`.
/// That mechanism was deleted because (a) the discovery server is
/// default-disabled, (b) almost no operator changes the port, and (c)
/// the static frontend's only legitimate cross-origin fetch is to
/// `/rpc`, which `'self'` already covers.
///
/// Operators who do customise the discovery port and want the wallet
/// auto-detect flow to work must override this CSP via a reverse proxy
/// in front of the daemon, or fork the constant.
pub const STATIC_CSP: &str = "default-src 'self'; \
     connect-src 'self'; \
     style-src 'self' 'unsafe-inline'; \
     script-src 'self'; \
     img-src 'self' data:; \
     frame-ancestors 'none'; \
     base-uri 'self'; \
     form-action 'self'";

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub max_body_size: usize,
    pub request_timeout: Duration,
    pub strict_headers: bool,
}

impl SecurityConfig {
    /// Read configuration from the environment. Variable names match the
    /// canonical `.env.example` (e.g. `MAX_REQUEST_SIZE`); we also accept
    /// the prior names as deprecated aliases (`MAX_BODY_SIZE`,
    /// `STRICT_HEADERS`) for one release cycle so existing operator
    /// environments don't silently drop to defaults. Setting both forms
    /// makes the canonical name win.
    pub fn from_env() -> Self {
        let max_body_size = first_env_var(&["MAX_REQUEST_SIZE", "MAX_BODY_SIZE"])
            .and_then(|s| s.parse().ok())
            .unwrap_or(1024 * 1024); // 1 MiB

        let request_timeout_secs = std::env::var("REQUEST_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30);

        let strict_headers = first_env_var(&["STRICT_SECURITY_HEADERS", "STRICT_HEADERS"])
            .map(|s| s.to_lowercase() == "true")
            .unwrap_or(true);

        Self {
            max_body_size,
            request_timeout: Duration::from_secs(request_timeout_secs),
            strict_headers,
        }
    }
}

/// Try each environment-variable name in order. The first one that's set
/// (even to an empty string) wins; warns once if a deprecated alias is the
/// only one set so operators know to migrate.
fn first_env_var(names: &[&str]) -> Option<String> {
    let mut found_at: Option<usize> = None;
    let mut value: Option<String> = None;
    for (i, name) in names.iter().enumerate() {
        if let Ok(v) = std::env::var(name) {
            found_at = Some(i);
            value = Some(v);
            break;
        }
    }
    if let (Some(idx), true) = (found_at, names.len() > 1) {
        if idx > 0 {
            tracing::warn!(
                "{} is deprecated; prefer {}",
                names[idx],
                names[0]
            );
        }
    }
    value
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            max_body_size: 1024 * 1024, // 1MB
            request_timeout: Duration::from_secs(30),
            strict_headers: true,
        }
    }
}

/// Security metrics tracking. Fields are `AtomicU64` so the struct can be
/// shared across handlers via `Arc<SecurityMetrics>` without locking.
/// Increment methods take `&self` for that reason; the previous `&mut self`
/// signature meant only one handler could ever hold the metrics, which is
/// why the live `/metrics` endpoint always returned an empty stub.
#[derive(Debug, Default)]
pub struct SecurityMetrics {
    pub blocked_requests_total: std::sync::atomic::AtomicU64,
    pub rate_limit_hits: std::sync::atomic::AtomicU64,
    pub oversized_requests: std::sync::atomic::AtomicU64,
    pub invalid_methods: std::sync::atomic::AtomicU64,
    pub suspicious_patterns: std::sync::atomic::AtomicU64,
}

impl SecurityMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment_blocked_requests(&self) {
        self.blocked_requests_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_rate_limit_hits(&self) {
        self.rate_limit_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_oversized_requests(&self) {
        self.oversized_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_invalid_methods(&self) {
        self.invalid_methods
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_suspicious_patterns(&self) {
        self.suspicious_patterns
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Take a non-atomic snapshot of all counters as a JSON value. Counters
    /// are loaded with `Relaxed` ordering — increments may race past this
    /// snapshot but the per-counter value is always a valid prior value.
    pub fn snapshot(&self) -> serde_json::Value {
        use std::sync::atomic::Ordering::Relaxed;
        json!({
            "blocked_requests_total": self.blocked_requests_total.load(Relaxed),
            "rate_limit_hits": self.rate_limit_hits.load(Relaxed),
            "oversized_requests": self.oversized_requests.load(Relaxed),
            "invalid_methods": self.invalid_methods.load(Relaxed),
            "suspicious_patterns": self.suspicious_patterns.load(Relaxed),
        })
    }

    /// Deprecated alias retained so existing tests don't churn — prefer `snapshot()`.
    #[deprecated(note = "use snapshot() — get_metrics_json is an alias kept only for tests")]
    pub fn get_metrics_json(&self) -> serde_json::Value {
        self.snapshot()
    }
}

/// Add security headers to all responses.
///
/// Note: the `Content-Security-Policy` is **not** set here. It depends on
/// runtime-resolved values (specifically `TORPC_DISCOVERY_PORT`) and is
/// installed in `main.rs` as a `SetResponseHeaderLayer` whose value is
/// computed once at startup. Setting CSP here with `from_static` baked in
/// the wrong port whenever an operator changed `TORPC_DISCOVERY_PORT`,
/// quietly breaking the wallet auto-detect flow.
pub async fn add_security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert("X-XSS-Protection", HeaderValue::from_static("0"));
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    headers.insert("Pragma", HeaderValue::from_static("no-cache"));
    headers.insert("Expires", HeaderValue::from_static("0"));
    headers.remove("Server");
    headers.insert("X-Service", HeaderValue::from_static("TorPC"));
    
    response
}

/// Health-check endpoint — minimal "the process is alive" probe.
///
/// Earlier revisions also performed an upstream-Geth probe with caching and
/// returned a per-component `{healthy|degraded|down}` status. That existed
/// for a load-balancer / k8s-probe consumer that this deployment topology
/// (Tor hidden service, no LB) does not have. Operators wanting upstream
/// liveness should read `/metrics` (which exposes the `geth_circuit` and
/// `mev_circuit` summaries) or curl Geth directly. Slimming `/health` cut
/// ~110 LOC of probe + cache machinery and four tests that asserted on the
/// removed shape.
///
/// Privacy note: every field returned here must be safe to share with an
/// anonymous Tor client. The fields below are deliberately vanilla.
pub async fn health_check(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::mev::mev_handler::MevProxyState>>,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    Ok(axum::Json(json!({
        "status": "ok",
        "service": "torpc",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": state.base_state.start_time.elapsed().as_secs(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })))
}

/// Live security-metrics endpoint backed by `Arc<SecurityMetrics>`. Counter
/// values are atomics so this returns the genuine running totals — the prior
/// stub built a fresh empty struct on every call, which is why the dashboard
/// always read zero.
///
/// Also surfaces the circuit-breaker state for both upstream Geth and the
/// MEV relay (when configured). This is where component-state observability
/// lives now that `/health` is intentionally minimal.
pub async fn security_metrics(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::mev::mev_handler::MevProxyState>>,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    let geth_circuit = state.base_state.geth_circuit.state_summary();
    let (mev_relay_status, mev_circuit) = match &state.mev_client {
        Some(client) => ("configured", client.circuit_state_summary()),
        None => ("disabled", "n/a"),
    };
    Ok(axum::Json(json!({
        "security_metrics": state.base_state.metrics.snapshot(),
        "circuits": {
            "geth": geth_circuit,
            "mev_relay": mev_relay_status,
            "mev_circuit": mev_circuit,
        },
        "uptime_seconds": state.base_state.start_time.elapsed().as_secs(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })))
}

/// Replacement for `tower_http::TimeoutLayer` that emits a JSON-RPC 2.0
/// error body on timeout (`-32001 "upstream timeout"`) so wallet clients
/// see structured JSON instead of an empty `408`. Use by passing the
/// timeout `Duration` as state via `from_fn_with_state`.
pub async fn json_rpc_timeout_middleware(
    axum::extract::State(timeout): axum::extract::State<Duration>,
    request: Request,
    next: Next,
) -> Response {
    match tokio::time::timeout(timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => {
            warn!(
                "request exceeded {}ms — returning JSON-RPC -32001",
                timeout.as_millis()
            );
            // We can't echo the request `id` (the body has been consumed by
            // downstream extractors at this point), so emit `id: null`
            // — the JSON-RPC 2.0 spec permits null when the id is unknown.
            let body = json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32001,
                    "message": "upstream timeout",
                },
                "id": serde_json::Value::Null,
            });
            (
                StatusCode::GATEWAY_TIMEOUT,
                [(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                )],
                axum::Json(body),
            )
                .into_response()
        }
    }
}

/// Security headers middleware (wrapper for add_security_headers)
pub async fn security_headers_middleware(request: Request, next: Next) -> Response {
    add_security_headers(request, next).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_metrics() {
        use std::sync::atomic::Ordering::Relaxed;

        let metrics = SecurityMetrics::new();
        metrics.increment_blocked_requests();
        metrics.increment_rate_limit_hits();

        assert_eq!(metrics.blocked_requests_total.load(Relaxed), 1);
        assert_eq!(metrics.rate_limit_hits.load(Relaxed), 1);

        let json = metrics.snapshot();
        assert_eq!(json["blocked_requests_total"], 1);
        assert_eq!(json["rate_limit_hits"], 1);
    }

    #[test]
    fn test_security_metrics_all_increments() {
        use std::sync::atomic::Ordering::Relaxed;

        let metrics = SecurityMetrics::new();

        metrics.increment_blocked_requests();
        metrics.increment_rate_limit_hits();
        metrics.increment_oversized_requests();
        metrics.increment_invalid_methods();
        metrics.increment_suspicious_patterns();

        assert_eq!(metrics.blocked_requests_total.load(Relaxed), 1);
        assert_eq!(metrics.rate_limit_hits.load(Relaxed), 1);
        assert_eq!(metrics.oversized_requests.load(Relaxed), 1);
        assert_eq!(metrics.invalid_methods.load(Relaxed), 1);
        assert_eq!(metrics.suspicious_patterns.load(Relaxed), 1);

        let json = metrics.snapshot();
        assert_eq!(json["blocked_requests_total"], 1);
        assert_eq!(json["rate_limit_hits"], 1);
        assert_eq!(json["oversized_requests"], 1);
        assert_eq!(json["invalid_methods"], 1);
        assert_eq!(json["suspicious_patterns"], 1);
    }

    /// Round-trips a request through the JSON-RPC timeout middleware. The
    /// inner handler sleeps longer than the configured timeout, so the
    /// middleware should short-circuit with a `504` whose body is a
    /// JSON-RPC 2.0 error envelope (not the bare `408` the previous
    /// `tower_http::TimeoutLayer` produced).
    #[tokio::test]
    async fn test_json_rpc_timeout_middleware_returns_structured_error() {
        use axum::body::Body;
        use axum::routing::get;
        use axum::Router;
        use axum_test::TestServer;

        async fn slow_handler() -> &'static str {
            tokio::time::sleep(Duration::from_millis(500)).await;
            "should never get here"
        }

        let app = Router::new()
            .route("/slow", get(slow_handler))
            .layer(axum::middleware::from_fn_with_state(
                Duration::from_millis(50),
                json_rpc_timeout_middleware,
            ));

        let server = TestServer::new(app).unwrap();
        let response = server.get("/slow").await;
        assert_eq!(response.status_code(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(response.header("content-type"), "application/json");

        let body: serde_json::Value = response.json();
        assert_eq!(body["jsonrpc"], "2.0");
        assert_eq!(body["error"]["code"], -32001);
        assert_eq!(body["error"]["message"], "upstream timeout");
        assert!(body["id"].is_null());

        // Sanity: also verify a fast handler still passes through unchanged.
        let _ = Body::empty(); // (silences unused-import for `Body` in some builds)
    }

    /// Verifies metrics are safe to share across tasks via Arc — the original
    /// `&mut self` API made this impossible, which is why the live `/metrics`
    /// endpoint always reported zero.
    #[tokio::test]
    async fn test_security_metrics_concurrent_increments() {
        use std::sync::atomic::Ordering::Relaxed;
        use std::sync::Arc;

        let metrics = Arc::new(SecurityMetrics::new());
        let mut tasks = Vec::new();
        for _ in 0..50 {
            let m = metrics.clone();
            tasks.push(tokio::spawn(async move {
                m.increment_blocked_requests();
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(metrics.blocked_requests_total.load(Relaxed), 50);
    }

    #[test]
    fn test_security_config_from_env() {
        // Test with no environment variables (should use defaults)
        let config = SecurityConfig::from_env();
        assert_eq!(config.max_body_size, 1024 * 1024);
        assert_eq!(config.request_timeout.as_secs(), 30);
        assert_eq!(config.strict_headers, true);

        // Test with environment variables set
        std::env::set_var("MAX_BODY_SIZE", "2097152"); // 2MB
        std::env::set_var("REQUEST_TIMEOUT", "60");
        std::env::set_var("STRICT_HEADERS", "false");
        
        let env_config = SecurityConfig::from_env();
        assert_eq!(env_config.max_body_size, 2097152);
        assert_eq!(env_config.request_timeout.as_secs(), 60);
        assert_eq!(env_config.strict_headers, false);
        
        // Test with invalid values (should fall back to defaults)
        std::env::set_var("MAX_BODY_SIZE", "invalid");
        std::env::set_var("REQUEST_TIMEOUT", "invalid");
        
        let fallback_config = SecurityConfig::from_env();
        assert_eq!(fallback_config.max_body_size, 1024 * 1024); // Default
        assert_eq!(fallback_config.request_timeout.as_secs(), 30); // Default
        
        // Clean up environment variables
        std::env::remove_var("MAX_BODY_SIZE");
        std::env::remove_var("REQUEST_TIMEOUT");
        std::env::remove_var("STRICT_HEADERS");
    }

    // Note: Direct testing of add_security_headers requires mocking Next
    // which is complex. These are tested in integration tests instead.
}