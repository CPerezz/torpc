//! ToRPC daemon entry point.
//!
//! Almost all logic lives in `torpc::app::build_app`. This file is just the
//! thin shell that parses env, builds the production router, binds the
//! socket, and serves with graceful shutdown. Keeping it small means
//! integration tests (`tests/daemon_e2e_test.rs`) can drive the same
//! `build_app` directly without re-implementing layer ordering.

use std::net::SocketAddr;

use anyhow::Context;
use tracing::info;
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

    info!("Starting TorPC proxy server");

    let config = AppConfig::from_env();
    let bind_addr = config.bind_addr.clone();
    let built = build_app(config).await?;

    // Tor advisory output (best-effort; never blocks startup).
    let tor_service = TorService::new();
    if let Err(e) = tor_service.check_configuration() {
        info!("Tor configuration issue: {}", e);
        info!("Run ./scripts/setup-tor.sh to configure Tor");
    } else {
        match tor_service.get_hostname() {
            Ok(Some(hostname)) => {
                info!("🧅 Tor hidden service available at: http://{}", hostname);
                info!("   RPC endpoint: http://{}/rpc", hostname);
                info!("   Flashbots endpoint: http://{}/rpc/flashbots", hostname);
            }
            Ok(None) => {
                info!("Tor is configured but not running yet");
                info!("Start Tor with: ./scripts/start-tor.sh");
            }
            Err(e) => info!("Could not read Tor hostname: {}", e),
        }
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
