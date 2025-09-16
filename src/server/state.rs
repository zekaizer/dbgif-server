use crate::host_services::HostServiceRegistry;
use crate::server::device_registry::DeviceRegistry;
use crate::server::session::ClientSession;
use crate::transport::TransportAddress;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Global server state and coordination
#[derive(Debug)]
pub struct ServerState {
    /// Server configuration
    pub config: ServerConfig,
    /// Active client sessions
    pub client_sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
    /// Device registry for managing connected devices
    pub device_registry: Arc<DeviceRegistry>,
    /// Host service registry for built-in services
    pub host_services: Arc<RwLock<HostServiceRegistry>>,
    /// Session ID counter
    pub session_counter: AtomicU64,
    /// Stream ID counter
    pub stream_counter: AtomicU64,
    /// Server statistics
    pub stats: ServerStats,
    /// Server start time
    pub start_time: Instant,
}

impl ServerState {
    /// Create a new server state with given configuration
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            client_sessions: Arc::new(RwLock::new(HashMap::new())),
            device_registry: Arc::new(DeviceRegistry::new()),
            host_services: Arc::new(RwLock::new(HostServiceRegistry::new())),
            session_counter: AtomicU64::new(0),
            stream_counter: AtomicU64::new(0),
            stats: ServerStats::new(),
            start_time: Instant::now(),
        }
    }

    /// Get next session ID
    pub fn next_session_id(&self) -> String {
        let id = self.session_counter.fetch_add(1, Ordering::SeqCst);
        format!("session-{}", id)
    }

    /// Get next stream ID
    pub fn next_stream_id(&self) -> u32 {
        // Stream IDs range from 1-65535 (0 is reserved)
        let id = self.stream_counter.fetch_add(1, Ordering::SeqCst) % 65535 + 1;
        id as u32
    }

    /// Get number of active sessions
    pub fn active_session_count(&self) -> usize {
        self.client_sessions.read().unwrap().len()
    }

    /// Check if server can accept new connections
    pub fn can_accept_connection(&self) -> bool {
        self.active_session_count() < self.config.max_connections
    }

    /// Get server uptime
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Validate server state consistency
    pub fn validate(&self) -> Result<(), ServerStateError> {
        let active_sessions = self.active_session_count();

        if active_sessions > self.config.max_connections {
            return Err(ServerStateError::ExceededMaxConnections {
                current: active_sessions,
                max: self.config.max_connections,
            });
        }

        // Validate stats consistency
        let _total_sessions = self.stats.total_sessions.load(Ordering::SeqCst);
        let active_sessions_stat = self.stats.active_sessions.load(Ordering::SeqCst);

        if active_sessions_stat as usize != active_sessions {
            return Err(ServerStateError::InconsistentStats {
                message: format!(
                    "Active sessions mismatch: actual={}, stats={}",
                    active_sessions, active_sessions_stat
                )
            });
        }

        Ok(())
    }
}

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind the server to
    pub bind_addr: TransportAddress,
    /// Maximum number of concurrent connections
    pub max_connections: usize,
    /// Connection timeout duration
    pub connection_timeout: Duration,
    /// Ping interval for connection health monitoring
    pub ping_interval: Duration,
    /// Ping response timeout
    pub ping_timeout: Duration,
    /// Device discovery interval
    pub device_discovery_interval: Duration,
    /// TCP ports to scan for device discovery
    pub tcp_discovery_ports: Vec<u16>,
}

impl ServerConfig {
    /// Create a new server configuration with defaults
    pub fn new(bind_addr: TransportAddress) -> Self {
        Self {
            bind_addr,
            max_connections: 100,
            connection_timeout: Duration::from_secs(30),
            ping_interval: Duration::from_secs(60),
            ping_timeout: Duration::from_secs(10),
            device_discovery_interval: Duration::from_secs(30),
            tcp_discovery_ports: vec![5555, 5556, 5557],
        }
    }

    /// Create a server configuration from TCP socket address
    pub fn from_tcp_addr(addr: SocketAddr) -> Self {
        Self::new(TransportAddress::Tcp(addr))
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), ServerConfigError> {
        if self.max_connections == 0 || self.max_connections > 1000 {
            return Err(ServerConfigError::InvalidMaxConnections(self.max_connections));
        }

        if self.connection_timeout.as_secs() == 0 {
            return Err(ServerConfigError::InvalidTimeout("connection_timeout".to_string()));
        }

        if self.ping_interval.as_secs() == 0 {
            return Err(ServerConfigError::InvalidTimeout("ping_interval".to_string()));
        }

        if self.ping_timeout.as_secs() == 0 {
            return Err(ServerConfigError::InvalidTimeout("ping_timeout".to_string()));
        }

        if self.device_discovery_interval.as_secs() == 0 {
            return Err(ServerConfigError::InvalidTimeout("device_discovery_interval".to_string()));
        }

        if self.tcp_discovery_ports.is_empty() {
            return Err(ServerConfigError::EmptyDiscoveryPorts);
        }

        // Check for duplicate ports
        let mut sorted_ports = self.tcp_discovery_ports.clone();
        sorted_ports.sort();
        sorted_ports.dedup();
        if sorted_ports.len() != self.tcp_discovery_ports.len() {
            return Err(ServerConfigError::DuplicateDiscoveryPorts);
        }

        Ok(())
    }

    /// Get bind address as string for logging
    pub fn bind_address_str(&self) -> String {
        self.bind_addr.to_string()
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::from_tcp_addr("127.0.0.1:5555".parse().unwrap())
    }
}

/// Server statistics
#[derive(Debug)]
pub struct ServerStats {
    /// Total number of sessions created
    pub total_sessions: AtomicU64,
    /// Current number of active sessions
    pub active_sessions: AtomicU64,
    /// Total number of streams created
    pub total_streams: AtomicU64,
    /// Current number of active streams
    pub active_streams: AtomicU64,
    /// Total messages processed
    pub messages_processed: AtomicU64,
    /// Total bytes transferred
    pub bytes_transferred: AtomicU64,
    /// Total errors encountered
    pub errors_count: AtomicU64,
}

impl ServerStats {
    /// Create new server statistics
    pub fn new() -> Self {
        Self {
            total_sessions: AtomicU64::new(0),
            active_sessions: AtomicU64::new(0),
            total_streams: AtomicU64::new(0),
            active_streams: AtomicU64::new(0),
            messages_processed: AtomicU64::new(0),
            bytes_transferred: AtomicU64::new(0),
            errors_count: AtomicU64::new(0),
        }
    }

    /// Record a new session created
    pub fn session_created(&self) {
        self.total_sessions.fetch_add(1, Ordering::SeqCst);
        self.active_sessions.fetch_add(1, Ordering::SeqCst);
    }

    /// Record a session closed
    pub fn session_closed(&self) {
        self.active_sessions.fetch_sub(1, Ordering::SeqCst);
    }

    /// Record a new stream created
    pub fn stream_created(&self) {
        self.total_streams.fetch_add(1, Ordering::SeqCst);
        self.active_streams.fetch_add(1, Ordering::SeqCst);
    }

    /// Record a stream closed
    pub fn stream_closed(&self) {
        self.active_streams.fetch_sub(1, Ordering::SeqCst);
    }

    /// Record message processed
    pub fn message_processed(&self, bytes: u64) {
        self.messages_processed.fetch_add(1, Ordering::SeqCst);
        self.bytes_transferred.fetch_add(bytes, Ordering::SeqCst);
    }

    /// Record an error
    pub fn error_occurred(&self) {
        self.errors_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Get statistics snapshot
    pub fn snapshot(&self) -> ServerStatsSnapshot {
        ServerStatsSnapshot {
            total_sessions: self.total_sessions.load(Ordering::SeqCst),
            active_sessions: self.active_sessions.load(Ordering::SeqCst),
            total_streams: self.total_streams.load(Ordering::SeqCst),
            active_streams: self.active_streams.load(Ordering::SeqCst),
            messages_processed: self.messages_processed.load(Ordering::SeqCst),
            bytes_transferred: self.bytes_transferred.load(Ordering::SeqCst),
            errors_count: self.errors_count.load(Ordering::SeqCst),
        }
    }

    /// Reset all statistics
    pub fn reset(&self) {
        self.total_sessions.store(0, Ordering::SeqCst);
        self.active_sessions.store(0, Ordering::SeqCst);
        self.total_streams.store(0, Ordering::SeqCst);
        self.active_streams.store(0, Ordering::SeqCst);
        self.messages_processed.store(0, Ordering::SeqCst);
        self.bytes_transferred.store(0, Ordering::SeqCst);
        self.errors_count.store(0, Ordering::SeqCst);
    }
}

impl Default for ServerStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of server statistics at a point in time
#[derive(Debug, Clone, Copy)]
pub struct ServerStatsSnapshot {
    pub total_sessions: u64,
    pub active_sessions: u64,
    pub total_streams: u64,
    pub active_streams: u64,
    pub messages_processed: u64,
    pub bytes_transferred: u64,
    pub errors_count: u64,
}

impl ServerStatsSnapshot {
    /// Get error rate as percentage (0.0 to 100.0)
    pub fn error_rate(&self) -> f64 {
        if self.messages_processed == 0 {
            0.0
        } else {
            (self.errors_count as f64 / self.messages_processed as f64) * 100.0
        }
    }

    /// Get average message size in bytes
    pub fn avg_message_size(&self) -> f64 {
        if self.messages_processed == 0 {
            0.0
        } else {
            self.bytes_transferred as f64 / self.messages_processed as f64
        }
    }
}

/// Server state errors
#[derive(Error, Debug)]
pub enum ServerStateError {
    #[error("Exceeded maximum connections: {current}/{max}")]
    ExceededMaxConnections { current: usize, max: usize },

    #[error("Inconsistent statistics: {message}")]
    InconsistentStats { message: String },

    #[error("Configuration error: {0}")]
    Configuration(#[from] ServerConfigError),
}

/// Server configuration errors
#[derive(Error, Debug)]
pub enum ServerConfigError {
    #[error("Invalid maximum connections: {0} (must be between 1 and 1000)")]
    InvalidMaxConnections(usize),

    #[error("Invalid timeout: {0} (must be greater than 0)")]
    InvalidTimeout(String),

    #[error("Discovery ports list is empty")]
    EmptyDiscoveryPorts,

    #[error("Duplicate ports in discovery ports list")]
    DuplicateDiscoveryPorts,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn create_test_config() -> ServerConfig {
        ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap())
    }

    #[test]
    fn test_server_state_creation() {
        let config = create_test_config();
        let state = ServerState::new(config);

        assert_eq!(state.active_session_count(), 0);
        assert!(state.can_accept_connection());
        assert!(state.uptime().as_nanos() > 0);
    }

    #[test]
    fn test_session_id_generation() {
        let config = create_test_config();
        let state = ServerState::new(config);

        let id1 = state.next_session_id();
        let id2 = state.next_session_id();

        assert_ne!(id1, id2);
        assert!(id1.starts_with("session-"));
        assert!(id2.starts_with("session-"));
    }

    #[test]
    fn test_stream_id_generation() {
        let config = create_test_config();
        let state = ServerState::new(config);

        let id1 = state.next_stream_id();
        let id2 = state.next_stream_id();

        assert_ne!(id1, id2);
        assert!(id1 >= 1 && id1 <= 65535);
        assert!(id2 >= 1 && id2 <= 65535);
    }

    #[test]
    fn test_server_config_validation() {
        let mut config = create_test_config();
        assert!(config.validate().is_ok());

        // Test invalid max connections
        config.max_connections = 0;
        assert!(config.validate().is_err());

        config.max_connections = 1001;
        assert!(config.validate().is_err());

        config.max_connections = 100;
        assert!(config.validate().is_ok());

        // Test invalid timeout
        config.connection_timeout = Duration::from_secs(0);
        assert!(config.validate().is_err());

        config.connection_timeout = Duration::from_secs(30);
        assert!(config.validate().is_ok());

        // Test empty discovery ports
        config.tcp_discovery_ports.clear();
        assert!(config.validate().is_err());

        config.tcp_discovery_ports = vec![5555, 5556, 5555]; // Duplicate
        assert!(config.validate().is_err());

        config.tcp_discovery_ports = vec![5555, 5556, 5557];
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.max_connections, 100);
        assert!(!config.tcp_discovery_ports.is_empty());
    }

    #[test]
    fn test_server_stats() {
        let stats = ServerStats::new();

        // Test initial state
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_sessions, 0);
        assert_eq!(snapshot.active_sessions, 0);

        // Test session tracking
        stats.session_created();
        stats.session_created();
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_sessions, 2);
        assert_eq!(snapshot.active_sessions, 2);

        stats.session_closed();
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_sessions, 2);
        assert_eq!(snapshot.active_sessions, 1);

        // Test stream tracking
        stats.stream_created();
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_streams, 1);
        assert_eq!(snapshot.active_streams, 1);

        // Test message processing
        stats.message_processed(100);
        stats.message_processed(200);
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.messages_processed, 2);
        assert_eq!(snapshot.bytes_transferred, 300);
        assert_eq!(snapshot.avg_message_size(), 150.0);

        // Test error tracking
        stats.error_occurred();
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.errors_count, 1);
        assert_eq!(snapshot.error_rate(), 50.0); // 1 error out of 2 messages

        // Test reset
        stats.reset();
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_sessions, 0);
        assert_eq!(snapshot.messages_processed, 0);
    }

    #[test]
    fn test_server_state_validation() {
        let config = create_test_config();
        let state = ServerState::new(config);

        // Initial state should be valid
        assert!(state.validate().is_ok());

        // Test connection limit
        assert!(state.can_accept_connection());
    }

    #[test]
    fn test_transport_address_config() {
        // Test TCP address
        let tcp_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let config = ServerConfig::from_tcp_addr(tcp_addr);
        assert_eq!(config.bind_address_str(), "tcp://127.0.0.1:8080");

        // Test USB address (future compatibility)
        let usb_addr = TransportAddress::Usb {
            vendor_id: 0x1234,
            product_id: 0x5678,
            serial: Some("ABC123".to_string()),
        };
        let config = ServerConfig::new(usb_addr);
        assert_eq!(config.bind_address_str(), "usb://4660:22136:ABC123");
    }

    #[test]
    fn test_stats_snapshot() {
        let stats = ServerStats::new();
        stats.message_processed(100);
        stats.error_occurred();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.error_rate(), 100.0);
        assert_eq!(snapshot.avg_message_size(), 100.0);

        // Test with no messages
        let empty_stats = ServerStats::new();
        let empty_snapshot = empty_stats.snapshot();
        assert_eq!(empty_snapshot.error_rate(), 0.0);
        assert_eq!(empty_snapshot.avg_message_size(), 0.0);
    }
}