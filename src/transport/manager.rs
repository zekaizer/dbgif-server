use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn, debug, error};
use anyhow::Result;

use crate::protocol::message::Message;
use super::{Transport, MonitorableTransport, TransportType, ConnectionStatus};

/// Unified manager for all transport types
pub struct TransportManager {
    transports: RwLock<HashMap<String, Box<dyn Transport + Send>>>,
    monitors: RwLock<HashMap<String, watch::Receiver<ConnectionStatus>>>,
    monitor_tasks: RwLock<HashMap<String, JoinHandle<()>>>,
}

impl TransportManager {
    /// Create new transport manager
    pub fn new() -> Self {
        Self {
            transports: RwLock::new(HashMap::new()),
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
        
        // Add transport to collection
        self.transports.write().await.insert(device_id.clone(), transport);
        
        info!("Added {} transport: {}", transport_type, device_id);
        Ok(device_id)
    }

    /// Add a monitorable transport with status monitoring
    pub async fn add_monitorable_transport(&self, mut transport: Box<dyn MonitorableTransport + Send>) -> Result<String> {
        let device_id = transport.device_id().to_string();
        let transport_type = transport.transport_type();
        
        // Initialize connection
        transport.connect().await?;
        
        // Start monitoring
        match transport.start_monitoring().await {
            Ok(rx) => {
                self.monitors.write().await.insert(device_id.clone(), rx);
                self.start_status_monitor(&device_id).await;
                info!("Started monitoring for {} transport: {}", transport_type, device_id);
            }
            Err(e) => {
                warn!("Failed to start monitoring for {}: {}", device_id, e);
            }
        }
        
        // Add transport to collection (upcast to Transport trait)
        let transport_boxed: Box<dyn Transport + Send> = transport;
        self.transports.write().await.insert(device_id.clone(), transport_boxed);
        
        info!("Added monitored {} transport: {}", transport_type, device_id);
        Ok(device_id)
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
            Some(transport) => {
                transport.send_message(message).await.map_err(|e| {
                    error!("Failed to send message to {}: {}", device_id, e);
                    e
                })
            }
            None => Err(anyhow::anyhow!("Transport not found: {}", device_id)),
        }
    }

    /// Receive message from specific transport
    pub async fn receive_message(&self, device_id: &str) -> Result<Message> {
        let mut transports = self.transports.write().await;
        
        match transports.get_mut(device_id) {
            Some(transport) => {
                transport.receive_message().await.map_err(|e| {
                    error!("Failed to receive message from {}: {}", device_id, e);
                    e
                })
            }
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
            (transport.transport_type(), futures::executor::block_on(transport.is_connected()))
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

    /// Start monitoring connection status changes
    async fn start_status_monitor(&self, device_id: &str) {
        let monitors = self.monitors.read().await;
        let device_id_clone = device_id.to_string();
        
        if let Some(mut rx) = monitors.get(device_id).cloned() {
            let device_id_for_task = device_id_clone.clone();
            
            let task = tokio::spawn(async move {
                while rx.changed().await.is_ok() {
                    let status = rx.borrow().clone();
                    debug!("Connection status changed for {}: {}", device_id_for_task, status);
                    
                    match status {
                        ConnectionStatus::Disconnected => {
                            warn!("Transport {} disconnected", device_id_for_task);
                        }
                        ConnectionStatus::Error(ref err) => {
                            error!("Transport {} error: {}", device_id_for_task, err);
                        }
                        ConnectionStatus::Connected => {
                            info!("Transport {} connected", device_id_for_task);
                        }
                        ConnectionStatus::Suspended => {
                            warn!("Transport {} suspended", device_id_for_task);
                        }
                        _ => {}
                    }
                }
                
                debug!("Status monitor stopped for transport {}", device_id_for_task);
            });
            
            self.monitor_tasks.write().await.insert(device_id_clone, task);
        }
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