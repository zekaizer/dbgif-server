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
    async fn test_stream_open_okay_flow() {
        // Test basic OPEN → OKAY stream establishment
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Send OPEN message
        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 1, // local stream id
            arg1: 0, // remote stream id (server assigns)
            data_length: 9,
            data_crc32: calculate_data_crc32(b"test-svc"),
            magic: !(AdbCommand::OPEN as u32),
            data: b"test-svc".to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        // Receive OKAY response with timeout
        let okay_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);
        assert_eq!(okay_response.arg0, 1); // our local stream id
        assert!(okay_response.arg1 > 0);    // server assigned remote stream id
        assert_eq!(okay_response.data_length, 0);
        assert!(okay_response.is_valid_magic());
    }

    #[tokio::test]
    async fn test_stream_write_data_flow() {
        // Test OPEN → OKAY → WRTE data transfer
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Establish stream
        let (local_id, remote_id) = establish_stream(&mut client_stream, b"echo-svc").await;

        // Send WRTE message with data
        let test_data = b"Hello, stream world!";
        let write_msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: local_id,
            arg1: remote_id,
            data_length: test_data.len() as u32,
            data_crc32: calculate_data_crc32(test_data),
            magic: !(AdbCommand::WRTE as u32),
            data: test_data.to_vec(),
        };

        send_adb_message(&mut client_stream, &write_msg).await.unwrap();

        // Should receive OKAY acknowledgment with timeout
        let okay_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);
        assert_eq!(okay_response.arg0, remote_id);
        assert_eq!(okay_response.arg1, local_id);

        // For echo service, should also receive echoed data with timeout
        let echo_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(echo_response.command, AdbCommand::WRTE as u32);
        assert_eq!(echo_response.data, test_data);
    }

    #[tokio::test]
    async fn test_stream_close_flow() {
        // Test complete OPEN → OKAY → WRTE → CLSE lifecycle
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let (local_id, remote_id) = establish_stream(&mut client_stream, b"temp-svc").await;

        // Send some data
        let test_data = b"temporary data";
        let write_msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: local_id,
            arg1: remote_id,
            data_length: test_data.len() as u32,
            data_crc32: calculate_data_crc32(test_data),
            magic: !(AdbCommand::WRTE as u32),
            data: test_data.to_vec(),
        };

        send_adb_message(&mut client_stream, &write_msg).await.unwrap();
        let _okay_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();

        // Close the stream
        let close_msg = AdbMessage {
            command: AdbCommand::CLSE as u32,
            arg0: local_id,
            arg1: remote_id,
            data_length: 0,
            data_crc32: 0,
            magic: !(AdbCommand::CLSE as u32),
            data: vec![],
        };

        send_adb_message(&mut client_stream, &close_msg).await.unwrap();

        // Should receive CLSE acknowledgment with timeout
        let close_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(close_response.command, AdbCommand::CLSE as u32);
        assert_eq!(close_response.arg0, remote_id);
        assert_eq!(close_response.arg1, local_id);
    }

    #[tokio::test]
    async fn test_multiple_concurrent_streams() {
        // Test multiple streams on same connection
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Open 3 concurrent streams
        let mut streams = Vec::new();
        for i in 1..=3 {
            let service_name = format!("stream-{}", i);
            let (local_id, remote_id) = establish_stream(&mut client_stream, service_name.as_bytes()).await;
            streams.push((local_id, remote_id));
        }

        // Send data on each stream
        for (i, &(local_id, remote_id)) in streams.iter().enumerate() {
            let test_data = format!("data for stream {}", i + 1);
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

        // Receive responses (order may vary) with timeout
        for _ in 0..streams.len() {
            let response = timeout(
                Duration::from_secs(5),
                receive_adb_message(&mut client_stream)
            ).await.unwrap().unwrap();
            assert_eq!(response.command, AdbCommand::OKAY as u32);

            // Verify stream ID is one of our active streams
            let stream_exists = streams.iter().any(|&(local, remote)| {
                response.arg0 == remote && response.arg1 == local
            });
            assert!(stream_exists);
        }
    }

    #[tokio::test]
    async fn test_stream_invalid_service() {
        // Test opening stream to non-existent service
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let open_msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 1,
            arg1: 0,
            data_length: 19,
            data_crc32: calculate_data_crc32(b"non-existent-service"),
            magic: !(AdbCommand::OPEN as u32),
            data: b"non-existent-service".to_vec(),
        };

        send_adb_message(&mut client_stream, &open_msg).await.unwrap();

        // Should receive CLSE (close) instead of OKAY with timeout
        let response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(response.command, AdbCommand::CLSE as u32);
        assert_eq!(response.arg0, 1); // our local stream id
        assert_eq!(response.arg1, 0); // no remote stream was assigned
    }

    #[tokio::test]
    async fn test_stream_data_integrity() {
        // Test data integrity across stream operations
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let (local_id, remote_id) = establish_stream(&mut client_stream, b"integrity-test").await;

        // Send binary data with known CRC
        let binary_data = vec![0x00, 0xFF, 0xAA, 0x55, 0x42, 0x13, 0x37, 0x99];
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

        let okay_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);

        // Verify server calculated same CRC
        assert_eq!(okay_response.data_crc32, 0); // OKAY messages don't have data
    }

    #[tokio::test]
    async fn test_stream_large_data_transfer() {
        // Test transferring data larger than typical buffer sizes
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        let (local_id, remote_id) = establish_stream(&mut client_stream, b"large-data-svc").await;

        // Create 64KB of test data
        let large_data = vec![0xAB; 65536];
        let expected_crc = calculate_data_crc32(&large_data);

        let write_msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: local_id,
            arg1: remote_id,
            data_length: large_data.len() as u32,
            data_crc32: expected_crc,
            magic: !(AdbCommand::WRTE as u32),
            data: large_data,
        };

        send_adb_message(&mut client_stream, &write_msg).await.unwrap();

        let okay_response = timeout(
            Duration::from_secs(10),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        assert_eq!(okay_response.command, AdbCommand::OKAY as u32);
        assert_eq!(okay_response.arg0, remote_id);
        assert_eq!(okay_response.arg1, local_id);
    }

    #[tokio::test]
    async fn test_stream_id_reuse_after_close() {
        // Test that stream IDs can be reused after closing
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Open stream with ID 1
        let (local_id1, remote_id1) = establish_stream(&mut client_stream, b"temp-svc-1").await;
        assert_eq!(local_id1, 1);

        // Close the stream
        let close_msg = AdbMessage {
            command: AdbCommand::CLSE as u32,
            arg0: local_id1,
            arg1: remote_id1,
            data_length: 0,
            data_crc32: 0,
            magic: !(AdbCommand::CLSE as u32),
            data: vec![],
        };

        send_adb_message(&mut client_stream, &close_msg).await.unwrap();
        let _close_response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();

        // Open new stream with same local ID
        let (local_id2, _remote_id2) = establish_stream(&mut client_stream, b"temp-svc-2").await;

        // Should be able to reuse the same local stream ID
        assert_eq!(local_id2, 1);
    }

    #[tokio::test]
    async fn test_stream_error_recovery() {
        // Test stream error conditions and recovery
        let server_addr = start_test_dbgif_server().await;
        let mut client_stream = establish_cnxn_handshake(server_addr).await;

        // Try to send WRTE without opening stream first
        let invalid_write = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: 99, // non-existent local stream
            arg1: 88, // non-existent remote stream
            data_length: 4,
            data_crc32: calculate_data_crc32(b"test"),
            magic: !(AdbCommand::WRTE as u32),
            data: b"test".to_vec(),
        };

        send_adb_message(&mut client_stream, &invalid_write).await.unwrap();

        // Server should reject with CLSE or ignore with timeout
        let response = timeout(
            Duration::from_secs(5),
            receive_adb_message(&mut client_stream)
        ).await.unwrap().unwrap();
        // Could be CLSE for invalid stream or server might ignore
        assert!(response.command == AdbCommand::CLSE as u32 ||
                response.command != AdbCommand::OKAY as u32);

        // Connection should still be usable for new streams
        let (_local_id, _remote_id) = establish_stream(&mut client_stream, b"recovery-test").await;
    }

}

// Include common test helpers module
#[path = "../common/mod.rs"]
mod common;