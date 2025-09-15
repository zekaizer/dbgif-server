use anyhow::Result;
use async_trait::async_trait;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportType {
    Tcp,
    UsbDevice,
    UsbBridge,
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportType::Tcp => write!(f, "TCP"),
            TransportType::UsbDevice => write!(f, "USB Device"),
            TransportType::UsbBridge => write!(f, "USB Bridge"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConnectionInfo {
    Tcp { host: String, port: u16 },
    UsbDevice { vid: u16, pid: u16, serial: String, path: String },
    UsbBridge { vid: u16, pid: u16, serial: String, path: String, bridge_status: String },
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub device_id: String,
    pub display_name: String,
    pub transport_type: TransportType,
    pub connection_info: ConnectionInfo,
    pub capabilities: Vec<String>,
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn connect(&mut self) -> Result<()>;
    async fn send(&mut self, data: &[u8]) -> Result<()>;
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<usize>;
    async fn disconnect(&mut self) -> Result<()>;
    fn is_connected(&self) -> bool;
    fn transport_type(&self) -> TransportType;
    fn device_id(&self) -> String;
    fn display_name(&self) -> String;
    fn max_transfer_size(&self) -> usize;
    fn is_bidirectional(&self) -> bool { true }
    fn connection_info(&self) -> ConnectionInfo;
}

#[async_trait]
pub trait TransportFactory: Send + Sync {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>>;
    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn Transport>>;
    fn transport_type(&self) -> TransportType;
    fn factory_name(&self) -> String;
    fn is_available(&self) -> bool;
}

pub mod mock_transport;
pub mod tcp_transport;
pub mod usb_device_transport;
pub mod usb_bridge_transport;

pub use mock_transport::*;
pub use tcp_transport::*;
pub use usb_device_transport::*;
pub use usb_bridge_transport::*;

pub mod manager;

pub use manager::*;