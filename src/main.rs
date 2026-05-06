//! ToRPC daemon entry point.
//!
//! Almost all logic lives in `torpc::app::build_app`. This file is the
//! thin shell that parses env, builds the production routers, binds two
//! sockets (public Tor-facing + admin localhost), and serves both with a
//! shared graceful shutdown. Keeping it small means integration tests
//! (`tests/daemon_e2e_test.rs`) can drive the same `build_app` directly
//! without re-implementing layer ordering.

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
    let admin_bind_addr = config.admin_bind_addr.clone();
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
        Ok(Some(_)) => {
            debug!("Tor hidden service hostname resolved (cat data/tor/torpc/hostname to view)")
        }
        Ok(None) => info!("Tor is configured but not running yet"),
        Err(e) => warn!("Could not read Tor hostname: {}", e),
    }

    let public_addr: SocketAddr = bind_addr
        .parse()
        .with_context(|| format!("invalid BIND_ADDR: {bind_addr}"))?;
    let admin_addr: SocketAddr = admin_bind_addr
        .parse()
        .with_context(|| format!("invalid ADMIN_BIND_ADDR: {admin_bind_addr}"))?;

    // Defense in depth: refuse to serve admin endpoints on a non-loopback
    // address. Operators wanting remote scrape should reverse-proxy
    // through their own auth or use an SSH tunnel.
    if !admin_addr.ip().is_loopback() {
        anyhow::bail!(
            "ADMIN_BIND_ADDR ({admin_addr}) must be a loopback address; \
             /health and /metrics expose operator state and are not safe \
             on a public interface"
        );
    }

    let public_listener = tokio::net::TcpListener::bind(public_addr)
        .await
        .with_context(|| format!("failed to bind {public_addr}"))?;
    let admin_listener = tokio::net::TcpListener::bind(admin_addr)
        .await
        .with_context(|| format!("failed to bind {admin_addr}"))?;

    info!("Public server listening on {} (Tor-facing)", public_addr);
    info!(
        "Admin server listening on  {} (localhost-only: /health, /metrics)",
        admin_addr
    );
    info!("Access the web interface at http://{}", public_addr);

    // Both serve loops watch the same shutdown channel. When SIGINT/SIGTERM
    // fires, we send once and both loops complete their graceful shutdown.
    // tokio::sync::broadcast is enough for this two-receiver case and avoids
    // pulling in `tokio-util::sync::CancellationToken` for a single use.
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    let mut public_rx = shutdown_tx.subscribe();
    let public_app = built.app;
    let public_handle = tokio::spawn(async move {
        axum::serve(
            public_listener,
            public_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = public_rx.recv().await;
        })
        .await
    });

    let mut admin_rx = shutdown_tx.subscribe();
    let admin_app = built.admin_app;
    let admin_handle = tokio::spawn(async move {
        axum::serve(admin_listener, admin_app.into_make_service())
            .with_graceful_shutdown(async move {
                let _ = admin_rx.recv().await;
            })
            .await
    });

    shutdown_signal().await;
    let _ = shutdown_tx.send(());

    // Stop the rate-limiter cleanup task so the runtime exits cleanly.
    built.cleanup_task.abort();

    let (public_res, admin_res) = tokio::join!(public_handle, admin_handle);
    public_res.context("public server task panicked")??;
    admin_res.context("admin server task panicked")??;

    info!("Server stopped cleanly");
    Ok(())
}
