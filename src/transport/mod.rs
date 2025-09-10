use anyhow::Result;
use async_trait::async_trait;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransportType {
    Tcp,
    AndroidUsb,
    BridgeUsb,
    Loopback,
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportType::Tcp => write!(f, "TCP"),
            TransportType::AndroidUsb => write!(f, "Android USB"),
            TransportType::BridgeUsb => write!(f, "Bridge USB"),
            TransportType::Loopback => write!(f, "Loopback"),
        }
    }
}

/// Connection status for transport layer
/// 
/// This enum defines the lifecycle states of transport connections:
/// - Physical connections (USB cables, TCP sockets) 
/// - Logical connections (protocol handshakes, remote readiness)
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    /// Transport is physically disconnected or unavailable
    /// - USB device unplugged
    /// - TCP connection closed
    /// - No communication possible
    Disconnected,
    
    /// Transport has physical connection but requires logical connection
    /// - USB device connected but remote side not ready
    /// - Bridge cable connected on one side only
    /// - Requires continuous polling until Ready
    Connected,
    
    /// Transport is fully ready for bi-directional communication
    /// - TCP connection established
    /// - USB device ready for ADB protocol
    /// - Bridge cable connected on both sides
    Ready,
    
    /// Transport encountered an error during connection attempt
    /// Contains error description for debugging
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
    async fn send(&mut self, data: &[u8]) -> Result<()>;
    async fn receive(&mut self, buffer_size: usize) -> Result<Vec<u8>>;

    /// Attempt to establish connection and return current status
    /// 
    /// Return values determine polling behavior:
    /// - `Ready`: Transport immediately usable, no polling needed
    /// - `Connected`: Transport needs logical connection, continuous polling started
    /// - `Disconnected`: Physical connection missing
    /// - `Error`: Connection attempt failed
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

    /// Get current real-time connection status
    /// 
    /// Used by TransportManager for continuous polling of Connected transports.
    /// Default implementation uses is_connected() for simple Ready/Disconnected check.
    /// Override for more sophisticated state detection (e.g., Bridge USB vendor commands).
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
pub mod loopback;
pub mod manager;
pub mod tcp;
pub mod usb_common;
pub mod usb_monitor;

pub use android_usb::*;
pub use bridge_usb::*;
pub use debug::{is_debug_env_enabled, DebugTransport};
pub use loopback::*;
pub use manager::*;
pub use tcp::*;
pub use usb_common::*;
pub use usb_monitor::*;
