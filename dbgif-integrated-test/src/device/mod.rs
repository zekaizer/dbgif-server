pub mod server;

use anyhow::Result;
use server::DeviceServer;
use crate::utils;
use std::time::Duration;
use std::net::SocketAddr;
use tracing::info;

pub struct DeviceConfig {
    pub device_id: String,
    pub port: Option<u16>,
    pub model: Option<String>,
    pub capabilities: Option<Vec<String>>,
}

pub struct EmbeddedDeviceServer {
    config: DeviceConfig,
    server: DeviceServer,
}

impl EmbeddedDeviceServer {
    pub async fn spawn(config: DeviceConfig) -> Result<Self> {
        let mut server = DeviceServer::new(config.device_id.clone());

        if let Some(model) = &config.model {
            server = server.with_model(model.clone());
        }

        if let Some(capabilities) = &config.capabilities {
            server = server.with_capabilities(capabilities.clone());
        }

        let mut embedded = Self { config, server };
        embedded.server.start(embedded.config.port).await?;

        Ok(embedded)
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.server.stop().await
    }

    pub fn port(&self) -> u16 {
        self.server.port()
    }

    pub async fn health_check(&self) -> Result<bool> {
        Ok(self.server.is_running())
    }

    /// Wait for the device server to be ready
    pub async fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let addr: SocketAddr = format!("127.0.0.1:{}", self.port()).parse()?;
        utils::wait_for_port(addr, timeout).await?;
        info!("Device server {} ready on port {}", self.config.device_id, self.port());
        Ok(())
    }
}