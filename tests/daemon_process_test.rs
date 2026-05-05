//! Subprocess-level end-to-end tests.
//!
//! Spawns the real `torpc` binary (built automatically by Cargo before
//! integration tests run, and located via `env!("CARGO_BIN_EXE_torpc")`)
//! against a mockito-backed Geth, drives it over real HTTP, then sends
//! `SIGTERM` and verifies the daemon shuts down gracefully with exit
//! code 0.
//!
//! Plugs the gap that no in-process test can plug: env-var parsing in
//! `main.rs`, the `bind→serve→shutdown_signal` glue, the production
//! tracing-subscriber, and the actual graceful-shutdown path. If any of
//! these regress, the subprocess test fails before any deploy does.
//!
//! Unix-only: SIGTERM doesn't exist on Windows.

#![cfg(unix)]

use std::process::Stdio;
use std::time::{Duration, Instant};

use mockito::Server;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use serde_json::json;
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::{sleep, timeout};

/// Reserve an ephemeral port by binding briefly. There's a small race
/// window between drop and the daemon's own bind, but on a single test
/// machine collisions are vanishingly rare. If we ever see flakes here
/// the right fix is to retry the whole spawn rather than to widen the
/// reserved range.
async fn pick_free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to ephemeral port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Poll the daemon's bind port until it accepts connections, or timeout.
async fn wait_for_listener(addr: &str, max_wait: Duration) -> bool {
    let deadline = Instant::now() + max_wait;
    while Instant::now() < deadline {
        if TcpStream::connect(addr).await.is_ok() {
            return true;
        }
        sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Drain a `tokio::process::ChildStderr` into an `Arc<Mutex<Vec<u8>>>`
/// so the child never blocks on stderr backpressure. Returned buffer
/// can be inspected after the child exits — useful for diagnosing
/// flakes when the daemon panics on some env-var parse.
fn spawn_stderr_drain(
    mut stderr: tokio::process::ChildStderr,
) -> std::sync::Arc<tokio::sync::Mutex<Vec<u8>>> {
    let buf = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let cloned = buf.clone();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut local = Vec::new();
        let _ = stderr.read_to_end(&mut local).await;
        *cloned.lock().await = local;
    });
    buf
}

/// Happy-path subprocess test: the binary starts, accepts an RPC call,
/// forwards it to mockito, and exits cleanly on SIGTERM.
#[tokio::test]
async fn daemon_subprocess_handles_request_and_shuts_down_on_sigterm() {
    let mut geth = Server::new_async().await;
    let m = geth
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","result":"0xfeedface","id":1}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    let port = pick_free_port().await;
    let bind_addr = format!("127.0.0.1:{}", port);

    let mut child = Command::new(env!("CARGO_BIN_EXE_torpc"))
        .env("BIND_ADDR", &bind_addr)
        .env("GETH_URL", geth.url())
        // Point Flashbots at an unreachable address so we don't accidentally
        // hit the real internet during tests.
        .env("FLASHBOTS_URL", "http://127.0.0.1:1")
        .env("RUST_LOG", "warn")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn torpc binary");

    let pid = child.id().expect("daemon must have a pid") as i32;
    let stderr_buf = spawn_stderr_drain(child.stderr.take().expect("piped stderr"));

    // 1. Wait for the bind socket. If this fails, dump captured stderr
    //    so the contributor can see why startup failed.
    let ready = wait_for_listener(&bind_addr, Duration::from_secs(15)).await;
    if !ready {
        let _ = child.kill().await;
        let stderr = stderr_buf.lock().await;
        panic!(
            "daemon did not bind {} within 15s; stderr was:\n{}",
            bind_addr,
            String::from_utf8_lossy(&stderr)
        );
    }

    // 2. Real HTTP request through the actual daemon.
    let url = format!("http://{}/rpc", bind_addr);
    let response = reqwest::Client::new()
        .post(&url)
        .json(&json!({"jsonrpc": "2.0", "method": "eth_blockNumber", "id": 1}))
        .send()
        .await
        .expect("daemon must respond");
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["result"], "0xfeedface");

    // 3. Send SIGTERM. With graceful shutdown wired, the daemon waits for
    //    in-flight requests to drain and then returns from main() with
    //    Ok(()). 10s is generous; in practice it's <100ms.
    kill(Pid::from_raw(pid), Signal::SIGTERM).expect("send SIGTERM");
    let status = match timeout(Duration::from_secs(10), child.wait()).await {
        Ok(result) => result.expect("child.wait must succeed"),
        Err(_) => {
            let captured = stderr_buf.lock().await.clone();
            panic!(
                "daemon did not exit within 10s of SIGTERM; stderr:\n{}",
                String::from_utf8_lossy(&captured)
            );
        }
    };
    assert!(
        status.success(),
        "daemon exited non-zero: {:?}; stderr:\n{}",
        status,
        String::from_utf8_lossy(&stderr_buf.lock().await)
    );

    // 4. Mockito must have actually been called — proves we're not just
    //    bouncing canned responses out of the daemon's own state.
    m.assert_async().await;
}

/// Negative test: an invalid `BIND_ADDR` env var must cause the daemon
/// to exit with a non-zero status. Verifies the `?`-propagation path
/// (Phase-3 fix that replaced `expect("Invalid bind address")`).
#[tokio::test]
async fn daemon_subprocess_exits_non_zero_on_bad_bind_addr() {
    let child = Command::new(env!("CARGO_BIN_EXE_torpc"))
        .env("BIND_ADDR", "this is not a socket address")
        .env("GETH_URL", "http://127.0.0.1:1")
        .env("FLASHBOTS_URL", "http://127.0.0.1:1")
        .env("RUST_LOG", "error")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn torpc");

    let status = timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .expect("daemon must exit promptly on invalid config")
        .expect("wait_with_output must succeed");
    assert!(
        !status.status.success(),
        "daemon should have exited non-zero on bad BIND_ADDR; got {:?}",
        status.status
    );
}

/// Negative test: a `BIND_ADDR` whose port is already in use must also
/// cause a non-zero exit. Covers the bind-error path that's separate
/// from the parse-error path above.
#[tokio::test]
async fn daemon_subprocess_exits_non_zero_when_port_already_bound() {
    // Hold the port for the lifetime of the test.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let bind_addr = format!("127.0.0.1:{}", port);

    let child = Command::new(env!("CARGO_BIN_EXE_torpc"))
        .env("BIND_ADDR", &bind_addr)
        .env("GETH_URL", "http://127.0.0.1:1")
        .env("FLASHBOTS_URL", "http://127.0.0.1:1")
        .env("RUST_LOG", "error")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn torpc");

    let status = timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .expect("daemon must exit promptly when bind fails")
        .expect("wait_with_output must succeed");
    assert!(
        !status.status.success(),
        "daemon should have exited non-zero when port {} was busy; got {:?}",
        port,
        status.status
    );

    drop(listener);
}
