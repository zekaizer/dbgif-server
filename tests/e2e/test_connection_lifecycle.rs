// End-to-end test for complete connection lifecycle management
// This test must FAIL until full system implementation is complete

use std::time::Duration;
use bytes::Bytes;
use anyhow::Result;
use tokio::process::Command as TokioCommand;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;

// Import contract types (will fail until implemented)
use crate::contract::test_adb_protocol::{AdbMessage, Command as AdbCommand};
use crate::contract::test_daemon_api::{DaemonServer, ServerConfig};
use crate::contract::test_transport_trait::{TransportType};

// Connection state tracking
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Disconnected,
    Connecting,
    Handshaking,
    Connected,
    Error(String),
}

// E2E client for testing complete connection lifecycle
struct E2eLifecycleClient {
    stream: Option<TcpStream>,
    server_host: String,
    server_port: u16,
    session_id: u32,
    state: ConnectionState,
    connection_start_time: Option<std::time::Instant>,
    handshake_complete_time: Option<std::time::Instant>,
    active_streams: HashMap<u32, StreamInfo>,
    next_local_id: u32,
}

#[derive(Debug, Clone)]
struct StreamInfo {
    local_id: u32,
    remote_id: u32,
    service_name: String,
    created_at: std::time::Instant,
    bytes_sent: u64,
    bytes_received: u64,
}

impl E2eLifecycleClient {
    fn new(host: &str, port: u16) -> Self {
        Self {
            stream: None,
            server_host: host.to_string(),
            server_port: port,
            session_id: rand::random(),
            state: ConnectionState::Disconnected,
            connection_start_time: None,
            handshake_complete_time: None,
            active_streams: HashMap::new(),
            next_local_id: 1000,
        }
    }

    async fn connect_with_lifecycle(&mut self) -> Result<Duration> {
        self.state = ConnectionState::Connecting;
        self.connection_start_time = Some(std::time::Instant::now());

        // Establish TCP connection
        match TcpStream::connect(format!("{}:{}", self.server_host, self.server_port)).await {
            Ok(stream) => {
                self.stream = Some(stream);
                let connection_time = self.connection_start_time.unwrap().elapsed();

                // Perform ADB handshake
                self.state = ConnectionState::Handshaking;
                self.perform_full_handshake().await?;

                self.state = ConnectionState::Connected;
                self.handshake_complete_time = Some(std::time::Instant::now());

                Ok(connection_time)
            }
            Err(e) => {
                self.state = ConnectionState::Error(e.to_string());
                Err(e.into())
            }
        }
    }

    async fn perform_full_handshake(&mut self) -> Result<()> {
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

        self.send_message_tracked(&cnxn_message).await?;

        // Wait for CNXN response
        let response = self.receive_message_tracked().await?;
        if response.command != AdbCommand::CNXN {
            return Err(anyhow::anyhow!("Expected CNXN response, got {:?}", response.command));
        }

        // Handle AUTH sequence if required (simplified)
        if let Ok(auth_message) = tokio::time::timeout(Duration::from_millis(100), self.receive_message_tracked()).await {
            if auth_message?.command == AdbCommand::AUTH {
                // Send simplified AUTH response
                let auth_response = AdbMessage {
                    command: AdbCommand::CNXN,
                    arg0: 0x01000000,
                    arg1: 256 * 1024,
                    data_length: 0,
                    data_checksum: 0,
                    magic: !AdbCommand::CNXN as u32,
                    payload: Bytes::new(),
                };
                self.send_message_tracked(&auth_response).await?;
            }
        }

        Ok(())
    }

    async fn create_service_stream(&mut self, service_name: &str) -> Result<u32> {
        if self.state != ConnectionState::Connected {
            return Err(anyhow::anyhow!("Not connected"));
        }

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

        self.send_message_tracked(&open_message).await?;

        let response = self.receive_message_tracked().await?;
        if response.command != AdbCommand::OKAY {
            return Err(anyhow::anyhow!("Failed to open service: {}", service_name));
        }

        let remote_id = response.arg0;
        let stream_info = StreamInfo {
            local_id,
            remote_id,
            service_name: service_name.to_string(),
            created_at: std::time::Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
        };

        self.active_streams.insert(local_id, stream_info);

        Ok(local_id)
    }

    async fn send_stream_data(&mut self, local_id: u32, data: &[u8]) -> Result<()> {
        if let Some(stream_info) = self.active_streams.get_mut(&local_id) {
            let checksum = crc32fast::hash(data);
            let payload = Bytes::from(data.to_vec());

            let write_message = AdbMessage {
                command: AdbCommand::WRTE,
                arg0: local_id,
                arg1: stream_info.remote_id,
                data_length: payload.len() as u32,
                data_checksum: checksum,
                magic: !AdbCommand::WRTE as u32,
                payload,
            };

            self.send_message_tracked(&write_message).await?;

            // Wait for OKAY acknowledgment
            let response = self.receive_message_tracked().await?;
            if response.command != AdbCommand::OKAY {
                return Err(anyhow::anyhow!("Data send not acknowledged"));
            }

            stream_info.bytes_sent += data.len() as u64;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Stream not found"))
        }
    }

    async fn receive_stream_data(&mut self, local_id: u32) -> Result<Vec<u8>> {
        let response = self.receive_message_tracked().await?;
        if response.command == AdbCommand::WRTE {
            // Send OKAY acknowledgment
            if let Some(stream_info) = self.active_streams.get_mut(&local_id) {
                let ack_message = AdbMessage {
                    command: AdbCommand::OKAY,
                    arg0: local_id,
                    arg1: stream_info.remote_id,
                    data_length: 0,
                    data_checksum: 0,
                    magic: !AdbCommand::OKAY as u32,
                    payload: Bytes::new(),
                };
                self.send_message_tracked(&ack_message).await?;

                stream_info.bytes_received += response.payload.len() as u64;
            }

            Ok(response.payload.to_vec())
        } else {
            Err(anyhow::anyhow!("Expected data response"))
        }
    }

    async fn close_stream(&mut self, local_id: u32) -> Result<()> {
        if let Some(stream_info) = self.active_streams.get(&local_id) {
            let close_message = AdbMessage {
                command: AdbCommand::CLSE,
                arg0: local_id,
                arg1: stream_info.remote_id,
                data_length: 0,
                data_checksum: 0,
                magic: !AdbCommand::CLSE as u32,
                payload: Bytes::new(),
            };

            self.send_message_tracked(&close_message).await?;

            // Wait for CLSE acknowledgment
            let response = self.receive_message_tracked().await?;
            if response.command == AdbCommand::CLSE {
                self.active_streams.remove(&local_id);
                Ok(())
            } else {
                Err(anyhow::anyhow!("Stream close not acknowledged"))
            }
        } else {
            Err(anyhow::anyhow!("Stream not found"))
        }
    }

    async fn disconnect_graceful(&mut self) -> Result<()> {
        // Close all active streams first
        let stream_ids: Vec<u32> = self.active_streams.keys().cloned().collect();
        for stream_id in stream_ids {
            let _ = self.close_stream(stream_id).await;
        }

        // Close connection
        if let Some(stream) = &mut self.stream {
            let _ = stream.shutdown().await;
        }

        self.stream = None;
        self.state = ConnectionState::Disconnected;
        self.active_streams.clear();

        Ok(())
    }

    async fn force_disconnect(&mut self) -> Result<()> {
        // Forcefully close without proper cleanup
        if let Some(stream) = &mut self.stream {
            let _ = stream.shutdown().await;
        }

        self.stream = None;
        self.state = ConnectionState::Disconnected;
        self.active_streams.clear();

        Ok(())
    }

    async fn send_message_tracked(&mut self, message: &AdbMessage) -> Result<()> {
        if let Some(stream) = &mut self.stream {
            let serialized = message.serialize()?;
            stream.write_all(&serialized).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Not connected"))
        }
    }

    async fn receive_message_tracked(&mut self) -> Result<AdbMessage> {
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

    fn get_connection_metrics(&self) -> ConnectionMetrics {
        let total_connection_time = if let (Some(start), Some(handshake)) =
            (self.connection_start_time, self.handshake_complete_time) {
            Some(handshake.duration_since(start))
        } else {
            None
        };

        let total_bytes_sent = self.active_streams.values().map(|s| s.bytes_sent).sum();
        let total_bytes_received = self.active_streams.values().map(|s| s.bytes_received).sum();

        ConnectionMetrics {
            state: self.state.clone(),
            connection_time: total_connection_time,
            active_stream_count: self.active_streams.len(),
            total_bytes_sent,
            total_bytes_received,
            streams: self.active_streams.clone(),
        }
    }

    fn is_connected(&self) -> bool {
        self.state == ConnectionState::Connected
    }
}

#[derive(Debug, Clone)]
struct ConnectionMetrics {
    state: ConnectionState,
    connection_time: Option<Duration>,
    active_stream_count: usize,
    total_bytes_sent: u64,
    total_bytes_received: u64,
    streams: HashMap<u32, StreamInfo>,
}

// Mock daemon for lifecycle testing
struct E2eLifecycleDaemon {
    child: Option<tokio::process::Child>,
    port: u16,
    config_file: String,
    start_time: Option<std::time::Instant>,
}

impl E2eLifecycleDaemon {
    async fn new() -> Result<Self> {
        let config = ServerConfig {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 0,
            max_clients: 20,
            client_timeout: Duration::from_secs(60),
            enable_logging: true,
        };

        let config_json = serde_json::to_string_pretty(&config)?;
        let config_file = format!("/tmp/dbgif_lifecycle_config_{}.json", rand::random::<u32>());
        tokio::fs::write(&config_file, config_json).await?;

        Ok(Self {
            child: None,
            port: 5037,
            config_file,
            start_time: None,
        })
    }

    async fn start(&mut self) -> Result<()> {
        let mut cmd = TokioCommand::new("target/debug/dbgif-server");
        cmd.arg("--config").arg(&self.config_file);
        cmd.arg("--port").arg(self.port.to_string());
        cmd.kill_on_drop(true);

        let child = cmd.spawn()?;
        self.child = Some(child);
        self.start_time = Some(std::time::Instant::now());

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

    async fn restart(&mut self) -> Result<()> {
        self.stop().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        self.start().await?;
        Ok(())
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn uptime(&self) -> Option<Duration> {
        self.start_time.map(|start| start.elapsed())
    }
}

#[tokio::test]
async fn test_e2e_connection_establishment_lifecycle() {
    let mut daemon = E2eLifecycleDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eLifecycleClient::new("127.0.0.1", daemon.port());

    // Test initial state
    assert_eq!(client.state, ConnectionState::Disconnected);
    assert!(!client.is_connected());

    // Test connection establishment
    let connection_time = client.connect_with_lifecycle().await.expect("Failed to connect");
    assert!(client.is_connected());
    assert!(connection_time < Duration::from_secs(5), "Connection should be fast");

    // Test connection metrics
    let metrics = client.get_connection_metrics();
    assert_eq!(metrics.state, ConnectionState::Connected);
    assert!(metrics.connection_time.is_some());
    assert_eq!(metrics.active_stream_count, 0);

    // Test graceful disconnection
    client.disconnect_graceful().await.expect("Failed to disconnect");
    assert_eq!(client.state, ConnectionState::Disconnected);

    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_stream_lifecycle_management() {
    let mut daemon = E2eLifecycleDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eLifecycleClient::new("127.0.0.1", daemon.port());
    client.connect_with_lifecycle().await.expect("Failed to connect");

    // Test stream creation
    let stream1_id = client.create_service_stream("host:devices").await.expect("Failed to create stream 1");
    let stream2_id = client.create_service_stream("shell:").await.expect("Failed to create stream 2");

    assert_ne!(stream1_id, stream2_id);

    let metrics = client.get_connection_metrics();
    assert_eq!(metrics.active_stream_count, 2);

    // Test stream data transfer
    let test_data = b"test stream data";
    client.send_stream_data(stream1_id, test_data).await.expect("Failed to send data");

    let metrics_after_send = client.get_connection_metrics();
    assert!(metrics_after_send.total_bytes_sent >= test_data.len() as u64);

    // Test stream closure
    client.close_stream(stream1_id).await.expect("Failed to close stream 1");

    let metrics_after_close = client.get_connection_metrics();
    assert_eq!(metrics_after_close.active_stream_count, 1);

    // Test remaining stream
    client.close_stream(stream2_id).await.expect("Failed to close stream 2");

    let final_metrics = client.get_connection_metrics();
    assert_eq!(final_metrics.active_stream_count, 0);

    client.disconnect_graceful().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_connection_resilience() {
    let mut daemon = E2eLifecycleDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eLifecycleClient::new("127.0.0.1", daemon.port());
    client.connect_with_lifecycle().await.expect("Failed to connect");

    // Create streams and send data
    let stream_id = client.create_service_stream("host:devices").await.expect("Failed to create stream");
    client.send_stream_data(stream_id, b"test data").await.expect("Failed to send data");

    // Test daemon restart while client is connected
    daemon.restart().await.expect("Failed to restart daemon");

    // Client should detect disconnection
    let send_result = client.send_stream_data(stream_id, b"more data").await;
    assert!(send_result.is_err(), "Should fail after daemon restart");

    // Test reconnection after daemon restart
    let mut new_client = E2eLifecycleClient::new("127.0.0.1", daemon.port());
    let reconnect_result = new_client.connect_with_lifecycle().await;
    assert!(reconnect_result.is_ok(), "Should be able to reconnect after restart");

    new_client.disconnect_graceful().await.expect("Failed to disconnect new client");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_concurrent_connections_lifecycle() {
    let mut daemon = E2eLifecycleDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let client_count = 5;
    let daemon_port = daemon.port();

    // Create multiple concurrent connections
    let mut connection_tasks = Vec::new();

    for i in 0..client_count {
        let task = tokio::spawn(async move {
            let mut client = E2eLifecycleClient::new("127.0.0.1", daemon_port);

            // Connect
            let connection_time = client.connect_with_lifecycle().await?;

            // Create and use streams
            let stream_id = client.create_service_stream("host:devices").await?;
            client.send_stream_data(stream_id, format!("data from client {}", i).as_bytes()).await?;

            // Measure session duration
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Get metrics before closing
            let metrics = client.get_connection_metrics();

            // Close properly
            client.close_stream(stream_id).await?;
            client.disconnect_graceful().await?;

            Ok::<(Duration, ConnectionMetrics), anyhow::Error>((connection_time, metrics))
        });
        connection_tasks.push(task);
    }

    // Wait for all connections to complete
    let results = futures::future::join_all(connection_tasks).await;

    // Analyze results
    let mut total_connection_time = Duration::new(0, 0);
    let mut success_count = 0;

    for result in results {
        let task_result = result.expect("Task should not panic");
        if let Ok((connection_time, metrics)) = task_result {
            total_connection_time += connection_time;
            success_count += 1;

            assert_eq!(metrics.state, ConnectionState::Connected);
            assert!(metrics.total_bytes_sent > 0);
        }
    }

    assert_eq!(success_count, client_count, "All clients should succeed");

    let avg_connection_time = total_connection_time / client_count as u32;
    assert!(avg_connection_time < Duration::from_secs(2), "Average connection time should be reasonable");

    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_error_recovery_lifecycle() {
    let mut daemon = E2eLifecycleDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eLifecycleClient::new("127.0.0.1", daemon.port());
    client.connect_with_lifecycle().await.expect("Failed to connect");

    // Create stream
    let stream_id = client.create_service_stream("host:devices").await.expect("Failed to create stream");

    // Force disconnect (simulate network error)
    client.force_disconnect().await.expect("Failed to force disconnect");
    assert_eq!(client.state, ConnectionState::Disconnected);

    // Attempt operations on disconnected client
    let send_result = client.send_stream_data(stream_id, b"data").await;
    assert!(send_result.is_err(), "Should fail on disconnected client");

    // Test reconnection
    let reconnect_result = client.connect_with_lifecycle().await;
    assert!(reconnect_result.is_ok(), "Should be able to reconnect");

    // Test that old stream is no longer valid
    let old_stream_send = client.send_stream_data(stream_id, b"data").await;
    assert!(old_stream_send.is_err(), "Old stream should be invalid");

    // Create new stream after reconnection
    let new_stream_id = client.create_service_stream("host:devices").await.expect("Failed to create new stream");
    assert_ne!(stream_id, new_stream_id);

    client.disconnect_graceful().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_connection_timeout_handling() {
    let mut daemon = E2eLifecycleDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eLifecycleClient::new("127.0.0.1", daemon.port());
    client.connect_with_lifecycle().await.expect("Failed to connect");

    // Create stream
    let stream_id = client.create_service_stream("host:devices").await.expect("Failed to create stream");

    // Stop daemon while client is connected
    daemon.stop().await.expect("Failed to stop daemon");

    // Operations should timeout or fail
    let timeout_result = tokio::time::timeout(
        Duration::from_secs(5),
        client.send_stream_data(stream_id, b"timeout test")
    ).await;

    // Should either timeout or fail quickly
    match timeout_result {
        Ok(Err(_)) => {
            // Failed immediately - good
        }
        Err(_) => {
            // Timed out - also acceptable
        }
        Ok(Ok(_)) => {
            panic!("Should not succeed when daemon is stopped");
        }
    }
}

#[tokio::test]
async fn test_e2e_performance_under_load() {
    let mut daemon = E2eLifecycleDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eLifecycleClient::new("127.0.0.1", daemon.port());
    client.connect_with_lifecycle().await.expect("Failed to connect");

    // Create multiple streams and send data rapidly
    let stream_count = 10;
    let mut stream_ids = Vec::new();

    // Create streams
    let start_time = std::time::Instant::now();
    for i in 0..stream_count {
        let service_name = format!("service_{}", i);
        let stream_id = client.create_service_stream(&service_name).await.expect("Failed to create stream");
        stream_ids.push(stream_id);
    }
    let stream_creation_time = start_time.elapsed();

    // Send data on all streams
    let data_start_time = std::time::Instant::now();
    for (i, &stream_id) in stream_ids.iter().enumerate() {
        let data = format!("load test data for stream {}", i);
        client.send_stream_data(stream_id, data.as_bytes()).await.expect("Failed to send data");
    }
    let data_send_time = data_start_time.elapsed();

    // Close all streams
    let close_start_time = std::time::Instant::now();
    for &stream_id in &stream_ids {
        client.close_stream(stream_id).await.expect("Failed to close stream");
    }
    let stream_close_time = close_start_time.elapsed();

    // Verify performance metrics
    assert!(stream_creation_time < Duration::from_secs(5), "Stream creation should be fast");
    assert!(data_send_time < Duration::from_secs(5), "Data sending should be fast");
    assert!(stream_close_time < Duration::from_secs(5), "Stream closing should be fast");

    let metrics = client.get_connection_metrics();
    assert_eq!(metrics.active_stream_count, 0);
    assert!(metrics.total_bytes_sent > 0);

    client.disconnect_graceful().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}

#[tokio::test]
async fn test_e2e_long_running_session() {
    let mut daemon = E2eLifecycleDaemon::new().await.expect("Failed to create daemon");
    daemon.start().await.expect("Failed to start daemon");

    let mut client = E2eLifecycleClient::new("127.0.0.1", daemon.port());
    client.connect_with_lifecycle().await.expect("Failed to connect");

    // Simulate long-running session with periodic activity
    let session_duration = Duration::from_secs(5); // Reduced for testing
    let activity_interval = Duration::from_millis(500);
    let start_time = std::time::Instant::now();

    let stream_id = client.create_service_stream("host:devices").await.expect("Failed to create stream");

    let mut activity_count = 0;
    while start_time.elapsed() < session_duration {
        // Send periodic data
        let data = format!("activity {}", activity_count);
        client.send_stream_data(stream_id, data.as_bytes()).await.expect("Failed to send periodic data");
        activity_count += 1;

        tokio::time::sleep(activity_interval).await;
    }

    // Verify session remained stable
    assert!(client.is_connected(), "Should remain connected during long session");

    let metrics = client.get_connection_metrics();
    assert!(metrics.total_bytes_sent > 0);
    assert_eq!(metrics.active_stream_count, 1);

    client.close_stream(stream_id).await.expect("Failed to close stream");
    client.disconnect_graceful().await.expect("Failed to disconnect");
    daemon.stop().await.expect("Failed to stop daemon");
}