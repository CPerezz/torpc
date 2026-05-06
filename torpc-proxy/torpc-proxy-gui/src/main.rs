// Hide the console window on Windows release builds. Tauri 1.x and 2.x
// agree on this attribute.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

//! ToRPC Proxy desktop GUI.
//!
//! Caffeine-shaped tray app: a small icon in the menu bar, click to open a
//! settings window, close-button hides instead of quits. Wraps
//! `torpc_proxy_core::ProxyController` so the same forwarding logic backs
//! the CLI and the GUI.
//!
//! Migrated to Tauri 2.x. The 1.x `SystemTray` / `SystemTrayMenu` /
//! `SystemTrayEvent` triple is gone; tray-icons + menus are constructed
//! against an `AppHandle` inside `setup` using `TrayIconBuilder` and the
//! new `tauri::menu::*` types. Window APIs renamed
//! `Manager::get_window` → `Manager::get_webview_window`. Allowlist
//! removed, replaced by `capabilities/default.json`.

use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tokio::sync::Mutex;
use torpc_proxy_core::{ProxyConfig, ProxyController, ProxyStatus};
use tracing::{error, info};

struct AppState {
    proxy_controller: Arc<Mutex<ProxyController>>,
}

// -----------------------------------------------------------------------------
// Configuration persistence
//
// Pre-PR-1 the CLI read TOML and the GUI read JSON, so installing both binaries
// on the same machine would silently fork the configuration. They now agree on
// a single TOML file at `dirs::config_dir() / torpc-proxy / config.toml`.
// -----------------------------------------------------------------------------

fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
        .join("torpc-proxy");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn config_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

fn default_config() -> ProxyConfig {
    // Default `onion_endpoint` is **empty**, not the legacy "placeholder.onion:80"
    // sentinel. The settings UI uses an HTML `placeholder=` attr to suggest
    // the format; persisting a real-looking placeholder string used to make
    // it through `update_config` and trigger the proxy to chase a non-existent
    // hidden service.
    ProxyConfig {
        listen_addr: ([127, 0, 0, 1], 8545).into(),
        tor_proxy: ([127, 0, 0, 1], 9050).into(),
        onion_endpoint: String::new(),
    }
}

fn load_config() -> Result<ProxyConfig> {
    let path = config_file_path()?;
    if path.exists() {
        info!("Loading configuration from {:?}", path);
        let raw = fs::read_to_string(&path)?;
        Ok(toml::from_str(&raw)?)
    } else {
        info!("No configuration file found, using defaults");
        Ok(default_config())
    }
}

fn save_config(config: &ProxyConfig) -> Result<()> {
    let path = config_file_path()?;
    let raw = toml::to_string_pretty(config)?;
    fs::write(&path, raw)?;
    info!("Configuration saved to {:?}", path);
    Ok(())
}

// -----------------------------------------------------------------------------
// Tauri commands invoked from the settings window's JS
// -----------------------------------------------------------------------------

/// Returns the typed `ProxyStatus` (serializes to `{"state":"Running"}` etc.)
/// rather than `Debug`-formatted text. The frontend can pattern-match on
/// `status.state` without parsing strings.
#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<ProxyStatus, String> {
    let controller = state.proxy_controller.lock().await;
    Ok(controller.get_status().await)
}

#[tauri::command]
async fn start_proxy(state: tauri::State<'_, AppState>) -> Result<(), String> {
    info!("start_proxy command called");
    let controller = state.proxy_controller.lock().await;

    let config = controller.get_config().await;
    if config.onion_endpoint.trim().is_empty() {
        let msg = "No onion endpoint configured. Open Settings and enter the .onion address before starting the proxy.";
        error!("Refusing to start proxy: {}", msg);
        return Err(msg.to_string());
    }

    match controller.start().await {
        Ok(()) => {
            info!("Proxy started successfully");
            Ok(())
        }
        Err(e) => {
            error!("Failed to start proxy: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
async fn stop_proxy(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let controller = state.proxy_controller.lock().await;
    controller.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_config(state: tauri::State<'_, AppState>) -> Result<ProxyConfig, String> {
    let controller = state.proxy_controller.lock().await;
    Ok(controller.get_config().await)
}

#[tauri::command]
async fn update_config(
    state: tauri::State<'_, AppState>,
    config: ProxyConfig,
) -> Result<(), String> {
    let controller = state.proxy_controller.lock().await;

    controller
        .update_config(config.clone())
        .await
        .map_err(|e| e.to_string())?;

    if let Err(e) = save_config(&config) {
        // Don't fail the command if we can't persist — the in-memory
        // controller is already updated. The next restart will fall back
        // to defaults, which is recoverable.
        error!("Failed to save configuration to file: {}", e);
    }

    Ok(())
}

/// Two-stage connection probe. Stage 1 establishes a SOCKS5 connection
/// through the local Tor daemon to the configured onion endpoint. Stage 2
/// sends a `web3_clientVersion` JSON-RPC and verifies the response is a
/// well-formed JSON-RPC envelope — without stage 2, "connection successful"
/// passes against any TCP listener happening to live on the configured
/// onion port. The HTTP request is built by hand to avoid pulling hyper
/// in just for the probe.
#[tauri::command]
async fn test_connection(onion_endpoint: String, tor_proxy: String) -> Result<String, String> {
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_socks::tcp::Socks5Stream;

    info!(
        "test_connection called with endpoint: {} via proxy: {}",
        onion_endpoint, tor_proxy
    );

    let tor_proxy_addr: std::net::SocketAddr = tor_proxy
        .parse()
        .map_err(|e| format!("Invalid Tor proxy address: {e}"))?;

    // Stage 1: SOCKS5 connect (15s budget — Tor circuit setup is sometimes slow).
    let connect = Socks5Stream::connect(tor_proxy_addr, onion_endpoint.as_str());
    let mut stream = match tokio::time::timeout(Duration::from_secs(15), connect).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            let s = e.to_string();
            // Distinguish "Tor not reachable" from "onion offline" so the
            // user knows which knob to turn.
            return Err(if s.contains("Connection refused") {
                "Tor SOCKS proxy refused the connection. Is Tor running on this address?"
                    .to_string()
            } else if s.contains("ttl expired") || s.contains("network unreachable") {
                "Onion service unreachable (rendezvous failed). The .onion may be offline."
                    .to_string()
            } else {
                format!("SOCKS5 connect failed: {s}")
            });
        }
        Err(_) => {
            return Err(
                "Connection timeout: SOCKS5 connect didn't complete within 15s.".to_string(),
            );
        }
    };

    // Stage 2: minimal JSON-RPC handshake. We don't need to handle chunked
    // encoding or other HTTP nuances — TorPC's daemon speaks
    // Content-Length-framed responses to JSON-RPC POSTs.
    let body = r#"{"jsonrpc":"2.0","method":"web3_clientVersion","params":[],"id":1}"#;
    let req = format!(
        "POST / HTTP/1.1\r\n\
         Host: {host}\r\n\
         User-Agent: torpc-proxy-gui/{ver}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        host = onion_endpoint,
        ver = env!("CARGO_PKG_VERSION"),
        len = body.len(),
        body = body
    );

    if let Err(e) =
        tokio::time::timeout(Duration::from_secs(15), stream.write_all(req.as_bytes())).await
    {
        return Err(format!("Failed to send probe request (timeout: {e})"));
    }

    let mut buf = Vec::with_capacity(2048);
    let read_result =
        tokio::time::timeout(Duration::from_secs(15), stream.read_to_end(&mut buf)).await;
    match read_result {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(format!("Probe read failed: {e}")),
        Err(_) => return Err("Probe timed out waiting for a response.".to_string()),
    }

    // Parse: status line + headers + body. Cheap manual split is enough
    // for HTTP/1.1 with `Connection: close`.
    let response = String::from_utf8_lossy(&buf);
    let (head, _, payload) = match response.find("\r\n\r\n") {
        Some(i) => {
            let head = &response[..i];
            let payload = &response[i + 4..];
            (head, "\r\n\r\n", payload)
        }
        None => return Err("Onion endpoint returned a non-HTTP response.".to_string()),
    };

    let status = head.lines().next().unwrap_or("");
    if !(status.contains(" 2") || status.contains(" 3")) {
        return Err(format!("Onion endpoint replied: {status}"));
    }

    if !(payload.contains("\"jsonrpc\"") && payload.contains("\"2.0\"")) {
        return Err("Endpoint reachable but didn't return a JSON-RPC envelope. \
             This may not be a TorPC daemon."
            .to_string());
    }

    Ok("Connection successful: reached a TorPC daemon and got a valid JSON-RPC reply.".to_string())
}

// -----------------------------------------------------------------------------
// Tray icon construction (Tauri 2.x)
//
// In Tauri 1.x the tray was a top-level builder method. In 2.x we build it
// from inside `setup` because we need an `AppHandle` to construct menu
// items and attach event handlers.
// -----------------------------------------------------------------------------

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let start = MenuItem::with_id(app, "start", "Start Proxy", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop Proxy", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(app, &[&start, &stop, &sep1, &settings, &sep2, &quit])?;

    TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .icon(app.default_window_icon().cloned().unwrap())
        .icon_as_template(true)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
                "start" => spawn_controller(app, |c| async move {
                    let controller = c.lock().await;
                    if let Err(e) = controller.start().await {
                        error!("Failed to start proxy from tray: {}", e);
                    }
                }),
                "stop" => spawn_controller(app, |c| async move {
                    let controller = c.lock().await;
                    if let Err(e) = controller.stop().await {
                        error!("Failed to stop proxy from tray: {}", e);
                    }
                }),
                "settings" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                        let _ = window.unminimize();
                    }
                }
                "quit" => {
                    spawn_controller(app, |c| async move {
                        let controller = c.lock().await;
                        let _ = controller.stop().await;
                        std::process::exit(0);
                    });
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

/// Helper to run a closure that needs the `ProxyController` from app state.
/// Avoids the State extraction + lock dance being repeated in every menu
/// arm above.
fn spawn_controller<F, Fut>(app: &tauri::AppHandle, f: F)
where
    F: FnOnce(Arc<Mutex<ProxyController>>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state: tauri::State<AppState> = app.state();
        let controller = state.proxy_controller.clone();
        f(controller).await;
    });
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "torpc_proxy=info".into()),
        )
        .init();

    // The GUI is a trusted local client of the discovery API; opt it in.
    // The token persists at `${XDG_RUNTIME_DIR:-/tmp}/torpc-discovery.token`
    // (mode 0600). PR 3 will read it from there if/when we add an embedded
    // wallet view that needs the discovery endpoint.
    if std::env::var_os("TORPC_DISCOVERY_ENABLE").is_none() {
        std::env::set_var("TORPC_DISCOVERY_ENABLE", "true");
    }

    let config = load_config().unwrap_or_else(|e| {
        error!("Failed to load configuration: {}; using defaults", e);
        default_config()
    });

    let proxy_controller = Arc::new(Mutex::new(ProxyController::new(config)));
    let app_state = AppState {
        proxy_controller: proxy_controller.clone(),
    };

    info!("Starting ToRPC Proxy GUI...");

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_proxy,
            stop_proxy,
            get_config,
            update_config,
            test_connection
        ])
        .setup(|app| {
            build_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Caffeine semantics: pressing the close button hides the
            // window into the tray, doesn't quit the app. Right-click the
            // tray → Quit terminates.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                info!("Window hidden to system tray");
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
