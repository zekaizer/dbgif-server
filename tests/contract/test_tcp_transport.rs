#[cfg(test)]
mod tests {
    use dbgif_protocol::transport::{TcpTransport, Transport, TransportConfig};
    use std::net::SocketAddr;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_tcp_transport_creation() {
        // Test that TCP transport can be created with valid configuration
        let transport = TcpTransport::new();
        assert!(transport.transport_type() == "tcp");
    }

    #[tokio::test]
    async fn test_tcp_transport_listen() {
        // Test that TCP transport can listen on a port
        let transport = TcpTransport::new();
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listen_result = transport.listen(addr).await;
        assert!(listen_result.is_ok());
    }

    #[tokio::test]
    async fn test_tcp_connection_establishment() {
        // Test that TCP connections can be established
        // This test MUST fail until proper implementation
        panic!("TODO: Implement TCP connection establishment test after full server setup");
    }

    #[tokio::test]
    async fn test_tcp_data_transmission() {
        // Test bidirectional data transmission over TCP
        // This test MUST fail until proper implementation
        panic!("TODO: Implement TCP data transmission test after full server setup");
    }

    #[tokio::test]
    async fn test_tcp_connection_timeout() {
        // Test connection timeout behavior
        let transport = TcpTransport::new();
        let invalid_addr: SocketAddr = "192.0.2.1:12345".parse().unwrap(); // RFC 5737 test address

        let connect_result = transport.connect_timeout(invalid_addr, Duration::from_millis(100)).await;
        assert!(connect_result.is_err());
    }

    #[tokio::test]
    async fn test_tcp_connection_close() {
        // Test graceful connection closure
        // This test MUST fail until proper implementation
        panic!("TODO: Implement TCP connection close test after full server setup");
    }

    #[tokio::test]
    async fn test_tcp_large_data_transfer() {
        // Test transfer of large data chunks
        // This test MUST fail until proper implementation
        panic!("TODO: Implement TCP large data transfer test after full server setup");
    }

    #[tokio::test]
    async fn test_tcp_concurrent_connections() {
        // Test multiple concurrent TCP connections
        // This test MUST fail until proper implementation
        panic!("TODO: Implement TCP concurrent connections test after full server setup");
    }

    #[tokio::test]
    async fn test_tcp_connection_error_handling() {
        // Test proper error handling for various failure scenarios
        // This test MUST fail until proper implementation
        panic!("TODO: Implement TCP connection error handling test after full server setup");
    }

    #[tokio::test]
    async fn test_tcp_transport_configuration() {
        // Test TCP transport configuration options
        let config = TransportConfig {
            max_connections: 50,
            connect_timeout: Duration::from_secs(5),
            keepalive_interval: Some(Duration::from_secs(30)),
            max_data_size: 1024 * 1024,
            buffer_size: 64 * 1024,
            tcp_nodelay: true,
            reuse_addr: true,
            listen_backlog: 128,
        };

        let transport = TcpTransport::with_config(config);
        assert!(transport.transport_type() == "tcp");
    }

    #[tokio::test]
    async fn test_tcp_connection_metadata() {
        // Test retrieval of connection metadata
        // This test MUST fail until proper implementation
        panic!("TODO: Implement TCP connection metadata test after full server setup");
    }

}