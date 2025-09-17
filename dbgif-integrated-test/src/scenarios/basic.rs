use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::{info, debug};

use crate::{
    client::{TestClient, connection::setup_device_connection},
    device::{DeviceConfig, EmbeddedDeviceServer},
    scenarios::Scenario,
};

/// Basic connection scenario
pub struct BasicConnectionScenario {
    server_addr: SocketAddr,
}

impl BasicConnectionScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for BasicConnectionScenario {
    fn name(&self) -> &str {
        "basic_connection"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Basic Connection Scenario ===");

        // Step 1: Start a device server
        info!("Step 1: Starting device server");
        let device_config = DeviceConfig {
            device_id: "test-device-001".to_string(),
            port: None, // Auto-allocate
            model: Some("TestDevice".to_string()),
            capabilities: None,
        };
        let device = EmbeddedDeviceServer::spawn(device_config).await?;
        info!("Device server started on port {}", device.port());

        // Step 2: Connect client to DBGIF server
        info!("Step 2: Connecting to DBGIF server at {}", self.server_addr);
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Step 3: Use host:connect to connect to device
        info!("Step 3: Connecting to device via host:connect");
        setup_device_connection(&mut client, &device).await?;

        // Step 4: Verify connection with host:list
        info!("Step 4: Verifying device appears in host:list");
        let devices = client.ascii()?.test_host_list().await?;

        if devices.is_empty() {
            return Err(anyhow::anyhow!("No devices found after connection"));
        }

        info!("✅ Basic connection scenario completed successfully");
        info!("Found {} device(s) connected", devices.len());

        Ok(())
    }
}

/// Host services test scenario
pub struct HostServicesScenario {
    server_addr: SocketAddr,
}

impl HostServicesScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for HostServicesScenario {
    fn name(&self) -> &str {
        "host_services"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Host Services Scenario ===");

        // Connect to server
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Test host:version
        info!("Testing host:version");
        let version = client.ascii()?.test_host_version().await?;
        info!("Server version: {}", version);

        // Test host:list before any devices
        info!("Testing host:list (no devices)");
        let devices = client.ascii()?.test_host_list().await?;
        debug!("Initial device count: {}", devices.len());

        // Start a device and connect
        let device_config = DeviceConfig {
            device_id: "host-test-device".to_string(),
            port: None,
            model: Some("HostTestDevice".to_string()),
            capabilities: Some(vec!["shell".to_string(), "file".to_string()]),
        };
        let device = EmbeddedDeviceServer::spawn(device_config).await?;

        // Connect to device
        setup_device_connection(&mut client, &device).await?;

        // Test host:list after device connection
        info!("Testing host:list (with device)");
        let devices = client.ascii()?.test_host_list().await?;

        if devices.is_empty() {
            return Err(anyhow::anyhow!("Expected device in list after connection"));
        }

        info!("✅ Host services scenario completed successfully");
        info!("All host services working correctly");

        Ok(())
    }
}

/// Multi-device connection scenario
pub struct MultiDeviceScenario {
    server_addr: SocketAddr,
    device_count: usize,
}

impl MultiDeviceScenario {
    pub fn new(server_addr: SocketAddr, device_count: usize) -> Self {
        Self {
            server_addr,
            device_count,
        }
    }
}

#[async_trait]
impl Scenario for MultiDeviceScenario {
    fn name(&self) -> &str {
        "multi_device"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Multi-Device Scenario ({} devices) ===", self.device_count);

        // Start multiple device servers
        info!("Starting {} device servers", self.device_count);
        let mut devices = Vec::new();

        for i in 0..self.device_count {
            let device_config = DeviceConfig {
                device_id: format!("multi-device-{:03}", i + 1),
                port: None,
                model: Some(format!("MultiDevice-{}", i + 1)),
                capabilities: None,
            };
            let device = EmbeddedDeviceServer::spawn(device_config).await?;
            info!("Started device {} on port {}", i + 1, device.port());
            devices.push(device);
        }

        // Connect client
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Connect to all devices
        for (i, device) in devices.iter().enumerate() {
            info!("Connecting to device {} of {}", i + 1, self.device_count);
            setup_device_connection(&mut client, device).await?;
        }

        // Verify all devices appear
        let device_list = client.ascii()?.test_host_list().await?;

        if device_list.len() < self.device_count {
            return Err(anyhow::anyhow!(
                "Expected {} devices, found {}",
                self.device_count,
                device_list.len()
            ));
        }

        info!("✅ Multi-device scenario completed successfully");
        info!("All {} devices connected and verified", self.device_count);

        Ok(())
    }
}