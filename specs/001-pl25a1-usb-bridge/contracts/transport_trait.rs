// Generalized transport trait contract for all communication methods
// This defines the unified interface for TCP, USB Device, and USB Bridge transports

use async_trait::async_trait;
use anyhow::Result;
use bytes::Bytes;
use std::time::Duration;

/// Transport abstraction for different communication methods
/// (TCP, USB Device, USB Bridge, etc.)
#[async_trait]
pub trait Transport: Send + Sync {
    /// Establish connection to the transport medium
    /// Returns Ok(()) on successful connection, Err on failure
    async fn connect(&mut self) -> Result<()>;

    /// Send data through the transport
    /// Must handle partial writes and ensure all data is sent
    async fn send(&mut self, data: &[u8]) -> Result<()>;

    /// Receive data from the transport
    /// Returns number of bytes read into buffer
    /// May return 0 if no data available (non-blocking)
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<usize>;

    /// Gracefully disconnect from transport
    async fn disconnect(&mut self) -> Result<()>;

    /// Check if transport is currently connected
    fn is_connected(&self) -> bool;

    /// Get transport type identifier
    fn transport_type(&self) -> TransportType;

    /// Get unique device identifier for this transport
    fn device_id(&self) -> String;

    /// Get human-readable display name
    fn display_name(&self) -> String;

    /// Get maximum transfer unit size for this transport
    fn max_transfer_size(&self) -> usize;

    /// Check if transport supports bidirectional communication
    fn is_bidirectional(&self) -> bool { true }

    /// Get transport-specific connection info
    fn connection_info(&self) -> ConnectionInfo;
}

/// Transport type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    Tcp,
    UsbDevice,
    UsbBridge,
}

/// Transport-specific connection information
#[derive(Debug, Clone)]
pub enum ConnectionInfo {
    Tcp { host: String, port: u16 },
    UsbDevice { vid: u16, pid: u16, serial: String, path: String },
    UsbBridge { vid: u16, pid: u16, path: String, bridge_type: String },
}

/// Transport factory for device discovery and transport creation
#[async_trait]
pub trait TransportFactory: Send + Sync {
    /// Discover available devices for this transport type
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>>;

    /// Create transport instance for specific device
    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn Transport>>;

    /// Get supported transport type
    fn transport_type(&self) -> TransportType;

    /// Get factory name for debugging
    fn factory_name(&self) -> &'static str;

    /// Check if factory is available on current platform
    fn is_available(&self) -> bool;
}

/// Device information for transport creation
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub device_id: String,
    pub display_name: String,
    pub transport_type: TransportType,
    pub connection_info: ConnectionInfo,
    pub capabilities: Vec<String>,
}

/// Transport manager for coordinating multiple transport types
pub struct TransportManager {
    factories: Vec<Box<dyn TransportFactory>>,
    active_transports: std::collections::HashMap<String, Box<dyn Transport>>,
}

impl TransportManager {
    pub fn new() -> Self {
        Self {
            factories: Vec::new(),
            active_transports: std::collections::HashMap::new(),
        }
    }

    /// Register a transport factory
    pub fn register_factory(&mut self, factory: Box<dyn TransportFactory>) {
        if factory.is_available() {
            self.factories.push(factory);
        }
    }

    /// Discover all available devices across all transports
    pub async fn discover_all_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut all_devices = Vec::new();

        for factory in &self.factories {
            match factory.discover_devices().await {
                Ok(mut devices) => all_devices.append(&mut devices),
                Err(e) => {
                    // Log error but continue with other factories
                    eprintln!("Discovery failed for {}: {}", factory.factory_name(), e);
                }
            }
        }

        Ok(all_devices)
    }

    /// Create and connect to a specific device
    pub async fn connect_to_device(&mut self, device_info: &DeviceInfo) -> Result<String> {
        // Find appropriate factory
        let factory = self.factories
            .iter()
            .find(|f| f.transport_type() == device_info.transport_type)
            .ok_or_else(|| anyhow::anyhow!("No factory for transport type {:?}", device_info.transport_type))?;

        // Create transport
        let mut transport = factory.create_transport(device_info).await?;

        // Connect
        transport.connect().await?;

        let device_id = transport.device_id();
        self.active_transports.insert(device_id.clone(), transport);

        Ok(device_id)
    }

    /// Get active transport by device ID
    pub fn get_transport(&mut self, device_id: &str) -> Option<&mut Box<dyn Transport>> {
        self.active_transports.get_mut(device_id)
    }

    /// Disconnect and remove transport
    pub async fn disconnect_device(&mut self, device_id: &str) -> Result<()> {
        if let Some(mut transport) = self.active_transports.remove(device_id) {
            transport.disconnect().await?;
        }
        Ok(())
    }
}

// Contract tests would verify:
// 1. Transport trait implementation for each type (TCP, USB Device, USB Bridge)
// 2. Factory pattern discovers devices correctly
// 3. Transport manager coordinates multiple transports
// 4. Device connection/disconnection lifecycle
// 5. Error handling across transport boundaries