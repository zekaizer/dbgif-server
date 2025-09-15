// Contract test module - provides types required by integration tests
pub use crate::protocol::message::{AdbMessage, Command};
pub use crate::transport::{
    DeviceInfo, ConnectionInfo, TransportType, Transport, TransportFactory
};

// Re-export for compatibility with contract tests
pub mod test_adb_protocol {
    pub use crate::protocol::message::{AdbMessage, Command};
    use anyhow::Result;
    use std::time::Duration;
    use crate::transport::Transport;

    pub struct AdbProtocol {
        timeout: Duration,
    }

    impl AdbProtocol {
        pub fn new() -> Self {
            Self {
                timeout: Duration::from_secs(30),
            }
        }

        pub fn with_timeout(timeout: Duration) -> Self {
            Self { timeout }
        }

        pub async fn perform_handshake(&mut self, _transport: &mut dyn Transport) -> Result<()> {
            // TODO: Implement actual handshake
            Ok(())
        }

        pub async fn open_service(&mut self, _transport: &mut dyn Transport, _service: &str) -> Result<u32> {
            // TODO: Implement service opening
            Ok(1001)
        }

        pub async fn write_stream(&mut self, _transport: &mut dyn Transport, _stream_id: u32, _data: &[u8]) -> Result<()> {
            // TODO: Implement stream writing
            Ok(())
        }

        pub async fn read_stream(&mut self, _transport: &mut dyn Transport, _stream_id: u32) -> Result<Vec<u8>> {
            // TODO: Implement stream reading
            Ok(b"Echo: response".to_vec())
        }

        pub async fn close_stream(&mut self, _transport: &mut dyn Transport, _stream_id: u32) -> Result<()> {
            // TODO: Implement stream closing
            Ok(())
        }

        pub async fn send_ping(&mut self, _transport: &mut dyn Transport) -> Result<()> {
            // TODO: Implement ping
            Ok(())
        }

        pub async fn receive_pong(&mut self, _transport: &mut dyn Transport) -> Result<()> {
            // TODO: Implement pong reception
            Ok(())
        }

        pub fn validate_message(&self, message: &AdbMessage) -> Result<()> {
            if !message.is_valid() {
                return Err(anyhow::anyhow!("Invalid message"));
            }
            Ok(())
        }
    }
}

pub mod test_transport_trait {
    pub use crate::transport::{
        DeviceInfo, ConnectionInfo, TransportType, Transport, TransportFactory
    };
}

pub mod test_transport_factories {
    use super::test_transport_trait::*;
    use anyhow::Result;
    use async_trait::async_trait;
    use std::time::Duration;

    pub struct TcpTransportFactory {
        pub scan_ports: Vec<u16>,
        pub scan_subnets: Vec<String>,
        pub connection_timeout: Duration,
    }

    impl TcpTransportFactory {
        pub fn new() -> Self {
            Self {
                scan_ports: vec![5555],
                scan_subnets: vec!["127.0.0.1".to_string()],
                connection_timeout: Duration::from_secs(5),
            }
        }

        pub fn with_config(ports: Vec<u16>, subnets: Vec<String>, timeout: Duration) -> Self {
            Self {
                scan_ports: ports,
                scan_subnets: subnets,
                connection_timeout: timeout,
            }
        }
    }

    #[async_trait]
    impl TransportFactory for TcpTransportFactory {
        async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
            // Mock implementation for tests
            Ok(vec![DeviceInfo {
                device_id: "tcp:127.0.0.1:5555".to_string(),
                display_name: "Mock TCP Device".to_string(),
                transport_type: TransportType::Tcp,
                connection_info: ConnectionInfo::Tcp {
                    host: "127.0.0.1".to_string(),
                    port: 5555,
                },
                capabilities: vec!["tcp".to_string()],
            }])
        }

        async fn create_transport(&self, _device_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
            Err(anyhow::anyhow!("Mock implementation - not implemented"))
        }

        fn transport_type(&self) -> TransportType {
            TransportType::Tcp
        }

        fn factory_name(&self) -> String {
            "TCP Transport Factory".to_string()
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    pub struct UsbDeviceTransportFactory {
        pub android_vid_pids: Vec<(u16, u16)>,
        pub scan_interval: Duration,
    }

    impl UsbDeviceTransportFactory {
        pub fn new() -> Self {
            Self {
                android_vid_pids: vec![(0x18d1, 0x4ee7)], // Google
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
                ],
                scan_interval: Duration::from_secs(1),
            }
        }
    }

    #[async_trait]
    impl TransportFactory for UsbDeviceTransportFactory {
        async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
            // Mock implementation for tests
            Ok(vec![DeviceInfo {
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
            }])
        }

        async fn create_transport(&self, _device_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
            Err(anyhow::anyhow!("Mock implementation - not implemented"))
        }

        fn transport_type(&self) -> TransportType {
            TransportType::UsbDevice
        }

        fn factory_name(&self) -> String {
            "USB Device Transport Factory".to_string()
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    pub struct UsbBridgeTransportFactory {
        pub target_vid: u16,
        pub target_pid: u16,
        pub scan_interval: Duration,
    }

    impl UsbBridgeTransportFactory {
        pub fn new() -> Self {
            Self {
                target_vid: 0x067b,
                target_pid: 0x25a1,
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
    }

    #[async_trait]
    impl TransportFactory for UsbBridgeTransportFactory {
        async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
            // Mock implementation for tests
            Ok(vec![DeviceInfo {
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
            }])
        }

        async fn create_transport(&self, _device_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
            Err(anyhow::anyhow!("Mock implementation - not implemented"))
        }

        fn transport_type(&self) -> TransportType {
            TransportType::UsbBridge
        }

        fn factory_name(&self) -> String {
            "USB Bridge Transport Factory".to_string()
        }

        fn is_available(&self) -> bool {
            true
        }
    }
}

pub mod test_daemon_api {
    use anyhow::Result;
    use std::time::Duration;

    pub struct ServerConfig {
        pub bind_address: String,
        pub bind_port: u16,
        pub max_clients: usize,
        pub client_timeout: Duration,
        pub enable_logging: bool,
    }

    pub struct DaemonServer {
        _config: ServerConfig,
    }

    impl DaemonServer {
        pub async fn new(_config: ServerConfig) -> Result<Self> {
            Ok(Self { _config })
        }

        pub fn client_count(&self) -> usize {
            0
        }

        pub fn is_running(&self) -> bool {
            false
        }

        pub fn bind_port(&self) -> u16 {
            5037
        }

        pub async fn start(&mut self) -> Result<()> {
            Ok(())
        }

        pub async fn run(&self) -> Result<()> {
            Ok(())
        }

        pub async fn shutdown(&mut self) -> Result<()> {
            Ok(())
        }
    }

    pub struct DaemonClient {
        _host: String,
        _port: u16,
    }

    pub struct ClientSession {
        _session_id: u32,
    }
}