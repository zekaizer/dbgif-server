use std::fmt;
use async_trait::async_trait;
use tokio::sync::watch;
use anyhow::Result;

use crate::protocol::message::Message;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransportType {
    Tcp,
    AndroidUsb,
    BridgeUsb,
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportType::Tcp => write!(f, "TCP"),
            TransportType::AndroidUsb => write!(f, "Android USB"),
            TransportType::BridgeUsb => write!(f, "Bridge USB"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Suspended,
    Error(String),
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionStatus::Disconnected => write!(f, "Disconnected"),
            ConnectionStatus::Connecting => write!(f, "Connecting"),
            ConnectionStatus::Connected => write!(f, "Connected"),
            ConnectionStatus::Suspended => write!(f, "Suspended"),
            ConnectionStatus::Error(err) => write!(f, "Error: {}", err),
        }
    }
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_message(&mut self, message: &Message) -> Result<()>;
    async fn receive_message(&mut self) -> Result<Message>;
    
    async fn connect(&mut self) -> Result<()>;
    async fn disconnect(&mut self) -> Result<()>;
    async fn is_connected(&self) -> bool;
    
    fn device_id(&self) -> &str;
    fn transport_type(&self) -> TransportType;
    
    async fn health_check(&self) -> Result<()> {
        if self.is_connected().await {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Transport is not connected"))
        }
    }
}

#[async_trait]
pub trait MonitorableTransport: Transport {
    async fn start_monitoring(&mut self) -> Result<watch::Receiver<ConnectionStatus>>;
    async fn stop_monitoring(&mut self) -> Result<()>;
    async fn get_connection_status(&self) -> ConnectionStatus;
    
    fn supports_hotplug(&self) -> bool {
        false
    }
    
    fn supports_reconnection(&self) -> bool {
        true
    }
}

pub mod tcp;
pub mod android_usb;
pub mod bridge_usb;
pub mod manager;

pub use tcp::*;
pub use android_usb::*;
pub use bridge_usb::*;
pub use manager::*;