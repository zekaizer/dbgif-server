// Contract: Hotplug Event Processing
// This contract defines the interface for processing USB hotplug events

use anyhow::Result;
use std::time::Instant;

/// Contract for hotplug event processing
/// Must be implemented by any component that handles USB device hotplug events
pub trait HotplugEventProcessor {
    /// Process a device connection event
    /// Must complete within 500ms to meet performance requirements
    async fn handle_device_connected(&mut self, device_info: DeviceInfo) -> Result<()>;

    /// Process a device disconnection event
    /// Must complete within 500ms and gracefully terminate active connections
    async fn handle_device_disconnected(&mut self, device_id: DeviceId) -> Result<()>;

    /// Get current hotplug capability status
    /// Returns whether hotplug detection is available and working
    fn hotplug_available(&self) -> bool;

    /// Get resource usage metrics
    /// Must return current CPU and memory usage for monitoring
    fn get_resource_metrics(&self) -> ResourceUsageMetrics;
}

/// Contract for detection mechanism management
/// Handles switching between hotplug and polling modes
pub trait DetectionMechanismManager {
    /// Activate hotplug detection
    /// Must fall back to polling if hotplug activation fails
    async fn activate_hotplug(&mut self) -> Result<DetectionMode>;

    /// Deactivate hotplug detection
    /// Must not lose any currently connected devices
    async fn deactivate_hotplug(&mut self) -> Result<DetectionMode>;

    /// Get current detection mode
    fn current_mode(&self) -> DetectionMode;

    /// Force fallback to polling mode
    /// Emergency fallback when hotplug fails
    async fn force_polling_fallback(&mut self) -> Result<()>;
}

/// Contract for migration coordination
/// Handles the transition from polling to hotplug without losing devices
pub trait MigrationCoordinator {
    /// Start migration from polling to hotplug
    /// Must capture current device state before switching
    async fn start_migration(&mut self) -> Result<MigrationPhase>;

    /// Verify migration completed successfully
    /// Must ensure all devices from before migration are still detected
    async fn verify_migration(&mut self) -> Result<bool>;

    /// Rollback migration if issues detected
    /// Must restore original polling mechanism
    async fn rollback_migration(&mut self, reason: String) -> Result<MigrationPhase>;

    /// Get current migration status
    fn migration_status(&self) -> MigrationState;
}

// Data types referenced in contracts
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub device_id: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
    pub device_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId(pub u64);

#[derive(Debug, Clone)]
pub struct ResourceUsageMetrics {
    pub cpu_usage_percent: f64,
    pub memory_bytes: u64,
    pub hotplug_events_processed: u64,
    pub polling_cycles: u64,
    pub measurement_period_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DetectionMode {
    HotplugOnly,
    HotplugWithBackup,
    PollingOnly,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationPhase {
    NotStarted,
    CapturingInitialState,
    ActivatingHotplug,
    VerifyingDevices,
    DisablingPolling,
    Completed,
    RolledBack(String),
}

#[derive(Debug, Clone)]
pub struct MigrationState {
    pub phase: MigrationPhase,
    pub devices_before: std::collections::HashSet<String>,
    pub devices_after: std::collections::HashSet<String>,
    pub migration_started: Instant,
    pub devices_lost: Vec<String>,
}

// Performance requirements embedded in contract
/// All hotplug event processing must complete within 500ms
pub const MAX_EVENT_PROCESSING_TIME_MS: u64 = 500;

/// Idle CPU usage must be less than 1% when using hotplug detection
pub const MAX_IDLE_CPU_USAGE_PERCENT: f64 = 1.0;

/// Maximum acceptable device detection delay
pub const MAX_DETECTION_DELAY_MS: u64 = 500;