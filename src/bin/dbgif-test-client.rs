use clap::{Parser, Subcommand};
use dbgif_protocol::{
    protocol::{
        commands::AdbCommand,
        message::AdbMessage,
    },
    transport::{TcpTransport, Transport, TransportAddress, Connection},
};
use std::{net::SocketAddr, time::Duration};
use tracing::{debug, error, info, warn};

/// DBGIF Protocol Test Client
///
/// Test client for the DBGIF protocol server. Supports various test scenarios
/// including connection handshake, host services, device communication, and
/// stream multiplexing tests.
#[derive(Parser)]
#[command(name = "dbgif-test-client")]
#[command(version = "1.0.0")]
#[command(about = "DBGIF Protocol Test Client - Testing tool for DBGIF server")]
#[command(long_about = None)]
struct Args {
    /// DBGIF server address
    #[arg(short, long, default_value = "127.0.0.1:5555")]
    server: String,

    /// Connection timeout in seconds
    #[arg(long, default_value = "10")]
    timeout: u64,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Number of concurrent connections for load testing
    #[arg(long, default_value = "1")]
    connections: usize,

    /// Test to run
    #[command(subcommand)]
    test: TestCommand,
}

/// Available test commands
#[derive(Subcommand)]
enum TestCommand {
    /// Basic connection test (CNXN handshake)
    Basic {
        /// Client identity string
        #[arg(long, default_value = "test-client:1.0.0")]
        identity: String,
    },

    /// Test host services (host:version, host:features, host:list, host:device)
    HostServices {
        /// Test all host services
        #[arg(long)]
        all: bool,
        /// Test specific service
        #[arg(long)]
        service: Option<String>,
    },

    /// Test device communication
    Device {
        /// Device ID to connect to
        #[arg(long)]
        device_id: String,
        /// Command to send to device
        #[arg(long, default_value = "ping")]
        command: String,
    },

    /// Test stream multiplexing
    Streams {
        /// Number of concurrent streams
        #[arg(long, default_value = "3")]
        count: usize,
        /// Data size per stream
        #[arg(long, default_value = "1024")]
        data_size: usize,
    },

    /// Load test with multiple connections
    Load {
        /// Number of connections
        #[arg(long, default_value = "10")]
        connections: usize,
        /// Duration in seconds
        #[arg(long, default_value = "60")]
        duration: u64,
    },

    /// Protocol compliance test
    Protocol {
        /// Test all protocol features
        #[arg(long)]
        all: bool,
        /// Test specific protocol feature
        #[arg(long)]
        feature: Option<String>,
    },

    /// Interactive mode for manual testing
    Interactive,
}

impl Args {
    /// Parse server address
    fn server_address(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        self.server.parse()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    setup_logging(&args);

    info!("DBGIF Test Client v1.0.0");
    info!("Connecting to server: {}", args.server);

    // Parse server address
    let server_addr = args.server_address()?;
    let transport_addr = TransportAddress::from(server_addr);

    // Execute the requested test
    match args.test {
        TestCommand::Basic { identity } => {
            run_basic_test(&transport_addr, &identity, args.timeout).await?;
        }
        TestCommand::HostServices { all, service } => {
            run_host_services_test(&transport_addr, all, service.as_deref(), args.timeout).await?;
        }
        TestCommand::Device { device_id, command } => {
            run_device_test(&transport_addr, &device_id, &command, args.timeout).await?;
        }
        TestCommand::Streams { count, data_size } => {
            run_streams_test(&transport_addr, count, data_size, args.timeout).await?;
        }
        TestCommand::Load { connections, duration } => {
            run_load_test(&transport_addr, connections, duration, args.timeout).await?;
        }
        TestCommand::Protocol { all, feature } => {
            run_protocol_test(&transport_addr, all, feature.as_deref(), args.timeout).await?;
        }
        TestCommand::Interactive => {
            run_interactive_mode(&transport_addr, args.timeout).await?;
        }
    }

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

/// Create a connection to the DBGIF server
async fn create_connection(
    addr: &TransportAddress,
    timeout_secs: u64,
) -> anyhow::Result<Box<dyn Connection>> {
    let transport = TcpTransport::new();
    let timeout = Duration::from_secs(timeout_secs);

    info!("Connecting to {}", addr);
    let connection = transport.connect_timeout(addr, timeout).await?;
    info!("Connected successfully");

    Ok(Box::new(connection))
}

/// Run basic connection test (CNXN handshake)
async fn run_basic_test(
    addr: &TransportAddress,
    identity: &str,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Basic Connection Test ===");

    let mut connection = create_connection(addr, timeout).await?;

    // Send CNXN message
    info!("Sending CNXN handshake with identity: {}", identity);
    let cnxn_message = AdbMessage::new_cnxn(1, 0x01000000, identity.as_bytes().to_vec());

    let data = cnxn_message.serialize();
    connection.send_bytes(&data).await?;

    // Wait for CNXN response
    let mut buffer = vec![0u8; 1024];
    let response_size = connection.receive_bytes(&mut buffer).await?;

    match response_size {
        Some(size) if size >= 24 => {
            let response = AdbMessage::deserialize(&buffer[..size])?;
            info!("Received response: command={:?}, arg0={}, arg1={}",
                  AdbCommand::from_u32(response.command), response.arg0, response.arg1);

            if response.command == AdbCommand::CNXN as u32 {
                info!("✅ CNXN handshake successful!");
                return Ok(());
            }
        }
        Some(size) => {
            warn!("Received incomplete response: {} bytes", size);
        }
        None => {
            warn!("Connection closed by server");
        }
    }

    error!("❌ CNXN handshake failed");
    Err(anyhow::anyhow!("CNXN handshake failed"))
}

/// Run host services test
async fn run_host_services_test(
    addr: &TransportAddress,
    test_all: bool,
    service: Option<&str>,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Host Services Test ===");

    let mut connection = create_connection(addr, timeout).await?;

    // First establish CNXN
    let cnxn_message = AdbMessage::new_cnxn(1, 0x01000000, "test-client:host-services".as_bytes().to_vec());
    let data = cnxn_message.serialize();
    connection.send_bytes(&data).await?;

    // Wait for CNXN response (simplified)
    let mut buffer = vec![0u8; 1024];
    let _ = connection.receive_bytes(&mut buffer).await?;

    if test_all {
        // Test all host services
        let services = vec!["host:version", "host:features", "host:list"];
        for service_name in services {
            info!("Testing service: {}", service_name);
            test_host_service(&mut *connection, service_name).await?;
        }
    } else if let Some(service_name) = service {
        info!("Testing service: {}", service_name);
        test_host_service(&mut *connection, service_name).await?;
    } else {
        // Default: test version service
        test_host_service(&mut *connection, "host:version").await?;
    }

    info!("✅ Host services test completed");
    Ok(())
}

/// Test a specific host service
async fn test_host_service(
    connection: &mut dyn Connection,
    service_name: &str,
) -> anyhow::Result<()> {
    // Send OPEN message for host service
    let stream_id = 1;
    let open_message = AdbMessage::new_open(stream_id, service_name.as_bytes().to_vec());

    let data = open_message.serialize();
    connection.send_bytes(&data).await?;

    // Wait for OKAY response
    let mut buffer = vec![0u8; 1024];
    let response_size = connection.receive_bytes(&mut buffer).await?;

    if let Some(size) = response_size {
        if size >= 24 {
            let response = AdbMessage::deserialize(&buffer[..size])?;
            debug!("Service {} response: command={:?}",
                   service_name, AdbCommand::from_u32(response.command));
            return Ok(());
        }
    }

    Err(anyhow::anyhow!("Failed to test service: {}", service_name))
}

/// Run device communication test
async fn run_device_test(
    _addr: &TransportAddress,
    device_id: &str,
    command: &str,
    _timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Device Communication Test ===");
    info!("Device ID: {}, Command: {}", device_id, command);

    // TODO: Implement device test logic
    warn!("Device test not yet fully implemented");

    Ok(())
}

/// Run stream multiplexing test
async fn run_streams_test(
    _addr: &TransportAddress,
    stream_count: usize,
    data_size: usize,
    _timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Stream Multiplexing Test ===");
    info!("Stream count: {}, Data size: {}", stream_count, data_size);

    // TODO: Implement streams test logic
    warn!("Streams test not yet fully implemented");

    Ok(())
}

/// Run load test
async fn run_load_test(
    _addr: &TransportAddress,
    connection_count: usize,
    duration: u64,
    _timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Load Test ===");
    info!("Connections: {}, Duration: {}s", connection_count, duration);

    // TODO: Implement load test logic
    warn!("Load test not yet fully implemented");

    Ok(())
}

/// Run protocol compliance test
async fn run_protocol_test(
    _addr: &TransportAddress,
    test_all: bool,
    feature: Option<&str>,
    _timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Protocol Compliance Test ===");

    if test_all {
        info!("Testing all protocol features");
    } else if let Some(feature_name) = feature {
        info!("Testing feature: {}", feature_name);
    }

    // TODO: Implement protocol test logic
    warn!("Protocol test not yet fully implemented");

    Ok(())
}

/// Run interactive mode
async fn run_interactive_mode(
    addr: &TransportAddress,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Interactive Mode ===");
    info!("Press Ctrl+C to exit");

    let _connection = create_connection(addr, timeout).await?;

    // TODO: Implement interactive command loop
    warn!("Interactive mode not yet fully implemented");

    // Simple placeholder loop
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        // In full implementation, this would read commands from stdin
        // and send them to the server
    }
}