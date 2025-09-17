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
    async fn test_device_selection_and_forwarding() {
        // Test complete device selection and stream forwarding flow
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;

        // Connect client to DBGIF server
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Select target device using host:device service
        let device_id = "test-device-001";
        let device_service = format!("host:device:{}", device_id);
        let (_local_id, _remote_id) = establish_stream(&mut client_stream, device_service.as_bytes()).await;

        // Device selection should succeed with timeout
        let selection_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(selection_response.command, AdbCommand::WRTE as u32);

        let response_str = String::from_utf8_lossy(&selection_response.data);
        assert!(response_str.contains("OKAY") || response_str.contains("selected"));

        // Now open a stream to a device service that should be forwarded
        let (forward_local_id, forward_remote_id) = establish_stream(&mut client_stream, b"shell:ls").await;

        // Send data that should be forwarded to the device
        let shell_command = b"ls -la";
        let write_msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: forward_local_id,
            arg1: forward_remote_id,
            data_length: shell_command.len() as u32,
            data_crc32: calculate_data_crc32(shell_command),
            magic: !(AdbCommand::WRTE as u32),
            data: shell_command.to_vec(),
        };

        send_adb_message(&mut client_stream, &write_msg).await.unwrap();

        // Should receive OKAY acknowledgment from server with timeout
        let okay_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

        // Should receive forwarded response from device with timeout
        let device_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(device_response.command, AdbCommand::WRTE as u32);
        assert_eq!(device_response.arg0, forward_remote_id);
        assert_eq!(device_response.arg1, forward_local_id);

        // Response should contain shell output (simulated)
        let output = String::from_utf8_lossy(&device_response.data);
        assert!(output.len() > 0);
    }

    #[tokio::test]
    async fn test_device_forwarding_without_selection() {
        // Test that device services fail without device selection
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Try to open device service without selecting device first
        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 1,
            arg1: 0,
            data_length: 8,
            data_crc32: calculate_data_crc32(b"shell:id"),
            magic: !(AdbCommand::OPEN as u32),
            data: b"shell:id".to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        // Should receive CLSE (error) since no device selected with timeout
        let response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(response.command, AdbCommand::CLSE as u32);
        assert_eq!(response.arg0, 1); // our local stream id
    }

    #[tokio::test]
    async fn test_device_connection_failure_handling() {
        // Test handling when device connection fails
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Select a non-existent device
        let device_service = b"host:device:non-existent-device";
        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 1,
            arg1: 0,
            data_length: device_service.len() as u32,
            data_crc32: calculate_data_crc32(device_service),
            magic: !(AdbCommand::OPEN as u32),
            data: device_service.to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        let okay_response = receive_adb_message(&mut client_stream).await.unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

        // Should receive error response for device selection with timeout
        let error_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(error_response.command, AdbCommand::WRTE as u32);

        let error_str = String::from_utf8_lossy(&error_response.data);
        assert!(error_str.contains("not found") || error_str.contains("error"));

        // Try to use device service - should fail
        let (_service_local_id, _) = establish_stream(&mut client_stream, b"shell:echo").await;

        // Server should close the stream since device not available with timeout
        let close_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(close_response.command, AdbCommand::CLSE as u32);
    }

    #[tokio::test]
    async fn test_device_stream_multiplexing() {
        // Test multiple concurrent streams to device
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Select device
        let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:test-device-001").await;
        let _selection_response = receive_adb_message(&mut client_stream).await.unwrap();

        // Open multiple concurrent streams to device
        let mut device_streams = Vec::new();
        for i in 1..=3 {
            let service_name = format!("shell:echo-{}", i);
            let (local_id, remote_id) = establish_stream(&mut client_stream, service_name.as_bytes()).await;
            device_streams.push((local_id, remote_id, i));
        }

        // Send data on each stream
        for &(local_id, remote_id, stream_num) in &device_streams {
            let test_data = format!("test data {}", stream_num);
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
        }

        // Receive responses (order may vary due to async processing) with timeout
        for _ in 0..device_streams.len() {
            let okay_response = timeout(
                Duration::from_secs(5),
                receive_adb_message(&mut client_stream)
            ).await.unwrap().unwrap();
            assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

            // Verify response belongs to one of our streams
            let stream_exists = device_streams.iter().any(|&(local, remote, _)| {
                okay_response.arg0 == remote && okay_response.arg1 == local
            });
            assert!(stream_exists);
        }
    }

    #[tokio::test]
    async fn test_device_disconnect_during_forwarding() {
        // Test handling device disconnection during active forwarding
        let server_addr = start_test_dbgif_server().await;
        let device_addr = start_test_device_daemon().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Select device and establish stream
        let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:test-device-001").await;
        let _selection_response = receive_adb_message(&mut client_stream).await.unwrap();

        let (forward_local_id, forward_remote_id) = establish_stream(&mut client_stream, b"shell:long-running").await;

        // Simulate device disconnection
        stop_test_device_daemon(device_addr).await;

        // Try to send data - should result in stream closure
        let test_data = b"data after disconnect";
        let write_msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: forward_local_id,
            arg1: forward_remote_id,
            data_length: test_data.len() as u32,
            data_crc32: calculate_data_crc32(test_data),
            magic: !(AdbCommand::WRTE as u32),
            data: test_data.to_vec(),
        };

        send_adb_message(&mut client_stream, &write_msg).await.unwrap();

        // Should receive CLSE indicating device disconnection with timeout
        let response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(response.command, AdbCommand::CLSE as u32);
        assert_eq!(response.arg0, forward_remote_id);
        assert_eq!(response.arg1, forward_local_id);
    }

    #[tokio::test]
    async fn test_forwarding_data_integrity() {
        // Test that data forwarded to device maintains integrity
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Select device
        let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:test-device-001").await;
        let _selection_response = receive_adb_message(&mut client_stream).await.unwrap();

        // Open stream to echo service (device should echo back data)
        let (local_id, remote_id) = establish_stream(&mut client_stream, b"shell:echo").await;

        // Send binary data to test integrity
        let binary_data = vec![0x00, 0xFF, 0xAA, 0x55, 0x42, 0x13, 0x37, 0x99, 0xDE, 0xAD, 0xBE, 0xEF];
        let expected_crc = calculate_data_crc32(&binary_data);

        let write_msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: local_id,
            arg1: remote_id,
            data_length: binary_data.len() as u32,
            data_crc32: expected_crc,
            magic: !(AdbCommand::WRTE as u32),
            data: binary_data.clone(),
        };

        send_adb_message(&mut client_stream, &write_msg).await.unwrap();

        // Receive OKAY with timeout
        let okay_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

        // Receive echoed data from device with timeout
        let echo_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(echo_response.command, AdbCommand::WRTE as u32);
        assert_eq!(echo_response.data, binary_data);
        assert_eq!(echo_response.data_crc32, expected_crc);
    }

    #[tokio::test]
    async fn test_device_service_discovery() {
        // Test discovery of services available on selected device
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Select device
        let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:test-device-001").await;
        let _selection_response = receive_adb_message(&mut client_stream).await.unwrap();

        // Query device services (this might be a custom DBGIF extension)
        let (_local_id, _remote_id) = establish_stream(&mut client_stream, b"host:services").await;

        // Should receive list of available device services with timeout
        let services_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(services_response.command, AdbCommand::WRTE as u32);

        let services_str = String::from_utf8_lossy(&services_response.data);
        // Should contain common shell services
        assert!(services_str.contains("shell") || services_str.len() > 0);
    }

    #[tokio::test]
    async fn test_bidirectional_device_forwarding() {
        // Test bidirectional data flow through device forwarding
        let server_addr = start_test_dbgif_server().await;
        let _device_addr = start_test_device_daemon().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Select device and establish forwarding stream
        let (_device_local_id, _device_remote_id) = establish_stream(&mut client_stream, b"host:device:test-device-001").await;
        let _selection_response = receive_adb_message(&mut client_stream).await.unwrap();

        let (local_id, remote_id) = establish_stream(&mut client_stream, b"shell:interactive").await;

        // Send initial command
        let command = b"pwd";
        let write_msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: local_id,
            arg1: remote_id,
            data_length: command.len() as u32,
            data_crc32: calculate_data_crc32(command),
            magic: !(AdbCommand::WRTE as u32),
            data: command.to_vec(),
        };

        send_adb_message(&mut client_stream, &write_msg).await.unwrap();

        // Receive OKAY and response with timeout
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

        // Send follow-up command
        let command2 = b"whoami";
        let write_msg2 = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: local_id,
            arg1: remote_id,
            data_length: command2.len() as u32,
            data_crc32: calculate_data_crc32(command2),
            magic: !(AdbCommand::WRTE as u32),
            data: command2.to_vec(),
        };

        send_adb_message(&mut client_stream, &write_msg2).await.unwrap();

        // Should receive responses for both commands with timeout
        let _okay_response2 = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        let device_response2 = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();

        assert_eq!(device_response2.command, AdbCommand::WRTE as u32);
        assert!(device_response2.data.len() > 0);
    }

}

// Include common test helpers module
#[path = "../common/mod.rs"]
mod common;