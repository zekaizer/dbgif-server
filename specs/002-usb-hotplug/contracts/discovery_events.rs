// Contract: Discovery Event Integration
// This contract defines how hotplug events integrate with the existing discovery system

use anyhow::Result;
use std::collections::HashSet;

/// Contract for integrating hotplug events with existing DiscoveryCoordinator
/// Must maintain backward compatibility with existing client interfaces
pub trait DiscoveryEventIntegration {
    /// Convert hotplug event to existing DiscoveryEvent format
    /// Must preserve all information needed by existing clients
    fn hotplug_to_discovery_event(&self, event: HotplugEventType, device_info: Option<DeviceInfo>) -> DiscoveryEvent;

    /// Send discovery event to existing event channels
    /// Must use the same mpsc channels as current polling implementation
    async fn emit_discovery_event(&self, event: DiscoveryEvent) -> Result<()>;

    /// Get list of known devices in existing format
    /// Must return same format as current get_known_devices() method
    async fn get_known_devices(&self) -> HashSet<String>;

    /// Perform compatibility check with existing transport manager
    /// Must verify that TransportManager can handle hotplug-generated events
    fn verify_transport_compatibility(&self) -> Result<bool>;
}

/// Contract for maintaining client API compatibility during migration
/// Ensures existing client code continues to work unchanged
pub trait ClientApiCompatibility {
    /// Handle host command "host:devices" with hotplug-detected devices
    /// Must return same format as polling-based implementation
    async fn handle_host_devices_command(&self) -> Result<String>;

    /// Handle host command "host:track-devices" with real-time updates
    /// Must provide immediate updates when hotplug events occur
    async fn handle_track_devices_command(&self) -> Result<()>;

    /// Maintain existing device connection APIs
    /// Existing connect/disconnect flows must work unchanged
    async fn connect_to_device(&self, device_id: &str) -> Result<()>;
}

// Event types for compatibility
#[derive(Debug, Clone)]
pub enum HotplugEventType {
    Connected(DeviceInfo),
    Disconnected(DeviceId),
}

#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    DeviceConnected(crate::transport::DeviceInfo),
    DeviceDisconnected(String),
    DiscoveryError(String),
}

// Compatibility requirements
/// All existing DiscoveryEvent consumers must continue working
pub const REQUIRED_DISCOVERY_EVENT_COMPATIBILITY: bool = true;

/// TransportManager interface must remain unchanged
pub const REQUIRED_TRANSPORT_MANAGER_COMPATIBILITY: bool = true;

/// Host service commands must return identical formats
pub const REQUIRED_HOST_SERVICE_COMPATIBILITY: bool = true;