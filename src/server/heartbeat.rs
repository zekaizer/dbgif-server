use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, warn};

use crate::protocol::message::AdbMessage;
use crate::protocol::error::{ProtocolError, ProtocolResult};
use crate::server::session::ClientSessionInfo;

/// Heartbeat manager for maintaining connection health
pub struct HeartbeatManager {
    /// Heartbeat configuration
    config: HeartbeatConfig,
    /// Active heartbeat sessions
    sessions: Arc<RwLock<HashMap<String, HeartbeatSession>>>,
    /// Shutdown signal receiver
    shutdown_rx: Option<mpsc::Receiver<()>>,
    /// Shutdown signal sender
    shutdown_tx: mpsc::Sender<()>,
}

/// Heartbeat configuration
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Interval between ping messages
    pub ping_interval: Duration,
    /// Timeout waiting for pong response
    pub pong_timeout: Duration,
    /// Maximum consecutive missed pongs before marking connection as dead
    pub max_missed_pongs: u32,
    /// Enable automatic ping sending
    pub auto_ping_enabled: bool,
    /// Heartbeat check interval
    pub check_interval: Duration,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(30),
            pong_timeout: Duration::from_secs(10),
            max_missed_pongs: 3,
            auto_ping_enabled: true,
            check_interval: Duration::from_secs(5),
        }
    }
}

/// Individual heartbeat session for a client connection
#[derive(Debug)]
pub struct HeartbeatSession {
    /// Session information
    pub session_info: ClientSessionInfo,
    /// Last ping sent time
    pub last_ping_sent: Option<Instant>,
    /// Last pong received time
    pub last_pong_received: Option<Instant>,
    /// Number of consecutive missed pongs
    pub missed_pongs: u32,
    /// Connection status
    pub status: ConnectionStatus,
    /// Message sender for sending pings
    pub message_tx: mpsc::Sender<AdbMessage>,
    /// Statistics
    pub stats: HeartbeatStats,
}

/// Connection health status
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    /// Connection is healthy
    Healthy,
    /// Ping sent, waiting for pong
    PingPending { sent_at: Instant },
    /// Connection is degraded (some missed pongs)
    Degraded { missed_count: u32 },
    /// Connection is considered dead
    Dead { reason: String },
}

/// Heartbeat statistics
#[derive(Debug)]
pub struct HeartbeatStats {
    /// Total pings sent
    pub pings_sent: u64,
    /// Total pongs received
    pub pongs_received: u64,
    /// Total timeouts
    pub timeouts: u64,
    /// Average round-trip time
    pub avg_rtt_ms: f64,
    /// Connection start time
    pub connection_start: Instant,
}

impl Default for HeartbeatStats {
    fn default() -> Self {
        Self::new()
    }
}

impl HeartbeatStats {
    pub fn new() -> Self {
        Self {
            pings_sent: 0,
            pongs_received: 0,
            timeouts: 0,
            avg_rtt_ms: 0.0,
            connection_start: Instant::now(),
        }
    }

    /// Record a ping sent
    pub fn record_ping_sent(&mut self) {
        self.pings_sent += 1;
    }

    /// Record a pong received and calculate RTT
    pub fn record_pong_received(&mut self, ping_sent_at: Instant) {
        self.pongs_received += 1;

        let rtt = ping_sent_at.elapsed();
        let rtt_ms = rtt.as_millis() as f64;

        // Update average RTT using exponential moving average
        if self.avg_rtt_ms == 0.0 {
            self.avg_rtt_ms = rtt_ms;
        } else {
            self.avg_rtt_ms = self.avg_rtt_ms * 0.8 + rtt_ms * 0.2;
        }

        debug!("RTT: {:.2}ms, Avg RTT: {:.2}ms", rtt_ms, self.avg_rtt_ms);
    }

    /// Record a timeout
    pub fn record_timeout(&mut self) {
        self.timeouts += 1;
    }

    /// Get connection uptime
    pub fn uptime(&self) -> Duration {
        self.connection_start.elapsed()
    }

    /// Get success rate as percentage
    pub fn success_rate(&self) -> f64 {
        if self.pings_sent == 0 {
            0.0
        } else {
            (self.pongs_received as f64 / self.pings_sent as f64) * 100.0
        }
    }
}

impl HeartbeatManager {
    /// Create a new heartbeat manager
    pub fn new(config: HeartbeatConfig) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            shutdown_rx: Some(shutdown_rx),
            shutdown_tx,
        }
    }

    /// Add a new session to heartbeat monitoring
    pub async fn add_session(
        &self,
        session_info: ClientSessionInfo,
        message_tx: mpsc::Sender<AdbMessage>,
    ) -> ProtocolResult<()> {
        let session = HeartbeatSession {
            session_info: session_info.clone(),
            last_ping_sent: None,
            last_pong_received: Some(Instant::now()), // Start healthy
            missed_pongs: 0,
            status: ConnectionStatus::Healthy,
            message_tx,
            stats: HeartbeatStats::new(),
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_info.session_id.clone(), session);

        info!("Added heartbeat monitoring for session {}", session_info.session_id);
        Ok(())
    }

    /// Remove a session from heartbeat monitoring
    pub async fn remove_session(&self, session_id: &str) -> ProtocolResult<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(session_id) {
            info!(
                "Removed heartbeat monitoring for session {} (uptime: {:?}, success rate: {:.1}%)",
                session_id,
                session.stats.uptime(),
                session.stats.success_rate()
            );
        }
        Ok(())
    }

    /// Handle received pong message
    pub async fn handle_pong(&self, session_id: &str) -> ProtocolResult<()> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            let now = Instant::now();
            session.last_pong_received = Some(now);
            session.missed_pongs = 0;

            // Calculate RTT if we have a pending ping
            if let ConnectionStatus::PingPending { sent_at } = session.status {
                session.stats.record_pong_received(sent_at);
                session.status = ConnectionStatus::Healthy;
                debug!("Received pong from session {} (RTT: {:.2}ms)", session_id, session.stats.avg_rtt_ms);
            } else {
                // Unsolicited pong
                session.stats.record_pong_received(now);
                debug!("Received unsolicited pong from session {}", session_id);
            }
        } else {
            warn!("Received pong from unknown session: {}", session_id);
        }

        Ok(())
    }

    /// Send ping to a specific session
    pub async fn send_ping(&self, session_id: &str) -> ProtocolResult<()> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(session_id) {
            let ping_message = AdbMessage::new_ping();

            match session.message_tx.send(ping_message).await {
                Ok(()) => {
                    let now = Instant::now();
                    session.last_ping_sent = Some(now);
                    session.status = ConnectionStatus::PingPending { sent_at: now };
                    session.stats.record_ping_sent();
                    debug!("Sent ping to session {}", session_id);
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to send ping to session {}: {}", session_id, e);
                    session.status = ConnectionStatus::Dead {
                        reason: format!("Failed to send ping: {}", e)
                    };
                    Err(ProtocolError::ConnectionClosed)
                }
            }
        } else {
            warn!("Attempted to ping unknown session: {}", session_id);
            Err(ProtocolError::InternalError { message: "Session not found".to_string() })
        }
    }

    /// Start the heartbeat monitoring task
    pub async fn start_monitoring(&mut self) -> ProtocolResult<tokio::task::JoinHandle<()>> {
        let config = self.config.clone();
        let sessions = self.sessions.clone();
        let mut shutdown_rx = self.shutdown_rx.take().unwrap();

        let handle = tokio::spawn(async move {
            let mut ping_interval = interval(config.ping_interval);
            let mut check_interval = interval(config.check_interval);

            info!("Starting heartbeat monitoring (ping interval: {:?})", config.ping_interval);

            loop {
                tokio::select! {
                    // Send pings periodically
                    _ = ping_interval.tick(), if config.auto_ping_enabled => {
                        Self::send_periodic_pings(&sessions, &config).await;
                    }

                    // Check connection health
                    _ = check_interval.tick() => {
                        Self::check_connection_health(&sessions, &config).await;
                    }

                    // Handle shutdown
                    _ = shutdown_rx.recv() => {
                        info!("Heartbeat monitoring shutting down");
                        break;
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Send pings to all healthy sessions
    async fn send_periodic_pings(
        sessions: &Arc<RwLock<HashMap<String, HeartbeatSession>>>,
        _config: &HeartbeatConfig,
    ) {
        let session_ids: Vec<String> = {
            let sessions_guard = sessions.read().await;
            sessions_guard.keys().cloned().collect()
        };

        for session_id in session_ids {
            let should_ping = {
                let sessions_guard = sessions.read().await;
                if let Some(session) = sessions_guard.get(&session_id) {
                    matches!(session.status, ConnectionStatus::Healthy)
                } else {
                    false
                }
            };

            if should_ping {
                Self::send_ping_to_session(sessions, &session_id).await;
            }
        }
    }

    /// Send ping to a specific session (internal helper)
    async fn send_ping_to_session(
        sessions: &Arc<RwLock<HashMap<String, HeartbeatSession>>>,
        session_id: &str,
    ) {
        let mut sessions_guard = sessions.write().await;

        if let Some(session) = sessions_guard.get_mut(session_id) {
            let ping_message = AdbMessage::new_ping();

            match session.message_tx.send(ping_message).await {
                Ok(()) => {
                    let now = Instant::now();
                    session.last_ping_sent = Some(now);
                    session.status = ConnectionStatus::PingPending { sent_at: now };
                    session.stats.record_ping_sent();
                    debug!("Sent periodic ping to session {}", session_id);
                }
                Err(e) => {
                    error!("Failed to send ping to session {}: {}", session_id, e);
                    session.status = ConnectionStatus::Dead {
                        reason: format!("Failed to send ping: {}", e)
                    };
                }
            }
        }
    }

    /// Check health of all connections and handle timeouts
    async fn check_connection_health(
        sessions: &Arc<RwLock<HashMap<String, HeartbeatSession>>>,
        config: &HeartbeatConfig,
    ) {
        let now = Instant::now();
        let mut sessions_to_remove = Vec::new();
        let mut sessions_to_update = Vec::new();

        {
            let sessions_guard = sessions.read().await;
            for (session_id, session) in sessions_guard.iter() {
                match &session.status {
                    ConnectionStatus::PingPending { sent_at } => {
                        if now.duration_since(*sent_at) > config.pong_timeout {
                            // Ping timed out
                            let new_missed_count = session.missed_pongs + 1;

                            if new_missed_count >= config.max_missed_pongs {
                                // Connection is dead
                                sessions_to_remove.push(session_id.clone());
                                warn!("Session {} marked as dead after {} missed pongs",
                                     session_id, new_missed_count);
                            } else {
                                // Connection is degraded
                                sessions_to_update.push((
                                    session_id.clone(),
                                    ConnectionStatus::Degraded { missed_count: new_missed_count }
                                ));
                                warn!("Session {} degraded: {} missed pongs",
                                     session_id, new_missed_count);
                            }
                        }
                    }
                    ConnectionStatus::Dead { .. } => {
                        sessions_to_remove.push(session_id.clone());
                    }
                    _ => {} // Healthy or degraded connections are fine
                }
            }
        }

        // Apply updates
        let mut sessions_guard = sessions.write().await;

        for (session_id, new_status) in sessions_to_update {
            if let Some(session) = sessions_guard.get_mut(&session_id) {
                session.status = new_status;
                session.missed_pongs += 1;
                session.stats.record_timeout();
            }
        }

        for session_id in sessions_to_remove {
            if let Some(session) = sessions_guard.remove(&session_id) {
                error!(
                    "Removed dead session {} (uptime: {:?}, success rate: {:.1}%)",
                    session_id,
                    session.stats.uptime(),
                    session.stats.success_rate()
                );
            }
        }
    }

    /// Get heartbeat statistics for all sessions
    pub async fn get_stats(&self) -> HashMap<String, HeartbeatStats> {
        let sessions = self.sessions.read().await;
        let mut stats = HashMap::new();

        for (session_id, session) in sessions.iter() {
            stats.insert(session_id.clone(), HeartbeatStats {
                pings_sent: session.stats.pings_sent,
                pongs_received: session.stats.pongs_received,
                timeouts: session.stats.timeouts,
                avg_rtt_ms: session.stats.avg_rtt_ms,
                connection_start: session.stats.connection_start,
            });
        }

        stats
    }

    /// Get connection status for a specific session
    pub async fn get_session_status(&self, session_id: &str) -> Option<ConnectionStatus> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|s| s.status.clone())
    }

    /// Shutdown heartbeat manager
    pub async fn shutdown(&self) -> ProtocolResult<()> {
        info!("Shutting down heartbeat manager");

        if let Err(e) = self.shutdown_tx.send(()).await {
            warn!("Failed to send shutdown signal: {}", e);
        }

        // Wait a bit for cleanup
        sleep(Duration::from_millis(100)).await;

        let sessions = self.sessions.read().await;
        info!("Heartbeat manager shutdown complete ({} sessions monitored)", sessions.len());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::session::{ClientSessionInfo, ClientInfo, SessionState, SessionStats, ClientCapabilities};
    use tokio::sync::mpsc;
    use tokio::time::timeout;
    use std::collections::HashMap;

    fn create_test_session_info() -> ClientSessionInfo {
        ClientSessionInfo {
            session_id: "test-session".to_string(),
            client_info: ClientInfo {
                connection_id: "tcp://127.0.0.1:1234".to_string(),
                identity: Some("test-client".to_string()),
                protocol_version: 1,
                max_data_size: 1048576,
                capabilities: ClientCapabilities::default(),
            },
            state: SessionState::Active,
            streams: HashMap::new(),
            stats: SessionStats::default(),
            established_at: Instant::now(),
            last_activity: Instant::now(),
        }
    }

    #[tokio::test]
    async fn test_heartbeat_manager_creation() {
        let config = HeartbeatConfig::default();
        let manager = HeartbeatManager::new(config);

        // Should start with no sessions
        let stats = manager.get_stats().await;
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_add_remove_session() {
        let manager = HeartbeatManager::new(HeartbeatConfig::default());
        let (tx, _rx) = mpsc::channel(10);

        let session_info = create_test_session_info();

        // Add session
        manager.add_session(session_info.clone(), tx).await.unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.len(), 1);
        assert!(stats.contains_key("test-session"));

        // Remove session
        manager.remove_session("test-session").await.unwrap();

        let stats = manager.get_stats().await;
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_ping_pong_flow() {
        let manager = HeartbeatManager::new(HeartbeatConfig::default());
        let (tx, mut rx) = mpsc::channel(10);

        let session_info = create_test_session_info();

        // Add session
        manager.add_session(session_info, tx).await.unwrap();

        // Send ping
        manager.send_ping("test-session").await.unwrap();

        // Should receive ping message
        let message = timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
        assert_eq!(message.command, crate::protocol::commands::AdbCommand::PING as u32);

        // Session should be in PingPending state
        let status = manager.get_session_status("test-session").await.unwrap();
        assert!(matches!(status, ConnectionStatus::PingPending { .. }));

        // Handle pong response
        manager.handle_pong("test-session").await.unwrap();

        // Session should be healthy again
        let status = manager.get_session_status("test-session").await.unwrap();
        assert_eq!(status, ConnectionStatus::Healthy);

        // Check stats
        let stats = manager.get_stats().await;
        let session_stats = stats.get("test-session").unwrap();
        assert_eq!(session_stats.pings_sent, 1);
        assert_eq!(session_stats.pongs_received, 1);
        assert!(session_stats.avg_rtt_ms >= 0.0); // RTT should be non-negative
    }

    #[tokio::test]
    async fn test_heartbeat_stats() {
        let mut stats = HeartbeatStats::new();

        assert_eq!(stats.pings_sent, 0);
        assert_eq!(stats.pongs_received, 0);
        assert_eq!(stats.success_rate(), 0.0);

        // Record some activity
        stats.record_ping_sent();
        stats.record_ping_sent();
        assert_eq!(stats.pings_sent, 2);
        assert_eq!(stats.success_rate(), 0.0); // No pongs yet

        let ping_time = Instant::now();
        stats.record_pong_received(ping_time);
        assert_eq!(stats.pongs_received, 1);
        assert_eq!(stats.success_rate(), 50.0); // 1/2 = 50%
        assert!(stats.avg_rtt_ms >= 0.0);

        stats.record_timeout();
        assert_eq!(stats.timeouts, 1);
    }

    #[tokio::test]
    async fn test_connection_status_transitions() {
        // Test status transitions
        let status = ConnectionStatus::Healthy;
        assert_eq!(status, ConnectionStatus::Healthy);

        let ping_status = ConnectionStatus::PingPending { sent_at: Instant::now() };
        assert!(matches!(ping_status, ConnectionStatus::PingPending { .. }));

        let degraded_status = ConnectionStatus::Degraded { missed_count: 1 };
        assert!(matches!(degraded_status, ConnectionStatus::Degraded { missed_count: 1 }));

        let dead_status = ConnectionStatus::Dead { reason: "timeout".to_string() };
        assert!(matches!(dead_status, ConnectionStatus::Dead { .. }));
    }
}