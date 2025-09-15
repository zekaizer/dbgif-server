use dbgif_server::*;
use std::net::TcpStream;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout};

/// Integration tests for daemon server functionality
/// These tests verify that the full daemon server can accept client connections
/// and handle basic operations properly.

#[cfg(test)]
mod daemon_server_integration_tests {
    use super::*;

    /// Test that daemon server can be created and configured
    #[tokio::test]
    async fn test_daemon_creation() {
        println!("Testing daemon creation");

        // Test development configuration
        let dev_config = DaemonConfig::development();
        let dev_daemon = AdbDaemon::builder()
            .with_config(dev_config)
            .build();
        assert!(dev_daemon.is_ok(), "Should build daemon with development config");

        // Test production configuration
        let prod_config = DaemonConfig::production();
        let prod_daemon = AdbDaemon::builder()
            .with_config(prod_config)
            .build();
        assert!(prod_daemon.is_ok(), "Should build daemon with production config");

        println!("Daemon creation test completed successfully");
    }

    /// Test daemon lifecycle (start and stop)
    #[tokio::test]
    async fn test_daemon_lifecycle() {
        println!("Testing daemon lifecycle");

        // Create daemon configuration with random port
        let listener = TcpListener::bind("127.0.0.1:0").await
            .expect("Should bind to random port");
        let addr = listener.local_addr().expect("Should get local address");
        drop(listener);

        let mut config = DaemonConfig::development();
        config.tcp_port = addr.port();
        config.host = "127.0.0.1".to_string();

        // Build daemon
        let daemon = AdbDaemon::builder()
            .with_config(config)
            .build()
            .expect("Should build daemon");

        // Start daemon in background task
        let daemon_handle = {
            let daemon_clone = daemon.clone();
            tokio::spawn(async move {
                daemon_clone.run().await
            })
        };

        // Give daemon time to start
        sleep(Duration::from_millis(100)).await;

        // Shutdown daemon gracefully
        daemon.shutdown().await.expect("Should shutdown cleanly");

        // Wait for daemon task to complete
        let result = timeout(Duration::from_secs(5), daemon_handle).await;
        assert!(result.is_ok(), "Daemon should shutdown within timeout");

        println!("Daemon lifecycle test completed successfully");
    }

    /// Test TCP server accepts connections
    #[tokio::test]
    async fn test_tcp_server_connections() {
        println!("Testing TCP server connection acceptance");

        // Find available port
        let listener = TcpListener::bind("127.0.0.1:0").await
            .expect("Should bind to random port");
        let addr = listener.local_addr().expect("Should get local address");
        drop(listener);

        // Create daemon configuration
        let mut config = DaemonConfig::development();
        config.tcp_port = addr.port();
        config.host = "127.0.0.1".to_string();

        // Build and start daemon
        let daemon = AdbDaemon::builder()
            .with_config(config)
            .build()
            .expect("Should build daemon");

        let daemon_handle = {
            let daemon_clone = daemon.clone();
            tokio::spawn(async move {
                daemon_clone.run().await
            })
        };

        // Give daemon time to start
        sleep(Duration::from_millis(200)).await;

        // Test basic TCP connection
        let connection_result = timeout(
            Duration::from_secs(2),
            TcpStream::connect(&addr)
        ).await;

        assert!(connection_result.is_ok(), "Should connect within timeout");
        let stream_result = connection_result.unwrap();
        assert!(stream_result.is_ok(), "Should successfully connect to daemon");

        println!("TCP connection established successfully");

        // Clean up
        drop(stream_result);
        daemon.shutdown().await.expect("Should shutdown cleanly");
        let _ = timeout(Duration::from_secs(3), daemon_handle).await;

        println!("TCP server connection test completed successfully");
    }

    /// Test daemon configuration variants
    #[test]
    fn test_daemon_configurations() {
        println!("Testing daemon configuration variants");

        // Test default configuration
        let default_config = DaemonConfig::default();
        assert_eq!(default_config.tcp_port, 5037);
        assert_eq!(default_config.host, "127.0.0.1");

        // Test development configuration
        let dev_config = DaemonConfig::development();
        assert!(dev_config.logging.enabled);

        // Test production configuration
        let prod_config = DaemonConfig::production();
        assert!(prod_config.logging.enabled);

        // Test custom configuration
        let mut custom_config = DaemonConfig::default();
        custom_config.tcp_port = 15037;
        custom_config.host = "0.0.0.0".to_string();
        custom_config.max_connections = 10;

        assert_eq!(custom_config.tcp_port, 15037);
        assert_eq!(custom_config.host, "0.0.0.0");
        assert_eq!(custom_config.max_connections, 10);

        println!("Configuration test completed successfully");
    }
}