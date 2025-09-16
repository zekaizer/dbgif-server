use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    signal,
    sync::{broadcast, mpsc, oneshot, watch},
    time::sleep,
};
use tracing::{debug, error, info, warn};

/// Shutdown signal types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// Graceful shutdown requested by user (SIGTERM, Ctrl+C)
    Graceful,
    /// Emergency shutdown due to critical error
    Emergency,
    /// Shutdown due to configuration reload
    Reload,
    /// Shutdown due to resource exhaustion
    ResourceExhaustion,
}

/// Shutdown phase tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownPhase {
    /// Normal operation
    Running,
    /// Shutdown initiated
    Initiated,
    /// Stopping new connections
    StoppingConnections,
    /// Draining existing connections
    DrainingConnections,
    /// Cleaning up resources
    Cleanup,
    /// Shutdown complete
    Complete,
    /// Shutdown failed
    Failed,
}

/// Shutdown error types
#[derive(thiserror::Error, Debug)]
pub enum ShutdownError {
    #[error("Shutdown timeout exceeded: {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    #[error("Component shutdown failed: {component}: {error}")]
    ComponentError { component: String, error: String },

    #[error("Signal handling error: {error}")]
    SignalError { error: String },

    #[error("Already shutting down")]
    AlreadyShuttingDown,

    #[error("Resource cleanup failed: {resource}: {error}")]
    CleanupError { resource: String, error: String },
}

/// Result type for shutdown operations
pub type ShutdownResult<T> = Result<T, ShutdownError>;

/// Shutdown coordinator that manages graceful shutdown of the entire server
#[derive(Debug)]
pub struct ShutdownCoordinator {
    /// Shutdown state tracking
    state: Arc<ShutdownState>,
    /// Broadcast sender for shutdown signals
    shutdown_tx: broadcast::Sender<ShutdownReason>,
    /// Phase change notifications
    phase_tx: watch::Sender<ShutdownPhase>,
    /// Component completion notifications
    completion_rx: mpsc::Receiver<ComponentShutdown>,
    completion_tx: mpsc::Sender<ComponentShutdown>,
}

/// Internal shutdown state
#[derive(Debug)]
struct ShutdownState {
    /// Whether shutdown is in progress
    is_shutting_down: AtomicBool,
    /// Current shutdown phase
    current_phase: Arc<std::sync::RwLock<ShutdownPhase>>,
    /// Shutdown start time
    shutdown_start: Arc<std::sync::RwLock<Option<Instant>>>,
    /// Registered components
    components: Arc<std::sync::RwLock<Vec<String>>>,
}

/// Component shutdown notification
#[derive(Debug, Clone)]
pub struct ComponentShutdown {
    pub component_name: String,
    pub shutdown_result: Result<(), String>,
    pub shutdown_duration: Duration,
}

/// Shutdown handle for components
#[derive(Debug)]
pub struct ShutdownHandle {
    /// Receives shutdown signals
    shutdown_rx: broadcast::Receiver<ShutdownReason>,
    /// Watches phase changes
    phase_rx: watch::Receiver<ShutdownPhase>,
    /// Component name
    component_name: String,
    /// Completion notification sender
    completion_tx: mpsc::Sender<ComponentShutdown>,
}

impl ShutdownCoordinator {
    /// Create a new shutdown coordinator
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        let (phase_tx, _) = watch::channel(ShutdownPhase::Running);
        let (completion_tx, completion_rx) = mpsc::channel(32);

        let state = Arc::new(ShutdownState {
            is_shutting_down: AtomicBool::new(false),
            current_phase: Arc::new(std::sync::RwLock::new(ShutdownPhase::Running)),
            shutdown_start: Arc::new(std::sync::RwLock::new(None)),
            components: Arc::new(std::sync::RwLock::new(Vec::new())),
        });

        Self {
            state,
            shutdown_tx,
            phase_tx,
            completion_rx,
            completion_tx,
        }
    }

    /// Register a component for shutdown coordination
    pub fn register_component(&self, component_name: String) -> ShutdownHandle {
        // Add to component list
        {
            let mut components = self.state.components.write().unwrap();
            components.push(component_name.clone());
        }

        // Create handle
        ShutdownHandle {
            shutdown_rx: self.shutdown_tx.subscribe(),
            phase_rx: self.phase_tx.subscribe(),
            component_name,
            completion_tx: self.completion_tx.clone(),
        }
    }

    /// Initiate graceful shutdown
    pub async fn shutdown(&mut self, reason: ShutdownReason, timeout: Duration) -> ShutdownResult<()> {
        info!("Initiating graceful shutdown: reason={:?}, timeout={:?}", reason, timeout);

        // Check if already shutting down
        if self.state.is_shutting_down.swap(true, Ordering::SeqCst) {
            return Err(ShutdownError::AlreadyShuttingDown);
        }

        // Record shutdown start time
        {
            let mut start_time = self.state.shutdown_start.write().unwrap();
            *start_time = Some(Instant::now());
        }

        // Execute shutdown with timeout
        match tokio::time::timeout(timeout, self.execute_shutdown(reason)).await {
            Ok(result) => {
                info!("Graceful shutdown completed successfully");
                result
            }
            Err(_) => {
                error!("Graceful shutdown timed out after {:?}", timeout);
                self.set_phase(ShutdownPhase::Failed).await;
                Err(ShutdownError::Timeout {
                    timeout_secs: timeout.as_secs(),
                })
            }
        }
    }

    /// Execute shutdown sequence
    async fn execute_shutdown(&mut self, reason: ShutdownReason) -> ShutdownResult<()> {
        info!("Starting shutdown sequence");

        // Phase 1: Initiate shutdown
        self.set_phase(ShutdownPhase::Initiated).await;
        if let Err(e) = self.shutdown_tx.send(reason) {
            warn!("Failed to broadcast shutdown signal: {}", e);
        }

        // Phase 2: Stop accepting new connections
        self.set_phase(ShutdownPhase::StoppingConnections).await;
        sleep(Duration::from_millis(100)).await; // Give components time to react

        // Phase 3: Drain existing connections
        self.set_phase(ShutdownPhase::DrainingConnections).await;
        self.wait_for_components().await?;

        // Phase 4: Cleanup resources
        self.set_phase(ShutdownPhase::Cleanup).await;
        sleep(Duration::from_millis(50)).await; // Final cleanup time

        // Phase 5: Complete
        self.set_phase(ShutdownPhase::Complete).await;

        let shutdown_duration = {
            let start_time = self.state.shutdown_start.read().unwrap();
            start_time.map(|t| t.elapsed()).unwrap_or_default()
        };

        info!("Shutdown sequence completed in {:?}", shutdown_duration);
        Ok(())
    }

    /// Wait for all registered components to report shutdown completion
    async fn wait_for_components(&mut self) -> ShutdownResult<()> {
        let registered_components = {
            let components = self.state.components.read().unwrap();
            components.len()
        };

        if registered_components == 0 {
            info!("No components registered for shutdown");
            return Ok(());
        }

        info!("Waiting for {} components to shutdown", registered_components);

        let mut completed_components = 0;
        let mut failed_components = Vec::new();

        while completed_components < registered_components {
            match self.completion_rx.recv().await {
                Some(completion) => {
                    completed_components += 1;
                    debug!(
                        "Component '{}' shutdown completed in {:?}: {:?}",
                        completion.component_name,
                        completion.shutdown_duration,
                        completion.shutdown_result
                    );

                    if let Err(error) = &completion.shutdown_result {
                        failed_components.push((completion.component_name.clone(), error.clone()));
                    }
                }
                None => {
                    warn!("Component completion channel closed unexpectedly");
                    break;
                }
            }
        }

        if !failed_components.is_empty() {
            for (component, error) in &failed_components {
                error!("Component '{}' shutdown failed: {}", component, error);
            }
            return Err(ShutdownError::ComponentError {
                component: failed_components[0].0.clone(),
                error: failed_components[0].1.clone(),
            });
        }

        info!("All {} components shutdown successfully", completed_components);
        Ok(())
    }

    /// Set shutdown phase and notify watchers
    async fn set_phase(&self, phase: ShutdownPhase) {
        debug!("Shutdown phase transition: {:?}", phase);

        // Update internal state
        {
            let mut current_phase = self.state.current_phase.write().unwrap();
            *current_phase = phase;
        }

        // Notify watchers
        if let Err(e) = self.phase_tx.send(phase) {
            warn!("Failed to broadcast phase change: {}", e);
        }
    }

    /// Get current shutdown phase
    pub fn current_phase(&self) -> ShutdownPhase {
        let current_phase = self.state.current_phase.read().unwrap();
        *current_phase
    }

    /// Check if shutdown is in progress
    pub fn is_shutting_down(&self) -> bool {
        self.state.is_shutting_down.load(Ordering::SeqCst)
    }

    /// Get shutdown duration if shutdown has started
    pub fn shutdown_duration(&self) -> Option<Duration> {
        let start_time = self.state.shutdown_start.read().unwrap();
        start_time.map(|t| t.elapsed())
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownHandle {
    /// Wait for shutdown signal
    pub async fn recv_shutdown(&mut self) -> Option<ShutdownReason> {
        match self.shutdown_rx.recv().await {
            Ok(reason) => Some(reason),
            Err(broadcast::error::RecvError::Closed) => None,
            Err(broadcast::error::RecvError::Lagged(count)) => {
                warn!("Component '{}' shutdown receiver lagged by {} messages", self.component_name, count);
                // Try to receive the latest signal
                match self.shutdown_rx.recv().await {
                    Ok(reason) => Some(reason),
                    Err(_) => None,
                }
            }
        }
    }

    /// Watch for phase changes
    pub async fn wait_for_phase(&mut self, phase: ShutdownPhase) -> bool {
        loop {
            if *self.phase_rx.borrow() == phase {
                return true;
            }

            if self.phase_rx.changed().await.is_err() {
                return false;
            }
        }
    }

    /// Get current shutdown phase
    pub fn current_phase(&self) -> ShutdownPhase {
        *self.phase_rx.borrow()
    }

    /// Report component shutdown completion
    pub async fn report_shutdown(&self, result: Result<(), String>, start_time: Instant) {
        let completion = ComponentShutdown {
            component_name: self.component_name.clone(),
            shutdown_result: result,
            shutdown_duration: start_time.elapsed(),
        };

        if let Err(e) = self.completion_tx.send(completion).await {
            error!("Failed to report component shutdown: {}", e);
        }
    }

    /// Component name getter
    pub fn component_name(&self) -> &str {
        &self.component_name
    }
}

/// System signal handler for graceful shutdown
pub struct SignalHandler {
    coordinator: Arc<tokio::sync::Mutex<ShutdownCoordinator>>,
    shutdown_timeout: Duration,
}

impl SignalHandler {
    /// Create a new signal handler
    pub fn new(coordinator: ShutdownCoordinator, shutdown_timeout: Duration) -> Self {
        Self {
            coordinator: Arc::new(tokio::sync::Mutex::new(coordinator)),
            shutdown_timeout,
        }
    }

    /// Start listening for system signals
    pub async fn listen_for_signals(self) -> ShutdownResult<()> {
        info!("Starting signal handler");

        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .map_err(|e| ShutdownError::SignalError {
                    error: format!("Failed to install Ctrl+C handler: {}", e),
                })
        };

        #[cfg(unix)]
        let terminate = async {
            let mut term_stream = signal::unix::signal(signal::unix::SignalKind::terminate())
                .map_err(|e| ShutdownError::SignalError {
                    error: format!("Failed to install SIGTERM handler: {}", e),
                })?;
            term_stream.recv().await;
            Ok(())
        };

        #[cfg(not(unix))]
        let terminate = async {
            // On non-Unix systems, just wait indefinitely
            std::future::pending::<ShutdownResult<()>>().await
        };

        tokio::select! {
            result = ctrl_c => {
                result?;
                info!("Received Ctrl+C signal");
                self.initiate_shutdown(ShutdownReason::Graceful).await
            }
            result = terminate => {
                result?;
                info!("Received SIGTERM signal");
                self.initiate_shutdown(ShutdownReason::Graceful).await
            }
        }
    }

    /// Initiate shutdown through the coordinator
    async fn initiate_shutdown(&self, reason: ShutdownReason) -> ShutdownResult<()> {
        let mut coordinator = self.coordinator.lock().await;
        coordinator.shutdown(reason, self.shutdown_timeout).await
    }
}

/// Shutdown utilities
pub mod utils {
    use super::*;

    /// Create a oneshot channel for component shutdown coordination
    pub fn create_shutdown_channel() -> (oneshot::Sender<()>, oneshot::Receiver<()>) {
        oneshot::channel()
    }

    /// Wait for either shutdown signal or operation completion
    pub async fn wait_for_shutdown_or_completion<T>(
        shutdown_handle: &mut ShutdownHandle,
        operation: impl std::future::Future<Output = T>,
    ) -> Either<ShutdownReason, T> {
        tokio::select! {
            reason = shutdown_handle.recv_shutdown() => {
                match reason {
                    Some(r) => Either::Shutdown(r),
                    None => Either::Shutdown(ShutdownReason::Emergency),
                }
            }
            result = operation => Either::Completed(result),
        }
    }

    /// Result type for shutdown or completion
    pub enum Either<A, B> {
        Shutdown(A),
        Completed(B),
    }

    /// Execute graceful component shutdown with timeout
    pub async fn graceful_component_shutdown<F, Fut>(
        component_name: &str,
        shutdown_fn: F,
        timeout: Duration,
    ) -> Result<(), String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        match tokio::time::timeout(timeout, shutdown_fn()).await {
            Ok(result) => result,
            Err(_) => Err(format!("Component '{}' shutdown timed out after {:?}", component_name, timeout)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shutdown_coordinator_creation() {
        let coordinator = ShutdownCoordinator::new();
        assert!(!coordinator.is_shutting_down());
        assert_eq!(coordinator.current_phase(), ShutdownPhase::Running);
    }

    #[tokio::test]
    async fn test_component_registration() {
        let coordinator = ShutdownCoordinator::new();
        let handle = coordinator.register_component("test-component".to_string());
        assert_eq!(handle.component_name(), "test-component");
    }

    #[tokio::test]
    async fn test_shutdown_sequence() {
        let mut coordinator = ShutdownCoordinator::new();
        let mut handle = coordinator.register_component("test-component".to_string());

        // Start shutdown in background
        let shutdown_task = tokio::spawn(async move {
            let start_time = Instant::now();
            let result = handle.recv_shutdown().await;
            handle.report_shutdown(Ok(()), start_time).await;
            result
        });

        // Initiate shutdown
        let shutdown_result = coordinator.shutdown(ShutdownReason::Graceful, Duration::from_secs(5)).await;

        // Wait for component to complete
        let signal_result = shutdown_task.await.unwrap();

        assert!(shutdown_result.is_ok());
        assert_eq!(signal_result, Some(ShutdownReason::Graceful));
        assert_eq!(coordinator.current_phase(), ShutdownPhase::Complete);
    }

    #[tokio::test]
    async fn test_shutdown_timeout() {
        let mut coordinator = ShutdownCoordinator::new();
        let _handle = coordinator.register_component("slow-component".to_string());

        // Component never reports completion, should timeout
        let result = coordinator.shutdown(ShutdownReason::Graceful, Duration::from_millis(100)).await;

        assert!(result.is_err());
        if let Err(ShutdownError::Timeout { .. }) = result {
            // Expected
        } else {
            panic!("Expected timeout error");
        }
    }

    #[test]
    fn test_shutdown_reason_equality() {
        assert_eq!(ShutdownReason::Graceful, ShutdownReason::Graceful);
        assert_ne!(ShutdownReason::Graceful, ShutdownReason::Emergency);
    }

    #[test]
    fn test_shutdown_phase_equality() {
        assert_eq!(ShutdownPhase::Running, ShutdownPhase::Running);
        assert_ne!(ShutdownPhase::Running, ShutdownPhase::Complete);
    }
}