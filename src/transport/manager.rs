use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use super::debug::DebugTransport;
use super::{ConnectionStatus, Transport, TransportType};
use crate::protocol::message::Message;

/// Unified manager for all transport types
pub struct TransportManager {
    transports: Arc<RwLock<HashMap<String, Box<dyn Transport + Send>>>>,
    monitors: RwLock<HashMap<String, watch::Receiver<ConnectionStatus>>>,
    monitor_tasks: RwLock<HashMap<String, JoinHandle<()>>>,
}

impl TransportManager {
    /// Create new transport manager
    pub fn new() -> Self {
        Self {
            transports: Arc::new(RwLock::new(HashMap::new())),
            monitors: RwLock::new(HashMap::new()),
            monitor_tasks: RwLock::new(HashMap::new()),
        }
    }

    /// Add a new transport
    pub async fn add_transport(&self, mut transport: Box<dyn Transport + Send>) -> Result<String> {
        let device_id = transport.device_id().to_string();
        let transport_type = transport.transport_type();

        // Initialize connection
        transport.connect().await?;

        // Check current connection status to determine if polling is needed
        let status = transport.get_connection_status().await;

        // Add transport to collection
        self.transports
            .write()
            .await
            .insert(device_id.clone(), transport);

        // Start polling for Connected -> Ready transition
        if status == ConnectionStatus::Connected {
            info!(
                "Added {} transport: {} (starting status polling)",
                transport_type, device_id
            );
            self.start_status_polling(&device_id).await?;
        } else {
            info!(
                "Added {} transport: {} (status: {})",
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
        let mut transports = self.transports.write().await;

        match transports.get_mut(device_id) {
            Some(transport) => transport.send_message(message).await.map_err(|e| {
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
            Some(transport) => transport.receive_message().await.map_err(|e| {
                error!("Failed to receive message from {}: {}", device_id, e);
                e
            }),
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
