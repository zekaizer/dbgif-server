use crate::transport::TransportManager;
use crate::session::AdbSessionManager;
use anyhow::{Result, Context};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use std::time::Duration;

/// ADB TCP server for handling client connections
pub struct AdbTcpServer {
    bind_addr: SocketAddr,
    transport_manager: Arc<TransportManager>,
    session_manager: Arc<AdbSessionManager>,
    connection_timeout: Duration,
    max_connections: usize,
    active_connections: Arc<tokio::sync::RwLock<usize>>,

    // Shutdown coordination
    shutdown_sender: Option<mpsc::Sender<()>>,
    shutdown_receiver: Option<mpsc::Receiver<()>>,
}

impl AdbTcpServer {
    /// Create new ADB TCP server
    pub fn new(
        bind_addr: SocketAddr,
        transport_manager: Arc<TransportManager>,
        session_manager: Arc<AdbSessionManager>,
    ) -> Self {
        let (shutdown_sender, shutdown_receiver) = mpsc::channel(1);

        Self {
            bind_addr,
            transport_manager,
            session_manager,
            connection_timeout: Duration::from_secs(30),
            max_connections: 100,
            active_connections: Arc::new(tokio::sync::RwLock::new(0)),
            shutdown_sender: Some(shutdown_sender),
            shutdown_receiver: Some(shutdown_receiver),
        }
    }

    /// Create server with custom configuration
    pub fn with_config(
        bind_addr: SocketAddr,
        transport_manager: Arc<TransportManager>,
        session_manager: Arc<AdbSessionManager>,
        connection_timeout: Duration,
        max_connections: usize,
    ) -> Self {
        let (shutdown_sender, shutdown_receiver) = mpsc::channel(1);

        Self {
            bind_addr,
            transport_manager,
            session_manager,
            connection_timeout,
            max_connections,
            active_connections: Arc::new(tokio::sync::RwLock::new(0)),
            shutdown_sender: Some(shutdown_sender),
            shutdown_receiver: Some(shutdown_receiver),
        }
    }

    /// Start the TCP server
    pub async fn start(&mut self) -> Result<()> {
        let listener = TcpListener::bind(self.bind_addr)
            .await
            .context(format!("Failed to bind to {}", self.bind_addr))?;

        tracing::info!("ADB TCP server listening on {}", self.bind_addr);

        let mut shutdown_receiver = self.shutdown_receiver.take()
            .ok_or_else(|| anyhow::anyhow!("Server already started"))?;

        // Server main loop
        loop {
            tokio::select! {
                // Handle incoming connections
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            if let Err(e) = self.handle_new_connection(stream, peer_addr).await {
                                tracing::warn!("Failed to handle connection from {}: {}", peer_addr, e);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to accept connection: {}", e);
                            // Continue accepting connections despite errors
                        }
                    }
                }

                // Handle shutdown signal
                _ = shutdown_receiver.recv() => {
                    tracing::info!("Received shutdown signal, stopping TCP server");
                    break;
                }
            }
        }

        tracing::info!("ADB TCP server stopped");
        Ok(())
    }

    /// Handle new client connection
    async fn handle_new_connection(&self, stream: TcpStream, peer_addr: SocketAddr) -> Result<()> {
        // Check connection limits
        {
            let active_count = *self.active_connections.read().await;
            if active_count >= self.max_connections {
                tracing::warn!(
                    "Rejecting connection from {} - max connections reached: {}",
                    peer_addr,
                    self.max_connections
                );
                return Ok(()); // Drop connection silently
            }
        }

        // Increment active connection count
        {
            let mut active_count = self.active_connections.write().await;
            *active_count += 1;
        }

        tracing::info!("Accepted new ADB client connection from {}", peer_addr);

        // Spawn connection handler
        let transport_manager = Arc::clone(&self.transport_manager);
        let session_manager = Arc::clone(&self.session_manager);
        let active_connections = Arc::clone(&self.active_connections);
        let timeout = self.connection_timeout;

        tokio::spawn(async move {
            // Handle the client connection
            let client_id = format!("client_{}", peer_addr);
            let handler = ClientConnectionHandler::new(
                client_id,
                stream,
                peer_addr,
                transport_manager,
                session_manager,
                timeout,
            );

            if let Err(e) = handler.handle().await {
                tracing::warn!("Client connection error for {}: {}", peer_addr, e);
            }

            // Decrement active connection count
            {
                let mut active_count = active_connections.write().await;
                *active_count = active_count.saturating_sub(1);
            }

            tracing::info!("Client connection closed: {}", peer_addr);
        });

        Ok(())
    }

    /// Get shutdown sender for graceful shutdown
    pub fn shutdown_sender(&mut self) -> Option<mpsc::Sender<()>> {
        self.shutdown_sender.take()
    }

    /// Get server statistics
    pub async fn get_stats(&self) -> ServerStats {
        let active_connections = *self.active_connections.read().await;

        ServerStats {
            bind_addr: self.bind_addr,
            active_connections,
            max_connections: self.max_connections,
            connection_timeout: self.connection_timeout,
        }
    }
}

/// Handles individual client connections
pub struct ClientConnectionHandler {
    client_id: String,
    stream: TcpStream,
    peer_addr: SocketAddr,
    transport_manager: Arc<TransportManager>,
    session_manager: Arc<AdbSessionManager>,
    timeout: Duration,
}

impl ClientConnectionHandler {
    pub fn new(
        client_id: String,
        stream: TcpStream,
        peer_addr: SocketAddr,
        transport_manager: Arc<TransportManager>,
        session_manager: Arc<AdbSessionManager>,
        timeout: Duration,
    ) -> Self {
        Self {
            client_id,
            stream,
            peer_addr,
            transport_manager,
            session_manager,
            timeout,
        }
    }

    /// Handle the client connection
    pub async fn handle(mut self) -> Result<()> {
        // Set up connection timeout
        let connection_future = async {
            // Import the actual client handler implementation
            let mut client_handler = crate::server::ClientHandler::new(
                self.client_id.clone(),
                self.stream,
                Arc::clone(&self.transport_manager),
                Arc::clone(&self.session_manager),
            );

            client_handler.handle().await
        };

        // Apply timeout to the connection handling
        match tokio::time::timeout(self.timeout, connection_future).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    "Client connection timed out after {:?}: {}",
                    self.timeout,
                    self.peer_addr
                );
                Err(anyhow::anyhow!("Connection timeout"))
            }
        }
    }
}

/// Server statistics
#[derive(Debug, Clone)]
pub struct ServerStats {
    pub bind_addr: SocketAddr,
    pub active_connections: usize,
    pub max_connections: usize,
    pub connection_timeout: Duration,
}

/// Default ADB server configuration
pub struct AdbServerConfig {
    pub bind_addr: SocketAddr,
    pub connection_timeout: Duration,
    pub max_connections: usize,
    pub session_timeout: Duration,
    pub max_sessions: usize,
}

impl Default for AdbServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:5037".parse().unwrap(), // Default ADB port
            connection_timeout: Duration::from_secs(30),
            max_connections: 100,
            session_timeout: Duration::from_secs(300), // 5 minutes
            max_sessions: 50,
        }
    }
}

impl AdbServerConfig {
    /// Create config for development (more permissive)
    pub fn development() -> Self {
        Self {
            bind_addr: "127.0.0.1:5037".parse().unwrap(),
            connection_timeout: Duration::from_secs(60),
            max_connections: 50,
            session_timeout: Duration::from_secs(600), // 10 minutes
            max_sessions: 25,
        }
    }

    /// Create config for production (more restrictive)
    pub fn production() -> Self {
        Self {
            bind_addr: "0.0.0.0:5037".parse().unwrap(), // Listen on all interfaces
            connection_timeout: Duration::from_secs(15),
            max_connections: 200,
            session_timeout: Duration::from_secs(180), // 3 minutes
            max_sessions: 100,
        }
    }

    /// Create server from this configuration
    pub fn build_server(
        &self,
        transport_manager: Arc<TransportManager>,
    ) -> Result<AdbTcpServer> {
        let session_manager = Arc::new(AdbSessionManager::with_config(
            Arc::clone(&transport_manager),
            self.session_timeout,
            self.max_sessions,
        ));

        Ok(AdbTcpServer::with_config(
            self.bind_addr,
            transport_manager,
            session_manager,
            self.connection_timeout,
            self.max_connections,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportManager;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_server_creation() {
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0); // Use port 0 for testing
        let transport_manager = Arc::new(TransportManager::new());
        let session_manager = Arc::new(AdbSessionManager::new(transport_manager.clone()));

        let server = AdbTcpServer::new(bind_addr, transport_manager, session_manager);

        let stats = server.get_stats().await;
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.max_connections, 100);
    }

    #[tokio::test]
    async fn test_server_config() {
        let config = AdbServerConfig::default();
        assert_eq!(config.bind_addr.port(), 5037);
        assert_eq!(config.max_connections, 100);

        let dev_config = AdbServerConfig::development();
        assert_eq!(dev_config.connection_timeout, Duration::from_secs(60));

        let prod_config = AdbServerConfig::production();
        assert_eq!(prod_config.bind_addr.ip(), IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }

    #[tokio::test]
    async fn test_config_build_server() {
        let transport_manager = Arc::new(TransportManager::new());
        let config = AdbServerConfig::development();

        let server = config.build_server(transport_manager).unwrap();
        let stats = server.get_stats().await;

        assert_eq!(stats.max_connections, 50); // Development config
        assert_eq!(stats.connection_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_server_bind_addresses() {
        // Test various bind address configurations
        let localhost_v4: SocketAddr = "127.0.0.1:5037".parse().unwrap();
        assert_eq!(localhost_v4.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));

        let any_v4: SocketAddr = "0.0.0.0:5037".parse().unwrap();
        assert_eq!(any_v4.ip(), IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let localhost_v6: SocketAddr = "[::1]:5037".parse().unwrap();
        assert_eq!(localhost_v6.ip(), IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
    }
}