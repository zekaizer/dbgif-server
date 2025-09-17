use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub enum DeviceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed(String),
}

#[derive(Debug)]
pub struct DeviceLifecycle {
    state: Arc<RwLock<DeviceState>>,
    started_at: Arc<RwLock<Option<Instant>>>,
    stopped_at: Arc<RwLock<Option<Instant>>>,
    restart_count: Arc<RwLock<u32>>,
}

impl DeviceLifecycle {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(DeviceState::Stopped)),
            started_at: Arc::new(RwLock::new(None)),
            stopped_at: Arc::new(RwLock::new(None)),
            restart_count: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn transition_to(&self, new_state: DeviceState) -> Result<()> {
        let mut state = self.state.write().await;
        let old_state = state.clone();

        // Validate state transition
        match (&old_state, &new_state) {
            (DeviceState::Stopped, DeviceState::Starting) |
            (DeviceState::Starting, DeviceState::Running) |
            (DeviceState::Starting, DeviceState::Failed(_)) |
            (DeviceState::Running, DeviceState::Stopping) |
            (DeviceState::Stopping, DeviceState::Stopped) |
            (DeviceState::Failed(_), DeviceState::Starting) => {
                // Valid transitions
                debug!("Device state transition: {:?} -> {:?}", old_state, new_state);
            }
            _ => {
                warn!("Invalid state transition: {:?} -> {:?}", old_state, new_state);
                return Err(anyhow::anyhow!(
                    "Invalid state transition from {:?} to {:?}",
                    old_state,
                    new_state
                ));
            }
        }

        // Update timestamps
        match new_state {
            DeviceState::Running => {
                *self.started_at.write().await = Some(Instant::now());
            }
            DeviceState::Stopped => {
                *self.stopped_at.write().await = Some(Instant::now());
            }
            _ => {}
        }

        *state = new_state;
        Ok(())
    }

    pub async fn get_state(&self) -> DeviceState {
        self.state.read().await.clone()
    }

    pub async fn is_running(&self) -> bool {
        matches!(*self.state.read().await, DeviceState::Running)
    }

    pub async fn uptime(&self) -> Option<Duration> {
        if let Some(started) = *self.started_at.read().await {
            if self.is_running().await {
                return Some(started.elapsed());
            }
        }
        None
    }

    pub async fn increment_restart_count(&self) {
        let mut count = self.restart_count.write().await;
        *count += 1;
        info!("Device restart count: {}", *count);
    }

    pub async fn get_restart_count(&self) -> u32 {
        *self.restart_count.read().await
    }

    pub async fn reset(&self) {
        *self.state.write().await = DeviceState::Stopped;
        *self.started_at.write().await = None;
        *self.stopped_at.write().await = None;
        *self.restart_count.write().await = 0;
    }
}