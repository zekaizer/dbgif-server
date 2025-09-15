use super::{Transport, TransportType, ConnectionInfo, DeviceInfo, TransportFactory};
use anyhow::Result;
use async_trait::async_trait;

/// Mock transport for testing purposes
pub struct MockTransport {
    device_id: String,
    transport_type: TransportType,
    connection_info: ConnectionInfo,
    connected: bool,
    max_size: usize,
    response_data: Vec<u8>,
}

impl MockTransport {
    pub fn new_tcp(device_id: String, host: String, port: u16) -> Self {
        Self {
            device_id: device_id.clone(),
            transport_type: TransportType::Tcp,
            connection_info: ConnectionInfo::Tcp { host, port },
            connected: false,
            max_size: 64 * 1024, // 64KB
            response_data: b"mock response".to_vec(),
        }
    }

    pub fn new_usb_device(device_id: String, vid: u16, pid: u16, serial: String, path: String) -> Self {
        Self {
            device_id: device_id.clone(),
            transport_type: TransportType::UsbDevice,
            connection_info: ConnectionInfo::UsbDevice { vid, pid, serial, path },
            connected: false,
            max_size: 256 * 1024, // 256KB
            response_data: b"usb device response".to_vec(),
        }
    }

    pub fn new_usb_bridge(device_id: String, vid: u16, pid: u16, serial: String, path: String, bridge_status: String) -> Self {
        Self {
            device_id: device_id.clone(),
            transport_type: TransportType::UsbBridge,
            connection_info: ConnectionInfo::UsbBridge { vid, pid, serial, path, bridge_status },
            connected: false,
            max_size: 64 * 1024, // 64KB for USB 2.0
            response_data: b"bridge response data".to_vec(),
        }
    }

    pub fn set_response_data(&mut self, data: Vec<u8>) {
        self.response_data = data;
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn connect(&mut self) -> Result<()> {
        self.connected = true;
        Ok(())
    }

    async fn send(&mut self, _data: &[u8]) -> Result<()> {
        if !self.connected {
            return Err(anyhow::anyhow!("Not connected"));
        }
        Ok(())
    }

    async fn receive(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if !self.connected {
            return Err(anyhow::anyhow!("Not connected"));
        }

        let copy_len = std::cmp::min(buffer.len(), self.response_data.len());
        buffer[..copy_len].copy_from_slice(&self.response_data[..copy_len]);
        Ok(copy_len)
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn transport_type(&self) -> TransportType {
        self.transport_type
    }

    fn device_id(&self) -> String {
        self.device_id.clone()
    }

    fn display_name(&self) -> String {
        format!("Mock {} Transport ({})", self.transport_type, self.device_id)
    }

    fn max_transfer_size(&self) -> usize {
        self.max_size
    }

    fn connection_info(&self) -> ConnectionInfo {
        self.connection_info.clone()
    }
}

/// Mock factory for testing all transport types
pub struct MockTransportFactory {
    transport_type: TransportType,
}

impl MockTransportFactory {
    pub fn new(transport_type: TransportType) -> Self {
        Self { transport_type }
    }
}

#[async_trait]
impl TransportFactory for MockTransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        // Return mock devices based on transport type
        let device = match self.transport_type {
            TransportType::Tcp => DeviceInfo {
                device_id: "tcp:127.0.0.1:5555".to_string(),
                display_name: "Mock TCP Device".to_string(),
                transport_type: TransportType::Tcp,
                connection_info: ConnectionInfo::Tcp {
                    host: "127.0.0.1".to_string(),
                    port: 5555,
                },
                capabilities: vec!["tcp".to_string(), "adb".to_string()],
            },
            TransportType::UsbDevice => DeviceInfo {
                device_id: "usb:18d1:4ee7".to_string(),
                display_name: "Mock Android Device".to_string(),
                transport_type: TransportType::UsbDevice,
                connection_info: ConnectionInfo::UsbDevice {
                    vid: 0x18d1,
                    pid: 0x4ee7,
                    serial: "mock_serial_123".to_string(),
                    path: "/dev/bus/usb/001/002".to_string(),
                },
                capabilities: vec!["android".to_string(), "adb".to_string()],
            },
            TransportType::UsbBridge => DeviceInfo {
                device_id: "usb_bridge:067b:25a1".to_string(),
                display_name: "Mock PL25A1 USB Bridge".to_string(),
                transport_type: TransportType::UsbBridge,
                connection_info: ConnectionInfo::UsbBridge {
                    vid: 0x067b,
                    pid: 0x25a1,
                    serial: "mock_pl25a1_serial".to_string(),
                    path: "/dev/bus/usb/001/004".to_string(),
                    bridge_status: "connected".to_string(),
                },
                capabilities: vec!["usb_bridge".to_string(), "host_to_host".to_string()],
            },
            TransportType::Echo => DeviceInfo {
                device_id: "echo:127.0.0.1:5038".to_string(),
                display_name: "Mock Echo Service".to_string(),
                transport_type: TransportType::Echo,
                connection_info: ConnectionInfo::Echo {
                    host: "127.0.0.1".to_string(),
                    port: 5038,
                },
                capabilities: vec!["echo".to_string(), "aging_test".to_string()],
            },
        };

        Ok(vec![device])
    }

    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
        let transport: Box<dyn Transport> = match &device_info.connection_info {
            ConnectionInfo::Tcp { host, port } => {
                Box::new(MockTransport::new_tcp(device_info.device_id.clone(), host.clone(), *port))
            }
            ConnectionInfo::UsbDevice { vid, pid, serial, path } => {
                Box::new(MockTransport::new_usb_device(
                    device_info.device_id.clone(),
                    *vid,
                    *pid,
                    serial.clone(),
                    path.clone(),
                ))
            }
            ConnectionInfo::UsbBridge { vid, pid, serial, path, bridge_status } => {
                Box::new(MockTransport::new_usb_bridge(
                    device_info.device_id.clone(),
                    *vid,
                    *pid,
                    serial.clone(),
                    path.clone(),
                    bridge_status.clone(),
                ))
            }
            ConnectionInfo::Echo { host, port } => {
                Box::new(MockTransport::new_tcp(device_info.device_id.clone(), host.clone(), *port))
            }
        };

        Ok(transport)
    }

    fn transport_type(&self) -> TransportType {
        self.transport_type
    }

    fn factory_name(&self) -> String {
        format!("Mock {} Transport Factory", self.transport_type)
    }

    fn is_available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_tcp_transport() {
        let mut transport = MockTransport::new_tcp(
            "tcp:127.0.0.1:5555".to_string(),
            "127.0.0.1".to_string(),
            5555,
        );

        assert!(!transport.is_connected());
        assert_eq!(transport.transport_type(), TransportType::Tcp);
        assert_eq!(transport.device_id(), "tcp:127.0.0.1:5555");

        transport.connect().await.unwrap();
        assert!(transport.is_connected());

        transport.send(b"test").await.unwrap();

        let mut buffer = vec![0u8; 64];
        let len = transport.receive(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..len], b"mock response");

        transport.disconnect().await.unwrap();
        assert!(!transport.is_connected());
    }

    #[tokio::test]
    async fn test_mock_factory() {
        let factory = MockTransportFactory::new(TransportType::Tcp);
        assert_eq!(factory.transport_type(), TransportType::Tcp);
        assert!(factory.is_available());

        let devices = factory.discover_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].transport_type, TransportType::Tcp);

        let transport = factory.create_transport(&devices[0]).await.unwrap();
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }
}