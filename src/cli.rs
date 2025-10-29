use clap::Parser;
use std::net::IpAddr;
use std::path::PathBuf;
use tracing::info;

use crate::config::{Config, MountConfig, ServerConfig};

/// NFS Mirror - Mirror local directories into an NFS shared service
#[derive(Parser)]
#[command(name = "nfs_mirror")]
#[command(about = "Mirror local directories into an NFS shared service.")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "Benign X <1341398182@qq.com>")]
pub struct Cli {
    /// Configuration file path (TOML format)
    #[arg(
        short = 'c',
        long = "config",
        help = "Configuration file path (TOML format)"
    )]
    pub config: Option<PathBuf>,

    /// Local directory path to mirror (for single directory mode)
    #[arg(help = "Local directory path to mirror (use with --target for single directory mode)")]
    pub directory: Option<PathBuf>,

    /// Target mount path (for single directory mode)
    #[arg(
        short = 't',
        long = "target",
        help = "Target mount path (for single directory mode)"
    )]
    pub target: Option<String>,

    /// Listen IP address
    #[arg(
        short = 'i',
        long = "ip",
        default_value = "127.0.0.1",
        help = "Listen IP address"
    )]
    pub ip: IpAddr,

    /// Listen port
    #[arg(
        short = 'p',
        long = "port",
        default_value = "11451",
        help = "Listen port"
    )]
    pub port: u16,

    /// Log level (trace, debug, info, warn, error)
    #[arg(
        short = 'l',
        long = "log-level",
        default_value = "error",
        value_parser = ["trace", "debug", "info", "warn", "error"],
        help = "Log level"
    )]
    pub log_level: String,

    /// Enable verbose output
    #[arg(short = 'v', long = "verbose", help = "Enable verbose output")]
    pub verbose: bool,

    /// Daemon mode (run in background)
    #[arg(short = 'd', long = "daemon", help = "Run in daemon mode")]
    pub daemon: bool,

    /// PID file path (for daemon mode)
    #[arg(long = "pid-file", help = "PID file path")]
    pub pid_file: Option<PathBuf>,

    /// Working directory
    #[arg(long = "work-dir", help = "Working directory")]
    pub work_dir: Option<PathBuf>,

    /// Maximum number of connections
    #[arg(
        long = "max-connections",
        default_value = "100",
        help = "Maximum number of connections"
    )]
    pub max_connections: usize,

    /// Read timeout in seconds
    #[arg(
        long = "read-timeout",
        default_value = "30",
        help = "Read timeout in seconds"
    )]
    pub read_timeout: u64,

    /// Write timeout in seconds
    #[arg(
        long = "write-timeout",
        default_value = "30",
        help = "Write timeout in seconds"
    )]
    pub write_timeout: u64,

    /// Enable read-only mode
    #[arg(long = "read-only", help = "Enable read-only mode")]
    pub read_only: bool,

    /// Comma-separated list of allowed client IP addresses
    #[arg(
        long = "allow-ips",
        help = "Comma-separated list of allowed client IP addresses"
    )]
    pub allow_ips: Option<String>,

    /// Disable log colors
    #[arg(long = "no-color", help = "Disable log colors")]
    pub no_color: bool,

    /// Generate a sample configuration file
    #[arg(
        long = "generate-config",
        help = "Generate a sample configuration file and exit"
    )]
    pub generate_config: Option<PathBuf>,
}

impl Cli {
    /// Parse allowed IP addresses from the comma-separated string
    pub fn parse_allowed_ips(&self) -> Vec<IpAddr> {
        if let Some(ref ips_str) = self.allow_ips {
            ips_str
                .split(',')
                .filter_map(|s| s.trim().parse::<IpAddr>().ok())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    }

    /// Get the effective log level based on verbose flag and log-level setting
    pub fn get_log_level(&self) -> tracing::Level {
        if self.verbose {
            tracing::Level::DEBUG
        } else {
            match self.log_level.as_str() {
                "trace" => tracing::Level::TRACE,
                "debug" => tracing::Level::DEBUG,
                "info" => tracing::Level::INFO,
                "warn" => tracing::Level::WARN,
                "error" => tracing::Level::ERROR,
                _ => tracing::Level::ERROR,
            }
        }
    }

    /// Create configuration from CLI arguments
    pub fn to_config(&self) -> Result<Config, String> {
        // Check if we're in single directory mode
        let is_single_mode = self.directory.is_some();

        if is_single_mode {
            // Single directory mode
            let directory = self.directory.as_ref().ok_or("Directory is required")?;
            let target = self
                .target
                .as_ref()
                .ok_or("Target path is required for single directory mode")?;

            let mount = MountConfig {
                source: directory.clone(),
                target: target.clone(),
                read_only: self.read_only,
                description: Some(format!("Mount from {} to {}", directory.display(), target)),
            };

            Ok(Config {
                server: ServerConfig {
                    ip: self.ip,
                    port: self.port,
                    log_level: self.log_level.clone(),
                    verbose: self.verbose,
                    daemon: self.daemon,
                    pid_file: self.pid_file.clone(),
                    work_dir: self.work_dir.clone(),
                    max_connections: self.max_connections,
                    read_timeout: self.read_timeout,
                    write_timeout: self.write_timeout,
                    read_only: self.read_only,
                    allow_ips: self.allow_ips.clone(),
                    no_color: self.no_color,
                },
                mounts: vec![mount],
            })
        } else {
            // Config file mode
            Err("Config file mode not implemented yet".to_string())
        }
    }

    /// Load configuration from file or create from CLI arguments
    pub fn load_config(&self) -> Result<Config, String> {
        // If generate config is requested, create and save a sample config
        if let Some(ref config_path) = self.generate_config {
            let sample_config = Self::create_sample_config();
            sample_config.to_file(config_path).map_err(|e| {
                format!(
                    "Failed to write sample configuration to '{}': {}",
                    config_path.display(),
                    e
                )
            })?;
            info!(
                "Sample configuration file written to: {}",
                config_path.display()
            );
            std::process::exit(0);
        }

        // Load from config file if specified
        if let Some(ref config_path) = self.config {
            let mut config = Config::from_file(config_path).map_err(|e| {
                format!(
                    "Failed to load configuration from '{}': {}",
                    config_path.display(),
                    e
                )
            })?;

            // Override config file settings with CLI arguments
            self.override_config(&mut config);

            // Validate the configuration
            config.validate()?;
            return Ok(config);
        }

        // Check if we're in single directory mode
        if self.directory.is_some() {
            let config = self.to_config()?;
            config.validate()?;
            return Ok(config);
        }

        Err("Either --config file or --directory with --target must be specified".to_string())
    }

    /// Override configuration file settings with CLI arguments
    fn override_config(&self, config: &mut Config) {
        // Override server settings if provided via CLI
        if self.ip.to_string() != "127.0.0.1" {
            config.server.ip = self.ip;
        }
        if self.port != 11451 {
            config.server.port = self.port;
        }
        if self.log_level != "error" {
            config.server.log_level = self.log_level.clone();
        }
        if self.verbose {
            config.server.verbose = self.verbose;
        }
        if self.daemon {
            config.server.daemon = self.daemon;
        }
        if self.pid_file.is_some() {
            config.server.pid_file = self.pid_file.clone();
        }
        if self.work_dir.is_some() {
            config.server.work_dir = self.work_dir.clone();
        }
        if self.max_connections != 100 {
            config.server.max_connections = self.max_connections;
        }
        if self.read_timeout != 30 {
            config.server.read_timeout = self.read_timeout;
        }
        if self.write_timeout != 30 {
            config.server.write_timeout = self.write_timeout;
        }
        if self.read_only {
            config.server.read_only = self.read_only;
        }
        if self.allow_ips.is_some() {
            config.server.allow_ips = self.allow_ips.clone();
        }
        if self.no_color {
            config.server.no_color = self.no_color;
        }
    }

    /// Create a sample configuration
    fn create_sample_config() -> Config {
        let mut config = Config::default();
        config.mounts = vec![
            MountConfig {
                source: PathBuf::from("/Users/aaaa"),
                target: "/bbbb".to_string(),
                read_only: false,
                description: Some("Example mount: maps /Users/aaaa to /bbbb".to_string()),
            },
            MountConfig {
                source: PathBuf::from("/tmp/shared"),
                target: "/shared".to_string(),
                read_only: true,
                description: Some("Read-only shared directory".to_string()),
            },
        ];
        config
    }

    /// Print startup information using log system
    pub fn print_startup_info(config: &Config, allowed_ips: &[IpAddr]) {
        info!("NFS Mirror service starting...");
        info!(
            "Listen address: {}:{}",
            config.server.ip, config.server.port
        );
        info!("Log level: {}", config.server.log_level);
        info!("Max connections: {}", config.server.max_connections);
        info!("Read timeout: {} seconds", config.server.read_timeout);
        info!("Write timeout: {} seconds", config.server.write_timeout);
        info!(
            "Global read-only mode: {}",
            if config.server.read_only { "Yes" } else { "No" }
        );

        if !allowed_ips.is_empty() {
            info!("Allowed IP addresses: {:?}", allowed_ips);
        }

        if config.server.daemon {
            info!("Daemon mode: Enabled");
        }

        info!("Configured mount points:");
        for (i, mount) in config.mounts.iter().enumerate() {
            info!(
                "  {}: {} -> {} (read-only: {}){}",
                i + 1,
                mount.source.display(),
                mount.target,
                if mount.read_only || config.server.read_only {
                    "Yes"
                } else {
                    "No"
                },
                mount
                    .description
                    .as_ref()
                    .map(|d| format!(" - {}", d))
                    .unwrap_or_default()
            );
        }

        info!("NFS service started, waiting for client connections...");
        info!("Mount command examples:");
        for mount in &config.mounts {
            info!(
                "mount -t nfs -o nolocks,vers=3,tcp,port={},mountport={},soft {}:{} /mnt{}",
                config.server.port,
                config.server.port,
                config.server.ip,
                mount.target,
                mount.target
            );
        }
    }
}
