use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::discovery::DiscoveryEvent;
use crate::transport::DeviceInfo;
use super::hotplug::{
    HotplugEvent, HotplugEventType, DetectionMechanism, DetectionStats,
    HotplugDetector, NusbHotplugDetector,
};

/// Trait for processing hotplug events and integrating with discovery system
#[async_trait]
pub trait HotplugEventProcessor: Send + Sync {
    /// Process a single hotplug event
    async fn process_event(&self, event: HotplugEvent) -> Result<()>;

    /// Start monitoring for hotplug events
    async fn start_monitoring(&self) -> Result<mpsc::Receiver<HotplugEvent>>;

    /// Stop monitoring for hotplug events
    async fn stop_monitoring(&self) -> Result<()>;

    /// Get current detection statistics
    async fn stats(&self) -> DetectionStats;

    /// Check if monitoring is active
    fn is_monitoring(&self) -> bool;
}

/// Implementation of HotplugEventProcessor that integrates with the discovery system
pub struct UsbHotplugMonitor {
    detector: Arc<RwLock<Box<dyn HotplugDetector>>>,
    discovery_tx: Option<mpsc::Sender<DiscoveryEvent>>,
    running: bool,
}

impl UsbHotplugMonitor {
    /// Create new USB hotplug monitor
    pub fn new() -> Self {
        let detector: Box<dyn HotplugDetector> = Box::new(NusbHotplugDetector::new());

        Self {
            detector: Arc::new(RwLock::new(detector)),
            discovery_tx: None,
            running: false,
        }
    }

    /// Create monitor with fallback mechanism
    pub fn with_fallback() -> Self {
        let detector: Box<dyn HotplugDetector> = Box::new(NusbHotplugDetector::fallback());

        Self {
            detector: Arc::new(RwLock::new(detector)),
            discovery_tx: None,
            running: false,
        }
    }

    /// Set discovery event sender for integration
    pub fn set_discovery_sender(&mut self, discovery_tx: mpsc::Sender<DiscoveryEvent>) {
        self.discovery_tx = Some(discovery_tx);
    }

    /// Convert hotplug event to discovery event
    fn convert_to_discovery_event(&self, hotplug_event: &HotplugEvent) -> Result<DiscoveryEvent> {
        let device_info = DeviceInfo {
            device_id: hotplug_event.generate_serial(),
            display_name: format!("USB Device {:04x}:{:04x}", hotplug_event.vendor_id, hotplug_event.product_id),
            transport_type: hotplug_event.transport_type(),
            connection_info: crate::transport::ConnectionInfo::UsbDevice {
                vid: hotplug_event.vendor_id,
                pid: hotplug_event.product_id,
                serial: hotplug_event.generate_serial(),
                path: hotplug_event.device_path().unwrap_or_else(|| "unknown".to_string()),
            },
            capabilities: vec!["hotplug".to_string()],
        };

        let discovery_event = match hotplug_event.event_type {
            HotplugEventType::Connected => DiscoveryEvent::DeviceConnected(device_info),
            HotplugEventType::Disconnected => DiscoveryEvent::DeviceDisconnected(hotplug_event.device_id.clone()),
        };

        Ok(discovery_event)
    }
}

#[async_trait]
impl HotplugEventProcessor for UsbHotplugMonitor {
    async fn process_event(&self, event: HotplugEvent) -> Result<()> {
        debug!("Processing hotplug event: {}", event);

        // Convert hotplug event to discovery event
        let discovery_event = self.convert_to_discovery_event(&event)?;

        // Send to discovery system if configured
        if let Some(ref discovery_tx) = self.discovery_tx {
            if let Err(e) = discovery_tx.send(discovery_event).await {
                warn!("Failed to send discovery event: {}", e);
                return Err(anyhow::anyhow!("Failed to send discovery event: {}", e));
            }
        }

        info!("Successfully processed hotplug event: {}", event);
        Ok(())
    }

    async fn start_monitoring(&self) -> Result<mpsc::Receiver<HotplugEvent>> {
        info!("Starting USB hotplug monitoring");

        let mut detector = self.detector.write().await;
        let receiver = detector.start().await?;

        info!("USB hotplug monitoring started with mechanism: {}",
            detector.mechanism().name());

        Ok(receiver)
    }

    async fn stop_monitoring(&self) -> Result<()> {
        info!("Stopping USB hotplug monitoring");

        let mut detector = self.detector.write().await;
        detector.stop().await?;

        info!("USB hotplug monitoring stopped");
        Ok(())
    }

    async fn stats(&self) -> DetectionStats {
        let detector = self.detector.read().await;
        detector.stats().await
    }

    fn is_monitoring(&self) -> bool {
        self.running
    }
}

impl Default for UsbHotplugMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Factory for creating hotplug monitors
pub struct HotplugMonitorFactory;

impl HotplugMonitorFactory {
    /// Create a new hotplug monitor with automatic fallback
    pub async fn create_monitor() -> Result<UsbHotplugMonitor> {
        let monitor = UsbHotplugMonitor::new();

        // Test if nusb hotplug detection is available
        match monitor.start_monitoring().await {
            Ok(_) => {
                // Stop immediately - we were just testing
                let _ = monitor.stop_monitoring().await;
                info!("nusb hotplug detection available, using real-time monitoring");
                Ok(monitor)
            }
            Err(_) => {
                warn!("nusb hotplug detection failed, creating fallback monitor");
                Ok(UsbHotplugMonitor::with_fallback())
            }
        }
    }

    /// Create monitor with specific detection mechanism
    pub fn create_with_mechanism(mechanism: DetectionMechanism) -> UsbHotplugMonitor {
        match mechanism {
            DetectionMechanism::NusbWatcher => UsbHotplugMonitor::new(),
            DetectionMechanism::PollingFallback => UsbHotplugMonitor::with_fallback(),
            DetectionMechanism::Disabled => {
                // For disabled, we still create a monitor but it won't detect anything
                UsbHotplugMonitor::with_fallback()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::hotplug::HotplugEvent;

    #[tokio::test]
    async fn test_hotplug_monitor_creation() {
        let monitor = UsbHotplugMonitor::new();
        assert!(!monitor.is_monitoring());

        let stats = monitor.stats().await;
        assert_eq!(stats.total_events, 0);
    }

    #[tokio::test]
    async fn test_event_conversion() {
        let monitor = UsbHotplugMonitor::new();

        let hotplug_event = HotplugEvent::connected(
            "test-device".to_string(),
            0x18d1,  // Google
            0x4ee7,  // Nexus
        );

        let discovery_event = monitor.convert_to_discovery_event(&hotplug_event)
            .expect("Failed to convert event");

        match discovery_event {
            DiscoveryEvent::DeviceConnected(device_info) => {
                assert_eq!(device_info.device_id, "hotplug_test-device");
                assert_eq!(device_info.transport_type, crate::transport::TransportType::UsbDevice);
                assert!(device_info.display_name.contains("18d1:4ee7"));
            }
            _ => panic!("Expected DeviceConnected event"),
        }
    }

    #[tokio::test]
    async fn test_factory_creation() {
        let monitor = HotplugMonitorFactory::create_with_mechanism(DetectionMechanism::PollingFallback);
        assert!(!monitor.is_monitoring());

        let stats = monitor.stats().await;
        assert!(stats.mechanism.is_none() || matches!(stats.mechanism, Some(DetectionMechanism::PollingFallback)));
    }

    #[test]
    fn test_monitor_default() {
        let monitor = UsbHotplugMonitor::default();
        assert!(!monitor.is_monitoring());
    }
}