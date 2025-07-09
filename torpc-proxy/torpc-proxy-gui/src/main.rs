#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use anyhow::Result;
use std::sync::Arc;
use std::path::PathBuf;
use std::fs;
use tauri::{
    CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem,
};
use tokio::sync::Mutex;
use torpc_proxy_core::{ProxyConfig, ProxyController};
use tracing::{error, info};

struct AppState {
    proxy_controller: Arc<Mutex<ProxyController>>,
}

/// Get the configuration directory path
fn get_config_dir() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
        .join("torpc-proxy");
    
    // Create directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
    }
    
    Ok(config_dir)
}

/// Get the configuration file path
fn get_config_file_path() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("config.json"))
}

/// Load configuration from file
fn load_config() -> Result<ProxyConfig> {
    let config_path = get_config_file_path()?;
    
    if config_path.exists() {
        info!("Loading configuration from {:?}", config_path);
        let config_str = fs::read_to_string(&config_path)?;
        let config: ProxyConfig = serde_json::from_str(&config_str)?;
        Ok(config)
    } else {
        info!("No configuration file found, using defaults");
        // Return default config
        Ok(ProxyConfig {
            listen_addr: ([127, 0, 0, 1], 8545).into(),
            tor_proxy: ([127, 0, 0, 1], 9050).into(),
            onion_endpoint: "placeholder.onion:80".to_string(),
        })
    }
}

/// Save configuration to file
fn save_config(config: &ProxyConfig) -> Result<()> {
    let config_path = get_config_file_path()?;
    let config_str = serde_json::to_string_pretty(config)?;
    fs::write(&config_path, config_str)?;
    info!("Configuration saved to {:?}", config_path);
    Ok(())
}

#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let controller = state.proxy_controller.lock().await;
    let status = controller.get_status().await;
    Ok(format!("{status:?}"))
}

#[tauri::command]
async fn start_proxy(state: tauri::State<'_, AppState>) -> Result<(), String> {
    info!("start_proxy command called");
    let controller = state.proxy_controller.lock().await;
    match controller.start().await {
        Ok(_) => {
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
    
    // Update the configuration in the controller
    controller
        .update_config(config.clone())
        .await
        .map_err(|e| e.to_string())?;
    
    // Save the configuration to file
    if let Err(e) = save_config(&config) {
        error!("Failed to save configuration to file: {}", e);
        // Don't fail the command if we can't save the file, just log the error
    }
    
    Ok(())
}

#[tauri::command]
async fn test_connection(onion_endpoint: String, tor_proxy: String) -> Result<String, String> {
    use tokio_socks::tcp::Socks5Stream;
    use std::time::Duration;
    
    info!("test_connection called with endpoint: {} via proxy: {}", onion_endpoint, tor_proxy);
    
    // Parse tor proxy address
    let tor_proxy_addr: std::net::SocketAddr = tor_proxy
        .parse()
        .map_err(|e| format!("Invalid Tor proxy address: {}", e))?;
    
    // Try to connect through Tor
    let connect_future = Socks5Stream::connect(tor_proxy_addr, onion_endpoint.as_str());
    let timeout_future = tokio::time::timeout(Duration::from_secs(30), connect_future);
    
    match timeout_future.await {
        Ok(Ok(_stream)) => {
            info!("Connection test successful");
            Ok("Connection successful! The onion endpoint is reachable.".to_string())
        }
        Ok(Err(e)) => {
            error!("Connection test failed: {}", e);
            if e.to_string().contains("Connection refused") {
                Err("Connection refused: The onion service may not be running or the address is incorrect.".to_string())
            } else {
                Err(format!("Connection failed: {}", e))
            }
        }
        Err(_) => {
            error!("Connection test timeout");
            Err("Connection timeout: Unable to reach the onion endpoint within 30 seconds.".to_string())
        }
    }
}

fn create_system_tray() -> SystemTray {
    let start = CustomMenuItem::new("start".to_string(), "Start Proxy");
    let stop = CustomMenuItem::new("stop".to_string(), "Stop Proxy");
    let settings = CustomMenuItem::new("settings".to_string(), "Settings");
    let quit = CustomMenuItem::new("quit".to_string(), "Quit");

    let tray_menu = SystemTrayMenu::new()
        .add_item(start)
        .add_item(stop)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(settings)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);

    SystemTray::new().with_menu(tray_menu)
}

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "torpc_proxy=info".into()),
        )
        .init();

    // Load configuration from file or use defaults
    let config = match load_config() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            info!("Using default configuration");
            ProxyConfig {
                listen_addr: ([127, 0, 0, 1], 8545).into(),
                tor_proxy: ([127, 0, 0, 1], 9050).into(),
                onion_endpoint: "placeholder.onion:80".to_string(),
            }
        }
    };

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
        .system_tray(create_system_tray())
        .on_window_event(|event| match event.event() {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                // Prevent the window from closing
                api.prevent_close();
                
                // Hide the window instead
                let window = event.window();
                let _ = window.hide();
                
                info!("Window hidden to system tray");
            }
            _ => {}
        })
        .on_system_tray_event(move |app, event| {
            #[allow(clippy::single_match)]
            match event {
                SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                "start" => {
                    let handle = app.app_handle();
                    tauri::async_runtime::spawn(async move {
                        let state: tauri::State<AppState> = handle.state();
                        let controller = state.proxy_controller.lock().await;
                        if let Err(e) = controller.start().await {
                            error!("Failed to start proxy: {}", e);
                        }
                    });
                }
                "stop" => {
                    let handle = app.app_handle();
                    tauri::async_runtime::spawn(async move {
                        let state: tauri::State<AppState> = handle.state();
                        let controller = state.proxy_controller.lock().await;
                        if let Err(e) = controller.stop().await {
                            error!("Failed to stop proxy: {}", e);
                        }
                    });
                }
                "settings" => {
                    if let Some(window) = app.get_window("main") {
                        // Show the window
                        let _ = window.show();
                        let _ = window.set_focus();
                        let _ = window.unminimize();
                    }
                }
                "quit" => {
                    let handle = app.app_handle();
                    tauri::async_runtime::spawn(async move {
                        let state: tauri::State<AppState> = handle.state();
                        let controller = state.proxy_controller.lock().await;
                        let _ = controller.stop().await;
                        std::process::exit(0);
                    });
                }
                _ => {}
            },
            _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}