#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn test_ascii_host_version() {
        // Start server on a random port for testing
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn server task
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();

            // Create a simple server state for testing
            use dbgif_protocol::server::state::{ServerConfig, ServerState};
            use dbgif_protocol::server::stream_forwarder::StreamForwarder;
            use dbgif_protocol::server::ascii_handler::AsciiHandler;
            use std::sync::Arc;

            let config = ServerConfig::default();
            let server_state = Arc::new(ServerState::new(config));
            let stream_forwarder = Arc::new(StreamForwarder::new(Arc::clone(&server_state)));

            let handler = AsciiHandler::new(
                server_state,
                stream_forwarder,
                "test-session".to_string(),
            );

            let _ = handler.handle_connection(stream).await;
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Connect as client
        let mut client = TcpStream::connect(addr).await.unwrap();

        // Send host:version request
        let request = "host:version";
        let length = format!("{:04x}", request.len());
        let message = format!("{}{}", length, request);

        client.write_all(message.as_bytes()).await.unwrap();
        client.flush().await.unwrap();

        // Read response
        let mut response = vec![0u8; 1024];
        let n = client.read(&mut response).await.unwrap();
        response.truncate(n);

        let response_str = String::from_utf8(response).unwrap();

        // Check response format: OKAY + 4-byte hex length + data
        assert!(response_str.starts_with("OKAY"));
        assert!(response_str.len() >= 8); // OKAY + 4 hex digits minimum

        // Extract and verify version
        let data = &response_str[8..];
        assert!(data.contains("dbgif-server"));
    }

    #[tokio::test]
    async fn test_ascii_host_list() {
        // Start server on a random port for testing
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn server task
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();

            use dbgif_protocol::server::state::{ServerConfig, ServerState};
            use dbgif_protocol::server::stream_forwarder::StreamForwarder;
            use dbgif_protocol::server::ascii_handler::AsciiHandler;
            use std::sync::Arc;

            let config = ServerConfig::default();
            let server_state = Arc::new(ServerState::new(config));
            let stream_forwarder = Arc::new(StreamForwarder::new(Arc::clone(&server_state)));

            let handler = AsciiHandler::new(
                server_state,
                stream_forwarder,
                "test-session".to_string(),
            );

            let _ = handler.handle_connection(stream).await;
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Connect as client
        let mut client = TcpStream::connect(addr).await.unwrap();

        // Send host:list request
        let request = "host:list";
        let length = format!("{:04x}", request.len());
        let message = format!("{}{}", length, request);

        client.write_all(message.as_bytes()).await.unwrap();
        client.flush().await.unwrap();

        // Read response
        let mut response = vec![0u8; 1024];
        let n = client.read(&mut response).await.unwrap();
        response.truncate(n);

        let response_str = String::from_utf8(response).unwrap();

        // Check response format
        assert!(response_str.starts_with("OKAY"));

        // The device list might be empty, which is fine for this test
        println!("Device list response: {}", response_str);
    }

    #[tokio::test]
    async fn test_ascii_strm_format() {
        use dbgif_protocol::protocol::ascii;

        // Test STRM encoding
        let stream_id = 0x42;
        let data = b"test data";
        let encoded = ascii::encode_strm(stream_id, data);

        // Verify format: STRM + 2-byte stream ID + 6-byte length + data
        assert_eq!(&encoded[0..4], b"STRM");

        // Decode and verify
        let (decoded_id, decoded_data) = ascii::decode_strm(&encoded).unwrap();
        assert_eq!(decoded_id, stream_id);
        assert_eq!(decoded_data, data);
    }
}