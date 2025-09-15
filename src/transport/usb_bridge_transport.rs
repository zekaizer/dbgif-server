use super::{Transport, TransportType, ConnectionInfo, DeviceInfo, TransportFactory};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use nusb::{Interface, DeviceInfo as NusbDeviceInfo};

/// USB Bridge transport for PL25A1 Host-to-Host cables
pub struct UsbBridgeTransport {
    device_id: String,
    vid: u16,
    pid: u16,
    serial: String,
    path: String,
    interface: Option<Interface>,
    bulk_in_endpoint: u8,
    bulk_out_endpoint: u8,
    bridge_status: String,
}

impl UsbBridgeTransport {
    pub fn new(device_id: String, vid: u16, pid: u16, serial: String, path: String, bridge_status: String) -> Self {
        Self {
            device_id,
            vid,
            pid,
            serial,
            path,
            interface: None,
            bulk_in_endpoint: 0x81,  // Default bulk IN endpoint
            bulk_out_endpoint: 0x01, // Default bulk OUT endpoint
            bridge_status,
        }
    }

    /// Check if this bridge supports status monitoring
    pub fn supports_bridge_status_monitoring(&self) -> bool {
        self.vid == 0x067b && self.pid == 0x25a1 // PL25A1 specific
    }

    /// Check if this bridge supports link control
    pub fn supports_link_control(&self) -> bool {
        self.vid == 0x067b && self.pid == 0x25a1 // PL25A1 specific
    }

    /// Set link state (PL25A1 specific vendor command)
    pub async fn set_link_state(&mut self, _enabled: bool) -> Result<()> {
        if !self.supports_link_control() {
            return Err(anyhow::anyhow!("Bridge does not support link control"));
        }

        if self.interface.is_none() {
            return Err(anyhow::anyhow!("Not connected"));
        }

        // TODO: Implement actual vendor command to PL25A1
        // This would involve sending a control transfer with vendor-specific request
        Ok(())
    }

    /// Get current link status (PL25A1 specific vendor command)
    pub async fn get_link_status(&mut self) -> Result<bool> {
        if !self.supports_bridge_status_monitoring() {
            return Err(anyhow::anyhow!("Bridge does not support status monitoring"));
        }

        if self.interface.is_none() {
            return Err(anyhow::anyhow!("Not connected"));
        }

        // TODO: Implement actual vendor command to query PL25A1 status
        // For now, return based on stored status
        Ok(self.bridge_status == "connected")
    }

    /// Get bridge status string
    pub async fn get_bridge_status(&mut self) -> Result<String> {
        if self.interface.is_none() {
            return Err(anyhow::anyhow!("Not connected"));
        }

        Ok(self.bridge_status.clone())
    }

    /// Reset the link (PL25A1 specific vendor command)
    pub async fn reset_link(&mut self) -> Result<()> {
        if !self.supports_link_control() {
            return Err(anyhow::anyhow!("Bridge does not support link control"));
        }

        if self.interface.is_none() {
            return Err(anyhow::anyhow!("Not connected"));
        }

        // TODO: Implement actual vendor command to reset PL25A1
        Ok(())
    }

    /// Get device information (PL25A1 specific vendor command)
    pub async fn get_device_info(&mut self) -> Result<String> {
        if self.interface.is_none() {
            return Err(anyhow::anyhow!("Not connected"));
        }

        // TODO: Implement actual vendor command to get device info
        Ok(format!("PL25A1 USB Bridge - Serial: {}", self.serial))
    }

    /// Get firmware version (PL25A1 specific vendor command)
    pub async fn get_firmware_version(&mut self) -> Result<String> {
        if self.interface.is_none() {
            return Err(anyhow::anyhow!("Not connected"));
        }

        // TODO: Implement actual vendor command to get firmware version
        Ok("1.0.0".to_string())
    }

    /// Configure bridge mode (PL25A1 specific vendor command)
    pub async fn configure_bridge_mode(&mut self, _enabled: bool) -> Result<()> {
        if self.interface.is_none() {
            return Err(anyhow::anyhow!("Not connected"));
        }

        // TODO: Implement actual vendor command to configure bridge mode
        Ok(())
    }

    /// Set transfer mode (PL25A1 specific vendor command)
    pub async fn set_transfer_mode(&mut self, _mode: &str) -> Result<()> {
        if self.interface.is_none() {
            return Err(anyhow::anyhow!("Not connected"));
        }

        // TODO: Implement actual vendor command to set transfer mode
        Ok(())
    }

    async fn find_endpoints(&mut self) -> Result<()> {
        if let Some(_interface) = &self.interface {
            // TODO: Find bulk endpoints similar to USB device transport
            // For now, use default endpoints
            self.bulk_in_endpoint = 0x81;
            self.bulk_out_endpoint = 0x01;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No interface available"))
        }
    }
}

#[async_trait]
impl Transport for UsbBridgeTransport {
    async fn connect(&mut self) -> Result<()> {
        // Find PL25A1 device by VID/PID/Serial
        let devices = nusb::list_devices().await?;
        let device_info = devices
            .filter(|d| d.vendor_id() == self.vid && d.product_id() == self.pid)
            .find(|_d| {
                // TODO: Match serial number when nusb API is fixed
                true // For now, take the first matching VID/PID
            })
            .ok_or_else(|| anyhow::anyhow!("USB bridge device not found"))?;

        let device = device_info.open().await?;

        // Claim interface 0 (bridge interface)
        let interface = device.claim_interface(0).await?;
        self.interface = Some(interface);

        // Find bulk endpoints
        self.find_endpoints().await?;

        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> Result<()> {
        if let Some(_interface) = &self.interface {
            // TODO: Implement actual bulk out transfer
            // let completion = interface.bulk_out(self.bulk_out_endpoint, data.to_vec());
            // completion.await?;
            let _ = data; // Suppress unused warning for now
            Ok(())
        } else {
            Err(anyhow::anyhow!("Not connected"))
        }
    }

    async fn receive(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if let Some(_interface) = &self.interface {
            // TODO: Implement actual bulk in transfer
            // let completion = interface.bulk_in(self.bulk_in_endpoint, buffer.len());
            // let data = completion.await?;
            // let copy_len = std::cmp::min(buffer.len(), data.len());
            // buffer[..copy_len].copy_from_slice(&data[..copy_len]);
            // Ok(copy_len)

            // Mock response for now
            let mock_response = b"bridge response data";
            let copy_len = std::cmp::min(buffer.len(), mock_response.len());
            buffer[..copy_len].copy_from_slice(&mock_response[..copy_len]);
            Ok(copy_len)
        } else {
            Err(anyhow::anyhow!("Not connected"))
        }
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.interface = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.interface.is_some()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::UsbBridge
    }

    fn device_id(&self) -> String {
        self.device_id.clone()
    }

    fn display_name(&self) -> String {
        format!("USB Bridge ({:04x}:{:04x} {})", self.vid, self.pid, self.serial)
    }

    fn max_transfer_size(&self) -> usize {
        64 * 1024 // 64KB for USB 2.0 bridge
    }

    fn connection_info(&self) -> ConnectionInfo {
        ConnectionInfo::UsbBridge {
            vid: self.vid,
            pid: self.pid,
            serial: self.serial.clone(),
            path: self.path.clone(),
            bridge_status: self.bridge_status.clone(),
        }
    }
}

/// Factory for discovering PL25A1 USB bridge devices
pub struct UsbBridgeTransportFactory {
    pub target_vid: u16,
    pub target_pid: u16,
    pub scan_interval: Duration,
}

impl UsbBridgeTransportFactory {
    pub fn new() -> Self {
        Self {
            target_vid: 0x067b, // Prolific
            target_pid: 0x25a1, // PL25A1
            scan_interval: Duration::from_secs(2),
        }
    }

    pub fn with_config(scan_interval: Duration) -> Self {
        Self {
            target_vid: 0x067b,
            target_pid: 0x25a1,
            scan_interval,
        }
    }

    fn create_device_info(&self, device_info: &NusbDeviceInfo) -> Result<DeviceInfo> {
        let vid = device_info.vendor_id();
        let pid = device_info.product_id();

        // Try to get serial number
        let serial = "mock_pl25a1_serial".to_string(); // TODO: Get actual serial when nusb API is fixed

        // Create device path (platform-specific)
        let path = format!("/dev/bus/usb/{:03}/{:03}",
            device_info.bus_id(),
            device_info.device_address()
        );

        let device_id = format!("usb_bridge:{:04x}:{:04x}", vid, pid);

        // TODO: Query actual bridge status via vendor commands
        let bridge_status = "connected".to_string();

        Ok(DeviceInfo {
            device_id,
            display_name: format!("PL25A1 USB Bridge ({:04x}:{:04x})", vid, pid),
            transport_type: TransportType::UsbBridge,
            connection_info: ConnectionInfo::UsbBridge {
                vid,
                pid,
                serial,
                path,
                bridge_status,
            },
            capabilities: vec!["usb_bridge".to_string(), "host_to_host".to_string()],
        })
    }
}

#[async_trait]
impl TransportFactory for UsbBridgeTransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        // List all USB devices
        let usb_devices = nusb::list_devices().await?;

        for device_info in usb_devices {
            let vid = device_info.vendor_id();
            let pid = device_info.product_id();

            // Check if this is a PL25A1 bridge
            if vid == self.target_vid && pid == self.target_pid {
                if let Ok(device) = self.create_device_info(&device_info) {
                    devices.push(device);
                }
            }
        }

        Ok(devices)
    }

    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
        match &device_info.connection_info {
            ConnectionInfo::UsbBridge { vid, pid, serial, path, bridge_status } => {
                let transport = UsbBridgeTransport::new(
                    device_info.device_id.clone(),
                    *vid,
                    *pid,
                    serial.clone(),
                    path.clone(),
                    bridge_status.clone(),
                );
                Ok(Box::new(transport))
            }
            _ => Err(anyhow::anyhow!("Invalid connection info for USB bridge transport")),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::UsbBridge
    }

    fn factory_name(&self) -> String {
        "USB Bridge Transport Factory".to_string()
    }

    fn is_available(&self) -> bool {
        // Check if nusb can list devices (basic USB support available)
        true // Assume USB support is available
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usb_bridge_properties() {
        let factory = UsbBridgeTransportFactory::new();

        assert_eq!(factory.target_vid, 0x067b);
        assert_eq!(factory.target_pid, 0x25a1);
        assert_eq!(factory.transport_type(), TransportType::UsbBridge);
        assert_eq!(factory.factory_name(), "USB Bridge Transport Factory");
    }

    #[test]
    fn test_usb_bridge_transport_creation() {
        let device_info = DeviceInfo {
            device_id: "usb_bridge:067b:25a1".to_string(),
            display_name: "Test PL25A1 Bridge".to_string(),
            transport_type: TransportType::UsbBridge,
            connection_info: ConnectionInfo::UsbBridge {
                vid: 0x067b,
                pid: 0x25a1,
                serial: "test_bridge_serial".to_string(),
                path: "/dev/bus/usb/001/004".to_string(),
                bridge_status: "connected".to_string(),
            },
            capabilities: vec!["usb_bridge".to_string()],
        };

        let transport = UsbBridgeTransport::new(
            device_info.device_id.clone(),
            0x067b,
            0x25a1,
            "test_bridge_serial".to_string(),
            "/dev/bus/usb/001/004".to_string(),
            "connected".to_string(),
        );

        assert_eq!(transport.device_id(), "usb_bridge:067b:25a1");
        assert_eq!(transport.transport_type(), TransportType::UsbBridge);
        assert!(!transport.is_connected());
        assert_eq!(transport.max_transfer_size(), 64 * 1024);
        assert!(transport.supports_bridge_status_monitoring());
        assert!(transport.supports_link_control());
    }

    #[tokio::test]
    async fn test_usb_bridge_vendor_commands() {
        let mut transport = UsbBridgeTransport::new(
            "usb_bridge:067b:25a1".to_string(),
            0x067b,
            0x25a1,
            "test_serial".to_string(),
            "/dev/bus/usb/001/004".to_string(),
            "connected".to_string(),
        );

        // Test vendor commands when not connected (should fail)
        assert!(transport.get_bridge_status().await.is_err());
        assert!(transport.set_link_state(true).await.is_err());
        assert!(transport.get_device_info().await.is_err());
    }

    #[tokio::test]
    async fn test_usb_bridge_discovery() {
        let factory = UsbBridgeTransportFactory::new();

        // This will only find devices if actual PL25A1 bridges are connected
        let devices = factory.discover_devices().await.unwrap();

        // Just verify the function runs without error
        // In CI/testing environment, there might be no USB bridges
        for device in devices {
            assert_eq!(device.transport_type, TransportType::UsbBridge);
            assert!(device.device_id.starts_with("usb_bridge:"));

            // Verify it's actually a PL25A1
            if let ConnectionInfo::UsbBridge { vid, pid, .. } = device.connection_info {
                assert_eq!(vid, 0x067b);
                assert_eq!(pid, 0x25a1);
            }
        }
    }
}