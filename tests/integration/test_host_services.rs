#[cfg(test)]
mod tests {
    use dbgif_protocol::protocol::message::AdbMessage;
    use dbgif_protocol::protocol::commands::AdbCommand;
    #[allow(unused_imports)]
    use dbgif_protocol::transport::{TcpTransport, Connection, TransportListener};
    use tokio::net::{TcpListener, TcpStream};
    #[allow(unused_imports)]
    use tokio::time::{timeout, Duration};
    use std::net::SocketAddr;
    use dbgif_protocol::protocol::crc::calculate_crc32;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn test_host_version_service_integration() {
        // Test complete host:version service flow
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Open stream to host:version service
        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 1, // local stream id
            arg1: 0, // remote stream id (assigned by server)
            data_length: 12,
            data_crc32: calculate_data_crc32(b"host:version"),
            magic: !(AdbCommand::OPEN as u32),
            data: b"host:version".to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        // Should receive OKAY response
        let okay_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);
        assert_eq!(okay_response.arg0, 1); // our local stream id
        assert!(okay_response.arg1 > 0);    // server assigned remote stream id

        let remote_stream_id = okay_response.arg1;

        // Service should immediately respond with version data
        let version_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(version_response.command, AdbCommand::WRTE as u32);
        assert_eq!(version_response.arg0, remote_stream_id);
        assert_eq!(version_response.arg1, 1); // our local stream id

        // Verify version data
        let version_str = String::from_utf8_lossy(&version_response.data);
        assert!(version_str.contains("dbgif"));
        assert!(version_str.len() > 0 && version_str.len() < 256);
    }

    #[tokio::test]
    async fn test_host_features_service_integration() {
        // Test complete host:features service flow
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 2, // local stream id
            arg1: 0,
            data_length: 13,
            data_crc32: calculate_data_crc32(b"host:features"),
            magic: !(AdbCommand::OPEN as u32),
            data: b"host:features".to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        let okay_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

        let features_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(features_response.command, AdbCommand::WRTE as u32);

        // Verify features data contains expected capabilities
        let features_str = String::from_utf8_lossy(&features_response.data);
        assert!(features_str.contains("dbgif")); // Should include dbgif protocol support
    }

    #[tokio::test]
    async fn test_host_list_service_integration() {
        // Test host:list service for device discovery
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 3,
            arg1: 0,
            data_length: 9,
            data_crc32: calculate_data_crc32(b"host:list"),
            magic: !(AdbCommand::OPEN as u32),
            data: b"host:list".to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        let okay_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

        let list_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(list_response.command, AdbCommand::WRTE as u32);

        // List response should be valid (even if empty)
        let list_str = String::from_utf8_lossy(&list_response.data);
        // Format should be: device_id\tdevice_type\tconnection_status\n
        // List response can be empty if no devices, but should be valid
        assert!(list_str.len() < 10000); // Reasonable upper bound
    }

    #[tokio::test]
    async fn test_host_device_service_selection() {
        // Test host:device:<id> service for device selection
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let device_service = b"host:device:test-device-001";
        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 4,
            arg1: 0,
            data_length: device_service.len() as u32,
            data_crc32: calculate_data_crc32(device_service),
            magic: !(AdbCommand::OPEN as u32),
            data: device_service.to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        let okay_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

        let device_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(device_response.command, AdbCommand::WRTE as u32);

        // Device response should indicate selection status
        let response_str = String::from_utf8_lossy(&device_response.data);
        // Should contain status like "OKAY" or "device not found"
        assert!(response_str.len() > 0);
    }

    #[tokio::test]
    async fn test_multiple_host_services_concurrent() {
        // Test accessing multiple host services concurrently
        let server_addr = start_test_dbgif_server().await;

        let services = vec![
            ("host:version", 10),
            ("host:features", 11),
            ("host:list", 12),
        ];

        let mut handles = Vec::new();

        for (service_name, stream_id) in services {
            let server_addr = server_addr.clone();
            let service_name = service_name.to_string();

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                let open_msg = AdbMessage {
                    command: AdbCommand::OPEN as u32,
                    arg0: stream_id,
                    arg1: 0,
                    data_length: service_name.len() as u32,
                    data_crc32: calculate_data_crc32(service_name.as_bytes()),
                    magic: !(AdbCommand::OPEN as u32),
                    data: service_name.as_bytes().to_vec(),
                };

                send_adb_message(&mut client_stream, &open_msg).await.unwrap();
                let okay_response = receive_adb_message(&mut client_stream).await.unwrap();
                let data_response = receive_adb_message(&mut client_stream).await.unwrap();

                okay_response.command == AdbCommand::OKAY as u32 &&
                data_response.command == AdbCommand::WRTE as u32
            });

            handles.push(handle);
        }

        // All services should respond successfully
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result);
        }
    }

    #[tokio::test]
    async fn test_host_service_error_handling() {
        // Test invalid host service names
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let invalid_service = b"host:invalid-service";
        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 5,
            arg1: 0,
            data_length: invalid_service.len() as u32,
            data_crc32: calculate_data_crc32(invalid_service),
            magic: !(AdbCommand::OPEN as u32),
            data: invalid_service.to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        // Should receive CLSE (close) response for invalid service
        let response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(response.command, AdbCommand::CLSE as u32);
        assert_eq!(response.arg0, 5); // our local stream id
    }

    #[tokio::test]
    async fn test_host_service_stream_lifecycle() {
        // Test complete stream lifecycle with host service
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // 1. OPEN host:version
        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 6,
            arg1: 0,
            data_length: 12,
            data_crc32: calculate_data_crc32(b"host:version"),
            magic: !(AdbCommand::OPEN as u32),
            data: b"host:version".to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        // 2. Receive OKAY
        let okay_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);
        let remote_stream_id = okay_response.arg1;

        // 3. Receive version data
        let version_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(version_response.command, AdbCommand::WRTE as u32);

        // 4. Send CLSE to close stream
        let close_msg = AdbMessage {
            command: AdbCommand::CLSE as u32,
            arg0: 6, // our local stream id
            arg1: remote_stream_id,
            data_length: 0,
            data_crc32: 0,
            magic: !(AdbCommand::CLSE as u32),
            data: vec![],
        };

        send_adb_message(&mut client_stream, &close_msg).await.unwrap();

        // 5. Should receive CLSE acknowledgment
        let close_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(close_response.command, AdbCommand::CLSE as u32);
        assert_eq!(close_response.arg0, remote_stream_id);
        assert_eq!(close_response.arg1, 6);
    }

    #[tokio::test]
    async fn test_host_service_data_integrity() {
        // Test that host service data is properly formatted and complete
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let services_to_test = vec![
            ("host:version", "version information"),
            ("host:features", "feature list"),
        ];

        for (service_name, expected_content_type) in services_to_test {
            let open_msg = AdbMessage {
                command: AdbCommand::OPEN as u32,
                arg0: 7,
                arg1: 0,
                data_length: service_name.len() as u32,
                data_crc32: calculate_data_crc32(service_name.as_bytes()),
                magic: !(AdbCommand::OPEN as u32),
                data: service_name.as_bytes().to_vec(),
            };

            send_adb_message(&mut client_stream, &open_msg).await.unwrap();
            let _okay_response = receive_adb_message(&mut client_stream).await.unwrap();
            let data_response = receive_adb_message(&mut client_stream).await.unwrap();

            // Verify CRC32 integrity
            let expected_crc = calculate_data_crc32(&data_response.data);
            assert_eq!(data_response.data_crc32, expected_crc);

            // Verify data is valid UTF-8
            let data_str = String::from_utf8(data_response.data.clone());
            assert!(data_str.is_ok(), "Service {} data should be valid UTF-8", service_name);

            // Verify reasonable data size
            assert!(data_response.data.len() > 0 && data_response.data.len() < 4096,
                "Service {} data size should be reasonable", service_name);

            println!("✓ {} returned valid {}", service_name, expected_content_type);
        }
    }

    // Helper functions - these should be implemented in the actual integration framework

    async fn start_test_dbgif_server() -> SocketAddr {
        // For now, create a mock server that just returns a valid address
        // TODO: Implement real server startup for integration testing

        // Bind to ephemeral port to get a valid address
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();

        // Start a simple echo server for testing
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0; 1024];
                    while let Ok(n) = stream.read(&mut buffer).await {
                        if n == 0 { break; }
                        let _ = stream.write_all(&buffer[..n]).await;
                    }
                });
            }
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        server_addr
    }

    async fn establish_cnxn_handshake(server_addr: SocketAddr) -> TcpStream {
        // Connect to server
        let mut stream = TcpStream::connect(server_addr).await.unwrap();

        // Send CNXN handshake message
        let data = b"host::integration-test";
        let cnxn_msg = AdbMessage {
            command: AdbCommand::CNXN as u32,
            arg0: 0x01000000, // version
            arg1: 1024 * 1024, // max data size
            data_length: data.len() as u32,
            data_crc32: calculate_crc32(data),
            magic: !(AdbCommand::CNXN as u32),
            data: data.to_vec(),
        };

        send_adb_message(&mut stream, &cnxn_msg).await.unwrap();

        // Receive CNXN response
        let _response = receive_adb_message(&mut stream).await.unwrap();

        stream
    }

    async fn send_adb_message(stream: &mut TcpStream, message: &AdbMessage) -> Result<(), Box<dyn std::error::Error>> {
        // Serialize and send the ADB message
        let mut buffer = Vec::with_capacity(24 + message.data.len());

        // Serialize header (24 bytes)
        buffer.extend_from_slice(&message.command.to_le_bytes());
        buffer.extend_from_slice(&message.arg0.to_le_bytes());
        buffer.extend_from_slice(&message.arg1.to_le_bytes());
        buffer.extend_from_slice(&message.data_length.to_le_bytes());
        buffer.extend_from_slice(&message.data_crc32.to_le_bytes());
        buffer.extend_from_slice(&message.magic.to_le_bytes());

        // Add data payload
        buffer.extend_from_slice(&message.data);

        stream.write_all(&buffer).await?;
        stream.flush().await?;

        Ok(())
    }

    async fn receive_adb_message(stream: &mut TcpStream) -> Result<AdbMessage, Box<dyn std::error::Error>> {
        // Read header (24 bytes)
        let mut header = [0u8; 24];
        stream.read_exact(&mut header).await?;

        // Parse header
        let command = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let arg0 = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let arg1 = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        let data_length = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
        let data_crc32 = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
        let magic = u32::from_le_bytes([header[20], header[21], header[22], header[23]]);

        // Read data payload
        let mut data = vec![0u8; data_length as usize];
        if data_length > 0 {
            stream.read_exact(&mut data).await?;
        }

        Ok(AdbMessage {
            command,
            arg0,
            arg1,
            data_length,
            data_crc32,
            magic,
            data,
        })
    }

    fn calculate_data_crc32(data: &[u8]) -> u32 {
        calculate_crc32(data)
    }
}