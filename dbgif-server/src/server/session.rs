use dbgif_protocol::error::{ProtocolError, ProtocolResult};
use dbgif_protocol::message::AdbMessage;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Notify};
use uuid::Uuid;

/// Client session management for tracking connected clients and their state
#[derive(Debug)]
pub struct ClientSessionManager {
    /// Active client sessions
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
    /// Session configuration
    config: SessionConfig,
    /// Notification system for session changes
    state_notifier: Arc<Notify>,
}

/// Individual client session
#[derive(Debug)]
pub struct ClientSession {
    /// Unique session identifier
    pub session_id: String,
    /// Client connection information
    pub client_info: ClientInfo,
    /// Session state
    pub state: SessionState,
    /// Active streams in this session
    pub streams: HashMap<u32, StreamInfo>,
    /// Session statistics
    pub stats: SessionStats,
    /// Session configuration
    pub config: SessionConfig,
    /// Message queue for outgoing messages
    pub message_queue: mpsc::UnboundedSender<AdbMessage>,
    /// Session established timestamp
    pub established_at: Instant,
    /// Last activity timestamp
    pub last_activity: Instant,
}

/// Client session information (cloneable version without message queue)
#[derive(Debug, Clone)]
pub struct ClientSessionInfo {
    /// Unique session identifier
    pub session_id: String,
    /// Client connection information
    pub client_info: ClientInfo,
    /// Session state
    pub state: SessionState,
    /// Active streams in this session
    pub streams: HashMap<u32, StreamInfo>,
    /// Session statistics
    pub stats: SessionStats,
    /// Session established timestamp
    pub established_at: Instant,
    /// Last activity timestamp
    pub last_activity: Instant,
}

/// Client connection information
#[derive(Debug, Clone)]
pub struct ClientInfo {
    /// Connection identifier (e.g., "tcp://127.0.0.1:12345")
    pub connection_id: String,
    /// Client identification string
    pub identity: Option<String>,
    /// Protocol version negotiated
    pub protocol_version: u32,
    /// Maximum data size
    pub max_data_size: usize,
    /// Client capabilities
    pub capabilities: ClientCapabilities,
}

/// Session state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Session is being established
    Connecting,
    /// Session is active and ready
    Active,
    /// Session is being terminated
    Disconnecting,
    /// Session has been terminated
    Disconnected,
    /// Session encountered an error
    Error { message: String },
}

/// Stream information within a session
#[derive(Debug, Clone)]
pub struct StreamInfo {
    /// Stream identifier (unique within session)
    pub stream_id: u32,
    /// Remote stream identifier
    pub remote_stream_id: u32,
    /// Service name for this stream
    pub service: String,
    /// Stream state
    pub state: StreamState,
    /// Stream statistics
    pub stats: StreamStats,
    /// Stream established timestamp
    pub established_at: Instant,
    /// Last activity timestamp
    pub last_activity: Instant,
}

/// Stream state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamState {
    /// Stream is being opened
    Opening,
    /// Stream is active
    Active,
    /// Stream is being closed
    Closing,
    /// Stream is closed
    Closed,
    /// Stream encountered an error
    Error { message: String },
}

/// Client capabilities
#[derive(Debug, Clone, Default)]
pub struct ClientCapabilities {
    /// Client supports shell commands
    pub shell_support: bool,
    /// Client supports file transfer
    pub file_transfer_support: bool,
    /// Client supports port forwarding
    pub port_forwarding_support: bool,
    /// Client supports multiplexing
    pub multiplexing_support: bool,
    /// Additional features
    pub features: Vec<String>,
}

/// Session statistics
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Number of streams created
    pub streams_created: u64,
    /// Number of errors
    pub error_count: u64,
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
}

/// Session configuration
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Maximum number of concurrent sessions
    pub max_sessions: usize,
    /// Maximum number of streams per session
    pub max_streams_per_session: usize,
    /// Session timeout duration
    pub session_timeout: Duration,
    /// Stream timeout duration
    pub stream_timeout: Duration,
    /// Keep-alive interval
    pub keepalive_interval: Duration,
    /// Enable session monitoring
    pub enable_monitoring: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_sessions: 100,
            max_streams_per_session: 64,
            session_timeout: Duration::from_secs(300), // 5 minutes
            stream_timeout: Duration::from_secs(60),   // 1 minute
            keepalive_interval: Duration::from_secs(30),
            enable_monitoring: true,
        }
    }
}

impl ClientSessionManager {
    /// Create a new session manager with default configuration
    pub fn new() -> Self {
        Self::with_config(SessionConfig::default())
    }

    /// Create a new session manager with custom configuration
    pub fn with_config(config: SessionConfig) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
            state_notifier: Arc::new(Notify::new()),
        }
    }

    /// Create a new client session
    pub fn create_session(&self, client_info: ClientInfo) -> ProtocolResult<String> {
        let mut sessions = self.sessions.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on sessions".to_string(),
            }
        })?;

        // Check session limit
        if sessions.len() >= self.config.max_sessions {
            return Err(ProtocolError::MaxConnectionsExceeded {
                count: sessions.len(),
                max: self.config.max_sessions,
            });
        }

        let session_id = Uuid::new_v4().to_string();
        let (tx, _rx) = mpsc::unbounded_channel();

        let session = ClientSession {
            session_id: session_id.clone(),
            client_info,
            state: SessionState::Connecting,
            streams: HashMap::new(),
            stats: SessionStats::default(),
            config: self.config.clone(),
            message_queue: tx,
            established_at: Instant::now(),
            last_activity: Instant::now(),
        };

        sessions.insert(session_id.clone(), session);

        // Notify listeners
        self.state_notifier.notify_waiters();

        Ok(session_id)
    }

    /// Remove a client session
    pub fn remove_session(&self, session_id: &str) -> ProtocolResult<Option<ClientSession>> {
        let mut sessions = self.sessions.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on sessions".to_string(),
            }
        })?;

        let removed = sessions.remove(session_id);

        if removed.is_some() {
            // Notify listeners
            self.state_notifier.notify_waiters();
        }

        Ok(removed)
    }

    /// Get session by ID (returns session info without the message queue)
    pub fn get_session_info(&self, session_id: &str) -> ProtocolResult<Option<ClientSessionInfo>> {
        let sessions = self.sessions.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on sessions".to_string(),
            }
        })?;

        if let Some(session) = sessions.get(session_id) {
            Ok(Some(ClientSessionInfo {
                session_id: session.session_id.clone(),
                client_info: session.client_info.clone(),
                state: session.state.clone(),
                streams: session.streams.clone(),
                stats: session.stats.clone(),
                established_at: session.established_at,
                last_activity: session.last_activity,
            }))
        } else {
            Ok(None)
        }
    }

    /// Update session state
    pub fn update_session_state(&self, session_id: &str, new_state: SessionState) -> ProtocolResult<()> {
        let mut sessions = self.sessions.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on sessions".to_string(),
            }
        })?;

        if let Some(session) = sessions.get_mut(session_id) {
            session.state = new_state;
            session.last_activity = Instant::now();

            // Notify listeners
            self.state_notifier.notify_waiters();

            Ok(())
        } else {
            Err(ProtocolError::InternalError {
                message: format!("Session {} not found", session_id),
            })
        }
    }

    /// Create a new stream in a session
    pub fn create_stream(&self, session_id: &str, stream_id: u32, remote_stream_id: u32, service: String) -> ProtocolResult<()> {
        let mut sessions = self.sessions.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on sessions".to_string(),
            }
        })?;

        if let Some(session) = sessions.get_mut(session_id) {
            // Check stream limit
            if session.streams.len() >= self.config.max_streams_per_session {
                return Err(ProtocolError::MaxStreamsExceeded {
                    count: session.streams.len(),
                    max: self.config.max_streams_per_session,
                });
            }

            // Check for duplicate stream ID
            if session.streams.contains_key(&stream_id) {
                return Err(ProtocolError::StreamAlreadyExists { local_id: stream_id });
            }

            let stream_info = StreamInfo {
                stream_id,
                remote_stream_id,
                service,
                state: StreamState::Opening,
                stats: StreamStats::default(),
                established_at: Instant::now(),
                last_activity: Instant::now(),
            };

            session.streams.insert(stream_id, stream_info);
            session.stats.streams_created += 1;
            session.last_activity = Instant::now();

            // Notify listeners
            self.state_notifier.notify_waiters();

            Ok(())
        } else {
            Err(ProtocolError::InternalError {
                message: format!("Session {} not found", session_id),
            })
        }
    }

    /// Remove a stream from a session
    pub fn remove_stream(&self, session_id: &str, stream_id: u32) -> ProtocolResult<Option<StreamInfo>> {
        let mut sessions = self.sessions.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on sessions".to_string(),
            }
        })?;

        if let Some(session) = sessions.get_mut(session_id) {
            let removed = session.streams.remove(&stream_id);

            if removed.is_some() {
                session.last_activity = Instant::now();
                // Notify listeners
                self.state_notifier.notify_waiters();
            }

            Ok(removed)
        } else {
            Err(ProtocolError::InternalError {
                message: format!("Session {} not found", session_id),
            })
        }
    }

    /// Update stream state
    pub fn update_stream_state(&self, session_id: &str, stream_id: u32, new_state: StreamState) -> ProtocolResult<()> {
        let mut sessions = self.sessions.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on sessions".to_string(),
            }
        })?;

        if let Some(session) = sessions.get_mut(session_id) {
            if let Some(stream) = session.streams.get_mut(&stream_id) {
                stream.state = new_state;
                stream.last_activity = Instant::now();
                session.last_activity = Instant::now();

                // Notify listeners
                self.state_notifier.notify_waiters();

                Ok(())
            } else {
                Err(ProtocolError::StreamNotFound {
                    local_id: stream_id,
                    remote_id: 0,
                })
            }
        } else {
            Err(ProtocolError::InternalError {
                message: format!("Session {} not found", session_id),
            })
        }
    }

    /// Record session activity
    pub fn record_activity(&self, session_id: &str, messages_sent: u64, messages_received: u64, bytes_sent: u64, bytes_received: u64) -> ProtocolResult<()> {
        let mut sessions = self.sessions.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on sessions".to_string(),
            }
        })?;

        if let Some(session) = sessions.get_mut(session_id) {
            session.stats.messages_sent += messages_sent;
            session.stats.messages_received += messages_received;
            session.stats.bytes_sent += bytes_sent;
            session.stats.bytes_received += bytes_received;
            session.last_activity = Instant::now();

            Ok(())
        } else {
            Err(ProtocolError::InternalError {
                message: format!("Session {} not found", session_id),
            })
        }
    }

    /// List all active sessions
    pub fn list_sessions(&self) -> ProtocolResult<Vec<ClientSessionInfo>> {
        let sessions = self.sessions.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on sessions".to_string(),
            }
        })?;

        let session_infos = sessions.values().map(|session| {
            ClientSessionInfo {
                session_id: session.session_id.clone(),
                client_info: session.client_info.clone(),
                state: session.state.clone(),
                streams: session.streams.clone(),
                stats: session.stats.clone(),
                established_at: session.established_at,
                last_activity: session.last_activity,
            }
        }).collect();

        Ok(session_infos)
    }

    /// Get session statistics
    pub fn get_session_stats(&self) -> ProtocolResult<SessionManagerStats> {
        let sessions = self.sessions.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on sessions".to_string(),
            }
        })?;

        let mut stats = SessionManagerStats::default();
        stats.total_sessions = sessions.len();

        for session in sessions.values() {
            match session.state {
                SessionState::Active => stats.active_sessions += 1,
                SessionState::Connecting => stats.connecting_sessions += 1,
                SessionState::Disconnecting => stats.disconnecting_sessions += 1,
                SessionState::Error { .. } => stats.error_sessions += 1,
                _ => {}
            }
            stats.total_streams += session.streams.len();
        }

        Ok(stats)
    }

    /// Wait for session state changes
    pub async fn wait_for_changes(&self) {
        self.state_notifier.notified().await;
    }

    /// Clean up inactive sessions
    pub fn cleanup_inactive_sessions(&self) -> ProtocolResult<usize> {
        let mut sessions = self.sessions.write().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire write lock on sessions".to_string(),
            }
        })?;

        let cutoff_time = Instant::now() - self.config.session_timeout;
        let mut removed_count = 0;

        sessions.retain(|_, session| {
            let should_retain = session.last_activity > cutoff_time || session.state == SessionState::Active;

            if !should_retain {
                removed_count += 1;
            }

            should_retain
        });

        if removed_count > 0 {
            // Notify listeners
            self.state_notifier.notify_waiters();
        }

        Ok(removed_count)
    }

    /// Get session count
    pub fn session_count(&self) -> ProtocolResult<usize> {
        let sessions = self.sessions.read().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to acquire read lock on sessions".to_string(),
            }
        })?;

        Ok(sessions.len())
    }
}

impl Default for ClientSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Session manager statistics
#[derive(Debug, Clone, Default)]
pub struct SessionManagerStats {
    /// Total number of sessions
    pub total_sessions: usize,
    /// Number of active sessions
    pub active_sessions: usize,
    /// Number of connecting sessions
    pub connecting_sessions: usize,
    /// Number of disconnecting sessions
    pub disconnecting_sessions: usize,
    /// Number of sessions in error state
    pub error_sessions: usize,
    /// Total number of streams across all sessions
    pub total_streams: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_client_info() -> ClientInfo {
        ClientInfo {
            connection_id: "tcp://127.0.0.1:12345→127.0.0.1:5555".to_string(),
            identity: Some("test-client".to_string()),
            protocol_version: 0x01000000,
            max_data_size: 1024 * 1024,
            capabilities: ClientCapabilities::default(),
        }
    }

    #[test]
    fn test_session_manager_creation() {
        let manager = ClientSessionManager::new();
        assert_eq!(manager.session_count().unwrap(), 0);
    }

    #[test]
    fn test_session_creation() {
        let manager = ClientSessionManager::new();
        let client_info = create_test_client_info();

        let session_id = manager.create_session(client_info).unwrap();
        assert_eq!(manager.session_count().unwrap(), 1);

        let session = manager.get_session_info(&session_id).unwrap().unwrap();
        assert_eq!(session.state, SessionState::Connecting);
    }

    #[test]
    fn test_session_removal() {
        let manager = ClientSessionManager::new();
        let client_info = create_test_client_info();

        let session_id = manager.create_session(client_info).unwrap();
        let removed = manager.remove_session(&session_id).unwrap();

        assert!(removed.is_some());
        assert_eq!(manager.session_count().unwrap(), 0);
    }

    #[test]
    fn test_stream_management() {
        let manager = ClientSessionManager::new();
        let client_info = create_test_client_info();

        let session_id = manager.create_session(client_info).unwrap();

        // Create stream
        manager.create_stream(&session_id, 1, 2, "shell".to_string()).unwrap();

        let session = manager.get_session_info(&session_id).unwrap().unwrap();
        assert_eq!(session.streams.len(), 1);
        assert!(session.streams.contains_key(&1));

        // Update stream state
        manager.update_stream_state(&session_id, 1, StreamState::Active).unwrap();

        let session = manager.get_session_info(&session_id).unwrap().unwrap();
        assert_eq!(session.streams[&1].state, StreamState::Active);

        // Remove stream
        let removed = manager.remove_stream(&session_id, 1).unwrap();
        assert!(removed.is_some());

        let session = manager.get_session_info(&session_id).unwrap().unwrap();
        assert_eq!(session.streams.len(), 0);
    }

    #[test]
    fn test_session_limits() {
        let config = SessionConfig {
            max_sessions: 1,
            ..Default::default()
        };
        let manager = ClientSessionManager::with_config(config);

        // First session should succeed
        let client_info1 = create_test_client_info();
        assert!(manager.create_session(client_info1).is_ok());

        // Second session should fail
        let client_info2 = create_test_client_info();
        assert!(manager.create_session(client_info2).is_err());
    }
}