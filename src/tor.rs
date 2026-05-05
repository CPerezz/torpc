use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use tracing::{info, warn};

/// Tor service configuration and utilities
pub struct TorService {
    pub hostname_path: String,
    pub config_path: String,
}

impl TorService {
    pub fn new() -> Self {
        Self {
            hostname_path: "./data/tor/torpc/hostname".to_string(),
            config_path: "./configs/torrc".to_string(),
        }
    }
    
    /// Check if Tor is properly configured.
    ///
    /// In addition to verifying the torrc file and data directory, this
    /// parses the torrc and refuses to start if either
    /// `HiddenServiceSingleHopMode 1` or `HiddenServiceNonAnonymousMode 1`
    /// is enabled — those flags effectively disable Tor's anonymity
    /// guarantees and are silent footguns for an operator who copy-pasted
    /// the wrong example. Set `TORPC_ALLOW_NON_ANONYMOUS=1` to override
    /// (intended for benchmarks/CI only).
    ///
    /// Permissions on the hidden-service data directory are re-tightened to
    /// 0700 on every startup, not only on first creation, so a misbehaving
    /// administrator that did `chmod 755 data/tor/torpc/` can't accidentally
    /// expose the service key to other users.
    pub fn check_configuration(&self) -> Result<()> {
        // No torrc → operator is running torpc without a hidden service
        // (e.g. for local dev or behind their own reverse proxy). Skip the
        // anonymity audit and the data-dir setup; it would be both useless
        // and intrusive (creating ./data/tor/torpc out of thin air).
        if !Path::new(&self.config_path).exists() {
            info!(
                "torrc not found at {}; skipping Tor configuration check",
                self.config_path
            );
            return Ok(());
        }

        // Refuse to start if torrc disables anonymity, unless explicitly
        // overridden. Comments (`#`) are ignored.
        let torrc = fs::read_to_string(&self.config_path)
            .context("Failed to read torrc for anonymity check")?;
        let allow_override = std::env::var("TORPC_ALLOW_NON_ANONYMOUS").as_deref() == Ok("1");
        for (idx, raw) in torrc.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut tokens = line.split_whitespace();
            let key = tokens.next().unwrap_or("");
            let val = tokens.next().unwrap_or("");
            let is_anonymity_disabling = matches!(
                (key, val),
                ("HiddenServiceSingleHopMode", "1")
                    | ("HiddenServiceNonAnonymousMode", "1")
            );
            if is_anonymity_disabling {
                if allow_override {
                    warn!(
                        "torrc line {}: '{}' disables Tor anonymity; \
                         continuing because TORPC_ALLOW_NON_ANONYMOUS=1",
                        idx + 1,
                        line
                    );
                } else {
                    anyhow::bail!(
                        "torrc line {}: '{}' disables Tor anonymity. \
                         Set TORPC_ALLOW_NON_ANONYMOUS=1 if this is intentional \
                         (e.g. for benchmarking).",
                        idx + 1,
                        line
                    );
                }
            }
        }

        // Ensure data directory exists with strict permissions.
        let data_dir = Path::new("./data/tor/torpc");
        if !data_dir.exists() {
            warn!("Tor data directory doesn't exist, creating it...");
            fs::create_dir_all(data_dir)
                .context("Failed to create Tor data directory")?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = fs::Permissions::from_mode(0o700);
            fs::set_permissions(data_dir, permissions)
                .context("Failed to set Tor directory permissions to 0700")?;
            // Verify the bits actually stuck — some filesystems silently
            // ignore mode changes (e.g. SMB mounts).
            let metadata = fs::metadata(data_dir)
                .context("Failed to read Tor data directory metadata")?;
            let mode = metadata.permissions().mode() & 0o777;
            if mode != 0o700 {
                anyhow::bail!(
                    "Tor data directory at {} has mode {:o}, expected 0700; \
                     refusing to start (the filesystem may not honour permissions)",
                    data_dir.display(),
                    mode
                );
            }
        }

        info!("Tor configuration verified");
        Ok(())
    }
    
    /// Get the .onion hostname if available
    pub fn get_hostname(&self) -> Result<Option<String>> {
        let hostname_path = Path::new(&self.hostname_path);
        
        if !hostname_path.exists() {
            info!("Hostname file not found - Tor service may not be running yet");
            return Ok(None);
        }
        
        let hostname = fs::read_to_string(hostname_path)
            .context("Failed to read hostname file")?
            .trim()
            .to_string();
            
        if hostname.is_empty() {
            return Ok(None);
        }
        
        // Validate it looks like a .onion address
        if !hostname.ends_with(".onion") {
            anyhow::bail!("Invalid hostname format: {}", hostname);
        }
        
        Ok(Some(hostname))
    }
    
    /// Get the full onion URL for a given path
    pub fn get_onion_url(&self, path: &str) -> Result<Option<String>> {
        match self.get_hostname()? {
            Some(hostname) => {
                let url = format!("http://{}{}", hostname, path);
                Ok(Some(url))
            }
            None => Ok(None),
        }
    }
    
    /// Check if Tor appears to be running by looking for the hostname file
    pub fn is_running(&self) -> bool {
        Path::new(&self.hostname_path).exists()
    }
}

impl Default for TorService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;
    
    #[test]
    fn test_get_hostname_missing_file() {
        let tor = TorService {
            hostname_path: "/nonexistent/path/hostname".to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        let result = tor.get_hostname().unwrap();
        assert!(result.is_none());
    }
    
    #[test]
    fn test_get_hostname_valid() {
        let temp_dir = TempDir::new().unwrap();
        let hostname_path = temp_dir.path().join("hostname");
        
        fs::write(&hostname_path, "test3xamplee2onion.onion\n").unwrap();
        
        let tor = TorService {
            hostname_path: hostname_path.to_str().unwrap().to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        let result = tor.get_hostname().unwrap();
        assert_eq!(result, Some("test3xamplee2onion.onion".to_string()));
    }
    
    #[test]
    fn test_get_hostname_invalid_format() {
        let temp_dir = TempDir::new().unwrap();
        let hostname_path = temp_dir.path().join("hostname");
        
        fs::write(&hostname_path, "not-an-onion-address").unwrap();
        
        let tor = TorService {
            hostname_path: hostname_path.to_str().unwrap().to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        let result = tor.get_hostname();
        assert!(result.is_err());
    }
    
    #[test]
    fn test_get_onion_url() {
        let temp_dir = TempDir::new().unwrap();
        let hostname_path = temp_dir.path().join("hostname");
        
        fs::write(&hostname_path, "test3xamplee2onion.onion\n").unwrap();
        
        let tor = TorService {
            hostname_path: hostname_path.to_str().unwrap().to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        let url = tor.get_onion_url("/rpc").unwrap();
        assert_eq!(url, Some("http://test3xamplee2onion.onion/rpc".to_string()));
    }
    
    #[test]
    fn test_is_running() {
        let temp_dir = TempDir::new().unwrap();
        let hostname_path = temp_dir.path().join("hostname");
        
        let tor = TorService {
            hostname_path: hostname_path.to_str().unwrap().to_string(),
            config_path: "./configs/torrc".to_string(),
        };
        
        // Not running initially
        assert!(!tor.is_running());
        
        // Create hostname file
        fs::write(&hostname_path, "test.onion").unwrap();
        
        // Should appear as running
        assert!(tor.is_running());
    }
}