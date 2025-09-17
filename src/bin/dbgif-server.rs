use clap::Parser;
use dbgif_protocol::{
    config::DbgifConfig,
    server::{
        ascii_handler::AsciiHandler,
        connection_manager::ConnectionManager,
        device_manager::DeviceManager,
        dispatcher::MessageDispatcher,
        state::{ServerConfig, ServerState},
        stream_forwarder::StreamForwarder,
    },
};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info};

/// DBGIF Protocol Server
///
/// An ADB-like protocol server for debugging and device communication.
/// Supports multiple concurrent clients and device connections with
/// stream multiplexing and host services.
#[derive(Parser)]
#[command(name = "dbgif-server")]
#[command(version = "1.0.0")]
#[command(about = "DBGIF Protocol Server - ADB-like debugging interface")]
#[command(long_about = None)]
struct Args {
    /// Server listening port
    #[arg(short, long, default_value = "5555")]
    port: u16,

    /// Server listening address
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Device discovery ports (comma-separated)
    #[arg(long, default_value = "5557,5558,5559")]
    device_ports: String,

    /// Maximum concurrent connections
    #[arg(long, default_value = "100")]
    max_connections: usize,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,

    /// Enable device auto-discovery
    #[arg(long, default_value = "true")]
    auto_discovery: bool,

    /// Connection timeout in seconds
    #[arg(long, default_value = "30")]
    connection_timeout: u64,

    /// Ping interval in seconds
    #[arg(long, default_value = "60")]
    ping_interval: u64,
}

impl Args {
    /// Parse device ports from comma-separated string
    fn parse_device_ports(&self) -> Vec<u16> {
        self.device_ports
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect()
    }

    /// Create server socket address
    fn server_address(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.host, self.port).parse()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    setup_logging(&args);

    info!("Starting DBGIF Server v1.0.0");
    info!("Server address: {}:{}", args.host, args.port);
    info!("Device discovery ports: {:?}", args.parse_device_ports());
    info!("Max connections: {}", args.max_connections);

    // Create server configuration
    let server_addr = args.server_address()?;
    let mut server_config = ServerConfig::from_tcp_addr(server_addr);

    // Apply command line overrides
    server_config.max_connections = args.max_connections;
    server_config.connection_timeout = std::time::Duration::from_secs(args.connection_timeout);
    server_config.ping_interval = std::time::Duration::from_secs(args.ping_interval);
    server_config.tcp_discovery_ports = args.parse_device_ports();
    // Note: auto_discovery will be handled by DeviceManager configuration

    // Load additional configuration from file if specified
    if let Some(config_path) = &args.config {
        info!("Loading configuration from: {}", config_path);
        match DbgifConfig::from_file(config_path) {
            Ok(file_config) => {
                info!("Successfully loaded configuration from file");
                info!("File config server: {:?}", file_config.server);
                info!("File config transport: {:?}", file_config.transport);
                info!("File config discovery: {:?}", file_config.discovery);
                info!("File config logging: {:?}", file_config.logging);

                // CLI args take precedence over file config
                info!("Note: CLI arguments override file configuration");
            }
            Err(e) => {
                error!("Failed to load configuration file: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Create server components
    let server_state = Arc::new(ServerState::new(server_config));

    // Register built-in host services with the server state
    use dbgif_protocol::host_services::{
        version::HostVersionService,
        features::HostFeaturesService,
        list::HostListService,
        device::HostDeviceService,
    };

    {
        let mut host_services = server_state.host_services.write().unwrap();
        host_services.register(HostVersionService::new());
        host_services.register(HostFeaturesService::new());
        host_services.register(HostListService::new(Arc::clone(&server_state.device_registry)));
        host_services.register(HostDeviceService::new(Arc::clone(&server_state.device_registry)));
    }

    let device_registry = Arc::clone(&server_state.device_registry);

    // Initialize core components
    let connection_manager = ConnectionManager::new(Arc::clone(&server_state));
    let device_manager = Arc::new(DeviceManager::new(Arc::clone(&server_state), Arc::clone(&device_registry)));
    let stream_forwarder = Arc::new(StreamForwarder::new(Arc::clone(&server_state)));
    let _message_dispatcher = MessageDispatcher::new(Arc::clone(&server_state));

    info!("Starting server components...");

    // Start background services
    connection_manager.start().await?;
    device_manager.start().await?;
    stream_forwarder.start().await?;

    info!("All components started successfully");

    // Start TCP listener for client connections
    info!("Starting TCP listener on {}", server_addr);
    let listener = TcpListener::bind(server_addr).await?;

    info!("DBGIF server is ready and listening on {}", server_addr);
    info!("Press Ctrl+C to shutdown gracefully");

    // Setup graceful shutdown handler
    let shutdown_signal = setup_shutdown_handler();

    // Main server loop
    tokio::select! {
        // Handle shutdown signal
        _ = shutdown_signal => {
            info!("Shutdown signal received, stopping server...");
        }

        // Accept connections using ASCII protocol
        result = accept_connections(listener, Arc::clone(&server_state), Arc::clone(&device_manager), Arc::clone(&stream_forwarder)) => {
            if let Err(e) = result {
                error!("Server error: {}", e);
                return Err(e);
            }
        }
    }

    info!("DBGIF server shutdown complete");
    Ok(())
}

/// Setup logging based on command line arguments
fn setup_logging(args: &Args) {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let filter = if args.debug {
        EnvFilter::new("debug")
    } else if args.verbose {
        EnvFilter::new("info")
    } else {
        EnvFilter::new("warn")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(args.debug)
                .with_line_number(args.debug)
        )
        .init();
}

/// Setup graceful shutdown signal handler
async fn setup_shutdown_handler() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        }
        _ = terminate => {
            info!("Received terminate signal");
        }
    }
}

/// Accept incoming connections with ASCII protocol
async fn accept_connections(
    listener: TcpListener,
    server_state: Arc<ServerState>,
    device_manager: Arc<DeviceManager>,
    stream_forwarder: Arc<StreamForwarder>,
) -> anyhow::Result<()> {
    info!("Connection acceptance loop started");

    let mut session_counter = 0u64;

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                info!("New client connection from: {}", addr);

                // Generate session ID
                session_counter += 1;
                let session_id = format!("session-{:04}", session_counter);

                // Create ASCII handler for this connection
                let handler = AsciiHandler::new(
                    Arc::clone(&server_state),
                    Arc::clone(&device_manager),
                    Arc::clone(&stream_forwarder),
                    session_id.clone(),
                );

                // Handle connection in background task
                tokio::spawn(async move {
                    if let Err(e) = handler.handle_connection(stream).await {
                        error!("ASCII connection handling failed: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
                // Continue accepting other connections despite this error
            }
        }
    }
}

