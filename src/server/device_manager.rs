use crate::protocol::error::{ProtocolError, ProtocolResult};
use crate::server::device_registry::{DeviceRegistry, DeviceInfo, DeviceState, DeviceCapabilities, DeviceMetadata};
use crate::server::state::ServerState;
use crate::transport::{Connection, Transport, TransportAddress, TcpTransport};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, timeout};
use tracing::{debug, info, warn};

/// Device manager for discovering and managing device connections
/// Implements lazy connection strategy - devices are only connected when needed
pub struct DeviceManager {
    /// Server state reference
    server_state: Arc<ServerState>,
    /// Device registry for tracking known devices
    device_registry: Arc<DeviceRegistry>,
    /// Active device connections
    active_connections: Arc<RwLock<HashMap<String, DeviceConnection>>>,
    /// Device discovery configuration
    discovery_config: DiscoveryConfig,
    /// Device event channel
    event_sender: mpsc::UnboundedSender<DeviceEvent>,
    /// Device event receiver
    event_receiver: Arc<RwLock<Option<mpsc::UnboundedReceiver<DeviceEvent>>>>,
}

/// Active device connection information
struct DeviceConnection {
    /// Device information
    #[allow(dead_code)]
    device_info: DeviceInfo,
    /// Transport connection to device (wrapped in Arc<Mutex> for safe concurrent access)
    connection: Arc<Mutex<Box<dyn Connection>>>,
    /// Connection established timestamp
    connected_at: Instant,
    /// Last activity timestamp
    last_activity: Instant,
    /// Connection health status
    health_status: ConnectionHealth,
}

/// Connection health status
#[derive(Debug, Clone)]
pub enum ConnectionHealth {
    /// Connection is healthy
    Healthy,
    /// Connection is experiencing issues
    #[allow(dead_code)]
    Degraded { reason: String },
    /// Connection is unhealthy
    #[allow(dead_code)]
    Unhealthy { reason: String },
}

/// Device discovery configuration
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// Discovery interval
    pub discovery_interval: Duration,
    /// TCP ports to scan for devices
    pub tcp_ports: Vec<u16>,
    /// Connection timeout for device connections
    pub connection_timeout: Duration,
    /// Device connection health check interval
    pub health_check_interval: Duration,
    /// Maximum concurrent device connections
    pub max_device_connections: usize,
    /// Device response timeout
    pub device_response_timeout: Duration,
    /// Enable automatic discovery
    pub enable_auto_discovery: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            discovery_interval: Duration::from_secs(30),
            tcp_ports: vec![5555, 5556, 5557, 5558, 5559],
            connection_timeout: Duration::from_secs(5),
            health_check_interval: Duration::from_secs(60),
            max_device_connections: 10,
            device_response_timeout: Duration::from_secs(3),
            enable_auto_discovery: true,
        }
    }
}

/// Device lifecycle events
#[derive(Debug)]
enum DeviceEvent {
    /// Device discovered
    DeviceDiscovered {
        device_id: String,
        address: TransportAddress,
    },
    /// Device connected
    DeviceConnected {
        device_id: String,
    },
    /// Device disconnected
    DeviceDisconnected {
        device_id: String,
        reason: String,
    },
    /// Device health check result
    HealthCheck {
        device_id: String,
        healthy: bool,
        latency_ms: u64,
    },
    /// Discovery scan completed
    DiscoveryScanCompleted {
        devices_found: usize,
        scan_duration_ms: u64,
    },
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new(server_state: Arc<ServerState>, device_registry: Arc<DeviceRegistry>) -> Self {
        let discovery_config = DiscoveryConfig {
            tcp_ports: server_state.config.tcp_discovery_ports.clone(),
            discovery_interval: server_state.config.device_discovery_interval,
            ..Default::default()
        };

        Self::with_config(server_state, device_registry, discovery_config)
    }

    /// Create a new device manager with custom configuration
    pub fn with_config(
        server_state: Arc<ServerState>,
        device_registry: Arc<DeviceRegistry>,
        discovery_config: DiscoveryConfig,
    ) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        Self {
            server_state,
            device_registry,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            discovery_config,
            event_sender,
            event_receiver: Arc::new(RwLock::new(Some(event_receiver))),
        }
    }

    /// Start the device manager background tasks
    pub async fn start(&self) -> ProtocolResult<()> {
        info!("Starting device manager");

        // Take the event receiver
        let event_receiver = {
            let mut receiver_opt = self.event_receiver.write().unwrap();
            receiver_opt.take()
        };

        if let Some(mut event_receiver) = event_receiver {
            // Start event processing task
            let manager = self.clone();
            tokio::spawn(async move {
                manager.process_events(&mut event_receiver).await;
            });
        }

        // Start discovery task if enabled
        if self.discovery_config.enable_auto_discovery {
            let manager = self.clone();
            tokio::spawn(async move {
                manager.discovery_task().await;
            });
        }

        // Start health check task
        let manager = self.clone();
        tokio::spawn(async move {
            manager.health_check_task().await;
        });

        // Start cleanup task
        let manager = self.clone();
        tokio::spawn(async move {
            manager.cleanup_task().await;
        });

        Ok(())
    }

    /// Connect to a specific device (lazy connection)
    pub async fn connect_device(&self, device_id: &str) -> ProtocolResult<()> {
        // Check if already connected
        {
            let connections = self.active_connections.read().unwrap();
            if connections.contains_key(device_id) {
                debug!("Device {} already connected", device_id);
                return Ok(());
            }
        }

        // Check connection limits
        {
            let connections = self.active_connections.read().unwrap();
            if connections.len() >= self.discovery_config.max_device_connections {
                return Err(ProtocolError::MaxConnectionsExceeded {
                    count: connections.len(),
                    max: self.discovery_config.max_device_connections,
                });
            }
        }

        // Get device info from registry
        let device_info = match self.device_registry.get_device(device_id)? {
            Some(info) => info,
            None => return Err(ProtocolError::DeviceNotFound {
                device_id: device_id.to_string(),
            }),
        };

        info!("Connecting to device: {} at {}", device_id, device_info.address);

        // Create transport and connect
        let transport = TcpTransport::new();
        let connection = timeout(
            self.discovery_config.connection_timeout,
            transport.connect(&TransportAddress::Tcp(device_info.address)),
        )
        .await
        .map_err(|_| ProtocolError::Timeout {
            timeout_ms: self.discovery_config.connection_timeout.as_millis() as u64,
        })?
        .map_err(|e| ProtocolError::DeviceConnectionFailed {
            device_id: device_id.to_string(),
            reason: e.to_string(),
        })?;

        // Create device connection
        let device_connection = DeviceConnection {
            device_info: device_info.clone(),
            connection: Arc::new(Mutex::new(Box::new(connection))),
            connected_at: Instant::now(),
            last_activity: Instant::now(),
            health_status: ConnectionHealth::Healthy,
        };

        // Store connection
        {
            let mut connections = self.active_connections.write().unwrap();
            connections.insert(device_id.to_string(), device_connection);
        }

        // Update device state in registry
        self.device_registry.update_device_state(device_id, DeviceState::Connected)?;

        // Send event
        let _ = self.event_sender.send(DeviceEvent::DeviceConnected {
            device_id: device_id.to_string(),
        });

        info!("Successfully connected to device: {}", device_id);
        Ok(())
    }

    /// Disconnect from a specific device
    pub async fn disconnect_device(&self, device_id: &str, reason: &str) -> ProtocolResult<()> {
        info!("Disconnecting device: {} ({})", device_id, reason);

        // Remove from active connections
        let connection = {
            let mut connections = self.active_connections.write().unwrap();
            connections.remove(device_id)
        };

        if let Some(connection) = connection {
            // Close the connection
            let mut conn_guard = connection.connection.lock().await;
            if let Err(e) = conn_guard.close().await {
                warn!("Error closing device connection {}: {}", device_id, e);
            }
            drop(conn_guard);

            // Update device state in registry
            self.device_registry.update_device_state(device_id, DeviceState::Discovered)?;

            // Send event
            let _ = self.event_sender.send(DeviceEvent::DeviceDisconnected {
                device_id: device_id.to_string(),
                reason: reason.to_string(),
            });
        }

        Ok(())
    }

    /// Get connection to device, establishing it if needed (lazy connection)
    pub async fn get_device_connection(&self, device_id: &str) -> ProtocolResult<()> {
        // Check if already connected
        {
            let connections = self.active_connections.read().unwrap();
            if connections.contains_key(device_id) {
                return Ok(());
            }
        }

        // Connect to device lazily
        self.connect_device(device_id).await
    }

    /// Send data to a device
    pub async fn send_to_device(&self, device_id: &str, data: &[u8]) -> ProtocolResult<usize> {
        // Ensure device is connected
        self.get_device_connection(device_id).await?;

        // Get the connection Arc<Mutex<>> to release RwLock quickly
        let connection = {
            let mut connections = self.active_connections.write().unwrap();
            match connections.get_mut(device_id) {
                Some(device_connection) => {
                    device_connection.last_activity = Instant::now();
                    Arc::clone(&device_connection.connection)
                }
                None => {
                    return Err(ProtocolError::DeviceNotFound {
                        device_id: device_id.to_string(),
                    })
                }
            }
        };

        // Now send data without holding RwLock
        let mut conn_guard = connection.lock().await;
        match conn_guard.send_bytes(data).await {
            Ok(_) => Ok(data.len()),
            Err(e) => Err(ProtocolError::DeviceConnectionFailed {
                device_id: device_id.to_string(),
                reason: e.to_string(),
            })
        }
    }

    /// Receive data from a device
    pub async fn receive_from_device(&self, device_id: &str, buffer: &mut [u8]) -> ProtocolResult<usize> {
        // Ensure device is connected
        self.get_device_connection(device_id).await?;

        // Get the connection Arc<Mutex<>> to release RwLock quickly
        let connection = {
            let mut connections = self.active_connections.write().unwrap();
            match connections.get_mut(device_id) {
                Some(device_connection) => {
                    device_connection.last_activity = Instant::now();
                    Arc::clone(&device_connection.connection)
                }
                None => {
                    return Err(ProtocolError::DeviceNotFound {
                        device_id: device_id.to_string(),
                    })
                }
            }
        };

        // Now receive data without holding RwLock
        let mut conn_guard = connection.lock().await;
        let bytes_received = match conn_guard.receive_bytes(buffer).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => 0, // Connection closed
            Err(e) => {
                return Err(ProtocolError::DeviceConnectionFailed {
                    device_id: device_id.to_string(),
                    reason: e.to_string(),
                })
            }
        };

        Ok(bytes_received)
    }

    /// Get list of connected devices
    pub fn get_connected_devices(&self) -> Vec<String> {
        let connections = self.active_connections.read().unwrap();
        connections.keys().cloned().collect()
    }

    /// Get device connection statistics
    pub fn get_device_stats(&self, device_id: &str) -> Option<DeviceConnectionStats> {
        let connections = self.active_connections.read().unwrap();
        connections.get(device_id).map(|conn| DeviceConnectionStats {
            device_id: device_id.to_string(),
            connected_at: conn.connected_at,
            last_activity: conn.last_activity,
            uptime: conn.connected_at.elapsed(),
            health_status: conn.health_status.clone(),
        })
    }

    /// Perform device discovery scan
    pub async fn discover_devices(&self) -> ProtocolResult<Vec<String>> {
        let start_time = Instant::now();
        let mut discovered_devices = Vec::new();

        info!("Starting device discovery scan");

        for port in &self.discovery_config.tcp_ports {
            // Scan localhost and common IP ranges
            let addresses_to_scan = vec![
                format!("127.0.0.1:{}", port),
                format!("localhost:{}", port),
            ];

            for addr_str in addresses_to_scan {
                if let Ok(addr) = addr_str.parse() {
                    let transport_addr = TransportAddress::Tcp(addr);

                    // Try to connect with timeout
                    let transport = TcpTransport::new();
                    let connection_result = timeout(
                        Duration::from_secs(1), // Quick discovery timeout
                        transport.connect(&transport_addr),
                    ).await;

                    match connection_result {
                        Ok(Ok(mut connection)) => {
                            let device_id = format!("device-{}", addr);

                            // Close the discovery connection
                            let _ = connection.close().await;

                            // Create device info
                            let device_info = DeviceInfo {
                                device_id: device_id.clone(),
                                name: format!("TCP Device at {}", addr),
                                address: addr,
                                state: DeviceState::Discovered,
                                capabilities: DeviceCapabilities::default(),
                                metadata: DeviceMetadata::default(),
                                last_seen: Instant::now(),
                                stats: Default::default(),
                            };

                            // Register device
                            if let Ok(_) = self.device_registry.register_device(device_info) {
                                discovered_devices.push(device_id.clone());

                                // Send discovery event
                                let _ = self.event_sender.send(DeviceEvent::DeviceDiscovered {
                                    device_id,
                                    address: TransportAddress::Tcp(addr),
                                });
                            }
                        }
                        _ => {
                            // Connection failed - device not available on this port
                        }
                    }
                }
            }
        }

        let scan_duration = start_time.elapsed();
        info!("Discovery scan completed: {} devices found in {:?}", discovered_devices.len(), scan_duration);

        // Send completion event
        let _ = self.event_sender.send(DeviceEvent::DiscoveryScanCompleted {
            devices_found: discovered_devices.len(),
            scan_duration_ms: scan_duration.as_millis() as u64,
        });

        Ok(discovered_devices)
    }

    /// Background discovery task
    async fn discovery_task(&self) {
        let mut interval = interval(self.discovery_config.discovery_interval);

        loop {
            interval.tick().await;

            if let Err(e) = self.discover_devices().await {
                warn!("Device discovery error: {}", e);
            }
        }
    }

    /// Background health check task
    async fn health_check_task(&self) {
        let mut interval = interval(self.discovery_config.health_check_interval);

        loop {
            interval.tick().await;

            let device_ids = {
                let connections = self.active_connections.read().unwrap();
                connections.keys().cloned().collect::<Vec<_>>()
            };

            for device_id in device_ids {
                let start_time = Instant::now();

                // Implement health check by validating connection state and device registry status
                // Get connection Arc to release RwLock quickly
                let connection_opt = {
                    let connections = self.active_connections.read().unwrap();
                    connections.get(&device_id).map(|conn| Arc::clone(&conn.connection))
                };

                let is_healthy = match connection_opt {
                    Some(connection) => {
                        // Check basic connection status
                        let conn_guard = connection.lock().await;
                        if conn_guard.is_connected() {
                            drop(conn_guard); // Release mutex before registry check
                            // Also verify device is still in registry and in good state
                            match self.device_registry.get_device(&device_id) {
                                Ok(Some(device_info)) => {
                                    match device_info.state {
                                        crate::server::device_registry::DeviceState::Connected => {
                                            debug!("Device {} is healthy (connected and in registry)", device_id);
                                            true
                                        }
                                        state => {
                                            warn!("Device {} connection exists but registry state is {:?}", device_id, state);
                                            false
                                        }
                                    }
                                }
                                Ok(None) => {
                                    warn!("Device {} has connection but not found in registry", device_id);
                                    false
                                }
                                Err(e) => {
                                    warn!("Failed to check device {} in registry: {}", device_id, e);
                                    false
                                }
                            }
                        } else {
                            debug!("Device {} connection is not active", device_id);
                            false
                        }
                    }
                    None => {
                        debug!("Device {} has no active connection", device_id);
                        false
                    }
                };

                let latency = start_time.elapsed().as_millis() as u64;

                // Send health check event
                let _ = self.event_sender.send(DeviceEvent::HealthCheck {
                    device_id: device_id.clone(),
                    healthy: is_healthy,
                    latency_ms: latency,
                });

                // Update health status
                if !is_healthy {
                    warn!("Device {} failed health check", device_id);
                    let _ = self.disconnect_device(&device_id, "Health check failed").await;
                }
            }
        }
    }

    /// Background cleanup task
    async fn cleanup_task(&self) {
        let mut interval = interval(Duration::from_secs(60));

        loop {
            interval.tick().await;

            let devices_to_cleanup = {
                let connections = self.active_connections.read().unwrap();
                let now = Instant::now();
                let inactive_timeout = Duration::from_secs(300); // 5 minutes

                connections
                    .iter()
                    .filter_map(|(device_id, conn)| {
                        if now.duration_since(conn.last_activity) > inactive_timeout {
                            Some(device_id.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            };

            for device_id in devices_to_cleanup {
                warn!("Cleaning up inactive device connection: {}", device_id);
                let _ = self.disconnect_device(&device_id, "Inactivity timeout").await;
            }
        }
    }

    /// Process device events
    async fn process_events(&self, event_receiver: &mut mpsc::UnboundedReceiver<DeviceEvent>) {
        while let Some(event) = event_receiver.recv().await {
            match event {
                DeviceEvent::DeviceDiscovered { device_id, address } => {
                    debug!("Device discovered: {} at {}", device_id, address);
                }
                DeviceEvent::DeviceConnected { device_id } => {
                    info!("Device connected: {}", device_id);
                }
                DeviceEvent::DeviceDisconnected { device_id, reason } => {
                    info!("Device disconnected: {} ({})", device_id, reason);
                }
                DeviceEvent::HealthCheck { device_id, healthy, latency_ms } => {
                    if healthy {
                        debug!("Device {} health check: OK ({}ms)", device_id, latency_ms);
                    } else {
                        warn!("Device {} health check: FAILED", device_id);
                    }
                }
                DeviceEvent::DiscoveryScanCompleted { devices_found, scan_duration_ms } => {
                    debug!(
                        "Discovery scan completed: {} devices found in {}ms",
                        devices_found, scan_duration_ms
                    );
                }
            }
        }
    }
}

impl Clone for DeviceManager {
    fn clone(&self) -> Self {
        Self {
            server_state: Arc::clone(&self.server_state),
            device_registry: Arc::clone(&self.device_registry),
            active_connections: Arc::clone(&self.active_connections),
            discovery_config: self.discovery_config.clone(),
            event_sender: self.event_sender.clone(),
            event_receiver: Arc::clone(&self.event_receiver),
        }
    }
}

/// Device connection statistics
#[derive(Debug, Clone)]
pub struct DeviceConnectionStats {
    pub device_id: String,
    pub connected_at: Instant,
    pub last_activity: Instant,
    pub uptime: Duration,
    pub health_status: ConnectionHealth,
}

impl DeviceConnectionStats {
    /// Check if connection is healthy
    pub fn is_healthy(&self) -> bool {
        matches!(self.health_status, ConnectionHealth::Healthy)
    }

    /// Get inactive duration
    pub fn inactive_duration(&self) -> Duration {
        self.last_activity.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::state::{ServerConfig, ServerState};
    use std::sync::Arc;

    fn create_test_manager() -> DeviceManager {
        let config = ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap());
        let server_state = Arc::new(ServerState::new(config));
        let device_registry = Arc::new(DeviceRegistry::new());

        DeviceManager::new(server_state, device_registry)
    }

    #[tokio::test]
    async fn test_device_manager_creation() {
        let manager = create_test_manager();
        let connected = manager.get_connected_devices();
        assert_eq!(connected.len(), 0);
    }

    #[tokio::test]
    async fn test_device_discovery() {
        let manager = create_test_manager();

        // Discovery might not find any devices in test environment
        let result = manager.discover_devices().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_connection_limits() {
        let mut discovery_config = DiscoveryConfig::default();
        discovery_config.max_device_connections = 0; // No connections allowed

        let config = ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap());
        let server_state = Arc::new(ServerState::new(config));
        let device_registry = Arc::new(DeviceRegistry::new());

        let manager = DeviceManager::with_config(server_state, device_registry, discovery_config);

        // Try to connect to non-existent device (should fail due to limits)
        let result = manager.connect_device("test-device").await;

        // This will fail because device doesn't exist in registry first
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_device_stats() {
        let manager = create_test_manager();

        // No devices connected initially
        let stats = manager.get_device_stats("nonexistent");
        assert!(stats.is_none());
    }

    #[tokio::test]
    async fn test_lazy_connection() {
        let manager = create_test_manager();

        // Try to get connection to non-existent device
        let result = manager.get_device_connection("test-device").await;
        assert!(result.is_err()); // Should fail because device not in registry
    }
}