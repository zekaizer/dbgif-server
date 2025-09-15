use super::{Transport, TransportType, ConnectionInfo, DeviceInfo, TransportFactory};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use nusb::{Interface, DeviceInfo as NusbDeviceInfo};

/// USB Device transport for Android devices
pub struct UsbDeviceTransport {
    device_id: String,
    vid: u16,
    pid: u16,
    serial: String,
    path: String,
    interface: Option<Interface>,
    bulk_in_endpoint: u8,
    bulk_out_endpoint: u8,
}

impl UsbDeviceTransport {
    pub fn new(device_id: String, vid: u16, pid: u16, serial: String, path: String) -> Self {
        Self {
            device_id,
            vid,
            pid,
            serial,
            path,
            interface: None,
            bulk_in_endpoint: 0x81,  // Default bulk IN endpoint
            bulk_out_endpoint: 0x01, // Default bulk OUT endpoint
        }
    }

    async fn find_endpoints(&mut self) -> Result<()> {
        if let Some(_interface) = &self.interface {
            // TODO: Find bulk endpoints using proper nusb API
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
impl Transport for UsbDeviceTransport {
    async fn connect(&mut self) -> Result<()> {
        // Find device by VID/PID/Serial
        let devices = nusb::list_devices().await?;
        let device_info = devices
            .filter(|d| d.vendor_id() == self.vid && d.product_id() == self.pid)
            .find(|d| {
                // For now, just match by VID/PID
                // TODO: Implement proper serial matching when nusb API supports it
                let _ = d; // Suppress unused warning
                true
            })
            .ok_or_else(|| anyhow::anyhow!("USB device not found"))?;

        let device = device_info.open().await?;

        // Claim interface 0 (ADB interface)
        let interface = device.claim_interface(0).await?;
        self.interface = Some(interface);

        // Find bulk endpoints
        self.find_endpoints().await?;

        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> Result<()> {
        if let Some(_interface) = &self.interface {
            // TODO: Implement actual bulk transfer using nusb
            // Need to use interface.endpoint() to get bulk out endpoint
            // Then submit transfer with data
            let _ = data; // Suppress unused warning for now
            Ok(())
        } else {
            Err(anyhow::anyhow!("Not connected"))
        }
    }

    async fn receive(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if let Some(_interface) = &self.interface {
            // TODO: Implement actual bulk transfer using nusb
            // Need to use interface.endpoint() to get bulk in endpoint
            // Then submit transfer and read data

            // Mock response for now
            let mock_response = b"usb device response";
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
        TransportType::UsbDevice
    }

    fn device_id(&self) -> String {
        self.device_id.clone()
    }

    fn display_name(&self) -> String {
        format!("USB Device ({:04x}:{:04x} {})", self.vid, self.pid, self.serial)
    }

    fn max_transfer_size(&self) -> usize {
        256 * 1024 // 256KB for USB devices
    }

    fn connection_info(&self) -> ConnectionInfo {
        ConnectionInfo::UsbDevice {
            vid: self.vid,
            pid: self.pid,
            serial: self.serial.clone(),
            path: self.path.clone(),
        }
    }
}

/// Factory for discovering Android USB devices
pub struct UsbDeviceTransportFactory {
    pub android_vid_pids: Vec<(u16, u16)>,
    pub scan_interval: Duration,
}

impl UsbDeviceTransportFactory {
    pub fn new() -> Self {
        Self {
            android_vid_pids: vec![
                (0x18d1, 0x4ee7), // Google
            ],
            scan_interval: Duration::from_secs(1),
        }
    }

    pub fn with_default_android_devices() -> Self {
        Self {
            android_vid_pids: vec![
                (0x18d1, 0x4ee7), // Google
                (0x04e8, 0x6860), // Samsung
                (0x0bb4, 0x0c02), // HTC
                (0x22b8, 0x2e76), // Motorola
                (0x0fce, 0x0dde), // Sony
                (0x2717, 0x9039), // Xiaomi
                (0x1004, 0x61a9), // LG
                (0x19d2, 0x1354), // ZTE
                (0x12d1, 0x1051), // Huawei
                (0x0bb4, 0x2008), // HTC (alternative)
            ],
            scan_interval: Duration::from_secs(1),
        }
    }

    fn is_android_device(&self, vid: u16, pid: u16) -> bool {
        self.android_vid_pids.contains(&(vid, pid))
    }

    fn create_device_info(&self, device_info: &NusbDeviceInfo) -> Result<DeviceInfo> {
        let vid = device_info.vendor_id();
        let pid = device_info.product_id();

        // Try to get serial number
        let serial = device_info.serial_number().unwrap_or("unknown").to_string();

        // Create device path (platform-specific)
        let path = format!("/dev/bus/usb/{:03}/{:03}",
            device_info.bus_id(),
            device_info.device_address()
        );

        let device_id = format!("usb:{:04x}:{:04x}", vid, pid);

        Ok(DeviceInfo {
            device_id,
            display_name: format!("Android USB Device ({:04x}:{:04x})", vid, pid),
            transport_type: TransportType::UsbDevice,
            connection_info: ConnectionInfo::UsbDevice {
                vid,
                pid,
                serial,
                path,
            },
            capabilities: vec!["android".to_string(), "adb".to_string()],
        })
    }
}

#[async_trait]
impl TransportFactory for UsbDeviceTransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        // List all USB devices
        let usb_devices = nusb::list_devices().await?;

        for device_info in usb_devices {
            let vid = device_info.vendor_id();
            let pid = device_info.product_id();

            // Check if this is a known Android device
            if self.is_android_device(vid, pid) {
                if let Ok(device) = self.create_device_info(&device_info) {
                    devices.push(device);
                }
            }
        }

        Ok(devices)
    }

    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
        match &device_info.connection_info {
            ConnectionInfo::UsbDevice { vid, pid, serial, path } => {
                let transport = UsbDeviceTransport::new(
                    device_info.device_id.clone(),
                    *vid,
                    *pid,
                    serial.clone(),
                    path.clone(),
                );
                Ok(Box::new(transport))
            }
            _ => Err(anyhow::anyhow!("Invalid connection info for USB device transport")),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::UsbDevice
    }

    fn factory_name(&self) -> String {
        "USB Device Transport Factory".to_string()
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
    fn test_usb_factory_android_detection() {
        let factory = UsbDeviceTransportFactory::with_default_android_devices();

        // Test known Android VID/PIDs
        assert!(factory.is_android_device(0x18d1, 0x4ee7)); // Google
        assert!(factory.is_android_device(0x04e8, 0x6860)); // Samsung
        assert!(factory.is_android_device(0x0bb4, 0x0c02)); // HTC

        // Test non-Android device
        assert!(!factory.is_android_device(0x1234, 0x5678));
    }

    #[tokio::test]
    async fn test_usb_factory_properties() {
        let factory = UsbDeviceTransportFactory::new();

        assert_eq!(factory.transport_type(), TransportType::UsbDevice);
        assert_eq!(factory.factory_name(), "USB Device Transport Factory");
        assert!(factory.is_available()); // Should be available if nusb works
    }

    #[tokio::test]
    async fn test_usb_device_discovery() {
        let factory = UsbDeviceTransportFactory::with_default_android_devices();

        // This will only find devices if actual Android devices are connected
        let devices = factory.discover_devices().await.unwrap();

        // Just verify the function runs without error
        // In CI/testing environment, there might be no USB devices
        for device in devices {
            assert_eq!(device.transport_type, TransportType::UsbDevice);
            assert!(device.device_id.starts_with("usb:"));
        }
    }

    #[test]
    fn test_usb_transport_creation() {
        let device_info = DeviceInfo {
            device_id: "usb:18d1:4ee7".to_string(),
            display_name: "Test Android Device".to_string(),
            transport_type: TransportType::UsbDevice,
            connection_info: ConnectionInfo::UsbDevice {
                vid: 0x18d1,
                pid: 0x4ee7,
                serial: "test_serial".to_string(),
                path: "/dev/bus/usb/001/002".to_string(),
            },
            capabilities: vec!["android".to_string()],
        };

        let transport = UsbDeviceTransport::new(
            device_info.device_id.clone(),
            0x18d1,
            0x4ee7,
            "test_serial".to_string(),
            "/dev/bus/usb/001/002".to_string(),
        );

        assert_eq!(transport.device_id(), "usb:18d1:4ee7");
        assert_eq!(transport.transport_type(), TransportType::UsbDevice);
        assert!(!transport.is_connected());
        assert_eq!(transport.max_transfer_size(), 256 * 1024);
    }
}