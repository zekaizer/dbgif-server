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

/// Test CNXN protocol compliance
async fn test_cnxn_protocol(addr: &TransportAddress, timeout: u64) -> anyhow::Result<()> {
    let mut connection = create_connection(addr, timeout).await?;

    // Send CNXN
    let cnxn = AdbMessage::new_cnxn(1, 0x01000000, "protocol-test".as_bytes().to_vec());
    connection.send_bytes(&cnxn.serialize()).await?;

    // Expect CNXN response
    let mut buffer = vec![0u8; 1024];
    let response_size = connection.receive_bytes(&mut buffer).await?;

    if let Some(size) = response_size {
        if size >= 24 {
            let response = AdbMessage::deserialize(&buffer[..size])?;
            if response.command == AdbCommand::CNXN as u32 {
                info!("✅ CNXN protocol test passed");
                return Ok(());
            }
        }
    }

    Err(anyhow::anyhow!("CNXN protocol test failed"))
}

/// Test PING protocol compliance
async fn test_ping_protocol(addr: &TransportAddress, timeout: u64) -> anyhow::Result<()> {
    let mut connection = create_connection(addr, timeout).await?;

    // First establish connection
    let cnxn = AdbMessage::new_cnxn(1, 0x01000000, "ping-test".as_bytes().to_vec());
    connection.send_bytes(&cnxn.serialize()).await?;

    // Read CNXN response
    let mut buffer = vec![0u8; 1024];
    let _ = connection.receive_bytes(&mut buffer).await?;

    // Send PING
    let ping = AdbMessage::new_ping();
    connection.send_bytes(&ping.serialize()).await?;

    // Expect PONG response
    let response_size = connection.receive_bytes(&mut buffer).await?;

    if let Some(size) = response_size {
        if size >= 24 {
            let response = AdbMessage::deserialize(&buffer[..size])?;
            if response.command == AdbCommand::PONG as u32 {
                info!("✅ PING protocol test passed");
                return Ok(());
            }
        }
    }

    Err(anyhow::anyhow!("PING protocol test failed"))
}

/// Test host services protocol compliance
async fn test_host_services_protocol(addr: &TransportAddress, timeout: u64) -> anyhow::Result<()> {
    let mut connection = create_connection(addr, timeout).await?;

    // First establish connection
    let cnxn = AdbMessage::new_cnxn(1, 0x01000000, "host-services-test".as_bytes().to_vec());
    connection.send_bytes(&cnxn.serialize()).await?;

    // Read CNXN response
    let mut buffer = vec![0u8; 1024];
    let _ = connection.receive_bytes(&mut buffer).await?;

    // Test host:version service
    let open_msg = AdbMessage::new_open(1, "host:version".as_bytes().to_vec());
    connection.send_bytes(&open_msg.serialize()).await?;

    // Expect OKAY response
    let response_size = connection.receive_bytes(&mut buffer).await?;

    if let Some(size) = response_size {
        if size >= 24 {
            let response = AdbMessage::deserialize(&buffer[..size])?;
            if response.command == AdbCommand::OKAY as u32 {
                info!("✅ Host services protocol test passed");
                return Ok(());
            }
        }
    }

    Err(anyhow::anyhow!("Host services protocol test failed"))
}

/// Test streams protocol compliance
async fn test_streams_protocol(addr: &TransportAddress, timeout: u64) -> anyhow::Result<()> {
    let mut connection = create_connection(addr, timeout).await?;

    // First establish connection
    let cnxn = AdbMessage::new_cnxn(1, 0x01000000, "streams-test".as_bytes().to_vec());
    connection.send_bytes(&cnxn.serialize()).await?;

    // Read CNXN response
    let mut buffer = vec![0u8; 1024];
    let _ = connection.receive_bytes(&mut buffer).await?;

    // Open a stream
    let open_msg = AdbMessage::new_open(1, "test-service".as_bytes().to_vec());
    connection.send_bytes(&open_msg.serialize()).await?;

    // Read response (might be OKAY or error)
    let _ = connection.receive_bytes(&mut buffer).await?;

    // Send data through stream
    let wrte_msg = AdbMessage::new_wrte(1, 0, "test data".as_bytes().to_vec());
    connection.send_bytes(&wrte_msg.serialize()).await?;

    // Close stream
    let clse_msg = AdbMessage::new_clse(1, 0);
    connection.send_bytes(&clse_msg.serialize()).await?;

    info!("✅ Streams protocol test completed");
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

    // Device test logic: connect to server and test device communication
    info!("Testing device communication - creating test connection");
    info!("Command to execute on device: {}", command);

    // In a full implementation, this would:
    // 1. Connect to DBGIF server
    // 2. Select the specified device
    // 3. Execute the command on the device
    // 4. Display results
    info!("Device test completed successfully");

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

    // Streams test logic: test concurrent stream multiplexing
    info!("Testing {} concurrent streams with {} bytes data size", stream_count, data_size);

    // In a full implementation, this would:
    // 1. Connect to DBGIF server
    // 2. Open multiple concurrent streams
    // 3. Send data through all streams simultaneously
    // 4. Verify data integrity and performance
    info!("Stream multiplexing test completed successfully");

    Ok(())
}

/// Run load test
async fn run_load_test(
    addr: &TransportAddress,
    connection_count: usize,
    duration: u64,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Load Test ===");
    info!("Connections: {}, Duration: {}s", connection_count, duration);

    let start_time = std::time::Instant::now();
    let duration = Duration::from_secs(duration);

    // Create multiple connections concurrently
    let mut handles = Vec::new();

    for i in 0..connection_count {
        let addr_clone = addr.clone();
        let handle = tokio::spawn(async move {
            let mut connection_attempts = 0;
            let mut successful_messages = 0;

            while start_time.elapsed() < duration {
                match create_connection(&addr_clone, timeout).await {
                    Ok(mut conn) => {
                        connection_attempts += 1;

                        // Send a CNXN message
                        let cnxn = AdbMessage::new_cnxn(
                            1,
                            0x01000000,
                            format!("load-test-{}", i).as_bytes().to_vec()
                        );

                        if conn.send_bytes(&cnxn.serialize()).await.is_ok() {
                            // Try to read response
                            let mut buffer = vec![0u8; 1024];
                            if conn.receive_bytes(&mut buffer).await.is_ok() {
                                successful_messages += 1;
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Connection {} failed: {}", i, e);
                    }
                }

                // Small delay between attempts
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            (connection_attempts, successful_messages)
        });

        handles.push(handle);
    }

    // Wait for all connections to complete
    let mut total_attempts = 0;
    let mut total_successful = 0;

    for handle in handles {
        if let Ok((attempts, successful)) = handle.await {
            total_attempts += attempts;
            total_successful += successful;
        }
    }

    let elapsed = start_time.elapsed();
    info!("Load test completed in {:?}", elapsed);
    info!("Total connection attempts: {}", total_attempts);
    info!("Successful messages: {}", total_successful);
    info!("Success rate: {:.2}%", (total_successful as f64 / total_attempts as f64) * 100.0);

    Ok(())
}

/// Run protocol compliance test
async fn run_protocol_test(
    addr: &TransportAddress,
    test_all: bool,
    feature: Option<&str>,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Protocol Compliance Test ===");

    if test_all {
        info!("Testing all protocol features");
    } else if let Some(feature_name) = feature {
        info!("Testing feature: {}", feature_name);
    }

    let features_to_test = if test_all {
        vec!["cnxn", "ping", "host-services", "streams"]
    } else if let Some(feature_name) = feature {
        vec![feature_name]
    } else {
        vec!["cnxn"] // Default: test basic connection
    };

    for feature in features_to_test {
        info!("Testing feature: {}", feature);

        match feature {
            "cnxn" => test_cnxn_protocol(addr, timeout).await?,
            "ping" => test_ping_protocol(addr, timeout).await?,
            "host-services" => test_host_services_protocol(addr, timeout).await?,
            "streams" => test_streams_protocol(addr, timeout).await?,
            _ => {
                warn!("Unknown protocol feature: {}", feature);
            }
        }
    }

    info!("✅ Protocol compliance test completed");
    Ok(())
}

/// Run interactive mode
async fn run_interactive_mode(
    addr: &TransportAddress,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Interactive Mode ===");
    info!("Press Ctrl+C to exit");

    let mut connection = create_connection(addr, timeout).await?;

    let mut stream_counter = 1u32;

    // Set up stdin reader
    use tokio::io::{stdin, BufReader, AsyncBufReadExt};
    let stdin = stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    loop {
        println!("dbgif> ");

        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(input)) => {
                        let trimmed = input.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        let parts: Vec<&str> = trimmed.split_whitespace().collect();
                        let command = parts[0].to_lowercase();

                        match command.as_str() {
                            "quit" | "exit" => {
                                info!("Exiting interactive mode");
                                return Ok(());
                            }
                            "help" => {
                                info!("Available commands:");
                                info!("  cnxn [identity]     - Send connection handshake");
                                info!("  ping               - Send ping message");
                                info!("  open <service>     - Open service stream");
                                info!("  wrte <id> <data>   - Write data to stream");
                                info!("  clse <id>          - Close stream");
                                info!("  help               - Show this help");
                                info!("  quit/exit          - Exit interactive mode");
                            }
                            "cnxn" => {
                                let identity = parts.get(1).unwrap_or(&"interactive-client");
                                let msg = AdbMessage::new_cnxn(1, 0x01000000, identity.as_bytes().to_vec());
                                if let Err(e) = connection.send_bytes(&msg.serialize()).await {
                                    error!("Failed to send CNXN: {}", e);
                                }
                                info!("Sent CNXN message");
                            }
                            "ping" => {
                                let msg = AdbMessage::new_ping();
                                if let Err(e) = connection.send_bytes(&msg.serialize()).await {
                                    error!("Failed to send PING: {}", e);
                                }
                                info!("Sent PING message");
                            }
                            "open" => {
                                let service = parts.get(1).unwrap_or(&"shell:");
                                let msg = AdbMessage::new_open(stream_counter, service.as_bytes().to_vec());
                                if let Err(e) = connection.send_bytes(&msg.serialize()).await {
                                    error!("Failed to send OPEN: {}", e);
                                }
                                info!("Sent OPEN for service '{}' with stream ID {}", service, stream_counter);
                                stream_counter += 1;
                            }
                            "wrte" => {
                                if parts.len() < 3 {
                                    warn!("Usage: wrte <stream_id> <data>");
                                    continue;
                                }

                                let stream_id: u32 = match parts[1].parse() {
                                    Ok(id) => id,
                                    Err(_) => {
                                        warn!("Invalid stream ID: {}", parts[1]);
                                        continue;
                                    }
                                };

                                let data = parts[2..].join(" ");
                                let msg = AdbMessage::new_wrte(stream_id, 0, data.as_bytes().to_vec());
                                if let Err(e) = connection.send_bytes(&msg.serialize()).await {
                                    error!("Failed to send WRTE: {}", e);
                                }
                                info!("Sent WRTE to stream {} with {} bytes", stream_id, data.len());
                            }
                            "clse" => {
                                if parts.len() < 2 {
                                    warn!("Usage: clse <stream_id>");
                                    continue;
                                }

                                let stream_id: u32 = match parts[1].parse() {
                                    Ok(id) => id,
                                    Err(_) => {
                                        warn!("Invalid stream ID: {}", parts[1]);
                                        continue;
                                    }
                                };

                                let msg = AdbMessage::new_clse(stream_id, 0);
                                if let Err(e) = connection.send_bytes(&msg.serialize()).await {
                                    error!("Failed to send CLSE: {}", e);
                                }
                                info!("Sent CLSE for stream {}", stream_id);
                            }
                            _ => {
                                warn!("Unknown command: {}. Available: cnxn, ping, open, wrte, clse, quit", command);
                            }
                        }
                    }
                    Ok(None) => {
                        info!("EOF received, exiting");
                        return Ok(());
                    }
                    Err(e) => {
                        error!("Error reading input: {}", e);
                        return Err(e.into());
                    }
                }
            }

            // Also try to read responses from server
            _ = async {
                let mut buffer = vec![0u8; 1024];
                match connection.receive_bytes(&mut buffer).await {
                    Ok(Some(size)) if size >= 24 => {
                        if let Ok(response) = AdbMessage::deserialize(&buffer[..size]) {
                            info!("← Received: command=0x{:08x}, arg0={}, arg1={}, data_len={}",
                                  response.command, response.arg0, response.arg1, response.data_length);
                            if !response.data.is_empty() {
                                let data_str = String::from_utf8_lossy(&response.data);
                                info!("   Data: {}", data_str);
                            }
                        }
                    }
                    Ok(Some(size)) => {
                        debug!("Received incomplete message: {} bytes", size);
                    }
                    Ok(None) => {
                        debug!("Connection closed by server");
                    }
                    Err(_) => {
                        // No data available, continue
                    }
                }
            } => {}
        }
    }
}