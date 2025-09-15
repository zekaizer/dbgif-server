// Transport factory implementations for each transport type
// Defines specific contracts for TCP, USB Device, and USB Bridge factories

use async_trait::async_trait;
use anyhow::Result;
use crate::contracts::transport_trait::{TransportFactory, DeviceInfo, TransportType, ConnectionInfo};

/// TCP Transport Factory
/// Discovers ADB daemons running on network addresses
pub struct TcpTransportFactory {
    pub scan_ports: Vec<u16>,       // Default: [5555]
    pub scan_subnets: Vec<String>,  // Default: ["127.0.0.1"]
    pub connection_timeout: std::time::Duration,
}

#[async_trait]
impl TransportFactory for TcpTransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        for subnet in &self.scan_subnets {
            for port in &self.scan_ports {
                // Try to connect to each host:port combination
                if let Ok(_) = tokio::time::timeout(
                    self.connection_timeout,
                    tokio::net::TcpStream::connect(format!("{}:{}", subnet, port))
                ).await {
                    let device_info = DeviceInfo {
                        device_id: format!("tcp:{}:{}", subnet, port),
                        display_name: format!("TCP Device ({}:{})", subnet, port),
                        transport_type: TransportType::Tcp,
                        connection_info: ConnectionInfo::Tcp {
                            host: subnet.clone(),
                            port: *port,
                        },
                        capabilities: vec!["tcp".to_string()],
                    };
                    devices.push(device_info);
                }
            }
        }

        Ok(devices)
    }

    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn crate::contracts::transport_trait::Transport>> {
        match &device_info.connection_info {
            ConnectionInfo::Tcp { host, port } => {
                Ok(Box::new(TcpTransport::new(host.clone(), *port)))
            }
            _ => Err(anyhow::anyhow!("Invalid connection info for TCP transport")),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn factory_name(&self) -> &'static str {
        "TCP Transport Factory"
    }

    fn is_available(&self) -> bool {
        true // TCP is always available
    }
}

/// USB Device Transport Factory
/// Discovers Android devices connected via USB
pub struct UsbDeviceTransportFactory {
    pub android_vid_pids: Vec<(u16, u16)>,  // Known Android VID/PID combinations
    pub scan_interval: std::time::Duration,
}

impl UsbDeviceTransportFactory {
    pub fn with_default_android_devices() -> Self {
        Self {
            android_vid_pids: vec![
                (0x18d1, 0x4ee7), // Google Nexus/Pixel
                (0x04e8, 0x6860), // Samsung
                (0x0bb4, 0x0c02), // HTC
                (0x22b8, 0x2e76), // Motorola
                (0x0fce, 0x0dde), // Sony
                // Add more VID/PID combinations as needed
            ],
            scan_interval: std::time::Duration::from_secs(1),
        }
    }
}

#[async_trait]
impl TransportFactory for UsbDeviceTransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        // Use nusb to enumerate USB devices
        let context = nusb::list_devices()?;

        for device in context {
            let device_desc = device.device_descriptor()?;
            let vid = device_desc.vendor_id();
            let pid = device_desc.product_id();

            // Check if this is a known Android device
            if self.android_vid_pids.contains(&(vid, pid)) {
                let serial = device.get_string_descriptor_ascii(device_desc.serial_number_index())
                    .unwrap_or_else(|_| format!("unknown-{:04x}{:04x}", vid, pid));

                let device_info = DeviceInfo {
                    device_id: format!("usb:{}:{}", vid, pid),
                    display_name: format!("Android Device ({:04x}:{:04x})", vid, pid),
                    transport_type: TransportType::UsbDevice,
                    connection_info: ConnectionInfo::UsbDevice {
                        vid,
                        pid,
                        serial: serial.clone(),
                        path: format!("/dev/bus/usb/{:03d}/{:03d}", device.bus_number(), device.device_address()),
                    },
                    capabilities: vec!["android".to_string(), "adb".to_string()],
                };
                devices.push(device_info);
            }
        }

        Ok(devices)
    }

    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn crate::contracts::transport_trait::Transport>> {
        match &device_info.connection_info {
            ConnectionInfo::UsbDevice { vid, pid, serial, path } => {
                Ok(Box::new(UsbDeviceTransport::new(*vid, *pid, serial.clone(), path.clone())))
            }
            _ => Err(anyhow::anyhow!("Invalid connection info for USB device transport")),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::UsbDevice
    }

    fn factory_name(&self) -> &'static str {
        "USB Device Transport Factory"
    }

    fn is_available(&self) -> bool {
        // Check if nusb can enumerate devices
        nusb::list_devices().is_ok()
    }
}

/// USB Bridge Transport Factory
/// Discovers USB Host-to-Host bridge devices (PL25A1, etc.)
pub struct UsbBridgeTransportFactory {
    pub bridge_vid_pids: Vec<(u16, u16, &'static str)>,  // VID, PID, Bridge Type
}

impl UsbBridgeTransportFactory {
    pub fn with_default_bridges() -> Self {
        Self {
            bridge_vid_pids: vec![
                (0x067b, 0x25a1, "PL25A1"),  // Prolific PL25A1
                // Add more bridge types as needed
            ],
        }
    }
}

#[async_trait]
impl TransportFactory for UsbBridgeTransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        // Use nusb to enumerate USB devices
        let context = nusb::list_devices()?;

        for device in context {
            let device_desc = device.device_descriptor()?;
            let vid = device_desc.vendor_id();
            let pid = device_desc.product_id();

            // Check if this is a known bridge device
            if let Some((_, _, bridge_type)) = self.bridge_vid_pids.iter()
                .find(|(v, p, _)| *v == vid && *p == pid) {

                let device_info = DeviceInfo {
                    device_id: format!("bridge:{}:{}", vid, pid),
                    display_name: format!("USB Bridge {} ({:04x}:{:04x})", bridge_type, vid, pid),
                    transport_type: TransportType::UsbBridge,
                    connection_info: ConnectionInfo::UsbBridge {
                        vid,
                        pid,
                        path: format!("/dev/bus/usb/{:03d}/{:03d}", device.bus_number(), device.device_address()),
                        bridge_type: bridge_type.to_string(),
                    },
                    capabilities: vec!["bridge".to_string(), bridge_type.to_lowercase()],
                };
                devices.push(device_info);
            }
        }

        Ok(devices)
    }

    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn crate::contracts::transport_trait::Transport>> {
        match &device_info.connection_info {
            ConnectionInfo::UsbBridge { vid, pid, path, bridge_type } => {
                match bridge_type.as_str() {
                    "PL25A1" => Ok(Box::new(Pl25a1BridgeTransport::new(*vid, *pid, path.clone()))),
                    _ => Err(anyhow::anyhow!("Unsupported bridge type: {}", bridge_type)),
                }
            }
            _ => Err(anyhow::anyhow!("Invalid connection info for USB bridge transport")),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::UsbBridge
    }

    fn factory_name(&self) -> &'static str {
        "USB Bridge Transport Factory"
    }

    fn is_available(&self) -> bool {
        // Check if nusb can enumerate devices
        nusb::list_devices().is_ok()
    }
}

// Placeholder transport implementations - these would be implemented in actual code
struct TcpTransport {
    host: String,
    port: u16,
}

impl TcpTransport {
    fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }
}

struct UsbDeviceTransport {
    vid: u16,
    pid: u16,
    serial: String,
    path: String,
}

impl UsbDeviceTransport {
    fn new(vid: u16, pid: u16, serial: String, path: String) -> Self {
        Self { vid, pid, serial, path }
    }
}

struct Pl25a1BridgeTransport {
    vid: u16,
    pid: u16,
    path: String,
}

impl Pl25a1BridgeTransport {
    fn new(vid: u16, pid: u16, path: String) -> Self {
        Self { vid, pid, path }
    }
}

// Contract tests would verify:
// 1. TCP factory discovers network ADB daemons
// 2. USB Device factory discovers Android devices
// 3. USB Bridge factory discovers bridge cables
// 4. Each factory creates appropriate transport instances
// 5. Factory availability checks work correctly