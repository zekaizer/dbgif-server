use dbgif_server::transport::*;
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// Contract tests for Transport trait
/// These tests ensure that all transport implementations behave consistently
/// and fulfill the contract defined by the Transport trait.

#[cfg(test)]
mod transport_trait_contract_tests {
    use super::*;

    /// Test that all transport types can be created and have correct basic properties
    #[tokio::test]
    async fn test_transport_creation_contract() {
        let test_cases = vec![
            ("Mock Transport", create_mock_transport()),
        ];

        for (name, mut transport) in test_cases {
            println!("Testing {} creation contract", name);

            // Contract: All transports must report correct type
            let transport_type = transport.transport_type();
            assert!(
                matches!(
                    transport_type,
                    TransportType::Tcp | TransportType::UsbDevice | TransportType::UsbBridge
                ),
                "{} reported invalid transport type: {:?}",
                name,
                transport_type
            );

            // Contract: All transports must have non-empty display name
            let display_name = transport.display_name();
            assert!(!display_name.is_empty(), "{} has empty display name", name);

            // Contract: All transports start disconnected
            assert!(
                !transport.is_connected(),
                "{} should start disconnected",
                name
            );

            // Contract: All transports must report reasonable max transfer size
            let max_size = transport.max_transfer_size();
            assert!(
                max_size >= 64 && max_size <= 16 * 1024 * 1024,
                "{} reported unreasonable max transfer size: {}",
                name,
                max_size
            );

            // Contract: All transports must be bidirectional
            assert!(
                transport.is_bidirectional(),
                "{} should be bidirectional",
                name
            );
        }
    }

    /// Test connection lifecycle contract
    #[tokio::test]
    async fn test_connection_lifecycle_contract() {
        let test_cases = vec![
            ("Mock Transport", create_mock_transport()),
        ];

        for (name, mut transport) in test_cases {
            println!("Testing {} connection lifecycle contract", name);

            // Contract: Initially disconnected
            assert!(!transport.is_connected(), "{} should start disconnected", name);

            // Contract: Connect operation should succeed or fail gracefully
            let connect_result = timeout(Duration::from_secs(5), transport.connect()).await;

            match connect_result {
                Ok(Ok(())) => {
                    // Contract: After successful connect, should report connected
                    assert!(transport.is_connected(), "{} should be connected after successful connect", name);

                    // Contract: Disconnect should work
                    let disconnect_result = transport.disconnect().await;
                    assert!(disconnect_result.is_ok(), "{} disconnect should succeed", name);
                    assert!(!transport.is_connected(), "{} should be disconnected after disconnect", name);
                }
                Ok(Err(e)) => {
                    println!("{} connect failed as expected: {}", name, e);
                    // Contract: Failed connect should leave transport disconnected
                    assert!(!transport.is_connected(), "{} should remain disconnected after failed connect", name);
                }
                Err(_) => {
                    println!("{} connect timed out (acceptable for real transports)", name);
                }
            }
        }
    }

    /// Test data transfer contract
    #[tokio::test]
    async fn test_data_transfer_contract() {
        // Only test with mock transport for reliable results
        let mut transport = create_mock_transport();

        println!("Testing data transfer contract");

        // Contract: Operations on disconnected transport should fail
        let test_data = b"test data";
        let send_result = transport.send(test_data).await;
        assert!(send_result.is_err(), "Send on disconnected transport should fail");

        let mut buffer = vec![0u8; 1024];
        let recv_result = transport.receive(&mut buffer).await;
        assert!(recv_result.is_err(), "Receive on disconnected transport should fail");

        // Connect for data transfer tests
        transport.connect().await.expect("Mock transport should connect");

        // Contract: Send valid data should succeed
        let test_data = b"Hello, Transport!";
        let send_result = transport.send(test_data).await;
        assert!(send_result.is_ok(), "Send valid data should succeed");

        // Contract: Receive should return data
        let mut buffer = vec![0u8; 1024];
        let recv_result = transport.receive(&mut buffer).await;
        assert!(recv_result.is_ok(), "Receive should succeed");

        let bytes_received = recv_result.unwrap();
        assert!(bytes_received > 0, "Should receive some bytes");
        assert!(bytes_received <= buffer.len(), "Received bytes should not exceed buffer size");

        // Contract: Send empty data should succeed
        let empty_result = transport.send(&[]).await;
        assert!(empty_result.is_ok(), "Send empty data should succeed");

        // Contract: Send data larger than max should fail or truncate
        let max_size = transport.max_transfer_size();
        let large_data = vec![0u8; max_size + 1];
        let large_send_result = transport.send(&large_data).await;
        // Either fails or succeeds (implementation dependent)
        match large_send_result {
            Ok(()) => println!("Large data send succeeded (truncation or chunking implemented)"),
            Err(e) => println!("Large data send failed as expected: {}", e),
        }
    }

    /// Test transport metadata contract
    #[tokio::test]
    async fn test_metadata_contract() {
        let test_cases = vec![
            ("Mock Transport", create_mock_transport()),
        ];

        for (name, transport) in test_cases {
            println!("Testing {} metadata contract", name);

            // Contract: Display name should be informative
            let display_name = transport.display_name();
            assert!(!display_name.is_empty(), "{} has empty display name", name);
            assert!(display_name.len() >= 3, "{} display name too short: '{}'", name, display_name);

            // Contract: Max transfer size should be reasonable
            let max_size = transport.max_transfer_size();
            assert!(max_size >= 64, "{} max transfer size too small: {}", name, max_size);
            assert!(max_size <= 16 * 1024 * 1024, "{} max transfer size too large: {}", name, max_size);

            // Contract: All transports should be bidirectional for ADB
            assert!(transport.is_bidirectional(), "{} should be bidirectional", name);
        }
    }

    // Helper functions to create transport instances
    fn create_mock_transport() -> Box<dyn Transport> {
        Box::new(MockTransport::new("mock_test_device".to_string()))
    }
}

/// Integration test helper to verify transport factories work with transports
#[cfg(test)]
mod transport_factory_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_factory_transport_compatibility() {
        println!("Testing factory-transport compatibility");

        // Test TCP factory creates compatible transports
        let tcp_factory = TcpTransportFactory::new();
        assert!(tcp_factory.is_available(), "TCP factory should be available");

        let tcp_devices = tcp_factory.discover_devices().await.unwrap();
        for device in tcp_devices {
            assert_eq!(device.transport_type, TransportType::Tcp,
                      "TCP factory should only create TCP devices");

            let transport = tcp_factory.create_transport(&device).await;
            assert!(transport.is_ok(), "TCP factory should create valid transports");

            let transport = transport.unwrap();
            assert_eq!(transport.transport_type(), TransportType::Tcp,
                      "Created transport should have correct type");
        }

        // Test USB Device factory
        let usb_factory = UsbDeviceTransportFactory::new();
        assert!(usb_factory.is_available(), "USB Device factory should be available");

        let usb_devices = usb_factory.discover_devices().await.unwrap();
        for device in usb_devices {
            assert_eq!(device.transport_type, TransportType::UsbDevice,
                      "USB Device factory should only create USB Device devices");

            let transport = usb_factory.create_transport(&device).await;
            assert!(transport.is_ok(), "USB Device factory should create valid transports");

            let transport = transport.unwrap();
            assert_eq!(transport.transport_type(), TransportType::UsbDevice,
                      "Created transport should have correct type");
        }

        // Test USB Bridge factory
        let bridge_factory = UsbBridgeTransportFactory::new();
        assert!(bridge_factory.is_available(), "USB Bridge factory should be available");

        let bridge_devices = bridge_factory.discover_devices().await.unwrap();
        for device in bridge_devices {
            assert_eq!(device.transport_type, TransportType::UsbBridge,
                      "USB Bridge factory should only create USB Bridge devices");

            let transport = bridge_factory.create_transport(&device).await;
            assert!(transport.is_ok(), "USB Bridge factory should create valid transports");

            let transport = transport.unwrap();
            assert_eq!(transport.transport_type(), TransportType::UsbBridge,
                      "Created transport should have correct type");
        }
    }
}