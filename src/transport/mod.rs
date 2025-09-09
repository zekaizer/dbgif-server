use anyhow::Result;
use async_trait::async_trait;
use std::fmt;

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
    Connected,
    Ready,
    Error(String),
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionStatus::Disconnected => write!(f, "Disconnected"),
            ConnectionStatus::Connected => write!(f, "Connected"),
            ConnectionStatus::Ready => write!(f, "Ready"),
            ConnectionStatus::Error(err) => write!(f, "Error: {}", err),
        }
    }
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_message(&mut self, message: &Message) -> Result<()>;
    async fn receive_message(&mut self) -> Result<Message>;

    async fn connect(&mut self) -> Result<ConnectionStatus>;
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

    /// Get current connection status - default implementation
    async fn get_connection_status(&self) -> ConnectionStatus {
        if self.is_connected().await {
            ConnectionStatus::Ready
        } else {
            ConnectionStatus::Disconnected
        }
    }
}

pub mod android_usb;
pub mod bridge_usb;
pub mod debug;
pub mod manager;
pub mod tcp;
pub mod usb_common;
pub mod usb_monitor;

pub use android_usb::*;
pub use bridge_usb::*;
pub use debug::{is_debug_env_enabled, DebugTransport};
pub use manager::*;
pub use tcp::*;
pub use usb_common::*;
pub use usb_monitor::*;
