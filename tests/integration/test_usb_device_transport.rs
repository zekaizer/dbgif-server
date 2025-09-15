use dbgif_server::transport::*;
use tokio::time::Duration;
use anyhow::Result;

/// Integration tests for USB Device Transport
/// These tests verify USB device transport functionality with nusb library.

#[cfg(test)]
mod usb_device_transport_integration_tests {
    use super::*;

    /// Test USB device factory properties
    #[tokio::test]
    async fn test_usb_device_factory_properties() {
        println!("Testing USB device factory properties");

        let factory = UsbDeviceTransportFactory::new();

        // Test factory properties
        assert_eq!(factory.transport_type(), TransportType::UsbDevice);
        assert!(factory.is_available(), "USB device factory should be available");

        let factory_name = factory.factory_name();
        assert!(!factory_name.is_empty(), "Factory name should not be empty");
        assert!(factory_name.to_lowercase().contains("usb"),
               "Factory name should contain 'usb'");
        assert!(factory_name.to_lowercase().contains("device"),
               "Factory name should contain 'device'");
    }

    /// Test USB device discovery
    #[tokio::test]
    async fn test_usb_device_discovery() {
        println!("Testing USB device discovery");

        let factory = UsbDeviceTransportFactory::new();

        // Test device discovery (may find 0 or more devices)
        let discovery_result = factory.discover_devices().await;
        assert!(discovery_result.is_ok(), "Device discovery should not fail");

        let devices = discovery_result.unwrap();
        println!("Discovered {} USB devices", devices.len());

        // Test that all discovered devices are USB device type
        for device in &devices {
            assert_eq!(device.transport_type, TransportType::UsbDevice,
                      "All discovered devices should be UsbDevice type");
            assert!(!device.id.is_empty(), "Device ID should not be empty");
            assert!(!device.product.is_empty(), "Device product should not be empty");
            assert!(!device.serial.is_empty(), "Device serial should not be empty");

            // Test device ID format
            assert!(device.id.starts_with("usb:") || device.id.contains(":"),
                   "Device ID should have proper format");
        }
    }

    /// Test USB device transport creation
    #[tokio::test]
    async fn test_usb_device_transport_creation() {
        println!("Testing USB device transport creation");

        let factory = UsbDeviceTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        if !devices.is_empty() {
            let transport_result = factory.create_transport(&devices[0]).await;
            assert!(transport_result.is_ok(), "Transport creation should succeed");

            let transport = transport_result.unwrap();
            assert_eq!(transport.transport_type(), TransportType::UsbDevice);
            assert!(!transport.is_connected(), "New transport should start disconnected");

            let max_size = transport.max_transfer_size();
            assert!(max_size >= 64, "Max transfer size should be at least 64 bytes");
            assert!(max_size <= 16 * 1024 * 1024, "Max transfer size should be reasonable");

            assert!(transport.is_bidirectional(), "Transport should be bidirectional");

            let display_name = transport.display_name();
            assert!(!display_name.is_empty(), "Display name should not be empty");
            assert!(display_name.to_lowercase().contains("usb"),
                   "Display name should contain 'usb'");
        } else {
            println!("No USB devices found for transport creation test");
        }
    }

    /// Test USB device transport operations
    #[tokio::test]
    async fn test_usb_device_transport_operations() {
        println!("Testing USB device transport operations");

        let factory = UsbDeviceTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        if !devices.is_empty() {
            let mut transport = factory.create_transport(&devices[0]).await
                .expect("Transport creation should succeed");

            // Test initial state
            assert_eq!(transport.transport_type(), TransportType::UsbDevice);
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
                    println!("Successfully connected to USB device");
                    assert!(transport.is_connected(), "Transport should be connected");

                    // Test data transfer (may fail depending on device state)
                    let send_result = transport.send(b"CNXN\x00\x00\x00\x00").await;
                    match send_result {
                        Ok(()) => {
                            println!("Successfully sent data to USB device");

                            // Try to receive response
                            let mut buffer = vec![0u8; 1024];
                            match transport.receive(&mut buffer).await {
                                Ok(bytes) => {
                                    println!("Received {} bytes from USB device", bytes);
                                }
                                Err(e) => {
                                    println!("Receive failed (expected): {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            println!("Send failed (may be expected): {}", e);
                        }
                    }

                    // Test disconnect
                    let disconnect_result = transport.disconnect().await;
                    assert!(disconnect_result.is_ok(), "Disconnect should succeed");
                    assert!(!transport.is_connected(), "Transport should be disconnected");
                }
                Err(e) => {
                    println!("Connection failed (may be expected): {}", e);
                    assert!(!transport.is_connected(), "Should remain disconnected on failed connect");
                }
            }
        } else {
            println!("No USB devices found for operations test");
        }
    }

    /// Test USB device error handling
    #[tokio::test]
    async fn test_usb_device_error_handling() {
        println!("Testing USB device error handling");

        let factory = UsbDeviceTransportFactory::new();

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

    /// Test USB device discovery consistency
    #[tokio::test]
    async fn test_usb_device_discovery_consistency() {
        println!("Testing USB device discovery consistency");

        let factory = UsbDeviceTransportFactory::new();

        // Test multiple discovery attempts
        let devices1 = factory.discover_devices().await
            .expect("First discovery should work");

        // Small delay to allow for any system changes
        tokio::time::sleep(Duration::from_millis(100)).await;

        let devices2 = factory.discover_devices().await
            .expect("Second discovery should work");

        // Results should generally be consistent
        println!("First discovery: {} devices", devices1.len());
        println!("Second discovery: {} devices", devices2.len());

        // Device counts might vary due to hotplug events, but should be reasonable
        let diff = (devices1.len() as i32 - devices2.len() as i32).abs();
        assert!(diff <= 2, "Device count shouldn't change dramatically between discoveries");

        // If we found devices both times, check consistency
        if !devices1.is_empty() && !devices2.is_empty() {
            // Look for common devices
            let common_devices: Vec<_> = devices1
                .iter()
                .filter(|d1| devices2.iter().any(|d2| d1.id == d2.id))
                .collect();

            if !common_devices.is_empty() {
                println!("Found {} consistent devices", common_devices.len());

                for device in common_devices {
                    assert!(!device.id.is_empty(), "Device ID should be consistent");
                    assert_eq!(device.transport_type, TransportType::UsbDevice);
                }
            }
        }
    }

    /// Test USB device vendor/product detection
    #[tokio::test]
    async fn test_usb_device_vendor_detection() {
        println!("Testing USB device vendor detection");

        let factory = UsbDeviceTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        for device in &devices {
            // Test device information completeness
            assert!(!device.product.is_empty(), "Product should not be empty");
            assert!(!device.model.is_empty(), "Model should not be empty");
            assert!(!device.device.is_empty(), "Device should not be empty");
            assert!(!device.serial.is_empty(), "Serial should not be empty");

            // Test ID format
            assert!(device.id.len() > 3, "Device ID should be meaningful length");

            println!("Found device: {} - {} {}",
                    device.id, device.product, device.model);
        }
    }

    /// Test USB device transport properties
    #[tokio::test]
    async fn test_usb_device_transport_properties() {
        println!("Testing USB device transport properties");

        // Create a mock device for property testing
        let device_info = DeviceInfo {
            id: "usb:18d1:4ee7".to_string(),
            transport_type: TransportType::UsbDevice,
            product: "Android".to_string(),
            model: "Test Device".to_string(),
            device: "android".to_string(),
            serial: "test_serial_123".to_string(),
        };

        let factory = UsbDeviceTransportFactory::new();
        let transport = factory.create_transport(&device_info).await
            .expect("Transport creation should succeed");

        // Test transport properties
        assert_eq!(transport.transport_type(), TransportType::UsbDevice);
        assert!(!transport.is_connected(), "New transport should start disconnected");

        let display_name = transport.display_name();
        assert!(!display_name.is_empty(), "Display name should not be empty");

        let max_size = transport.max_transfer_size();
        assert!(max_size >= 512, "USB should support reasonable transfer sizes");
        assert!(max_size <= 16 * 1024 * 1024, "Max size should be reasonable");

        assert!(transport.is_bidirectional(), "USB transport should be bidirectional");
    }

    /// Test concurrent USB operations
    #[tokio::test]
    async fn test_usb_device_concurrent_operations() {
        println!("Testing USB device concurrent operations");

        // Test concurrent device discovery
        let discovery_tasks = (0..3).map(|_| {
            let factory = UsbDeviceTransportFactory::new();
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

    /// Test USB device factory availability
    #[test]
    fn test_usb_device_factory_availability() {
        println!("Testing USB device factory availability");

        let factory = UsbDeviceTransportFactory::new();

        // Factory availability should be consistent
        assert_eq!(factory.is_available(), factory.is_available(),
                  "Availability should be consistent");

        // USB device factory should generally be available
        // (nusb should work on most platforms)
        assert!(factory.is_available(), "USB device factory should be available");
    }

    /// Test USB device timeout behavior
    #[tokio::test]
    async fn test_usb_device_timeout_behavior() {
        println!("Testing USB device timeout behavior");

        let factory = UsbDeviceTransportFactory::new();

        // Test discovery timeout
        let start = std::time::Instant::now();
        let _devices = factory.discover_devices().await
            .expect("Discovery should not fail");
        let elapsed = start.elapsed();

        // Discovery should complete in reasonable time
        assert!(elapsed < Duration::from_secs(10),
               "Discovery should complete within 10 seconds");

        println!("Discovery completed in {:?}", elapsed);
    }

    /// Integration test with real Android device
    #[tokio::test]
    #[ignore] // Run with --ignored when Android device is connected
    async fn test_usb_device_with_real_android() {
        println!("Testing USB device with real Android device");

        let factory = UsbDeviceTransportFactory::new();
        let devices = factory.discover_devices().await
            .expect("Device discovery should work");

        if !devices.is_empty() {
            println!("Found {} USB devices", devices.len());

            for device in &devices {
                println!("Device: {} - {} {} (Serial: {})",
                        device.id, device.product, device.model, device.serial);

                let mut transport = factory.create_transport(device).await
                    .expect("Transport creation should succeed");

                // Test connection
                println!("Attempting to connect to {}", device.id);

                match tokio::time::timeout(Duration::from_secs(5), transport.connect()).await {
                    Ok(Ok(())) => {
                        println!("Successfully connected to Android device");
                        assert!(transport.is_connected());

                        // Test ADB handshake
                        let cnxn_msg = b"CNXN\x00\x00\x00\x01\x00\x00\x40\x00\x0c\x00\x00\x00\x32\x05\x00\x00\xbc\xb1\xa7\xb1host::\x00";

                        match transport.send(cnxn_msg).await {
                            Ok(()) => {
                                println!("Sent CNXN message to device");

                                let mut buffer = vec![0u8; 1024];
                                match tokio::time::timeout(Duration::from_secs(2), transport.receive(&mut buffer)).await {
                                    Ok(Ok(bytes)) => {
                                        println!("Received {} bytes response from device", bytes);
                                        if bytes >= 24 {
                                            let cmd = &buffer[0..4];
                                            println!("Response command: {:?}", std::str::from_utf8(cmd).unwrap_or("???"));
                                        }
                                    }
                                    Ok(Err(e)) => {
                                        println!("Receive error: {}", e);
                                    }
                                    Err(_) => {
                                        println!("Receive timeout");
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
            println!("No USB devices found");
            println!("Make sure:");
            println!("1. Android device is connected via USB");
            println!("2. USB debugging is enabled");
            println!("3. Device is authorized for ADB");
            println!("4. Proper USB drivers are installed");
            println!("5. Device is not in use by another ADB instance");
        }
    }

    /// Test USB device permissions handling
    #[tokio::test]
    async fn test_usb_device_permissions() {
        println!("Testing USB device permissions handling");

        let factory = UsbDeviceTransportFactory::new();

        // Discovery should handle permission issues gracefully
        let discovery_result = factory.discover_devices().await;
        assert!(discovery_result.is_ok(),
               "Discovery should not fail due to permission issues");

        // Should return an empty list if no permissions rather than error
        let devices = discovery_result.unwrap();
        println!("Discovery returned {} devices (may be limited by permissions)", devices.len());

        // Test that we can create the factory even with potential permission issues
        assert!(factory.is_available(), "Factory should be available regardless of permissions");
    }
}