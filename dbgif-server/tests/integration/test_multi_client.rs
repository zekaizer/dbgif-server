#[cfg(test)]
mod tests {
    use dbgif_protocol::protocol::message::AdbMessage;
    use dbgif_protocol::protocol::commands::AdbCommand;
    #[allow(unused_imports)]
    use dbgif_protocol::transport::{TcpTransport, Connection};
    #[allow(unused_imports)]
    use tokio::net::TcpStream;
    #[allow(unused_imports)]
    use tokio::time::{timeout, Duration};

    // Import common test helpers
    use crate::common::*;

    #[tokio::test]
    async fn test_multiple_clients_concurrent_connections() {
        // Test that multiple clients can connect and perform handshakes concurrently
        let server_addr = start_test_dbgif_server().await;

        let mut handles = Vec::new();

        // Launch 10 concurrent client connections
        for client_id in 0..10 {
            let server_addr = server_addr.clone();
            let handle = tokio::spawn(async move {
                let mut client_stream = TcpStream::connect(server_addr).await.unwrap();

                // Perform CNXN handshake
                let client_identity = format!("device::multi-client-test-{}\\0", client_id);
                let cnxn_request = AdbMessage::new_cnxn(
                    0x01000000,
                    0x100000,
                    client_identity.as_bytes().to_vec()
                );

                send_adb_message(&mut client_stream, &cnxn_request).await.unwrap();
                let response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();

                assert_eq!(response.command, AdbCommand::CNXN as u32);
                assert!(response.is_valid_magic());

                client_id
            });

            handles.push(handle);
        }

        // All clients should successfully connect
        for handle in handles {
            let client_id = handle.await.unwrap();
            assert!(client_id < 10);
        }
    }

    #[tokio::test]
    async fn test_multiple_clients_host_services_access() {
        // Test that multiple clients can access host services concurrently
        let server_addr = start_test_dbgif_server().await;

        let host_services = vec![
            "host:version",
            "host:features",
            "host:list",
        ];

        let mut handles = Vec::new();

        for (client_id, service_name) in host_services.iter().enumerate() {
            let server_addr = server_addr.clone();
            let service_name = service_name.to_string();

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // Access host service
                let open_msg = AdbMessage {
                    command: AdbCommand::OPEN as u32,
                    arg0: (client_id + 1) as u32,
                    arg1: 0,
                    data_length: service_name.len() as u32,
                    data_crc32: calculate_data_crc32(service_name.as_bytes()),
                    magic: !(AdbCommand::OPEN as u32),
                    data: service_name.as_bytes().to_vec(),
                };

                send_adb_message(&mut client_stream, &open_msg).await.unwrap();

                // Should receive OKAY with timeout
                let okay_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

                // Should receive service data with timeout
                let data_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                assert_eq!(data_response.command, AdbCommand::WRTE as u32);
                assert!(data_response.data.len() > 0);

                service_name
            });

            handles.push(handle);
        }

        // All clients should successfully access their host services
        for handle in handles {
            let service_name = handle.await.unwrap();
            assert!(service_name.starts_with("host:"));
        }
    }

    #[tokio::test]
    async fn test_multiple_clients_device_selection() {
        // Test that multiple clients can select different devices
        let server_addr = start_test_dbgif_server().await;
        let _device_addr1 = start_test_device_daemon().await;
        let _device_addr2 = start_test_device_daemon().await;

        let device_ids = vec![
            "test-device-001",
            "test-device-002",
        ];

        let mut handles = Vec::new();

        for (_client_id, device_id) in device_ids.iter().enumerate() {
            let server_addr = server_addr.clone();
            let device_id = device_id.to_string();

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // Select device
                let device_service = format!("host:device:{}", device_id);
                let (_local_id, _remote_id) = establish_stream(&mut client_stream, device_service.as_bytes()).await;

                // Should receive device selection response with timeout
                let selection_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                assert_eq!(selection_response.command, AdbCommand::WRTE as u32);

                let response_str = String::from_utf8_lossy(&selection_response.data);
                assert!(response_str.contains("OKAY") || response_str.contains("selected"));

                device_id
            });

            handles.push(handle);
        }

        // All clients should successfully select their devices
        for handle in handles {
            let device_id = handle.await.unwrap();
            assert!(device_id.starts_with("test-device-"));
        }
    }

    #[tokio::test]
    async fn test_multiple_clients_stream_independence() {
        // Test that streams from different clients are independent
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;

        let mut handles = Vec::new();

        // Launch 5 clients, each with their own stream
        for client_id in 0..5 {
            let server_addr = server_addr.clone();

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // Select device
                let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:test-device-001").await;
                let _selection_response = receive_adb_message(&mut client_stream).await.unwrap();

                // Open shell stream
                let (local_id, remote_id) = establish_stream(&mut client_stream, b"shell:echo").await;

                // Send unique data for this client
                let unique_data = format!("client-{}-data", client_id);
                let write_msg = AdbMessage {
                    command: AdbCommand::WRTE as u32,
                    arg0: local_id,
                    arg1: remote_id,
                    data_length: unique_data.len() as u32,
                    data_crc32: calculate_data_crc32(unique_data.as_bytes()),
                    magic: !(AdbCommand::WRTE as u32),
                    data: unique_data.as_bytes().to_vec(),
                };

                send_adb_message(&mut client_stream, &write_msg).await.unwrap();

                // Receive OKAY with timeout
                let _okay_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();

                // Should receive echoed data for this specific client with timeout
                let echo_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                assert_eq!(echo_response.command, AdbCommand::WRTE as u32);

                let echoed_data = String::from_utf8_lossy(&echo_response.data);
                assert!(echoed_data.contains(&format!("client-{}", client_id)));

                client_id
            });

            handles.push(handle);
        }

        // All clients should receive their own unique echoed data
        for handle in handles {
            let client_id = handle.await.unwrap();
            assert!(client_id < 5);
        }
    }

    #[tokio::test]
    async fn test_multiple_clients_shared_device_access() {
        // Test that multiple clients can share access to the same device
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;

        let mut handles = Vec::new();

        // Launch 3 clients accessing the same device
        for client_id in 0..3 {
            let server_addr = server_addr.clone();

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // All clients select the same device
                let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:shared-device").await;
                let _selection_response = receive_adb_message(&mut client_stream).await.unwrap();

                // Each client opens a different service on the same device
                let service_name = format!("shell:service-{}", client_id);
                let (local_id, remote_id) = establish_stream(&mut client_stream, service_name.as_bytes()).await;

                // Send request
                let request_data = format!("request from client {}", client_id);
                let write_msg = AdbMessage {
                    command: AdbCommand::WRTE as u32,
                    arg0: local_id,
                    arg1: remote_id,
                    data_length: request_data.len() as u32,
                    data_crc32: calculate_data_crc32(request_data.as_bytes()),
                    magic: !(AdbCommand::WRTE as u32),
                    data: request_data.as_bytes().to_vec(),
                };

                send_adb_message(&mut client_stream, &write_msg).await.unwrap();

                // Receive responses with timeout
                let _okay_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                let device_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();

                assert_eq!(device_response.command, AdbCommand::WRTE as u32);
                assert!(device_response.data.len() > 0);

                client_id
            });

            handles.push(handle);
        }

        // All clients should successfully communicate with the shared device
        for handle in handles {
            let client_id = handle.await.unwrap();
            assert!(client_id < 3);
        }
    }

    #[tokio::test]
    async fn test_client_disconnect_doesnt_affect_others() {
        // Test that one client disconnecting doesn't affect other clients
        let server_addr = start_test_dbgif_server().await;

        // Start 3 clients
        let mut client_streams = Vec::new();
        for client_id in 0..3 {
            let mut client_stream = establish_cnxn_handshake(server_addr).await;

            // Each client opens a host service stream
            let service_name = if client_id == 0 { "host:version" }
                              else if client_id == 1 { "host:features" }
                              else { "host:list" };

            let (_local_id, _remote_id) = establish_stream(&mut client_stream, service_name.as_bytes()).await;
            let _service_response = receive_adb_message(&mut client_stream).await.unwrap();

            client_streams.push(client_stream);
        }

        // Disconnect the middle client (client 1)
        drop(client_streams.remove(1));

        // Remaining clients should still be able to access services
        for (i, mut client_stream) in client_streams.into_iter().enumerate() {
            let service_name = if i == 0 { "host:version" } else { "host:list" };

            let (_local_id, _remote_id) = establish_stream(&mut client_stream, service_name.as_bytes()).await;
            let service_response = timeout(
                Duration::from_secs(5),
                receive_adb_message(&mut client_stream)
            ).await.unwrap().unwrap();

            assert_eq!(service_response.command, AdbCommand::WRTE as u32);
            assert!(service_response.data.len() > 0);
        }
    }

    #[tokio::test]
    async fn test_multiple_clients_stream_id_isolation() {
        // Test that stream IDs are properly isolated between clients
        let server_addr = start_test_dbgif_server().await;

        let mut handles = Vec::new();

        // Launch multiple clients that use the same local stream IDs
        for _client_id in 0..5 {
            let server_addr = server_addr.clone();

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // Each client uses the same local stream ID (1)
                let open_msg = AdbMessage {
                    command: AdbCommand::OPEN as u32,
                    arg0: 1, // Same local stream ID for all clients
                    arg1: 0,
                    data_length: 12,
                    data_crc32: calculate_data_crc32(b"host:version"),
                    magic: !(AdbCommand::OPEN as u32),
                    data: b"host:version".to_vec(),
                };

                send_adb_message(&mut client_stream, &open_msg).await.unwrap();

                let okay_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                assert_eq!(okay_response.command, AdbCommand::OKAY as u32);
                assert_eq!(okay_response.arg0, 1); // Our local stream ID
                assert!(okay_response.arg1 > 0);    // Server assigned remote stream ID

                let version_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                assert_eq!(version_response.command, AdbCommand::WRTE as u32);
                assert_eq!(version_response.arg1, 1); // Our local stream ID

                // Return the server-assigned remote stream ID
                okay_response.arg1
            });

            handles.push(handle);
        }

        let mut remote_stream_ids = Vec::new();
        for handle in handles {
            let remote_id = handle.await.unwrap();
            remote_stream_ids.push(remote_id);
        }

        // All remote stream IDs should be different (server isolates clients)
        remote_stream_ids.sort();
        remote_stream_ids.dedup();
        assert_eq!(remote_stream_ids.len(), 5); // All should be unique
    }

    #[tokio::test]
    async fn test_concurrent_device_forwarding_multiple_clients() {
        // Test that device forwarding works correctly with multiple concurrent clients
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;

        let mut handles = Vec::new();

        // Launch 4 clients that will all forward to the same device
        for client_id in 0..4 {
            let server_addr = server_addr.clone();

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // Select device
                let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:concurrent-test").await;
                let _selection_response = receive_adb_message(&mut client_stream).await.unwrap();

                // Open forwarding stream
                let service_name = format!("shell:concurrent-{}", client_id);
                let (local_id, remote_id) = establish_stream(&mut client_stream, service_name.as_bytes()).await;

                // Send data
                let test_data = format!("concurrent test data from client {}", client_id);
                let write_msg = AdbMessage {
                    command: AdbCommand::WRTE as u32,
                    arg0: local_id,
                    arg1: remote_id,
                    data_length: test_data.len() as u32,
                    data_crc32: calculate_data_crc32(test_data.as_bytes()),
                    magic: !(AdbCommand::WRTE as u32),
                    data: test_data.as_bytes().to_vec(),
                };

                send_adb_message(&mut client_stream, &write_msg).await.unwrap();

                // Receive OKAY and device response with timeout
                let _okay_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                let device_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();

                assert_eq!(device_response.command, AdbCommand::WRTE as u32);
                let response_data = String::from_utf8_lossy(&device_response.data);
                assert!(response_data.contains(&client_id.to_string()));

                client_id
            });

            handles.push(handle);
        }

        // All clients should successfully communicate through device forwarding
        for handle in handles {
            let client_id = handle.await.unwrap();
            assert!(client_id < 4);
        }
    }

    #[tokio::test]
    async fn test_client_limits_and_backpressure() {
        // Test server behavior under high client load
        let server_addr = start_test_dbgif_server().await;

        let mut successful_connections = 0;
        let mut handles = Vec::new();

        // Try to establish many concurrent connections
        for client_id in 0..50 {
            let server_addr = server_addr.clone();

            let handle = tokio::spawn(async move {
                // Some connections might fail or timeout under high load
                let connection_result = timeout(
                    Duration::from_secs(5),
                    TcpStream::connect(server_addr)
                ).await;

                if let Ok(Ok(mut client_stream)) = connection_result {
                    let client_identity = format!("device::load-test-{}\\0", client_id);
                    let cnxn_request = AdbMessage::new_cnxn(
                        0x01000000,
                        0x100000,
                        client_identity.as_bytes().to_vec()
                    );

                    if send_adb_message(&mut client_stream, &cnxn_request).await.is_ok() {
                        if let Ok(Ok(response)) = timeout(
                            Duration::from_secs(5),
                            receive_adb_message(&mut client_stream)
                        ).await {
                            if response.command == AdbCommand::CNXN as u32 {
                                return true; // Successful connection
                            }
                        }
                    }
                }
                false // Failed connection
            });

            handles.push(handle);
        }

        for handle in handles {
            if let Ok(success) = handle.await {
                if success {
                    successful_connections += 1;
                }
            }
        }

        // Server should handle at least some connections successfully
        // (exact number depends on server limits and system resources)
        assert!(successful_connections > 0);
        assert!(successful_connections <= 50);
    }

}

// Include common test helpers module
#[path = "../common/mod.rs"]
mod common;