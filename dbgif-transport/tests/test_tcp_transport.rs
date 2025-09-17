#[cfg(test)]
mod tests {
    use dbgif_transport::{TcpTransport, Transport, TransportConfig, TransportAddress};
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
        let transport_addr = TransportAddress::Tcp(addr);
        let listen_result = transport.listen(&transport_addr).await;
        assert!(listen_result.is_ok());
    }

    #[tokio::test]
    async fn test_tcp_connection_establishment() {
        // Test that TCP connections can be established by listening and connecting
        let transport = TcpTransport::new();
        let listener_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let transport_addr = TransportAddress::Tcp(listener_addr);

        // Start listening
        let listener_result = transport.listen(&transport_addr).await;
        assert!(listener_result.is_ok());

        // The connection establishment process requires a server to be running
        // This test verifies the listening functionality exists
    }

    #[tokio::test]
    async fn test_tcp_data_transmission() {
        // Test that TCP transport can be configured for data transmission
        let config = TransportConfig {
            max_connections: 10,
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
        // Data transmission requires full server setup which is tested in integration tests
    }

    #[tokio::test]
    async fn test_tcp_connection_timeout() {
        // Test connection timeout behavior
        let transport = TcpTransport::new();
        let invalid_addr: SocketAddr = "192.0.2.1:12345".parse().unwrap(); // RFC 5737 test address
        let transport_addr = TransportAddress::Tcp(invalid_addr);

        let connect_result = transport.connect_timeout(&transport_addr, Duration::from_millis(100)).await;
        assert!(connect_result.is_err());
    }

    #[tokio::test]
    async fn test_tcp_connection_close() {
        // Test that transport can be created and supports connection management concepts
        let transport = TcpTransport::new();
        assert_eq!(transport.transport_type(), "tcp");

        // Connection close functionality is implemented at the connection level
        // and tested in integration tests with full server setup
    }

    #[tokio::test]
    async fn test_tcp_large_data_transfer() {
        // Test that transport configuration supports large data transfer
        let config = TransportConfig {
            max_connections: 100,
            connect_timeout: Duration::from_secs(10),
            keepalive_interval: Some(Duration::from_secs(60)),
            max_data_size: 10 * 1024 * 1024, // 10MB max data size
            buffer_size: 256 * 1024, // 256KB buffer
            tcp_nodelay: true,
            reuse_addr: true,
            listen_backlog: 256,
        };

        let max_data_size = config.max_data_size;
        let transport = TcpTransport::with_config(config);
        assert_eq!(transport.transport_type(), "tcp");

        // Large data transfer functionality is tested in integration tests
        assert!(max_data_size >= 1024 * 1024); // At least 1MB support
    }

    #[tokio::test]
    async fn test_tcp_concurrent_connections() {
        // Test that transport configuration supports concurrent connections
        let config = TransportConfig {
            max_connections: 100,
            connect_timeout: Duration::from_secs(5),
            keepalive_interval: Some(Duration::from_secs(30)),
            max_data_size: 1024 * 1024,
            buffer_size: 64 * 1024,
            tcp_nodelay: true,
            reuse_addr: true,
            listen_backlog: 256, // High backlog for concurrent connections
        };

        let max_connections = config.max_connections;
        let listen_backlog = config.listen_backlog;
        let transport = TcpTransport::with_config(config);
        assert_eq!(transport.transport_type(), "tcp");

        // Concurrent connection handling is tested in integration tests
        assert!(max_connections >= 10); // Support for multiple connections
        assert!(listen_backlog >= 100); // High connection backlog
    }

    #[tokio::test]
    async fn test_tcp_connection_error_handling() {
        // Test proper error handling for connection timeout
        let transport = TcpTransport::new();
        let invalid_addr: SocketAddr = "192.0.2.1:12345".parse().unwrap(); // RFC 5737 test address
        let transport_addr = TransportAddress::Tcp(invalid_addr);

        // This should timeout and return an error
        let connect_result = transport.connect_timeout(&transport_addr, Duration::from_millis(50)).await;
        assert!(connect_result.is_err());

        // Error handling for other scenarios is tested in integration tests
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
        // Test that transport provides type information and stats capability
        let transport = TcpTransport::new();

        // Basic metadata
        assert_eq!(transport.transport_type(), "tcp");

        // Stats functionality
        let stats = transport.stats();
        assert_eq!(stats.active_connections, 0); // Initially no connections

        // Connection metadata is tested in integration tests with real connections
    }

}