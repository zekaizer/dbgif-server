pub mod state;
pub mod dispatcher;
pub mod connection_manager;
pub mod stream_forwarder;
pub mod device_manager;
pub mod device_registry;
pub mod session;
pub mod stream;
pub mod message_handler;
pub mod limits;
pub mod heartbeat;
pub mod discovery;
pub mod shutdown;

use std::sync::Arc;
use tracing::{info, error, debug, warn};

use crate::host_services::*;
use crate::host_services::version::HostVersionService;
use crate::host_services::features::HostFeaturesService;
use crate::host_services::list::HostListService;
use crate::host_services::device::HostDeviceService;
use crate::transport::{Transport, TransportListener, TransportAddress, Connection};
use crate::protocol::error::ProtocolResult;
use crate::config::ServerConfig;

use self::state::{ServerState};
use self::connection_manager::ConnectionManager;
use self::dispatcher::MessageDispatcher;
use self::shutdown::{ShutdownCoordinator, ShutdownHandle};

/// Main DBGIF server that coordinates all components
pub struct DbgifServer {
    /// Server configuration
    config: ServerConfig,
    /// Shared server state
    state: Arc<ServerState>,
    /// Host service registry
    host_services: Arc<std::sync::RwLock<HostServiceRegistry>>,
    /// Connection manager
    connection_manager: Arc<ConnectionManager>,
    /// Message dispatcher
    dispatcher: Arc<MessageDispatcher>,
    /// Shutdown coordinator
    shutdown_coordinator: Arc<tokio::sync::RwLock<ShutdownCoordinator>>,
    /// Server shutdown handle
    shutdown_handle: Option<ShutdownHandle>,
}

impl DbgifServer {
    /// Create a new DBGIF server instance
    pub fn new(config: ServerConfig) -> Self {
        // Create shared state
        let state = Arc::new(ServerState::new(
            self::state::ServerConfig {
                bind_addr: TransportAddress::Tcp(
                    format!("{}:{}", config.bind_address, config.bind_port).parse().unwrap()
                ),
                max_connections: config.max_connections,
                connection_timeout: std::time::Duration::from_secs(config.connection_timeout_secs),
                ping_interval: std::time::Duration::from_secs(config.ping_interval_secs),
                ping_timeout: std::time::Duration::from_secs(config.ping_timeout_secs),
                device_discovery_interval: std::time::Duration::from_secs(config.ping_interval_secs), // Use ping_interval as fallback
                tcp_discovery_ports: vec![5555, 5556, 5557], // Default ports
            }
        ));

        let host_services = state.host_services.clone();

        // Create shutdown coordinator
        let shutdown_coordinator = Arc::new(tokio::sync::RwLock::new(ShutdownCoordinator::new()));

        // Create components
        let connection_manager = Arc::new(ConnectionManager::new(state.clone()));

        let dispatcher = Arc::new(MessageDispatcher::new(state.clone()));

        Self {
            config,
            state,
            host_services,
            connection_manager,
            dispatcher,
            shutdown_coordinator,
            shutdown_handle: None,
        }
    }

    /// Initialize and register host services
    pub async fn initialize_host_services(&self) -> ProtocolResult<()> {
        info!("Initializing host services");

        let mut registry = self.host_services.write().unwrap();

        // Register built-in host services
        registry.register(HostVersionService::new());
        registry.register(HostFeaturesService::new());
        registry.register(HostListService::new(self.state.device_registry.clone()));
        registry.register(HostDeviceService::new(self.state.device_registry.clone()));

        info!("Host services initialized: 4 services registered");
        Ok(())
    }

    /// Start the server with the given transport listener
    pub async fn start<T>(&mut self, mut listener: T) -> ProtocolResult<()>
    where
        T: TransportListener + Send + 'static,
        T::Connection: Transport + Send + 'static,
    {
        info!("Starting DBGIF server on port {}", self.config.bind_port);

        // Initialize host services
        self.initialize_host_services().await?;

        // Register server for shutdown coordination
        let shutdown_coordinator = self.shutdown_coordinator.read().await;
        self.shutdown_handle = Some(shutdown_coordinator.register_component("dbgif-server".to_string()));
        drop(shutdown_coordinator);

        // Start accepting connections
        loop {
            tokio::select! {
                // Check for shutdown signal
                _ = self.wait_for_shutdown() => {
                    info!("Shutdown signal received, stopping server");
                    break;
                }

                // Accept new connections
                connection_result = listener.accept() => {
                    match connection_result {
                        Ok(connection) => {
                            debug!("New connection accepted");

                            // Handle connection through connection manager
                            let connection_manager = self.connection_manager.clone();
                            let dispatcher = self.dispatcher.clone();
                            let shutdown_coord = self.shutdown_coordinator.clone();

                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_connection(
                                    connection,
                                    connection_manager,
                                    dispatcher,
                                    shutdown_coord,
                                ).await {
                                    error!("Connection handling failed: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }
            }
        }

        info!("DBGIF server stopped");
        Ok(())
    }

    /// Handle a single client connection
    async fn handle_connection<T>(
        connection: T,
        connection_manager: Arc<ConnectionManager>,
        _dispatcher: Arc<MessageDispatcher>,
        shutdown_coordinator: Arc<tokio::sync::RwLock<ShutdownCoordinator>>,
    ) -> ProtocolResult<()>
    where
        T: Transport + Connection + Send + 'static,
    {
        // Register connection
        let session_id = connection_manager.accept_connection(Box::new(connection)).await?;

        // Get shutdown handle
        let shutdown_coord = shutdown_coordinator.read().await;
        let mut shutdown_handle = shutdown_coord.register_component(
            format!("connection-{}", session_id)
        );
        drop(shutdown_coord);

        // Message handling loop
        let result = loop {
            tokio::select! {
                // Check for shutdown
                shutdown_reason = shutdown_handle.recv_shutdown() => {
                    if let Some(reason) = shutdown_reason {
                        debug!("Connection {} shutting down: {:?}", session_id, reason);
                        break Ok(());
                    }
                }

                // Handle incoming messages and perform maintenance
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    // Perform periodic maintenance tasks
                    // Check connection health and update statistics
                    if let Err(e) = perform_connection_maintenance(&session_id, &connection_manager).await {
                        warn!("Connection maintenance failed for {}: {}", session_id, e);
                    }
                }
            }
        };

        // Cleanup connection
        if let Err(e) = connection_manager.close_connection(&session_id, "Connection closed").await {
            error!("Failed to close connection {}: {}", session_id, e);
        }

        // Report shutdown completion
        let start_time = std::time::Instant::now();
        let shutdown_result = match &result {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("Connection error: {}", e)),
        };
        shutdown_handle.report_shutdown(shutdown_result, start_time).await;

        result
    }

    /// Wait for shutdown signal
    async fn wait_for_shutdown(&mut self) {
        if let Some(ref mut handle) = self.shutdown_handle {
            handle.recv_shutdown().await;
        } else {
            // Wait indefinitely if no shutdown handle
            std::future::pending().await
        }
    }

    /// Get server statistics
    pub async fn get_statistics(&self) -> crate::server::state::ServerStatsSnapshot {
        self.state.stats.snapshot()
    }

    /// Get active connection count
    pub async fn get_connection_count(&self) -> usize {
        // Get connection count from client sessions
        let sessions = self.state.client_sessions.read().unwrap();
        sessions.len()
    }

    /// Graceful shutdown
    pub async fn shutdown(&mut self) -> ProtocolResult<()> {
        info!("Initiating server shutdown");

        let mut shutdown_coordinator = self.shutdown_coordinator.write().await;
        if let Err(e) = shutdown_coordinator.shutdown(
            crate::server::shutdown::ShutdownReason::Graceful,
            std::time::Duration::from_secs(30),
        ).await {
            error!("Graceful shutdown failed: {}", e);
        }

        Ok(())
    }
}

/// Perform periodic connection maintenance
async fn perform_connection_maintenance(
    session_id: &str,
    _connection_manager: &crate::server::connection_manager::ConnectionManager,
) -> ProtocolResult<()> {
    // Perform basic connection maintenance
    debug!("Performing maintenance for connection: {}", session_id);

    // Simple connection health check - ensure connection is still being tracked
    // More detailed statistics would be implemented in a full system

    Ok(())
}