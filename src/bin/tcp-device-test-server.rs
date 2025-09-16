use clap::Parser;
use dbgif_protocol::{
    protocol::{
        commands::AdbCommand,
        message::AdbMessage,
    },
    transport::{TcpTransport, Transport, TransportAddress, TransportListener, Connection},
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{signal, sync::RwLock, time::interval};
use tracing::{debug, error, info, warn};

/// TCP Device Test Server
///
/// Simulates a DBGIF-compatible device for testing the DBGIF server.
/// This server acts as a test device that can respond to DBGIF protocol
/// messages and simulate device behavior for development and testing.
#[derive(Parser)]
#[command(name = "tcp-device-test-server")]
#[command(version = "1.0.0")]
#[command(about = "TCP Device Test Server - Simulates DBGIF-compatible devices")]
#[command(long_about = None)]
struct Args {
    /// TCP port to listen on
    #[arg(short, long, default_value = "5557")]
    port: u16,

    /// Bind address
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Device ID to simulate
    #[arg(long, default_value = "test-device-001")]
    device_id: String,

    /// Device type/model
    #[arg(long, default_value = "DBGIF-TestDevice")]
    device_model: String,

    /// Device capabilities (comma-separated)
    #[arg(long, default_value = "shell,file,debug")]
    capabilities: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Simulate device latency (milliseconds)
    #[arg(long, default_value = "10")]
    latency: u64,

    /// Simulate device errors (error rate 0.0-1.0)
    #[arg(long, default_value = "0.0")]
    error_rate: f64,

    /// Enable automatic disconnect simulation
    #[arg(long)]
    auto_disconnect: bool,

    /// Auto disconnect interval in seconds
    #[arg(long, default_value = "300")]
    disconnect_interval: u64,
}

impl Args {
    /// Get bind socket address
    fn bind_address(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.host, self.port).parse()
    }

    /// Parse device capabilities
    fn parse_capabilities(&self) -> Vec<String> {
        self.capabilities
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    }
}

/// Device simulation state
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct DeviceState {
    device_id: String,
    device_model: String,
    capabilities: Vec<String>,
    connection_count: u64,
    message_count: u64,
    last_activity: std::time::Instant,
    is_connected: bool,
}

impl DeviceState {
    fn new(device_id: String, device_model: String, capabilities: Vec<String>) -> Self {
        Self {
            device_id,
            device_model,
            capabilities,
            connection_count: 0,
            message_count: 0,
            last_activity: std::time::Instant::now(),
            is_connected: false,
        }
    }

    fn connect(&mut self) {
        self.connection_count += 1;
        self.is_connected = true;
        self.last_activity = std::time::Instant::now();
    }

    fn disconnect(&mut self) {
        self.is_connected = false;
    }

    fn handle_message(&mut self) {
        self.message_count += 1;
        self.last_activity = std::time::Instant::now();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    setup_logging(&args);

    info!("TCP Device Test Server v1.0.0");
    info!("Device ID: {}", args.device_id);
    info!("Device Model: {}", args.device_model);
    info!("Listening on: {}:{}", args.host, args.port);
    info!("Capabilities: {:?}", args.parse_capabilities());

    // Create device state
    let device_state = Arc::new(RwLock::new(DeviceState::new(
        args.device_id.clone(),
        args.device_model.clone(),
        args.parse_capabilities(),
    )));

    // Setup transport and listener
    let bind_addr = args.bind_address()?;
    let transport = TcpTransport::new();
    let transport_addr = TransportAddress::from(bind_addr);

    info!("Starting device transport listener...");
    let mut listener = transport.listen(&transport_addr).await?;

    info!("TCP Device Test Server is ready on {}", bind_addr);
    info!("Waiting for DBGIF server connections...");

    // Setup graceful shutdown
    let shutdown_signal = setup_shutdown_handler();

    // Start background tasks
    if args.auto_disconnect {
        let state_clone = Arc::clone(&device_state);
        let disconnect_interval = args.disconnect_interval;
        tokio::spawn(async move {
            simulate_auto_disconnect(state_clone, disconnect_interval).await;
        });
    }

    // Start stats reporting
    let state_clone = Arc::clone(&device_state);
    tokio::spawn(async move {
        report_stats(state_clone).await;
    });

    // Main server loop
    tokio::select! {
        // Handle shutdown signal
        _ = shutdown_signal => {
            info!("Shutdown signal received, stopping device server...");
        }

        // Accept connections
        result = accept_connections(&mut *listener, device_state, args.latency, args.error_rate) => {
            if let Err(e) = result {
                error!("Device server error: {}", e);
                return Err(e);
            }
        }
    }

    info!("TCP Device Test Server shutdown complete");
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

/// Accept and handle incoming connections
async fn accept_connections(
    listener: &mut dyn TransportListener<Connection = impl Connection + 'static>,
    device_state: Arc<RwLock<DeviceState>>,
    latency_ms: u64,
    error_rate: f64,
) -> anyhow::Result<()> {
    info!("Starting connection acceptance loop");

    loop {
        // Accept new connection
        match listener.accept().await {
            Ok(connection) => {
                let connection_id = connection.connection_id();
                info!("New connection accepted: {}", connection_id);

                // Update device state
                {
                    let mut state = device_state.write().await;
                    state.connect();
                }

                // Handle connection in separate task
                let state_clone = Arc::clone(&device_state);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(
                        Box::new(connection),
                        state_clone,
                        latency_ms,
                        error_rate,
                    ).await {
                        error!("Connection handler error: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Handle an individual connection
async fn handle_connection(
    mut connection: Box<dyn Connection>,
    device_state: Arc<RwLock<DeviceState>>,
    latency_ms: u64,
    _error_rate: f64,
) -> anyhow::Result<()> {
    let connection_id = connection.connection_id();
    info!("Handling connection: {}", connection_id);

    let mut buffer = vec![0u8; 1024];

    loop {
        // Simulate device latency
        if latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(latency_ms)).await;
        }

        // Read message from connection
        match connection.receive_bytes(&mut buffer).await? {
            Some(size) if size >= 24 => {
                // Parse ADB message
                match AdbMessage::deserialize(&buffer[..size]) {
                    Ok(message) => {
                        debug!("Received message: command={:?}, arg0={}, arg1={}",
                               AdbCommand::from_u32(message.command), message.arg0, message.arg1);

                        // Update device state
                        {
                            let mut state = device_state.write().await;
                            state.handle_message();
                        }

                        // Handle the message
                        if let Some(response) = handle_device_message(&message, &device_state).await? {
                            let response_data = response.serialize();
                            connection.send_bytes(&response_data).await?;
                            debug!("Sent response: command={:?}",
                                   AdbCommand::from_u32(response.command));
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse message: {}", e);
                    }
                }
            }
            Some(size) => {
                warn!("Received incomplete message: {} bytes", size);
            }
            None => {
                info!("Connection closed by peer: {}", connection_id);
                break;
            }
        }
    }

    // Update device state on disconnect
    {
        let mut state = device_state.write().await;
        state.disconnect();
    }

    info!("Connection handler finished: {}", connection_id);
    Ok(())
}

/// Handle device-specific protocol messages
async fn handle_device_message(
    message: &AdbMessage,
    device_state: &Arc<RwLock<DeviceState>>,
) -> anyhow::Result<Option<AdbMessage>> {
    let command = AdbCommand::from_u32(message.command);

    match command {
        Some(AdbCommand::CNXN) => {
            info!("Handling CNXN handshake");
            let state = device_state.read().await;
            let device_info = format!("device::{}", state.device_id);

            Ok(Some(AdbMessage::new_cnxn(
                0x01000000, // Protocol version
                1024 * 1024, // Max data size
                device_info.as_bytes().to_vec(),
            )))
        }
        Some(AdbCommand::PING) => {
            debug!("Handling PING");
            Ok(Some(AdbMessage::new_pong()))
        }
        Some(AdbCommand::OPEN) => {
            info!("Handling OPEN request");
            // For simplicity, acknowledge all OPEN requests
            Ok(Some(AdbMessage::new_okay(message.arg0, message.arg1)))
        }
        Some(AdbCommand::WRTE) => {
            debug!("Handling WRTE data transfer");
            // Echo back the data or send acknowledgment
            Ok(Some(AdbMessage::new_okay(message.arg1, message.arg0)))
        }
        Some(AdbCommand::CLSE) => {
            debug!("Handling CLSE stream close");
            Ok(Some(AdbMessage::new_okay(message.arg1, message.arg0)))
        }
        Some(cmd) => {
            debug!("Unhandled command: {:?}", cmd);
            Ok(None)
        }
        None => {
            warn!("Unknown command: 0x{:08x}", message.command);
            Ok(None)
        }
    }
}

/// Simulate automatic disconnection
async fn simulate_auto_disconnect(
    device_state: Arc<RwLock<DeviceState>>,
    interval_seconds: u64,
) {
    let mut interval = interval(Duration::from_secs(interval_seconds));

    loop {
        interval.tick().await;

        let should_disconnect = {
            let state = device_state.read().await;
            state.is_connected
        };

        if should_disconnect {
            info!("Simulating automatic disconnect");
            let mut state = device_state.write().await;
            state.disconnect();
        }
    }
}

/// Report device statistics periodically
async fn report_stats(device_state: Arc<RwLock<DeviceState>>) {
    let mut interval = interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        let state = device_state.read().await;
        info!("Device Stats - Connections: {}, Messages: {}, Status: {}",
              state.connection_count,
              state.message_count,
              if state.is_connected { "Connected" } else { "Disconnected" });
    }
}