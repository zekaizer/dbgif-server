use clap::Parser;
use dbgif_protocol::{
    protocol::{
        commands::AdbCommand,
        message::AdbMessage,
    },
};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    signal, sync::RwLock, time::{interval, timeout}
};
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

/// Connection state for 3-way handshake
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ConnectionState {
    WaitingForServerCnxn,
    SentDeviceCnxn,
    Connected(u32), // Store connect_id
}

/// Stream information
#[derive(Debug, Clone)]
struct StreamInfo {
    _local_id: u32,
    _remote_id: u32,
    service_name: String,
    _is_open: bool,
}

/// Device simulation state
#[derive(Debug, Clone)]
struct DeviceState {
    device_id: String,
    device_model: String,
    _capabilities: Vec<String>,
    connection_count: u64,
    message_count: u64,
    _last_activity: std::time::Instant,
    is_connected: bool,
    connection_state: ConnectionState,
    connect_id: Option<u32>,
    streams: HashMap<u32, StreamInfo>,
}

impl DeviceState {
    fn new(device_id: String, device_model: String, capabilities: Vec<String>) -> Self {
        Self {
            device_id,
            device_model,
            _capabilities: capabilities,
            connection_count: 0,
            message_count: 0,
            _last_activity: std::time::Instant::now(),
            is_connected: false,
            connection_state: ConnectionState::WaitingForServerCnxn,
            connect_id: None,
            streams: HashMap::new(),
        }
    }

    fn connect(&mut self) {
        self.connection_count += 1;
        self.is_connected = true;
        self._last_activity = std::time::Instant::now();
    }

    fn disconnect(&mut self) {
        self.is_connected = false;
    }

    fn handle_message(&mut self) {
        self.message_count += 1;
        self._last_activity = std::time::Instant::now();
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

    // Setup TCP listener
    let bind_addr = args.bind_address()?;

    info!("Starting device TCP listener...");
    let listener = TcpListener::bind(&bind_addr).await?;

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
        result = accept_connections(listener, device_state, args.latency, args.error_rate) => {
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
    listener: TcpListener,
    device_state: Arc<RwLock<DeviceState>>,
    latency_ms: u64,
    error_rate: f64,
) -> anyhow::Result<()> {
    info!("Starting connection acceptance loop");

    loop {
        // Accept new connection
        match listener.accept().await {
            Ok((stream, addr)) => {
                info!("New connection accepted from: {}", addr);

                // Update device state
                {
                    let mut state = device_state.write().await;
                    state.connect();
                }

                // Handle connection in separate task
                let state_clone = Arc::clone(&device_state);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(
                        stream,
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

/// Handle an individual connection with 3-way handshake
async fn handle_connection(
    mut stream: TcpStream,
    device_state: Arc<RwLock<DeviceState>>,
    latency_ms: u64,
    error_rate: f64,
) -> anyhow::Result<()> {
    info!("Handling connection");

    let mut buffer = vec![0u8; 4096];

    // Start heartbeat task
    let state_clone = Arc::clone(&device_state);
    let heartbeat_handle = tokio::spawn(async move {
        heartbeat_task(state_clone).await;
    });

    loop {
        // Simulate device latency
        if latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(latency_ms)).await;
        }

        // Read message from connection with timeout
        match timeout(Duration::from_secs(30), stream.read(&mut buffer)).await {
            Ok(Ok(size)) if size >= 24 => {
                // Parse ADB message
                match AdbMessage::deserialize(&buffer[..size]) {
                    Ok(message) => {
                        debug!("Received message: command={:?}, arg0={}, arg1={}",
                               AdbCommand::from_u32(message.command), message.arg0, message.arg1);

                        // Simulate random errors based on error_rate
                        if error_rate > 0.0 && rand::random::<f64>() < error_rate {
                            warn!("Simulating error (rate: {})", error_rate);
                            // Send corrupted response or drop connection
                            if rand::random::<bool>() {
                                // Send error response
                                let error_msg = AdbMessage {
                                    command: 0xDEADBEEF,
                                    arg0: 0,
                                    arg1: 0,
                                    data_length: 0,
                                    data_crc32: 0,
                                    magic: !0xDEADBEEF,
                                    data: vec![],
                                };
                                let _ = stream.write_all(&error_msg.serialize()).await;
                                continue;
                            } else {
                                // Drop connection
                                warn!("Simulating connection drop");
                                break;
                            }
                        }

                        // Update device state
                        {
                            let mut state = device_state.write().await;
                            state.handle_message();
                        }

                        // Handle the message based on connection state
                        let responses = handle_device_message_with_state(&message, &device_state, &mut stream).await?;

                        for response in responses {
                            let response_data = response.serialize();
                            stream.write_all(&response_data).await?;
                            debug!("Sent response: command={:?}",
                                   AdbCommand::from_u32(response.command));
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse message: {}", e);
                    }
                }
            }
            Ok(Ok(size)) if size == 0 => {
                info!("Connection closed by peer");
                break;
            }
            Ok(Ok(size)) => {
                warn!("Received incomplete message: {} bytes", size);
            }
            Ok(Err(e)) => {
                error!("Error receiving message: {}", e);
                break;
            }
            Err(_) => {
                warn!("Connection timeout, sending PING");
                // Send PING on timeout
                let ping = AdbMessage::new_ping();
                stream.write_all(&ping.serialize()).await?;
            }
        }
    }

    // Abort heartbeat task
    heartbeat_handle.abort();

    // Update device state on disconnect
    {
        let mut state = device_state.write().await;
        state.disconnect();
    }

    info!("Connection handler finished");
    Ok(())
}

/// Handle device messages with proper 3-way handshake state machine
async fn handle_device_message_with_state(
    message: &AdbMessage,
    device_state: &Arc<RwLock<DeviceState>>,
    _stream: &mut TcpStream,
) -> anyhow::Result<Vec<AdbMessage>> {
    let command = AdbCommand::from_u32(message.command);
    let mut responses = Vec::new();

    match command {
        Some(AdbCommand::CNXN) => {
            let mut state = device_state.write().await;

            match state.connection_state {
                ConnectionState::WaitingForServerCnxn => {
                    info!("Received initial CNXN from server");

                    // Parse connect_id from server's CNXN
                    let connect_id = message.arg0;

                    // Send device CNXN response
                    let device_info = format!("device:{}:{}",
                        state.device_model, state.device_id);

                    let cnxn_response = AdbMessage::new_cnxn(
                        0x01000000,  // Protocol version
                        1024 * 1024, // Max data size
                        device_info.as_bytes().to_vec(),
                    );

                    responses.push(cnxn_response);

                    // Update state
                    state.connection_state = ConnectionState::SentDeviceCnxn;
                    state.connect_id = Some(connect_id);

                    info!("Sent device CNXN, moving to SentDeviceCnxn state");
                }
                ConnectionState::SentDeviceCnxn => {
                    info!("Received final CNXN from server");

                    // Extract connect_id
                    let connect_id = message.arg0;

                    // Complete handshake
                    state.connection_state = ConnectionState::Connected(connect_id);
                    state.connect_id = Some(connect_id);
                    state.is_connected = true;

                    info!("3-way handshake complete! Connected with ID: {}", connect_id);
                }
                ConnectionState::Connected(_) => {
                    warn!("Received unexpected CNXN in connected state");
                }
            }
        }
        Some(AdbCommand::PING) => {
            debug!("Handling PING");

            // Get connect_id for PONG
            let connect_id = {
                let state = device_state.read().await;
                state.connect_id.unwrap_or(0)
            };

            // Send PONG with matching token
            let pong = AdbMessage {
                command: AdbCommand::PONG as u32,
                arg0: connect_id,
                arg1: message.arg1, // Echo back the token
                data_length: 0,
                data_crc32: 0,
                magic: !(AdbCommand::PONG as u32),
                data: vec![],
            };

            responses.push(pong);
        }
        Some(AdbCommand::OPEN) => {
            info!("Handling OPEN request for stream {}", message.arg0);

            let mut state = device_state.write().await;

            // Parse service name from data
            let service_name = String::from_utf8_lossy(&message.data).to_string();
            info!("Opening service: {}", service_name);

            // Create stream
            let stream_info = StreamInfo {
                _local_id: message.arg0,
                _remote_id: message.arg0, // Will be updated when we send OKAY
                service_name,
                _is_open: true,
            };

            state.streams.insert(message.arg0, stream_info);

            // Send OKAY with stream IDs
            responses.push(AdbMessage::new_okay(message.arg0, message.arg0));
        }
        Some(AdbCommand::WRTE) => {
            debug!("Handling WRTE for stream {}", message.arg0);

            // Check if stream exists
            let stream_exists = {
                let state = device_state.read().await;
                state.streams.contains_key(&message.arg0)
            };

            if stream_exists {
                // Send OKAY acknowledgment
                responses.push(AdbMessage::new_okay(message.arg1, message.arg0));

                // Echo back data (for testing)
                if !message.data.is_empty() {
                    let echo_msg = AdbMessage::new_wrte(
                        message.arg1, // Remote's stream ID
                        message.arg0, // Our stream ID
                        message.data.clone(),
                    );
                    responses.push(echo_msg);
                }
            } else {
                warn!("WRTE for unknown stream {}", message.arg0);
            }
        }
        Some(AdbCommand::CLSE) => {
            debug!("Handling CLSE for stream {}", message.arg0);

            let mut state = device_state.write().await;

            // Remove stream
            if let Some(mut stream) = state.streams.remove(&message.arg0) {
                stream._is_open = false;
                info!("Closed stream {} for service {}", message.arg0, stream.service_name);

                // Send CLSE acknowledgment
                responses.push(AdbMessage::new_clse(message.arg1, message.arg0));
            } else {
                warn!("CLSE for unknown stream {}", message.arg0);
            }
        }
        Some(cmd) => {
            debug!("Unhandled command: {:?}", cmd);
        }
        None => {
            warn!("Unknown command: 0x{:08x}", message.command);
        }
    }

    Ok(responses)
}

/// Periodic heartbeat task
async fn heartbeat_task(device_state: Arc<RwLock<DeviceState>>) {
    let mut interval = interval(Duration::from_secs(1));

    loop {
        interval.tick().await;

        let should_ping = {
            let state = device_state.read().await;
            matches!(state.connection_state, ConnectionState::Connected(_))
        };

        if should_ping {
            debug!("Heartbeat check - connection active");
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