use async_trait::async_trait;
use crate::protocol::error::{ProtocolError, ProtocolResult};
use crate::protocol::message::AdbMessage;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Connection abstraction for different transport types
/// Provides async read/write operations with ADB message framing
#[async_trait]
pub trait Connection: Send + Sync {
    /// Send raw bytes over the connection
    async fn send_bytes(&mut self, data: &[u8]) -> ProtocolResult<()>;

    /// Receive raw bytes from the connection
    /// Returns None if connection is closed gracefully
    async fn receive_bytes(&mut self, buffer: &mut [u8]) -> ProtocolResult<Option<usize>>;

    /// Send a complete ADB message
    async fn send_message(&mut self, message: &AdbMessage) -> ProtocolResult<()> {
        let serialized = message.serialize();
        self.send_bytes(&serialized).await
    }

    /// Receive a complete ADB message
    /// Handles message framing and deserialization
    async fn receive_message(&mut self) -> ProtocolResult<Option<AdbMessage>>;

    /// Send bytes with timeout
    async fn send_bytes_timeout(&mut self, data: &[u8], timeout: Duration) -> ProtocolResult<()>;

    /// Receive bytes with timeout
    async fn receive_bytes_timeout(&mut self, buffer: &mut [u8], timeout: Duration) -> ProtocolResult<Option<usize>>;

    /// Close the connection gracefully
    async fn close(&mut self) -> ProtocolResult<()>;

    /// Forcefully shutdown the connection
    async fn shutdown(&mut self) -> ProtocolResult<()>;

    /// Check if the connection is still active
    fn is_connected(&self) -> bool;

    /// Check if the connection is closed
    fn is_closed(&self) -> bool {
        !self.is_connected()
    }

    /// Get local socket address
    fn local_addr(&self) -> ProtocolResult<SocketAddr>;

    /// Get remote socket address
    fn remote_addr(&self) -> ProtocolResult<SocketAddr>;

    /// Get connection statistics
    fn stats(&self) -> ConnectionStats;

    /// Reset connection statistics
    fn reset_stats(&mut self);

    /// Get connection metadata
    fn metadata(&self) -> ConnectionMetadata;

    /// Set connection options
    async fn set_options(&mut self, options: ConnectionOptions) -> ProtocolResult<()>;

    /// Get current connection options
    fn get_options(&self) -> ConnectionOptions;

    /// Ping the connection (send keep-alive)
    async fn ping(&mut self) -> ProtocolResult<Duration> {
        let start = Instant::now();
        let ping_msg = AdbMessage::new_ping();
        self.send_message(&ping_msg).await?;

        // Wait for PONG response
        if let Some(response) = self.receive_message().await? {
            if response.command == crate::protocol::commands::AdbCommand::PONG as u32 {
                return Ok(start.elapsed());
            }
        }

        Err(ProtocolError::Timeout { timeout_ms: 5000 })
    }

    /// Flush any pending write buffers
    async fn flush(&mut self) -> ProtocolResult<()>;

    /// Get the maximum message size for this connection
    fn max_message_size(&self) -> usize;

    /// Check if the connection supports a specific feature
    fn supports_feature(&self, feature: ConnectionFeature) -> bool;
}

/// Connection statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Connection established time
    pub connected_at: Option<Instant>,
    /// Last activity time
    pub last_activity: Option<Instant>,
    /// Number of errors
    pub error_count: u64,
    /// Average round-trip time for pings
    pub avg_rtt: Option<Duration>,
    /// Connection uptime
    pub uptime: Duration,
}

impl ConnectionStats {
    /// Create new connection stats
    pub fn new() -> Self {
        Self {
            connected_at: Some(Instant::now()),
            ..Default::default()
        }
    }

    /// Record bytes sent
    pub fn record_bytes_sent(&mut self, count: u64) {
        self.bytes_sent += count;
        self.last_activity = Some(Instant::now());
    }

    /// Record bytes received
    pub fn record_bytes_received(&mut self, count: u64) {
        self.bytes_received += count;
        self.last_activity = Some(Instant::now());
    }

    /// Record message sent
    pub fn record_message_sent(&mut self) {
        self.messages_sent += 1;
        self.last_activity = Some(Instant::now());
    }

    /// Record message received
    pub fn record_message_received(&mut self) {
        self.messages_received += 1;
        self.last_activity = Some(Instant::now());
    }

    /// Record an error
    pub fn record_error(&mut self) {
        self.error_count += 1;
    }

    /// Update RTT measurement
    pub fn update_rtt(&mut self, rtt: Duration) {
        self.avg_rtt = match self.avg_rtt {
            Some(avg) => Some(Duration::from_nanos((avg.as_nanos() + rtt.as_nanos()) as u64 / 2)),
            None => Some(rtt),
        };
    }

    /// Get current uptime
    pub fn current_uptime(&self) -> Duration {
        self.connected_at
            .map(|start| start.elapsed())
            .unwrap_or_default()
    }
}

/// Connection metadata
#[derive(Debug, Clone)]
pub struct ConnectionMetadata {
    /// Connection ID for tracking
    pub connection_id: String,
    /// Connection type (TCP, USB, etc.)
    pub connection_type: String,
    /// Protocol version negotiated
    pub protocol_version: Option<u32>,
    /// Maximum data size
    pub max_data_size: usize,
    /// Connection established timestamp
    pub established_at: Instant,
    /// Client identification string
    pub client_identity: Option<String>,
    /// Server identification string
    pub server_identity: Option<String>,
    /// Additional properties
    pub properties: std::collections::HashMap<String, String>,
}

impl ConnectionMetadata {
    /// Create new connection metadata
    pub fn new(connection_type: impl Into<String>) -> Self {
        Self {
            connection_id: uuid::Uuid::new_v4().to_string(),
            connection_type: connection_type.into(),
            protocol_version: None,
            max_data_size: 1024 * 1024, // 1MB default
            established_at: Instant::now(),
            client_identity: None,
            server_identity: None,
            properties: std::collections::HashMap::new(),
        }
    }

    /// Set protocol version
    pub fn set_protocol_version(&mut self, version: u32) {
        self.protocol_version = Some(version);
    }

    /// Set client identity
    pub fn set_client_identity(&mut self, identity: impl Into<String>) {
        self.client_identity = Some(identity.into());
    }

    /// Set server identity
    pub fn set_server_identity(&mut self, identity: impl Into<String>) {
        self.server_identity = Some(identity.into());
    }

    /// Add a custom property
    pub fn add_property(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.properties.insert(key.into(), value.into());
    }

    /// Get a custom property
    pub fn get_property(&self, key: &str) -> Option<&String> {
        self.properties.get(key)
    }
}

/// Connection configuration options
#[derive(Debug, Clone)]
pub struct ConnectionOptions {
    /// Read timeout
    pub read_timeout: Option<Duration>,
    /// Write timeout
    pub write_timeout: Option<Duration>,
    /// Keep-alive interval
    pub keepalive_interval: Option<Duration>,
    /// Enable TCP nodelay
    pub tcp_nodelay: bool,
    /// Buffer size for reading
    pub read_buffer_size: usize,
    /// Buffer size for writing
    pub write_buffer_size: usize,
    /// Maximum message size
    pub max_message_size: usize,
    /// Enable compression
    pub enable_compression: bool,
    /// Enable encryption
    pub enable_encryption: bool,
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            read_timeout: Some(Duration::from_secs(30)),
            write_timeout: Some(Duration::from_secs(30)),
            keepalive_interval: Some(Duration::from_secs(60)),
            tcp_nodelay: true,
            read_buffer_size: 64 * 1024,  // 64KB
            write_buffer_size: 64 * 1024, // 64KB
            max_message_size: 1024 * 1024, // 1MB
            enable_compression: false,
            enable_encryption: false,
        }
    }
}

/// Connection features that may be supported
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionFeature {
    /// Connection supports keep-alive pings
    Keepalive,
    /// Connection supports compression
    Compression,
    /// Connection supports encryption
    Encryption,
    /// Connection supports multiplexing
    Multiplexing,
    /// Connection supports backpressure
    Backpressure,
    /// Connection supports graceful shutdown
    GracefulShutdown,
}

/// Helper trait for ADB message creation
impl AdbMessage {
    /// Create a PING message for keep-alive
    pub fn new_ping() -> Self {
        let command = crate::protocol::commands::AdbCommand::PING as u32;
        Self {
            command,
            arg0: 0,
            arg1: 0,
            data_length: 0,
            data_crc32: 0,
            magic: !command,
            data: Vec::new(),
        }
    }

    /// Create a PONG response message
    pub fn new_pong() -> Self {
        let command = crate::protocol::commands::AdbCommand::PONG as u32;
        Self {
            command,
            arg0: 0,
            arg1: 0,
            data_length: 0,
            data_crc32: 0,
            magic: !command,
            data: Vec::new(),
        }
    }
}