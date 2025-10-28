use clap::Parser;
use std::net::IpAddr;
use std::path::PathBuf;
use tracing::info;

/// NFS Mirror - Mirror a local directory into an NFS shared service
#[derive(Parser)]
#[command(name = "nfs_mirror")]
#[command(about = "Mirror a local directory into an NFS shared service.")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "Benign X <1341398182@qq.com>")]
pub struct Cli {
    /// Local directory path to mirror
    #[arg(help = "Local directory path to mirror")]
    pub directory: PathBuf,

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

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        // Check if directory exists
        if !self.directory.exists() {
            return Err(format!(
                "Directory '{}' does not exist",
                self.directory.display()
            ));
        }

        if !self.directory.is_dir() {
            return Err(format!("'{}' is not a directory", self.directory.display()));
        }

        // Validate port range
        if self.port == 0 {
            return Err("Port cannot be 0".to_string());
        }

        Ok(())
    }

    /// Print startup information using log system
    pub fn print_startup_info(&self, allowed_ips: &[IpAddr]) {
        info!("NFS Mirror service starting...");
        info!("Mirror directory: {}", self.directory.display());
        info!("Listen address: {}:{}", self.ip, self.port);
        info!("Log level: {}", self.log_level);
        info!("Max connections: {}", self.max_connections);
        info!("Read timeout: {} seconds", self.read_timeout);
        info!("Write timeout: {} seconds", self.write_timeout);
        info!(
            "Read-only mode: {}",
            if self.read_only { "Yes" } else { "No" }
        );

        if !allowed_ips.is_empty() {
            info!("Allowed IP addresses: {:?}", allowed_ips);
        }

        if self.daemon {
            info!("Daemon mode: Enabled");
        }

        info!("NFS service started, waiting for client connections...");
        info!("Mount command example:");
        info!(
            "mount -t nfs -o nolocks,vers=3,tcp,port={},mountport={},soft {}:/ /mnt/nfs/",
            self.port, self.port, self.ip
        );
    }
}