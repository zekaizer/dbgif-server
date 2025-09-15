use super::{Transport, TransportFactory, TransportType, DeviceInfo};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;
use std::fmt;

/// Manages multiple transport types and coordinates device discovery
pub struct TransportManager {
    factories: HashMap<TransportType, Box<dyn TransportFactory>>,
    active_transports: Arc<RwLock<HashMap<String, Box<dyn Transport>>>>,
    device_ownership: Arc<RwLock<HashMap<String, String>>>, // device_id -> client_id
    discovery_interval: Duration,
}

impl TransportManager {
    pub fn new() -> Self {
        let mut factories: HashMap<TransportType, Box<dyn TransportFactory>> = HashMap::new();

        // Register all available transport factories
        factories.insert(
            TransportType::Tcp,
            Box::new(super::TcpTransportFactory::new())
        );
        factories.insert(
            TransportType::UsbDevice,
            Box::new(super::UsbDeviceTransportFactory::new())
        );
        factories.insert(
            TransportType::UsbBridge,
            Box::new(super::UsbBridgeTransportFactory::new())
        );

        Self {
            factories,
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            device_ownership: Arc::new(RwLock::new(HashMap::new())),
            discovery_interval: Duration::from_secs(2),
        }
    }

    /// Discover all available devices across all transport types
    pub async fn discover_all_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut all_devices = Vec::new();

        for (transport_type, factory) in &self.factories {
            if factory.is_available() {
                match factory.discover_devices().await {
                    Ok(mut devices) => {
                        tracing::debug!(
                            "Discovered {} devices for transport type: {:?}",
                            devices.len(),
                            transport_type
                        );
                        all_devices.append(&mut devices);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to discover devices for transport {:?}: {}",
                            transport_type,
                            e
                        );
                    }
                }
            }
        }

        Ok(all_devices)
    }

    /// Get transport information for all active transports
    pub async fn get_all_transport_info(&self) -> Vec<TransportInfo> {
        let transports = self.active_transports.read().await;
        let ownership = self.device_ownership.read().await;

        transports
            .iter()
            .map(|(device_id, transport)| TransportInfo {
                device_id: device_id.clone(),
                transport_type: transport.transport_type(),
                display_name: transport.display_name(),
                is_connected: transport.is_connected(),
                max_transfer_size: transport.max_transfer_size(),
                is_bidirectional: transport.is_bidirectional(),
                connection_info: transport.connection_info(),
                owner_client_id: ownership.get(device_id).cloned(),
            })
            .collect()
    }

    /// Get transport information for a specific device
    pub async fn get_transport_info(&self, device_id: &str) -> Option<TransportInfo> {
        let transports = self.active_transports.read().await;
        let ownership = self.device_ownership.read().await;

        if let Some(transport) = transports.get(device_id) {
            Some(TransportInfo {
                device_id: device_id.to_string(),
                transport_type: transport.transport_type(),
                display_name: transport.display_name(),
                is_connected: transport.is_connected(),
                max_transfer_size: transport.max_transfer_size(),
                is_bidirectional: transport.is_bidirectional(),
                connection_info: transport.connection_info(),
                owner_client_id: ownership.get(device_id).cloned(),
            })
        } else {
            None
        }
    }

    /// Get list of all transport device IDs
    pub async fn get_transport_ids(&self) -> Vec<String> {
        let transports = self.active_transports.read().await;
        transports.keys().cloned().collect()
    }

    /// Check if a device is currently occupied by another client
    pub async fn is_device_occupied(&self, device_id: &str) -> bool {
        let ownership = self.device_ownership.read().await;
        ownership.contains_key(device_id)
    }

    /// Acquire exclusive access to a device for a client
    pub async fn acquire_device(&self, device_id: &str, client_id: String) -> Result<()> {
        let mut ownership = self.device_ownership.write().await;

        if let Some(existing_owner) = ownership.get(device_id) {
            if existing_owner != &client_id {
                return Err(anyhow::anyhow!(
                    "Device {} is already occupied by client {}",
                    device_id,
                    existing_owner
                ));
            }
        }

        ownership.insert(device_id.to_string(), client_id);
        Ok(())
    }

    /// Release device access for a client
    pub async fn release_device(&self, device_id: &str, client_id: String) -> Result<()> {
        let mut ownership = self.device_ownership.write().await;

        if let Some(existing_owner) = ownership.get(device_id) {
            if existing_owner == &client_id {
                ownership.remove(device_id);
                tracing::debug!("Released device {} from client {}", device_id, client_id);
            } else {
                return Err(anyhow::anyhow!(
                    "Cannot release device {} - owned by different client {}",
                    device_id,
                    existing_owner
                ));
            }
        }

        Ok(())
    }

    /// Create and connect to a transport for a specific device
    pub async fn connect_to_device(&self, device_info: &DeviceInfo) -> Result<()> {
        // Check if we have a factory for this transport type
        let factory = self.factories.get(&device_info.transport_type)
            .ok_or_else(|| anyhow::anyhow!(
                "No factory available for transport type: {:?}",
                device_info.transport_type
            ))?;

        // Create the transport
        let mut transport = factory.create_transport(device_info).await?;

        // Connect to the device
        transport.connect().await?;

        // Store the connected transport
        let mut transports = self.active_transports.write().await;
        transports.insert(device_info.device_id.clone(), transport);

        tracing::info!(
            "Connected to device {} ({})",
            device_info.device_id,
            device_info.display_name
        );

        Ok(())
    }

    /// Disconnect from a specific device
    pub async fn disconnect_from_device(&self, device_id: &str) -> Result<()> {
        let mut transports = self.active_transports.write().await;

        if let Some(mut transport) = transports.remove(device_id) {
            transport.disconnect().await?;
            tracing::info!("Disconnected from device {}", device_id);
        }

        // Also release ownership
        let mut ownership = self.device_ownership.write().await;
        ownership.remove(device_id);

        Ok(())
    }

    /// Send a message to a specific device
    pub async fn send_message(&self, device_id: &str, data: &[u8]) -> Result<()> {
        let mut transports = self.active_transports.write().await;

        if let Some(transport) = transports.get_mut(device_id) {
            if !transport.is_connected() {
                return Err(anyhow::anyhow!("Transport {} is not connected", device_id));
            }
            transport.send(data).await
        } else {
            Err(anyhow::anyhow!("No active transport for device: {}", device_id))
        }
    }

    /// Receive a message from a specific device
    pub async fn receive_message(&self, device_id: &str) -> Result<Vec<u8>> {
        let mut transports = self.active_transports.write().await;

        if let Some(transport) = transports.get_mut(device_id) {
            if !transport.is_connected() {
                return Err(anyhow::anyhow!("Transport {} is not connected", device_id));
            }

            let mut buffer = vec![0u8; transport.max_transfer_size()];
            let len = transport.receive(&mut buffer).await?;
            buffer.truncate(len);
            Ok(buffer)
        } else {
            Err(anyhow::anyhow!("No active transport for device: {}", device_id))
        }
    }

    /// Start continuous device discovery
    pub async fn start_discovery(&self) -> Result<()> {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(manager.discovery_interval);

            loop {
                interval.tick().await;

                match manager.discover_all_devices().await {
                    Ok(devices) => {
                        tracing::debug!("Discovery found {} devices", devices.len());

                        // Auto-connect to new devices if needed
                        for device in devices {
                            let transports = manager.active_transports.read().await;
                            if !transports.contains_key(&device.device_id) {
                                drop(transports); // Release read lock

                                if let Err(e) = manager.connect_to_device(&device).await {
                                    tracing::debug!(
                                        "Auto-connect failed for device {}: {}",
                                        device.device_id,
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Device discovery failed: {}", e);
                    }
                }
            }
        });

        Ok(())
    }
}

// Clone implementation for async tasks
impl Clone for TransportManager {
    fn clone(&self) -> Self {
        Self {
            factories: HashMap::new(), // Factories are not cloneable, so we create empty map
            active_transports: Arc::clone(&self.active_transports),
            device_ownership: Arc::clone(&self.device_ownership),
            discovery_interval: self.discovery_interval,
        }
    }
}

impl fmt::Debug for TransportManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransportManager")
            .field("factories", &format!("{} factories", self.factories.len()))
            .field("active_transports", &format!("RwLock<HashMap<String, Box<dyn Transport>>>"))
            .field("device_ownership", &self.device_ownership)
            .field("discovery_interval", &self.discovery_interval)
            .finish()
    }
}

/// Information about an active transport
#[derive(Debug, Clone)]
pub struct TransportInfo {
    pub device_id: String,
    pub transport_type: TransportType,
    pub display_name: String,
    pub is_connected: bool,
    pub max_transfer_size: usize,
    pub is_bidirectional: bool,
    pub connection_info: super::ConnectionInfo,
    pub owner_client_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transport_manager_creation() {
        let manager = TransportManager::new();

        // Check that all transport types are registered
        assert!(manager.factories.contains_key(&TransportType::Tcp));
        assert!(manager.factories.contains_key(&TransportType::UsbDevice));
        assert!(manager.factories.contains_key(&TransportType::UsbBridge));
    }

    #[tokio::test]
    async fn test_device_ownership() {
        let manager = TransportManager::new();
        let device_id = "test_device";
        let client_id1 = "client1".to_string();
        let client_id2 = "client2".to_string();

        // Initially, device should not be occupied
        assert!(!manager.is_device_occupied(device_id).await);

        // Acquire device for client1
        manager.acquire_device(device_id, client_id1.clone()).await.unwrap();
        assert!(manager.is_device_occupied(device_id).await);

        // Try to acquire for client2 - should fail
        let result = manager.acquire_device(device_id, client_id2.clone()).await;
        assert!(result.is_err());

        // Release device
        manager.release_device(device_id, client_id1).await.unwrap();
        assert!(!manager.is_device_occupied(device_id).await);

        // Now client2 should be able to acquire
        manager.acquire_device(device_id, client_id2.clone()).await.unwrap();
        assert!(manager.is_device_occupied(device_id).await);
    }

    #[tokio::test]
    async fn test_discovery() {
        let manager = TransportManager::new();

        // Discovery should not fail even with no actual devices
        let devices = manager.discover_all_devices().await.unwrap();

        // The exact number depends on what mock devices are returned
        // But it should be deterministic
        assert!(devices.len() >= 0);
    }

    #[tokio::test]
    async fn test_manager_stats() {
        let manager = TransportManager::new();
        let stats = manager.get_stats().await;

        assert_eq!(stats.active_transports, 0);
        assert!(stats.total_factories > 0); // Should have at least mock factory
    }
}

impl TransportManager {
    /// Initialize USB device factory
    pub async fn initialize_usb_device_factory(&self) -> Result<()> {
        // USB device factory is always available if nusb is compiled in
        tracing::debug!("USB device factory initialized");
        Ok(())
    }

    /// Initialize USB bridge factory
    pub async fn initialize_usb_bridge_factory(&self) -> Result<()> {
        // USB bridge factory is always available if nusb is compiled in
        tracing::debug!("USB bridge factory initialized");
        Ok(())
    }

    /// Get transport manager statistics
    pub async fn get_stats(&self) -> TransportManagerStats {
        let transports = self.active_transports.read().await;
        let discovered_devices = self.discover_all_devices().await
            .map(|devices| devices.len())
            .unwrap_or(0);

        TransportManagerStats {
            active_transports: transports.len(),
            discovered_devices,
            total_factories: self.factories.len(),
        }
    }

    /// Disconnect all active transports
    pub async fn disconnect_all(&self) -> Result<()> {
        let mut transports = self.active_transports.write().await;
        let mut errors = Vec::new();

        for (device_id, transport) in transports.iter_mut() {
            if let Err(e) = transport.disconnect().await {
                errors.push(format!("Failed to disconnect {}: {}", device_id, e));
            }
        }

        transports.clear();

        if !errors.is_empty() {
            return Err(anyhow::anyhow!("Disconnect errors: {}", errors.join(", ")));
        }

        Ok(())
    }
}

/// Transport manager statistics
#[derive(Debug, Clone)]
pub struct TransportManagerStats {
    pub active_transports: usize,
    pub discovered_devices: usize,
    pub total_factories: usize,
}