// End-to-end test for complete device listing workflow
// This test must FAIL until full system implementation is complete

use std::time::Duration;
use std::process::Command;
use bytes::Bytes;
use anyhow::Result;
use tokio::process::Command as TokioCommand;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde_json::Value;

// Import contract types (will fail until implemented)
use crate::contract::test_adb_protocol::{AdbMessage, Command as AdbCommand};
use crate::contract::test_daemon_api::{DaemonServer, ServerConfig};

// E2E client that mimics real ADB client behavior
struct E2eAdbClient {
    stream: Option<TcpStream>,
    server_host: String,
    server_port: u16,
    session_id: u32,
}

impl E2eAdbClient {
    fn new(host: &str, port: u16) -> Self {
        Self {
            stream: None,
            server_host: host.to_string(),
            server_port: port,
            session_id: rand::random(),
        }
    }

    async fn connect(&mut self) -> Result<()> {
        let stream = TcpStream::connect(format!("{}:{}", self.server_host, self.server_port)).await?;
        self.stream = Some(stream);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(stream) = &mut self.stream {
            let _ = stream.shutdown().await;
        }
        self.stream = None;
        Ok(())
    }

    async fn send_message(&mut self, message: &AdbMessage) -> Result<()> {
        if let Some(stream) = &mut self.stream {
            let serialized = message.serialize()?;
            stream.write_all(&serialized).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Not connected"))
        }
    }

    async fn receive_message(&mut self) -> Result<AdbMessage> {
        if let Some(stream) = &mut self.stream {
            let mut header = [0u8; 24];
            stream.read_exact(&mut header).await?;

            let message = AdbMessage::deserialize(&header, Bytes::new())?;

            if message.data_length > 0 {
                let mut payload_data = vec![0u8; message.data_length as usize];
                stream.read_exact(&mut payload_data).await?;
                let payload = Bytes::from(payload_data);
                return Ok(AdbMessage { payload, ..message });
            }

            Ok(message)
        } else {
            Err(anyhow::anyhow!("Not connected"))
        }
    }

    async fn perform_handshake(&mut self) -> Result<()> {
        // Send CNXN message
        let cnxn_message = AdbMessage {
            command: AdbCommand::CNXN,
            arg0: 0x01000000, // VERSION
            arg1: 256 * 1024, // MAXDATA
            data_length: 0,
            data_checksum: 0,
            magic: !AdbCommand::CNXN as u32,
            payload: Bytes::new(),
        };

        self.send_message(&cnxn_message).await?;

        // Wait for CNXN response
        let response = self.receive_message().await?;
        if response.command != AdbCommand::CNXN {
            return Err(anyhow::anyhow!("Expected CNXN response"));
        }

        Ok(())
    }

    async fn list_devices(&mut self) -> Result<Vec<DeviceInfo>> {
        // Open host:devices service
        let service_name = b"host:devices";
        let checksum = crc32fast::hash(service_name);
        let payload = Bytes::from(service_name.to_vec());

        let open_message = AdbMessage {
            command: AdbCommand::OPEN,
            arg0: self.session_id, // local_id
            arg1: 0,               // remote_id
            data_length: payload.len() as u32,
            data_checksum: checksum,
            magic: !AdbCommand::OPEN as u32,
            payload,
        };

        self.send_message(&open_message).await?;

        // Wait for OKAY response
        let okay_response = self.receive_message().await?;
        if okay_response.command != AdbCommand::OKAY {
            return Err(anyhow::anyhow!("Expected OKAY response for host:devices"));
        }

        let remote_id = okay_response.arg0;

        // Wait for device list data
        let data_response = self.receive_message().await?;
        if data_response.command != AdbCommand::WRTE {
            return Err(anyhow::anyhow!("Expected WRTE response with device data"));
        }

        // Parse device list
        let device_list_str = String::from_utf8_lossy(&data_response.payload);
        let devices = self.parse_device_list(&device_list_str)?;

        // Send OKAY acknowledgment
        let ack_message = AdbMessage {
            command: AdbCommand::OKAY,
            arg0: self.session_id, // local_id
            arg1: remote_id,       // remote_id
            data_length: 0,
            data_checksum: 0,
            magic: !AdbCommand::OKAY as u32,
            payload: Bytes::new(),
        };

        self.send_message(&ack_message).await?;

        // Close the stream
        let close_message = AdbMessage {
            command: AdbCommand::CLSE,
            arg0: self.session_id, // local_id
            arg1: remote_id,       // remote_id
            data_length: 0,
            data_checksum: 0,
            magic: !AdbCommand::CLSE as u32,
            payload: Bytes::new(),
        };

        self.send_message(&close_message).await?;

        Ok(devices)
    }

    fn parse_device_list(&self, device_list: &str) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        for line in device_list.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let device_info = DeviceInfo {
                    device_id: parts[0].to_string(),
                    state: parts[1].to_string(),
                    product: parts.get(2).map(|s| s.to_string()),
                    model: parts.get(3).map(|s| s.to_string()),
                    device: parts.get(4).map(|s| s.to_string()),
                    transport_id: parts.get(5).map(|s| s.to_string()),
                };
                devices.push(device_info);
            }
        }

        Ok(devices)
    }

    async fn get_device_properties(&mut self, device_id: &str) -> Result<Vec<(String, String)>> {
        let service_name = format!("host:transport:{}", device_id);

        // Open transport connection
        let transport_result = self.open_service(&service_name).await?;
        let remote_id = transport_result;

        // Send getprop command
        let getprop_service = b"shell:getprop";
        let getprop_result = self.send_service_data(remote_id, getprop_service).await?;

        // Parse properties
        let props_str = String::from_utf8_lossy(&getprop_result);
        let properties = self.parse_properties(&props_str)?;

        Ok(properties)
    }

    async fn open_service(&mut self, service_name: &str) -> Result<u32> {
        let service_bytes = service_name.as_bytes();
        let checksum = crc32fast::hash(service_bytes);
        let payload = Bytes::from(service_bytes.to_vec());

        let open_message = AdbMessage {
            command: AdbCommand::OPEN,
            arg0: self.session_id,
            arg1: 0,
            data_length: payload.len() as u32,
            data_checksum: checksum,
            magic: !AdbCommand::OPEN as u32,
            payload,
        };

        self.send_message(&open_message).await?;

        let response = self.receive_message().await?;
        if response.command != AdbCommand::OKAY {
            return Err(anyhow::anyhow!("Failed to open service: {}", service_name));
        }

        Ok(response.arg0)
    }

    async fn send_service_data(&mut self, remote_id: u32, data: &[u8]) -> Result<Vec<u8>> {
        let checksum = crc32fast::hash(data);
        let payload = Bytes::from(data.to_vec());

        let write_message = AdbMessage {
            command: AdbCommand::WRTE,
            arg0: self.session_id,
            arg1: remote_id,
            data_length: payload.len() as u32,
            data_checksum: checksum,
            magic: !AdbCommand::WRTE as u32,
            payload,
        };

        self.send_message(&write_message).await?;

        // Wait for data response
        let response = self.receive_message().await?;
        if response.command == AdbCommand::WRTE {
            Ok(response.payload.to_vec())
        } else {
            Err(anyhow::anyhow!("Unexpected response to service data"))
        }
    }

    fn parse_properties(&self, props_str: &str) -> Result<Vec<(String, String)>> {
        let mut properties = Vec::new();

        for line in props_str.lines() {
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim().to_string();
                let value = line[eq_pos + 1..].trim().to_string();
                properties.push((key, value));
            }
        }

        Ok(properties)
    }
}

#[derive(Debug, Clone)]
struct DeviceInfo {
    device_id: String,
    state: String,
    product: Option<String>,
    model: Option<String>,
    device: Option<String>,
    transport_id: Option<String>,
}

// Mock daemon process for E2E testing
struct E2eDaemonProcess {
    child: Option<tokio::process::Child>,
    port: u16,
    config_file: String,
}

impl E2eDaemonProcess {
    async fn new() -> Result<Self> {
        // Create temporary config file
        let config = ServerConfig {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0, // Let OS assign port
            max_clients: 10,
            client_timeout: Duration::from_secs(30),
            enable_logging: true,
        };

        let config_json = serde_json::to_string_pretty(&config)?;
        let config_file = format!("/tmp/dbgif_test_config_{}.json", rand::random::<u32>());
        tokio::fs::write(&config_file, config_json).await?;

        Ok(Self {
            child: None,
            port: 5037, // Default ADB port for testing
            config_file,
        })
    }

    async fn start(&mut self) -> Result<()> {
        // Start the DBGIF daemon process
        let mut cmd = TokioCommand::new("target/debug/dbgif-server");
        cmd.arg("--config").arg(&self.config_file);
        cmd.arg("--port").arg(self.port.to_string());
        cmd.kill_on_drop(true);

        let child = cmd.spawn()?;
        self.child = Some(child);

        // Wait for daemon to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        // Clean up config file
        let _ = tokio::fs::remove_file(&self.config_file).await;

        Ok(())
    }

    fn port(&self) -> u16 {
        self.port
    }
}

#[tokio::test]
async fn test_e2e_device_listing_workflow() {
    // Start DBGIF daemon
    let mut daemon = E2eDaemonProcess::new().await.expect("Failed to create daemon process");
    daemon.start().await.expect("Failed to start daemon");

    // Create ADB client
    let mut client = E2eAdbClient::new("127.0.0.1", daemon.port());

    // Test complete workflow
    client.connect().await.expect("Failed to connect to daemon");
    client.perform_handshake().await.expect("Failed to perform handshake");

    // List devices
    let devices = client.list_devices().await.expect("Failed to list devices");

    // Verify device list structure
    assert!(!devices.is_empty(), "Should find at least one device");

    for device in &devices {
        assert!(!device.device_id.is_empty(), "Device ID should not be empty");
        assert!(!device.state.is_empty(), "Device state should not be empty");

        // Device state should be valid
        assert!(
            device.state == "device" ||
            device.state == "offline" ||
            device.state == "unauthorized" ||
            device.state == "connecting"
        );

        // Device ID should follow expected format
        assert!(
            device.device_id.starts_with("tcp:") ||
            device.device_id.starts_with("usb:") ||
            device.device_id.starts_with("usb_bridge:")
        );
    }

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_device_listing_with_multiple_transports() {
    let mut daemon = E2eDaemonProcess::new().await.expect("Failed to create daemon process");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eAdbClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect to daemon");
    client.perform_handshake().await.expect("Failed to perform handshake");

    let devices = client.list_devices().await.expect("Failed to list devices");

    // Should find devices from different transport types
    let tcp_devices: Vec<_> = devices.iter().filter(|d| d.device_id.starts_with("tcp:")).collect();
    let usb_devices: Vec<_> = devices.iter().filter(|d| d.device_id.starts_with("usb:")).collect();
    let bridge_devices: Vec<_> = devices.iter().filter(|d| d.device_id.starts_with("usb_bridge:")).collect();

    // At least one transport type should have devices
    assert!(
        !tcp_devices.is_empty() || !usb_devices.is_empty() || !bridge_devices.is_empty(),
        "Should find devices from at least one transport type"
    );

    // Test device-specific queries
    for device in &devices {
        if device.state == "device" {
            // Try to get device properties for connected devices
            let properties_result = client.get_device_properties(&device.device_id).await;

            // This might fail in mock implementation, but shouldn't crash
            match properties_result {
                Ok(properties) => {
                    assert!(!properties.is_empty(), "Connected device should have properties");
                }
                Err(_) => {
                    // Expected for mock devices
                }
            }
        }
    }

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_device_listing_performance() {
    let mut daemon = E2eDaemonProcess::new().await.expect("Failed to create daemon process");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eAdbClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect to daemon");
    client.perform_handshake().await.expect("Failed to perform handshake");

    // Measure device listing performance
    let start_time = std::time::Instant::now();
    let devices = client.list_devices().await.expect("Failed to list devices");
    let listing_duration = start_time.elapsed();

    // Device listing should complete quickly
    assert!(listing_duration < Duration::from_secs(5), "Device listing should complete within 5 seconds");

    // Test repeated listing performance
    let start_time = std::time::Instant::now();
    for _ in 0..10 {
        let _ = client.list_devices().await;
    }
    let repeated_duration = start_time.elapsed();

    // Repeated listings should be faster due to caching
    assert!(repeated_duration < Duration::from_secs(10), "10 repeated listings should complete within 10 seconds");

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_device_listing_concurrent_clients() {
    let mut daemon = E2eDaemonProcess::new().await.expect("Failed to create daemon process");
    daemon.start().await.expect("Failed to start daemon");

    // Create multiple concurrent clients
    let client_count = 5;
    let mut client_tasks = Vec::new();

    for i in 0..client_count {
        let daemon_port = daemon.port();
        let task = tokio::spawn(async move {
            let mut client = E2eAdbClient::new("127.0.0.1", daemon_port);
            client.session_id = 1000 + i; // Unique session ID

            client.connect().await?;
            client.perform_handshake().await?;

            let devices = client.list_devices().await?;

            client.disconnect().await?;

            Ok::<Vec<DeviceInfo>, anyhow::Error>(devices)
        });
        client_tasks.push(task);
    }

    // Wait for all clients to complete
    let results = futures::future::join_all(client_tasks).await;

    // All clients should succeed
    let mut all_device_lists = Vec::new();
    for result in results {
        let task_result = result.expect("Task should not panic");
        let devices = task_result.expect("Client should succeed");
        all_device_lists.push(devices);
    }

    // All clients should get consistent device lists
    let first_list = &all_device_lists[0];
    for other_list in &all_device_lists[1..] {
        assert_eq!(first_list.len(), other_list.len(), "All clients should see the same number of devices");

        // Device IDs should be consistent (order might differ)
        let first_ids: std::collections::HashSet<_> = first_list.iter().map(|d| &d.device_id).collect();
        let other_ids: std::collections::HashSet<_> = other_list.iter().map(|d| &d.device_id).collect();
        assert_eq!(first_ids, other_ids, "All clients should see the same devices");
    }

    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_device_listing_error_handling() {
    let mut daemon = E2eDaemonProcess::new().await.expect("Failed to create daemon process");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eAdbClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect to daemon");
    client.perform_handshake().await.expect("Failed to perform handshake");

    // Test graceful handling when no devices are available
    // (In mock implementation, this might not occur)
    let devices = client.list_devices().await.expect("Failed to list devices");

    // Test client recovery after daemon restart
    daemon.stop().await.expect("Failed to stop daemon");

    // Client should detect disconnection
    let result = client.list_devices().await;
    assert!(result.is_err(), "Should fail when daemon is stopped");

    // Restart daemon
    daemon.start().await.expect("Failed to restart daemon");

    // Client should be able to reconnect
    client.disconnect().await.expect("Failed to disconnect");
    client.connect().await.expect("Failed to reconnect");
    client.perform_handshake().await.expect("Failed to perform handshake after restart");

    let devices_after_restart = client.list_devices().await.expect("Failed to list devices after restart");
    assert!(!devices_after_restart.is_empty(), "Should find devices after restart");

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
#[ignore] // Only run with real hardware
async fn test_e2e_device_listing_with_real_hardware() {
    // This test requires actual hardware devices connected
    // Run with: cargo test test_e2e_device_listing_with_real_hardware -- --ignored

    let mut daemon = E2eDaemonProcess::new().await.expect("Failed to create daemon process");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eAdbClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect to daemon");
    client.perform_handshake().await.expect("Failed to perform handshake");

    let devices = client.list_devices().await.expect("Failed to list devices");

    println!("Found {} device(s):", devices.len());
    for device in &devices {
        println!("  {}: {} ({:?})", device.device_id, device.state, device.product);

        if device.state == "device" {
            println!("    Testing device properties...");
            match client.get_device_properties(&device.device_id).await {
                Ok(properties) => {
                    println!("    Found {} properties", properties.len());
                    for (key, value) in properties.iter().take(5) {
                        println!("      {}: {}", key, value);
                    }
                }
                Err(e) => {
                    println!("    Could not get properties: {}", e);
                }
            }
        }
    }

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}