use crate::protocol::error::{ProtocolError, ProtocolResult};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// Stream manager for handling stream mapping and lifecycle
#[derive(Debug)]
pub struct StreamManager {
    /// Active streams mapped by session ID and stream ID
    streams: Arc<RwLock<HashMap<String, HashMap<u32, StreamEntry>>>>,
    /// Stream configuration
    config: StreamConfig,
    /// Notification system for stream changes
    state_notifier: Arc<Notify>,
    /// Next available stream ID per session
    next_stream_ids: Arc<RwLock<HashMap<String, u32>>>,
}

/// Stream entry with complete information
#[derive(Debug, Clone)]
pub struct StreamEntry {
    /// Stream information
    pub info: StreamInfo,
    /// Stream handler for processing messages
    pub handler: Option<StreamHandler>,
    /// Stream buffer for pending data
    pub buffer: StreamBuffer,
    /// Last activity timestamp
    pub last_activity: Instant,
}

/// Stream information and metadata
#[derive(Debug, Clone)]
pub struct StreamInfo {
    /// Session identifier this stream belongs to
    pub session_id: String,
    /// Local stream identifier (unique within session)
    pub local_stream_id: u32,
    /// Remote stream identifier
    pub remote_stream_id: u32,
    /// Service name for this stream
    pub service_name: String,
    /// Stream state
    pub state: StreamState,
    /// Stream type
    pub stream_type: StreamType,
    /// Stream statistics
    pub stats: StreamStats,
    /// Stream metadata
    pub metadata: StreamMetadata,
    /// Stream creation timestamp
    pub created_at: Instant,
    /// Stream last modification timestamp
    pub modified_at: Instant,
}

/// Stream state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamState {
    /// Stream is being opened (OPEN sent, waiting for OKAY)
    Opening,
    /// Stream is active and ready for data transfer
    Active,
    /// Stream is being closed (CLSE sent, waiting for acknowledgment)
    Closing,
    /// Stream is closed
    Closed,
    /// Stream encountered an error
    Error { message: String, code: u32 },
}

/// Stream type based on service
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamType {
    /// Shell command execution
    Shell,
    /// File transfer operation
    FileTransfer,
    /// Port forwarding
    PortForward { local_port: u16, remote_port: u16 },
    /// Host service (version, list, etc.)
    HostService,
    /// Raw data stream
    Raw,
    /// Custom service type
    Custom(String),
}

/// Stream statistics
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    /// Messages sent on this stream
    pub messages_sent: u64,
    /// Messages received on this stream
    pub messages_received: u64,
    /// Bytes sent on this stream
    pub bytes_sent: u64,
    /// Bytes received on this stream
    pub bytes_received: u64,
    /// Number of errors encountered
    pub error_count: u64,
    /// Average message size sent
    pub avg_message_size_sent: f64,
    /// Average message size received
    pub avg_message_size_received: f64,
}

/// Stream metadata
#[derive(Debug, Clone, Default)]
pub struct StreamMetadata {
    /// Stream priority (0-255, higher is more priority)
    pub priority: u8,
    /// Stream description
    pub description: Option<String>,
    /// Additional properties
    pub properties: HashMap<String, String>,
    /// Stream timeout override
    pub timeout_override: Option<Duration>,
}

/// Stream message handler
#[derive(Debug, Clone)]
pub struct StreamHandler {
    /// Handler type
    pub handler_type: StreamHandlerType,
    /// Handler configuration
    pub config: StreamHandlerConfig,
}

/// Types of stream handlers
#[derive(Debug, Clone)]
pub enum StreamHandlerType {
    /// Echo handler (for testing)
    Echo,
    /// Shell command handler
    Shell { command: String },
    /// File transfer handler
    FileTransfer { operation: FileOperation },
    /// Port forwarding handler
    PortForward { target_address: String },
    /// Custom handler
    Custom { handler_name: String },
}

/// File operations for file transfer streams
#[derive(Debug, Clone)]
pub enum FileOperation {
    /// Upload file to device
    Upload { local_path: String, remote_path: String },
    /// Download file from device
    Download { remote_path: String, local_path: String },
    /// List directory contents
    List { path: String },
    /// Delete file or directory
    Delete { path: String },
}

/// Stream handler configuration
#[derive(Debug, Clone, Default)]
pub struct StreamHandlerConfig {
    /// Buffer size for the handler
    pub buffer_size: usize,
    /// Timeout for handler operations
    pub timeout: Duration,
    /// Enable handler logging
    pub enable_logging: bool,
    /// Custom parameters
    pub parameters: HashMap<String, String>,
}

/// Stream data buffer
#[derive(Debug, Clone)]
pub struct StreamBuffer {
    /// Incoming data buffer
    pub incoming: Vec<u8>,
    /// Outgoing data buffer
    pub outgoing: Vec<u8>,
    /// Buffer capacity limits
    pub capacity: StreamBufferCapacity,
    /// Buffer statistics
    pub stats: StreamBufferStats,
}

/// Stream buffer capacity configuration
#[derive(Debug, Clone)]
pub struct StreamBufferCapacity {
    /// Maximum incoming buffer size
    pub max_incoming: usize,
    /// Maximum outgoing buffer size
    pub max_outgoing: usize,
    /// Buffer overflow behavior
    pub overflow_behavior: BufferOverflowBehavior,
}

/// Buffer overflow behavior
#[derive(Debug, Clone)]
pub enum BufferOverflowBehavior {
    /// Drop oldest data
    DropOldest,
    /// Drop newest data
    DropNewest,
    /// Return error
    Error,
    /// Block until space available
    Block,
}

/// Stream buffer statistics
#[derive(Debug, Clone, Default)]
pub struct StreamBufferStats {
    /// Total bytes written to incoming buffer
    pub incoming_bytes_written: u64,
    /// Total bytes read from incoming buffer
    pub incoming_bytes_read: u64,
    /// Total bytes written to outgoing buffer
    pub outgoing_bytes_written: u64,
    /// Total bytes read from outgoing buffer
    pub outgoing_bytes_read: u64,
    /// Number of buffer overflows
    pub overflow_count: u64,
}

/// Stream configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Maximum number of streams per session
    pub max_streams_per_session: usize,
    /// Default stream timeout
    pub default_timeout: Duration,
    /// Default buffer size
    pub default_buffer_size: usize,
    /// Enable stream monitoring
    pub enable_monitoring: bool,
    /// Auto-cleanup inactive streams
    pub auto_cleanup: bool,
    /// Cleanup interval
    pub cleanup_interval: Duration,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_streams_per_session: 64,
            default_timeout: Duration::from_secs(60),
            default_buffer_size: 64 * 1024, // 64KB
            enable_monitoring: true,
            auto_cleanup: true,
            cleanup_interval: Duration::from_secs(30),
        }
    }
}

impl StreamManager {
    /// Create a new stream manager with default configuration
    pub fn new() -> Self {
        Self::with_config(StreamConfig::default())
    }

    /// Create a new stream manager with custom configuration
    pub fn with_config(config: StreamConfig) -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
            config,
            state_notifier: Arc::new(Notify::new()),
            next_stream_ids: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new stream
    pub fn create_stream(&self, session_id: &str, remote_stream_id: u32, service_name: String) -> ProtocolResult<u32> {
        let mut streams = self.streams.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on streams".to_string(),
            }
        })?;

        let mut next_ids = self.next_stream_ids.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on stream IDs".to_string(),
            }
        })?;

        // Get or create session streams map
        let session_streams = streams.entry(session_id.to_string()).or_insert_with(HashMap::new);

        // Check stream limit
        if session_streams.len() >= self.config.max_streams_per_session {
            return Err(ProtocolError::MaxStreamsExceeded {
                count: session_streams.len(),
                max: self.config.max_streams_per_session,
            });
        }

        // Get next available stream ID for this session
        let next_id = next_ids.entry(session_id.to_string()).or_insert(1);
        let local_stream_id = *next_id;
        *next_id += 1;

        // Determine stream type from service name
        let stream_type = StreamType::from_service_name(&service_name);

        let stream_info = StreamInfo {
            session_id: session_id.to_string(),
            local_stream_id,
            remote_stream_id,
            service_name,
            state: StreamState::Opening,
            stream_type,
            stats: StreamStats::default(),
            metadata: StreamMetadata::default(),
            created_at: Instant::now(),
            modified_at: Instant::now(),
        };

        let stream_entry = StreamEntry {
            info: stream_info,
            handler: None,
            buffer: StreamBuffer::new(self.config.default_buffer_size),
            last_activity: Instant::now(),
        };

        session_streams.insert(local_stream_id, stream_entry);

        // Notify listeners
        self.state_notifier.notify_waiters();

        Ok(local_stream_id)
    }

    /// Remove a stream
    pub fn remove_stream(&self, session_id: &str, local_stream_id: u32) -> ProtocolResult<Option<StreamEntry>> {
        let mut streams = self.streams.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on streams".to_string(),
            }
        })?;

        if let Some(session_streams) = streams.get_mut(session_id) {
            let removed = session_streams.remove(&local_stream_id);

            // Clean up empty session
            if session_streams.is_empty() {
                streams.remove(session_id);
            }

            if removed.is_some() {
                // Notify listeners
                self.state_notifier.notify_waiters();
            }

            Ok(removed)
        } else {
            Ok(None)
        }
    }

    /// Get stream by session and stream ID
    pub fn get_stream(&self, session_id: &str, local_stream_id: u32) -> ProtocolResult<Option<StreamEntry>> {
        let streams = self.streams.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on streams".to_string(),
            }
        })?;

        if let Some(session_streams) = streams.get(session_id) {
            Ok(session_streams.get(&local_stream_id).cloned())
        } else {
            Ok(None)
        }
    }

    /// Update stream state
    pub fn update_stream_state(&self, session_id: &str, local_stream_id: u32, new_state: StreamState) -> ProtocolResult<()> {
        let mut streams = self.streams.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on streams".to_string(),
            }
        })?;

        if let Some(session_streams) = streams.get_mut(session_id) {
            if let Some(stream_entry) = session_streams.get_mut(&local_stream_id) {
                stream_entry.info.state = new_state;
                stream_entry.info.modified_at = Instant::now();
                stream_entry.last_activity = Instant::now();

                // Notify listeners
                self.state_notifier.notify_waiters();

                Ok(())
            } else {
                Err(ProtocolError::StreamNotFound {
                    local_id: local_stream_id,
                    remote_id: 0,
                })
            }
        } else {
            Err(ProtocolError::StreamNotFound {
                local_id: local_stream_id,
                remote_id: 0,
            })
        }
    }

    /// List streams for a session
    pub fn list_session_streams(&self, session_id: &str) -> ProtocolResult<Vec<StreamEntry>> {
        let streams = self.streams.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on streams".to_string(),
            }
        })?;

        if let Some(session_streams) = streams.get(session_id) {
            Ok(session_streams.values().cloned().collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// List all streams
    pub fn list_all_streams(&self) -> ProtocolResult<Vec<StreamEntry>> {
        let streams = self.streams.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on streams".to_string(),
            }
        })?;

        let mut all_streams = Vec::new();
        for session_streams in streams.values() {
            all_streams.extend(session_streams.values().cloned());
        }

        Ok(all_streams)
    }

    /// Record stream activity
    pub fn record_activity(&self, session_id: &str, local_stream_id: u32, bytes_sent: u64, bytes_received: u64) -> ProtocolResult<()> {
        let mut streams = self.streams.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on streams".to_string(),
            }
        })?;

        if let Some(session_streams) = streams.get_mut(session_id) {
            if let Some(stream_entry) = session_streams.get_mut(&local_stream_id) {
                stream_entry.info.stats.bytes_sent += bytes_sent;
                stream_entry.info.stats.bytes_received += bytes_received;

                if bytes_sent > 0 {
                    stream_entry.info.stats.messages_sent += 1;
                }
                if bytes_received > 0 {
                    stream_entry.info.stats.messages_received += 1;
                }

                stream_entry.last_activity = Instant::now();
                stream_entry.info.modified_at = Instant::now();

                Ok(())
            } else {
                Err(ProtocolError::StreamNotFound {
                    local_id: local_stream_id,
                    remote_id: 0,
                })
            }
        } else {
            Err(ProtocolError::StreamNotFound {
                local_id: local_stream_id,
                remote_id: 0,
            })
        }
    }

    /// Get stream statistics
    pub fn get_stream_stats(&self) -> ProtocolResult<StreamManagerStats> {
        let streams = self.streams.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on streams".to_string(),
            }
        })?;

        let mut stats = StreamManagerStats::default();
        stats.total_sessions = streams.len();

        for session_streams in streams.values() {
            stats.total_streams += session_streams.len();

            for stream_entry in session_streams.values() {
                match stream_entry.info.state {
                    StreamState::Active => stats.active_streams += 1,
                    StreamState::Opening => stats.opening_streams += 1,
                    StreamState::Closing => stats.closing_streams += 1,
                    StreamState::Closed => stats.closed_streams += 1,
                    StreamState::Error { .. } => stats.error_streams += 1,
                }
            }
        }

        Ok(stats)
    }

    /// Wait for stream state changes
    pub async fn wait_for_changes(&self) {
        self.state_notifier.notified().await;
    }

    /// Clean up inactive streams
    pub fn cleanup_inactive_streams(&self) -> ProtocolResult<usize> {
        if !self.config.auto_cleanup {
            return Ok(0);
        }

        let mut streams = self.streams.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on streams".to_string(),
            }
        })?;

        let cutoff_time = Instant::now() - self.config.default_timeout;
        let mut removed_count = 0;

        streams.retain(|_, session_streams| {
            session_streams.retain(|_, stream_entry| {
                let should_retain = stream_entry.last_activity > cutoff_time ||
                                  matches!(stream_entry.info.state, StreamState::Active | StreamState::Opening);

                if !should_retain {
                    removed_count += 1;
                }

                should_retain
            });

            !session_streams.is_empty()
        });

        if removed_count > 0 {
            // Notify listeners
            self.state_notifier.notify_waiters();
        }

        Ok(removed_count)
    }
}

impl Default for StreamManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamType {
    /// Determine stream type from service name
    pub fn from_service_name(service_name: &str) -> Self {
        match service_name {
            s if s.starts_with("shell") => StreamType::Shell,
            s if s.starts_with("host:") => StreamType::HostService,
            s if s.starts_with("tcp:") => {
                // Parse port forwarding: tcp:local_port:remote_port
                if let Some(parts) = s.strip_prefix("tcp:") {
                    let ports: Vec<&str> = parts.split(':').collect();
                    if ports.len() == 2 {
                        if let (Ok(local), Ok(remote)) = (ports[0].parse(), ports[1].parse()) {
                            return StreamType::PortForward { local_port: local, remote_port: remote };
                        }
                    }
                }
                StreamType::Custom(service_name.to_string())
            }
            s if s.starts_with("sync:") => StreamType::FileTransfer,
            _ => StreamType::Custom(service_name.to_string()),
        }
    }
}

impl StreamBuffer {
    /// Create a new stream buffer with specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            incoming: Vec::new(),
            outgoing: Vec::new(),
            capacity: StreamBufferCapacity {
                max_incoming: capacity,
                max_outgoing: capacity,
                overflow_behavior: BufferOverflowBehavior::DropOldest,
            },
            stats: StreamBufferStats::default(),
        }
    }

    /// Write data to incoming buffer
    pub fn write_incoming(&mut self, data: &[u8]) -> ProtocolResult<usize> {
        let available_space = self.capacity.max_incoming.saturating_sub(self.incoming.len());

        if data.len() > available_space {
            match self.capacity.overflow_behavior {
                BufferOverflowBehavior::DropOldest => {
                    let bytes_to_drop = data.len() - available_space;
                    self.incoming.drain(0..bytes_to_drop);
                    self.stats.overflow_count += 1;
                }
                BufferOverflowBehavior::DropNewest => {
                    let bytes_to_write = available_space;
                    self.incoming.extend_from_slice(&data[..bytes_to_write]);
                    self.stats.incoming_bytes_written += bytes_to_write as u64;
                    self.stats.overflow_count += 1;
                    return Ok(bytes_to_write);
                }
                BufferOverflowBehavior::Error => {
                    return Err(ProtocolError::StreamBufferOverflow {
                        size: self.incoming.len() + data.len(),
                        capacity: self.capacity.max_incoming,
                    });
                }
                BufferOverflowBehavior::Block => {
                    // For now, just drop oldest data
                    let bytes_to_drop = data.len() - available_space;
                    self.incoming.drain(0..bytes_to_drop);
                    self.stats.overflow_count += 1;
                }
            }
        }

        self.incoming.extend_from_slice(data);
        self.stats.incoming_bytes_written += data.len() as u64;
        Ok(data.len())
    }

    /// Read data from incoming buffer
    pub fn read_incoming(&mut self, buffer: &mut [u8]) -> usize {
        let bytes_to_read = buffer.len().min(self.incoming.len());
        buffer[..bytes_to_read].copy_from_slice(&self.incoming[..bytes_to_read]);
        self.incoming.drain(0..bytes_to_read);
        self.stats.incoming_bytes_read += bytes_to_read as u64;
        bytes_to_read
    }
}

/// Stream manager statistics
#[derive(Debug, Clone, Default)]
pub struct StreamManagerStats {
    /// Total number of sessions with streams
    pub total_sessions: usize,
    /// Total number of streams across all sessions
    pub total_streams: usize,
    /// Number of active streams
    pub active_streams: usize,
    /// Number of opening streams
    pub opening_streams: usize,
    /// Number of closing streams
    pub closing_streams: usize,
    /// Number of closed streams
    pub closed_streams: usize,
    /// Number of streams in error state
    pub error_streams: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_manager_creation() {
        let manager = StreamManager::new();
        let stats = manager.get_stream_stats().unwrap();
        assert_eq!(stats.total_streams, 0);
    }

    #[test]
    fn test_stream_creation() {
        let manager = StreamManager::new();
        let stream_id = manager.create_stream("session1", 1, "shell".to_string()).unwrap();
        assert_eq!(stream_id, 1);

        let stream = manager.get_stream("session1", stream_id).unwrap().unwrap();
        assert_eq!(stream.info.state, StreamState::Opening);
        assert_eq!(stream.info.service_name, "shell");
    }

    #[test]
    fn test_stream_type_detection() {
        assert_eq!(StreamType::from_service_name("shell"), StreamType::Shell);
        assert_eq!(StreamType::from_service_name("host:version"), StreamType::HostService);

        if let StreamType::PortForward { local_port, remote_port } = StreamType::from_service_name("tcp:8080:80") {
            assert_eq!(local_port, 8080);
            assert_eq!(remote_port, 80);
        } else {
            panic!("Expected PortForward type");
        }
    }

    #[test]
    fn test_stream_state_updates() {
        let manager = StreamManager::new();
        let stream_id = manager.create_stream("session1", 1, "shell".to_string()).unwrap();

        manager.update_stream_state("session1", stream_id, StreamState::Active).unwrap();
        let stream = manager.get_stream("session1", stream_id).unwrap().unwrap();
        assert_eq!(stream.info.state, StreamState::Active);
    }

    #[test]
    fn test_stream_removal() {
        let manager = StreamManager::new();
        let stream_id = manager.create_stream("session1", 1, "shell".to_string()).unwrap();

        let removed = manager.remove_stream("session1", stream_id).unwrap();
        assert!(removed.is_some());

        let stream = manager.get_stream("session1", stream_id).unwrap();
        assert!(stream.is_none());
    }

    #[test]
    fn test_stream_buffer() {
        let mut buffer = StreamBuffer::new(1024);

        let data = b"test data";
        let written = buffer.write_incoming(data).unwrap();
        assert_eq!(written, data.len());

        let mut read_buffer = vec![0u8; 1024];
        let read_bytes = buffer.read_incoming(&mut read_buffer);
        assert_eq!(read_bytes, data.len());
        assert_eq!(&read_buffer[..read_bytes], data);
    }
}