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
    
    /// Check if Tor is properly configured
    pub fn check_configuration(&self) -> Result<()> {
        // Check if torrc exists
        if !Path::new(&self.config_path).exists() {
            anyhow::bail!("Tor configuration file not found at: {}", self.config_path);
        }
        
        // Check if data directory exists
        let data_dir = Path::new("./data/tor/torpc");
        if !data_dir.exists() {
            warn!("Tor data directory doesn't exist, creating it...");
            fs::create_dir_all(data_dir)
                .context("Failed to create Tor data directory")?;
            
            // Set proper permissions (700)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = fs::Permissions::from_mode(0o700);
                fs::set_permissions(data_dir, permissions)
                    .context("Failed to set Tor directory permissions")?;
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
    fn test_tor_service_new() {
        let tor = TorService::new();
        assert_eq!(tor.hostname_path, "./data/tor/torpc/hostname");
        assert_eq!(tor.config_path, "./configs/torrc");
    }
    
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