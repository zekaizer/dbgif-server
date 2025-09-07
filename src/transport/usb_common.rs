use anyhow::Result;
use async_trait::async_trait;
use rusb::{Device, GlobalContext};
use std::fmt;

use super::Transport;

/// USB device information for transport matching
#[derive(Debug, Clone)]
pub struct UsbDeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus_number: u8,
    pub address: u8,
    pub serial: Option<String>,
}

impl fmt::Display for UsbDeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "VID={:04x} PID={:04x} at {}:{} (serial: {})",
            self.vendor_id,
            self.product_id,
            self.bus_number,
            self.address,
            self.serial.as_deref().unwrap_or("none")
        )
    }
}

/// Factory trait for creating USB transports based on device VID/PID
#[async_trait]
pub trait UsbTransportFactory: Send + Sync {
    /// Get list of supported VID/PID pairs
    fn supported_devices(&self) -> &[(u16, u16)];

    /// Check if this factory can handle the device
    fn matches(&self, info: &UsbDeviceInfo) -> bool {
        self.supported_devices()
            .iter()
            .any(|(vid, pid)| *vid == info.vendor_id && *pid == info.product_id)
    }

    /// Create transport instance for the device
    async fn create_transport(
        &self,
        device: Device<GlobalContext>,
    ) -> Result<Box<dyn Transport + Send>>;

    /// Get factory name for debugging
    fn name(&self) -> &str;

    /// Additional device validation (optional)
    fn validate_device(&self, device: &Device<GlobalContext>) -> Result<()> {
        // Default implementation - just check if device can be opened
        let _handle = device.open()?;
        Ok(())
    }
}

/// Helper function to extract device information
pub fn get_device_info(device: &Device<GlobalContext>) -> Result<UsbDeviceInfo> {
    let descriptor = device.device_descriptor()?;

    // Try to get serial number
    let serial = match device.open() {
        Ok(handle) => handle.read_serial_number_string_ascii(&descriptor).ok(),
        Err(_) => None,
    };

    Ok(UsbDeviceInfo {
        vendor_id: descriptor.vendor_id(),
        product_id: descriptor.product_id(),
        bus_number: device.bus_number(),
        address: device.address(),
        serial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_info_display() {
        let info = UsbDeviceInfo {
            vendor_id: 0x18d1,
            product_id: 0x4ee7,
            bus_number: 1,
            address: 2,
            serial: Some("ABC123".to_string()),
        };

        let display = format!("{}", info);
        assert!(display.contains("VID=18d1"));
        assert!(display.contains("PID=4ee7"));
        assert!(display.contains("1:2"));
        assert!(display.contains("ABC123"));
    }

    #[test]
    fn test_device_info_no_serial() {
        let info = UsbDeviceInfo {
            vendor_id: 0x18d1,
            product_id: 0x4ee7,
            bus_number: 1,
            address: 2,
            serial: None,
        };

        let display = format!("{}", info);
        assert!(display.contains("serial: none"));
    }
}
