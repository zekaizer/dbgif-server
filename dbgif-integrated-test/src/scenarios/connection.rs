use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::{info, debug, warn};

use crate::{
    client::{TestClient, connection::ConnectionManager},
    device::{DeviceConfig, EmbeddedDeviceServer},
    scenarios::Scenario,
};

/// Test connection handshake scenario
pub struct ConnectionHandshakeScenario {
    server_addr: SocketAddr,
}

impl ConnectionHandshakeScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for ConnectionHandshakeScenario {
    fn name(&self) -> &str {
        "connection_handshake"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Connection Handshake Test ===");

        // Step 1: Connect to server
        info!("Step 1: Connecting to DBGIF server");
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Step 2: Test basic command
        info!("Step 2: Testing basic host:version command");
        let version = client.ascii()?.test_host_version().await?;
        info!("Server version: {}", version);

        // Step 3: Disconnect and reconnect
        info!("Step 3: Testing disconnect/reconnect");
        client.disconnect().await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        client.connect().await?;

        // Step 4: Verify connection still works
        info!("Step 4: Verifying reconnection");
        let version2 = client.ascii()?.test_host_version().await?;

        if version != version2 {
            return Err(anyhow::anyhow!("Version mismatch after reconnect"));
        }

        info!("✅ Connection handshake test passed");
        Ok(())
    }
}

/// Test error handling scenario
pub struct ErrorHandlingScenario {
    server_addr: SocketAddr,
}

impl ErrorHandlingScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for ErrorHandlingScenario {
    fn name(&self) -> &str {
        "error_handling"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Error Handling Test ===");

        // Test 1: Invalid command
        info!("Test 1: Sending invalid command");
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Send invalid command
        match client.ascii()?.send_command("invalid:command").await {
            Ok(_) => {
                match client.ascii()?.receive_response().await {
                    Ok((success, response)) => {
                        if success {
                            warn!("Server accepted invalid command - unexpected");
                        } else {
                            info!("Server correctly rejected invalid command: {}", response);
                        }
                    }
                    Err(e) => {
                        info!("Expected error for invalid command: {}", e);
                    }
                }
            }
            Err(e) => {
                info!("Error sending invalid command (expected): {}", e);
            }
        }

        // Test 2: Connection to non-existent device
        info!("Test 2: Connecting to non-existent device");
        match client.ascii()?.test_host_connect("192.168.99.99", 9999).await {
            Ok(_) => {
                warn!("Connection to non-existent device succeeded - unexpected");
            }
            Err(e) => {
                info!("Expected error for non-existent device: {}", e);
            }
        }

        // Test 3: Timeout handling
        info!("Test 3: Testing timeout handling");
        let mut slow_client = TestClient::new(self.server_addr)
            .with_timeout(Duration::from_millis(100));

        // This should timeout if server doesn't respond quickly
        match slow_client.connect().await {
            Ok(_) => {
                info!("Connected within timeout");
            }
            Err(e) => {
                info!("Connection timed out (may be expected): {}", e);
            }
        }

        info!("✅ Error handling test completed");
        Ok(())
    }
}

/// Test stream multiplexing scenario
pub struct StreamMultiplexingScenario {
    server_addr: SocketAddr,
}

impl StreamMultiplexingScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for StreamMultiplexingScenario {
    fn name(&self) -> &str {
        "stream_multiplexing"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Stream Multiplexing Test ===");

        // Setup device
        let device_config = DeviceConfig {
            device_id: "stream-test-device".to_string(),
            port: None,
            model: Some("StreamTestDevice".to_string()),
            capabilities: Some(vec!["stream".to_string(), "multiplex".to_string()]),
        };
        let device = EmbeddedDeviceServer::spawn(device_config).await?;
        info!("Device server started on port {}", device.port());

        // Connect client
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;

        // Connect to device
        let conn_mgr = ConnectionManager::new();
        conn_mgr.connect_device(&mut client, &device).await?;

        // Test 1: Open multiple streams
        info!("Test 1: Opening multiple streams");
        for i in 0..3 {
            let stream_id = i + 1;
            debug!("Opening stream {}", stream_id);

            // Send STRM data
            client.ascii()?.send_strm(stream_id as u8, b"test data").await?;

            // Should receive echo back
            match client.ascii()?.receive_strm().await {
                Ok((id, data)) => {
                    debug!("Received STRM response: id={}, data={:?}", id,
                        String::from_utf8_lossy(&data));
                }
                Err(e) => {
                    warn!("Failed to receive STRM response: {}", e);
                }
            }
        }

        // Test 2: Concurrent data transfer
        info!("Test 2: Testing concurrent data transfer");
        for i in 0..5 {
            let data = format!("Message {}", i).into_bytes();
            client.ascii()?.send_strm((i % 3 + 1) as u8, &data).await?;
        }

        info!("✅ Stream multiplexing test completed");
        Ok(())
    }
}

/// Test timeout scenarios
pub struct TimeoutScenario {
    server_addr: SocketAddr,
}

impl TimeoutScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self { server_addr }
    }
}

#[async_trait]
impl Scenario for TimeoutScenario {
    fn name(&self) -> &str {
        "timeout_test"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Timeout Test Scenario ===");

        // Test 1: Quick timeout
        info!("Test 1: Testing quick timeout (100ms)");
        let mut quick_client = TestClient::new(self.server_addr)
            .with_timeout(Duration::from_millis(100));

        match quick_client.connect().await {
            Ok(_) => info!("Connected within 100ms"),
            Err(_) => info!("Connection timed out after 100ms (expected if server is slow)"),
        }

        // Test 2: Normal timeout
        info!("Test 2: Testing normal timeout (5s)");
        let mut normal_client = TestClient::new(self.server_addr)
            .with_timeout(Duration::from_secs(5));

        normal_client.connect().await?;
        info!("Connected within 5s");

        // Test 3: Device startup timeout
        info!("Test 3: Testing device startup timeout");
        let device_config = DeviceConfig {
            device_id: "timeout-test-device".to_string(),
            port: None,
            model: Some("TimeoutDevice".to_string()),
            capabilities: None,
        };

        let device = EmbeddedDeviceServer::spawn(device_config).await?;

        // Wait for device with short timeout
        match device.wait_ready(Duration::from_millis(500)).await {
            Ok(_) => info!("Device ready within 500ms"),
            Err(e) => info!("Device not ready within 500ms: {}", e),
        }

        info!("✅ Timeout test completed");
        Ok(())
    }
}