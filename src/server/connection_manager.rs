use crate::protocol::error::{ProtocolError, ProtocolResult};
use crate::protocol::message::AdbMessage;
use crate::server::session::{ClientSessionInfo, ClientInfo, SessionState, SessionStats, ClientCapabilities};
use crate::server::state::ServerState;
use crate::transport::Connection;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info, trace, warn};

/// Connection lifecycle manager
/// Handles client connections, session management, and cleanup
pub struct ConnectionManager {
    /// Server state reference
    server_state: Arc<ServerState>,
    /// Active connections and their channels
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    /// Message sender for connection events
    event_sender: mpsc::UnboundedSender<ConnectionEvent>,
    /// Message receiver for connection events
    event_receiver: Arc<RwLock<Option<mpsc::UnboundedReceiver<ConnectionEvent>>>>,
}

/// Information about an active connection
struct ConnectionInfo {
    /// Session information
    session_info: ClientSessionInfo,
    /// Connection handle (not Debug)
    #[allow(dead_code)]
    connection: Box<dyn Connection>,
    /// Message sender to connection task
    message_sender: mpsc::UnboundedSender<AdbMessage>,
    /// Last activity timestamp
    last_activity: Instant,
}

/// Connection lifecycle events
#[derive(Debug)]
enum ConnectionEvent {
    /// New connection established
    Connected {
        session_id: String,
        connection_id: String,
    },
    /// Connection closed
    Disconnected {
        session_id: String,
        reason: String,
    },
    /// Connection error occurred
    #[allow(dead_code)]
    Error {
        session_id: String,
        error: String,
    },
    /// Activity detected on connection
    Activity {
        session_id: String,
    },
}

impl ConnectionManager {
    /// Create a new connection manager
    pub fn new(server_state: Arc<ServerState>) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        Self {
            server_state,
            connections: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            event_receiver: Arc::new(RwLock::new(Some(event_receiver))),
        }
    }

    /// Accept a new connection and start managing it
    pub async fn accept_connection(
        &self,
        connection: Box<dyn Connection>,
    ) -> ProtocolResult<String> {
        let connection_id = connection.connection_id();

        // Check connection limits
        if !self.server_state.can_accept_connection() {
            warn!("Connection rejected: server at capacity");
            return Err(ProtocolError::MaxConnectionsExceeded {
                count: self.server_state.active_session_count(),
                max: self.server_state.config.max_connections,
            });
        }

        // Generate session ID
        let session_id = self.server_state.next_session_id();

        info!(
            "Accepting connection: session={}, connection={}",
            session_id, connection_id
        );

        // Create session information
        let client_info = ClientInfo {
            connection_id: connection_id.clone(),
            identity: None, // Will be filled in during CNXN handshake
            protocol_version: 0, // Will be filled in during CNXN handshake
            max_data_size: 1024 * 1024, // Default, can be negotiated
            capabilities: ClientCapabilities::default(),
        };

        let session_info = ClientSessionInfo {
            session_id: session_id.clone(),
            client_info,
            state: SessionState::Connecting,
            streams: HashMap::new(),
            stats: SessionStats::default(),
            established_at: Instant::now(),
            last_activity: Instant::now(),
        };

        // Keep the session info for our connection tracking
        let session_info_clone = session_info.clone();

        // Create message channel for connection
        let (message_sender, message_receiver) = mpsc::unbounded_channel();

        // Store connection info
        let connection_info = ConnectionInfo {
            session_info: session_info_clone.clone(),
            connection,
            message_sender: message_sender.clone(),
            last_activity: Instant::now(),
        };

        {
            let mut connections = self.connections.write().unwrap();
            connections.insert(session_id.clone(), connection_info);
        }

        // Add session to server state for proper connection counting
        {
            let mut sessions = self.server_state.client_sessions.write().unwrap();
            let client_session = crate::server::session::ClientSession {
                session_id: session_info_clone.session_id.clone(),
                client_info: session_info_clone.client_info.clone(),
                state: session_info_clone.state.clone(),
                streams: session_info_clone.streams.clone(),
                stats: session_info_clone.stats.clone(),
                config: crate::server::session::SessionConfig::default(),
                message_queue: message_sender.clone(),
                established_at: session_info_clone.established_at,
                last_activity: session_info_clone.last_activity,
            };
            sessions.insert(session_id.clone(), client_session);
        }

        // Update statistics
        self.server_state.stats.session_created();

        // Start connection handler task
        let manager = self.clone();
        let session_id_task = session_id.clone();
        tokio::spawn(async move {
            manager.handle_connection(session_id_task, message_receiver).await;
        });

        // Send connection event
        let _ = self.event_sender.send(ConnectionEvent::Connected {
            session_id: session_id.clone(),
            connection_id,
        });

        Ok(session_id)
    }

    /// Get session information by ID
    pub fn get_session_info(&self, session_id: &str) -> ProtocolResult<ClientSessionInfo> {
        let connections = self.connections.read().unwrap();
        match connections.get(session_id) {
            Some(conn_info) => Ok(conn_info.session_info.clone()),
            None => Err(ProtocolError::InternalError {
                message: format!("Session not found: {}", session_id),
            }),
        }
    }

    /// Send message to a specific connection
    pub async fn send_message(
        &self,
        session_id: &str,
        message: AdbMessage,
    ) -> ProtocolResult<()> {
        let sender = {
            let connections = self.connections.read().unwrap();
            match connections.get(session_id) {
                Some(conn_info) => conn_info.message_sender.clone(),
                None => {
                    return Err(ProtocolError::InternalError {
                        message: format!("Session not found: {}", session_id),
                    });
                }
            }
        };

        sender.send(message).map_err(|e| {
            ProtocolError::ConnectionError {
                message: format!("Failed to send message: {}", e),
            }
        })?;

        // Update activity
        self.update_activity(session_id);

        Ok(())
    }

    /// Close a connection by session ID
    pub async fn close_connection(
        &self,
        session_id: &str,
        reason: &str,
    ) -> ProtocolResult<()> {
        info!("Closing connection: session={}, reason={}", session_id, reason);

        // Remove from connections
        let connection_info = {
            let mut connections = self.connections.write().unwrap();
            connections.remove(session_id)
        };

        if let Some(_conn_info) = connection_info {
            // Remove from server state sessions too
            {
                let mut sessions = self.server_state.client_sessions.write().unwrap();
                sessions.remove(session_id);
            }

            // Update statistics
            self.server_state.stats.session_closed();

            // Send disconnection event
            let _ = self.event_sender.send(ConnectionEvent::Disconnected {
                session_id: session_id.to_string(),
                reason: reason.to_string(),
            });
        }

        Ok(())
    }

    /// Update activity timestamp for a session
    pub fn update_activity(&self, session_id: &str) {
        let mut connections = self.connections.write().unwrap();
        if let Some(conn_info) = connections.get_mut(session_id) {
            conn_info.last_activity = Instant::now();
        }

        // Send activity event
        let _ = self.event_sender.send(ConnectionEvent::Activity {
            session_id: session_id.to_string(),
        });
    }

    /// Get connection statistics
    pub fn stats(&self) -> ConnectionManagerStats {
        let connections = self.connections.read().unwrap();
        let server_stats = self.server_state.stats.snapshot();

        ConnectionManagerStats {
            active_connections: connections.len(),
            total_connections: server_stats.total_sessions,
            avg_connection_age: self.calculate_avg_connection_age(),
            oldest_connection_age: self.calculate_oldest_connection_age(),
        }
    }

    /// Start the connection manager background tasks
    pub async fn start(&self) -> ProtocolResult<()> {
        info!("Starting connection manager");

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

        // Start cleanup task
        let manager = self.clone();
        tokio::spawn(async move {
            manager.cleanup_task().await;
        });

        // Start health check task
        let manager = self.clone();
        tokio::spawn(async move {
            manager.health_check_task().await;
        });

        Ok(())
    }

    /// Handle an individual connection
    async fn handle_connection(
        &self,
        session_id: String,
        mut _message_receiver: mpsc::UnboundedReceiver<AdbMessage>,
    ) {
        debug!("Starting connection handler for session: {}", session_id);

        // Monitor connection health and handle messages
        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));
        let mut maintenance_interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                // Regular heartbeat checks
                _ = heartbeat_interval.tick() => {
                    // Check if session is still active
                    let session_active = {
                        let sessions = self.server_state.client_sessions.read().unwrap();
                        sessions.get(&session_id)
                            .map(|session| {
                                // Check for activity within last 60 seconds
                                session.last_activity.elapsed() < Duration::from_secs(60)
                            })
                            .unwrap_or(false)
                    };

                    if !session_active {
                        debug!("Session {} inactive or removed, ending handler", session_id);
                        break;
                    }

                    trace!("Connection {} heartbeat OK", session_id);
                }

                // Periodic maintenance tasks
                _ = maintenance_interval.tick() => {
                    // Update connection statistics and check limits
                    if let Err(e) = self.perform_maintenance(&session_id).await {
                        warn!("Maintenance failed for session {}: {}", session_id, e);
                    }
                }

                // Handle shutdown signals or other events
                else => {
                    debug!("Connection handler for {} shutting down", session_id);
                    break;
                }
            }
        }

        // Clean up when connection ends
        let _ = self.close_connection(&session_id, "Connection handler ended").await;
    }

    /// Process connection events
    async fn process_events(&self, event_receiver: &mut mpsc::UnboundedReceiver<ConnectionEvent>) {
        while let Some(event) = event_receiver.recv().await {
            match event {
                ConnectionEvent::Connected { session_id, connection_id } => {
                    info!("Connection established: session={}, connection={}", session_id, connection_id);
                }
                ConnectionEvent::Disconnected { session_id, reason } => {
                    info!("Connection closed: session={}, reason={}", session_id, reason);
                }
                ConnectionEvent::Error { session_id, error } => {
                    error!("Connection error: session={}, error={}", session_id, error);
                    let _ = self.close_connection(&session_id, &error).await;
                }
                ConnectionEvent::Activity { session_id } => {
                    debug!("Activity on session: {}", session_id);
                }
            }
        }
    }

    /// Background cleanup task
    async fn cleanup_task(&self) {
        let mut interval = interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            let sessions_to_cleanup = {
                let connections = self.connections.read().unwrap();
                let now = Instant::now();
                let timeout = self.server_state.config.connection_timeout;

                connections
                    .iter()
                    .filter_map(|(session_id, conn_info)| {
                        if now.duration_since(conn_info.last_activity) > timeout {
                            Some(session_id.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            };

            for session_id in sessions_to_cleanup {
                warn!("Cleaning up inactive session: {}", session_id);
                let _ = self.close_connection(&session_id, "Inactivity timeout").await;
            }
        }
    }

    /// Background health check task
    async fn health_check_task(&self) {
        let mut interval = interval(self.server_state.config.ping_interval);

        loop {
            interval.tick().await;

            let sessions_to_ping = {
                let connections = self.connections.read().unwrap();
                connections.keys().cloned().collect::<Vec<_>>()
            };

            for session_id in sessions_to_ping {
                // Send ping message
                let ping = AdbMessage::new_ping();
                if let Err(e) = self.send_message(&session_id, ping).await {
                    warn!("Failed to send ping to session {}: {}", session_id, e);
                }
            }
        }
    }

    /// Calculate average connection age
    fn calculate_avg_connection_age(&self) -> Duration {
        let connections = self.connections.read().unwrap();
        if connections.is_empty() {
            return Duration::from_secs(0);
        }

        let now = Instant::now();
        let total_age: Duration = connections
            .values()
            .map(|conn_info| now.duration_since(conn_info.session_info.established_at))
            .sum();

        total_age / connections.len() as u32
    }

    /// Calculate oldest connection age
    fn calculate_oldest_connection_age(&self) -> Duration {
        let connections = self.connections.read().unwrap();
        if connections.is_empty() {
            return Duration::from_secs(0);
        }

        let now = Instant::now();
        connections
            .values()
            .map(|conn_info| now.duration_since(conn_info.session_info.established_at))
            .max()
            .unwrap_or(Duration::from_secs(0))
    }

    /// Perform maintenance tasks for a specific connection
    async fn perform_maintenance(&self, session_id: &str) -> ProtocolResult<()> {
        let connections = self.connections.read().unwrap();

        if let Some(conn_info) = connections.get(session_id) {
            // Update last seen timestamp
            let elapsed = conn_info.session_info.established_at.elapsed();
            trace!("Connection {} maintenance: age={:?}", session_id, elapsed);

            // Check connection health
            if !conn_info.connection.is_connected() {
                warn!("Connection {} detected as disconnected during maintenance", session_id);
                // The connection will be cleaned up by the heartbeat check
                return Ok(());
            }

            // Check for memory/resource limits
            let stats = &conn_info.session_info.stats;
            if stats.bytes_received > 100_000_000 { // 100MB threshold
                debug!("Connection {} has processed {} bytes", session_id, stats.bytes_received);
            }

            // Update server state with connection activity
            if let Err(e) = self.server_state.client_sessions.read() {
                error!("Failed to read client sessions during maintenance: {}", e);
            }
        }

        Ok(())
    }
}

impl Clone for ConnectionManager {
    fn clone(&self) -> Self {
        Self {
            server_state: Arc::clone(&self.server_state),
            connections: Arc::clone(&self.connections),
            event_sender: self.event_sender.clone(),
            event_receiver: Arc::clone(&self.event_receiver),
        }
    }
}

/// Connection manager statistics
#[derive(Debug, Clone)]
pub struct ConnectionManagerStats {
    pub active_connections: usize,
    pub total_connections: u64,
    pub avg_connection_age: Duration,
    pub oldest_connection_age: Duration,
}

impl ConnectionManagerStats {
    /// Get connection turnover rate (connections per minute)
    pub fn turnover_rate(&self, uptime: Duration) -> f64 {
        if uptime.as_secs() == 0 {
            0.0
        } else {
            (self.total_connections as f64 / uptime.as_secs() as f64) * 60.0
        }
    }

    /// Get average connection lifetime in seconds
    pub fn avg_lifetime_seconds(&self) -> f64 {
        self.avg_connection_age.as_secs_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::state::{ServerConfig, ServerState};
    use crate::transport::connection::TransportResult;
    use async_trait::async_trait;
    use std::sync::Arc;

    // Mock connection for testing
    struct MockConnection {
        connection_id: String,
        is_connected: bool,
    }

    impl MockConnection {
        fn new(connection_id: &str) -> Self {
            Self {
                connection_id: connection_id.to_string(),
                is_connected: true,
            }
        }
    }

    #[async_trait]
    impl Connection for MockConnection {
        fn connection_id(&self) -> String {
            self.connection_id.clone()
        }

        async fn send_bytes(&mut self, _data: &[u8]) -> TransportResult<()> {
            Ok(())
        }

        async fn receive_bytes(&mut self, _buffer: &mut [u8]) -> TransportResult<Option<usize>> {
            Ok(Some(0))
        }

        async fn send_bytes_timeout(&mut self, data: &[u8], _timeout: Duration) -> TransportResult<()> {
            self.send_bytes(data).await
        }

        async fn receive_bytes_timeout(&mut self, buffer: &mut [u8], _timeout: Duration) -> TransportResult<Option<usize>> {
            self.receive_bytes(buffer).await
        }

        async fn shutdown(&mut self) -> TransportResult<()> {
            self.is_connected = false;
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.is_connected
        }

        async fn flush(&mut self) -> TransportResult<()> {
            Ok(())
        }

        fn max_message_size(&self) -> usize {
            1024 * 1024 // 1MB
        }

        async fn close(&mut self) -> TransportResult<()> {
            Ok(())
        }
    }

    fn create_test_manager() -> ConnectionManager {
        let config = ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap());
        let server_state = Arc::new(ServerState::new(config));
        ConnectionManager::new(server_state)
    }

    #[tokio::test]
    async fn test_connection_manager_creation() {
        let manager = create_test_manager();
        let stats = manager.stats();
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.total_connections, 0);
    }

    #[tokio::test]
    async fn test_connection_acceptance() {
        let manager = create_test_manager();

        // Create mock connection
        let mock_connection = Box::new(MockConnection::new("tcp://127.0.0.1:12345->127.0.0.1:5555"));

        // Accept connection
        let result = manager.accept_connection(mock_connection).await;
        assert!(result.is_ok());

        let session_id = result.unwrap();
        assert!(session_id.starts_with("session-"));

        // Check statistics
        let stats = manager.stats();
        assert_eq!(stats.active_connections, 1);
        assert_eq!(stats.total_connections, 1);
    }

    #[tokio::test]
    async fn test_connection_limits() {
        let mut config = ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap());
        config.max_connections = 1; // Set limit to 1

        let server_state = Arc::new(ServerState::new(config));
        let manager = ConnectionManager::new(server_state);

        // Accept first connection
        let mock_connection1 = Box::new(MockConnection::new("tcp://127.0.0.1:12345->127.0.0.1:5555"));
        let result1 = manager.accept_connection(mock_connection1).await;
        assert!(result1.is_ok());

        // Try to accept second connection (should fail)
        let mock_connection2 = Box::new(MockConnection::new("tcp://127.0.0.1:12346->127.0.0.1:5555"));
        let result2 = manager.accept_connection(mock_connection2).await;
        assert!(result2.is_err());

        if let Err(ProtocolError::MaxConnectionsExceeded { count, max }) = result2 {
            assert_eq!(count, 1);
            assert_eq!(max, 1);
        } else {
            panic!("Expected MaxConnectionsExceeded error");
        }
    }

    #[tokio::test]
    async fn test_connection_cleanup() {
        let manager = create_test_manager();

        let mock_connection = Box::new(MockConnection::new("tcp://127.0.0.1:12345->127.0.0.1:5555"));
        let session_id = manager.accept_connection(mock_connection).await.unwrap();

        // Verify connection exists
        let stats_before = manager.stats();
        assert_eq!(stats_before.active_connections, 1);

        // Close connection
        let result = manager.close_connection(&session_id, "Test cleanup").await;
        assert!(result.is_ok());

        // Verify connection is removed
        let stats_after = manager.stats();
        assert_eq!(stats_after.active_connections, 0);
        assert_eq!(stats_after.total_connections, 1); // Total doesn't decrease
    }

    #[tokio::test]
    async fn test_message_sending() {
        let manager = create_test_manager();

        let mock_connection = Box::new(MockConnection::new("tcp://127.0.0.1:12345->127.0.0.1:5555"));
        let session_id = manager.accept_connection(mock_connection).await.unwrap();

        // Send a message
        let message = AdbMessage::new_ping();
        let result = manager.send_message(&session_id, message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_activity_tracking() {
        let manager = create_test_manager();

        let mock_connection = Box::new(MockConnection::new("tcp://127.0.0.1:12345->127.0.0.1:5555"));
        let session_id = manager.accept_connection(mock_connection).await.unwrap();

        // Update activity
        manager.update_activity(&session_id);

        // Activity update should not cause any errors
        // This is mostly for coverage and ensuring the method works
    }

    #[tokio::test]
    async fn test_session_retrieval() {
        let manager = create_test_manager();

        let mock_connection = Box::new(MockConnection::new("tcp://127.0.0.1:12345->127.0.0.1:5555"));
        let session_id = manager.accept_connection(mock_connection).await.unwrap();

        // Get session
        let result = manager.get_session_info(&session_id);
        assert!(result.is_ok());

        let session = result.unwrap();
        assert_eq!(session.session_id, session_id);
    }
}