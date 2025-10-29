mod cli;
mod config;
mod daemon;
mod filesystem;
mod fsmap;

use clap::Parser;
use tracing_subscriber::FmtSubscriber;

use zerofs_nfsserve::tcp::{NFSTcp, NFSTcpListener};

use cli::Cli;
use daemon::{change_working_directory, handle_daemon_mode};
use filesystem::MirrorFS;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let cli = Cli::parse();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(cli.get_log_level())
        .with_ansi(!cli.no_color)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Load configuration
    let config = cli.load_config()?;

    // Handle daemon mode
    if config.server.daemon {
        handle_daemon_mode(&cli)?;
    }

    // Change working directory if specified
    change_working_directory(&config.server.work_dir)?;

    // Parse allowed IP addresses
    let allowed_ips = cli.parse_allowed_ips();

    // Print startup information
    Cli::print_startup_info(&config, &allowed_ips);

    // Create NFS file system - use the first mount's source as root directory
    let root_dir = if !config.mounts.is_empty() {
        config.mounts[0].source.canonicalize()?
    } else {
        return Err("No mount points configured".into());
    };

    let fs = MirrorFS::new_with_mounts(root_dir, config.server.read_only, config.mounts);

    // Start NFS TCP server
    let addr = format!("{}:{}", config.server.ip, config.server.port).parse()?;
    let listener = NFSTcpListener::bind(addr, fs).await?;

    // Start the server
    listener.handle_forever().await?;

    Ok(())
}
