use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use torpc_proxy_core::{Config, ProxyConfig, TorRpcProxy};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Configuration file path
    #[arg(short, long, default_value = "torpc-proxy.toml")]
    config: PathBuf,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy server
    Start {
        /// Port to listen on (overrides config)
        #[arg(short, long)]
        port: Option<u16>,

        /// Onion endpoint (overrides config)
        #[arg(short, long)]
        onion: Option<String>,
    },

    /// Show the default configuration
    Config,

    /// Test Tor connectivity
    Test,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("torpc_proxy=info")),
        )
        .init();

    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Start { port, onion }) => {
            start_proxy(cli.config, *port, onion.clone()).await
        }
        Some(Commands::Config) => {
            show_default_config();
            Ok(())
        }
        Some(Commands::Test) => test_tor_connectivity().await,
        None => {
            // Default to start if no subcommand
            start_proxy(cli.config, None, None).await
        }
    }
}

async fn start_proxy(
    config_path: PathBuf,
    port_override: Option<u16>,
    onion_override: Option<String>,
) -> Result<()> {
    // Load configuration
    let mut config = if config_path.exists() {
        Config::load_from_file(&config_path).context("Failed to load configuration")?
    } else {
        info!("No config file found, using defaults");
        Config::default()
    };

    // Apply command-line overrides
    if let Some(port) = port_override {
        config.port = port;
    }
    if let Some(onion) = onion_override {
        config.onion_endpoint = onion;
    }

    // Validate configuration
    if config.onion_endpoint.is_empty() {
        error!("No onion endpoint specified!");
        error!("Please provide --onion flag or set onion_endpoint in config file");
        std::process::exit(1);
    }

    // Create proxy configuration
    let proxy_config = ProxyConfig {
        listen_addr: ([127, 0, 0, 1], config.port).into(),
        tor_proxy: config.tor_proxy_addr(),
        onion_endpoint: config.onion_endpoint,
    };

    // Create and run proxy
    let proxy = TorRpcProxy::new(proxy_config);

    info!("Starting ToRPC proxy...");
    info!("Wallet RPC URL: http://localhost:{}", config.port);
    info!("Press Ctrl+C to stop");

    // Handle shutdown gracefully
    let proxy_handle = tokio::spawn(async move {
        if let Err(e) = proxy.run().await {
            error!("Proxy error: {}", e);
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .context("Failed to install signal handler")?;

    info!("Shutting down...");
    proxy_handle.abort();

    Ok(())
}

fn show_default_config() {
    println!("# ToRPC Proxy Configuration");
    println!();
    println!("# Port to listen on for wallet connections");
    println!("port = 8545");
    println!();
    println!("# Tor SOCKS5 proxy address");
    println!("tor_proxy_host = \"127.0.0.1\"");
    println!("tor_proxy_port = 9050");
    println!();
    println!("# Target .onion RPC endpoint");
    println!("onion_endpoint = \"your-onion-address.onion:8545\"");
    println!();
    println!("# Logging level (trace, debug, info, warn, error)");
    println!("log_level = \"info\"");
}

async fn test_tor_connectivity() -> Result<()> {
    use std::time::Duration;
    use tokio::time::timeout;
    use tokio_socks::tcp::Socks5Stream;

    info!("Testing Tor connectivity...");

    // Step 1: confirm Tor itself is reachable by hitting a well-known onion.
    // DuckDuckGo's onion is stable and tolerates a single TCP probe.
    let tor_addr: SocketAddr = ([127, 0, 0, 1], 9050).into();
    info!("Using Tor SOCKS5 proxy at: {}", tor_addr);

    let well_known = "duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion:80";
    info!("Step 1: probing Tor reachability via {}", well_known);
    let probe = Socks5Stream::connect(tor_addr, well_known);
    match timeout(Duration::from_secs(30), probe).await {
        Ok(Ok(_)) => info!("✓ Tor is reachable; SOCKS5 working"),
        Ok(Err(e)) => {
            error!("✗ Tor SOCKS5 reach test failed: {}", e);
            error!("Make sure Tor is running and listening on port 9050");
            return Err(e.into());
        }
        Err(_) => {
            error!("✗ Tor reachability probe timed out after 30s");
            return Err(anyhow::anyhow!("Tor reachability probe timed out"));
        }
    }

    // Step 2: also probe the user's *configured* onion endpoint, since that's
    // the one their wallet will actually use. Tor itself working doesn't
    // imply the configured onion is up, and that's the more common failure.
    let config_path = PathBuf::from("torpc-proxy.toml");
    let configured = if config_path.exists() {
        match Config::load_from_file(&config_path) {
            Ok(c) if !c.onion_endpoint.is_empty() => Some(c.onion_endpoint),
            _ => None,
        }
    } else {
        None
    };

    if let Some(onion) = configured {
        info!("Step 2: probing configured onion {}", onion);
        let probe = Socks5Stream::connect(tor_addr, onion.as_str());
        match timeout(Duration::from_secs(30), probe).await {
            Ok(Ok(_)) => info!("✓ Configured onion endpoint is reachable"),
            Ok(Err(e)) => {
                error!("✗ Could not reach configured onion {}: {}", onion, e);
                error!("Confirm the .onion address is correct and the remote service is up");
                return Err(e.into());
            }
            Err(_) => {
                error!("✗ Configured-onion probe timed out after 30s");
                return Err(anyhow::anyhow!("Configured-onion probe timed out"));
            }
        }
    } else {
        info!("Step 2 skipped: no `onion_endpoint` configured in torpc-proxy.toml");
    }

    Ok(())
}
