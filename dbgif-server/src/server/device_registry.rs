use dbgif_protocol::error::{ProtocolError, ProtocolResult};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// Device registry for managing connected devices and their metadata
#[derive(Debug, Clone)]
pub struct DeviceRegistry {
    /// Internal storage for devices
    devices: Arc<RwLock<HashMap<String, DeviceInfo>>>,
    /// Notification system for device state changes
    state_notifier: Arc<Notify>,
    /// Registry configuration
    config: DeviceRegistryConfig,
}

/// Information about a registered device
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Unique device identifier
    pub device_id: String,
    /// Device display name
    pub name: String,
    /// Device connection address
    pub address: SocketAddr,
    /// Device connection state
    pub state: DeviceState,
    /// Device capabilities and features
    pub capabilities: DeviceCapabilities,
    /// Device metadata and properties
    pub metadata: DeviceMetadata,
    /// Last seen timestamp
    pub last_seen: Instant,
    /// Connection statistics
    pub stats: DeviceStats,
}

/// Device connection state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    /// Device is discovered but not connected
    Discovered,
    /// Device is currently connecting
    Connecting,
    /// Device is connected and ready
    Connected,
    /// Device is disconnecting
    Disconnecting,
    /// Device is offline/disconnected
    Offline,
    /// Device has encountered an error
    Error { message: String },
}

/// Device capabilities and supported features
#[derive(Debug, Clone, Default)]
pub struct DeviceCapabilities {
    /// Maximum data size supported
    pub max_data_size: usize,
    /// Supported protocol version
    pub protocol_version: u32,
    /// Device-specific features
    pub features: Vec<String>,
    /// Whether device supports shell commands
    pub shell_support: bool,
    /// Whether device supports file transfer
    pub file_transfer_support: bool,
    /// Whether device supports port forwarding
    pub port_forwarding_support: bool,
}

/// Device metadata and properties
#[derive(Debug, Clone, Default)]
pub struct DeviceMetadata {
    /// Device model/product name
    pub model: Option<String>,
    /// Device manufacturer
    pub manufacturer: Option<String>,
    /// Device serial number
    pub serial_number: Option<String>,
    /// Operating system version
    pub os_version: Option<String>,
    /// Additional custom properties
    pub properties: HashMap<String, String>,
}

/// Device connection statistics
#[derive(Debug, Clone, Default)]
pub struct DeviceStats {
    /// Number of successful connections
    pub connection_count: u64,
    /// Number of failed connection attempts
    pub failed_connections: u64,
    /// Total bytes sent to device
    pub bytes_sent: u64,
    /// Total bytes received from device
    pub bytes_received: u64,
    /// Last connection timestamp
    pub last_connected: Option<Instant>,
    /// Total uptime duration
    pub total_uptime: Duration,
}

/// Configuration for device registry
#[derive(Debug, Clone)]
pub struct DeviceRegistryConfig {
    /// Maximum number of devices to track
    pub max_devices: usize,
    /// Device discovery timeout
    pub discovery_timeout: Duration,
    /// Device connection timeout
    pub connection_timeout: Duration,
    /// How long to keep offline devices in registry
    pub offline_retention: Duration,
    /// Enable automatic device cleanup
    pub auto_cleanup: bool,
}

impl Default for DeviceRegistryConfig {
    fn default() -> Self {
        Self {
            max_devices: 100,
            discovery_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(10),
            offline_retention: Duration::from_secs(300), // 5 minutes
            auto_cleanup: true,
        }
    }
}

impl DeviceRegistry {
    /// Create a new device registry with default configuration
    pub fn new() -> Self {
        Self::with_config(DeviceRegistryConfig::default())
    }

    /// Create a new device registry with custom configuration
    pub fn with_config(config: DeviceRegistryConfig) -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            state_notifier: Arc::new(Notify::new()),
            config,
        }
    }

    /// Register a new device
    pub fn register_device(&self, device_info: DeviceInfo) -> ProtocolResult<()> {
        let mut devices = self.devices.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on device registry".to_string(),
            }
        })?;

        // Check device limit
        if devices.len() >= self.config.max_devices {
            return Err(ProtocolError::ResourceExhausted {
                resource: format!("Device registry (max: {})", self.config.max_devices),
            });
        }

        // Check for duplicate device ID
        if devices.contains_key(&device_info.device_id) {
            return Err(ProtocolError::InternalError {
                message: format!("Device {} already registered", device_info.device_id),
            });
        }

        devices.insert(device_info.device_id.clone(), device_info);

        // Notify listeners of state change
        self.state_notifier.notify_waiters();

        Ok(())
    }

    /// Unregister a device
    pub fn unregister_device(&self, device_id: &str) -> ProtocolResult<Option<DeviceInfo>> {
        let mut devices = self.devices.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on device registry".to_string(),
            }
        })?;

        let removed = devices.remove(device_id);

        if removed.is_some() {
            // Notify listeners of state change
            self.state_notifier.notify_waiters();
        }

        Ok(removed)
    }

    /// Get device information by ID
    pub fn get_device(&self, device_id: &str) -> ProtocolResult<Option<DeviceInfo>> {
        let devices = self.devices.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on device registry".to_string(),
            }
        })?;

        Ok(devices.get(device_id).cloned())
    }

    /// Update device state
    pub fn update_device_state(&self, device_id: &str, new_state: DeviceState) -> ProtocolResult<()> {
        let mut devices = self.devices.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on device registry".to_string(),
            }
        })?;

        if let Some(device) = devices.get_mut(device_id) {
            device.state = new_state;
            device.last_seen = Instant::now();

            // Notify listeners of state change
            self.state_notifier.notify_waiters();

            Ok(())
        } else {
            Err(ProtocolError::DeviceNotFound {
                device_id: device_id.to_string(),
            })
        }
    }

    /// List all registered devices
    pub fn list_devices(&self) -> ProtocolResult<Vec<DeviceInfo>> {
        let devices = self.devices.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on device registry".to_string(),
            }
        })?;

        Ok(devices.values().cloned().collect())
    }

    /// List devices by state
    pub fn list_devices_by_state(&self, state: &DeviceState) -> ProtocolResult<Vec<DeviceInfo>> {
        let devices = self.devices.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on device registry".to_string(),
            }
        })?;

        Ok(devices
            .values()
            .filter(|device| &device.state == state)
            .cloned()
            .collect())
    }

    /// Get registry statistics
    pub fn get_stats(&self) -> ProtocolResult<RegistryStats> {
        let devices = self.devices.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on device registry".to_string(),
            }
        })?;

        let mut stats = RegistryStats::default();
        stats.total_devices = devices.len();

        for device in devices.values() {
            match device.state {
                DeviceState::Connected => stats.connected_devices += 1,
                DeviceState::Connecting => stats.connecting_devices += 1,
                DeviceState::Offline => stats.offline_devices += 1,
                DeviceState::Error { .. } => stats.error_devices += 1,
                _ => {}
            }
        }

        Ok(stats)
    }

    /// Wait for device state changes
    pub async fn wait_for_changes(&self) {
        self.state_notifier.notified().await;
    }

    /// Clean up offline devices based on retention policy
    pub fn cleanup_offline_devices(&self) -> ProtocolResult<usize> {
        if !self.config.auto_cleanup {
            return Ok(0);
        }

        let mut devices = self.devices.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on device registry".to_string(),
            }
        })?;

        let cutoff_time = Instant::now() - self.config.offline_retention;
        let mut removed_count = 0;

        devices.retain(|_, device| {
            let should_retain = match device.state {
                DeviceState::Offline => device.last_seen > cutoff_time,
                _ => true, // Keep all non-offline devices
            };

            if !should_retain {
                removed_count += 1;
            }

            should_retain
        });

        if removed_count > 0 {
            // Notify listeners of state change
            self.state_notifier.notify_waiters();
        }

        Ok(removed_count)
    }

    /// Update device statistics
    pub fn update_device_stats(&self, device_id: &str, bytes_sent: u64, bytes_received: u64) -> ProtocolResult<()> {
        let mut devices = self.devices.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on device registry".to_string(),
            }
        })?;

        if let Some(device) = devices.get_mut(device_id) {
            device.stats.bytes_sent += bytes_sent;
            device.stats.bytes_received += bytes_received;
            device.last_seen = Instant::now();
            Ok(())
        } else {
            Err(ProtocolError::DeviceNotFound {
                device_id: device_id.to_string(),
            })
        }
    }

    /// Get device count
    pub fn device_count(&self) -> ProtocolResult<usize> {
        let devices = self.devices.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on device registry".to_string(),
            }
        })?;

        Ok(devices.len())
    }

    /// Check if device exists
    pub fn contains_device(&self, device_id: &str) -> ProtocolResult<bool> {
        let devices = self.devices.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on device registry".to_string(),
            }
        })?;

        Ok(devices.contains_key(device_id))
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry statistics
#[derive(Debug, Clone, Default)]
pub struct RegistryStats {
    /// Total number of registered devices
    pub total_devices: usize,
    /// Number of connected devices
    pub connected_devices: usize,
    /// Number of connecting devices
    pub connecting_devices: usize,
    /// Number of offline devices
    pub offline_devices: usize,
    /// Number of devices in error state
    pub error_devices: usize,
}

impl DeviceInfo {
    /// Create new device info
    pub fn new(device_id: impl Into<String>, name: impl Into<String>, address: SocketAddr) -> Self {
        Self {
            device_id: device_id.into(),
            name: name.into(),
            address,
            state: DeviceState::Discovered,
            capabilities: DeviceCapabilities::default(),
            metadata: DeviceMetadata::default(),
            last_seen: Instant::now(),
            stats: DeviceStats::default(),
        }
    }

    /// Update last seen timestamp
    pub fn update_last_seen(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Check if device is online (connected or connecting)
    pub fn is_online(&self) -> bool {
        matches!(self.state, DeviceState::Connected | DeviceState::Connecting)
    }

    /// Check if device is available for connections
    pub fn is_available(&self) -> bool {
        matches!(self.state, DeviceState::Connected)
    }
}

impl DeviceStats {
    /// Record a successful connection
    pub fn record_connection(&mut self) {
        self.connection_count += 1;
        self.last_connected = Some(Instant::now());
    }

    /// Record a failed connection
    pub fn record_failed_connection(&mut self) {
        self.failed_connections += 1;
    }

    /// Get connection success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.connection_count + self.failed_connections;
        if total == 0 {
            0.0
        } else {
            self.connection_count as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_device(id: &str) -> DeviceInfo {
        DeviceInfo::new(
            id,
            format!("Test Device {}", id),
            "127.0.0.1:5555".parse().unwrap(),
        )
    }

    #[test]
    fn test_device_registry_creation() {
        let registry = DeviceRegistry::new();
        assert_eq!(registry.device_count().unwrap(), 0);
    }

    #[test]
    fn test_device_registration() {
        let registry = DeviceRegistry::new();
        let device = create_test_device("test1");

        assert!(registry.register_device(device.clone()).is_ok());
        assert_eq!(registry.device_count().unwrap(), 1);
        assert!(registry.contains_device("test1").unwrap());
    }

    #[test]
    fn test_device_unregistration() {
        let registry = DeviceRegistry::new();
        let device = create_test_device("test1");

        registry.register_device(device.clone()).unwrap();
        let removed = registry.unregister_device("test1").unwrap();

        assert!(removed.is_some());
        assert_eq!(registry.device_count().unwrap(), 0);
        assert!(!registry.contains_device("test1").unwrap());
    }

    #[test]
    fn test_device_state_update() {
        let registry = DeviceRegistry::new();
        let device = create_test_device("test1");

        registry.register_device(device).unwrap();
        registry.update_device_state("test1", DeviceState::Connected).unwrap();

        let device_info = registry.get_device("test1").unwrap().unwrap();
        assert_eq!(device_info.state, DeviceState::Connected);
    }

    #[test]
    fn test_device_listing() {
        let registry = DeviceRegistry::new();

        registry.register_device(create_test_device("test1")).unwrap();
        registry.register_device(create_test_device("test2")).unwrap();

        registry.update_device_state("test1", DeviceState::Connected).unwrap();

        let all_devices = registry.list_devices().unwrap();
        assert_eq!(all_devices.len(), 2);

        let connected_devices = registry.list_devices_by_state(&DeviceState::Connected).unwrap();
        assert_eq!(connected_devices.len(), 1);
        assert_eq!(connected_devices[0].device_id, "test1");
    }
}