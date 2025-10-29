use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::PathBuf;

/// NFS Mirror configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Global server configuration
    pub server: ServerConfig,
    /// Mount point configurations
    pub mounts: Vec<MountConfig>,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Listen IP address
    #[serde(default = "default_ip")]
    pub ip: IpAddr,
    /// Listen port
    #[serde(default = "default_port")]
    pub port: u16,
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Enable verbose output
    #[serde(default)]
    pub verbose: bool,
    /// Run in daemon mode
    #[serde(default)]
    pub daemon: bool,
    /// PID file path (for daemon mode)
    pub pid_file: Option<PathBuf>,
    /// Working directory
    pub work_dir: Option<PathBuf>,
    /// Maximum number of connections
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    /// Read timeout in seconds
    #[serde(default = "default_read_timeout")]
    pub read_timeout: u64,
    /// Write timeout in seconds
    #[serde(default = "default_write_timeout")]
    pub write_timeout: u64,
    /// Enable read-only mode
    #[serde(default)]
    pub read_only: bool,
    /// Comma-separated list of allowed client IP addresses
    pub allow_ips: Option<String>,
    /// Disable log colors
    #[serde(default)]
    pub no_color: bool,
}

/// Mount point configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfig {
    /// Local directory path to mirror
    pub source: PathBuf,
    /// Remote mount path (NFS export path)
    pub target: String,
    /// Enable read-only mode for this mount (overrides global setting)
    #[serde(default)]
    pub read_only: bool,
    /// Description for this mount point
    pub description: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            ip: default_ip(),
            port: default_port(),
            log_level: default_log_level(),
            verbose: false,
            daemon: false,
            pid_file: None,
            work_dir: None,
            max_connections: default_max_connections(),
            read_timeout: default_read_timeout(),
            write_timeout: default_write_timeout(),
            read_only: false,
            allow_ips: None,
            no_color: false,
        }
    }
}

// Default value functions
fn default_ip() -> IpAddr {
    "127.0.0.1".parse().unwrap()
}

fn default_port() -> u16 {
    11451
}

fn default_log_level() -> String {
    "error".to_string()
}

fn default_max_connections() -> usize {
    100
}

fn default_read_timeout() -> u64 {
    30
}

fn default_write_timeout() -> u64 {
    30
}

#[allow(unused)]
impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn to_file<P: AsRef<std::path::Path>>(
        &self,
        path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Create a default configuration file
    pub fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            mounts: vec![],
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate mounts
        if self.mounts.is_empty() {
            return Err("At least one mount point must be configured".to_string());
        }

        for (i, mount) in self.mounts.iter().enumerate() {
            if !mount.source.exists() {
                return Err(format!(
                    "Mount point {}: source directory '{}' does not exist",
                    i,
                    mount.source.display()
                ));
            }

            if !mount.source.is_dir() {
                return Err(format!(
                    "Mount point {}: source '{}' is not a directory",
                    i,
                    mount.source.display()
                ));
            }

            if mount.target.is_empty() {
                return Err(format!("Mount point {}: target path cannot be empty", i));
            }

            // Target path should start with /
            if !mount.target.starts_with('/') {
                return Err(format!(
                    "Mount point {}: target path '{}' must start with '/'",
                    i, mount.target
                ));
            }
        }

        // Check for duplicate target paths
        let mut target_paths = std::collections::HashSet::new();
        for (i, mount) in self.mounts.iter().enumerate() {
            if !target_paths.insert(&mount.target) {
                return Err(format!(
                    "Mount point {}: duplicate target path '{}'",
                    i, mount.target
                ));
            }
        }

        // Validate server port
        if self.server.port == 0 {
            return Err("Server port cannot be 0".to_string());
        }

        Ok(())
    }

    /// Get mount by target path
    pub fn get_mount_by_target(&self, target: &str) -> Option<&MountConfig> {
        self.mounts.iter().find(|m| m.target == target)
    }

    /// Get all mount targets
    pub fn get_mount_targets(&self) -> Vec<&str> {
        self.mounts.iter().map(|m| m.target.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.validate().is_err()); // No mounts configured
    }

    #[test]
    fn test_config_serialization() {
        let config = Config {
            server: ServerConfig {
                port: 11451,
                ..Default::default()
            },
            mounts: vec![MountConfig {
                source: PathBuf::from("/tmp/test"),
                target: "/test".to_string(),
                read_only: false,
                description: Some("Test mount".to_string()),
            }],
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.server.port, parsed.server.port);
        assert_eq!(config.mounts.len(), parsed.mounts.len());
    }
}
