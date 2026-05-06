//! Real-socket end-to-end tests.
//!
//! `axum_test::TestServer` uses an in-memory transport that does **not**
//! inject `ConnectInfo<SocketAddr>` into the request extensions. The
//! existing `daemon_e2e_test.rs` therefore can't actually verify the
//! per-source-port bucketing of the rate limiter — it can only verify
//! that the limiter fires when its single "unknown" bucket is exhausted.
//!
//! These tests fix that gap by binding the production router to a real
//! ephemeral TCP port via `axum::serve(.., into_make_service_with_connect_info)`
//! and driving it with `reqwest`. Two scenarios:
//!
//! 1. Fresh TCP connection per request (`Connection: close` + no pooling)
//!    → each request lands on a different source port → each gets its
//!    own rate-limit bucket → all succeed even under a tight `max=1` cap.
//! 2. Keep-alive single connection → same source port → same bucket →
//!    third request is rejected with `429`.
//!
//! Together they prove the `ConnectInfo<SocketAddr>` is actually wired
//! through `into_make_service_with_connect_info` and that the limiter
//! reads the source port (not the fallback `"unknown"` identifier).

use std::net::SocketAddr;
use std::time::Duration;

use mockito::Server;
use serde_json::json;
use tokio::net::TcpListener;

use torpc::app::{build_app, AppConfig};
use torpc::rate_limit::RateLimitConfig;

/// Boot the production router on an ephemeral port. Returns the bound
/// address (so the test can connect) and a `JoinHandle` the caller can
/// abort when done. Mirrors the exact wiring `main.rs` uses.
async fn boot_daemon(
    geth_url: String,
    tweak: impl FnOnce(&mut AppConfig),
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut config = AppConfig::for_testing(geth_url);
    tweak(&mut config);

    let built = build_app(config).await.expect("build_app must succeed");

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");

    let handle = tokio::spawn(async move {
        // `into_make_service_with_connect_info::<SocketAddr>` is what makes
        // `ConnectInfo<SocketAddr>` available to the rate-limit middleware.
        // Without it, every request shares the literal `"unknown"` bucket
        // and the per-source-port test below can never pass.
        let _ = axum::serve(
            listener,
            built
                .app
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    // Briefly wait for the listener to be accepting — `bind` already
    // succeeded so the OS kernel is listening, but yielding once gives
    // axum a chance to install its top-level service.
    tokio::time::sleep(Duration::from_millis(20)).await;

    (addr, handle)
}

fn block_number_mock(server: &mut mockito::ServerGuard) -> mockito::Mock {
    server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","result":"0x1","id":1}"#)
        // Tightest assertion mockito offers: any call (matched or not) is
        // counted, and `assert_async` verifies the expected count was hit.
        .expect_at_least(1)
        .create()
}

async fn post_block_number(client: &reqwest::Client, addr: SocketAddr) -> reqwest::Response {
    client
        .post(format!("http://{addr}/rpc"))
        .header("connection", "close")
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
        .send()
        .await
        .expect("daemon must accept connection")
}

/// Each request opens a brand-new TCP connection (and therefore lands on a
/// new ephemeral source port), so each falls into a *different* rate-limit
/// bucket. With `max=1`, all requests succeed — the per-port limiter is
/// independent of the global wall-clock budget.
///
/// This test FAILS if `ConnectInfo` isn't wired (the daemon would fall back
/// to the literal `"unknown"` identifier, every request shares one bucket,
/// and the second request would be rejected).
#[tokio::test]
async fn fresh_connections_get_independent_buckets() {
    let mut geth = Server::new_async().await;
    let m = block_number_mock(&mut geth);

    let (addr, handle) = boot_daemon(geth.url(), |c| {
        c.rate_limit = RateLimitConfig {
            max_requests: 1,
            window_duration: Duration::from_secs(60),
        };
    })
    .await;

    // No connection pool → every request opens a fresh socket from a
    // distinct ephemeral source port.
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .expect("reqwest client");

    for i in 0..5 {
        let resp = post_block_number(&client, addr).await;
        assert_eq!(
            resp.status(),
            200,
            "iteration {} should succeed under per-port limiter; status was {}",
            i,
            resp.status()
        );
    }

    m.assert_async().await;
    handle.abort();
}

/// All requests reuse a single TCP connection, so every request lands on
/// the *same* source port. With `max=2`, the third request must be
/// rejected — proving the limiter is keyed on the source port (not on
/// the request itself, the URL, or anything else).
#[tokio::test]
async fn same_connection_shares_bucket_and_trips_limit() {
    let mut geth = Server::new_async().await;
    let _m = block_number_mock(&mut geth);

    let (addr, handle) = boot_daemon(geth.url(), |c| {
        c.rate_limit = RateLimitConfig {
            max_requests: 2,
            window_duration: Duration::from_secs(60),
        };
    })
    .await;

    // Default keep-alive pool. Sequential requests reuse the same TCP
    // connection (same source port) for the duration of the test.
    let client = reqwest::Client::new();
    let url = format!("http://{addr}/rpc");

    let r1 = client
        .post(&url)
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200);

    let r2 = client
        .post(&url)
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 200);

    let r3 = client
        .post(&url)
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 3}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        r3.status(),
        429,
        "third request on the same TCP connection (same source port) must trip the limit"
    );

    handle.abort();
}

/// Ancillary check: the listener actually listens, the daemon actually
/// returns the mocked block number end-to-end. Catches "did the daemon
/// even bind?" failures separately from the limiter-specific assertions
/// so debugging one isn't muddled by the other.
#[tokio::test]
async fn real_socket_round_trip_returns_mocked_block_number() {
    let mut geth = Server::new_async().await;
    let m = geth
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","result":"0x4242","id":1}"#)
        .expect_at_least(1)
        .create();

    let (addr, handle) = boot_daemon(geth.url(), |_| {}).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/rpc"))
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
        .send()
        .await
        .expect("real-socket round trip");
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["result"], "0x4242");

    m.assert_async().await;
    handle.abort();
}

/// Per-source bucketing must persist after the per-port cleanup task
/// would have run (cleanup interval is 5 minutes; we don't wait that
/// long, just verify the in-memory map keeps separate entries for two
/// distinct ports without leaking state between them).
#[tokio::test]
async fn two_clients_dont_share_state_across_their_lifetime() {
    let mut geth = Server::new_async().await;
    let _m = block_number_mock(&mut geth);

    let (addr, handle) = boot_daemon(geth.url(), |c| {
        c.rate_limit = RateLimitConfig {
            max_requests: 2,
            window_duration: Duration::from_secs(60),
        };
    })
    .await;

    // Two clients with default keep-alive pools — each opens its own
    // connection from its own source port.
    let client_a = reqwest::Client::new();
    let client_b = reqwest::Client::new();
    let url = format!("http://{addr}/rpc");

    // Exhaust client A's bucket.
    for _ in 0..2 {
        let r = client_a
            .post(&url)
            .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }
    let blocked = client_a
        .post(&url)
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status(), 429, "client A should be rate-limited");

    // Client B uses its own source port → still has a fresh budget.
    for _ in 0..2 {
        let r = client_b
            .post(&url)
            .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.status(),
            200,
            "client B is on a different source port and must NOT share client A's bucket"
        );
    }

    handle.abort();
}
