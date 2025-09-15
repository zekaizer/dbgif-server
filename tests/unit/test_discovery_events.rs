use anyhow::Result;
use dbgif_server::discovery::{DiscoveryEvent, DeviceInfo};
use dbgif_server::transport::TransportType;

// Test data structures for hotplug event conversion
#[derive(Debug, Clone)]
pub struct HotplugEvent {
    pub device_id: String,
    pub event_type: HotplugEventType,
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus_number: Option<u8>,
    pub device_address: Option<u8>,
}

#[derive(Debug, Clone)]
pub enum HotplugEventType {
    Connected,
    Disconnected,
}

// Conversion functions that will be implemented in the actual code
pub fn convert_hotplug_to_discovery_event(hotplug_event: HotplugEvent) -> Result<DiscoveryEvent> {
    // This function will be implemented in the actual code
    // For now, return a mock implementation to test the conversion logic

    let device_info = DeviceInfo {
        serial: format!("device_{}", hotplug_event.device_id),
        vendor_id: hotplug_event.vendor_id,
        product_id: hotplug_event.product_id,
        device_path: Some(format!("/dev/bus/usb/{:03}/{:03}",
            hotplug_event.bus_number.unwrap_or(1),
            hotplug_event.device_address.unwrap_or(1)
        )),
        interface_number: Some(0),
    };

    let transport_type = match (hotplug_event.vendor_id, hotplug_event.product_id) {
        // PL-25A1 Bridge Cable
        (0x067b, 0x25a1) => TransportType::UsbBridge,
        // Android devices (Google VID as example)
        (0x18d1, _) => TransportType::UsbDevice,
        // Default to Android USB for other devices
        _ => TransportType::UsbDevice,
    };

    match hotplug_event.event_type {
        HotplugEventType::Connected => Ok(DiscoveryEvent::DeviceConnected {
            device_info,
            transport_type,
        }),
        HotplugEventType::Disconnected => Ok(DiscoveryEvent::DeviceDisconnected {
            device_info,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotplug_to_discovery_event_connected() {
        let hotplug_event = HotplugEvent {
            device_id: "usb-001-002".to_string(),
            event_type: HotplugEventType::Connected,
            vendor_id: 0x18d1,  // Google
            product_id: 0x4ee7,  // Nexus
            bus_number: Some(1),
            device_address: Some(2),
        };

        let discovery_event = convert_hotplug_to_discovery_event(hotplug_event.clone())
            .expect("Failed to convert hotplug event");

        match discovery_event {
            DiscoveryEvent::DeviceConnected { device_info, transport_type } => {
                assert_eq!(device_info.serial, "device_usb-001-002");
                assert_eq!(device_info.vendor_id, 0x18d1);
                assert_eq!(device_info.product_id, 0x4ee7);
                assert_eq!(device_info.device_path, Some("/dev/bus/usb/001/002".to_string()));
                assert!(matches!(transport_type, TransportType::UsbDevice));
            }
            _ => panic!("Expected DeviceConnected event"),
        }
    }

    #[test]
    fn test_hotplug_to_discovery_event_disconnected() {
        let hotplug_event = HotplugEvent {
            device_id: "usb-001-003".to_string(),
            event_type: HotplugEventType::Disconnected,
            vendor_id: 0x067b,  // Prolific
            product_id: 0x25a1,  // PL-25A1
            bus_number: Some(1),
            device_address: Some(3),
        };

        let discovery_event = convert_hotplug_to_discovery_event(hotplug_event)
            .expect("Failed to convert hotplug event");

        match discovery_event {
            DiscoveryEvent::DeviceDisconnected { device_info } => {
                assert_eq!(device_info.serial, "device_usb-001-003");
                assert_eq!(device_info.vendor_id, 0x067b);
                assert_eq!(device_info.product_id, 0x25a1);
                assert_eq!(device_info.device_path, Some("/dev/bus/usb/001/003".to_string()));
            }
            _ => panic!("Expected DeviceDisconnected event"),
        }
    }

    #[test]
    fn test_transport_type_detection() {
        // Test PL-25A1 Bridge Cable detection
        let bridge_event = HotplugEvent {
            device_id: "bridge".to_string(),
            event_type: HotplugEventType::Connected,
            vendor_id: 0x067b,
            product_id: 0x25a1,
            bus_number: None,
            device_address: None,
        };

        let discovery_event = convert_hotplug_to_discovery_event(bridge_event)
            .expect("Failed to convert bridge event");

        match discovery_event {
            DiscoveryEvent::DeviceConnected { transport_type, .. } => {
                assert!(matches!(transport_type, TransportType::UsbBridge));
            }
            _ => panic!("Expected DeviceConnected event"),
        }

        // Test Android device detection
        let android_event = HotplugEvent {
            device_id: "android".to_string(),
            event_type: HotplugEventType::Connected,
            vendor_id: 0x18d1,  // Google
            product_id: 0x4ee7,
            bus_number: None,
            device_address: None,
        };

        let discovery_event = convert_hotplug_to_discovery_event(android_event)
            .expect("Failed to convert Android event");

        match discovery_event {
            DiscoveryEvent::DeviceConnected { transport_type, .. } => {
                assert!(matches!(transport_type, TransportType::UsbDevice));
            }
            _ => panic!("Expected DeviceConnected event"),
        }
    }

    #[test]
    fn test_device_path_generation() {
        let event_with_bus_addr = HotplugEvent {
            device_id: "test".to_string(),
            event_type: HotplugEventType::Connected,
            vendor_id: 0x1234,
            product_id: 0x5678,
            bus_number: Some(5),
            device_address: Some(10),
        };

        let discovery_event = convert_hotplug_to_discovery_event(event_with_bus_addr)
            .expect("Failed to convert event");

        match discovery_event {
            DiscoveryEvent::DeviceConnected { device_info, .. } => {
                assert_eq!(device_info.device_path, Some("/dev/bus/usb/005/010".to_string()));
            }
            _ => panic!("Expected DeviceConnected event"),
        }

        // Test default values when bus/addr are None
        let event_without_bus_addr = HotplugEvent {
            device_id: "test2".to_string(),
            event_type: HotplugEventType::Connected,
            vendor_id: 0x1234,
            product_id: 0x5678,
            bus_number: None,
            device_address: None,
        };

        let discovery_event = convert_hotplug_to_discovery_event(event_without_bus_addr)
            .expect("Failed to convert event");

        match discovery_event {
            DiscoveryEvent::DeviceConnected { device_info, .. } => {
                assert_eq!(device_info.device_path, Some("/dev/bus/usb/001/001".to_string()));
            }
            _ => panic!("Expected DeviceConnected event"),
        }
    }

    #[test]
    fn test_serial_generation() {
        let hotplug_event = HotplugEvent {
            device_id: "my_device_123".to_string(),
            event_type: HotplugEventType::Connected,
            vendor_id: 0xabcd,
            product_id: 0xef01,
            bus_number: None,
            device_address: None,
        };

        let discovery_event = convert_hotplug_to_discovery_event(hotplug_event)
            .expect("Failed to convert event");

        match discovery_event {
            DiscoveryEvent::DeviceConnected { device_info, .. } => {
                assert_eq!(device_info.serial, "device_my_device_123");
            }
            _ => panic!("Expected DeviceConnected event"),
        }
    }
}