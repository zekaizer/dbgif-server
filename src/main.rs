use anyhow::{Result, Context};
use clap::{Parser, Subcommand};
use dbgif_server::{DaemonBuilder, DaemonConfig};
use std::path::PathBuf;
use tracing::{error, info, warn};

/// DBGIF Server - Multi-transport ADB daemon
#[derive(Parser)]
#[command(
    name = "dbgif-server",
    version = env!("CARGO_PKG_VERSION"),
    about = "Multi-transport ADB daemon supporting TCP, USB Device, and USB Bridge connections",
    long_about = "DBGIF Server provides ADB protocol compatibility with support for multiple transport types including direct TCP connections, USB device connections, and USB-to-USB bridge connections using PL25A1 hardware."
)]
struct Cli {
    /// Configuration file path
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Bind address for the ADB server
    #[arg(short, long, value_name = "ADDR")]
    bind: Option<String>,

    /// Maximum number of concurrent connections
    #[arg(short, long, value_name = "NUM")]
    max_connections: Option<usize>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, value_name = "LEVEL")]
    log_level: Option<String>,

    /// Log format (compact, pretty, json)
    #[arg(long, value_name = "FORMAT")]
    log_format: Option<String>,

    /// Enable file logging
    #[arg(long)]
    log_file: bool,

    /// Log file path
    #[arg(long, value_name = "FILE")]
    log_file_path: Option<PathBuf>,

    /// Disable TCP transport
    #[arg(long)]
    no_tcp: bool,

    /// Disable USB device transport
    #[arg(long)]
    no_usb_device: bool,

    /// Disable USB bridge transport
    #[arg(long)]
    no_usb_bridge: bool,

    /// Enable debug mode (equivalent to --log-level debug --log-format pretty)
    #[arg(short, long)]
    debug: bool,

    /// Use development configuration preset
    #[arg(long)]
    dev: bool,

    /// Use production configuration preset
    #[arg(long)]
    prod: bool,

    /// Run as daemon (background process)
    #[arg(short = 'D', long)]
    daemon: bool,

    /// Subcommands
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Clone)]
enum Commands {
    /// Start the DBGIF server
    Start {
        /// Force start even if another instance is running
        #[arg(short, long)]
        force: bool,
    },
    /// Stop the running DBGIF server
    Stop {
        /// Force stop (kill immediately)
        #[arg(short, long)]
        force: bool,
    },
    /// Restart the DBGIF server
    Restart {
        /// Force restart
        #[arg(short, long)]
        force: bool,
    },
    /// Show server status
    Status,
    /// Show server configuration
    Config {
        /// Generate example configuration file
        #[arg(short, long)]
        generate: bool,
        /// Output path for generated config
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// List available devices
    Devices {
        /// Show detailed device information
        #[arg(short, long)]
        long: bool,
        /// Continuously monitor for device changes
        #[arg(short, long)]
        watch: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging first
    setup_logging(&cli)?;

    info!("DBGIF Server v{} starting", env!("CARGO_PKG_VERSION"));

    // Handle subcommands
    if let Some(ref command) = cli.command {
        return handle_command(command.clone(), &cli).await;
    }

    // Load or create configuration
    let mut config = load_configuration(&cli)?;

    // Apply CLI overrides to configuration
    apply_cli_overrides(&mut config, &cli)?;

    // Validate final configuration
    config.validate().context("Configuration validation failed")?;

    info!("Starting daemon with configuration:");
    info!("  Bind address: {}", config.server.bind_address);
    info!("  Max connections: {}", config.server.max_connections);
    info!("  TCP transport: {}", config.transport.enable_tcp);
    info!("  USB device transport: {}", config.transport.enable_usb_device);
    info!("  USB bridge transport: {}", config.transport.enable_usb_bridge);

    // Create and start daemon
    let mut daemon = DaemonBuilder::new()
        .with_config(config)
        .build()
        .context("Failed to create daemon")?;

    // Handle daemon mode
    if cli.daemon {
        info!("Starting in daemon mode");
        // TODO: Implement proper daemonization for production
        // For now, just run normally
    }

    // Run the daemon
    match daemon.run().await {
        Ok(()) => {
            info!("DBGIF Server shut down successfully");
            Ok(())
        }
        Err(e) => {
            error!("DBGIF Server error: {}", e);
            Err(e)
        }
    }
}

/// Set up tracing/logging based on CLI arguments
fn setup_logging(cli: &Cli) -> Result<()> {
    use dbgif_server::logging::{init_logging, setup_panic_hook, LoggingConfig};

    // Create logging configuration based on CLI arguments
    let mut logging_config = if cli.dev {
        LoggingConfig::development()
    } else if cli.prod {
        LoggingConfig::production()
    } else {
        LoggingConfig::default()
    };

    // Apply CLI overrides
    if cli.debug {
        logging_config.level = "debug".to_string();
        logging_config.format = "pretty".to_string();
        logging_config.include_spans = true;
        logging_config.include_line_numbers = true;
    }

    if let Some(ref level) = cli.log_level {
        logging_config.level = level.clone();
    }

    if let Some(ref format) = cli.log_format {
        logging_config.format = format.clone();
    }

    if cli.log_file || cli.log_file_path.is_some() {
        logging_config.enable_file = true;
        if let Some(ref path) = cli.log_file_path {
            logging_config.file_path = Some(path.clone());
        } else {
            logging_config.file_path = Some("dbgif-server.log".into());
        }
    }

    // Initialize structured logging
    init_logging(&logging_config).context("Failed to initialize logging")?;

    // Set up panic hook for better panic logging
    setup_panic_hook();

    info!("Structured logging initialized with level: {}", logging_config.level);
    if logging_config.enable_file {
        if let Some(ref path) = logging_config.file_path {
            info!("File logging enabled: {}", path.display());
        }
    }

    Ok(())
}

/// Load configuration from various sources
fn load_configuration(cli: &Cli) -> Result<DaemonConfig> {
    // Priority order:
    // 1. Explicit config file from CLI
    // 2. Preset configurations (dev/prod)
    // 3. Default config file location
    // 4. Default configuration

    if let Some(config_path) = &cli.config {
        info!("Loading configuration from: {}", config_path.display());
        return DaemonConfig::load_from_file(config_path);
    }

    if cli.dev {
        info!("Using development configuration preset");
        return Ok(DaemonConfig::development());
    }

    if cli.prod {
        info!("Using production configuration preset");
        return Ok(DaemonConfig::production());
    }

    // Try default config file location
    let default_config_path = DaemonConfig::default_config_path();
    if default_config_path.exists() {
        info!("Loading configuration from default location: {}", default_config_path.display());
        match DaemonConfig::load_from_file(&default_config_path) {
            Ok(config) => return Ok(config),
            Err(e) => {
                warn!("Failed to load default config file: {}", e);
                warn!("Falling back to default configuration");
            }
        }
    }

    info!("Using default configuration");
    Ok(DaemonConfig::default())
}

/// Apply CLI argument overrides to configuration
fn apply_cli_overrides(config: &mut DaemonConfig, cli: &Cli) -> Result<()> {
    // Server overrides
    if let Some(ref bind_addr) = cli.bind {
        config.server.bind_address = bind_addr.parse()
            .context("Invalid bind address format")?;
    }

    if let Some(max_conn) = cli.max_connections {
        config.server.max_connections = max_conn;
    }

    // Logging overrides
    if let Some(ref level) = cli.log_level {
        config.logging.level = level.clone();
    }

    if let Some(ref format) = cli.log_format {
        config.logging.format = format.clone();
    }

    if cli.log_file || cli.log_file_path.is_some() {
        config.logging.enable_file = true;
        if let Some(ref path) = cli.log_file_path {
            config.logging.file_path = Some(path.clone());
        }
    }

    if cli.debug {
        config.logging.level = "debug".to_string();
        config.logging.format = "pretty".to_string();
    }

    // Transport overrides
    if cli.no_tcp {
        config.transport.enable_tcp = false;
    }

    if cli.no_usb_device {
        config.transport.enable_usb_device = false;
    }

    if cli.no_usb_bridge {
        config.transport.enable_usb_bridge = false;
    }

    // Apply environment variable overrides
    config.apply_env_overrides();

    Ok(())
}

/// Handle subcommands
async fn handle_command(command: Commands, cli: &Cli) -> Result<()> {
    match command {
        Commands::Start { force } => {
            info!("Starting DBGIF Server (force={})", force);
            // For now, just start normally - in production would check for existing instances
            let config = load_configuration(cli)?;
            let mut daemon = DaemonBuilder::new()
                .with_config(config)
                .build()?;
            daemon.run().await
        }

        Commands::Stop { force } => {
            info!("Stopping DBGIF Server (force={})", force);
            // TODO: Implement stop command (send signal to running daemon)
            println!("Stop command not yet implemented");
            Ok(())
        }

        Commands::Restart { force } => {
            info!("Restarting DBGIF Server (force={})", force);
            // TODO: Implement restart command
            println!("Restart command not yet implemented");
            Ok(())
        }

        Commands::Status => {
            info!("Checking DBGIF Server status");
            // TODO: Implement status check (connect to running daemon)
            println!("Status command not yet implemented");
            Ok(())
        }

        Commands::Config { generate, output } => {
            if generate {
                let config = if cli.dev {
                    DaemonConfig::development()
                } else if cli.prod {
                    DaemonConfig::production()
                } else {
                    DaemonConfig::default()
                };

                let output_path = output.unwrap_or_else(|| "dbgif-server.toml".into());
                config.save_to_file(&output_path)?;
                println!("Configuration saved to: {}", output_path.display());
            } else {
                let config = load_configuration(cli)?;
                println!("{}", toml::to_string_pretty(&config)?);
            }
            Ok(())
        }

        Commands::Devices { long, watch } => {
            info!("Listing devices (long={}, watch={})", long, watch);
            // TODO: Implement device listing (connect to running daemon or direct query)
            println!("Devices command not yet implemented");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_parse() {
        // Test that CLI can be parsed
        let _cli = Cli::command();
    }

    #[test]
    fn test_basic_args() {
        let cli = Cli::parse_from([
            "dbgif-server",
            "--bind", "0.0.0.0:5037",
            "--max-connections", "200",
            "--log-level", "debug"
        ]);

        assert_eq!(cli.bind, Some("0.0.0.0:5037".to_string()));
        assert_eq!(cli.max_connections, Some(200));
        assert_eq!(cli.log_level, Some("debug".to_string()));
    }

    #[test]
    fn test_subcommands() {
        let cli = Cli::parse_from(["dbgif-server", "start", "--force"]);

        match cli.command {
            Some(Commands::Start { force }) => assert!(force),
            _ => panic!("Expected Start command"),
        }
    }

    #[test]
    fn test_config_generation() {
        let cli = Cli::parse_from([
            "dbgif-server",
            "config",
            "--generate",
            "--output", "test-config.toml"
        ]);

        match cli.command {
            Some(Commands::Config { generate, output }) => {
                assert!(generate);
                assert_eq!(output, Some("test-config.toml".into()));
            }
            _ => panic!("Expected Config command"),
        }
    }

    #[tokio::test]
    async fn test_configuration_loading() {
        // Test default configuration loading
        let cli = Cli::parse_from(["dbgif-server"]);
        let config = load_configuration(&cli);
        assert!(config.is_ok());

        // Test development preset
        let cli = Cli::parse_from(["dbgif-server", "--dev"]);
        let config = load_configuration(&cli).unwrap();
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_cli_overrides() {
        let cli = Cli::parse_from([
            "dbgif-server",
            "--bind", "192.168.1.100:6037",
            "--max-connections", "50",
            "--debug",
            "--no-usb-bridge"
        ]);

        let mut config = DaemonConfig::default();
        apply_cli_overrides(&mut config, &cli).unwrap();

        assert_eq!(config.server.bind_address.to_string(), "192.168.1.100:6037");
        assert_eq!(config.server.max_connections, 50);
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.logging.format, "pretty");
        assert!(!config.transport.enable_usb_bridge);
    }
}