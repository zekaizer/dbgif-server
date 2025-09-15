use dbgif_server::transport::*;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};
use anyhow::Result;

/// Integration tests for TCP Transport
/// These tests verify TCP transport functionality with real network connections.

// Mock ADB daemon for testing TCP transport
struct MockAdbDaemon {
    listener: TcpListener,
    port: u16,
}

impl MockAdbDaemon {
    async fn new() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    fn port(&self) -> u16 {
        self.port
    }

    async fn run_background(&mut self) {
        loop {
            if let Ok((mut stream, _)) = self.listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 1024];
                    while let Ok(bytes_read) = stream.read(&mut buffer).await {
                        if bytes_read == 0 {
                            break;
                        }
                        // Echo back simple response
                        let _ = stream.write_all(b"OKAY\n").await;
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tcp_transport_integration_tests {
    use super::*;

    /// Test TCP transport discovery with running daemon
    #[tokio::test]
    async fn test_tcp_transport_discovery_with_daemon() {
        println!("Testing TCP transport discovery with daemon");

        // Start mock ADB daemon
        let mut daemon = MockAdbDaemon::new().await
            .expect("Failed to start mock daemon");
        let daemon_port = daemon.port();

        // Run daemon in background
        tokio::spawn(async move {
            daemon.run_background().await;
        });

        // Give daemon time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create TCP transport factory
        let factory = TcpTransportFactory::new();

        // Since we can't easily configure the factory to scan our random port,
        // we'll test the basic functionality
        let devices = factory.discover_devices().await
            .expect("Device discovery should not fail");

        // The factory may or may not find devices depending on what's running on the system
        println!("Found {} TCP devices", devices.len());

        // Test that all discovered devices are TCP type
        for device in &devices {
            assert_eq!(device.transport_type, TransportType::Tcp,
                      "All discovered devices should be TCP type");
            assert!(!device.id.is_empty(), "Device ID should not be empty");
            assert!(!device.product.is_empty(), "Device product should not be empty");
        }
    }

    /// Test TCP transport creation and basic operations
    #[tokio::test]
    async fn test_tcp_transport_creation() {
        println!("Testing TCP transport creation");

        let factory = TcpTransportFactory::new();

        // Test factory properties
        assert_eq!(factory.transport_type(), TransportType::Tcp);
        assert!(factory.is_available(), "TCP factory should be available");

        let factory_name = factory.factory_name();
        assert!(!factory_name.is_empty(), "Factory name should not be empty");
        assert!(factory_name.to_lowercase().contains("tcp"),
               "Factory name should contain 'tcp'");

        // Test device discovery (may find 0 or more devices)
        let discovery_result = factory.discover_devices().await;
        assert!(discovery_result.is_ok(), "Device discovery should not fail");

        let devices = discovery_result.unwrap();
        println!("Discovered {} TCP devices", devices.len());

        // If devices are found, test transport creation
        if !devices.is_empty() {
            let transport_result = factory.create_transport(&devices[0]).await;
            assert!(transport_result.is_ok(), "Transport creation should succeed");

            let transport = transport_result.unwrap();
            assert_eq!(transport.transport_type(), TransportType::Tcp);
            assert!(!transport.is_connected(), "New transport should start disconnected");

            let max_size = transport.max_transfer_size();
            assert!(max_size >= 64, "Max transfer size should be at least 64 bytes");
            assert!(transport.is_bidirectional(), "Transport should be bidirectional");
        }
    }

    /// Test TCP transport with mock server
    #[tokio::test]
    async fn test_tcp_transport_with_mock_server() {
        println!("Testing TCP transport with mock server");

        // Start a simple echo server
        let listener = TcpListener::bind("127.0.0.1:0").await
            .expect("Failed to bind listener");
        let server_addr = listener.local_addr()
            .expect("Failed to get server address");

        // Run echo server in background
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = vec![0u8; 1024];
                while let Ok(bytes_read) = stream.read(&mut buffer).await {
                    if bytes_read == 0 {
                        break;
                    }
                    // Echo back the data
                    let _ = stream.write_all(&buffer[..bytes_read]).await;
                }
            }
        });

        // Wait for server to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Create a mock device info for our server
        let device_info = DeviceInfo {
            id: format!("tcp:{}", server_addr),
            transport_type: TransportType::Tcp,
            product: "Mock".to_string(),
            model: "Test Server".to_string(),
            device: "test".to_string(),
            serial: "mock_serial".to_string(),
        };

        let factory = TcpTransportFactory::new();
        let mut transport = factory.create_transport(&device_info).await
            .expect("Failed to create transport");

        // Test connection
        let connect_result = timeout(Duration::from_secs(5), transport.connect()).await;
        assert!(connect_result.is_ok(), "Connect should not timeout");
        assert!(connect_result.unwrap().is_ok(), "Connect should succeed");
        assert!(transport.is_connected(), "Transport should be connected");

        // Test data transfer
        let test_data = b"Hello, TCP transport!";
        let send_result = transport.send(test_data).await;
        assert!(send_result.is_ok(), "Send should succeed");

        let mut buffer = vec![0u8; 1024];
        let receive_result = transport.receive(&mut buffer).await;
        assert!(receive_result.is_ok(), "Receive should succeed");

        let bytes_received = receive_result.unwrap();
        assert!(bytes_received > 0, "Should receive some data");
        assert_eq!(&buffer[..bytes_received], test_data, "Should echo back the same data");

        // Test disconnect
        let disconnect_result = transport.disconnect().await;
        assert!(disconnect_result.is_ok(), "Disconnect should succeed");
        assert!(!transport.is_connected(), "Transport should be disconnected");
    }

    /// Test TCP transport error handling
    #[tokio::test]
    async fn test_tcp_transport_error_handling() {
        println!("Testing TCP transport error handling");

        // Create device info for non-existent server
        let device_info = DeviceInfo {
            id: "tcp:127.0.0.1:12345".to_string(), // Unlikely to be used
            transport_type: TransportType::Tcp,
            product: "NonExistent".to_string(),
            model: "Test".to_string(),
            device: "test".to_string(),
            serial: "test_serial".to_string(),
        };

        let factory = TcpTransportFactory::new();
        let mut transport = factory.create_transport(&device_info).await
            .expect("Transport creation should succeed");

        // Test that operations on disconnected transport fail
        let send_result = transport.send(b"test").await;
        assert!(send_result.is_err(), "Send on disconnected transport should fail");

        let mut buffer = vec![0u8; 64];
        let receive_result = transport.receive(&mut buffer).await;
        assert!(receive_result.is_err(), "Receive on disconnected transport should fail");

        // Test connection to non-existent server
        let connect_result = timeout(Duration::from_secs(1), transport.connect()).await;
        match connect_result {
            Ok(Ok(())) => {
                // Connection succeeded unexpectedly - maybe something is running on that port
                println!("Unexpected connection success - something running on port 12345");
                transport.disconnect().await.ok();
            }
            Ok(Err(_)) => {
                // Expected - connection failed
                assert!(!transport.is_connected(), "Should remain disconnected on failed connect");
            }
            Err(_) => {
                // Timeout - also acceptable
                assert!(!transport.is_connected(), "Should remain disconnected on timeout");
            }
        }
    }

    /// Test TCP transport properties
    #[tokio::test]
    async fn test_tcp_transport_properties() {
        println!("Testing TCP transport properties");

        let device_info = DeviceInfo {
            id: "tcp:127.0.0.1:5555".to_string(),
            transport_type: TransportType::Tcp,
            product: "Android".to_string(),
            model: "Emulator".to_string(),
            device: "emulator".to_string(),
            serial: "emulator-5554".to_string(),
        };

        let factory = TcpTransportFactory::new();
        let transport = factory.create_transport(&device_info).await
            .expect("Transport creation should succeed");

        // Test transport properties
        assert_eq!(transport.transport_type(), TransportType::Tcp);
        assert!(!transport.is_connected(), "New transport should start disconnected");

        let display_name = transport.display_name();
        assert!(!display_name.is_empty(), "Display name should not be empty");
        assert!(display_name.to_lowercase().contains("tcp"),
               "Display name should contain 'tcp'");

        let max_size = transport.max_transfer_size();
        assert!(max_size >= 1024, "TCP should support reasonable transfer sizes");
        assert!(max_size <= 16 * 1024 * 1024, "Max size should be reasonable");

        assert!(transport.is_bidirectional(), "TCP transport should be bidirectional");
    }

    /// Test TCP factory with multiple discovery attempts
    #[tokio::test]
    async fn test_tcp_factory_consistency() {
        println!("Testing TCP factory consistency");

        let factory = TcpTransportFactory::new();

        // Test that factory properties are consistent
        assert_eq!(factory.transport_type(), TransportType::Tcp);
        assert!(factory.is_available());
        assert_eq!(factory.factory_name(), factory.factory_name());

        // Test that discovery is consistent
        let devices1 = factory.discover_devices().await
            .expect("First discovery should work");
        let devices2 = factory.discover_devices().await
            .expect("Second discovery should work");

        // Results should be the same (assuming no changes in environment)
        assert_eq!(devices1.len(), devices2.len(),
                  "Discovery results should be consistent");

        for (dev1, dev2) in devices1.iter().zip(devices2.iter()) {
            assert_eq!(dev1.id, dev2.id, "Device IDs should be consistent");
            assert_eq!(dev1.transport_type, dev2.transport_type,
                      "Transport types should be consistent");
        }
    }

    /// Test TCP transport with real connection timeout
    #[tokio::test]
    async fn test_tcp_transport_timeout() {
        println!("Testing TCP transport timeout behavior");

        // Use a non-routable IP address to force timeout
        let device_info = DeviceInfo {
            id: "tcp:10.255.255.1:5555".to_string(),
            transport_type: TransportType::Tcp,
            product: "Timeout".to_string(),
            model: "Test".to_string(),
            device: "timeout".to_string(),
            serial: "timeout_serial".to_string(),
        };

        let factory = TcpTransportFactory::new();
        let mut transport = factory.create_transport(&device_info).await
            .expect("Transport creation should succeed");

        // Test connection timeout
        let start = std::time::Instant::now();
        let connect_result = timeout(Duration::from_secs(2), transport.connect()).await;
        let elapsed = start.elapsed();

        // Should timeout or fail quickly
        match connect_result {
            Ok(Ok(())) => {
                // Unexpected success
                println!("Unexpected connection success to non-routable IP");
                transport.disconnect().await.ok();
            }
            Ok(Err(_)) => {
                // Connection failed - expected
                assert!(elapsed < Duration::from_secs(5), "Should fail reasonably quickly");
            }
            Err(_) => {
                // Timeout - expected
                assert!(elapsed >= Duration::from_secs(2), "Should respect timeout");
                assert!(elapsed < Duration::from_secs(3), "Should not take much longer than timeout");
            }
        }

        assert!(!transport.is_connected(), "Should not be connected after timeout/failure");
    }

    /// Test concurrent TCP operations
    #[tokio::test]
    async fn test_tcp_transport_concurrent_operations() {
        println!("Testing TCP transport concurrent operations");

        let factory = TcpTransportFactory::new();

        // Test concurrent device discovery
        let discovery_tasks = (0..5).map(|_| {
            let factory = TcpTransportFactory::new();
            tokio::spawn(async move {
                factory.discover_devices().await
            })
        });

        let results = futures::future::join_all(discovery_tasks).await;

        // All discoveries should succeed
        for result in results {
            let discovery_result = result.expect("Task should not panic");
            assert!(discovery_result.is_ok(), "Discovery should not fail");
        }
    }

    /// Integration test with real ADB daemon (if available)
    #[tokio::test]
    #[ignore] // Run with --ignored flag when testing with real ADB
    async fn test_tcp_transport_with_real_adb() {
        println!("Testing TCP transport with real ADB daemon");

        let factory = TcpTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        if !devices.is_empty() {
            println!("Found {} ADB devices", devices.len());

            for device in &devices {
                println!("Device: {} ({})", device.id, device.product);

                let mut transport = factory.create_transport(device).await
                    .expect("Transport creation should succeed");

                // Test connection
                match timeout(Duration::from_secs(5), transport.connect()).await {
                    Ok(Ok(())) => {
                        println!("Successfully connected to {}", device.id);
                        assert!(transport.is_connected());

                        // Test basic communication
                        if let Ok(()) = transport.send(b"host:version").await {
                            let mut buffer = vec![0u8; 1024];
                            if let Ok(bytes) = transport.receive(&mut buffer).await {
                                println!("Received {} bytes from ADB daemon", bytes);
                            }
                        }

                        transport.disconnect().await.ok();
                    }
                    Ok(Err(e)) => {
                        println!("Failed to connect to {}: {}", device.id, e);
                    }
                    Err(_) => {
                        println!("Timeout connecting to {}", device.id);
                    }
                }
            }
        } else {
            println!("No TCP ADB devices found - make sure ADB daemon is running");
        }
    }
}