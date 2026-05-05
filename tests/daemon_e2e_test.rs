//! Daemon-level end-to-end tests.
//!
//! These tests drive the *exact* router that `main.rs` serves, via
//! `torpc::app::build_app`, against a mockito-backed Geth. They catch
//! integration bugs the layer-specific tests can't: layer ordering
//! mistakes in `main.rs`, route registration regressions, missing CSP
//! on some response paths.
//!
//! No external services required — runs in `make test` and CI.

use std::time::Duration;

use axum_test::TestServer;
use mockito::Server;
use serde_json::{json, Value};

use torpc::app::{build_app, AppConfig};
use torpc::rate_limit::RateLimitConfig;
use torpc::security::SecurityConfig;

/// `TestServer`s for both the public Tor-facing router and the localhost-only
/// admin router. They share the same `MevProxyState` (so `/metrics` reflects
/// counters incremented by `/rpc` traffic on the public side) but live on
/// separate routers in production.
struct DaemonServers {
    /// Public, Tor-facing: `/rpc`, `/rpc/flashbots`, static UI.
    public: TestServer,
    /// Localhost-only: `/health`, `/metrics`.
    admin: TestServer,
}

/// Build the real production routers via `build_app` against the given Geth
/// URL. `tweak` lets a test mutate the default `AppConfig` before `build_app`
/// runs (e.g. tighten the body limit, shorten the timeout).
async fn make_servers(geth_url: String, tweak: impl FnOnce(&mut AppConfig)) -> DaemonServers {
    let mut config = AppConfig::for_testing(geth_url);
    tweak(&mut config);
    let built = build_app(config).await.expect("build_app must succeed");
    DaemonServers {
        public: TestServer::new(built.app).expect("public router must accept TestServer"),
        admin: TestServer::new(built.admin_app).expect("admin router must accept TestServer"),
    }
}

/// Convenience wrapper for tests that only need the public router.
async fn make_server(geth_url: String, tweak: impl FnOnce(&mut AppConfig)) -> TestServer {
    make_servers(geth_url, tweak).await.public
}

/// Builds a mockito mock that REQUIRES at least one matching call. The
/// `match_body` predicate additionally validates the daemon sent a
/// well-formed `eth_blockNumber` JSON-RPC request — without this, a
/// mutation that proxied to upstream Geth but with a corrupted body would
/// still pass (mockito would 200 anything; the test would only catch
/// gross response-shape errors).
fn mock_geth_block_number(server: &mut mockito::ServerGuard, value: &str) -> mockito::Mock {
    server
        .mock("POST", "/")
        .match_header(
            "content-type",
            mockito::Matcher::Regex("application/json.*".into()),
        )
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"jsonrpc":"2.0","result":"{value}","id":1}}"#))
        .expect_at_least(1)
        .create()
}

// -----------------------------------------------------------------------------
// Routes
// -----------------------------------------------------------------------------

#[tokio::test]
async fn rpc_forwards_to_geth() {
    let mut geth = Server::new_async().await;
    let m = mock_geth_block_number(&mut geth, "0xabc");

    let server = make_server(geth.url(), |_| {}).await;
    let response = server
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
        .await;

    assert_eq!(response.status_code(), 200);
    let body: Value = response.json();
    assert_eq!(body["result"], "0xabc");
    // Strict: prove the daemon actually called mockito. A code path that
    // returned `"0xabc"` without forwarding would be caught by this.
    m.assert();
}

#[tokio::test]
async fn rpc_blocks_disallowed_methods() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = make_server(geth.url(), |_| {}).await;
    let response = server
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_accounts", "id": 1}))
        .await;
    assert_eq!(response.status_code(), 405);
}

#[tokio::test]
async fn flashbots_send_bundle_without_signing_key_returns_minus_32004() {
    // No FLASHBOTS_SIGNING_KEY → bundle path must error structurally rather
    // than fake a hash. Regression guard: an earlier version returned
    // `0x000…00` which made wallets wait forever on a non-existent bundle.
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = make_server(geth.url(), |_| {}).await;
    let response = server
        .post("/rpc/flashbots")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_sendBundle",
            "params": [{"txs": ["0x"], "blockNumber": "0x1"}],
            "id": 7,
        }))
        .await;

    let body: Value = response.json();
    assert_eq!(body["error"]["code"], -32004);
    assert_eq!(body["id"], 7);
}

#[tokio::test]
async fn health_reports_minimal_process_uptime_payload() {
    // After Phase-Option-C, `/health` is intentionally minimal — process
    // alive, version, uptime. Component-state observability moved to
    // `/metrics`. This test pins the new shape so a future re-introduction
    // of upstream-Geth probing doesn't sneak past CI.
    let mut geth = Server::new_async().await;
    // Mock is provided so the rest of the daemon initializes happily, but
    // /health does NOT call Geth — `expect(0)` enforces that.
    let m = geth
        .mock("POST", "/")
        .with_status(200)
        .with_body(r#"{"jsonrpc":"2.0","result":"0x1","id":1}"#)
        .expect(0)
        .create();

    // /health lives on the admin router after the Phase-2 split.
    let servers = make_servers(geth.url(), |_| {}).await;
    let response = servers.admin.get("/health").await;
    assert_eq!(response.status_code(), 200);
    let body: Value = response.json();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "torpc");
    assert!(body["uptime_seconds"].is_number());
    assert!(body["version"].is_string());
    // Component-state has moved out of /health.
    assert!(body.get("components").is_none());
    m.assert();
}

#[tokio::test]
async fn metrics_endpoint_exposes_live_counters() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    // Public router for the request, admin router for /metrics. They share
    // the same atomic counters so the count incremented on the public side
    // is visible immediately on the admin side.
    let servers = make_servers(geth.url(), |_| {}).await;
    let _ = servers
        .public
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_accounts", "id": 1}))
        .await;
    let body: Value = servers.admin.get("/metrics").await.json();
    assert_eq!(body["security_metrics"]["invalid_methods"], 1);
    assert_eq!(body["security_metrics"]["blocked_requests_total"], 1);
}

/// Regression guard for the Phase-2 admin/public split. `/health` and
/// `/metrics` must NOT be reachable through the Tor-facing router — that
/// would publish operator-side state (uptime, version, component circuit
/// state, blocked-request counters) to anonymous .onion visitors.
#[tokio::test]
async fn health_and_metrics_are_404_on_public_router() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let servers = make_servers(geth.url(), |_| {}).await;

    // Public side must 404 both endpoints.
    let h = servers.public.get("/health").await;
    assert_eq!(
        h.status_code(),
        404,
        "/health must not be reachable on the Tor-facing router"
    );
    let m = servers.public.get("/metrics").await;
    assert_eq!(
        m.status_code(),
        404,
        "/metrics must not be reachable on the Tor-facing router"
    );

    // Admin side serves both happily.
    assert_eq!(servers.admin.get("/health").await.status_code(), 200);
    assert_eq!(servers.admin.get("/metrics").await.status_code(), 200);
}

// -----------------------------------------------------------------------------
// Cross-cutting middleware: header presence on every response path.
// These specifically catch layer-ordering regressions in `build_app`.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn security_headers_present_on_success() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = make_server(geth.url(), |_| {}).await;
    let response = server
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
        .await;
    assert_eq!(response.header("x-content-type-options"), "nosniff");
    assert_eq!(response.header("x-frame-options"), "DENY");
    assert!(response
        .header("content-security-policy")
        .to_str()
        .unwrap()
        .contains("default-src 'self'"));
}

#[tokio::test]
async fn security_headers_present_on_blocked_method_response() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = make_server(geth.url(), |_| {}).await;
    let response = server
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_accounts", "id": 1}))
        .await;
    assert_eq!(response.status_code(), 405);
    assert_eq!(response.header("x-content-type-options"), "nosniff");
}

#[tokio::test]
async fn body_limit_response_keeps_headers() {
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = make_server(geth.url(), |c| c.security.max_body_size = 64).await;
    let response = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": ["x".repeat(2000)],
            "id": 1,
        }))
        .await;
    assert_eq!(response.status_code(), 413);
    assert_eq!(response.header("x-content-type-options"), "nosniff");
}

#[tokio::test]
async fn write_method_rate_limit_enforced_through_full_router() {
    // mockito returns a real bundle hash so successful sends actually
    // reach upstream — what we want to verify is that the limiter
    // still reigns them in after the configured budget.
    let mut geth = Server::new_async().await;
    let _m = geth
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","result":"0xdead","id":1}"#)
        .expect_at_least(2)
        .create();

    let server = make_server(geth.url(), |c| {
        c.write_method_limit_max = 2;
        c.write_method_limit_window = Duration::from_secs(60);
    })
    .await;

    let send = || async {
        server
            .post("/rpc")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_sendRawTransaction",
                "params": ["0xdeadbeef"],
                "id": 1,
            }))
            .await
    };

    assert_eq!(send().await.status_code(), 200);
    assert_eq!(send().await.status_code(), 200);
    assert_eq!(send().await.status_code(), 429);
}

#[tokio::test]
async fn jsonrpc_timeout_returns_minus_32001() {
    // mockito will sleep longer than the configured timeout; the daemon's
    // JSON-RPC timeout middleware must fire and return a structured
    // `-32001` body rather than the bare 408 `tower-http::TimeoutLayer`
    // produced before Phase 10.
    let mut geth = Server::new_async().await;
    let _m = geth
        .mock("POST", "/")
        .with_status(200)
        .with_chunked_body(|w| {
            std::thread::sleep(Duration::from_millis(500));
            w.write_all(b"{\"jsonrpc\":\"2.0\",\"result\":\"0x1\",\"id\":1}")?;
            Ok(())
        })
        .create();

    let server = make_server(geth.url(), |c| {
        c.security = SecurityConfig {
            request_timeout: Duration::from_millis(80),
            ..SecurityConfig::default()
        };
    })
    .await;

    let response = server
        .post("/rpc")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
        .await;
    assert_eq!(response.status_code(), 504);
    let body: Value = response.json();
    assert_eq!(body["error"]["code"], -32001);
    assert_eq!(body["error"]["message"], "upstream timeout");
}

#[tokio::test]
async fn per_source_rate_limiter_fires_through_full_router() {
    // TestServer hits the in-memory transport, so all requests share the
    // "unknown" identifier bucket — the test sets a tiny budget and
    // verifies the limiter rejects the third request through the full
    // production router stack (route_layer → handler).
    let mut geth = Server::new_async().await;
    let _m = mock_geth_block_number(&mut geth, "0x1");

    let server = make_server(geth.url(), |c| {
        c.rate_limit = RateLimitConfig {
            max_requests: 2,
            window_duration: Duration::from_secs(60),
        };
    })
    .await;

    let send = || async {
        server
            .post("/rpc")
            .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
            .await
    };
    assert_eq!(send().await.status_code(), 200);
    assert_eq!(send().await.status_code(), 200);
    assert_eq!(send().await.status_code(), 429);
}

#[tokio::test]
async fn from_env_defaults_match_documented_values() {
    // Belt-and-suspenders: AppConfig::from_env on a clean environment
    // should match what `.env.example` documents. A drift between code
    // defaults and example values is exactly the bug the env-var alias
    // pass was meant to prevent — keep a regression guard here.
    let prev: Vec<(_, _)> = [
        "GETH_URL",
        "FLASHBOTS_URL",
        "BIND_ADDR",
        "FLASHBOTS_SIGNING_KEY",
        "FLASHBOTS_RELAY_URL",
        "RATE_LIMIT_REQUESTS",
        "RATE_LIMIT_WINDOW",
        "MAX_CONCURRENT_CONNECTIONS",
        "WRITE_RATE_LIMIT_REQUESTS",
        "WRITE_RATE_LIMIT_WINDOW",
    ]
    .iter()
    .map(|k| (k.to_string(), std::env::var(k).ok()))
    .collect();
    for (k, _) in &prev {
        std::env::remove_var(k);
    }

    let cfg = AppConfig::from_env();
    assert_eq!(cfg.geth_url, "http://127.0.0.1:8545");
    assert_eq!(cfg.bind_addr, "127.0.0.1:8080");
    assert!(cfg.mev_signing_key.is_none());
    assert_eq!(cfg.rate_limit.max_requests, 100);
    assert_eq!(cfg.rate_limit.window_duration, Duration::from_secs(60));
    assert_eq!(cfg.max_concurrent, 256);

    // Restore caller env so other tests aren't affected.
    for (k, v) in prev {
        match v {
            Some(v) => std::env::set_var(&k, v),
            None => std::env::remove_var(&k),
        }
    }

    // Light type-check on the unused module to satisfy `unused_imports`.
    let _ = RateLimitConfig {
        max_requests: 1,
        window_duration: Duration::from_secs(1),
    };
}
