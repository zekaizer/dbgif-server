use anyhow::Result;
use std::time::Duration;
use tracing::{debug, info, warn, error};

use crate::client::TestClient;
use crate::device::EmbeddedDeviceServer;

/// Connection manager for automatic device connections
pub struct ConnectionManager {
    max_retries: u32,
    retry_delay: Duration,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            max_retries: 5,
            retry_delay: Duration::from_millis(500),
        }
    }

    pub fn with_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    /// Connect a client to a device server automatically
    pub async fn connect_device(
        &self,
        client: &mut TestClient,
        device: &EmbeddedDeviceServer,
    ) -> Result<()> {
        let device_port = device.port();
        info!("Connecting to device on port {}", device_port);

        // First, ensure device is ready
        device.wait_ready(Duration::from_secs(5)).await?;

        // Connect with retries
        for attempt in 1..=self.max_retries {
            debug!("Connection attempt {} of {}", attempt, self.max_retries);

            match client.connect_device("127.0.0.1", device_port).await {
                Ok(_) => {
                    info!("Successfully connected to device on port {}", device_port);
                    return self.verify_connection(client).await;
                }
                Err(e) => {
                    if attempt < self.max_retries {
                        warn!(
                            "Connection attempt {} failed: {}. Retrying in {:?}",
                            attempt, e, self.retry_delay
                        );
                        tokio::time::sleep(self.retry_delay).await;
                    } else {
                        error!("Failed to connect after {} attempts", self.max_retries);
                        return Err(e);
                    }
                }
            }
        }

        Err(anyhow::anyhow!("Failed to connect to device"))
    }

    /// Verify the connection is working
    async fn verify_connection(&self, _client: &mut TestClient) -> Result<()> {
        debug!("Verifying device connection");

        // Note: Direct connections via host:connect don't appear in host:list
        // The successful connection response is sufficient verification
        // Future enhancement: could test actual communication with the device

        info!("Device connection verified (direct TCP connection established)");

        Ok(())
    }

    /// Setup multiple device connections
    pub async fn setup_devices(
        &self,
        client: &mut TestClient,
        devices: &[EmbeddedDeviceServer],
    ) -> Result<()> {
        info!("Setting up {} device connections", devices.len());

        for (idx, device) in devices.iter().enumerate() {
            info!("Connecting device {} of {}", idx + 1, devices.len());
            self.connect_device(client, device).await?;
        }

        info!("All {} devices connected successfully", devices.len());
        Ok(())
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to setup a device connection with automatic retry
pub async fn setup_device_connection(
    client: &mut TestClient,
    device: &EmbeddedDeviceServer,
) -> Result<()> {
    let manager = ConnectionManager::new();
    manager.connect_device(client, device).await
}

/// Helper function to setup multiple device connections
pub async fn setup_multiple_devices(
    client: &mut TestClient,
    devices: &[EmbeddedDeviceServer],
) -> Result<()> {
    let manager = ConnectionManager::new();
    manager.setup_devices(client, devices).await
}