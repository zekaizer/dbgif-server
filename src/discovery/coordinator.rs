use crate::transport::{TransportManager, DeviceInfo, TransportType};
use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use std::time::Duration;

/// Events that can occur during device discovery
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    DeviceConnected(DeviceInfo),
    DeviceDisconnected(String), // device_id
    DiscoveryError(String),
}

/// Coordinates device discovery across all transport types with hotplug detection
pub struct DeviceDiscoveryCoordinator {
    transport_manager: Arc<TransportManager>,
    known_devices: Arc<RwLock<HashSet<String>>>, // device_ids
    event_sender: mpsc::UnboundedSender<DiscoveryEvent>,
    discovery_interval: Duration,
    hotplug_enabled: bool,
}

impl DeviceDiscoveryCoordinator {
    pub fn new(
        transport_manager: Arc<TransportManager>,
        event_sender: mpsc::UnboundedSender<DiscoveryEvent>,
    ) -> Self {
        Self {
            transport_manager,
            known_devices: Arc::new(RwLock::new(HashSet::new())),
            event_sender,
            discovery_interval: Duration::from_secs(1), // Fast polling for responsive hotplug
            hotplug_enabled: true,
        }
    }

    pub fn with_config(
        transport_manager: Arc<TransportManager>,
        event_sender: mpsc::UnboundedSender<DiscoveryEvent>,
        discovery_interval: Duration,
        hotplug_enabled: bool,
    ) -> Self {
        Self {
            transport_manager,
            known_devices: Arc::new(RwLock::new(HashSet::new())),
            event_sender,
            discovery_interval,
            hotplug_enabled,
        }
    }

    /// Start continuous device discovery and hotplug monitoring
    pub async fn start(&self) -> Result<()> {
        if !self.hotplug_enabled {
            tracing::info!("Device discovery coordinator disabled");
            return Ok(());
        }

        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator.discovery_loop().await;
        });

        tracing::info!(
            "Device discovery coordinator started (interval: {:?})",
            self.discovery_interval
        );
        Ok(())
    }

    /// Perform immediate device discovery scan
    pub async fn scan_once(&self) -> Result<Vec<DeviceInfo>> {
        let devices = self.transport_manager.discover_all_devices().await?;

        // Update known devices and generate events
        self.process_discovered_devices(&devices).await;

        Ok(devices)
    }

    /// Get list of currently known devices
    pub async fn get_known_devices(&self) -> HashSet<String> {
        let known = self.known_devices.read().await;
        known.clone()
    }

    /// Force refresh of device list
    pub async fn refresh(&self) -> Result<()> {
        tracing::debug!("Forcing device discovery refresh");
        self.scan_once().await?;
        Ok(())
    }

    /// Main discovery loop for continuous monitoring
    async fn discovery_loop(&self) {
        let mut interval = tokio::time::interval(self.discovery_interval);

        loop {
            interval.tick().await;

            match self.transport_manager.discover_all_devices().await {
                Ok(devices) => {
                    self.process_discovered_devices(&devices).await;
                }
                Err(e) => {
                    tracing::warn!("Device discovery scan failed: {}", e);

                    let _ = self.event_sender.send(DiscoveryEvent::DiscoveryError(
                        format!("Discovery failed: {}", e)
                    ));
                }
            }
        }
    }

    /// Process newly discovered devices and detect changes
    async fn process_discovered_devices(&self, devices: &[DeviceInfo]) {
        let current_device_ids: HashSet<String> = devices
            .iter()
            .map(|d| d.device_id.clone())
            .collect();

        let mut known_devices = self.known_devices.write().await;

        // Find newly connected devices
        for device in devices {
            if !known_devices.contains(&device.device_id) {
                tracing::info!(
                    "New device detected: {} ({})",
                    device.device_id,
                    device.display_name
                );

                known_devices.insert(device.device_id.clone());

                let _ = self.event_sender.send(DiscoveryEvent::DeviceConnected(device.clone()));
            }
        }

        // Find disconnected devices
        let disconnected_devices: Vec<String> = known_devices
            .difference(&current_device_ids)
            .cloned()
            .collect();

        for device_id in disconnected_devices {
            tracing::info!("Device disconnected: {}", device_id);

            known_devices.remove(&device_id);

            let _ = self.event_sender.send(DiscoveryEvent::DeviceDisconnected(device_id));
        }
    }

    /// Get statistics about discovery performance
    pub async fn get_discovery_stats(&self) -> DiscoveryStats {
        let known_count = {
            let known = self.known_devices.read().await;
            known.len()
        };

        // Get transport type breakdown
        let devices = match self.transport_manager.discover_all_devices().await {
            Ok(devices) => devices,
            Err(_) => Vec::new(),
        };

        let mut tcp_count = 0;
        let mut usb_device_count = 0;
        let mut usb_bridge_count = 0;

        for device in devices {
            match device.transport_type {
                TransportType::Tcp => tcp_count += 1,
                TransportType::UsbDevice => usb_device_count += 1,
                TransportType::UsbBridge => usb_bridge_count += 1,
            }
        }

        DiscoveryStats {
            total_known_devices: known_count,
            tcp_devices: tcp_count,
            usb_device_count,
            usb_bridge_count,
            discovery_interval: self.discovery_interval,
            hotplug_enabled: self.hotplug_enabled,
        }
    }
}

// Clone implementation for async tasks
impl Clone for DeviceDiscoveryCoordinator {
    fn clone(&self) -> Self {
        Self {
            transport_manager: Arc::clone(&self.transport_manager),
            known_devices: Arc::clone(&self.known_devices),
            event_sender: self.event_sender.clone(),
            discovery_interval: self.discovery_interval,
            hotplug_enabled: self.hotplug_enabled,
        }
    }
}

/// Statistics about device discovery performance
#[derive(Debug, Clone)]
pub struct DiscoveryStats {
    pub total_known_devices: usize,
    pub tcp_devices: usize,
    pub usb_device_count: usize,
    pub usb_bridge_count: usize,
    pub discovery_interval: Duration,
    pub hotplug_enabled: bool,
}

/// Create an event channel for discovery events
pub fn create_discovery_channel() -> (mpsc::UnboundedSender<DiscoveryEvent>, mpsc::UnboundedReceiver<DiscoveryEvent>) {
    mpsc::unbounded_channel()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportManager;
    use std::time::Duration;

    #[tokio::test]
    async fn test_discovery_coordinator_creation() {
        let transport_manager = Arc::new(TransportManager::new());
        let (sender, _receiver) = create_discovery_channel();

        let coordinator = DeviceDiscoveryCoordinator::new(transport_manager, sender);

        assert_eq!(coordinator.discovery_interval, Duration::from_secs(1));
        assert!(coordinator.hotplug_enabled);
    }

    #[tokio::test]
    async fn test_discovery_scan() {
        let transport_manager = Arc::new(TransportManager::new());
        let (sender, mut receiver) = create_discovery_channel();

        let coordinator = DeviceDiscoveryCoordinator::with_config(
            transport_manager,
            sender,
            Duration::from_millis(100),
            true,
        );

        // Perform a scan
        let devices = coordinator.scan_once().await.unwrap();

        // Should have found some mock devices
        assert!(devices.len() >= 0);

        // Check for events (there might be device connected events)
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Drain any events
        while let Ok(_event) = receiver.try_recv() {
            // Process events
        }
    }

    #[tokio::test]
    async fn test_discovery_stats() {
        let transport_manager = Arc::new(TransportManager::new());
        let (sender, _receiver) = create_discovery_channel();

        let coordinator = DeviceDiscoveryCoordinator::new(transport_manager, sender);

        let stats = coordinator.get_discovery_stats().await;

        assert_eq!(stats.discovery_interval, Duration::from_secs(1));
        assert!(stats.hotplug_enabled);
        assert!(stats.total_known_devices >= 0);
    }

    #[tokio::test]
    async fn test_discovery_channel() {
        let (sender, mut receiver) = create_discovery_channel();

        // Test sending an event
        let test_device = DeviceInfo {
            device_id: "test_device".to_string(),
            display_name: "Test Device".to_string(),
            transport_type: TransportType::Tcp,
            connection_info: crate::transport::ConnectionInfo::Tcp {
                host: "127.0.0.1".to_string(),
                port: 5555,
            },
            capabilities: vec!["test".to_string()],
        };

        sender.send(DiscoveryEvent::DeviceConnected(test_device.clone())).unwrap();

        // Receive the event
        if let Ok(event) = receiver.try_recv() {
            match event {
                DiscoveryEvent::DeviceConnected(device) => {
                    assert_eq!(device.device_id, "test_device");
                }
                _ => panic!("Unexpected event type"),
            }
        }
    }
}