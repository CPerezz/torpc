//! Production daemon assembly.
//!
//! `build_app(AppConfig)` constructs the exact `axum::Router` the binary
//! serves, plus the rate-limiter and its cleanup task. Extracting it into
//! the library lets integration tests drive the *same* router `main.rs`
//! installs — including the layer ordering, the JSON-RPC timeout middleware,
//! the per-method rate limit, and the dynamic CSP header. Without this,
//! every test had to rebuild a private router and a layer-ordering
//! regression in `main.rs` could ship undetected.
//!
//! See `tests/daemon_e2e_test.rs` for end-to-end coverage that uses this.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
    Router,
};
use tower::limit::ConcurrencyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info};

use crate::mev::mev_handler::{handle_flashbots_with_mev, MevProxyState};
use crate::mev::{create_mev_client, MevConfig};
use crate::proxy::{self, handle_rpc, ProxyState};
use crate::rate_limit::{rate_limit_middleware, RateLimitConfig, RateLimiter};
use crate::security::{
    config_js, health_check, json_rpc_timeout_middleware, monitor_request_patterns,
    security_headers_middleware, security_metrics, RuntimeWebConfig, SecurityConfig,
};

/// All operator-configurable knobs the daemon needs at startup. Construct
/// via `from_env()` for production or by literal in tests.
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub geth_url: String,
    pub flashbots_url: String,
    pub bind_addr: String,

    /// Hex-encoded private key used to sign Flashbots auth headers. `None`
    /// disables MEV protection (bundles return a JSON-RPC error rather than
    /// being silently faked).
    pub mev_signing_key: Option<String>,
    pub mev_relay_url: String,
    pub mev_request_timeout: Duration,

    pub rate_limit: RateLimitConfig,
    pub write_method_limit_max: u32,
    pub write_method_limit_window: Duration,
    pub max_concurrent: usize,

    pub security: SecurityConfig,
    pub web: RuntimeWebConfig,

    /// Path to the static directory served at `/`. Tests override to skip
    /// the wallet-helper UI; production uses `static/`.
    pub static_dir: String,
}

impl AppConfig {
    /// Read every operator-tunable field from environment variables,
    /// falling back to the documented defaults. Mirrors what `main.rs`
    /// previously did inline.
    pub fn from_env() -> Self {
        let geth_url =
            std::env::var("GETH_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        let flashbots_url = std::env::var("FLASHBOTS_URL")
            .unwrap_or_else(|_| "https://relay.flashbots.net".to_string());
        let bind_addr =
            std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());

        let mev_signing_key = std::env::var("FLASHBOTS_SIGNING_KEY").ok();
        let mev_relay_url = std::env::var("FLASHBOTS_RELAY_URL")
            .unwrap_or_else(|_| flashbots_url.clone());
        let mev_request_timeout = Duration::from_secs(env_u64("FLASHBOTS_REQUEST_TIMEOUT", 5));

        let rate_limit = RateLimitConfig {
            max_requests: env_u64("RATE_LIMIT_REQUESTS", 100) as u32,
            window_duration: Duration::from_secs(env_u64("RATE_LIMIT_WINDOW", 60)),
        };
        let write_method_limit_max = env_u64(
            "WRITE_RATE_LIMIT_REQUESTS",
            proxy::WRITE_METHOD_DEFAULT_REQUESTS as u64,
        ) as u32;
        let write_method_limit_window = Duration::from_secs(env_u64(
            "WRITE_RATE_LIMIT_WINDOW",
            proxy::WRITE_METHOD_DEFAULT_WINDOW_SECS,
        ));
        let max_concurrent = env_u64("MAX_CONCURRENT_CONNECTIONS", 256) as usize;

        Self {
            geth_url,
            flashbots_url,
            bind_addr,
            mev_signing_key,
            mev_relay_url,
            mev_request_timeout,
            rate_limit,
            write_method_limit_max,
            write_method_limit_window,
            max_concurrent,
            security: SecurityConfig::from_env(),
            web: RuntimeWebConfig::from_env(),
            static_dir: "static".to_string(),
        }
    }

    /// Test-friendly defaults — you'll typically override `geth_url` to
    /// point at a mockito server. `bind_addr` is set to an ephemeral port
    /// so several tests can run in parallel without colliding.
    pub fn for_testing(geth_url: String) -> Self {
        Self {
            geth_url,
            flashbots_url: "http://127.0.0.1:1".to_string(),
            bind_addr: "127.0.0.1:0".to_string(),
            mev_signing_key: None,
            mev_relay_url: "http://127.0.0.1:1".to_string(),
            mev_request_timeout: Duration::from_secs(2),
            rate_limit: RateLimitConfig {
                max_requests: 1_000,
                window_duration: Duration::from_secs(60),
            },
            write_method_limit_max: 100,
            write_method_limit_window: Duration::from_secs(60),
            max_concurrent: 32,
            security: SecurityConfig::default(),
            web: RuntimeWebConfig {
                discovery_url: "http://localhost:8081/api/discovery".to_string(),
                discovery_timeout_ms: 2000,
                fallback_rpc_url: "http://localhost:8545".to_string(),
            },
            // Tests typically don't need the wallet-helper UI; point at a
            // path that exists (the same dir is fine).
            static_dir: "static".to_string(),
        }
    }
}

/// What `build_app` returns. Holding the cleanup `JoinHandle` lets callers
/// abort the rate-limiter cleanup task on shutdown — the task otherwise
/// runs until the runtime drops.
pub struct BuiltApp {
    pub app: Router,
    pub cleanup_task: tokio::task::JoinHandle<()>,
}

/// Construct the production daemon router from `AppConfig`. This is the
/// single source of truth for layer ordering and route registration —
/// `main.rs` and integration tests both go through it.
pub async fn build_app(config: AppConfig) -> anyhow::Result<BuiltApp> {
    info!("Geth URL: {}", config.geth_url);
    info!("Flashbots URL: {}", config.flashbots_url);
    info!("Bind address: {}", config.bind_addr);
    info!(
        "Rate limit: {} req per {}s window",
        config.rate_limit.max_requests,
        config.rate_limit.window_duration.as_secs()
    );
    info!(
        "Write-method rate limit: {} req per {}s window (per method)",
        config.write_method_limit_max,
        config.write_method_limit_window.as_secs()
    );
    info!("Max concurrent connections: {}", config.max_concurrent);
    info!(
        "Security config: max_body_size={}KB, timeout={}s, strict_headers={}",
        config.security.max_body_size / 1024,
        config.security.request_timeout.as_secs(),
        config.security.strict_headers
    );
    info!(
        "Runtime web config: discovery_url={}, fallback_rpc_url={}",
        config.web.discovery_url, config.web.fallback_rpc_url
    );

    // ----- ProxyState -------------------------------------------------------
    let base_state = Arc::new(
        ProxyState::new_with_write_limit(
            config.geth_url.clone(),
            config.flashbots_url.clone(),
            config.write_method_limit_max,
            config.write_method_limit_window,
        )
        .context("failed to construct ProxyState")?,
    );

    // ----- MEV state --------------------------------------------------------
    let mev_state = if let Some(signing_key) = config.mev_signing_key.clone() {
        let mev_config = MevConfig {
            relay_url: config.mev_relay_url.clone(),
            signing_key,
            request_timeout: config.mev_request_timeout,
            blocks_ahead: 1,
        };
        match create_mev_client(mev_config) {
            Ok(client) => {
                info!("MEV protection enabled with relay: {}", config.mev_relay_url);
                Arc::new(MevProxyState {
                    base_state: base_state.clone(),
                    mev_client: Some(client),
                })
            }
            Err(e) => {
                error!("Failed to initialize MEV client: {}", e);
                info!("Falling back to standard proxy without MEV protection");
                Arc::new(MevProxyState {
                    base_state: base_state.clone(),
                    mev_client: None,
                })
            }
        }
    } else {
        info!("MEV protection not configured (set FLASHBOTS_SIGNING_KEY to enable)");
        Arc::new(MevProxyState {
            base_state: base_state.clone(),
            mev_client: None,
        })
    };

    // ----- Per-port rate limiter + cleanup task ---------------------------
    let rate_limiter = Arc::new(RateLimiter::new(config.rate_limit.clone()));
    let cleanup_limiter = rate_limiter.clone();
    let cleanup_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            cleanup_limiter.cleanup().await;
        }
    });

    // ----- Web/runtime config -> CSP + /config.js -------------------------
    let web_config = Arc::new(config.web.clone());
    let csp_header = axum::http::HeaderValue::from_str(&web_config.build_csp())
        .context("CSP header value contained invalid bytes")?;

    // ----- Router assembly --------------------------------------------------
    let config_router = Router::new()
        .route("/config.js", get(config_js))
        .with_state(web_config.clone());

    let app = Router::new()
        .merge(config_router)
        .route("/health", get(health_check))
        .route("/metrics", get(security_metrics))
        .route(
            "/rpc",
            post({
                move |axum::extract::State(s): axum::extract::State<Arc<MevProxyState>>, req| async move {
                    handle_rpc(axum::extract::State(s.base_state.clone()), req).await
                }
            }),
        )
        .route(
            "/rpc/",
            post({
                move |axum::extract::State(s): axum::extract::State<Arc<MevProxyState>>, req| async move {
                    handle_rpc(axum::extract::State(s.base_state.clone()), req).await
                }
            }),
        )
        .route("/rpc/flashbots", post(handle_flashbots_with_mev))
        .route("/rpc/flashbots/", post(handle_flashbots_with_mev))
        .route_layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit_middleware,
        ))
        .nest_service("/", ServeDir::new(&config.static_dir))
        .with_state(mev_state)
        .layer(ConcurrencyLimitLayer::new(config.max_concurrent))
        .layer(DefaultBodyLimit::max(config.security.max_body_size))
        .layer(middleware::from_fn(monitor_request_patterns))
        .layer(middleware::from_fn_with_state(
            config.security.request_timeout,
            json_rpc_timeout_middleware,
        ))
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            csp_header,
        ))
        .layer(TraceLayer::new_for_http());

    Ok(BuiltApp { app, cleanup_task })
}

fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(raw) => raw.parse().unwrap_or_else(|_| {
            tracing::warn!(
                "{} is not a valid u64 (got {:?}); using default {}",
                name,
                raw,
                default
            );
            default
        }),
        Err(_) => default,
    }
}
