use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Coordinates graceful shutdown across all system components
pub struct ShutdownManager {
    /// Broadcast channel for shutdown signals
    shutdown_tx: broadcast::Sender<ShutdownSignal>,

    /// Components that need to be shut down in order
    components: Arc<RwLock<Vec<Box<dyn ShutdownComponent + Send + Sync>>>>,

    /// Timeout for graceful shutdown
    graceful_timeout: Duration,

    /// Force shutdown timeout (hard kill)
    force_timeout: Duration,

    /// Track shutdown state
    is_shutting_down: Arc<RwLock<bool>>,

    /// Shutdown reason
    shutdown_reason: Arc<Mutex<Option<ShutdownReason>>>,
}

impl ShutdownManager {
    /// Create new shutdown manager
    pub fn new(graceful_timeout: Duration, force_timeout: Duration) -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            shutdown_tx,
            components: Arc::new(RwLock::new(Vec::new())),
            graceful_timeout,
            force_timeout,
            is_shutting_down: Arc::new(RwLock::new(false)),
            shutdown_reason: Arc::new(Mutex::new(None)),
        }
    }

    /// Register a component for shutdown
    pub async fn register_component(&self, component: Box<dyn ShutdownComponent + Send + Sync>) {
        let mut components = self.components.write().await;
        components.push(component);
        debug!("Registered component for shutdown");
    }

    /// Get shutdown signal receiver
    pub fn subscribe(&self) -> broadcast::Receiver<ShutdownSignal> {
        self.shutdown_tx.subscribe()
    }

    /// Initiate graceful shutdown
    pub async fn shutdown(&self, reason: ShutdownReason) -> Result<()> {
        // Check if already shutting down
        {
            let mut is_shutting_down = self.is_shutting_down.write().await;
            if *is_shutting_down {
                warn!("Shutdown already in progress");
                return Ok(());
            }
            *is_shutting_down = true;
        }

        // Store shutdown reason
        {
            let mut shutdown_reason = self.shutdown_reason.lock().await;
            *shutdown_reason = Some(reason.clone());
        }

        info!("Initiating graceful shutdown: {:?}", reason);

        // Send shutdown signal to all subscribers
        let signal = ShutdownSignal::new(reason.clone(), self.graceful_timeout);
        if let Err(e) = self.shutdown_tx.send(signal) {
            warn!("Failed to broadcast shutdown signal: {}", e);
        }

        // Give components time to receive and start processing shutdown
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Perform graceful shutdown with timeout
        let shutdown_result = timeout(
            self.graceful_timeout,
            self.shutdown_components()
        ).await;

        match shutdown_result {
            Ok(Ok(())) => {
                info!("Graceful shutdown completed successfully");
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Graceful shutdown failed: {}", e);
                self.force_shutdown().await?;
                Err(e)
            }
            Err(_) => {
                warn!("Graceful shutdown timed out, forcing shutdown");
                self.force_shutdown().await?;
                Err(anyhow::anyhow!("Shutdown timed out"))
            }
        }
    }

    /// Force shutdown (hard kill)
    async fn force_shutdown(&self) -> Result<()> {
        info!("Forcing immediate shutdown");

        let force_result = timeout(
            self.force_timeout,
            self.force_shutdown_components()
        ).await;

        match force_result {
            Ok(Ok(())) => {
                info!("Force shutdown completed");
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Force shutdown failed: {}", e);
                Err(e)
            }
            Err(_) => {
                error!("Force shutdown timed out - some components may not have stopped");
                Ok(()) // Continue anyway
            }
        }
    }

    /// Shutdown all registered components gracefully
    async fn shutdown_components(&self) -> Result<()> {
        let components = self.components.read().await;

        info!("Shutting down {} components", components.len());

        // Shutdown components in reverse registration order (LIFO)
        for (i, component) in components.iter().rev().enumerate() {
            debug!("Shutting down component {} of {}", i + 1, components.len());

            if let Err(e) = component.shutdown().await {
                error!("Component shutdown failed: {}", e);
                // Continue with other components
            }
        }

        Ok(())
    }

    /// Force shutdown all components
    async fn force_shutdown_components(&self) -> Result<()> {
        let components = self.components.read().await;

        info!("Force shutting down {} components", components.len());

        // Force shutdown in reverse order
        for (i, component) in components.iter().rev().enumerate() {
            debug!("Force shutting down component {} of {}", i + 1, components.len());

            if let Err(e) = component.force_shutdown().await {
                error!("Component force shutdown failed: {}", e);
                // Continue with other components
            }
        }

        Ok(())
    }

    /// Check if shutdown is in progress
    pub async fn is_shutting_down(&self) -> bool {
        *self.is_shutting_down.read().await
    }

    /// Get shutdown reason if available
    pub async fn shutdown_reason(&self) -> Option<ShutdownReason> {
        self.shutdown_reason.lock().await.clone()
    }

    /// Wait for system signals and initiate shutdown
    pub async fn wait_for_signal(&self) -> Result<()> {
        let reason = wait_for_system_signal().await;
        self.shutdown(reason).await
    }
}

/// Component that can be gracefully shut down
#[async_trait::async_trait]
pub trait ShutdownComponent {
    /// Component name for logging
    fn name(&self) -> &'static str;

    /// Graceful shutdown
    async fn shutdown(&self) -> Result<()>;

    /// Force shutdown (immediate termination)
    async fn force_shutdown(&self) -> Result<()> {
        // Default implementation calls regular shutdown
        self.shutdown().await
    }
}

/// Shutdown signal sent to components
#[derive(Debug, Clone)]
pub struct ShutdownSignal {
    pub reason: ShutdownReason,
    pub graceful_timeout: Duration,
    pub timestamp: std::time::Instant,
}

impl ShutdownSignal {
    pub fn new(reason: ShutdownReason, graceful_timeout: Duration) -> Self {
        Self {
            reason,
            graceful_timeout,
            timestamp: std::time::Instant::now(),
        }
    }
}

/// Reason for shutdown
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownReason {
    /// User requested shutdown (SIGTERM, SIGINT, Ctrl+C)
    UserRequested,
    /// System signal (other than user signals)
    SystemSignal(String),
    /// Application error
    ApplicationError(String),
    /// Configuration reload
    ConfigReload,
    /// Administrative command
    AdminCommand,
    /// Resource exhaustion
    ResourceExhaustion(String),
    /// Unexpected error
    InternalError(String),
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShutdownReason::UserRequested => write!(f, "User requested"),
            ShutdownReason::SystemSignal(signal) => write!(f, "System signal: {}", signal),
            ShutdownReason::ApplicationError(err) => write!(f, "Application error: {}", err),
            ShutdownReason::ConfigReload => write!(f, "Configuration reload"),
            ShutdownReason::AdminCommand => write!(f, "Administrative command"),
            ShutdownReason::ResourceExhaustion(resource) => write!(f, "Resource exhaustion: {}", resource),
            ShutdownReason::InternalError(err) => write!(f, "Internal error: {}", err),
        }
    }
}

/// Wait for system signals
async fn wait_for_system_signal() -> ShutdownReason {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate())
            .expect("Failed to register SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt())
            .expect("Failed to register SIGINT handler");
        let mut sigquit = signal(SignalKind::quit())
            .expect("Failed to register SIGQUIT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
                ShutdownReason::SystemSignal("SIGTERM".to_string())
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
                ShutdownReason::UserRequested
            }
            _ = sigquit.recv() => {
                info!("Received SIGQUIT");
                ShutdownReason::SystemSignal("SIGQUIT".to_string())
            }
        }
    }

    #[cfg(windows)]
    {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Received Ctrl+C");
                ShutdownReason::UserRequested
            }
            Err(e) => {
                error!("Failed to listen for Ctrl+C: {}", e);
                ShutdownReason::InternalError(format!("Signal handler error: {}", e))
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Fallback for other platforms
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Received interrupt signal");
                ShutdownReason::UserRequested
            }
            Err(e) => {
                error!("Failed to listen for signals: {}", e);
                ShutdownReason::InternalError(format!("Signal handler error: {}", e))
            }
        }
    }
}

/// Helper function to create a shutdown-aware task
pub async fn spawn_shutdown_aware_task<F, T>(
    shutdown_rx: &mut broadcast::Receiver<ShutdownSignal>,
    task_name: &str,
    future: F,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    tokio::select! {
        result = future => {
            debug!("Task '{}' completed normally", task_name);
            result
        }
        shutdown_signal = shutdown_rx.recv() => {
            match shutdown_signal {
                Ok(signal) => {
                    info!("Task '{}' received shutdown signal: {}", task_name, signal.reason);
                    Err(anyhow::anyhow!("Task interrupted by shutdown: {}", signal.reason))
                }
                Err(e) => {
                    error!("Task '{}' failed to receive shutdown signal: {}", task_name, e);
                    Err(anyhow::anyhow!("Shutdown signal error: {}", e))
                }
            }
        }
    }
}

/// Helper to create a component from a closure
pub struct ClosureComponent {
    name: &'static str,
    shutdown_fn: Box<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync>,
    force_shutdown_fn: Option<Box<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync>>,
}

impl ClosureComponent {
    pub fn new<F, Fut>(name: &'static str, shutdown_fn: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            name,
            shutdown_fn: Box::new(move || Box::pin(shutdown_fn())),
            force_shutdown_fn: None,
        }
    }

    pub fn with_force_shutdown<F, G, Fut1, Fut2>(
        name: &'static str,
        shutdown_fn: F,
        force_shutdown_fn: G,
    ) -> Self
    where
        F: Fn() -> Fut1 + Send + Sync + 'static,
        G: Fn() -> Fut2 + Send + Sync + 'static,
        Fut1: std::future::Future<Output = Result<()>> + Send + 'static,
        Fut2: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            name,
            shutdown_fn: Box::new(move || Box::pin(shutdown_fn())),
            force_shutdown_fn: Some(Box::new(move || Box::pin(force_shutdown_fn()))),
        }
    }
}

#[async_trait::async_trait]
impl ShutdownComponent for ClosureComponent {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn shutdown(&self) -> Result<()> {
        debug!("Shutting down component: {}", self.name);
        (self.shutdown_fn)().await
    }

    async fn force_shutdown(&self) -> Result<()> {
        debug!("Force shutting down component: {}", self.name);
        if let Some(force_fn) = &self.force_shutdown_fn {
            force_fn().await
        } else {
            // Fallback to regular shutdown
            (self.shutdown_fn)().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_shutdown_manager_creation() {
        let manager = ShutdownManager::new(
            Duration::from_secs(5),
            Duration::from_secs(1)
        );

        assert!(!manager.is_shutting_down().await);
        assert!(manager.shutdown_reason().await.is_none());
    }

    #[tokio::test]
    async fn test_component_registration() {
        let manager = ShutdownManager::new(
            Duration::from_secs(5),
            Duration::from_secs(1)
        );

        let component = ClosureComponent::new("test", || async { Ok(()) });
        manager.register_component(Box::new(component)).await;

        // Component count is not directly accessible, but registration should not fail
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let manager = ShutdownManager::new(
            Duration::from_secs(1),
            Duration::from_millis(500)
        );

        let shutdown_called = Arc::new(AtomicBool::new(false));
        let shutdown_called_clone = Arc::clone(&shutdown_called);

        let component = ClosureComponent::new("test", move || {
            let flag = Arc::clone(&shutdown_called_clone);
            async move {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            }
        });

        manager.register_component(Box::new(component)).await;

        // Perform shutdown
        let result = manager.shutdown(ShutdownReason::UserRequested).await;
        assert!(result.is_ok());

        // Verify component was shut down
        assert!(shutdown_called.load(Ordering::SeqCst));
        assert!(manager.is_shutting_down().await);

        let reason = manager.shutdown_reason().await;
        assert_eq!(reason, Some(ShutdownReason::UserRequested));
    }

    #[tokio::test]
    async fn test_shutdown_signal() {
        let signal = ShutdownSignal::new(
            ShutdownReason::UserRequested,
            Duration::from_secs(5)
        );

        assert_eq!(signal.reason, ShutdownReason::UserRequested);
        assert_eq!(signal.graceful_timeout, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_shutdown_reason_display() {
        let reasons = vec![
            ShutdownReason::UserRequested,
            ShutdownReason::SystemSignal("SIGTERM".to_string()),
            ShutdownReason::ApplicationError("test error".to_string()),
            ShutdownReason::ConfigReload,
            ShutdownReason::AdminCommand,
            ShutdownReason::ResourceExhaustion("memory".to_string()),
            ShutdownReason::InternalError("internal".to_string()),
        ];

        for reason in reasons {
            let display = format!("{}", reason);
            assert!(!display.is_empty());
        }
    }

    #[tokio::test]
    async fn test_spawn_shutdown_aware_task() {
        let manager = ShutdownManager::new(
            Duration::from_secs(1),
            Duration::from_millis(500)
        );

        let mut shutdown_rx = manager.subscribe();

        // Test normal completion
        let result = spawn_shutdown_aware_task(
            &mut shutdown_rx,
            "test_task",
            async { Ok(42) }
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_closure_component() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let component = ClosureComponent::new("test", move || {
            let flag = Arc::clone(&called_clone);
            async move {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            }
        });

        assert_eq!(component.name(), "test");

        let result = component.shutdown().await;
        assert!(result.is_ok());
        assert!(called.load(Ordering::SeqCst));
    }
}