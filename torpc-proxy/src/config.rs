use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::SocketAddr;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Port to listen on for wallet connections
    #[serde(default = "default_port")]
    pub port: u16,

    /// Tor SOCKS5 proxy host
    #[serde(default = "default_tor_host")]
    pub tor_proxy_host: String,

    /// Tor SOCKS5 proxy port
    #[serde(default = "default_tor_port")]
    pub tor_proxy_port: u16,

    /// Target .onion RPC endpoint (e.g., "abc123.onion:8545")
    #[serde(default)]
    pub onion_endpoint: String,

    /// Logging level
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path).context("Failed to read configuration file")?;

        toml::from_str(&contents).context("Failed to parse configuration file")
    }

    /// Save configuration to a TOML file
    #[allow(dead_code)]
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let contents = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        fs::write(path, contents).context("Failed to write configuration file")?;

        Ok(())
    }

    /// Get the Tor proxy address as SocketAddr
    pub fn tor_proxy_addr(&self) -> SocketAddr {
        format!("{}:{}", self.tor_proxy_host, self.tor_proxy_port)
            .parse()
            .unwrap_or_else(|_| ([127, 0, 0, 1], 9050).into())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: default_port(),
            tor_proxy_host: default_tor_host(),
            tor_proxy_port: default_tor_port(),
            onion_endpoint: String::new(),
            log_level: default_log_level(),
        }
    }
}

fn default_port() -> u16 {
    8545
}

fn default_tor_host() -> String {
    "127.0.0.1".to_string()
}

fn default_tor_port() -> u16 {
    9050
}

fn default_log_level() -> String {
    "info".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.port, 8545);
        assert_eq!(config.tor_proxy_host, "127.0.0.1");
        assert_eq!(config.tor_proxy_port, 9050);
        assert_eq!(config.onion_endpoint, "");
        assert_eq!(config.log_level, "info");
    }

    #[test]
    fn test_save_and_load_config() {
        let config = Config {
            port: 8546,
            tor_proxy_host: "localhost".to_string(),
            tor_proxy_port: 9051,
            onion_endpoint: "test.onion:8545".to_string(),
            log_level: "debug".to_string(),
        };

        let temp_file = NamedTempFile::new().unwrap();
        config.save_to_file(temp_file.path()).unwrap();

        let loaded = Config::load_from_file(temp_file.path()).unwrap();
        assert_eq!(loaded.port, config.port);
        assert_eq!(loaded.tor_proxy_host, config.tor_proxy_host);
        assert_eq!(loaded.tor_proxy_port, config.tor_proxy_port);
        assert_eq!(loaded.onion_endpoint, config.onion_endpoint);
        assert_eq!(loaded.log_level, config.log_level);
    }

    #[test]
    fn test_tor_proxy_addr() {
        let config = Config {
            tor_proxy_host: "127.0.0.1".to_string(),
            tor_proxy_port: 9050,
            ..Default::default()
        };

        let addr = config.tor_proxy_addr();
        assert_eq!(addr, ([127, 0, 0, 1], 9050).into());
    }
}
