//! ToRPC daemon entry point.
//!
//! Almost all logic lives in `torpc::app::build_app`. This file is just the
//! thin shell that parses env, builds the production router, binds the
//! socket, and serves with graceful shutdown. Keeping it small means
//! integration tests (`tests/daemon_e2e_test.rs`) can drive the same
//! `build_app` directly without re-implementing layer ordering.

use std::net::SocketAddr;

use anyhow::Context;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

use torpc::app::{build_app, AppConfig};
use torpc::tor::TorService;

/// Block on Ctrl+C and (on Unix) SIGTERM, returning when either fires.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("received SIGINT, shutting down"),
        _ = terminate => info!("received SIGTERM, shutting down"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Honour `RUST_LOG`. Previously the daemon ignored it and pinned to INFO.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // Load `.env` if present. Operators who used to do `cp .env.example .env`
    // and then `cargo run` previously had no env vars applied — the daemon
    // never sourced the file. `dotenvy` is a no-op when the file is absent,
    // so this is safe in systemd / docker setups that inject env directly.
    let _ = dotenvy::dotenv();

    info!("Starting TorPC proxy server");

    let config = AppConfig::from_env();
    let bind_addr = config.bind_addr.clone();
    let built = build_app(config).await?;

    // Tor configuration check. When `configs/torrc` is present, anonymity-
    // disabling flags now fail the daemon hard (set TORPC_ALLOW_NON_ANONYMOUS=1
    // to override for benchmarks/CI). When torrc is absent, this is a no-op.
    let tor_service = TorService::new();
    tor_service
        .check_configuration()
        .context("Tor configuration check failed; see error above for the offending line")?;
    match tor_service.get_hostname() {
        // The .onion hostname is the operator's public-facing identity. At
        // INFO level it shows up in journalctl / syslog / log shippers,
        // which makes accidental disclosure surprisingly easy. Keep it
        // behind RUST_LOG=debug; operators who want it can `cat
        // data/tor/torpc/hostname`.
        Ok(Some(_)) => debug!("Tor hidden service hostname resolved (cat data/tor/torpc/hostname to view)"),
        Ok(None) => info!("Tor is configured but not running yet"),
        Err(e) => warn!("Could not read Tor hostname: {}", e),
    }

    let addr: SocketAddr = bind_addr
        .parse()
        .with_context(|| format!("invalid BIND_ADDR: {}", bind_addr))?;
    info!("Server listening on {}", addr);
    info!("Access the web interface at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {}", addr))?;

    axum::serve(
        listener,
        built
            .app
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("server task failed")?;

    // Stop the rate-limiter cleanup task so the runtime exits cleanly.
    built.cleanup_task.abort();

    info!("Server stopped cleanly");
    Ok(())
}
