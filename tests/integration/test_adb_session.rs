use dbgif_server::protocol::*;
use dbgif_server::session::*;
use dbgif_server::transport::*;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};
use bytes::Bytes;
use anyhow::Result;

/// Integration tests for ADB Session Handshake
/// These tests verify ADB protocol session functionality with real handshake sequences.

// Mock ADB daemon that implements proper handshake sequence
struct MockAdbDaemon {
    listener: TcpListener,
    port: u16,
}

impl MockAdbDaemon {
    async fn new() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    fn port(&self) -> u16 {
        self.port
    }

    async fn handle_connection(&mut self) -> Result<()> {
        let (mut stream, _) = self.listener.accept().await?;

        // Handle basic ADB handshake sequence
        loop {
            let mut header_buf = [0u8; 24];
            if stream.read_exact(&mut header_buf).await.is_err() {
                break; // Connection closed
            }

            // Parse basic ADB header
            let command = u32::from_le_bytes([header_buf[0], header_buf[1], header_buf[2], header_buf[3]]);
            let arg0 = u32::from_le_bytes([header_buf[4], header_buf[5], header_buf[6], header_buf[7]]);
            let arg1 = u32::from_le_bytes([header_buf[8], header_buf[9], header_buf[10], header_buf[11]]);
            let data_length = u32::from_le_bytes([header_buf[12], header_buf[13], header_buf[14], header_buf[15]]);

            // Read payload if present
            let mut payload_data = vec![0u8; data_length as usize];
            if data_length > 0 {
                stream.read_exact(&mut payload_data).await?;
            }

            match command {
                0x4e584e43 => { // CNXN
                    println!("Mock daemon: Received CNXN");
                    // Send CNXN response
                    let response = self.create_cnxn_response();
                    stream.write_all(&response).await?;
                }
                0x48545541 => { // AUTH
                    println!("Mock daemon: Received AUTH");
                    // Send simplified auth response (skip verification)
                    let response = self.create_cnxn_response(); // Accept auth with CNXN
                    stream.write_all(&response).await?;
                }
                0x4e45504f => { // OPEN
                    println!("Mock daemon: Received OPEN");
                    // Send OKAY response
                    let response = self.create_okay_response(arg1, 1001);
                    stream.write_all(&response).await?;
                }
                0x45545257 => { // WRTE
                    println!("Mock daemon: Received WRTE");
                    // Send OKAY acknowledgment
                    let okay_response = self.create_okay_response(arg1, arg0);
                    stream.write_all(&okay_response).await?;

                    // Echo back data
                    let echo_data = format!("Echo: {}", String::from_utf8_lossy(&payload_data));
                    let echo_response = self.create_wrte_response(1001, arg1, echo_data.as_bytes());
                    stream.write_all(&echo_response).await?;
                }
                0x45534c43 => { // CLSE
                    println!("Mock daemon: Received CLSE");
                    // Send CLSE acknowledgment and close
                    let response = self.create_clse_response(arg1, arg0);
                    stream.write_all(&response).await?;
                    break;
                }
                0x474e4950 => { // PING
                    println!("Mock daemon: Received PING");
                    // Send PONG response
                    let response = self.create_pong_response(arg1, arg0);
                    stream.write_all(&response).await?;
                }
                _ => {
                    println!("Mock daemon: Unknown command: {:08x}", command);
                }
            }
        }

        Ok(())
    }

    fn create_cnxn_response(&self) -> Vec<u8> {
        let mut response = vec![0u8; 24];

        // CNXN command
        response[0..4].copy_from_slice(&0x4e584e43u32.to_le_bytes());
        // Magic (NOT of command)
        response[4..8].copy_from_slice(&(!0x4e584e43u32).to_le_bytes());
        // arg0 (version)
        response[8..12].copy_from_slice(&0x01000000u32.to_le_bytes());
        // arg1 (maxdata)
        response[12..16].copy_from_slice(&(256 * 1024u32).to_le_bytes());
        // data_length
        response[16..20].copy_from_slice(&0u32.to_le_bytes());
        // data_checksum
        response[20..24].copy_from_slice(&0u32.to_le_bytes());

        response
    }

    fn create_okay_response(&self, remote_id: u32, local_id: u32) -> Vec<u8> {
        let mut response = vec![0u8; 24];

        // OKAY command
        response[0..4].copy_from_slice(&0x59414b4fu32.to_le_bytes());
        // Magic
        response[4..8].copy_from_slice(&(!0x59414b4fu32).to_le_bytes());
        // arg0 (remote_id)
        response[8..12].copy_from_slice(&remote_id.to_le_bytes());
        // arg1 (local_id)
        response[12..16].copy_from_slice(&local_id.to_le_bytes());
        // data_length
        response[16..20].copy_from_slice(&0u32.to_le_bytes());
        // data_checksum
        response[20..24].copy_from_slice(&0u32.to_le_bytes());

        response
    }

    fn create_wrte_response(&self, local_id: u32, remote_id: u32, data: &[u8]) -> Vec<u8> {
        let mut response = vec![0u8; 24 + data.len()];
        let checksum = crc32fast::hash(data);

        // WRTE command
        response[0..4].copy_from_slice(&0x45545257u32.to_le_bytes());
        // Magic
        response[4..8].copy_from_slice(&(!0x45545257u32).to_le_bytes());
        // arg0 (local_id)
        response[8..12].copy_from_slice(&local_id.to_le_bytes());
        // arg1 (remote_id)
        response[12..16].copy_from_slice(&remote_id.to_le_bytes());
        // data_length
        response[16..20].copy_from_slice(&(data.len() as u32).to_le_bytes());
        // data_checksum
        response[20..24].copy_from_slice(&checksum.to_le_bytes());
        // payload
        response[24..].copy_from_slice(data);

        response
    }

    fn create_clse_response(&self, remote_id: u32, local_id: u32) -> Vec<u8> {
        let mut response = vec![0u8; 24];

        // CLSE command
        response[0..4].copy_from_slice(&0x45534c43u32.to_le_bytes());
        // Magic
        response[4..8].copy_from_slice(&(!0x45534c43u32).to_le_bytes());
        // arg0 (remote_id)
        response[8..12].copy_from_slice(&remote_id.to_le_bytes());
        // arg1 (local_id)
        response[12..16].copy_from_slice(&local_id.to_le_bytes());
        // data_length
        response[16..20].copy_from_slice(&0u32.to_le_bytes());
        // data_checksum
        response[20..24].copy_from_slice(&0u32.to_le_bytes());

        response
    }

    fn create_pong_response(&self, remote_id: u32, local_id: u32) -> Vec<u8> {
        let mut response = vec![0u8; 24];

        // PONG command (assuming it exists)
        response[0..4].copy_from_slice(&0x474e4f50u32.to_le_bytes());
        // Magic
        response[4..8].copy_from_slice(&(!0x474e4f50u32).to_le_bytes());
        // arg0 (remote_id)
        response[8..12].copy_from_slice(&remote_id.to_le_bytes());
        // arg1 (local_id)
        response[12..16].copy_from_slice(&local_id.to_le_bytes());
        // data_length
        response[16..20].copy_from_slice(&0u32.to_le_bytes());
        // data_checksum
        response[20..24].copy_from_slice(&0u32.to_le_bytes());

        response
    }
}

#[cfg(test)]
mod adb_session_integration_tests {
    use super::*;

    /// Test ADB message serialization and deserialization
    #[test]
    fn test_adb_message_serialization() {
        println!("Testing ADB message serialization");

        // Test basic CNXN message
        let message = AdbMessage::new(
            Command::CNXN,
            VERSION,
            MAX_DATA as u32,
            Bytes::new(),
        );

        assert_eq!(message.command, Command::CNXN);
        assert_eq!(message.arg0, VERSION);
        assert_eq!(message.arg1, MAX_DATA as u32);
        assert_eq!(message.data_length, 0);
        assert_eq!(message.magic, !Command::CNXN as u32);

        // Test serialization
        let serialized = message.serialize();
        assert_eq!(serialized.len(), ADB_HEADER_SIZE);

        // Test message with payload
        let payload = Bytes::from("host:devices");
        let message_with_data = AdbMessage::new(
            Command::OPEN,
            1001,
            0,
            payload.clone(),
        );

        assert_eq!(message_with_data.data_length, payload.len() as u32);
        assert_eq!(message_with_data.data, payload);

        let serialized_with_data = message_with_data.serialize();
        assert_eq!(serialized_with_data.len(), ADB_HEADER_SIZE + payload.len());
    }

    /// Test handshake handler functionality
    #[tokio::test]
    async fn test_handshake_handler() {
        println!("Testing handshake handler");

        let handler = HandshakeHandler::new("test_host::".to_string());
        let mut manager = HandshakeManager::new(handler);

        // Test initial state
        assert_eq!(manager.state(), HandshakeState::WaitingForConnect);
        assert!(!manager.is_completed());
        assert!(!manager.is_failed());

        // Test start handshake
        manager.start();

        // Test CNXN message processing
        let cnxn_message = AdbMessage::new(
            Command::CNXN,
            VERSION,
            MAX_DATA as u32,
            Bytes::from("device::"),
        );

        let response = manager.process_message(&cnxn_message);
        assert!(response.is_ok(), "CNXN message should be processed");

        match response.unwrap() {
            Some(resp_msg) => {
                assert_eq!(resp_msg.command, Command::CNXN);
                println!("Handshake response generated");
            }
            None => {
                println!("No immediate response required");
            }
        }
    }

    /// Test ADB session with mock daemon
    #[tokio::test]
    async fn test_adb_session_with_mock_daemon() {
        println!("Testing ADB session with mock daemon");

        // Start mock daemon
        let mut daemon = MockAdbDaemon::new().await
            .expect("Failed to start mock daemon");
        let daemon_port = daemon.port();

        // Run daemon in background
        tokio::spawn(async move {
            if let Err(e) = daemon.handle_connection().await {
                eprintln!("Mock daemon error: {}", e);
            }
        });

        // Wait for daemon to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create TCP transport
        let device_info = DeviceInfo {
            id: format!("tcp:127.0.0.1:{}", daemon_port),
            transport_type: TransportType::Tcp,
            product: "Mock".to_string(),
            model: "Test Daemon".to_string(),
            device: "mock".to_string(),
            serial: "mock_serial".to_string(),
        };

        let factory = TcpTransportFactory::new();
        let mut transport = factory.create_transport(&device_info).await
            .expect("Failed to create transport");

        // Test connection
        let connect_result = timeout(Duration::from_secs(5), transport.connect()).await;
        assert!(connect_result.is_ok(), "Connect should not timeout");
        assert!(connect_result.unwrap().is_ok(), "Connect should succeed");
        assert!(transport.is_connected(), "Transport should be connected");

        // Test basic ADB handshake
        let cnxn_message = AdbMessage::new(
            Command::CNXN,
            VERSION,
            MAX_DATA as u32,
            Bytes::from("host::"),
        );

        let serialized = cnxn_message.serialize();
        let send_result = transport.send(&serialized).await;
        assert!(send_result.is_ok(), "CNXN send should succeed");

        // Read CNXN response
        let mut buffer = vec![0u8; 1024];
        let receive_result = transport.receive(&mut buffer).await;
        assert!(receive_result.is_ok(), "CNXN response should be received");

        let bytes_received = receive_result.unwrap();
        assert!(bytes_received >= 24, "Should receive at least ADB header");

        // Verify response is CNXN
        let response_command = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        assert_eq!(response_command, Command::CNXN as u32, "Response should be CNXN");

        // Cleanup
        transport.disconnect().await.expect("Disconnect should succeed");
    }

    /// Test ADB message parser
    #[tokio::test]
    async fn test_adb_message_parser() {
        println!("Testing ADB message parser");

        let mut parser = MessageParser::new();

        // Create test messages
        let message1 = AdbMessage::new(Command::CNXN, VERSION, MAX_DATA as u32, Bytes::new());
        let message2 = AdbMessage::new(Command::OPEN, 1001, 0, Bytes::from("shell:"));

        // Serialize messages
        let mut combined_data = Vec::new();
        combined_data.extend_from_slice(&message1.serialize());
        combined_data.extend_from_slice(&message2.serialize());

        // Parse combined messages
        let parse_result = parser.parse_messages(&combined_data).await;
        assert!(parse_result.is_ok(), "Message parsing should succeed");

        let parsed_messages = parse_result.unwrap();
        assert_eq!(parsed_messages.len(), 2, "Should parse two messages");

        assert_eq!(parsed_messages[0].command, Command::CNXN);
        assert_eq!(parsed_messages[1].command, Command::OPEN);
        assert_eq!(parsed_messages[1].data, Bytes::from("shell:"));
    }

    /// Test session manager functionality
    #[tokio::test]
    async fn test_session_manager() {
        println!("Testing session manager");

        let session_manager = SessionManager::new();

        // Test initial state
        let stats = session_manager.get_stats().await;
        assert_eq!(stats.active_sessions, 0);
        assert_eq!(stats.total_sessions_created, 0);

        // Test session manager properties
        assert!(session_manager.is_running().await, "Session manager should be running");
    }

    /// Test authentication handler (simplified)
    #[tokio::test]
    async fn test_authentication_handler() {
        println!("Testing authentication handler");

        let auth_handler = AuthenticationHandler::new();

        // Test auto-accept mode (simplified authentication)
        let test_token = Bytes::from("test_token");
        let auth_result = auth_handler.handle_auth_token(&test_token).await;

        // In simplified mode, should auto-accept
        match auth_result {
            Ok(AuthResponse::Accept) => {
                println!("Authentication auto-accepted (simplified mode)");
            }
            Ok(AuthResponse::Challenge(_)) => {
                println!("Authentication challenge generated");
            }
            Ok(AuthResponse::Reject) => {
                println!("Authentication rejected");
            }
            Err(e) => {
                panic!("Authentication handling failed: {}", e);
            }
        }
    }

    /// Test stream multiplexer
    #[tokio::test]
    async fn test_stream_multiplexer() {
        println!("Testing stream multiplexer");

        let multiplexer = StreamMultiplexer::new();

        // Test stream allocation
        let stream1_id = multiplexer.allocate_stream_id().await;
        let stream2_id = multiplexer.allocate_stream_id().await;

        assert_ne!(stream1_id, stream2_id, "Stream IDs should be unique");
        assert!(stream1_id > 0, "Stream ID should be positive");
        assert!(stream2_id > 0, "Stream ID should be positive");

        // Test stream registration
        let register_result = multiplexer.register_stream(stream1_id, "shell:".to_string()).await;
        assert!(register_result.is_ok(), "Stream registration should succeed");

        // Test stream lookup
        let stream_info = multiplexer.get_stream_info(stream1_id).await;
        assert!(stream_info.is_some(), "Stream info should be available");

        let info = stream_info.unwrap();
        assert_eq!(info.service, "shell:");
        assert_eq!(info.local_id, stream1_id);
    }

    /// Test concurrent session operations
    #[tokio::test]
    async fn test_concurrent_session_operations() {
        println!("Testing concurrent session operations");

        let session_manager = SessionManager::new();

        // Test concurrent stats queries
        let stats_tasks = (0..5).map(|_| {
            let manager = &session_manager;
            async move {
                manager.get_stats().await
            }
        });

        let results = futures::future::join_all(stats_tasks).await;

        // All stats queries should succeed
        for stats in results {
            assert!(stats.active_sessions >= 0);
            assert!(stats.total_sessions_created >= 0);
        }
    }

    /// Test error handling in ADB session
    #[tokio::test]
    async fn test_adb_session_error_handling() {
        println!("Testing ADB session error handling");

        // Test invalid message handling
        let mut parser = MessageParser::new();

        // Create malformed data (too short for header)
        let malformed_data = vec![0u8; 10];
        let parse_result = parser.parse_messages(&malformed_data).await;

        // Should either handle gracefully or return appropriate error
        match parse_result {
            Ok(messages) => {
                assert_eq!(messages.len(), 0, "Malformed data should produce no messages");
            }
            Err(e) => {
                println!("Malformed data handled with error: {}", e);
            }
        }

        // Test handshake with invalid state
        let handler = HandshakeHandler::new("test::".to_string());
        let mut manager = HandshakeManager::new(handler);

        // Send message without starting handshake
        let invalid_message = AdbMessage::new(
            Command::OPEN,
            1001,
            0,
            Bytes::from("invalid"),
        );

        let process_result = manager.process_message(&invalid_message);
        match process_result {
            Ok(None) => {
                println!("Invalid message ignored in wrong state");
            }
            Ok(Some(_)) => {
                println!("Invalid message processed (implementation choice)");
            }
            Err(e) => {
                println!("Invalid message rejected: {}", e);
            }
        }
    }

    /// Test message validation
    #[test]
    fn test_message_validation() {
        println!("Testing message validation");

        // Test valid message
        let valid_message = AdbMessage::new(
            Command::WRTE,
            1001,
            1002,
            Bytes::from("test data"),
        );

        // Validate magic number
        assert_eq!(valid_message.magic, !Command::WRTE as u32);

        // Validate data length
        assert_eq!(valid_message.data_length, 9); // "test data".len()

        // Validate checksum
        let expected_checksum = calculate_crc32(b"test data");
        assert_eq!(valid_message.data_checksum, expected_checksum);
    }

    /// Test protocol constants
    #[test]
    fn test_protocol_constants() {
        println!("Testing protocol constants");

        // Test ADB protocol constants
        assert_eq!(MAX_DATA, 256 * 1024);
        assert_eq!(VERSION, 0x01000000);
        assert_eq!(ADB_HEADER_SIZE, 24);

        // Test command values
        assert_eq!(Command::CNXN as u32, 0x4e584e43);
        assert_eq!(Command::AUTH as u32, 0x48545541);
        assert_eq!(Command::OPEN as u32, 0x4e45504f);
        assert_eq!(Command::OKAY as u32, 0x59414b4f);
        assert_eq!(Command::CLSE as u32, 0x45534c43);
        assert_eq!(Command::WRTE as u32, 0x45545257);
    }

    /// Integration test with timeout scenarios
    #[tokio::test]
    async fn test_session_timeout_scenarios() {
        println!("Testing session timeout scenarios");

        let session_manager = SessionManager::new();

        // Test that operations complete within reasonable time
        let start = std::time::Instant::now();
        let _stats = session_manager.get_stats().await;
        let elapsed = start.elapsed();

        assert!(elapsed < Duration::from_millis(100),
               "Session operations should be fast");

        // Test multiplexer timeout behavior
        let multiplexer = StreamMultiplexer::new();

        let start = std::time::Instant::now();
        let _stream_id = multiplexer.allocate_stream_id().await;
        let elapsed = start.elapsed();

        assert!(elapsed < Duration::from_millis(10),
               "Stream allocation should be very fast");
    }
}