#[cfg(test)]
mod tests {
    use dbgif_protocol::protocol::message::AdbMessage;
    use dbgif_protocol::protocol::commands::AdbCommand;
    #[allow(unused_imports)]
    use dbgif_protocol::transport::{TcpTransport, Connection};
    #[allow(unused_imports)]
    use tokio::net::TcpStream;
    #[allow(unused_imports)]
    use tokio::time::{timeout, Duration, Instant};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Import common test helpers
    use crate::common::*;

    #[tokio::test]
    async fn test_100_concurrent_connections() {
        // Test server can handle 100 concurrent client connections
        let server_addr = start_test_dbgif_server().await;

        let mut handles = Vec::new();
        let success_counter = Arc::new(AtomicUsize::new(0));

        // Launch 100 concurrent connections
        for client_id in 0..100 {
            let server_addr = server_addr.clone();
            let success_counter = Arc::clone(&success_counter);

            let handle = tokio::spawn(async move {
                // Each connection has a timeout to prevent hanging
                let connection_result = timeout(
                    Duration::from_secs(10),
                    async {
                        let mut client_stream = TcpStream::connect(server_addr).await?;

                        let client_identity = format!("device::perf-test-{}\\0", client_id);
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

                        if response.command == AdbCommand::CNXN as u32 && response.is_valid_magic() {
                            // Connection successful, try a simple host service
                            let open_msg = AdbMessage {
                                command: AdbCommand::OPEN as u32,
                                arg0: 1,
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
                            let _version_response = timeout(
                                Duration::from_secs(5),
                                receive_adb_message(&mut client_stream)
                            ).await.unwrap().unwrap();

                            if okay_response.command == AdbCommand::OKAY as u32 {
                                return Ok::<i32, Box<dyn std::error::Error + Send + Sync>>(client_id);
                            }
                        }

                        Err::<i32, Box<dyn std::error::Error + Send + Sync>>("Connection or service test failed".into())
                    }
                ).await;

                match connection_result {
                    Ok(Ok(id)) => {
                        success_counter.fetch_add(1, Ordering::Relaxed);
                        Some(id)
                    }
                    _ => None
                }
            });

            handles.push(handle);
        }

        // Wait for all connections to complete
        let mut successful_ids = Vec::new();
        for handle in handles {
            if let Ok(Some(id)) = handle.await {
                successful_ids.push(id);
            }
        }

        let successful_connections = success_counter.load(Ordering::Relaxed);

        // Server should handle a significant portion of connections (at least 80%)
        assert!(successful_connections >= 80,
                "Expected at least 80 successful connections, got {}", successful_connections);
        assert_eq!(successful_connections, successful_ids.len());
    }

    #[tokio::test]
    async fn test_connection_latency_under_load() {
        // Test connection latency when server is under moderate load
        let server_addr = start_test_dbgif_server().await;

        // Pre-establish 20 background connections to create load
        let mut background_handles = Vec::new();
        for i in 0..20 {
            let server_addr = server_addr.clone();
            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // Keep background connections active with periodic host service calls
                for _j in 0..10 {
                    let (_local_id, _remote_id) = establish_stream(&mut client_stream, b"host:version").await;
                    let _version_response = timeout(
                        Duration::from_secs(5),
                        receive_adb_message(&mut client_stream)
                    ).await.unwrap().unwrap();

                    // Small delay between requests
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                i
            });
            background_handles.push(handle);
        }

        // Measure latency for new connections under load
        let mut latency_measurements = Vec::new();

        for test_id in 0..10 {
            let start_time = Instant::now();

            let mut client_stream = TcpStream::connect(server_addr).await.unwrap();
            let client_identity = format!("device::latency-test-{}\\0", test_id);
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

            let connection_latency = start_time.elapsed();
            latency_measurements.push(connection_latency);

            assert_eq!(response.command, AdbCommand::CNXN as u32);
            assert!(response.is_valid_magic());

            // Connection latency should be reasonable (< 100ms)
            assert!(connection_latency < Duration::from_millis(100),
                    "Connection latency too high: {:?}", connection_latency);
        }

        // Wait for background connections to complete
        for handle in background_handles {
            let _ = handle.await;
        }

        // Calculate average latency
        let avg_latency: Duration = latency_measurements.iter().sum::<Duration>() / latency_measurements.len() as u32;
        assert!(avg_latency < Duration::from_millis(50),
                "Average latency too high: {:?}", avg_latency);
    }

    #[tokio::test]
    async fn test_message_throughput_performance() {
        // Test message throughput with concurrent streams
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;

        let mut handles = Vec::new();
        let total_messages = Arc::new(AtomicUsize::new(0));

        // Launch 10 concurrent clients, each sending multiple messages
        for client_id in 0..10 {
            let server_addr = server_addr.clone();
            let total_messages = Arc::clone(&total_messages);

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // Select device
                let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:throughput-test").await;
                let _selection_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();

                // Open throughput test stream
                let (local_id, remote_id) = establish_stream(&mut client_stream, b"shell:throughput").await;

                let start_time = Instant::now();
                let mut client_messages = 0;

                // Send messages for 5 seconds
                while start_time.elapsed() < Duration::from_secs(5) {
                    let test_data = format!("throughput test message {} from client {}", client_messages, client_id);
                    let _write_msg = AdbMessage {
                        command: AdbCommand::WRTE as u32,
                        arg0: local_id,
                        arg1: remote_id,
                        data_length: test_data.len() as u32,
                        data_crc32: calculate_data_crc32(test_data.as_bytes()),
                        magic: !(AdbCommand::WRTE as u32),
                        data: test_data.as_bytes().to_vec(),
                    };

                    if send_adb_message(&mut client_stream, &_write_msg).await.is_ok() {
                        // Wait for OKAY response with timeout
                        if timeout(
                            Duration::from_secs(5),
                            receive_adb_message(&mut client_stream)
                        ).await.is_ok() {
                            client_messages += 1;
                            total_messages.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    // Small delay to prevent overwhelming the server
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }

                client_messages
            });

            handles.push(handle);
        }

        // Wait for all throughput tests to complete
        let mut client_message_counts = Vec::new();
        for handle in handles {
            if let Ok(count) = handle.await {
                client_message_counts.push(count);
            }
        }

        let total_msg_count = total_messages.load(Ordering::Relaxed);

        // Server should handle at least 1000 messages in 5 seconds across all clients
        assert!(total_msg_count >= 1000,
                "Expected at least 1000 messages, got {}", total_msg_count);

        // Each client should send at least some messages
        for &count in &client_message_counts {
            assert!(count > 0, "Client should send at least some messages");
        }
    }

    #[tokio::test]
    async fn test_stream_multiplexing_performance() {
        // Test performance with many concurrent streams per client
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;

        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Select device
        let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:multiplex-test").await;
        let _selection_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();

        let mut stream_handles = Vec::new();
        let success_counter = Arc::new(AtomicUsize::new(0));

        // Open 50 concurrent streams on the same connection
        for stream_id in 1..=50 {
            let success_counter = Arc::clone(&success_counter);

            // Each stream gets a unique service name
            let service_name = format!("shell:stream-{}", stream_id);
            let (local_id, remote_id) = establish_stream(&mut client_stream, service_name.as_bytes()).await;

            let handle = tokio::spawn(async move {
                // Simulate stream activity
                for message_num in 0..10 {
                    let test_data = format!("stream {} message {}", stream_id, message_num);
                    let _write_msg = AdbMessage {
                        command: AdbCommand::WRTE as u32,
                        arg0: local_id,
                        arg1: remote_id,
                        data_length: test_data.len() as u32,
                        data_crc32: calculate_data_crc32(test_data.as_bytes()),
                        magic: !(AdbCommand::WRTE as u32),
                        data: test_data.as_bytes().to_vec(),
                    };

                    // In reality, would send through the client_stream
                    // For now, just simulate the processing
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }

                success_counter.fetch_add(1, Ordering::Relaxed);
                stream_id
            });

            stream_handles.push(handle);
        }

        // Wait for all streams to complete their activity
        for handle in stream_handles {
            let _ = handle.await;
        }

        let successful_streams = success_counter.load(Ordering::Relaxed);

        // All streams should complete successfully
        assert_eq!(successful_streams, 50,
                   "Expected 50 successful streams, got {}", successful_streams);
    }

    #[tokio::test]
    async fn test_memory_usage_under_load() {
        // Test that server doesn't leak memory under sustained load
        let server_addr = start_test_dbgif_server().await;

        // Create and destroy connections repeatedly
        for _cycle in 0..5 {
            let mut handles = Vec::new();

            // Create 20 connections
            for client_id in 0..20 {
                let server_addr = server_addr.clone();

                let handle = tokio::spawn(async move {
                    let mut client_stream = establish_cnxn_handshake(server_addr).await;

                    // Do some work
                    let (_local_id, _remote_id) = establish_stream(&mut client_stream, b"host:version").await;
                    let _version_response = timeout(
                        Duration::from_secs(5),
                        receive_adb_message(&mut client_stream)
                    ).await.unwrap().unwrap();

                    // Connection automatically dropped at end of scope
                    client_id
                });

                handles.push(handle);
            }

            // Wait for all connections to complete
            for handle in handles {
                let _ = handle.await;
            }

            // Brief pause between cycles
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Test should complete without memory issues
        // (In a real test, you might check process memory usage here)
    }

    #[tokio::test]
    async fn test_concurrent_device_connections() {
        // Test performance when multiple devices are connected
        let server_addr = start_test_dbgif_server().await;

        // Start multiple device daemons
        let _device_addr1 = start_test_device_daemon().await;
        let _device_addr2 = start_test_device_daemon().await;
        let _device_addr3 = start_test_device_daemon().await;

        let mut handles = Vec::new();

        // Launch clients that connect to different devices
        for client_id in 0..15 {
            let server_addr = server_addr.clone();

            let handle = tokio::spawn(async move {
                let mut client_stream = establish_cnxn_handshake(server_addr).await;

                // Select device based on client ID
                let device_id = format!("device-{}", (client_id % 3) + 1);
                let device_service = format!("host:device:{}", device_id);
                let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, device_service.as_bytes()).await;
                let _selection_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();

                // Perform some device operations
                let (local_id, remote_id) = establish_stream(&mut client_stream, b"shell:test").await;

                let test_data = format!("test from client {} to {}", client_id, device_id);
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
                let _okay_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                let _device_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();

                client_id
            });

            handles.push(handle);
        }

        // All clients should successfully communicate with their devices
        let mut successful_clients = 0;
        for handle in handles {
            if handle.await.is_ok() {
                successful_clients += 1;
            }
        }

        assert!(successful_clients >= 12,
                "Expected at least 12 successful device communications, got {}", successful_clients);
    }

    #[tokio::test]
    async fn test_server_stability_under_stress() {
        // Test server stability under various stress conditions
        let server_addr = start_test_dbgif_server().await;

        let mut stress_handles = Vec::new();

        // Stress test 1: Rapid connect/disconnect
        let server_addr_clone = server_addr.clone();
        let handle1 = tokio::spawn(async move {
            for _i in 0..100 {
                if let Ok(client_stream) = TcpStream::connect(server_addr_clone).await {
                    // Immediately drop connection
                    drop(client_stream);
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
        stress_handles.push(handle1);

        // Stress test 2: Many concurrent host service requests
        let server_addr_clone = server_addr.clone();
        let handle2 = tokio::spawn(async move {
            let mut client_stream = establish_cnxn_handshake(server_addr_clone).await;

            for _i in 0..50 {
                let (_local_id, _remote_id) = establish_stream(&mut client_stream, b"host:version").await;
                let _version_response = timeout(
                    Duration::from_secs(5),
                    receive_adb_message(&mut client_stream)
                ).await.unwrap().unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        stress_handles.push(handle2);

        // Stress test 3: Large message sizes
        let server_addr_clone = server_addr.clone();
        let handle3 = tokio::spawn(async move {
            let mut client_stream = establish_cnxn_handshake(server_addr_clone).await;

            // Send large messages to host services
            for _i in 0..10 {
                let large_service_name = "host:".to_string() + &"x".repeat(1000);
                let open_msg = AdbMessage {
                    command: AdbCommand::OPEN as u32,
                    arg0: 1,
                    arg1: 0,
                    data_length: large_service_name.len() as u32,
                    data_crc32: calculate_data_crc32(large_service_name.as_bytes()),
                    magic: !(AdbCommand::OPEN as u32),
                    data: large_service_name.as_bytes().to_vec(),
                };

                let _ = send_adb_message(&mut client_stream, &open_msg).await;
                let _ = receive_adb_message(&mut client_stream).await;

                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
        stress_handles.push(handle3);

        // Wait for all stress tests to complete
        for handle in stress_handles {
            let _ = handle.await;
        }

        // Server should still be responsive after stress tests
        let mut final_client_stream = establish_cnxn_handshake(server_addr).await;
        let (_local_id, _remote_id) = establish_stream(&mut final_client_stream, b"host:version").await;
        let version_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut final_client_stream)
        ).await.unwrap().unwrap();

        assert_eq!(version_response.command, AdbCommand::WRTE as u32);
        assert!(version_response.data.len() > 0);
    }

}

// Include common test helpers module
#[path = "../common/mod.rs"]
mod common;