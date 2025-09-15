use std::fmt;
use serde::{Deserialize, Serialize};

/// USB hotplug event types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotplugEventType {
    /// Device was connected to the system
    Connected,
    /// Device was disconnected from the system
    Disconnected,
}

impl fmt::Display for HotplugEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HotplugEventType::Connected => write!(f, "Connected"),
            HotplugEventType::Disconnected => write!(f, "Disconnected"),
        }
    }
}

/// Represents a USB hotplug event
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HotplugEvent {
    /// Unique device identifier (bus-port based)
    pub device_id: String,
    /// Type of hotplug event
    pub event_type: HotplugEventType,
    /// USB vendor ID
    pub vendor_id: u16,
    /// USB product ID
    pub product_id: u16,
    /// USB bus number (optional)
    pub bus_number: Option<u8>,
    /// USB device address (optional)
    pub device_address: Option<u8>,
    /// Timestamp when the event was detected
    pub timestamp: std::time::SystemTime,
}

impl HotplugEvent {
    /// Create a new hotplug event
    pub fn new(
        device_id: String,
        event_type: HotplugEventType,
        vendor_id: u16,
        product_id: u16,
    ) -> Self {
        Self {
            device_id,
            event_type,
            vendor_id,
            product_id,
            bus_number: None,
            device_address: None,
            timestamp: std::time::SystemTime::now(),
        }
    }

    /// Create a new connection event
    pub fn connected(
        device_id: String,
        vendor_id: u16,
        product_id: u16,
    ) -> Self {
        Self::new(device_id, HotplugEventType::Connected, vendor_id, product_id)
    }

    /// Create a new disconnection event
    pub fn disconnected(
        device_id: String,
        vendor_id: u16,
        product_id: u16,
    ) -> Self {
        Self::new(device_id, HotplugEventType::Disconnected, vendor_id, product_id)
    }

    /// Set bus number and device address
    pub fn with_bus_info(mut self, bus_number: u8, device_address: u8) -> Self {
        self.bus_number = Some(bus_number);
        self.device_address = Some(device_address);
        self
    }

    /// Get device path in Linux format (/dev/bus/usb/BBB/DDD)
    pub fn device_path(&self) -> Option<String> {
        match (self.bus_number, self.device_address) {
            (Some(bus), Some(addr)) => Some(format!("/dev/bus/usb/{:03}/{:03}", bus, addr)),
            _ => None,
        }
    }

    /// Check if this is a connection event
    pub fn is_connected(&self) -> bool {
        matches!(self.event_type, HotplugEventType::Connected)
    }

    /// Check if this is a disconnection event
    pub fn is_disconnected(&self) -> bool {
        matches!(self.event_type, HotplugEventType::Disconnected)
    }

    /// Generate a serial number for this device
    pub fn generate_serial(&self) -> String {
        format!("hotplug_{}", self.device_id)
    }

    /// Check if this device matches Android USB VID/PID patterns
    pub fn is_android_device(&self) -> bool {
        match self.vendor_id {
            0x18d1 => true, // Google
            0x04e8 => true, // Samsung
            0x0bb4 => true, // HTC
            0x22b8 => true, // Motorola
            0x0fce => true, // Sony Ericsson
            0x19d2 => true, // ZTE
            0x12d1 => true, // Huawei
            0x24e3 => true, // OnePlus
            0x2717 => true, // Xiaomi
            0x1004 => true, // LG
            _ => false,
        }
    }

    /// Check if this device is a bridge cable
    pub fn is_bridge_device(&self) -> bool {
        matches!((self.vendor_id, self.product_id), (0x067b, 0x25a1)) // PL-25A1
    }

    /// Get the appropriate transport type for this device
    pub fn transport_type(&self) -> crate::transport::TransportType {
        if self.is_bridge_device() {
            crate::transport::TransportType::UsbBridge
        } else {
            crate::transport::TransportType::UsbDevice
        }
    }
}

impl fmt::Display for HotplugEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "HotplugEvent {{ {} - {:04x}:{:04x} {} }}",
            self.event_type,
            self.vendor_id,
            self.product_id,
            self.device_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotplug_event_creation() {
        let event = HotplugEvent::connected(
            "usb-1-2".to_string(),
            0x18d1,
            0x4ee7,
        );

        assert_eq!(event.device_id, "usb-1-2");
        assert_eq!(event.event_type, HotplugEventType::Connected);
        assert_eq!(event.vendor_id, 0x18d1);
        assert_eq!(event.product_id, 0x4ee7);
        assert!(event.is_connected());
        assert!(!event.is_disconnected());
    }

    #[test]
    fn test_device_path_generation() {
        let event = HotplugEvent::connected("test".to_string(), 0x1234, 0x5678)
            .with_bus_info(1, 2);

        assert_eq!(event.device_path(), Some("/dev/bus/usb/001/002".to_string()));

        let event_no_bus = HotplugEvent::connected("test".to_string(), 0x1234, 0x5678);
        assert_eq!(event_no_bus.device_path(), None);
    }

    #[test]
    fn test_device_type_detection() {
        let android_event = HotplugEvent::connected("android".to_string(), 0x18d1, 0x4ee7);
        assert!(android_event.is_android_device());
        assert!(!android_event.is_bridge_device());
        assert_eq!(android_event.transport_type(), crate::transport::TransportType::UsbDevice);

        let bridge_event = HotplugEvent::connected("bridge".to_string(), 0x067b, 0x25a1);
        assert!(!bridge_event.is_android_device());
        assert!(bridge_event.is_bridge_device());
        assert_eq!(bridge_event.transport_type(), crate::transport::TransportType::UsbBridge);
    }

    #[test]
    fn test_serial_generation() {
        let event = HotplugEvent::connected("usb-1-2-3".to_string(), 0x1234, 0x5678);
        assert_eq!(event.generate_serial(), "hotplug_usb-1-2-3");
    }

    #[test]
    fn test_display() {
        let event = HotplugEvent::connected("test".to_string(), 0x18d1, 0x4ee7);
        let display = format!("{}", event);
        assert!(display.contains("Connected"));
        assert!(display.contains("18d1:4ee7"));
        assert!(display.contains("test"));
    }
}