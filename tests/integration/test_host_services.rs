#[cfg(test)]
mod tests {
    use dbgif_protocol::protocol::message::AdbMessage;
    use dbgif_protocol::protocol::commands::AdbCommand;
    #[allow(unused_imports)]
    use dbgif_protocol::transport::{TcpTransport, Connection};
    #[allow(unused_imports)]
    use tokio::net::{TcpListener, TcpStream};
    #[allow(unused_imports)]
    use tokio::time::{timeout, Duration};
    use std::net::SocketAddr;

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
        // TODO: Start actual DBGIF server instance for testing
        unimplemented!()
    }

    #[allow(unused_variables)]
    async fn establish_cnxn_handshake(server_addr: SocketAddr) -> TcpStream {
        // TODO: Perform CNXN handshake and return connected stream
        unimplemented!()
    }

    #[allow(unused_variables)]
    async fn send_adb_message(stream: &mut TcpStream, message: &AdbMessage) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement ADB message sending
        unimplemented!()
    }

    #[allow(unused_variables)]
    async fn receive_adb_message(stream: &mut TcpStream) -> Result<AdbMessage, Box<dyn std::error::Error>> {
        // TODO: Implement ADB message receiving
        unimplemented!()
    }

    #[allow(unused_variables)]
    fn calculate_data_crc32(data: &[u8]) -> u32 {
        // TODO: Implement CRC32 calculation
        unimplemented!()
    }
}