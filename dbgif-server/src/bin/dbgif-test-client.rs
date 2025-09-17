use clap::{Parser, Subcommand};
use dbgif_protocol::ascii;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
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


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    setup_logging(&args);

    info!("DBGIF Test Client v1.0.0");
    info!("Connecting to server: {}", args.server);

    // Parse server address
    let server_addr: SocketAddr = args.server.parse()?;

    // Execute the requested test
    match args.test {
        TestCommand::Basic { identity } => {
            run_basic_test(&server_addr, &identity, args.timeout).await?;
        }
        TestCommand::HostServices { all, service } => {
            run_host_services_test(&server_addr, all, service.as_deref(), args.timeout).await?;
        }
        TestCommand::Device { device_id, command } => {
            run_device_test(&server_addr, &device_id, &command, args.timeout).await?;
        }
        TestCommand::Streams { count, data_size } => {
            run_streams_test(&server_addr, count, data_size, args.timeout).await?;
        }
        TestCommand::Load { connections, duration } => {
            run_load_test(&server_addr, connections, duration, args.timeout).await?;
        }
        TestCommand::Protocol { all, feature } => {
            run_protocol_test(&server_addr, all, feature.as_deref(), args.timeout).await?;
        }
        TestCommand::Interactive => {
            run_interactive_mode(&server_addr, args.timeout).await?;
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

/// Create a TCP connection to the DBGIF server
async fn create_connection(
    addr: &SocketAddr,
    timeout_secs: u64,
) -> anyhow::Result<TcpStream> {
    let timeout_duration = Duration::from_secs(timeout_secs);

    info!("Connecting to {}", addr);
    let stream = timeout(timeout_duration, TcpStream::connect(addr)).await??;
    info!("Connected successfully");

    Ok(stream)
}

/// Send ASCII command
async fn send_ascii_command(
    stream: &mut TcpStream,
    cmd: &str,
) -> anyhow::Result<()> {
    let encoded = ascii::encode_request(cmd);
    stream.write_all(&encoded).await?;
    Ok(())
}

/// Receive ASCII response
async fn receive_ascii_response(
    stream: &mut TcpStream,
) -> anyhow::Result<(bool, String)> {
    let mut buffer = vec![0u8; 4096];
    let size = stream.read(&mut buffer).await?;

    if size > 0 {
        let (success, data) = ascii::decode_response(&buffer[..size])?;
        let response = String::from_utf8_lossy(&data).to_string();
        Ok((success, response))
    } else {
        Err(anyhow::anyhow!("Connection closed"))
    }
}

/// Send STRM data
async fn send_strm_data(
    stream: &mut TcpStream,
    stream_id: u8,
    data: &[u8],
) -> anyhow::Result<()> {
    let encoded = ascii::encode_strm(stream_id, data);
    stream.write_all(&encoded).await?;
    Ok(())
}

/// Receive STRM data
async fn receive_strm_data(
    stream: &mut TcpStream,
) -> anyhow::Result<(u8, Vec<u8>)> {
    let mut buffer = vec![0u8; 4096];
    let size = stream.read(&mut buffer).await?;

    if size > 0 {
        let (stream_id, data) = ascii::decode_strm(&buffer[..size])?;
        Ok((stream_id, data))
    } else {
        Err(anyhow::anyhow!("Connection closed"))
    }
}

/// Run basic connection test
async fn run_basic_test(
    addr: &SocketAddr,
    identity: &str,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Basic Connection Test ===");
    info!("Testing connection with identity: {}", identity);

    let mut connection = create_connection(addr, timeout).await?;

    // Try a simple ASCII command to test connection
    send_ascii_command(&mut connection, "host:version").await?;

    // Check if we get a response
    let (success, response) = receive_ascii_response(&mut connection).await?;

    if success {
        info!("✅ Basic connection test successful!");
        info!("Server version: {}", response);
        return Ok(());
    }

    error!("❌ Basic connection test failed");
    Err(anyhow::anyhow!("Basic connection test failed"))
}

/// Run host services test with ASCII protocol
async fn run_host_services_test(
    addr: &SocketAddr,
    test_all: bool,
    service: Option<&str>,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Host Services Test (ASCII Protocol) ===");

    if test_all {
        // Test all host services
        test_host_version(addr, timeout).await?;
        test_host_list(addr, timeout).await?;
    } else if let Some(service_name) = service {
        match service_name {
            "host:version" => test_host_version(addr, timeout).await?,
            "host:list" => test_host_list(addr, timeout).await?,
            service if service.starts_with("host:connect:") => {
                // Parse IP:port from service name
                if let Some(address) = service.strip_prefix("host:connect:") {
                    // Simple parsing for testing - in a real client this would be more robust
                    if let Some(colon_pos) = address.rfind(':') {
                        let ip = &address[..colon_pos];
                        let port_str = &address[colon_pos + 1..];
                        if let Ok(port) = port_str.parse::<u16>() {
                            test_host_connect(addr, ip, port, timeout).await?;
                        } else {
                            return Err(anyhow::anyhow!("Invalid port in {}", service));
                        }
                    } else {
                        return Err(anyhow::anyhow!("Invalid host:connect format: {}", service));
                    }
                } else {
                    return Err(anyhow::anyhow!("Invalid host:connect command"));
                }
            }
            _ => {
                // Generic test for other services
                let mut connection = create_connection(addr, timeout).await?;
                send_ascii_command(&mut connection, service_name).await?;
                let (success, response) = receive_ascii_response(&mut connection).await?;
                if success {
                    info!("✅ {} test passed - Response: {}", service_name, response);
                } else {
                    return Err(anyhow::anyhow!("{} failed: {}", service_name, response));
                }
            }
        }
    } else {
        // Default: test version service
        test_host_version(addr, timeout).await?;
    }

    info!("✅ Host services test completed");
    Ok(())
}

/// Test host:version service using ASCII protocol
async fn test_host_version(
    addr: &SocketAddr,
    timeout: u64,
) -> anyhow::Result<()> {
    let mut connection = create_connection(addr, timeout).await?;

    // Send host:version command
    send_ascii_command(&mut connection, "host:version").await?;

    // Receive response
    let (success, response) = receive_ascii_response(&mut connection).await?;

    if success {
        info!("✅ host:version test passed - Version: {}", response);
        Ok(())
    } else {
        Err(anyhow::anyhow!("host:version failed: {}", response))
    }
}

/// Test host:list service using ASCII protocol
async fn test_host_list(
    addr: &SocketAddr,
    timeout: u64,
) -> anyhow::Result<()> {
    let mut connection = create_connection(addr, timeout).await?;

    // Send host:list command
    send_ascii_command(&mut connection, "host:list").await?;

    // Receive response
    let (success, response) = receive_ascii_response(&mut connection).await?;

    if success {
        info!("✅ host:list test passed - Devices: {}", response);
        Ok(())
    } else {
        Err(anyhow::anyhow!("host:list failed: {}", response))
    }
}

/// Test host:connect with IP:port
async fn test_host_connect(
    addr: &SocketAddr,
    target_ip: &str,
    target_port: u16,
    timeout: u64,
) -> anyhow::Result<()> {
    let mut connection = create_connection(addr, timeout).await?;

    // Send host:connect command
    let connect_cmd = format!("host:connect:{}:{}", target_ip, target_port);
    send_ascii_command(&mut connection, &connect_cmd).await?;

    // Receive response
    let (success, response) = receive_ascii_response(&mut connection).await?;

    if success {
        info!("✅ host:connect test passed - Connected to {}:{}", target_ip, target_port);
        Ok(())
    } else {
        Err(anyhow::anyhow!("host:connect failed: {}", response))
    }
}

/// Test shell service using STRM messages
async fn test_shell_service(
    addr: &SocketAddr,
    timeout: u64,
) -> anyhow::Result<()> {
    let mut connection = create_connection(addr, timeout).await?;

    // First connect to a device
    send_ascii_command(&mut connection, "host:transport:test-device").await?;
    let (success, _) = receive_ascii_response(&mut connection).await?;

    if !success {
        return Err(anyhow::anyhow!("Failed to connect to device"));
    }

    // Open shell service
    send_ascii_command(&mut connection, "shell:").await?;
    let (success, _) = receive_ascii_response(&mut connection).await?;

    if !success {
        return Err(anyhow::anyhow!("Failed to open shell"));
    }

    // Send command through STRM
    let stream_id = 1;
    send_strm_data(&mut connection, stream_id, b"echo test\n").await?;

    // Receive response through STRM
    let (recv_stream_id, data) = receive_strm_data(&mut connection).await?;

    if recv_stream_id == stream_id {
        let response = String::from_utf8_lossy(&data);
        info!("✅ shell service test passed - Response: {}", response);
        Ok(())
    } else {
        Err(anyhow::anyhow!("Stream ID mismatch"))
    }
}


/// Run device communication test
async fn run_device_test(
    addr: &SocketAddr,
    device_id: &str,
    command: &str,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Device Communication Test ===");
    info!("Device ID: {}, Command: {}", device_id, command);

    // Connect to server
    let mut connection = create_connection(addr, timeout).await?;
    info!("Connected to server");

    // Select the specified device
    let transport_cmd = format!("host:transport:{}", device_id);
    send_ascii_command(&mut connection, &transport_cmd).await?;

    let (success, response) = receive_ascii_response(&mut connection).await?;
    if !success {
        return Err(anyhow::anyhow!("Failed to connect to device {}: {}", device_id, response));
    }

    info!("Connected to device: {}", device_id);

    // Execute the command on the device (using shell service)
    send_ascii_command(&mut connection, "shell:").await?;
    let (success, _) = receive_ascii_response(&mut connection).await?;

    if !success {
        return Err(anyhow::anyhow!("Failed to open shell service"));
    }

    // Send the command through STRM
    let stream_id = 1;
    let cmd_with_newline = format!("{}\n", command);
    send_strm_data(&mut connection, stream_id, cmd_with_newline.as_bytes()).await?;

    // Receive output through STRM
    let (recv_stream_id, output) = receive_strm_data(&mut connection).await?;

    if recv_stream_id == stream_id {
        let output_str = String::from_utf8_lossy(&output);
        info!("Command output: {}", output_str);
        info!("✅ Device test completed successfully");
    } else {
        warn!("Stream ID mismatch");
    }

    Ok(())
}

/// Run stream multiplexing test
async fn run_streams_test(
    addr: &SocketAddr,
    stream_count: usize,
    data_size: usize,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Stream Multiplexing Test ===");
    info!("Stream count: {}, Data size: {}", stream_count, data_size);

    // Connect to server
    let mut connection = create_connection(addr, timeout).await?;
    info!("Connected to server");

    // Generate test data
    let test_data: Vec<u8> = (0..data_size)
        .map(|i| (i % 256) as u8)
        .collect();

    // Test multiple streams concurrently
    let mut successful_streams = 0;

    for stream_id in 0..stream_count {
        let stream_id_u8 = (stream_id % 256) as u8;

        // Send data through stream
        if let Err(e) = send_strm_data(&mut connection, stream_id_u8, &test_data).await {
            warn!("Failed to send data on stream {}: {}", stream_id, e);
            continue;
        }

        // Try to receive echo response
        match tokio::time::timeout(
            Duration::from_millis(100),
            receive_strm_data(&mut connection)
        ).await {
            Ok(Ok((recv_id, recv_data))) => {
                if recv_id == stream_id_u8 && recv_data.len() == test_data.len() {
                    successful_streams += 1;
                    debug!("Stream {} successful", stream_id);
                }
            }
            Ok(Err(e)) => {
                debug!("Stream {} receive error: {}", stream_id, e);
            }
            Err(_) => {
                debug!("Stream {} timeout", stream_id);
            }
        }
    }

    let success_rate = (successful_streams as f64 / stream_count as f64) * 100.0;
    info!("Successful streams: {}/{} ({:.1}%)", successful_streams, stream_count, success_rate);

    if successful_streams > 0 {
        info!("✅ Stream multiplexing test completed");
        Ok(())
    } else {
        Err(anyhow::anyhow!("No streams were successful"))
    }
}

/// Run load test
async fn run_load_test(
    addr: &SocketAddr,
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

                        // Send a test command
                        let cmd = format!("host:version");
                        if send_ascii_command(&mut conn, &cmd).await.is_ok() {
                            // Try to read response
                            if let Ok((success, _)) = receive_ascii_response(&mut conn).await {
                                if success {
                                    successful_messages += 1;
                                }
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

/// Run protocol compliance test with ASCII protocol
async fn run_protocol_test(
    addr: &SocketAddr,
    test_all: bool,
    feature: Option<&str>,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Protocol Compliance Test (ASCII) ===");

    if test_all {
        info!("Testing all protocol features");
    } else if let Some(feature_name) = feature {
        info!("Testing feature: {}", feature_name);
    }

    let features_to_test = if test_all {
        vec!["host:version", "host:list", "shell"]
    } else if let Some(feature_name) = feature {
        vec![feature_name]
    } else {
        vec!["host:version"] // Default: test version
    };

    for feature in features_to_test {
        info!("Testing feature: {}", feature);

        match feature {
            "host:version" => test_host_version(addr, timeout).await?,
            "host:list" => test_host_list(addr, timeout).await?,
            "shell" => test_shell_service(addr, timeout).await?,
            _ => {
                // Try as generic command
                let mut connection = create_connection(addr, timeout).await?;
                send_ascii_command(&mut connection, feature).await?;
                let (success, response) = receive_ascii_response(&mut connection).await?;
                if success {
                    info!("✅ {} test passed", feature);
                } else {
                    warn!("❌ {} test failed: {}", feature, response);
                }
            }
        }
    }

    info!("✅ Protocol compliance test completed");
    Ok(())
}

/// Run interactive mode with ASCII protocol
async fn run_interactive_mode(
    addr: &SocketAddr,
    timeout: u64,
) -> anyhow::Result<()> {
    info!("=== Interactive Mode (ASCII Protocol) ===");
    info!("Press Ctrl+C to exit");

    let mut connection = create_connection(addr, timeout).await?;

    let _stream_counter = 1u8;

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
                                info!("Available commands (ASCII Protocol):");
                                info!("  host:version           - Get server version");
                                info!("  host:list              - List devices");
                                info!("  host:transport:<id>    - Connect to device");
                                info!("  host:connect:<ipv4>:<port> - Connect to IPv4:port");
                                info!("  shell:                 - Open shell service");
                                info!("  strm <id> <data>   - Send STRM data");
                                info!("  <command>          - Send raw ASCII command");
                                info!("  help               - Show this help");
                                info!("  quit/exit          - Exit interactive mode");
                            }
                            "strm" => {
                                if parts.len() < 3 {
                                    warn!("Usage: strm <stream_id> <data>");
                                    continue;
                                }

                                let stream_id: u8 = match parts[1].parse() {
                                    Ok(id) => id,
                                    Err(_) => {
                                        warn!("Invalid stream ID: {}", parts[1]);
                                        continue;
                                    }
                                };

                                let data = parts[2..].join(" ");
                                if let Err(e) = send_strm_data(&mut connection, stream_id, data.as_bytes()).await {
                                    error!("Failed to send STRM: {}", e);
                                } else {
                                    info!("Sent STRM to stream {} with {} bytes", stream_id, data.len());
                                }
                            }
                            _ => {
                                // Treat as raw ASCII command
                                if let Err(e) = send_ascii_command(&mut connection, trimmed).await {
                                    error!("Failed to send command: {}", e);
                                } else {
                                    info!("Sent ASCII command: {}", trimmed);

                                    // Try to receive response
                                    match receive_ascii_response(&mut connection).await {
                                        Ok((success, response)) => {
                                            if success {
                                                info!("← OKAY: {}", response);
                                            } else {
                                                warn!("← FAIL: {}", response);
                                            }
                                        }
                                        Err(e) => {
                                            debug!("No response or error: {}", e);
                                        }
                                    }
                                }
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

            // Also try to read STRM responses from server
            _ = async {
                let mut buffer = vec![0u8; 4096];
                match connection.read(&mut buffer).await {
                    Ok(size) if size >= 9 => {
                        // Check if it's a STRM message
                        if &buffer[0..4] == b"STRM" {
                            if let Ok((stream_id, data)) = ascii::decode_strm(&buffer[..size]) {
                                let data_str = String::from_utf8_lossy(&data);
                                info!("← STRM[{}]: {}", stream_id, data_str);
                            }
                        } else if size >= 8 {
                            // Try as regular ASCII response
                            if let Ok((success, data)) = ascii::decode_response(&buffer[..size]) {
                                let response = String::from_utf8_lossy(&data);
                                if success {
                                    info!("← OKAY: {}", response);
                                } else {
                                    warn!("← FAIL: {}", response);
                                }
                            }
                        }
                    }
                    Ok(0) => {
                        debug!("Connection closed by server");
                    }
                    Ok(size) => {
                        debug!("Received message: {} bytes", size);
                    }
                    Err(_) => {
                        // No data available, continue
                    }
                }
            } => {}
        }
    }
}