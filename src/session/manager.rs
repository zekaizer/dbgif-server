use crate::protocol::{AdbMessage as Message, Command, HandshakeManager, HandshakeHandler, HandshakeState};
use crate::transport::TransportManager;
use anyhow::{Result, Context};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock, Mutex};
use std::time::{Duration, Instant};

/// Manages ADB sessions for multiple clients and devices
pub struct AdbSessionManager {
    transport_manager: Arc<TransportManager>,
    active_sessions: Arc<RwLock<HashMap<String, Arc<AdbSession>>>>,
    session_timeout: Duration,
    max_concurrent_sessions: usize,
}

impl AdbSessionManager {
    pub fn new(transport_manager: Arc<TransportManager>) -> Self {
        Self {
            transport_manager,
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            session_timeout: Duration::from_secs(300), // 5 minutes
            max_concurrent_sessions: 100,
        }
    }

    pub fn with_config(
        transport_manager: Arc<TransportManager>,
        session_timeout: Duration,
        max_concurrent_sessions: usize,
    ) -> Self {
        Self {
            transport_manager,
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            session_timeout,
            max_concurrent_sessions,
        }
    }

    /// Create a new ADB session for a client
    pub async fn create_session(
        &self,
        client_id: String,
        device_id: String,
        system_identity: String,
    ) -> Result<Arc<AdbSession>> {
        // Check session limits
        {
            let sessions = self.active_sessions.read().await;
            if sessions.len() >= self.max_concurrent_sessions {
                return Err(anyhow::anyhow!(
                    "Maximum concurrent sessions reached: {}",
                    self.max_concurrent_sessions
                ));
            }
        }

        // Create unique session ID
        let session_id = format!("{}:{}", client_id, device_id);

        // Check if session already exists
        {
            let sessions = self.active_sessions.read().await;
            if sessions.contains_key(&session_id) {
                return Err(anyhow::anyhow!(
                    "Session already exists for client {} on device {}",
                    client_id,
                    device_id
                ));
            }
        }

        // Acquire device access through transport manager
        self.transport_manager
            .acquire_device(&device_id, client_id.clone())
            .await
            .context("Failed to acquire device access")?;

        // Create session
        let session = Arc::new(AdbSession::new(
            session_id.clone(),
            client_id,
            device_id.clone(),
            system_identity,
            self.session_timeout,
        ));

        // Store session
        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(session_id.clone(), Arc::clone(&session));
        }

        tracing::info!(
            "Created ADB session: {} for device {}",
            session_id,
            device_id
        );

        Ok(session)
    }

    /// Get an existing session
    pub async fn get_session(&self, session_id: &str) -> Option<Arc<AdbSession>> {
        let sessions = self.active_sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Remove and cleanup a session
    pub async fn remove_session(&self, session_id: &str) -> Result<()> {
        let session = {
            let mut sessions = self.active_sessions.write().await;
            sessions.remove(session_id)
        };

        if let Some(session) = session {
            // Cleanup session resources
            session.shutdown().await?;

            // Release device access
            self.transport_manager
                .release_device(&session.device_id, session.client_id.clone())
                .await
                .context("Failed to release device access")?;

            tracing::info!("Removed ADB session: {}", session_id);
        }

        Ok(())
    }

    /// Get all active sessions
    pub async fn get_all_sessions(&self) -> Vec<Arc<AdbSession>> {
        let sessions = self.active_sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self) -> Result<usize> {
        let now = Instant::now();
        let mut expired_sessions = Vec::new();

        // Find expired sessions
        {
            let sessions = self.active_sessions.read().await;
            for (session_id, session) in sessions.iter() {
                if session.is_expired(now) {
                    expired_sessions.push(session_id.clone());
                }
            }
        }

        // Remove expired sessions
        for session_id in &expired_sessions {
            if let Err(e) = self.remove_session(session_id).await {
                tracing::warn!("Failed to cleanup expired session {}: {}", session_id, e);
            }
        }

        let count = expired_sessions.len();
        if count > 0 {
            tracing::info!("Cleaned up {} expired sessions", count);
        }

        Ok(count)
    }

    /// Get session statistics
    pub async fn get_stats(&self) -> SessionManagerStats {
        let sessions = self.active_sessions.read().await;
        let active_count = sessions.len();

        let mut connected_count = 0;
        let mut handshaking_count = 0;

        for session in sessions.values() {
            match session.get_state().await {
                SessionState::Connected => connected_count += 1,
                SessionState::Handshaking => handshaking_count += 1,
                _ => {}
            }
        }

        SessionManagerStats {
            active_sessions: active_count,
            connected_sessions: connected_count,
            handshaking_sessions: handshaking_count,
            max_concurrent_sessions: self.max_concurrent_sessions,
            session_timeout: self.session_timeout,
            total_sessions_created: active_count, // Simplified - in reality would track total
        }
    }

    /// Start session cleanup task
    pub async fn start_cleanup_task(&self) -> Result<()> {
        let manager = Arc::new(self.clone());
        tokio::spawn(async move {
            let mut cleanup_interval = tokio::time::interval(Duration::from_secs(60)); // Every minute

            loop {
                cleanup_interval.tick().await;

                if let Err(e) = manager.cleanup_expired_sessions().await {
                    tracing::error!("Session cleanup failed: {}", e);
                }
            }
        });

        Ok(())
    }
}

impl Clone for AdbSessionManager {
    fn clone(&self) -> Self {
        Self {
            transport_manager: Arc::clone(&self.transport_manager),
            active_sessions: Arc::clone(&self.active_sessions),
            session_timeout: self.session_timeout,
            max_concurrent_sessions: self.max_concurrent_sessions,
        }
    }
}

/// Individual ADB session for a client-device pair
pub struct AdbSession {
    pub session_id: String,
    pub client_id: String,
    pub device_id: String,
    state: Arc<Mutex<SessionState>>,
    handshake_manager: Arc<Mutex<Option<HandshakeManager>>>,
    created_at: Instant,
    last_activity: Arc<Mutex<Instant>>,
    timeout: Duration,

    // Communication channels
    message_sender: mpsc::UnboundedSender<Message>,
    message_receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<Message>>>>,
}

impl AdbSession {
    pub fn new(
        session_id: String,
        client_id: String,
        device_id: String,
        system_identity: String,
        timeout: Duration,
    ) -> Self {
        let (message_sender, message_receiver) = mpsc::unbounded_channel();

        // Create handshake handler
        let handshake_handler = HandshakeHandler::new(system_identity);
        let handshake_manager = HandshakeManager::new(handshake_handler);

        let now = Instant::now();

        Self {
            session_id,
            client_id,
            device_id,
            state: Arc::new(Mutex::new(SessionState::Created)),
            handshake_manager: Arc::new(Mutex::new(Some(handshake_manager))),
            created_at: now,
            last_activity: Arc::new(Mutex::new(now)),
            timeout,
            message_sender,
            message_receiver: Arc::new(Mutex::new(Some(message_receiver))),
        }
    }

    /// Start the handshake process
    pub async fn start_handshake(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        if *state != SessionState::Created {
            return Err(anyhow::anyhow!(
                "Cannot start handshake in state: {:?}",
                *state
            ));
        }

        *state = SessionState::Handshaking;

        // Start handshake manager
        if let Some(handshake_manager) = self.handshake_manager.lock().await.as_mut() {
            handshake_manager.start();
        }

        self.update_activity().await;

        tracing::debug!("Started handshake for session: {}", self.session_id);
        Ok(())
    }

    /// Process incoming message during handshake
    pub async fn process_handshake_message(&self, message: &Message) -> Result<Option<Message>> {
        let mut state = self.state.lock().await;
        if *state != SessionState::Handshaking {
            return Err(anyhow::anyhow!(
                "Not in handshaking state: {:?}",
                *state
            ));
        }

        self.update_activity().await;

        let response = if let Some(handshake_manager) = self.handshake_manager.lock().await.as_mut() {
            handshake_manager.process_message(message)?
        } else {
            return Err(anyhow::anyhow!("Handshake manager not available"));
        };

        // Check if handshake completed
        if let Some(handshake_manager) = self.handshake_manager.lock().await.as_ref() {
            if handshake_manager.is_completed() {
                *state = SessionState::Connected;
                tracing::info!("Handshake completed for session: {}", self.session_id);
            } else if handshake_manager.is_failed() {
                *state = SessionState::Failed;
                tracing::warn!("Handshake failed for session: {}", self.session_id);
            }
        }

        Ok(response)
    }

    /// Send a message through the session
    pub async fn send_message(&self, message: Message) -> Result<()> {
        let state = self.state.lock().await;
        if *state != SessionState::Connected {
            return Err(anyhow::anyhow!(
                "Cannot send message in state: {:?}",
                *state
            ));
        }

        self.message_sender.send(message)
            .map_err(|_| anyhow::anyhow!("Failed to send message - channel closed"))?;

        self.update_activity().await;
        Ok(())
    }

    /// Take the message receiver (for session processing loop)
    pub async fn take_message_receiver(&self) -> Option<mpsc::UnboundedReceiver<Message>> {
        self.message_receiver.lock().await.take()
    }

    /// Get current session state
    pub async fn get_state(&self) -> SessionState {
        *self.state.lock().await
    }

    /// Check if session is expired
    pub fn is_expired(&self, current_time: Instant) -> bool {
        current_time.duration_since(self.created_at) > self.timeout
    }

    /// Update last activity timestamp
    async fn update_activity(&self) {
        let mut last_activity = self.last_activity.lock().await;
        *last_activity = Instant::now();
    }

    /// Get session info
    pub async fn get_info(&self) -> SessionInfo {
        let state = *self.state.lock().await;
        let last_activity = *self.last_activity.lock().await;

        SessionInfo {
            session_id: self.session_id.clone(),
            client_id: self.client_id.clone(),
            device_id: self.device_id.clone(),
            state,
            created_at: self.created_at,
            last_activity,
            timeout: self.timeout,
        }
    }

    /// Shutdown the session
    pub async fn shutdown(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        *state = SessionState::Closed;

        tracing::debug!("Session shutdown: {}", self.session_id);
        Ok(())
    }
}

/// ADB session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Created,
    Handshaking,
    Connected,
    Failed,
    Closed,
}

/// Session information for monitoring
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub client_id: String,
    pub device_id: String,
    pub state: SessionState,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub timeout: Duration,
}

/// Session manager statistics
#[derive(Debug, Clone)]
pub struct SessionManagerStats {
    pub active_sessions: usize,
    pub connected_sessions: usize,
    pub handshaking_sessions: usize,
    pub max_concurrent_sessions: usize,
    pub session_timeout: Duration,
    pub total_sessions_created: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportManager;

    #[tokio::test]
    async fn test_session_manager_creation() {
        let transport_manager = Arc::new(TransportManager::new());
        let session_manager = AdbSessionManager::new(transport_manager);

        let stats = session_manager.get_stats().await;
        assert_eq!(stats.active_sessions, 0);
        assert_eq!(stats.max_concurrent_sessions, 100);
    }

    #[tokio::test]
    async fn test_session_creation() {
        let session = AdbSession::new(
            "test_session".to_string(),
            "client1".to_string(),
            "device1".to_string(),
            "test_system".to_string(),
            Duration::from_secs(60),
        );

        assert_eq!(session.session_id, "test_session");
        assert_eq!(session.client_id, "client1");
        assert_eq!(session.device_id, "device1");

        let state = session.get_state().await;
        assert_eq!(state, SessionState::Created);
    }

    #[tokio::test]
    async fn test_session_handshake() {
        let session = AdbSession::new(
            "test_session".to_string(),
            "client1".to_string(),
            "device1".to_string(),
            "test_system".to_string(),
            Duration::from_secs(60),
        );

        // Start handshake
        session.start_handshake().await.unwrap();

        let state = session.get_state().await;
        assert_eq!(state, SessionState::Handshaking);

        // Process CNXN message
        let cnxn_message = Message::new(
            Command::CNXN,
            0x01000000,
            256 * 1024,
            bytes::Bytes::from("client_system"),
        );

        let response = session.process_handshake_message(&cnxn_message).await.unwrap();
        assert!(response.is_some());

        let state = session.get_state().await;
        assert_eq!(state, SessionState::Connected);
    }

    #[tokio::test]
    async fn test_session_expiry() {
        let session = AdbSession::new(
            "test_session".to_string(),
            "client1".to_string(),
            "device1".to_string(),
            "test_system".to_string(),
            Duration::from_millis(1), // Very short timeout
        );

        // Wait for expiry
        tokio::time::sleep(Duration::from_millis(10)).await;

        let now = Instant::now();
        assert!(session.is_expired(now));
    }
}

impl AdbSessionManager {
    /// Shutdown all active sessions
    pub async fn shutdown_all_sessions(&self) {
        let sessions = self.active_sessions.read().await;

        for (session_id, _session) in sessions.iter() {
            tracing::debug!("Shutting down session: {}", session_id);
            // Sessions will be cleaned up when they're dropped
            // In a more complete implementation, we would send termination signals
        }

        // Clear all sessions
        drop(sessions);
        let mut sessions = self.active_sessions.write().await;
        sessions.clear();

        tracing::info!("All sessions shut down");
    }
}

