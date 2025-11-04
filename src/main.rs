mod error;
mod geth_client;
mod mev;
mod proxy;
mod rate_limit;
mod rpc_types;
mod security;
mod tor;
mod whitelist;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
    Router,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::mev::mev_handler::{handle_flashbots_with_mev, MevProxyState};
use crate::proxy::{handle_inbound, handle_rpc, InboundState, ProxyState};
use crate::rate_limit::{rate_limit_middleware, RateLimitConfig, RateLimiter};
use crate::security::{
    build_security_layers, health_check, monitor_request_patterns, security_headers_middleware,
    security_metrics, SecurityConfig,
};
use crate::tor::TorService;
use crate::{
    mev::mev_client_impl::{create_mev_client, MevConfig},
    tor::{Onion, OnionConfig},
};

#[tokio::main]
async fn main() {
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    info!("Starting TorPC proxy server");

    // Configuration
    // MOO: not GETH but EL client in general!
    let geth_url =
        std::env::var("GETH_URL").unwrap_or_else(|_| "http://127.0.0.1:8656".to_string()); // MOO: figure out default addr!
    let flashbots_url = std::env::var("FLASHBOTS_URL")
        .unwrap_or_else(|_| "https://relay.flashbots.net".to_string());
    let outbound_bind_addr =
        std::env::var("OUTBOUND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());

    let inbound_bind_addr =
        std::env::var("INBOUND_ADDR").unwrap_or_else(|_| "127.0.0.1:8545".to_string());

    // MOO: how we want to handle case with no onion peers?
    let onion_peers: Vec<_> = std::env::var("ONION_PEERS")
        .unwrap_or_else(|_| {
            "ethereumbbdyhyy33d4f3frmsxm6anm6bdrffvzft4kdk5p43odcvaid.onion:8545".to_string()
        })
        .split(',')
        .map(|s| s.to_string())
        .collect();

    info!("Geth URL: {}", geth_url);
    info!("Flashbots URL: {}", flashbots_url);
    info!("Outbound address: {}", outbound_bind_addr);
    info!("Inbound address: {}", inbound_bind_addr);

    // Create base proxy state
    let base_state = Arc::new(ProxyState::new(geth_url.clone(), flashbots_url.clone()));

    let inbound_state = InboundState {
        proxy_state: ProxyState::new(geth_url.clone(), flashbots_url.clone()),
        onion_peers: Onion::try_new(OnionConfig::default().with_peers(onion_peers)).unwrap(),
    };

    // Create MEV-aware state if signing key is configured
    let (mev_state, _has_mev) = if let Ok(signing_key) = std::env::var("FLASHBOTS_SIGNING_KEY") {
        // MEV protection is enabled
        let relay_url =
            std::env::var("FLASHBOTS_RELAY_URL").unwrap_or_else(|_| flashbots_url.clone());

        let mev_config = MevConfig {
            relay_url: relay_url.clone(),
            signing_key,
            request_timeout: Duration::from_secs(5),
            blocks_ahead: 1,
        };

        match create_mev_client(mev_config) {
            Ok(mev_client) => {
                info!("MEV protection enabled with relay: {}", relay_url);
                let mev_proxy_state = Arc::new(MevProxyState {
                    base_state: base_state.clone(),
                    mev_client: Some(mev_client),
                });
                (mev_proxy_state, true)
            }
            Err(e) => {
                error!("Failed to initialize MEV client: {}", e);
                info!("Falling back to standard proxy without MEV protection");
                let mev_proxy_state = Arc::new(MevProxyState {
                    base_state: base_state.clone(),
                    mev_client: None,
                });
                (mev_proxy_state, false)
            }
        }
    } else {
        info!("MEV protection not configured (set FLASHBOTS_SIGNING_KEY to enable)");
        let mev_proxy_state = Arc::new(MevProxyState {
            base_state: base_state.clone(),
            mev_client: None,
        });
        (mev_proxy_state, false)
    };

    // Create rate limiter
    let rate_limit_config = RateLimitConfig {
        max_requests: 100,
        window_duration: Duration::from_secs(60), // 100 requests per minute
    };
    let rate_limiter = Arc::new(RateLimiter::new(rate_limit_config));

    // Spawn cleanup task for rate limiter
    let cleanup_limiter = rate_limiter.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300)); // Cleanup every 5 minutes
        loop {
            interval.tick().await;
            cleanup_limiter.cleanup().await;
        }
    });

    // Load security configuration
    let security_config = SecurityConfig::from_env();
    info!(
        "Security config: max_body_size={}KB, timeout={}s, strict_headers={}",
        security_config.max_body_size / 1024,
        security_config.request_timeout.as_secs(),
        security_config.strict_headers
    );

    // accepts connections from TOR network
    let outbound_server = Router::new()
        // Health and monitoring endpoints (no rate limiting)
        .route("/health", get(health_check))
        .route("/metrics", get(security_metrics))
        // RPC endpoints with rate limiting
        .route("/", post({
            move |axum::extract::State(s): axum::extract::State<Arc<MevProxyState>>, req| async move {
                handle_rpc(axum::extract::State(s.base_state.clone()), req).await
            }
        }))
        .route("/flashbots", post(handle_flashbots_with_mev))
        .route("/flashbots/", post(handle_flashbots_with_mev))
        .route_layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit_middleware,
        ))
        // Static file serving (no rate limiting)
        // Add MEV state
        .with_state(mev_state)
        // Add request body limit
        .layer(DefaultBodyLimit::max(security_config.max_body_size))
        // Add request pattern monitoring
        .layer(middleware::from_fn(monitor_request_patterns))
        // Add security layers (timeouts)
        .layer(build_security_layers(security_config.clone()))
        // Add security headers middleware
        .layer(middleware::from_fn(security_headers_middleware))
        // Add tracing
        .layer(TraceLayer::new_for_http());

    // accepts connections from local node and forwards them into TOR network
    let inbound_server = Router::new()
        // RPC endpoints with rate limiting
        .route("/", post({
            move |axum::extract::State(s): axum::extract::State<Arc<InboundState>>, req| async move {
                handle_inbound(axum::extract::State(s), req).await
            }
        }))
        .route_layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit_middleware,
        ))
        // Add inbound
        .with_state(inbound_state.into())
        // Add request body limit
        .layer(DefaultBodyLimit::max(security_config.max_body_size))
        // Add request pattern monitoring
        .layer(middleware::from_fn(monitor_request_patterns))
        // Add security layers (timeouts)
        .layer(build_security_layers(security_config.clone()))
        // Add security headers middleware
        .layer(middleware::from_fn(security_headers_middleware))
        // Add tracing
        .layer(TraceLayer::new_for_http());

    // Parse bind address
    let outbound_addr: SocketAddr = outbound_bind_addr.parse().expect("Invalid bind address");
    let inbound_addr: SocketAddr = inbound_bind_addr.parse().expect("Invalid bind address");

    info!("Server listening on {}", outbound_addr);
    info!("Server listening on {}", inbound_addr);

    // Check Tor status
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
            Err(e) => {
                info!("Could not read Tor hostname: {}", e);
            }
        }
    }

    // Run server
    let outbound_listener = tokio::net::TcpListener::bind(outbound_addr)
        .await
        .expect("Failed to bind to address");

    let inbound_listener = tokio::net::TcpListener::bind(inbound_addr)
        .await
        .expect("Failed to bind to address");

    // Spawn tasks to serve each application concurrently
    let res = tokio::join!(
        axum::serve(outbound_listener, outbound_server),
        axum::serve(inbound_listener, inbound_server)
    );

    match res {
        (Ok(_), Ok(_)) => {}
        (Err(e1), Ok(_)) => {
            println!("{}", e1)
        }
        (Ok(_), Err(e2)) => {
            println!("{}", e2)
        }
        (Err(e1), Err(e2)) => {
            println!("{}, {}", e1, e2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum_test::TestServer;
    use serde_json::json;

    async fn create_test_app(server_url: String) -> Router {
        let base_state = Arc::new(ProxyState::new(
            server_url.clone(),
            format!("{}/flashbots", server_url),
        ));

        let mev_state = Arc::new(MevProxyState {
            base_state: base_state.clone(),
            mev_client: None,
        });

        Router::new()
            .route(
                "/rpc",
                post({
                    let base_state = base_state.clone();
                    move |axum::extract::State(_): axum::extract::State<Arc<MevProxyState>>, req| {
                        let state = base_state.clone();
                        async move { handle_rpc(axum::extract::State(state), req).await }
                    }
                }),
            )
            .route("/rpc/flashbots", post(handle_flashbots_with_mev))
            .with_state(mev_state)
    }

    #[tokio::test]
    async fn test_server_routes() {
        let mock_server = mockito::Server::new_async().await;
        let app = create_test_app(mock_server.url()).await;
        let server = TestServer::new(app).unwrap();

        // Test RPC endpoint exists
        let response = server
            .post("/rpc")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "invalid_method",
                "id": 1
            }))
            .await;

        assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);

        // Test Flashbots endpoint exists
        let response = server
            .post("/rpc/flashbots")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "invalid_method",
                "id": 1
            }))
            .await;

        assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
