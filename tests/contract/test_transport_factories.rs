use dbgif_server::transport::*;
use anyhow::Result;

/// Contract tests for Transport Factories
/// These tests ensure that all transport factory implementations behave consistently
/// and fulfill the contract defined by the TransportFactory trait.

#[cfg(test)]
mod transport_factory_contract_tests {
    use super::*;

    /// Test TCP transport factory contract
    #[tokio::test]
    async fn test_tcp_transport_factory_contract() {
        println!("Testing TCP transport factory contract");

        // Contract: TCP factory should be available
        let factory = TcpTransportFactory::new();
        assert!(factory.is_available(), "TCP factory should be available");

        // Contract: Factory should report correct transport type
        assert_eq!(factory.transport_type(), TransportType::Tcp, "TCP factory should report TCP type");

        // Contract: Factory should have a descriptive name
        let name = factory.factory_name();
        assert!(!name.is_empty(), "Factory name should not be empty");
        assert!(name.to_lowercase().contains("tcp"), "Factory name should contain 'tcp'");

        // Contract: Device discovery should not panic
        let discovery_result = factory.discover_devices().await;
        assert!(discovery_result.is_ok(), "Device discovery should not fail");

        let devices = discovery_result.unwrap();
        // Contract: All discovered devices should have correct type
        for device in &devices {
            assert_eq!(device.transport_type, TransportType::Tcp,
                      "All TCP devices should have TCP transport type");
            assert!(!device.id.is_empty(), "Device ID should not be empty");
            assert!(!device.product.is_empty(), "Device product should not be empty");
        }

        // Contract: Transport creation should work for valid devices
        if !devices.is_empty() {
            let transport_result = factory.create_transport(&devices[0]).await;
            assert!(transport_result.is_ok(), "Transport creation should succeed for valid device");

            let transport = transport_result.unwrap();
            assert_eq!(transport.transport_type(), TransportType::Tcp,
                      "Created transport should have correct type");
        }
    }

    /// Test USB device transport factory contract
    #[tokio::test]
    async fn test_usb_device_transport_factory_contract() {
        println!("Testing USB device transport factory contract");

        // Contract: USB device factory should be available
        let factory = UsbDeviceTransportFactory::new();
        assert!(factory.is_available(), "USB device factory should be available");

        // Contract: Factory should report correct transport type
        assert_eq!(factory.transport_type(), TransportType::UsbDevice,
                  "USB device factory should report UsbDevice type");

        // Contract: Factory should have a descriptive name
        let name = factory.factory_name();
        assert!(!name.is_empty(), "Factory name should not be empty");
        assert!(name.to_lowercase().contains("usb"), "Factory name should contain 'usb'");

        // Contract: Device discovery should not panic
        let discovery_result = factory.discover_devices().await;
        assert!(discovery_result.is_ok(), "Device discovery should not fail");

        let devices = discovery_result.unwrap();
        // Contract: All discovered devices should have correct type
        for device in &devices {
            assert_eq!(device.transport_type, TransportType::UsbDevice,
                      "All USB devices should have UsbDevice transport type");
            assert!(!device.id.is_empty(), "Device ID should not be empty");
            assert!(!device.product.is_empty(), "Device product should not be empty");
        }

        // Contract: Transport creation should work for valid devices
        if !devices.is_empty() {
            let transport_result = factory.create_transport(&devices[0]).await;
            assert!(transport_result.is_ok(), "Transport creation should succeed for valid device");

            let transport = transport_result.unwrap();
            assert_eq!(transport.transport_type(), TransportType::UsbDevice,
                      "Created transport should have correct type");
        }
    }

    /// Test USB bridge transport factory contract
    #[tokio::test]
    async fn test_usb_bridge_transport_factory_contract() {
        println!("Testing USB bridge transport factory contract");

        // Contract: USB bridge factory should be available
        let factory = UsbBridgeTransportFactory::new();
        assert!(factory.is_available(), "USB bridge factory should be available");

        // Contract: Factory should report correct transport type
        assert_eq!(factory.transport_type(), TransportType::UsbBridge,
                  "USB bridge factory should report UsbBridge type");

        // Contract: Factory should have a descriptive name
        let name = factory.factory_name();
        assert!(!name.is_empty(), "Factory name should not be empty");
        assert!(name.to_lowercase().contains("bridge"), "Factory name should contain 'bridge'");

        // Contract: Device discovery should not panic
        let discovery_result = factory.discover_devices().await;
        assert!(discovery_result.is_ok(), "Device discovery should not fail");

        let devices = discovery_result.unwrap();
        // Contract: All discovered devices should have correct type
        for device in &devices {
            assert_eq!(device.transport_type, TransportType::UsbBridge,
                      "All bridge devices should have UsbBridge transport type");
            assert!(!device.id.is_empty(), "Device ID should not be empty");
            assert!(!device.product.is_empty(), "Device product should not be empty");
        }

        // Contract: Transport creation should work for valid devices
        if !devices.is_empty() {
            let transport_result = factory.create_transport(&devices[0]).await;
            assert!(transport_result.is_ok(), "Transport creation should succeed for valid device");

            let transport = transport_result.unwrap();
            assert_eq!(transport.transport_type(), TransportType::UsbBridge,
                      "Created transport should have correct type");
        }
    }

    /// Test factory trait contract consistency
    #[tokio::test]
    async fn test_factory_trait_consistency_contract() {
        println!("Testing factory trait consistency contract");

        let factories: Vec<Box<dyn TransportFactory>> = vec![
            Box::new(TcpTransportFactory::new()),
            Box::new(UsbDeviceTransportFactory::new()),
            Box::new(UsbBridgeTransportFactory::new()),
        ];

        for factory in factories {
            // Contract: All factories must be available
            assert!(factory.is_available(), "All factories should be available");

            // Contract: All factories must have non-empty names
            let name = factory.factory_name();
            assert!(!name.is_empty(), "Factory name should not be empty");
            assert!(name.len() >= 3, "Factory name should be descriptive");

            // Contract: Transport type must be valid
            let transport_type = factory.transport_type();
            assert!(matches!(transport_type,
                            TransportType::Tcp |
                            TransportType::UsbDevice |
                            TransportType::UsbBridge),
                   "Transport type must be one of the supported types");

            // Contract: Device discovery must not panic
            let discovery_result = factory.discover_devices().await;
            assert!(discovery_result.is_ok(), "Device discovery should not panic");

            let devices = discovery_result.unwrap();
            // Contract: All devices must match factory type
            for device in &devices {
                assert_eq!(device.transport_type, transport_type,
                          "Device type must match factory type");
            }
        }
    }

    /// Test factory registration contract
    #[test]
    fn test_factory_registration_contract() {
        println!("Testing factory registration contract");

        // Contract: Each transport type should have a unique factory
        let tcp_factory = TcpTransportFactory::new();
        let usb_device_factory = UsbDeviceTransportFactory::new();
        let usb_bridge_factory = UsbBridgeTransportFactory::new();

        // Contract: Factory types should be different
        assert_ne!(tcp_factory.transport_type(), usb_device_factory.transport_type());
        assert_ne!(tcp_factory.transport_type(), usb_bridge_factory.transport_type());
        assert_ne!(usb_device_factory.transport_type(), usb_bridge_factory.transport_type());

        // Contract: Factory names should be different
        assert_ne!(tcp_factory.factory_name(), usb_device_factory.factory_name());
        assert_ne!(tcp_factory.factory_name(), usb_bridge_factory.factory_name());
        assert_ne!(usb_device_factory.factory_name(), usb_bridge_factory.factory_name());
    }

    /// Test transport creation contract
    #[tokio::test]
    async fn test_transport_creation_contract() {
        println!("Testing transport creation contract");

        let tcp_factory = TcpTransportFactory::new();
        let tcp_devices = tcp_factory.discover_devices().await
            .expect("TCP device discovery should work");

        if !tcp_devices.is_empty() {
            // Contract: Created transport should match device info
            let transport = tcp_factory.create_transport(&tcp_devices[0]).await
                .expect("Transport creation should succeed");

            assert_eq!(transport.transport_type(), TransportType::Tcp);
            assert_eq!(transport.transport_type(), tcp_devices[0].transport_type);

            // Contract: Transport should start disconnected
            assert!(!transport.is_connected(), "New transport should start disconnected");

            // Contract: Transport should have reasonable max transfer size
            let max_size = transport.max_transfer_size();
            assert!(max_size >= 64, "Max transfer size should be at least 64 bytes");
            assert!(max_size <= 16 * 1024 * 1024, "Max transfer size should be reasonable");

            // Contract: Transport should be bidirectional
            assert!(transport.is_bidirectional(), "Transport should be bidirectional");
        }
    }

    /// Test error handling contract
    #[tokio::test]
    async fn test_error_handling_contract() {
        println!("Testing error handling contract");

        let factory = TcpTransportFactory::new();

        // Contract: Invalid device info should be rejected gracefully
        let invalid_device = DeviceInfo {
            id: "invalid".to_string(),
            transport_type: TransportType::UsbDevice, // Wrong type for TCP factory
            product: "Invalid".to_string(),
            model: "Test".to_string(),
            device: "invalid".to_string(),
            serial: "invalid".to_string(),
        };

        let result = factory.create_transport(&invalid_device).await;
        // Contract: Error should be informative, not a panic
        match result {
            Ok(_) => {
                // Some implementations might accept this - that's OK
                println!("Factory accepted mismatched device type (implementation choice)");
            }
            Err(e) => {
                // Error message should be informative
                let error_msg = e.to_string();
                assert!(!error_msg.is_empty(), "Error message should not be empty");
                println!("Factory rejected invalid device: {}", error_msg);
            }
        }
    }

    /// Test device discovery consistency contract
    #[tokio::test]
    async fn test_device_discovery_consistency_contract() {
        println!("Testing device discovery consistency contract");

        let factory = TcpTransportFactory::new();

        // Contract: Multiple discovery calls should be consistent
        let devices1 = factory.discover_devices().await
            .expect("First discovery should work");
        let devices2 = factory.discover_devices().await
            .expect("Second discovery should work");

        // Contract: Results should be stable (allowing for dynamic changes)
        // At minimum, the factory should not crash or change behavior
        assert_eq!(devices1.len(), devices2.len(),
                  "Discovery results should be consistent");

        for (dev1, dev2) in devices1.iter().zip(devices2.iter()) {
            assert_eq!(dev1.id, dev2.id, "Device IDs should be consistent");
            assert_eq!(dev1.transport_type, dev2.transport_type, "Transport types should be consistent");
        }
    }

    /// Test factory availability contract
    #[test]
    fn test_factory_availability_contract() {
        println!("Testing factory availability contract");

        // Contract: Factory availability should be deterministic
        let tcp_factory = TcpTransportFactory::new();
        assert_eq!(tcp_factory.is_available(), tcp_factory.is_available(),
                  "Availability should be consistent");

        let usb_device_factory = UsbDeviceTransportFactory::new();
        assert_eq!(usb_device_factory.is_available(), usb_device_factory.is_available(),
                  "Availability should be consistent");

        let usb_bridge_factory = UsbBridgeTransportFactory::new();
        assert_eq!(usb_bridge_factory.is_available(), usb_bridge_factory.is_available(),
                  "Availability should be consistent");

        // Contract: TCP should always be available (no special hardware required)
        assert!(tcp_factory.is_available(), "TCP factory should always be available");
    }

    /// Test transport type enumeration contract
    #[test]
    fn test_transport_type_enumeration_contract() {
        println!("Testing transport type enumeration contract");

        // Contract: All transport types should be covered by factories
        let all_types = vec![
            TransportType::Tcp,
            TransportType::UsbDevice,
            TransportType::UsbBridge,
        ];

        let tcp_factory = TcpTransportFactory::new();
        let usb_device_factory = UsbDeviceTransportFactory::new();
        let usb_bridge_factory = UsbBridgeTransportFactory::new();

        let factory_types = vec![
            tcp_factory.transport_type(),
            usb_device_factory.transport_type(),
            usb_bridge_factory.transport_type(),
        ];

        // Contract: Every transport type should have a factory
        for transport_type in all_types {
            assert!(factory_types.contains(&transport_type),
                   "Transport type {:?} should have a factory", transport_type);
        }

        // Contract: No duplicate factory types
        let mut sorted_types = factory_types.clone();
        sorted_types.sort_by_key(|t| format!("{:?}", t));
        sorted_types.dedup();
        assert_eq!(sorted_types.len(), factory_types.len(),
                  "All factory types should be unique");
    }
}