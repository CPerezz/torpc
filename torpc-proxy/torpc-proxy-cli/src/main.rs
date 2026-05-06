use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

use torpc_proxy_core::{Config, ProxyConfig, TorRpcProxy};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Configuration file path. When unspecified, the CLI looks at
    /// `dirs::config_dir() / torpc-proxy / config.toml` first (shared with
    /// the GUI), then falls back to `./torpc-proxy.toml` for backward
    /// compatibility with cwd-based setups.
    #[arg(short, long)]
    config: Option<PathBuf>,
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

        /// Tor SOCKS5 host (overrides config; default 127.0.0.1)
        #[arg(long)]
        tor_host: Option<String>,

        /// Tor SOCKS5 port (overrides config; default 9050 — Tor Browser uses 9150)
        #[arg(long)]
        tor_port: Option<u16>,
    },

    /// Show the default configuration
    Config,

    /// Test Tor connectivity. Probes a well-known onion to confirm Tor is
    /// reachable, then probes the configured `onion_endpoint` if present.
    Test {
        /// Tor SOCKS5 host (overrides config; default 127.0.0.1)
        #[arg(long)]
        tor_host: Option<String>,

        /// Tor SOCKS5 port (overrides config; default 9050)
        #[arg(long)]
        tor_port: Option<u16>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("torpc_proxy=info")),
        )
        .init();

    let cli = Cli::parse();
    let config_path = resolve_config_path(cli.config.clone());

    match cli.command {
        Some(Commands::Start {
            port,
            onion,
            tor_host,
            tor_port,
        }) => start_proxy(config_path, port, onion, tor_host, tor_port).await,
        Some(Commands::Config) => {
            show_default_config();
            Ok(())
        }
        Some(Commands::Test { tor_host, tor_port }) => {
            test_tor_connectivity(config_path, tor_host, tor_port).await
        }
        None => start_proxy(config_path, None, None, None, None).await,
    }
}

/// Resolve which `Config` file the CLI should read.
///
/// Priority:
/// 1. `--config` flag (explicit user choice; honour even if missing).
/// 2. `dirs::config_dir() / torpc-proxy / config.toml` (shared with the GUI).
/// 3. `./torpc-proxy.toml` (cwd; backward-compat with operator setups that
///    pre-date the user-config path).
///
/// The returned path may not exist on disk — `start_proxy` falls back to
/// `Config::default()` in that case.
fn resolve_config_path(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(user) = dirs::config_dir().map(|d| d.join("torpc-proxy/config.toml")) {
        if user.exists() {
            debug!("Using user-config path {:?}", user);
            return user;
        }
    }
    PathBuf::from("torpc-proxy.toml")
}

async fn start_proxy(
    config_path: PathBuf,
    port_override: Option<u16>,
    onion_override: Option<String>,
    tor_host_override: Option<String>,
    tor_port_override: Option<u16>,
) -> Result<()> {
    let mut config = if config_path.exists() {
        Config::load_from_file(&config_path).context("Failed to load configuration")?
    } else {
        info!("No config file found, using defaults");
        Config::default()
    };

    if let Some(port) = port_override {
        config.port = port;
    }
    if let Some(onion) = onion_override {
        config.onion_endpoint = onion;
    }
    if let Some(host) = tor_host_override {
        config.tor_proxy_host = host;
    }
    if let Some(port) = tor_port_override {
        config.tor_proxy_port = port;
    }

    if config.onion_endpoint.is_empty() {
        error!("No onion endpoint specified!");
        error!("Provide --onion <addr.onion:port> or set onion_endpoint in the config file");
        std::process::exit(1);
    }

    let proxy_config = ProxyConfig {
        listen_addr: ([127, 0, 0, 1], config.port).into(),
        tor_proxy: config.tor_proxy_addr(),
        onion_endpoint: config.onion_endpoint,
    };

    let proxy = TorRpcProxy::new(proxy_config);

    info!("Starting ToRPC proxy...");
    info!("Wallet RPC URL: http://localhost:{}", config.port);
    info!("Press Ctrl+C to stop");

    let proxy_handle = tokio::spawn(async move {
        if let Err(e) = proxy.run().await {
            error!("Proxy error: {}", e);
        }
    });

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

/// Probe the local Tor daemon and (if configured) the user's onion endpoint.
///
/// `tor_host_override` / `tor_port_override` come from `--tor-host` /
/// `--tor-port`. Without overrides we fall back to the values in the
/// configured file (so a Tor Browser user with `tor_proxy_port = 9150`
/// gets the right address tested). Pre-PR-1 the test hardcoded 9050,
/// which produced confusing "Tor not running" errors for those users.
async fn test_tor_connectivity(
    config_path: PathBuf,
    tor_host_override: Option<String>,
    tor_port_override: Option<u16>,
) -> Result<()> {
    use std::time::Duration;
    use tokio::time::timeout;
    use tokio_socks::tcp::Socks5Stream;

    info!("Testing Tor connectivity...");

    let config = if config_path.exists() {
        Config::load_from_file(&config_path).unwrap_or_else(|e| {
            info!("Could not load {:?}: {}; using defaults", config_path, e);
            Config::default()
        })
    } else {
        Config::default()
    };

    let tor_host = tor_host_override.unwrap_or(config.tor_proxy_host.clone());
    let tor_port = tor_port_override.unwrap_or(config.tor_proxy_port);
    let tor_addr: SocketAddr = format!("{tor_host}:{tor_port}")
        .parse()
        .with_context(|| format!("Invalid Tor SOCKS5 address: {tor_host}:{tor_port}"))?;
    info!("Using Tor SOCKS5 proxy at: {}", tor_addr);

    // Step 1: confirm Tor itself is reachable by hitting a well-known onion.
    // DuckDuckGo's onion is stable and tolerates a single TCP probe.
    let well_known = "duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion:80";
    info!("Step 1: probing Tor reachability via {}", well_known);
    let probe = Socks5Stream::connect(tor_addr, well_known);
    match timeout(Duration::from_secs(30), probe).await {
        Ok(Ok(_)) => info!("✓ Tor is reachable; SOCKS5 working"),
        Ok(Err(e)) => {
            error!("✗ Tor SOCKS5 reach test failed: {}", e);
            error!(
                "Make sure Tor is running and listening on {} (Tor Browser uses 9150)",
                tor_addr
            );
            return Err(e.into());
        }
        Err(_) => {
            error!("✗ Tor reachability probe timed out after 30s");
            return Err(anyhow::anyhow!("Tor reachability probe timed out"));
        }
    }

    // Step 2: probe the configured onion (the more common failure mode).
    if !config.onion_endpoint.is_empty() {
        info!("Step 2: probing configured onion {}", config.onion_endpoint);
        let probe = Socks5Stream::connect(tor_addr, config.onion_endpoint.as_str());
        match timeout(Duration::from_secs(30), probe).await {
            Ok(Ok(_)) => info!("✓ Configured onion endpoint is reachable"),
            Ok(Err(e)) => {
                error!(
                    "✗ Could not reach configured onion {}: {}",
                    config.onion_endpoint, e
                );
                error!("Confirm the .onion address is correct and the remote service is up");
                return Err(e.into());
            }
            Err(_) => {
                error!("✗ Configured-onion probe timed out after 30s");
                return Err(anyhow::anyhow!("Configured-onion probe timed out"));
            }
        }
    } else {
        info!("Step 2 skipped: no `onion_endpoint` configured");
    }

    Ok(())
}
