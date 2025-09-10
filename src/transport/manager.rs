use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use super::debug::DebugTransport;
use super::{ConnectionStatus, LoopbackTransport, Transport, TransportType};
use crate::protocol::message::Message;

/// Device ownership information
#[derive(Debug, Clone)]
struct DeviceOccupancy {
    client_id: u32,
    acquired_at: std::time::Instant,
}

/// Unified manager for all transport types
pub struct TransportManager {
    transports: Arc<RwLock<HashMap<String, Box<dyn Transport + Send>>>>,
    monitors: RwLock<HashMap<String, watch::Receiver<ConnectionStatus>>>,
    monitor_tasks: RwLock<HashMap<String, JoinHandle<()>>>,
    /// Track which client owns which device (exclusive access)
    device_occupancy: Arc<RwLock<HashMap<String, DeviceOccupancy>>>,
}

impl TransportManager {
    /// Create new transport manager
    pub fn new() -> Self {
        Self {
            transports: Arc::new(RwLock::new(HashMap::new())),
            monitors: RwLock::new(HashMap::new()),
            monitor_tasks: RwLock::new(HashMap::new()),
            device_occupancy: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a new transport
    pub async fn add_transport(&self, mut transport: Box<dyn Transport + Send>) -> Result<String> {
        let device_id = transport.device_id().to_string();
        let transport_type = transport.transport_type();

        // Initialize connection and initial connection status
        let status = transport.connect().await?;

        // Add transport to collection
        self.transports
            .write()
            .await
            .insert(device_id.clone(), transport);

        // Start continuous polling for transports that need logical connection
        // Rule: Connected status = requires polling, Ready status = immediately usable
        if status == ConnectionStatus::Connected {
            info!(
                "Added {} transport: {} (starting continuous status polling)",
                transport_type, device_id
            );
            self.start_status_polling(&device_id).await?;
        } else {
            info!(
                "Added {} transport: {} (status: {}, no polling needed)",
                transport_type, device_id, status
            );
        }

        Ok(device_id)
    }

    /// Add a transport with optional debug wrapping
    pub async fn add_transport_with_debug(
        &self,
        transport: Box<dyn Transport + Send>,
        enable_debug: bool,
    ) -> Result<String> {
        if enable_debug {
            let debug_transport = DebugTransport::with_debug(transport, true);
            self.add_transport(Box::new(debug_transport)).await
        } else {
            self.add_transport(transport).await
        }
    }

    /// Add transport with debug automatically enabled based on environment
    #[inline]
    pub async fn add_transport_auto_debug(
        &self,
        transport: Box<dyn Transport + Send>,
    ) -> Result<String> {
        #[cfg(feature = "transport-debug")]
        {
            let debug_enabled = crate::transport::debug::is_debug_env_enabled();
            self.add_transport_with_debug(transport, debug_enabled)
                .await
        }

        #[cfg(not(feature = "transport-debug"))]
        {
            // When debug feature is disabled, just add transport directly
            self.add_transport(transport).await
        }
    }

    /// Remove a transport
    pub async fn remove_transport(&self, device_id: &str) -> Result<()> {
        // Stop monitoring first
        if let Some(task) = self.monitor_tasks.write().await.remove(device_id) {
            task.abort();
        }
        self.monitors.write().await.remove(device_id);

        // Remove and disconnect transport
        if let Some(mut transport) = self.transports.write().await.remove(device_id) {
            let _ = transport.disconnect().await;
            info!("Removed transport: {}", device_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Transport not found: {}", device_id))
        }
    }

    /// Send message to specific transport
    pub async fn send_message(&self, device_id: &str, message: &Message) -> Result<()> {
        let serialized_data = message.serialize();
        let mut transports = self.transports.write().await;

        match transports.get_mut(device_id) {
            Some(transport) => transport.send(&serialized_data).await.map_err(|e| {
                error!("Failed to send message to {}: {}", device_id, e);
                e
            }),
            None => Err(anyhow::anyhow!("Transport not found: {}", device_id)),
        }
    }

    /// Receive message from specific transport
    pub async fn receive_message(&self, device_id: &str) -> Result<Message> {
        let mut transports = self.transports.write().await;

        match transports.get_mut(device_id) {
            Some(transport) => {
                let raw_data = transport.receive(4096).await.map_err(|e| {
                    error!("Failed to receive data from {}: {}", device_id, e);
                    e
                })?;
                
                Message::deserialize(raw_data.as_slice()).map_err(|e| {
                    error!("Failed to deserialize message from {}: {}", device_id, e);
                    e
                })
            },
            None => Err(anyhow::anyhow!("Transport not found: {}", device_id)),
        }
    }

    /// Get list of all transport IDs
    pub async fn get_transport_ids(&self) -> Vec<String> {
        self.transports.read().await.keys().cloned().collect()
    }

    /// Get transport information
    pub async fn get_transport_info(&self, device_id: &str) -> Option<(TransportType, bool)> {
        let transports = self.transports.read().await;

        transports.get(device_id).map(|transport| {
            (
                transport.transport_type(),
                futures::executor::block_on(transport.is_connected()),
            )
        })
    }

    /// Get all transport information
    pub async fn get_all_transport_info(&self) -> HashMap<String, (TransportType, bool)> {
        let transports = self.transports.read().await;
        let mut info = HashMap::new();

        for (id, transport) in transports.iter() {
            let connected = futures::executor::block_on(transport.is_connected());
            info.insert(id.clone(), (transport.transport_type(), connected));
        }

        info
    }

    /// Get connection status for monitored transport
    pub async fn get_connection_status(&self, device_id: &str) -> Option<ConnectionStatus> {
        let monitors = self.monitors.read().await;

        monitors.get(device_id).map(|rx| rx.borrow().clone())
    }

    /// Check health of all transports
    pub async fn health_check(&self) -> HashMap<String, Result<(), String>> {
        let transports = self.transports.read().await;
        let mut results = HashMap::new();

        for (id, transport) in transports.iter() {
            let result = match transport.health_check().await {
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            };
            results.insert(id.clone(), result);
        }

        results
    }

    /// Start status polling for Connected -> Ready transition
    async fn start_status_polling(&self, device_id: &str) -> Result<()> {
        let device_id_clone = device_id.to_string();
        let transports = self.transports.clone();

        let task = tokio::spawn(async move {
            let mut last_status = ConnectionStatus::Connected;

            info!("Starting status polling for transport {}", device_id_clone);

            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                let transports_read = transports.read().await;
                if let Some(transport) = transports_read.get(&device_id_clone) {
                    // Use transport's get_connection_status for more accurate status
                    let status = transport.get_connection_status().await;

                    // Check for status changes
                    if status != last_status {
                        match (&last_status, &status) {
                            (ConnectionStatus::Connected, ConnectionStatus::Ready) => {
                                info!("Transport {} is now ready", device_id_clone);
                            }
                            (ConnectionStatus::Ready, ConnectionStatus::Connected) => {
                                info!(
                                    "Transport {} remote disconnected, continuing polling",
                                    device_id_clone
                                );
                            }
                            (_, ConnectionStatus::Disconnected) => {
                                warn!(
                                    "Transport {} disconnected - stopping polling",
                                    device_id_clone
                                );
                                break;
                            }
                            (_, ConnectionStatus::Error(ref err)) => {
                                error!(
                                    "Transport {} error: {} - stopping polling",
                                    device_id_clone, err
                                );
                                break;
                            }
                            _ => {
                                debug!(
                                    "Transport {} status changed: {} -> {}",
                                    device_id_clone, last_status, status
                                );
                            }
                        }
                        last_status = status;
                    }
                } else {
                    debug!("Transport {} removed - stopping polling", device_id_clone);
                    break;
                }
            }

            debug!("Status polling stopped for transport {}", device_id_clone);
        });

        self.monitor_tasks
            .write()
            .await
            .insert(device_id.to_string(), task);

        Ok(())
    }

    /// Handle USB device disconnect (called by UsbMonitor)
    pub async fn handle_usb_disconnect(&self, device_id: &str) -> Result<()> {
        info!("Handling USB disconnect for device: {}", device_id);
        
        // Remove the transport (this also calls disconnect on the transport)
        self.remove_transport(device_id).await?;
        
        // Additional cleanup could go here:
        // - Notify connected clients about device loss
        // - Clean up any device-specific resources
        // - Update device status in any registries
        
        info!("USB device {} disconnect handling complete", device_id);
        Ok(())
    }

    /// Shutdown all transports
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down transport manager");

        // Stop all monitoring tasks
        let mut monitor_tasks = self.monitor_tasks.write().await;
        for (_, task) in monitor_tasks.drain() {
            task.abort();
        }

        // Disconnect all transports
        let mut transports = self.transports.write().await;
        let mut errors = Vec::new();

        for (device_id, mut transport) in transports.drain() {
            if let Err(e) = transport.disconnect().await {
                errors.push(format!("Failed to disconnect {}: {}", device_id, e));
            }
        }

        self.monitors.write().await.clear();

        if errors.is_empty() {
            info!("Transport manager shutdown complete");
            Ok(())
        } else {
            Err(anyhow::anyhow!("Shutdown errors: {}", errors.join(", ")))
        }
    }

    /// Add a virtual loopback transport for testing
    pub async fn add_loopback_transport(&self, device_id: Option<String>) -> Result<String> {
        let id = device_id.unwrap_or_else(|| "loopback_virtual".to_string());
        let transport = LoopbackTransport::new(id.clone());
        
        info!("Adding loopback transport: {}", id);
        
        self.add_transport_auto_debug(Box::new(transport)).await
    }

    /// Add a loopback transport with custom latency
    pub async fn add_loopback_transport_with_latency(
        &self, 
        device_id: Option<String>,
        latency: std::time::Duration,
    ) -> Result<String> {
        let id = device_id.unwrap_or_else(|| "loopback_virtual".to_string());
        let transport = LoopbackTransport::with_latency(id.clone(), latency);
        
        info!("Adding loopback transport with {}ms latency: {}", latency.as_millis(), id);
        
        self.add_transport_auto_debug(Box::new(transport)).await
    }

    /// Add a loopback transport with network simulation
    pub async fn add_loopback_transport_with_simulation(
        &self,
        device_id: Option<String>, 
        latency: std::time::Duration,
        packet_loss_rate: f32,
    ) -> Result<String> {
        let id = device_id.unwrap_or_else(|| "loopback_virtual".to_string());
        let transport = LoopbackTransport::with_simulation(id.clone(), latency, packet_loss_rate);
        
        info!(
            "Adding loopback transport with {}ms latency and {:.1}% packet loss: {}", 
            latency.as_millis(), 
            packet_loss_rate * 100.0,
            id
        );
        
        self.add_transport_auto_debug(Box::new(transport)).await
    }

    /// Add a pair of loopback transports for bidirectional testing
    pub async fn add_loopback_pair(&self, device_a_id: Option<String>, device_b_id: Option<String>) -> Result<(String, String)> {
        let id_a = device_a_id.unwrap_or_else(|| "loopback_a".to_string());
        let id_b = device_b_id.unwrap_or_else(|| "loopback_b".to_string());
        
        let (transport_a, transport_b) = LoopbackTransport::create_pair(id_a.clone(), id_b.clone());
        
        info!("Adding loopback transport pair: {} <-> {}", id_a, id_b);
        
        // Add both transports
        let actual_id_a = self.add_transport_auto_debug(Box::new(transport_a)).await?;
        let actual_id_b = self.add_transport_auto_debug(Box::new(transport_b)).await?;
        
        info!("Loopback transport pair added successfully: {} <-> {}", actual_id_a, actual_id_b);
        Ok((actual_id_a, actual_id_b))
    }

    /// Add a pair of loopback transports with custom latency
    pub async fn add_loopback_pair_with_latency(
        &self,
        device_a_id: Option<String>,
        device_b_id: Option<String>,
        latency: std::time::Duration,
    ) -> Result<(String, String)> {
        let id_a = device_a_id.unwrap_or_else(|| "loopback_a".to_string());
        let id_b = device_b_id.unwrap_or_else(|| "loopback_b".to_string());
        
        let (transport_a, transport_b) = LoopbackTransport::create_pair_with_latency(
            id_a.clone(),
            id_b.clone(),
            latency,
        );
        
        info!(
            "Adding loopback transport pair with {}ms latency: {} <-> {}", 
            latency.as_millis(),
            id_a, 
            id_b
        );
        
        // Add both transports
        let actual_id_a = self.add_transport_auto_debug(Box::new(transport_a)).await?;
        let actual_id_b = self.add_transport_auto_debug(Box::new(transport_b)).await?;
        
        info!("Loopback transport pair added successfully: {} <-> {}", actual_id_a, actual_id_b);
        Ok((actual_id_a, actual_id_b))
    }

    /// Device Ownership Management

    /// Check if a device is currently occupied by any client
    pub async fn is_device_occupied(&self, device_id: &str) -> bool {
        let occupancy = self.device_occupancy.read().await;
        occupancy.contains_key(device_id)
    }

    /// Acquire exclusive access to a device for a client
    pub async fn acquire_device(&self, device_id: &str, client_id: u32) -> Result<()> {
        let mut occupancy = self.device_occupancy.write().await;

        // Check if device is already occupied
        if let Some(existing) = occupancy.get(device_id) {
            if existing.client_id != client_id {
                return Err(anyhow::anyhow!(
                    "Device {} already occupied by client {}",
                    device_id,
                    existing.client_id
                ));
            }
            // Same client re-acquiring is OK
            return Ok(());
        }

        // Acquire the device
        let ownership = DeviceOccupancy {
            client_id,
            acquired_at: std::time::Instant::now(),
        };
        occupancy.insert(device_id.to_string(), ownership);

        info!("Client {} acquired exclusive access to device {}", client_id, device_id);
        Ok(())
    }

    /// Release device ownership for a client
    pub async fn release_device(&self, device_id: &str, client_id: u32) -> Result<()> {
        let mut occupancy = self.device_occupancy.write().await;

        if let Some(existing) = occupancy.get(device_id) {
            if existing.client_id != client_id {
                return Err(anyhow::anyhow!(
                    "Cannot release device {}: owned by client {}, not {}",
                    device_id,
                    existing.client_id,
                    client_id
                ));
            }
            occupancy.remove(device_id);
            info!("Client {} released device {}", client_id, device_id);
        }
        
        Ok(())
    }

    /// Get current device ownership status
    pub async fn get_device_owner(&self, device_id: &str) -> Option<u32> {
        let occupancy = self.device_occupancy.read().await;
        occupancy.get(device_id).map(|o| o.client_id)
    }

    /// Force release all devices owned by a specific client (cleanup on client disconnect)
    pub async fn release_all_devices_for_client(&self, client_id: u32) -> Result<()> {
        let mut occupancy = self.device_occupancy.write().await;
        let devices_to_release: Vec<String> = occupancy
            .iter()
            .filter(|(_, ownership)| ownership.client_id == client_id)
            .map(|(device_id, _)| device_id.clone())
            .collect();

        for device_id in devices_to_release {
            occupancy.remove(&device_id);
            info!("Force released device {} from client {}", device_id, client_id);
        }

        Ok(())
    }
}

impl Default for TransportManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transport_manager_creation() {
        let manager = TransportManager::new();
        let ids = manager.get_transport_ids().await;
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn test_transport_manager_info() {
        let manager = TransportManager::new();
        let info = manager.get_all_transport_info().await;
        assert!(info.is_empty());
    }

    #[tokio::test]
    async fn test_health_check_empty() {
        let manager = TransportManager::new();
        let health = manager.health_check().await;
        assert!(health.is_empty());
    }
}
