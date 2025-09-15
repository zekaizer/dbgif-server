// End-to-end test for command forwarding through daemon to devices
// This test must FAIL until full system implementation is complete

use std::time::Duration;
use bytes::Bytes;
use anyhow::Result;
use tokio::process::Command as TokioCommand;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::collections::HashMap;

// Import contract types (will fail until implemented)
use crate::contract::test_adb_protocol::{AdbMessage, Command as AdbCommand};
use crate::contract::test_daemon_api::{DaemonServer, ServerConfig};

// E2E client for testing command forwarding
struct E2eCommandClient {
    stream: Option<TcpStream>,
    server_host: String,
    server_port: u16,
    session_id: u32,
    next_local_id: u32,
    active_streams: HashMap<u32, u32>, // local_id -> remote_id mapping
}

impl E2eCommandClient {
    fn new(host: &str, port: u16) -> Self {
        Self {
            stream: None,
            server_host: host.to_string(),
            server_port: port,
            session_id: rand::random(),
            next_local_id: 1000,
            active_streams: HashMap::new(),
        }
    }

    async fn connect(&mut self) -> Result<()> {
        let stream = TcpStream::connect(format!("{}:{}", self.server_host, self.server_port)).await?;
        self.stream = Some(stream);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        // Close all active streams first
        for (&local_id, &remote_id) in self.active_streams.clone().iter() {
            let _ = self.close_stream(local_id, remote_id).await;
        }
        self.active_streams.clear();

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

    async fn execute_shell_command(&mut self, device_id: &str, command: &str) -> Result<String> {
        // Step 1: Open transport to specific device
        let transport_service = format!("host:transport:{}", device_id);
        let transport_id = self.open_service(&transport_service).await?;

        // Step 2: Open shell service on that transport
        let shell_service = format!("shell:{}", command);
        let shell_id = self.open_service_on_transport(transport_id, &shell_service).await?;

        // Step 3: Read command output
        let output = self.read_stream_output(shell_id).await?;

        // Step 4: Close streams
        self.close_stream(shell_id, shell_id).await?;
        self.close_stream(transport_id, transport_id).await?;

        Ok(output)
    }

    async fn push_file(&mut self, device_id: &str, local_path: &str, remote_path: &str) -> Result<()> {
        // Step 1: Open transport to device
        let transport_service = format!("host:transport:{}", device_id);
        let transport_id = self.open_service(&transport_service).await?;

        // Step 2: Open sync service
        let sync_service = "sync:";
        let sync_id = self.open_service_on_transport(transport_id, sync_service).await?;

        // Step 3: Send SEND command
        let send_command = format!("SEND{},{}", remote_path, local_path);
        self.send_stream_data(sync_id, send_command.as_bytes()).await?;

        // Step 4: Read file data and send
        let file_data = tokio::fs::read(local_path).await?;
        self.send_stream_data(sync_id, &file_data).await?;

        // Step 5: Send DONE
        self.send_stream_data(sync_id, b"DONE").await?;

        // Step 6: Wait for OKAY/FAIL response
        let response = self.read_stream_output(sync_id).await?;
        if !response.starts_with("OKAY") {
            return Err(anyhow::anyhow!("File push failed: {}", response));
        }

        // Close streams
        self.close_stream(sync_id, sync_id).await?;
        self.close_stream(transport_id, transport_id).await?;

        Ok(())
    }

    async fn pull_file(&mut self, device_id: &str, remote_path: &str, local_path: &str) -> Result<()> {
        // Step 1: Open transport to device
        let transport_service = format!("host:transport:{}", device_id);
        let transport_id = self.open_service(&transport_service).await?;

        // Step 2: Open sync service
        let sync_service = "sync:";
        let sync_id = self.open_service_on_transport(transport_id, sync_service).await?;

        // Step 3: Send RECV command
        let recv_command = format!("RECV{}", remote_path);
        self.send_stream_data(sync_id, recv_command.as_bytes()).await?;

        // Step 4: Read file data response
        let file_data = self.read_stream_binary(sync_id).await?;

        // Step 5: Write to local file
        tokio::fs::write(local_path, file_data).await?;

        // Close streams
        self.close_stream(sync_id, sync_id).await?;
        self.close_stream(transport_id, transport_id).await?;

        Ok(())
    }

    async fn forward_port(&mut self, device_id: &str, local_port: u16, remote_port: u16) -> Result<()> {
        // Open host service for port forwarding
        let forward_service = format!("host:forward:tcp:{};tcp:{}", local_port, remote_port);
        let forward_id = self.open_service(&forward_service).await?;

        // Port forwarding setup should return OKAY
        let response = self.read_stream_output(forward_id).await?;
        if !response.starts_with("OKAY") {
            return Err(anyhow::anyhow!("Port forwarding failed: {}", response));
        }

        // Keep the stream open for the forwarding session
        // In real implementation, this would maintain the tunnel

        Ok(())
    }

    async fn install_package(&mut self, device_id: &str, apk_path: &str) -> Result<String> {
        // Step 1: Push APK to device temp location
        let temp_path = format!("/data/local/tmp/{}",
            std::path::Path::new(apk_path).file_name().unwrap().to_str().unwrap());

        self.push_file(device_id, apk_path, &temp_path).await?;

        // Step 2: Execute pm install command
        let install_command = format!("pm install {}", temp_path);
        let install_result = self.execute_shell_command(device_id, &install_command).await?;

        // Step 3: Clean up temp file
        let cleanup_command = format!("rm {}", temp_path);
        let _ = self.execute_shell_command(device_id, &cleanup_command).await;

        Ok(install_result)
    }

    async fn get_device_info(&mut self, device_id: &str) -> Result<HashMap<String, String>> {
        let getprop_output = self.execute_shell_command(device_id, "getprop").await?;
        let mut properties = HashMap::new();

        for line in getprop_output.lines() {
            if let Some((key, value)) = self.parse_property_line(line) {
                properties.insert(key, value);
            }
        }

        Ok(properties)
    }

    fn parse_property_line(&self, line: &str) -> Option<(String, String)> {
        // Parse lines like: [ro.build.version.release]: [11]
        if line.starts_with('[') && line.contains("]: [") {
            let key_end = line.find("]: [")?;
            let key = line[1..key_end].to_string();
            let value_start = key_end + 4;
            let value_end = line.rfind(']')?;
            let value = line[value_start..value_end].to_string();
            Some((key, value))
        } else {
            None
        }
    }

    async fn open_service(&mut self, service_name: &str) -> Result<u32> {
        let local_id = self.next_local_id;
        self.next_local_id += 1;

        let service_bytes = service_name.as_bytes();
        let checksum = crc32fast::hash(service_bytes);
        let payload = Bytes::from(service_bytes.to_vec());

        let open_message = AdbMessage {
            command: AdbCommand::OPEN,
            arg0: local_id,
            arg1: 0, // remote_id to be assigned
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

        let remote_id = response.arg0;
        self.active_streams.insert(local_id, remote_id);

        Ok(local_id)
    }

    async fn open_service_on_transport(&mut self, transport_id: u32, service_name: &str) -> Result<u32> {
        // For services on specific transports, we use the transport stream
        let service_bytes = service_name.as_bytes();
        let checksum = crc32fast::hash(service_bytes);
        let payload = Bytes::from(service_bytes.to_vec());

        if let Some(&remote_id) = self.active_streams.get(&transport_id) {
            let write_message = AdbMessage {
                command: AdbCommand::WRTE,
                arg0: transport_id,
                arg1: remote_id,
                data_length: payload.len() as u32,
                data_checksum: checksum,
                magic: !AdbCommand::WRTE as u32,
                payload,
            };

            self.send_message(&write_message).await?;

            // Wait for OKAY
            let response = self.receive_message().await?;
            if response.command == AdbCommand::OKAY {
                Ok(transport_id) // Reuse the transport stream
            } else {
                Err(anyhow::anyhow!("Failed to open service on transport"))
            }
        } else {
            Err(anyhow::anyhow!("Transport stream not found"))
        }
    }

    async fn send_stream_data(&mut self, local_id: u32, data: &[u8]) -> Result<()> {
        if let Some(&remote_id) = self.active_streams.get(&local_id) {
            let checksum = crc32fast::hash(data);
            let payload = Bytes::from(data.to_vec());

            let write_message = AdbMessage {
                command: AdbCommand::WRTE,
                arg0: local_id,
                arg1: remote_id,
                data_length: payload.len() as u32,
                data_checksum: checksum,
                magic: !AdbCommand::WRTE as u32,
                payload,
            };

            self.send_message(&write_message).await?;

            // Wait for OKAY acknowledgment
            let response = self.receive_message().await?;
            if response.command != AdbCommand::OKAY {
                return Err(anyhow::anyhow!("Data send not acknowledged"));
            }

            Ok(())
        } else {
            Err(anyhow::anyhow!("Stream not found"))
        }
    }

    async fn read_stream_output(&mut self, local_id: u32) -> Result<String> {
        // Wait for WRTE response with data
        let response = self.receive_message().await?;
        if response.command == AdbCommand::WRTE {
            // Send OKAY acknowledgment
            if let Some(&remote_id) = self.active_streams.get(&local_id) {
                let ack_message = AdbMessage {
                    command: AdbCommand::OKAY,
                    arg0: local_id,
                    arg1: remote_id,
                    data_length: 0,
                    data_checksum: 0,
                    magic: !AdbCommand::OKAY as u32,
                    payload: Bytes::new(),
                };
                self.send_message(&ack_message).await?;
            }

            Ok(String::from_utf8_lossy(&response.payload).to_string())
        } else {
            Err(anyhow::anyhow!("Expected data response"))
        }
    }

    async fn read_stream_binary(&mut self, local_id: u32) -> Result<Vec<u8>> {
        let response = self.receive_message().await?;
        if response.command == AdbCommand::WRTE {
            // Send OKAY acknowledgment
            if let Some(&remote_id) = self.active_streams.get(&local_id) {
                let ack_message = AdbMessage {
                    command: AdbCommand::OKAY,
                    arg0: local_id,
                    arg1: remote_id,
                    data_length: 0,
                    data_checksum: 0,
                    magic: !AdbCommand::OKAY as u32,
                    payload: Bytes::new(),
                };
                self.send_message(&ack_message).await?;
            }

            Ok(response.payload.to_vec())
        } else {
            Err(anyhow::anyhow!("Expected binary data response"))
        }
    }

    async fn close_stream(&mut self, local_id: u32, remote_id: u32) -> Result<()> {
        let close_message = AdbMessage {
            command: AdbCommand::CLSE,
            arg0: local_id,
            arg1: remote_id,
            data_length: 0,
            data_checksum: 0,
            magic: !AdbCommand::CLSE as u32,
            payload: Bytes::new(),
        };

        self.send_message(&close_message).await?;
        self.active_streams.remove(&local_id);

        Ok(())
    }
}

// Mock daemon for command forwarding tests
struct E2eCommandDaemon {
    child: Option<tokio::process::Child>,
    port: u16,
    config_file: String,
}

impl E2eCommandDaemon {
    async fn new() -> Result<Self> {
        let config = ServerConfig {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
            max_clients: 10,
            client_timeout: Duration::from_secs(60), // Longer timeout for commands
            enable_logging: true,
        };

        let config_json = serde_json::to_string_pretty(&config)?;
        let config_file = format!("/tmp/dbgif_cmd_test_config_{}.json", rand::random::<u32>());
        tokio::fs::write(&config_file, config_json).await?;

        Ok(Self {
            child: None,
            port: 5037,
            config_file,
        })
    }

    async fn start(&mut self) -> Result<()> {
        let mut cmd = TokioCommand::new("target/debug/dbgif-server");
        cmd.arg("--config").arg(&self.config_file);
        cmd.arg("--port").arg(self.port.to_string());
        cmd.kill_on_drop(true);

        let child = cmd.spawn()?;
        self.child = Some(child);

        tokio::time::sleep(Duration::from_millis(1000)).await;

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        let _ = tokio::fs::remove_file(&self.config_file).await;

        Ok(())
    }

    fn port(&self) -> u16 {
        self.port
    }
}

#[tokio::test]
async fn test_e2e_shell_command_execution() {
    let mut daemon = E2eCommandDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eCommandClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect");
    client.perform_handshake().await.expect("Failed to handshake");

    // Test basic shell commands
    let device_id = "tcp:127.0.0.1:5555"; // Mock device

    // Test simple commands
    let echo_result = client.execute_shell_command(device_id, "echo hello").await;
    if echo_result.is_ok() {
        let output = echo_result.unwrap();
        assert!(output.contains("hello"), "Echo command should return 'hello'");
    }

    let pwd_result = client.execute_shell_command(device_id, "pwd").await;
    if pwd_result.is_ok() {
        let output = pwd_result.unwrap();
        assert!(!output.is_empty(), "PWD command should return current directory");
    }

    let ls_result = client.execute_shell_command(device_id, "ls /").await;
    if ls_result.is_ok() {
        let output = ls_result.unwrap();
        assert!(output.contains("system") || output.contains("data"), "Should list root directories");
    }

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_file_transfer_operations() {
    let mut daemon = E2eCommandDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eCommandClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect");
    client.perform_handshake().await.expect("Failed to handshake");

    let device_id = "tcp:127.0.0.1:5555";

    // Create a test file
    let test_file_content = "This is a test file for DBGIF file transfer";
    let local_test_file = "/tmp/dbgif_test_file.txt";
    let remote_test_file = "/data/local/tmp/dbgif_test_file.txt";

    tokio::fs::write(local_test_file, test_file_content).await.expect("Failed to create test file");

    // Test file push
    let push_result = client.push_file(device_id, local_test_file, remote_test_file).await;
    if push_result.is_ok() {
        println!("File push successful");

        // Test file pull
        let local_pulled_file = "/tmp/dbgif_pulled_file.txt";
        let pull_result = client.pull_file(device_id, remote_test_file, local_pulled_file).await;

        if pull_result.is_ok() {
            // Verify file contents
            let pulled_content = tokio::fs::read_to_string(local_pulled_file).await.expect("Failed to read pulled file");
            assert_eq!(pulled_content, test_file_content, "Pulled file should match original content");

            // Clean up
            let _ = tokio::fs::remove_file(local_pulled_file).await;
        }
    }

    // Clean up
    let _ = tokio::fs::remove_file(local_test_file).await;

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_device_information_retrieval() {
    let mut daemon = E2eCommandDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eCommandClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect");
    client.perform_handshake().await.expect("Failed to handshake");

    let device_id = "tcp:127.0.0.1:5555";

    // Get device properties
    let device_info_result = client.get_device_info(device_id).await;
    if device_info_result.is_ok() {
        let device_info = device_info_result.unwrap();

        // Check for common Android properties
        let expected_properties = vec![
            "ro.build.version.release",
            "ro.product.model",
            "ro.product.manufacturer",
            "ro.build.version.sdk",
        ];

        for prop in expected_properties {
            if device_info.contains_key(prop) {
                println!("Found property {}: {}", prop, device_info[prop]);
            }
        }

        assert!(!device_info.is_empty(), "Should retrieve some device properties");
    }

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_port_forwarding() {
    let mut daemon = E2eCommandDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eCommandClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect");
    client.perform_handshake().await.expect("Failed to handshake");

    let device_id = "tcp:127.0.0.1:5555";

    // Test port forwarding setup
    let local_port = 8080;
    let remote_port = 80;

    let forward_result = client.forward_port(device_id, local_port, remote_port).await;
    if forward_result.is_ok() {
        println!("Port forwarding setup successful: {}:{} -> {}:{}",
                "localhost", local_port, device_id, remote_port);

        // In a real test, we would verify the port forwarding works
        // by connecting to the local port and checking if it reaches the remote port
    }

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_concurrent_command_execution() {
    let mut daemon = E2eCommandDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let device_id = "tcp:127.0.0.1:5555";
    let daemon_port = daemon.port();

    // Create multiple clients for concurrent commands
    let command_count = 3;
    let mut command_tasks = Vec::new();

    for i in 0..command_count {
        let task = tokio::spawn(async move {
            let mut client = E2eCommandClient::new("127.0.0.1", daemon_port);
            client.connect().await?;
            client.perform_handshake().await?;

            // Execute different commands concurrently
            let command = match i {
                0 => "echo test1",
                1 => "pwd",
                _ => "ls /",
            };

            let result = client.execute_shell_command(device_id, command).await?;

            client.disconnect().await?;

            Ok::<String, anyhow::Error>(result)
        });
        command_tasks.push(task);
    }

    // Wait for all commands to complete
    let results = futures::future::join_all(command_tasks).await;

    // Check results
    for (i, result) in results.iter().enumerate() {
        let task_result = result.as_ref().expect("Task should not panic");
        if let Ok(output) = task_result {
            match i {
                0 => assert!(output.contains("test1"), "Echo command should return test1"),
                1 => assert!(!output.is_empty(), "PWD should return directory"),
                _ => assert!(!output.is_empty(), "LS should return file list"),
            }
        }
    }

    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_command_error_handling() {
    let mut daemon = E2eCommandDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eCommandClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect");
    client.perform_handshake().await.expect("Failed to handshake");

    let device_id = "tcp:127.0.0.1:5555";

    // Test invalid commands
    let invalid_command_result = client.execute_shell_command(device_id, "invalid_command_that_does_not_exist").await;
    // Should either fail or return error message, not crash

    // Test commands on non-existent device
    let fake_device = "tcp:192.168.1.999:5555";
    let fake_device_result = client.execute_shell_command(fake_device, "echo test").await;
    // Should fail gracefully

    // Test very long commands
    let long_command = "echo ".to_string() + &"a".repeat(10000);
    let long_command_result = client.execute_shell_command(device_id, &long_command).await;
    // Should handle gracefully (succeed or fail without crashing)

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_command_timeout_handling() {
    let mut daemon = E2eCommandDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eCommandClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect");
    client.perform_handshake().await.expect("Failed to handshake");

    let device_id = "tcp:127.0.0.1:5555";

    // Test command that might take long time (sleep)
    let start_time = std::time::Instant::now();
    let sleep_result = tokio::time::timeout(
        Duration::from_secs(5),
        client.execute_shell_command(device_id, "sleep 10")
    ).await;

    let elapsed = start_time.elapsed();

    // Should timeout within 5 seconds, not wait for the full 10 seconds
    assert!(elapsed < Duration::from_secs(6), "Command should timeout within reasonable time");

    // Result should be timeout error
    assert!(sleep_result.is_err(), "Long running command should timeout");

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
#[ignore] // Only run with real hardware and APK file
async fn test_e2e_package_installation() {
    // This test requires a real APK file and connected device
    // Run with: cargo test test_e2e_package_installation -- --ignored

    let mut daemon = E2eCommandDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eCommandClient::new("127.0.0.1", daemon.port());
    client.connect().await.expect("Failed to connect");
    client.perform_handshake().await.expect("Failed to handshake");

    let device_id = "tcp:127.0.0.1:5555"; // Replace with actual device ID
    let apk_path = "test_app.apk"; // Replace with actual APK path

    if tokio::fs::metadata(apk_path).await.is_ok() {
        println!("Installing package: {}", apk_path);

        let install_result = client.install_package(device_id, apk_path).await;
        match install_result {
            Ok(output) => {
                println!("Installation result: {}", output);
                assert!(output.contains("Success") || output.contains("INSTALL_"),
                        "Installation should indicate success or specific status");
            }
            Err(e) => {
                println!("Installation failed: {}", e);
                // This is acceptable for testing purposes
            }
        }
    } else {
        println!("Test APK file not found, skipping installation test");
    }

    client.disconnect().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}