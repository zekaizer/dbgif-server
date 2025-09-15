// Daemon API contract for client connections
// Defines how clients interact with the USB bridge daemon

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// TCP server configuration for daemon
pub struct DaemonConfig {
    pub listen_address: String,  // Default: "127.0.0.1"
    pub listen_port: u16,        // Default: 5037 (ADB standard)
    pub max_clients: usize,      // Default: 10
    pub buffer_size: usize,      // Default: 256KB
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 5037,
            max_clients: 10,
            buffer_size: 256 * 1024,
        }
    }
}

/// Client connection handler interface
#[async_trait::async_trait]
pub trait ClientHandler: Send + Sync {
    /// Handle new client connection
    async fn handle_client(&self, stream: tokio::net::TcpStream) -> anyhow::Result<()>;

    /// Process ADB command from client
    async fn process_command(&self, command: &str) -> anyhow::Result<String>;

    /// Handle client disconnection
    async fn client_disconnected(&self, client_id: uuid::Uuid);

    /// Get active client count
    fn active_client_count(&self) -> usize;
}

/// Device status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStatus {
    pub device_id: String,           // USB bridge identifier
    pub state: DeviceState,          // Current connection state
    pub transport_type: String,      // "usb_bridge"
    pub product: String,             // "PL25A1"
    pub model: String,               // Device model if available
    pub device_path: String,         // USB device path
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceState {
    #[serde(rename = "device")]
    Connected,
    #[serde(rename = "offline")]
    Disconnected,
    #[serde(rename = "unauthorized")]
    Unauthorized,
    #[serde(rename = "connecting")]
    Connecting,
}

/// ADB host commands that the daemon must support
pub enum AdbHostCommand {
    /// List connected devices
    Devices,
    /// List devices with additional info
    DevicesLong,
    /// Get device state
    GetState(String),
    /// Connect to specific device
    Transport(String),
    /// Kill daemon
    Kill,
    /// Get daemon version
    Version,
}

impl AdbHostCommand {
    /// Parse command string from client
    pub fn parse(command: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = command.trim().split(':').collect();

        match parts.as_slice() {
            ["host", "devices"] => Ok(AdbHostCommand::Devices),
            ["host", "devices-l"] => Ok(AdbHostCommand::DevicesLong),
            ["host", "get-state"] => Ok(AdbHostCommand::GetState("".to_string())),
            ["host", "transport", device] => Ok(AdbHostCommand::Transport(device.to_string())),
            ["host", "kill"] => Ok(AdbHostCommand::Kill),
            ["host", "version"] => Ok(AdbHostCommand::Version),
            _ => Err(anyhow::anyhow!("Unknown command: {}", command)),
        }
    }

    /// Format response for client
    pub fn format_response(&self, device_list: &[DeviceStatus]) -> String {
        match self {
            AdbHostCommand::Devices => {
                device_list
                    .iter()
                    .map(|d| format!("{}\t{:?}", d.device_id, d.state))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            AdbHostCommand::DevicesLong => {
                device_list
                    .iter()
                    .map(|d| {
                        format!(
                            "{}\t{:?} usb:{} product:{} model:{} device:{}",
                            d.device_id, d.state, d.device_path, d.product, d.model, d.device_path
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            AdbHostCommand::GetState(device_id) => {
                device_list
                    .iter()
                    .find(|d| d.device_id == *device_id)
                    .map(|d| format!("{:?}", d.state))
                    .unwrap_or("unknown".to_string())
            }
            AdbHostCommand::Transport(device_id) => {
                if device_list.iter().any(|d| d.device_id == *device_id) {
                    "OKAY".to_string()
                } else {
                    "FAIL".to_string()
                }
            }
            AdbHostCommand::Kill => "".to_string(), // Daemon will shutdown
            AdbHostCommand::Version => "0041".to_string(), // ADB protocol version
        }
    }
}

/// Stream multiplexing for concurrent ADB sessions
#[derive(Debug, Clone)]
pub struct StreamManager {
    pub streams: HashMap<u32, StreamInfo>,
    pub next_local_id: u32,
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub local_id: u32,
    pub remote_id: Option<u32>,
    pub service: String,
    pub state: crate::contracts::adb_protocol::StreamState,
    pub client_id: uuid::Uuid,
}

impl StreamManager {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            next_local_id: 1,
        }
    }

    /// Allocate new local stream ID
    pub fn allocate_stream_id(&mut self) -> u32 {
        let id = self.next_local_id;
        self.next_local_id += 1;
        id
    }

    /// Register new stream
    pub fn add_stream(&mut self, service: String, client_id: uuid::Uuid) -> u32 {
        let local_id = self.allocate_stream_id();
        let info = StreamInfo {
            local_id,
            remote_id: None,
            service,
            state: crate::contracts::adb_protocol::StreamState::Opening,
            client_id,
        };
        self.streams.insert(local_id, info);
        local_id
    }

    /// Update stream with remote ID
    pub fn set_remote_id(&mut self, local_id: u32, remote_id: u32) {
        if let Some(stream) = self.streams.get_mut(&local_id) {
            stream.remote_id = Some(remote_id);
            stream.state = crate::contracts::adb_protocol::StreamState::Open;
        }
    }

    /// Remove stream
    pub fn remove_stream(&mut self, local_id: u32) {
        self.streams.remove(&local_id);
    }

    /// Get stream by local ID
    pub fn get_stream(&self, local_id: u32) -> Option<&StreamInfo> {
        self.streams.get(&local_id)
    }
}

// Contract tests would verify:
// 1. TCP server accepts client connections on port 5037
// 2. ADB host commands parsed and formatted correctly
// 3. Device status updates reflected in responses
// 4. Stream multiplexing handles concurrent sessions
// 5. Client disconnection cleanup works properly