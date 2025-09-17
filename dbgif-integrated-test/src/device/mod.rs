pub mod lifecycle;
pub mod server;

use anyhow::Result;
use lifecycle::{DeviceLifecycle, DeviceState};
use server::DeviceServer;
use crate::utils;
use std::time::Duration;
use std::net::SocketAddr;
use tracing::{info, error};

pub struct DeviceConfig {
    pub device_id: String,
    pub port: Option<u16>,
    pub model: Option<String>,
    pub capabilities: Option<Vec<String>>,
}

pub struct EmbeddedDeviceServer {
    config: DeviceConfig,
    server: DeviceServer,
    lifecycle: DeviceLifecycle,
}

impl EmbeddedDeviceServer {
    pub async fn spawn(config: DeviceConfig) -> Result<Self> {
        let lifecycle = DeviceLifecycle::new();
        lifecycle.transition_to(DeviceState::Starting).await?;

        let mut server = DeviceServer::new(config.device_id.clone());

        if let Some(model) = &config.model {
            server = server.with_model(model.clone());
        }

        if let Some(capabilities) = &config.capabilities {
            server = server.with_capabilities(capabilities.clone());
        }

        let mut embedded = Self {
            config,
            server,
            lifecycle,
        };

        match embedded.server.start(embedded.config.port).await {
            Ok(_) => {
                embedded.lifecycle.transition_to(DeviceState::Running).await?;
                info!("Device server {} started successfully", embedded.config.device_id);
                Ok(embedded)
            }
            Err(e) => {
                embedded.lifecycle.transition_to(
                    DeviceState::Failed(e.to_string())
                ).await?;
                error!("Failed to start device server: {}", e);
                Err(e)
            }
        }
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.lifecycle.transition_to(DeviceState::Stopping).await?;

        match self.server.stop().await {
            Ok(_) => {
                self.lifecycle.transition_to(DeviceState::Stopped).await?;
                Ok(())
            }
            Err(e) => {
                self.lifecycle.transition_to(
                    DeviceState::Failed(format!("Stop failed: {}", e))
                ).await?;
                Err(e)
            }
        }
    }

    pub async fn restart(&mut self) -> Result<()> {
        info!("Restarting device server {}", self.config.device_id);
        self.lifecycle.increment_restart_count().await;

        // Stop if running
        if self.is_running().await {
            self.stop().await?;
        }

        // Wait a bit before restarting
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Start again
        self.lifecycle.transition_to(DeviceState::Starting).await?;
        match self.server.start(self.config.port).await {
            Ok(_) => {
                self.lifecycle.transition_to(DeviceState::Running).await?;
                info!("Device server {} restarted successfully", self.config.device_id);
                Ok(())
            }
            Err(e) => {
                self.lifecycle.transition_to(
                    DeviceState::Failed(e.to_string())
                ).await?;
                Err(e)
            }
        }
    }

    pub fn port(&self) -> u16 {
        self.server.port()
    }

    pub async fn health_check(&self) -> Result<bool> {
        Ok(self.server.is_running() && self.lifecycle.is_running().await)
    }

    pub async fn is_running(&self) -> bool {
        self.lifecycle.is_running().await
    }

    pub async fn get_state(&self) -> DeviceState {
        self.lifecycle.get_state().await
    }

    pub async fn uptime(&self) -> Option<Duration> {
        self.lifecycle.uptime().await
    }

    /// Wait for the device server to be ready
    pub async fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let addr: SocketAddr = format!("127.0.0.1:{}", self.port()).parse()?;
        utils::wait_for_port(addr, timeout).await?;
        info!("Device server {} ready on port {}", self.config.device_id, self.port());
        Ok(())
    }
}