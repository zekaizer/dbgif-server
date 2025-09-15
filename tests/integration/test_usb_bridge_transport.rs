use dbgif_server::transport::*;
use tokio::time::Duration;
use anyhow::Result;

/// Integration tests for USB Bridge Transport (PL25A1)
/// These tests verify USB bridge transport functionality with nusb library.

#[cfg(test)]
mod usb_bridge_transport_integration_tests {
    use super::*;

    /// Test USB bridge factory properties
    #[tokio::test]
    async fn test_usb_bridge_factory_properties() {
        println!("Testing USB bridge factory properties");

        let factory = UsbBridgeTransportFactory::new();

        // Test factory properties
        assert_eq!(factory.transport_type(), TransportType::UsbBridge);
        assert!(factory.is_available(), "USB bridge factory should be available");

        let factory_name = factory.factory_name();
        assert!(!factory_name.is_empty(), "Factory name should not be empty");
        assert!(factory_name.to_lowercase().contains("bridge"),
               "Factory name should contain 'bridge'");
        assert!(factory_name.to_lowercase().contains("usb"),
               "Factory name should contain 'usb'");
    }

    /// Test USB bridge device discovery
    #[tokio::test]
    async fn test_usb_bridge_discovery() {
        println!("Testing USB bridge discovery");

        let factory = UsbBridgeTransportFactory::new();

        // Test device discovery (may find 0 or more devices)
        let discovery_result = factory.discover_devices().await;
        assert!(discovery_result.is_ok(), "Device discovery should not fail");

        let devices = discovery_result.unwrap();
        println!("Discovered {} USB bridge devices", devices.len());

        // Test that all discovered devices are USB bridge type
        for device in &devices {
            assert_eq!(device.transport_type, TransportType::UsbBridge,
                      "All discovered devices should be UsbBridge type");
            assert!(!device.id.is_empty(), "Device ID should not be empty");
            assert!(!device.product.is_empty(), "Device product should not be empty");
            assert!(!device.serial.is_empty(), "Device serial should not be empty");

            // Test device ID format for bridge devices
            assert!(device.id.starts_with("bridge:") || device.id.contains("bridge"),
                   "Bridge device ID should indicate bridge type");

            // Test that product name indicates bridge functionality
            let product_lower = device.product.to_lowercase();
            assert!(product_lower.contains("bridge") ||
                   product_lower.contains("pl25a1") ||
                   product_lower.contains("link") ||
                   product_lower.contains("host"),
                   "Product name should indicate bridge/link functionality");
        }
    }

    /// Test USB bridge transport creation
    #[tokio::test]
    async fn test_usb_bridge_transport_creation() {
        println!("Testing USB bridge transport creation");

        let factory = UsbBridgeTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        if !devices.is_empty() {
            let transport_result = factory.create_transport(&devices[0]).await;
            assert!(transport_result.is_ok(), "Transport creation should succeed");

            let transport = transport_result.unwrap();
            assert_eq!(transport.transport_type(), TransportType::UsbBridge);
            assert!(!transport.is_connected(), "New transport should start disconnected");

            let max_size = transport.max_transfer_size();
            assert!(max_size >= 64, "Max transfer size should be at least 64 bytes");
            assert!(max_size <= 16 * 1024 * 1024, "Max transfer size should be reasonable");

            assert!(transport.is_bidirectional(), "Transport should be bidirectional");

            let display_name = transport.display_name();
            assert!(!display_name.is_empty(), "Display name should not be empty");
            assert!(display_name.to_lowercase().contains("bridge") ||
                   display_name.to_lowercase().contains("usb"),
                   "Display name should indicate bridge/USB");
        } else {
            println!("No USB bridge devices found for transport creation test");
        }
    }

    /// Test USB bridge transport operations
    #[tokio::test]
    async fn test_usb_bridge_transport_operations() {
        println!("Testing USB bridge transport operations");

        let factory = UsbBridgeTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        if !devices.is_empty() {
            let mut transport = factory.create_transport(&devices[0]).await
                .expect("Transport creation should succeed");

            // Test initial state
            assert_eq!(transport.transport_type(), TransportType::UsbBridge);
            assert!(!transport.is_connected(), "Transport should start disconnected");

            // Test operations on disconnected transport
            let send_result = transport.send(b"test").await;
            assert!(send_result.is_err(), "Send on disconnected transport should fail");

            let mut buffer = vec![0u8; 64];
            let receive_result = transport.receive(&mut buffer).await;
            assert!(receive_result.is_err(), "Receive on disconnected transport should fail");

            // Test connection attempt
            let connect_result = transport.connect().await;
            match connect_result {
                Ok(()) => {
                    println!("Successfully connected to USB bridge device");
                    assert!(transport.is_connected(), "Transport should be connected");

                    // Test basic data transfer (may fail depending on bridge state)
                    let send_result = transport.send(b"PING").await;
                    match send_result {
                        Ok(()) => {
                            println!("Successfully sent data to USB bridge");

                            // Try to receive response
                            let mut buffer = vec![0u8; 1024];
                            match transport.receive(&mut buffer).await {
                                Ok(bytes) => {
                                    println!("Received {} bytes from USB bridge", bytes);
                                }
                                Err(e) => {
                                    println!("Receive failed (may be expected for bridge): {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            println!("Send failed (may be expected for bridge): {}", e);
                        }
                    }

                    // Test disconnect
                    let disconnect_result = transport.disconnect().await;
                    assert!(disconnect_result.is_ok(), "Disconnect should succeed");
                    assert!(!transport.is_connected(), "Transport should be disconnected");
                }
                Err(e) => {
                    println!("Connection failed (may be expected for bridge): {}", e);
                    assert!(!transport.is_connected(), "Should remain disconnected on failed connect");
                }
            }
        } else {
            println!("No USB bridge devices found for operations test");
        }
    }

    /// Test USB bridge error handling
    #[tokio::test]
    async fn test_usb_bridge_error_handling() {
        println!("Testing USB bridge error handling");

        let factory = UsbBridgeTransportFactory::new();

        // Test creating transport with invalid device info
        let invalid_device = DeviceInfo {
            id: "invalid".to_string(),
            transport_type: TransportType::Tcp, // Wrong type
            product: "Invalid".to_string(),
            model: "Test".to_string(),
            device: "invalid".to_string(),
            serial: "invalid".to_string(),
        };

        let result = factory.create_transport(&invalid_device).await;
        // Should either reject or handle gracefully
        match result {
            Ok(_) => {
                println!("Factory accepted mismatched device type (implementation choice)");
            }
            Err(e) => {
                println!("Factory rejected invalid device: {}", e);
                let error_msg = e.to_string();
                assert!(!error_msg.is_empty(), "Error message should not be empty");
            }
        }
    }

    /// Test USB bridge discovery consistency
    #[tokio::test]
    async fn test_usb_bridge_discovery_consistency() {
        println!("Testing USB bridge discovery consistency");

        let factory = UsbBridgeTransportFactory::new();

        // Test multiple discovery attempts
        let devices1 = factory.discover_devices().await
            .expect("First discovery should work");

        // Small delay to allow for any system changes
        tokio::time::sleep(Duration::from_millis(100)).await;

        let devices2 = factory.discover_devices().await
            .expect("Second discovery should work");

        // Results should generally be consistent
        println!("First discovery: {} bridge devices", devices1.len());
        println!("Second discovery: {} bridge devices", devices2.len());

        // Bridge device counts might vary due to hotplug events, but should be reasonable
        let diff = (devices1.len() as i32 - devices2.len() as i32).abs();
        assert!(diff <= 2, "Bridge device count shouldn't change dramatically between discoveries");

        // If we found devices both times, check consistency
        if !devices1.is_empty() && !devices2.is_empty() {
            // Look for common devices
            let common_devices: Vec<_> = devices1
                .iter()
                .filter(|d1| devices2.iter().any(|d2| d1.id == d2.id))
                .collect();

            if !common_devices.is_empty() {
                println!("Found {} consistent bridge devices", common_devices.len());

                for device in common_devices {
                    assert!(!device.id.is_empty(), "Device ID should be consistent");
                    assert_eq!(device.transport_type, TransportType::UsbBridge);
                }
            }
        }
    }

    /// Test USB bridge specific properties
    #[tokio::test]
    async fn test_usb_bridge_specific_properties() {
        println!("Testing USB bridge specific properties");

        let factory = UsbBridgeTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        for device in &devices {
            // Test bridge-specific device information
            assert!(!device.product.is_empty(), "Product should not be empty");
            assert!(!device.model.is_empty(), "Model should not be empty");
            assert!(!device.device.is_empty(), "Device should not be empty");
            assert!(!device.serial.is_empty(), "Serial should not be empty");

            // Test that bridge devices have appropriate product names
            let product_lower = device.product.to_lowercase();
            let is_bridge_product = product_lower.contains("bridge") ||
                                   product_lower.contains("link") ||
                                   product_lower.contains("pl25a1") ||
                                   product_lower.contains("host");

            if !is_bridge_product {
                println!("Warning: Device '{}' doesn't have obvious bridge product name", device.product);
            }

            println!("Found bridge device: {} - {} {}",
                    device.id, device.product, device.model);
        }
    }

    /// Test USB bridge transport properties
    #[tokio::test]
    async fn test_usb_bridge_transport_properties() {
        println!("Testing USB bridge transport properties");

        // Create a mock device for property testing
        let device_info = DeviceInfo {
            id: "bridge:067b:25a1".to_string(),
            transport_type: TransportType::UsbBridge,
            product: "PL25A1 USB Bridge".to_string(),
            model: "Host-to-Host Link".to_string(),
            device: "bridge".to_string(),
            serial: "bridge_serial_123".to_string(),
        };

        let factory = UsbBridgeTransportFactory::new();
        let transport = factory.create_transport(&device_info).await
            .expect("Transport creation should succeed");

        // Test transport properties
        assert_eq!(transport.transport_type(), TransportType::UsbBridge);
        assert!(!transport.is_connected(), "New transport should start disconnected");

        let display_name = transport.display_name();
        assert!(!display_name.is_empty(), "Display name should not be empty");

        let max_size = transport.max_transfer_size();
        assert!(max_size >= 256, "Bridge should support reasonable transfer sizes");
        assert!(max_size <= 16 * 1024 * 1024, "Max size should be reasonable");

        assert!(transport.is_bidirectional(), "Bridge transport should be bidirectional");
    }

    /// Test concurrent USB bridge operations
    #[tokio::test]
    async fn test_usb_bridge_concurrent_operations() {
        println!("Testing USB bridge concurrent operations");

        // Test concurrent device discovery
        let discovery_tasks = (0..3).map(|_| {
            let factory = UsbBridgeTransportFactory::new();
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

    /// Test USB bridge factory availability
    #[test]
    fn test_usb_bridge_factory_availability() {
        println!("Testing USB bridge factory availability");

        let factory = UsbBridgeTransportFactory::new();

        // Factory availability should be consistent
        assert_eq!(factory.is_available(), factory.is_available(),
                  "Availability should be consistent");

        // USB bridge factory should generally be available
        // (nusb should work on most platforms)
        assert!(factory.is_available(), "USB bridge factory should be available");
    }

    /// Test USB bridge timeout behavior
    #[tokio::test]
    async fn test_usb_bridge_timeout_behavior() {
        println!("Testing USB bridge timeout behavior");

        let factory = UsbBridgeTransportFactory::new();

        // Test discovery timeout
        let start = std::time::Instant::now();
        let _devices = factory.discover_devices().await
            .expect("Discovery should not fail");
        let elapsed = start.elapsed();

        // Discovery should complete in reasonable time
        assert!(elapsed < Duration::from_secs(15),
               "Bridge discovery should complete within 15 seconds");

        println!("Bridge discovery completed in {:?}", elapsed);
    }

    /// Test USB bridge selective discovery
    #[tokio::test]
    async fn test_usb_bridge_selective_discovery() {
        println!("Testing USB bridge selective discovery");

        let factory = UsbBridgeTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        // Bridge factory should only find bridge devices
        for device in &devices {
            assert_eq!(device.transport_type, TransportType::UsbBridge,
                      "Bridge factory should only find bridge devices");

            // Device should have bridge-like characteristics
            let device_info_lower = format!("{} {} {}",
                device.id.to_lowercase(),
                device.product.to_lowercase(),
                device.model.to_lowercase());

            let has_bridge_indicators = device_info_lower.contains("bridge") ||
                                       device_info_lower.contains("link") ||
                                       device_info_lower.contains("pl25a1") ||
                                       device_info_lower.contains("067b") ||
                                       device_info_lower.contains("25a1");

            if !has_bridge_indicators {
                println!("Warning: Device might not be a typical bridge: {}", device.id);
            }
        }
    }

    /// Integration test with real PL25A1 bridge hardware
    #[tokio::test]
    #[ignore] // Run with --ignored when PL25A1 bridge is connected
    async fn test_usb_bridge_with_real_pl25a1() {
        println!("Testing USB bridge with real PL25A1 hardware");

        let factory = UsbBridgeTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        if !devices.is_empty() {
            println!("Found {} USB bridge devices", devices.len());

            for device in &devices {
                println!("Bridge: {} - {} {} (Serial: {})",
                        device.id, device.product, device.model, device.serial);

                let mut transport = factory.create_transport(device).await
                    .expect("Transport creation should succeed");

                // Test connection
                println!("Attempting to connect to bridge {}", device.id);

                match tokio::time::timeout(Duration::from_secs(5), transport.connect()).await {
                    Ok(Ok(())) => {
                        println!("Successfully connected to USB bridge");
                        assert!(transport.is_connected());

                        // Test basic bridge communication
                        let test_data = b"BRIDGE_TEST_DATA";
                        match transport.send(test_data).await {
                            Ok(()) => {
                                println!("Sent test data to bridge");

                                let mut buffer = vec![0u8; 1024];
                                match tokio::time::timeout(Duration::from_secs(2), transport.receive(&mut buffer)).await {
                                    Ok(Ok(bytes)) => {
                                        println!("Received {} bytes from bridge", bytes);
                                    }
                                    Ok(Err(e)) => {
                                        println!("Receive error (expected for bridge): {}", e);
                                    }
                                    Err(_) => {
                                        println!("Receive timeout (expected for bridge)");
                                    }
                                }
                            }
                            Err(e) => {
                                println!("Send error: {}", e);
                            }
                        }

                        // Cleanup
                        transport.disconnect().await.ok();
                    }
                    Ok(Err(e)) => {
                        println!("Connection failed: {}", e);
                    }
                    Err(_) => {
                        println!("Connection timeout");
                    }
                }
            }
        } else {
            println!("No USB bridge devices found");
            println!("Make sure:");
            println!("1. PL25A1 USB bridge is connected");
            println!("2. Both ends of bridge cable are connected to USB ports");
            println!("3. Bridge drivers are properly installed");
            println!("4. Bridge is not in use by another application");
            println!("5. USB ports have sufficient power");
        }
    }

    /// Test USB bridge permissions handling
    #[tokio::test]
    async fn test_usb_bridge_permissions() {
        println!("Testing USB bridge permissions handling");

        let factory = UsbBridgeTransportFactory::new();

        // Discovery should handle permission issues gracefully
        let discovery_result = factory.discover_devices().await;
        assert!(discovery_result.is_ok(),
               "Discovery should not fail due to permission issues");

        // Should return an empty list if no permissions rather than error
        let devices = discovery_result.unwrap();
        println!("Discovery returned {} bridge devices (may be limited by permissions)", devices.len());

        // Test that we can create the factory even with potential permission issues
        assert!(factory.is_available(), "Factory should be available regardless of permissions");
    }

    /// Test USB bridge link detection
    #[tokio::test]
    async fn test_usb_bridge_link_detection() {
        println!("Testing USB bridge link detection");

        let factory = UsbBridgeTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        for device in &devices {
            // Bridge devices should have proper identification
            assert!(!device.id.is_empty(), "Bridge ID should not be empty");
            assert!(!device.serial.is_empty(), "Bridge serial should not be empty");

            // Test that the device is recognized as a bridge
            assert_eq!(device.transport_type, TransportType::UsbBridge,
                      "Device should be identified as USB bridge");

            println!("Detected bridge: {} ({})", device.id, device.product);
        }
    }
}