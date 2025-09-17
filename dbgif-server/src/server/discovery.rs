use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, timeout};
use tracing::{debug, info, warn};

use dbgif_protocol::error::{ProtocolError, ProtocolResult};
use dbgif_protocol::commands::AdbCommand;
use crate::server::device_registry::{DeviceRegistry, DeviceInfo, DeviceState, DeviceCapabilities, DeviceMetadata, DeviceStats};

/// Device discovery service for finding available devices on the network
pub struct DeviceDiscovery {
    /// Device registry to update with discovered devices
    device_registry: Arc<DeviceRegistry>,
    /// Discovery configuration
    config: DiscoveryConfig,
    /// Currently scanning addresses
    scanning_addresses: Arc<RwLock<HashSet<SocketAddr>>>,
    /// Discovery statistics
    stats: Arc<RwLock<DiscoveryStats>>,
    /// Shutdown signal receiver
    shutdown_rx: Option<mpsc::Receiver<()>>,
    /// Shutdown signal sender
    shutdown_tx: mpsc::Sender<()>,
}

/// Device discovery configuration
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// Discovery interval (how often to scan)
    pub discovery_interval: Duration,
    /// Connection timeout for device probing
    pub connection_timeout: Duration,
    /// TCP ports to scan for devices
    pub tcp_ports: Vec<u16>,
    /// IP address ranges to scan
    pub ip_ranges: Vec<IpRange>,
    /// Maximum concurrent scans
    pub max_concurrent_scans: usize,
    /// Device response timeout
    pub device_timeout: Duration,
    /// Enable automatic discovery
    pub auto_discovery_enabled: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            discovery_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(5),
            tcp_ports: vec![5555, 5556, 5557], // Common ADB ports
            ip_ranges: vec![
                IpRange::new(Ipv4Addr::new(127, 0, 0, 1), Ipv4Addr::new(127, 0, 0, 1)), // localhost
                IpRange::new(Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(192, 168, 1, 254)), // common LAN
            ],
            max_concurrent_scans: 50,
            device_timeout: Duration::from_secs(10),
            auto_discovery_enabled: true,
        }
    }
}

/// IP address range for scanning
#[derive(Debug, Clone)]
pub struct IpRange {
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
}

impl IpRange {
    pub fn new(start: Ipv4Addr, end: Ipv4Addr) -> Self {
        Self { start, end }
    }

    /// Generate all IP addresses in this range
    pub fn addresses(&self) -> Vec<Ipv4Addr> {
        let start_u32: u32 = self.start.into();
        let end_u32: u32 = self.end.into();

        (start_u32..=end_u32)
            .map(|ip| ip.into())
            .collect()
    }
}

/// Discovery statistics
#[derive(Debug, Default)]
pub struct DiscoveryStats {
    /// Total scans performed
    pub total_scans: u64,
    /// Total addresses scanned
    pub addresses_scanned: u64,
    /// Devices discovered
    pub devices_discovered: u64,
    /// Failed connection attempts
    pub failed_connections: u64,
    /// Last discovery start time
    pub last_discovery: Option<Instant>,
    /// Discovery duration (last scan)
    pub last_discovery_duration: Option<Duration>,
    /// Currently active scans
    pub active_scans: usize,
}

/// Discovery result for a single address
#[derive(Debug)]
pub struct DiscoveryResult {
    pub address: SocketAddr,
    pub success: bool,
    pub device_info: Option<DeviceInfo>,
    pub error: Option<String>,
    pub response_time: Duration,
}

impl DeviceDiscovery {
    /// Create a new device discovery service
    pub fn new(device_registry: Arc<DeviceRegistry>, config: DiscoveryConfig) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        Self {
            device_registry,
            config,
            scanning_addresses: Arc::new(RwLock::new(HashSet::new())),
            stats: Arc::new(RwLock::new(DiscoveryStats::default())),
            shutdown_rx: Some(shutdown_rx),
            shutdown_tx,
        }
    }

    /// Start the device discovery background task
    pub async fn start_discovery(&mut self) -> ProtocolResult<tokio::task::JoinHandle<()>> {
        let config = self.config.clone();
        let device_registry = self.device_registry.clone();
        let scanning_addresses = self.scanning_addresses.clone();
        let stats = self.stats.clone();
        let mut shutdown_rx = self.shutdown_rx.take().unwrap();

        let handle = tokio::spawn(async move {
            let mut discovery_interval = interval(config.discovery_interval);

            info!("Starting device discovery (interval: {:?})", config.discovery_interval);

            loop {
                tokio::select! {
                    // Perform periodic discovery
                    _ = discovery_interval.tick(), if config.auto_discovery_enabled => {
                        Self::perform_discovery(&config, &device_registry, &scanning_addresses, &stats).await;
                    }

                    // Handle shutdown
                    _ = shutdown_rx.recv() => {
                        info!("Device discovery shutting down");
                        break;
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Perform a single discovery scan
    async fn perform_discovery(
        config: &DiscoveryConfig,
        device_registry: &Arc<DeviceRegistry>,
        scanning_addresses: &Arc<RwLock<HashSet<SocketAddr>>>,
        stats: &Arc<RwLock<DiscoveryStats>>,
    ) {
        let start_time = Instant::now();

        // Update stats
        {
            let mut stats_guard = stats.write().await;
            stats_guard.total_scans += 1;
            stats_guard.last_discovery = Some(start_time);
        }

        info!("Starting device discovery scan");

        // Generate all addresses to scan
        let mut addresses_to_scan = Vec::new();
        for ip_range in &config.ip_ranges {
            for ip in ip_range.addresses() {
                for port in &config.tcp_ports {
                    addresses_to_scan.push(SocketAddr::new(IpAddr::V4(ip), *port));
                }
            }
        }

        debug!("Scanning {} addresses across {} ports", addresses_to_scan.len(), config.tcp_ports.len());

        // Update scanning addresses
        {
            let mut scanning_guard = scanning_addresses.write().await;
            scanning_guard.clear();
            scanning_guard.extend(&addresses_to_scan);
        }

        // Scan addresses concurrently
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent_scans));
        let mut scan_tasks = Vec::new();

        for address in addresses_to_scan {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let config = config.clone();
            let device_registry = device_registry.clone();
            let stats = stats.clone();

            let task = tokio::spawn(async move {
                let result = Self::scan_address(address, &config).await;

                // Update stats
                {
                    let mut stats_guard = stats.write().await;
                    stats_guard.addresses_scanned += 1;
                    stats_guard.active_scans -= 1;

                    if result.success {
                        stats_guard.devices_discovered += 1;
                    } else {
                        stats_guard.failed_connections += 1;
                    }
                }

                // Register discovered device
                if let Some(ref device_info) = result.device_info {
                    if let Err(e) = device_registry.register_device(device_info.clone()) {
                        warn!("Failed to register discovered device at {}: {}", address, e);
                    } else {
                        info!("Discovered device at {}", address);
                    }
                }

                drop(permit);
                result
            });

            // Note: active_scans counter is updated inside the spawn task

            scan_tasks.push(task);
        }

        // Wait for all scans to complete
        let mut results = Vec::new();
        for task in scan_tasks {
            match task.await {
                Ok(result) => results.push(result),
                Err(e) => warn!("Scan task failed: {}", e),
            }
        }
        let successful_scans = results.iter().filter(|r| r.success).count();

        // Clear scanning addresses
        {
            let mut scanning_guard = scanning_addresses.write().await;
            scanning_guard.clear();
        }

        let duration = start_time.elapsed();

        // Update final stats
        {
            let mut stats_guard = stats.write().await;
            stats_guard.last_discovery_duration = Some(duration);
        }

        info!(
            "Discovery scan completed in {:?}: {} devices found from {} addresses",
            duration,
            successful_scans,
            results.len()
        );
    }

    /// Scan a single address for device presence
    async fn scan_address(address: SocketAddr, config: &DiscoveryConfig) -> DiscoveryResult {
        let start_time = Instant::now();

        debug!("Scanning address: {}", address);

        match timeout(config.connection_timeout, TcpStream::connect(address)).await {
            Ok(Ok(mut stream)) => {
                // Successfully connected, try to identify device
                match Self::probe_device(&mut stream, config).await {
                    Ok(device_info) => {
                        debug!("Device discovered at {}: {:?}", address, device_info.device_id);
                        DiscoveryResult {
                            address,
                            success: true,
                            device_info: Some(device_info),
                            error: None,
                            response_time: start_time.elapsed(),
                        }
                    }
                    Err(e) => {
                        debug!("Device probe failed at {}: {}", address, e);
                        DiscoveryResult {
                            address,
                            success: false,
                            device_info: None,
                            error: Some(e.to_string()),
                            response_time: start_time.elapsed(),
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                debug!("Connection failed to {}: {}", address, e);
                DiscoveryResult {
                    address,
                    success: false,
                    device_info: None,
                    error: Some(e.to_string()),
                    response_time: start_time.elapsed(),
                }
            }
            Err(_) => {
                debug!("Connection timeout to {}", address);
                DiscoveryResult {
                    address,
                    success: false,
                    device_info: None,
                    error: Some("Connection timeout".to_string()),
                    response_time: start_time.elapsed(),
                }
            }
        }
    }

    /// Probe a connected device to identify it
    async fn probe_device(stream: &mut TcpStream, config: &DiscoveryConfig) -> ProtocolResult<DeviceInfo> {
        let peer_addr = stream.peer_addr()
            .map_err(|e| ProtocolError::ConnectionError {
                message: format!("Failed to get peer address: {}", e)
            })?;

        // Send ADB CNXN message to identify the device
        let cnxn_message = dbgif_protocol::message::AdbMessage::new_cnxn(
            1, // local_id
            0x01000000, // version
            format!("host:discovery:{}", peer_addr).into_bytes()
        );

        // Send CNXN message
        let serialized = cnxn_message.serialize();
        match stream.write_all(&serialized).await {
            Ok(()) => {
                debug!("Sent CNXN message to device at {}", peer_addr);
            }
            Err(e) => {
                debug!("Failed to send CNXN to {}: {}", peer_addr, e);
                // Continue with fallback identification
            }
        }

        // Wait for response with timeout
        let mut buffer = vec![0u8; 1024];
        let device_id = match tokio::time::timeout(
            config.connection_timeout,
            stream.read(&mut buffer)
        ).await {
            Ok(Ok(response_size)) if response_size >= 24 => {
                // Try to parse ADB response
                match dbgif_protocol::message::AdbMessage::deserialize(&buffer[..response_size]) {
                    Ok(response) => {
                        if response.command == AdbCommand::CNXN as u32 {
                            // Extract device identity from response data
                            let identity = String::from_utf8_lossy(&response.data);
                            if identity.is_empty() {
                                format!("tcp_device_{}", peer_addr.port())
                            } else {
                                format!("{}_{}", identity.trim(), peer_addr.port())
                            }
                        } else {
                            format!("tcp_device_{}", peer_addr.port())
                        }
                    }
                    Err(_) => format!("tcp_device_{}", peer_addr.port())
                }
            }
            _ => {
                debug!("No valid response from {}, using fallback ID", peer_addr);
                format!("tcp_device_{}", peer_addr.port())
            }
        };

        Ok(DeviceInfo {
            device_id: device_id.clone(),
            name: format!("Device at {}", peer_addr),
            address: peer_addr,
            state: DeviceState::Discovered,
            capabilities: DeviceCapabilities::default(),
            metadata: DeviceMetadata {
                model: None,
                manufacturer: None,
                serial_number: None,
                os_version: None,
                properties: std::collections::HashMap::from([
                    ("discovery_method".to_string(), "tcp_scan".to_string()),
                    ("discovered_at".to_string(), std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                        .to_string()),
                    ("probe_timeout".to_string(), format!("{:?}", config.device_timeout)),
                ]),
            },
            last_seen: std::time::Instant::now(),
            stats: DeviceStats::default(),
        })
    }

    /// Manually trigger a discovery scan
    pub async fn trigger_discovery(&self) -> ProtocolResult<()> {
        info!("Manually triggering device discovery");

        Self::perform_discovery(
            &self.config,
            &self.device_registry,
            &self.scanning_addresses,
            &self.stats,
        ).await;

        Ok(())
    }

    /// Get discovery statistics
    pub async fn get_stats(&self) -> DiscoveryStats {
        let stats = self.stats.read().await;
        DiscoveryStats {
            total_scans: stats.total_scans,
            addresses_scanned: stats.addresses_scanned,
            devices_discovered: stats.devices_discovered,
            failed_connections: stats.failed_connections,
            last_discovery: stats.last_discovery,
            last_discovery_duration: stats.last_discovery_duration,
            active_scans: stats.active_scans,
        }
    }

    /// Check if discovery is currently running
    pub async fn is_scanning(&self) -> bool {
        let scanning = self.scanning_addresses.read().await;
        !scanning.is_empty()
    }

    /// Get currently scanning addresses
    pub async fn get_scanning_addresses(&self) -> Vec<SocketAddr> {
        let scanning = self.scanning_addresses.read().await;
        scanning.iter().copied().collect()
    }

    /// Update discovery configuration
    pub fn update_config(&mut self, new_config: DiscoveryConfig) {
        self.config = new_config;
        info!("Discovery configuration updated");
    }

    /// Add IP range for scanning
    pub fn add_ip_range(&mut self, range: IpRange) {
        let start = range.start;
        let end = range.end;
        self.config.ip_ranges.push(range);
        info!("Added IP range for discovery: {:?}-{:?}", start, end);
    }

    /// Add TCP port for scanning
    pub fn add_tcp_port(&mut self, port: u16) {
        if !self.config.tcp_ports.contains(&port) {
            self.config.tcp_ports.push(port);
            info!("Added TCP port for discovery: {}", port);
        }
    }

    /// Shutdown discovery service
    pub async fn shutdown(&self) -> ProtocolResult<()> {
        info!("Shutting down device discovery");

        if let Err(e) = self.shutdown_tx.send(()).await {
            warn!("Failed to send discovery shutdown signal: {}", e);
        }

        // Wait a bit for shutdown to complete
        tokio::time::sleep(Duration::from_millis(100)).await;

        let stats = self.get_stats().await;
        info!(
            "Device discovery shutdown complete (scanned {} addresses, found {} devices)",
            stats.addresses_scanned,
            stats.devices_discovered
        );

        Ok(())
    }
}

/// Builder for device discovery configuration
pub struct DiscoveryConfigBuilder {
    config: DiscoveryConfig,
}

impl DiscoveryConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: DiscoveryConfig::default(),
        }
    }

    pub fn discovery_interval(mut self, interval: Duration) -> Self {
        self.config.discovery_interval = interval;
        self
    }

    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.config.connection_timeout = timeout;
        self
    }

    pub fn tcp_ports(mut self, ports: Vec<u16>) -> Self {
        self.config.tcp_ports = ports;
        self
    }

    pub fn ip_ranges(mut self, ranges: Vec<IpRange>) -> Self {
        self.config.ip_ranges = ranges;
        self
    }

    pub fn max_concurrent_scans(mut self, max: usize) -> Self {
        self.config.max_concurrent_scans = max;
        self
    }

    pub fn auto_discovery(mut self, enabled: bool) -> Self {
        self.config.auto_discovery_enabled = enabled;
        self
    }

    pub fn build(self) -> DiscoveryConfig {
        self.config
    }
}

impl Default for DiscoveryConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_ip_range_addresses() {
        let range = IpRange::new(
            Ipv4Addr::new(192, 168, 1, 1),
            Ipv4Addr::new(192, 168, 1, 3)
        );

        let addresses = range.addresses();
        assert_eq!(addresses.len(), 3);
        assert_eq!(addresses[0], Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(addresses[1], Ipv4Addr::new(192, 168, 1, 2));
        assert_eq!(addresses[2], Ipv4Addr::new(192, 168, 1, 3));
    }

    #[test]
    fn test_discovery_config_builder() {
        let config = DiscoveryConfigBuilder::new()
            .discovery_interval(Duration::from_secs(60))
            .connection_timeout(Duration::from_secs(10))
            .tcp_ports(vec![5555, 5556])
            .max_concurrent_scans(25)
            .auto_discovery(false)
            .build();

        assert_eq!(config.discovery_interval, Duration::from_secs(60));
        assert_eq!(config.connection_timeout, Duration::from_secs(10));
        assert_eq!(config.tcp_ports, vec![5555, 5556]);
        assert_eq!(config.max_concurrent_scans, 25);
        assert!(!config.auto_discovery_enabled);
    }

    #[tokio::test]
    async fn test_device_discovery_creation() {
        let registry = Arc::new(DeviceRegistry::new());
        let config = DiscoveryConfig::default();
        let discovery = DeviceDiscovery::new(registry, config);

        // Should start with no scanning addresses
        let scanning = discovery.get_scanning_addresses().await;
        assert!(scanning.is_empty());

        // Should not be scanning initially
        assert!(!discovery.is_scanning().await);

        // Should have default stats
        let stats = discovery.get_stats().await;
        assert_eq!(stats.total_scans, 0);
        assert_eq!(stats.devices_discovered, 0);
    }

    #[tokio::test]
    async fn test_discovery_stats() {
        let registry = Arc::new(DeviceRegistry::new());
        let config = DiscoveryConfig::default();
        let discovery = DeviceDiscovery::new(registry, config);

        let mut stats = discovery.get_stats().await;
        assert_eq!(stats.total_scans, 0);
        assert_eq!(stats.addresses_scanned, 0);
        assert_eq!(stats.devices_discovered, 0);
        assert_eq!(stats.failed_connections, 0);
        assert!(stats.last_discovery.is_none());

        // Manually update some stats to test
        {
            let mut stats_guard = discovery.stats.write().await;
            stats_guard.total_scans = 1;
            stats_guard.addresses_scanned = 10;
            stats_guard.devices_discovered = 2;
            stats_guard.failed_connections = 8;
            stats_guard.last_discovery = Some(Instant::now());
        }

        stats = discovery.get_stats().await;
        assert_eq!(stats.total_scans, 1);
        assert_eq!(stats.addresses_scanned, 10);
        assert_eq!(stats.devices_discovered, 2);
        assert_eq!(stats.failed_connections, 8);
        assert!(stats.last_discovery.is_some());
    }

    #[test]
    fn test_default_discovery_config() {
        let config = DiscoveryConfig::default();

        assert_eq!(config.discovery_interval, Duration::from_secs(30));
        assert_eq!(config.connection_timeout, Duration::from_secs(5));
        assert_eq!(config.tcp_ports, vec![5555, 5556, 5557]);
        assert_eq!(config.ip_ranges.len(), 2); // localhost + common LAN
        assert_eq!(config.max_concurrent_scans, 50);
        assert!(config.auto_discovery_enabled);
    }

    #[tokio::test]
    async fn test_discovery_config_updates() {
        let registry = Arc::new(DeviceRegistry::new());
        let config = DiscoveryConfig::default();
        let mut discovery = DeviceDiscovery::new(registry, config);

        // Add IP range
        discovery.add_ip_range(IpRange::new(
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(10, 0, 0, 10)
        ));
        assert_eq!(discovery.config.ip_ranges.len(), 3);

        // Add TCP port
        discovery.add_tcp_port(8080);
        assert!(discovery.config.tcp_ports.contains(&8080));

        // Don't add duplicate port
        discovery.add_tcp_port(8080);
        assert_eq!(discovery.config.tcp_ports.iter().filter(|&&p| p == 8080).count(), 1);
    }
}