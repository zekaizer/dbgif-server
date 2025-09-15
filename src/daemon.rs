use crate::config::DaemonConfig;
use crate::server::AdbTcpServer;
use crate::session::AdbSessionManager;
use crate::transport::TransportManager;
use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// Main daemon orchestrator that coordinates all server components
pub struct AdbDaemon {
    config: DaemonConfig,
    transport_manager: Arc<TransportManager>,
    session_manager: Arc<AdbSessionManager>,
    tcp_server: Option<AdbTcpServer>,
    shutdown_sender: Option<mpsc::Sender<()>>,
    is_running: Arc<RwLock<bool>>,
}

impl AdbDaemon {
    /// Create new daemon with configuration
    pub fn new(config: DaemonConfig) -> Result<Self> {
        info!("Initializing ADB daemon with configuration");

        // Validate configuration
        config.validate().context("Configuration validation failed")?;

        // Initialize transport manager
        let transport_manager = Arc::new(TransportManager::new());

        // Initialize session manager
        let session_manager = Arc::new(AdbSessionManager::with_config(
            Arc::clone(&transport_manager),
            config.session.session_timeout,
            config.session.max_total_sessions,
        ));

        Ok(Self {
            config,
            transport_manager,
            session_manager,
            tcp_server: None,
            shutdown_sender: None,
            is_running: Arc::new(RwLock::new(false)),
        })
    }

    /// Start the daemon
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting ADB daemon");

        // Check if already running
        {
            let running = self.is_running.read().await;
            if *running {
                return Err(anyhow::anyhow!("Daemon is already running"));
            }
        }

        // Set running state
        {
            let mut running = self.is_running.write().await;
            *running = true;
        }

        // Initialize transport factories based on configuration
        self.initialize_transports().await?;

        // Start device discovery
        self.start_device_discovery().await?;

        // Create and start TCP server
        self.start_tcp_server().await?;

        // Set up signal handlers
        self.setup_signal_handlers().await?;

        info!("ADB daemon started successfully");
        Ok(())
    }

    /// Run the daemon (blocking)
    pub async fn run(&mut self) -> Result<()> {
        // Start the daemon
        self.start().await?;

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_sender = Some(shutdown_tx);

        info!("ADB daemon is now running");

        // Main daemon loop
        tokio::select! {
            // Wait for shutdown signal
            _ = shutdown_rx.recv() => {
                info!("Received shutdown signal");
            }

            // Handle system signals
            _ = self.wait_for_signals() => {
                info!("Received system signal, shutting down");
            }

            // Monitor daemon health
            _ = self.health_monitor() => {
                warn!("Health monitor detected issues, shutting down");
            }
        }

        // Perform graceful shutdown
        self.shutdown().await?;

        Ok(())
    }

    /// Initialize transport factories based on configuration
    async fn initialize_transports(&self) -> Result<()> {
        info!("Initializing transport factories");

        if self.config.transport.enable_tcp {
            info!("TCP transport enabled");
            // TCP factory is always available - no special initialization needed
        }

        if self.config.transport.enable_usb_device {
            info!("USB device transport enabled");
            // USB device factory initialization
            match self.transport_manager.initialize_usb_device_factory().await {
                Ok(()) => info!("USB device transport initialized"),
                Err(e) => warn!("Failed to initialize USB device transport: {}", e),
            }
        }

        if self.config.transport.enable_usb_bridge {
            info!("USB bridge transport enabled");
            // USB bridge factory initialization
            match self.transport_manager.initialize_usb_bridge_factory().await {
                Ok(()) => info!("USB bridge transport initialized"),
                Err(e) => warn!("Failed to initialize USB bridge transport: {}", e),
            }
        }

        Ok(())
    }

    /// Start device discovery background task
    async fn start_device_discovery(&self) -> Result<()> {
        info!("Starting device discovery");

        let transport_manager = Arc::clone(&self.transport_manager);
        let poll_interval = self.config.transport.usb_poll_interval;
        let enable_hotplug = self.config.transport.enable_hotplug;

        tokio::spawn(async move {
            device_discovery_task(transport_manager, poll_interval, enable_hotplug).await;
        });

        Ok(())
    }

    /// Create and start TCP server
    async fn start_tcp_server(&mut self) -> Result<()> {
        info!("Starting TCP server on {}", self.config.server.bind_address);

        let mut tcp_server = AdbTcpServer::with_config(
            self.config.server.bind_address,
            Arc::clone(&self.transport_manager),
            Arc::clone(&self.session_manager),
            self.config.server.connection_timeout,
            self.config.server.max_connections,
        );

        // Get shutdown sender before moving server to task
        let shutdown_sender = tcp_server.shutdown_sender();

        // Start server in background task
        let _server_task = tokio::spawn(async move {
            if let Err(e) = tcp_server.start().await {
                error!("TCP server error: {}", e);
            }
        });

        // Store shutdown sender for graceful shutdown
        if let Some(_sender) = shutdown_sender {
            // We'll use this for shutdown coordination
            // For now, we'll handle it in the shutdown method
        }

        info!("TCP server started");
        Ok(())
    }

    /// Set up signal handlers
    async fn setup_signal_handlers(&self) -> Result<()> {
        debug!("Setting up signal handlers");

        #[cfg(unix)]
        {
            // On Unix systems, handle SIGTERM and SIGINT
            // This will be handled in wait_for_signals method
        }

        #[cfg(windows)]
        {
            // On Windows, handle Ctrl+C
            // This will be handled in wait_for_signals method
        }

        Ok(())
    }

    /// Wait for system signals
    async fn wait_for_signals(&self) {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};

            let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
            let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");

            tokio::select! {
                _ = sigterm.recv() => {
                    info!("Received SIGTERM");
                }
                _ = sigint.recv() => {
                    info!("Received SIGINT");
                }
            }
        }

        #[cfg(windows)]
        {
            match signal::ctrl_c().await {
                Ok(()) => info!("Received Ctrl+C"),
                Err(e) => error!("Failed to listen for Ctrl+C: {}", e),
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            // Fallback for other platforms
            match signal::ctrl_c().await {
                Ok(()) => info!("Received interrupt signal"),
                Err(e) => error!("Failed to listen for signals: {}", e),
            }
        }
    }

    /// Health monitoring loop
    async fn health_monitor(&self) -> Result<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            // Check if daemon is still supposed to be running
            {
                let running = self.is_running.read().await;
                if !*running {
                    break;
                }
            }

            // Check transport manager health
            let transport_stats = self.transport_manager.get_stats().await;
            debug!("Transport stats: active={}, discovered={}",
                   transport_stats.active_transports,
                   transport_stats.discovered_devices);

            // Check session manager health
            let session_stats = self.session_manager.get_stats().await;
            debug!("Session stats: active={}, total_created={}",
                   session_stats.active_sessions,
                   session_stats.total_sessions_created);

            // Check memory usage and other health metrics
            // For now, just log that monitoring is active
            debug!("Health check completed");
        }

        Ok(())
    }

    /// Graceful shutdown
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Starting graceful shutdown");

        // Set running state to false
        {
            let mut running = self.is_running.write().await;
            *running = false;
        }

        // Send shutdown signal to TCP server if available
        if let Some(sender) = &self.shutdown_sender {
            if let Err(e) = sender.send(()).await {
                warn!("Failed to send shutdown signal to TCP server: {}", e);
            }
        }

        // Wait for server shutdown timeout
        let shutdown_timeout = self.config.server.shutdown_timeout;

        if self.config.server.graceful_shutdown {
            info!("Waiting for graceful shutdown (timeout: {:?})", shutdown_timeout);

            // Give components time to shut down gracefully
            tokio::time::timeout(shutdown_timeout, async {
                // Wait for active sessions to complete
                self.session_manager.shutdown_all_sessions().await;

                // Disconnect all transports
                self.transport_manager.disconnect_all().await;

                // Small delay to ensure everything is cleaned up
                sleep(Duration::from_millis(100)).await;
            }).await.unwrap_or_else(|_| {
                warn!("Graceful shutdown timed out, forcing shutdown");
            });
        } else {
            info!("Performing immediate shutdown");
        }

        info!("ADB daemon shutdown completed");
        Ok(())
    }

    /// Get daemon statistics
    pub async fn get_stats(&self) -> DaemonStats {
        let running = *self.is_running.read().await;
        let transport_stats = self.transport_manager.get_stats().await;
        let session_stats = self.session_manager.get_stats().await;

        DaemonStats {
            is_running: running,
            bind_address: self.config.server.bind_address,
            active_transports: transport_stats.active_transports,
            discovered_devices: transport_stats.discovered_devices,
            active_sessions: session_stats.active_sessions,
            total_sessions_created: session_stats.total_sessions_created,
            uptime: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default(),
        }
    }

    /// Request shutdown (can be called from signal handlers)
    pub async fn request_shutdown(&self) -> Result<()> {
        info!("Shutdown requested");

        if let Some(sender) = &self.shutdown_sender {
            sender.send(()).await
                .context("Failed to send shutdown request")?;
        }

        Ok(())
    }
}

/// Daemon statistics
#[derive(Debug, Clone)]
pub struct DaemonStats {
    pub is_running: bool,
    pub bind_address: std::net::SocketAddr,
    pub active_transports: usize,
    pub discovered_devices: usize,
    pub active_sessions: usize,
    pub total_sessions_created: usize,
    pub uptime: Duration,
}

/// Background task for device discovery
async fn device_discovery_task(
    transport_manager: Arc<TransportManager>,
    poll_interval: Duration,
    enable_hotplug: bool,
) {
    info!("Device discovery task started (poll_interval={:?}, hotplug={})",
          poll_interval, enable_hotplug);

    let mut interval = tokio::time::interval(poll_interval);

    loop {
        interval.tick().await;

        // Perform device discovery
        match transport_manager.discover_all_devices().await {
            Ok(devices) => {
                debug!("Discovered {} devices", devices.len());

                // Log device changes
                for device in &devices {
                    debug!("Device: {} ({})", device.device_id, device.display_name);
                }
            }
            Err(e) => {
                warn!("Device discovery failed: {}", e);
            }
        }

        // TODO: Handle hotplug events if enabled
        if enable_hotplug {
            // For now, we rely on polling
            // In a more advanced implementation, we would listen for hotplug events
        }
    }
}

/// Builder for daemon configuration and setup
pub struct DaemonBuilder {
    config: Option<DaemonConfig>,
}

impl DaemonBuilder {
    /// Create new daemon builder
    pub fn new() -> Self {
        Self { config: None }
    }

    /// Set configuration
    pub fn with_config(mut self, config: DaemonConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Use default configuration
    pub fn with_default_config(mut self) -> Self {
        self.config = Some(DaemonConfig::default());
        self
    }

    /// Use development configuration
    pub fn with_development_config(mut self) -> Self {
        self.config = Some(DaemonConfig::development());
        self
    }

    /// Use production configuration
    pub fn with_production_config(mut self) -> Self {
        self.config = Some(DaemonConfig::production());
        self
    }

    /// Load configuration from file
    pub fn with_config_file(mut self, path: &std::path::PathBuf) -> Result<Self> {
        let config = DaemonConfig::load_from_file(path)?;
        self.config = Some(config);
        Ok(self)
    }

    /// Build the daemon
    pub fn build(self) -> Result<AdbDaemon> {
        let config = self.config
            .ok_or_else(|| anyhow::anyhow!("Configuration not provided"))?;

        AdbDaemon::new(config)
    }
}

impl Default for DaemonBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_daemon_creation() {
        let config = DaemonConfig::development();
        let daemon = AdbDaemon::new(config);
        assert!(daemon.is_ok());
    }

    #[tokio::test]
    async fn test_daemon_builder() {
        let daemon = DaemonBuilder::new()
            .with_development_config()
            .build();

        assert!(daemon.is_ok());
    }

    #[tokio::test]
    async fn test_daemon_stats() {
        let config = DaemonConfig::development();
        let daemon = AdbDaemon::new(config).unwrap();

        let stats = daemon.get_stats().await;
        assert!(!stats.is_running); // Should not be running yet
        assert_eq!(stats.active_sessions, 0);
    }

    #[test]
    fn test_daemon_builder_default() {
        let builder = DaemonBuilder::default();
        // Should be able to create a builder
        assert!(builder.config.is_none());
    }
}