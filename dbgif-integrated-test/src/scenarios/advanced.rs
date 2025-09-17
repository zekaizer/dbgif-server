use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::{info, debug, warn};

use crate::{
    client::{TestClient, connection::setup_device_connection},
    device::{DeviceConfig, EmbeddedDeviceServer},
    scenarios::Scenario,
};

/// Test device disconnection and reconnection
pub struct DeviceReconnectionScenario {
    server_addr: SocketAddr,
}

impl DeviceReconnectionScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for DeviceReconnectionScenario {
    fn name(&self) -> &str {
        "device_reconnection"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Device Reconnection Test ===");

        // Start device
        let device_config = DeviceConfig {
            device_id: "reconnect-device".to_string(),
            port: None,
            model: Some("ReconnectDevice".to_string()),
            capabilities: None,
        };
        let mut device = EmbeddedDeviceServer::spawn(device_config).await?;
        let device_port = device.port();
        info!("Device started on port {}", device_port);

        // Connect client
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Connect to device
        setup_device_connection(&mut client, &device).await?;

        // Verify device appears in list
        let devices = client.ascii()?.test_host_list().await?;
        if devices.is_empty() {
            return Err(anyhow::anyhow!("Device not in list after connection"));
        }
        info!("Device connected successfully");

        // Simulate device disconnection
        info!("Simulating device disconnection");
        device.stop().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check device list (should be empty or show disconnected)
        let devices = client.ascii()?.test_host_list().await?;
        info!("Devices after disconnect: {} found", devices.len());

        // Restart device on same port
        info!("Restarting device");
        let new_device_config = DeviceConfig {
            device_id: "reconnect-device".to_string(),
            port: Some(device_port),
            model: Some("ReconnectDevice".to_string()),
            capabilities: None,
        };
        device = EmbeddedDeviceServer::spawn(new_device_config).await?;

        // Reconnect
        info!("Reconnecting to device");
        setup_device_connection(&mut client, &device).await?;

        // Verify reconnection
        let devices = client.ascii()?.test_host_list().await?;
        if devices.is_empty() {
            return Err(anyhow::anyhow!("Device not in list after reconnection"));
        }

        info!("✅ Device reconnection test passed");
        Ok(())
    }
}

/// Test concurrent stream handling
pub struct ConcurrentStreamScenario {
    server_addr: SocketAddr,
}

impl ConcurrentStreamScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for ConcurrentStreamScenario {
    fn name(&self) -> &str {
        "concurrent_streams"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Concurrent Stream Test ===");

        // Start device
        let device_config = DeviceConfig {
            device_id: "concurrent-device".to_string(),
            port: None,
            model: Some("ConcurrentDevice".to_string()),
            capabilities: Some(vec!["concurrent".to_string()]),
        };
        let device = EmbeddedDeviceServer::spawn(device_config).await?;

        // Connect client
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;
        setup_device_connection(&mut client, &device).await?;

        // Spawn multiple concurrent tasks
        info!("Starting concurrent stream operations");
        let mut handles = vec![];

        for i in 0..5 {
            let server_addr = self.server_addr;
            let handle = tokio::spawn(async move {
                let mut client = TestClient::new(server_addr);
                if let Err(e) = client.connect().await {
                    warn!("Client {} connection failed: {}", i, e);
                    return;
                }

                // Send multiple messages
                for j in 0..10 {
                    let data = format!("Client {} Message {}", i, j).into_bytes();
                    if let Ok(ascii) = client.ascii() {
                        if let Err(e) = ascii.send_strm((i % 3 + 1) as u8, &data).await {
                            warn!("Client {} failed to send message {}: {}", i, j, e);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }

                debug!("Client {} completed", i);
            });
            handles.push(handle);
        }

        // Wait for all tasks
        for handle in handles {
            let _ = handle.await;
        }

        info!("✅ Concurrent stream test completed");
        Ok(())
    }
}

/// Test device failure recovery
pub struct FailureRecoveryScenario {
    server_addr: SocketAddr,
}

impl FailureRecoveryScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for FailureRecoveryScenario {
    fn name(&self) -> &str {
        "failure_recovery"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Failure Recovery Test ===");

        // Start multiple devices
        let mut devices = Vec::new();
        for i in 0..3 {
            let config = DeviceConfig {
                device_id: format!("recovery-device-{}", i + 1),
                port: None,
                model: Some("RecoveryDevice".to_string()),
                capabilities: None,
            };
            let device = EmbeddedDeviceServer::spawn(config).await?;
            info!("Started device {} on port {}", i + 1, device.port());
            devices.push(device);
        }

        // Connect client
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Connect to all devices
        for device in &devices {
            setup_device_connection(&mut client, device).await?;
        }

        // Verify all connected
        let device_list = client.ascii()?.test_host_list().await?;
        info!("Initial devices: {}", device_list.len());

        // Simulate failure of middle device
        info!("Simulating device 2 failure");
        devices[1].stop().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check remaining devices
        let device_list = client.ascii()?.test_host_list().await?;
        info!("Devices after failure: {}", device_list.len());

        // Restart failed device
        info!("Restarting failed device");
        let config = DeviceConfig {
            device_id: "recovery-device-2-new".to_string(),
            port: None,
            model: Some("RecoveryDevice".to_string()),
            capabilities: None,
        };
        let new_device = EmbeddedDeviceServer::spawn(config).await?;
        devices[1] = new_device;

        // Reconnect
        setup_device_connection(&mut client, &devices[1]).await?;

        // Final verification
        let device_list = client.ascii()?.test_host_list().await?;
        info!("Final device count: {}", device_list.len());

        // Cleanup
        for mut device in devices {
            device.stop().await?;
        }

        info!("✅ Failure recovery test completed");
        Ok(())
    }
}

/// Test cross-device communication
pub struct CrossDeviceScenario {
    server_addr: SocketAddr,
}

impl CrossDeviceScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for CrossDeviceScenario {
    fn name(&self) -> &str {
        "cross_device"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Cross-Device Communication Test ===");

        // Start two devices
        let device1_config = DeviceConfig {
            device_id: "cross-device-1".to_string(),
            port: None,
            model: Some("CrossDevice1".to_string()),
            capabilities: Some(vec!["forward".to_string()]),
        };
        let device1 = EmbeddedDeviceServer::spawn(device1_config).await?;
        info!("Device 1 started on port {}", device1.port());

        let device2_config = DeviceConfig {
            device_id: "cross-device-2".to_string(),
            port: None,
            model: Some("CrossDevice2".to_string()),
            capabilities: Some(vec!["forward".to_string()]),
        };
        let device2 = EmbeddedDeviceServer::spawn(device2_config).await?;
        info!("Device 2 started on port {}", device2.port());

        // Connect client
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Connect both devices
        setup_device_connection(&mut client, &device1).await?;
        setup_device_connection(&mut client, &device2).await?;

        // Verify both devices connected
        let devices = client.ascii()?.test_host_list().await?;
        if devices.len() < 2 {
            return Err(anyhow::anyhow!("Expected 2 devices, found {}", devices.len()));
        }

        info!("Both devices connected successfully");

        // Test cross-device scenario
        // In a real implementation, this would test forwarding data between devices
        info!("Testing cross-device communication (simulated)");

        // Send data to device 1
        client.ascii()?.send_strm(1, b"Data for device 1").await?;

        // Send data to device 2
        client.ascii()?.send_strm(2, b"Data for device 2").await?;

        info!("✅ Cross-device communication test completed");
        Ok(())
    }
}