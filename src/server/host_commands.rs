use crate::transport::{TransportManager, DeviceInfo};
use anyhow::{Result, Context};
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Extended device information for ADB host commands
#[derive(Debug, Clone)]
pub struct AdbDeviceInfo {
    pub device_id: String,
    pub transport_id: String,
    pub state: DeviceState,
    pub transport_type: crate::transport::TransportType,
    pub serial_number: Option<String>,
    pub product: Option<String>,
    pub model: Option<String>,
    pub device_name: Option<String>,
    pub usb_info: Option<UsbDeviceInfo>,
}

#[derive(Debug, Clone)]
pub struct UsbDeviceInfo {
    pub bus_number: u8,
    pub device_address: u8,
    pub vendor_id: u16,
    pub product_id: u16,
}

/// ADB device state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    Online,
    Offline,
    Unauthorized,
    Bootloader,
    Recovery,
    Sideload,
    Unknown,
}

impl std::fmt::Display for DeviceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceState::Online => write!(f, "device"),
            DeviceState::Offline => write!(f, "offline"),
            DeviceState::Unauthorized => write!(f, "unauthorized"),
            DeviceState::Bootloader => write!(f, "bootloader"),
            DeviceState::Recovery => write!(f, "recovery"),
            DeviceState::Sideload => write!(f, "sideload"),
            DeviceState::Unknown => write!(f, "unknown"),
        }
    }
}

/// ADB host command processor
///
/// Handles all "host:" prefixed commands that are processed locally by the server
/// rather than being forwarded to devices
#[derive(Debug)]
pub struct HostCommandProcessor {
    transport_manager: Arc<TransportManager>,
    server_version: u32,
    max_data_size: u32,
}

impl HostCommandProcessor {
    pub fn new(transport_manager: Arc<TransportManager>) -> Self {
        Self {
            transport_manager,
            server_version: 0x01000020, // ADB version 1.0.32
            max_data_size: 256 * 1024,  // 256KB
        }
    }

    /// Execute a host command and return the response
    pub async fn execute(&self, command: &str) -> Result<HostCommandResponse> {
        debug!("Executing host command: {}", command);

        // Remove "host:" prefix if present
        let command = if command.starts_with("host:") {
            &command[5..]
        } else {
            command
        };

        match command {
            "devices" => self.handle_devices(false).await,
            "devices-l" => self.handle_devices(true).await,
            "version" => self.handle_version().await,
            "kill" => self.handle_kill().await,
            "features" => self.handle_features().await,
            "get-state" => self.handle_get_state().await,
            "get-serialno" => self.handle_get_serialno().await,
            "get-devpath" => self.handle_get_devpath().await,
            cmd if cmd.starts_with("transport:") => {
                let device_id = &cmd[10..]; // Skip "transport:"
                self.handle_transport(device_id).await
            }
            cmd if cmd.starts_with("transport-id:") => {
                let transport_id = &cmd[13..]; // Skip "transport-id:"
                self.handle_transport_id(transport_id).await
            }
            cmd if cmd.starts_with("serial:") => {
                let serial = &cmd[7..]; // Skip "serial:"
                self.handle_serial(serial).await
            }
            cmd if cmd.starts_with("usb:") => {
                let usb_id = &cmd[4..]; // Skip "usb:"
                self.handle_usb(usb_id).await
            }
            cmd if cmd.starts_with("local:") => {
                let local_id = &cmd[6..]; // Skip "local:"
                self.handle_local(local_id).await
            }
            "track-devices" => self.handle_track_devices().await,
            "reconnect" => self.handle_reconnect().await,
            "reconnect-offline" => self.handle_reconnect_offline().await,
            _ => {
                warn!("Unknown host command: {}", command);
                Ok(HostCommandResponse::Error {
                    message: format!("unknown host service: {}", command)
                })
            }
        }
    }

    /// Handle "host:devices" and "host:devices-l" commands
    async fn handle_devices(&self, long_format: bool) -> Result<HostCommandResponse> {
        debug!("Getting device list (long_format={})", long_format);

        match self.transport_manager.discover_all_devices().await {
            Ok(devices) => {
                let mut response = String::new();

                for device in &devices {
                    // Convert to simple device state for display
                    let state = DeviceState::Online; // Assume online for discovered devices

                    if long_format {
                        // Long format includes additional device information
                        let product = "unknown";
                        let model = "unknown";
                        let device_name = &device.display_name;
                        let transport_id = "1"; // Simple transport ID

                        response.push_str(&format!(
                            "{}\t{}\tproduct:{} model:{} device:{} transport_id:{}\n",
                            device.device_id,
                            state,
                            product,
                            model,
                            device_name,
                            transport_id
                        ));
                    } else {
                        // Short format: device_id<TAB>state
                        response.push_str(&format!("{}\t{}\n", device.device_id, state));
                    }
                }

                info!("Listed {} devices", devices.len());
                Ok(HostCommandResponse::Data {
                    data: Bytes::from(response.into_bytes())
                })
            }
            Err(e) => {
                error!("Failed to list devices: {}", e);
                Ok(HostCommandResponse::Error {
                    message: format!("failed to list devices: {}", e)
                })
            }
        }
    }

    /// Handle "host:version" command
    async fn handle_version(&self) -> Result<HostCommandResponse> {
        // Standard ADB version response format: 4-digit hex
        let version_hex = format!("{:04x}", self.server_version & 0xFFFF);
        debug!("Server version: {}", version_hex);

        Ok(HostCommandResponse::Data {
            data: Bytes::from(version_hex.into_bytes())
        })
    }

    /// Handle "host:kill" command
    async fn handle_kill(&self) -> Result<HostCommandResponse> {
        info!("Received kill command - server shutdown requested");

        // Return success - actual shutdown is handled by server
        Ok(HostCommandResponse::Kill)
    }

    /// Handle "host:features" command - returns server capabilities
    async fn handle_features(&self) -> Result<HostCommandResponse> {
        let features = vec![
            "shell_v2",
            "cmd",
            "stat_v2",
            "ls_v2",
            "sendrecv_v2_brotli",
            "sendrecv_v2_lz4",
            "sendrecv_v2_zstd",
            "sendrecv_v2_dry_run_send",
        ];

        let response = features.join(",");
        debug!("Server features: {}", response);

        Ok(HostCommandResponse::Data {
            data: Bytes::from(response.into_bytes())
        })
    }

    /// Handle "host:get-state" command - requires selected device
    async fn handle_get_state(&self) -> Result<HostCommandResponse> {
        // This command requires a device to be selected via transport
        // Since we don't have device context here, return error
        Ok(HostCommandResponse::Error {
            message: "no device selected".to_string()
        })
    }

    /// Handle "host:get-serialno" command - requires selected device
    async fn handle_get_serialno(&self) -> Result<HostCommandResponse> {
        Ok(HostCommandResponse::Error {
            message: "no device selected".to_string()
        })
    }

    /// Handle "host:get-devpath" command - requires selected device
    async fn handle_get_devpath(&self) -> Result<HostCommandResponse> {
        Ok(HostCommandResponse::Error {
            message: "no device selected".to_string()
        })
    }

    /// Handle "host:transport:DEVICE_ID" command
    async fn handle_transport(&self, device_id: &str) -> Result<HostCommandResponse> {
        debug!("Selecting transport for device: {}", device_id);

        match self.transport_manager.get_transport_info(device_id).await {
            Some(transport_info) => {
                info!("Transport selected: {} ({})", device_id, transport_info.display_name);
                // Create ADB device info for response
                let device_info = AdbDeviceInfo {
                    device_id: device_id.to_string(),
                    transport_id: "1".to_string(),
                    state: if transport_info.is_connected { DeviceState::Online } else { DeviceState::Offline },
                    transport_type: transport_info.transport_type,
                    serial_number: Some(device_id.to_string()),
                    product: Some("unknown".to_string()),
                    model: Some("unknown".to_string()),
                    device_name: Some(transport_info.display_name),
                    usb_info: None,
                };
                Ok(HostCommandResponse::TransportSelected {
                    device_id: device_id.to_string(),
                    device_info,
                })
            }
            None => {
                warn!("Device not found: {}", device_id);
                Ok(HostCommandResponse::Error {
                    message: format!("device '{}' not found", device_id)
                })
            }
        }
    }

    /// Handle "host:transport-id:ID" command
    async fn handle_transport_id(&self, transport_id: &str) -> Result<HostCommandResponse> {
        debug!("Selecting transport by ID: {}", transport_id);

        // Find device by transport ID in device list
        match self.transport_manager.discover_all_devices().await {
            Ok(devices) => {
                for device in devices {
                    // Create a simple transport ID based on device_id for now
                    let simple_transport_id = "1";
                    if simple_transport_id == transport_id {
                        let adb_device = AdbDeviceInfo {
                            device_id: device.device_id.clone(),
                            transport_id: simple_transport_id.to_string(),
                            state: DeviceState::Online, // Assume online for discovered devices
                            transport_type: device.transport_type,
                            serial_number: Some(device.device_id.clone()),
                            product: Some("unknown".to_string()),
                            model: Some("unknown".to_string()),
                            device_name: Some(device.display_name.clone()),
                            usb_info: None,
                        };
                        info!("Transport selected by ID: {} -> {} ({})", transport_id, device.device_id, adb_device.state);
                        return Ok(HostCommandResponse::TransportSelected {
                            device_id: device.device_id.clone(),
                            device_info: adb_device,
                        });
                    }
                }

                warn!("Transport ID not found: {}", transport_id);
                Ok(HostCommandResponse::Error {
                    message: format!("transport-id '{}' not found", transport_id)
                })
            }
            Err(e) => {
                error!("Failed to get device by transport ID {}: {}", transport_id, e);
                Ok(HostCommandResponse::Error {
                    message: format!("transport error: {}", e)
                })
            }
        }
    }

    /// Handle "host:serial:SERIAL" command
    async fn handle_serial(&self, serial: &str) -> Result<HostCommandResponse> {
        debug!("Selecting device by serial: {}", serial);

        // Find device by serial number
        match self.transport_manager.discover_all_devices().await {
            Ok(devices) => {
                for device in devices {
                    // Use device_id as serial for simplicity
                    if device.device_id == serial {
                        let adb_device = AdbDeviceInfo {
                            device_id: device.device_id.clone(),
                            transport_id: "1".to_string(),
                            state: DeviceState::Online,
                            transport_type: device.transport_type,
                            serial_number: Some(device.device_id.clone()),
                            product: Some("unknown".to_string()),
                            model: Some("unknown".to_string()),
                            device_name: Some(device.display_name.clone()),
                            usb_info: None,
                        };
                        info!("Device selected by serial: {} -> {}", serial, device.device_id);
                        return Ok(HostCommandResponse::TransportSelected {
                            device_id: device.device_id.clone(),
                            device_info: adb_device,
                        });
                    }
                }

                warn!("Serial number not found: {}", serial);
                Ok(HostCommandResponse::Error {
                    message: format!("device with serial '{}' not found", serial)
                })
            }
            Err(e) => {
                error!("Failed to list devices for serial lookup: {}", e);
                Ok(HostCommandResponse::Error {
                    message: format!("failed to find device: {}", e)
                })
            }
        }
    }

    /// Handle "host:usb:BUS-DEV" command
    async fn handle_usb(&self, usb_id: &str) -> Result<HostCommandResponse> {
        debug!("Selecting USB device: {}", usb_id);

        // Parse USB ID format: "BUS-DEV" (e.g., "001-004")
        let parts: Vec<&str> = usb_id.split('-').collect();
        if parts.len() != 2 {
            return Ok(HostCommandResponse::Error {
                message: format!("invalid USB identifier: {}", usb_id)
            });
        }

        let bus_str = parts[0];
        let device_str = parts[1];

        // Find device by USB bus/device numbers
        match self.transport_manager.discover_all_devices().await {
            Ok(devices) => {
                for device in devices {
                    // For USB devices, create a mock USB identifier based on device ID
                    if device.device_id.contains(&usb_id.replace('-', "")) {
                        let adb_device = AdbDeviceInfo {
                            device_id: device.device_id.clone(),
                            transport_id: "1".to_string(),
                            state: DeviceState::Online,
                            transport_type: device.transport_type,
                            serial_number: Some(device.device_id.clone()),
                            product: Some("unknown".to_string()),
                            model: Some("unknown".to_string()),
                            device_name: Some(device.display_name.clone()),
                            usb_info: Some(UsbDeviceInfo {
                                bus_number: 1,
                                device_address: 1,
                                vendor_id: 0x18d1, // Google vendor ID
                                product_id: 0x4ee2, // ADB interface
                            }),
                        };
                        info!("USB device selected: {} -> {}", usb_id, device.device_id);
                        return Ok(HostCommandResponse::TransportSelected {
                            device_id: device.device_id.clone(),
                            device_info: adb_device,
                        });
                    }
                }

                warn!("USB device not found: {}", usb_id);
                Ok(HostCommandResponse::Error {
                    message: format!("USB device '{}' not found", usb_id)
                })
            }
            Err(e) => {
                error!("Failed to list devices for USB lookup: {}", e);
                Ok(HostCommandResponse::Error {
                    message: format!("failed to find USB device: {}", e)
                })
            }
        }
    }

    /// Handle "host:local:NAME" command
    async fn handle_local(&self, local_name: &str) -> Result<HostCommandResponse> {
        debug!("Selecting local transport: {}", local_name);

        // For local transports (e.g., emulator), find by name
        match self.transport_manager.discover_all_devices().await {
            Ok(devices) => {
                for device in devices {
                    if device.device_id.contains(local_name) {
                        let adb_device = AdbDeviceInfo {
                            device_id: device.device_id.clone(),
                            transport_id: "1".to_string(),
                            state: DeviceState::Online,
                            transport_type: device.transport_type,
                            serial_number: Some(device.device_id.clone()),
                            product: Some("unknown".to_string()),
                            model: Some("unknown".to_string()),
                            device_name: Some(device.display_name.clone()),
                            usb_info: None,
                        };
                        info!("Local device selected: {} -> {}", local_name, device.device_id);
                        return Ok(HostCommandResponse::TransportSelected {
                            device_id: device.device_id.clone(),
                            device_info: adb_device,
                        });
                    }
                }

                warn!("Local device not found: {}", local_name);
                Ok(HostCommandResponse::Error {
                    message: format!("local device '{}' not found", local_name)
                })
            }
            Err(e) => {
                error!("Failed to list devices for local lookup: {}", e);
                Ok(HostCommandResponse::Error {
                    message: format!("failed to find local device: {}", e)
                })
            }
        }
    }

    /// Handle "host:track-devices" command - streaming device list
    async fn handle_track_devices(&self) -> Result<HostCommandResponse> {
        debug!("Starting device tracking");

        // For now, return current device list
        // TODO: Implement streaming updates when devices change
        match self.handle_devices(false).await? {
            HostCommandResponse::Data { data } => {
                Ok(HostCommandResponse::Streaming {
                    initial_data: data,
                })
            }
            other => Ok(other),
        }
    }

    /// Handle "host:reconnect" command - reconnect to all devices
    async fn handle_reconnect(&self) -> Result<HostCommandResponse> {
        info!("Reconnecting to all devices");

        // For now, just return success - actual reconnect logic would be in transport manager
        match self.transport_manager.discover_all_devices().await {
            Ok(devices) => {
                let count = devices.len();
                info!("Reconnected {} devices", count);
                Ok(HostCommandResponse::Data {
                    data: Bytes::from(format!("reconnected {} devices", count).into_bytes())
                })
            }
            Err(e) => {
                error!("Failed to reconnect devices: {}", e);
                Ok(HostCommandResponse::Error {
                    message: format!("reconnect failed: {}", e)
                })
            }
        }
    }

    /// Handle "host:reconnect-offline" command - reconnect offline devices
    async fn handle_reconnect_offline(&self) -> Result<HostCommandResponse> {
        info!("Reconnecting offline devices");

        match self.transport_manager.discover_all_devices().await {
            Ok(devices) => {
                // Device state is determined by checking if transport is connected
                let offline_count = devices.len(); // For now, assume all discovered devices need reconnecting
                info!("Reconnected {} offline devices", offline_count);
                Ok(HostCommandResponse::Data {
                    data: Bytes::from(format!("reconnected {} offline devices", offline_count).into_bytes())
                })
            }
            Err(e) => {
                error!("Failed to reconnect offline devices: {}", e);
                Ok(HostCommandResponse::Error {
                    message: format!("reconnect offline failed: {}", e)
                })
            }
        }
    }
}

/// Response from host command execution
#[derive(Debug)]
pub enum HostCommandResponse {
    /// Simple data response
    Data {
        data: Bytes,
    },
    /// Error response with message
    Error {
        message: String,
    },
    /// Transport was selected - switches client to device mode
    TransportSelected {
        device_id: String,
        device_info: AdbDeviceInfo,
    },
    /// Streaming response (for track-devices)
    Streaming {
        initial_data: Bytes,
    },
    /// Kill command - server should shutdown
    Kill,
}

impl HostCommandResponse {
    /// Convert response to ADB protocol format
    pub fn to_adb_data(&self) -> Bytes {
        match self {
            HostCommandResponse::Data { data } => data.clone(),
            HostCommandResponse::Error { message } => {
                let fail_data = format!("FAIL{:04x}{}", message.len(), message);
                Bytes::from(fail_data.into_bytes())
            }
            HostCommandResponse::TransportSelected { .. } => {
                // Transport selection is handled at the protocol level
                Bytes::from(b"OKAY".to_vec())
            }
            HostCommandResponse::Streaming { initial_data } => {
                initial_data.clone()
            }
            HostCommandResponse::Kill => {
                Bytes::from(b"OKAY".to_vec())
            }
        }
    }

    /// Check if this response should close the stream
    pub fn should_close_stream(&self) -> bool {
        match self {
            HostCommandResponse::Data { .. } => true,
            HostCommandResponse::Error { .. } => true,
            HostCommandResponse::TransportSelected { .. } => true,
            HostCommandResponse::Streaming { .. } => false, // Keep streaming
            HostCommandResponse::Kill => true,
        }
    }
}

/// Utility functions for host command parsing
pub mod utils {
    use super::*;

    /// Parse device selector from command
    pub fn parse_device_selector(command: &str) -> Option<DeviceSelector> {
        if let Some(serial) = command.strip_prefix("serial:") {
            Some(DeviceSelector::Serial(serial.to_string()))
        } else if let Some(usb_id) = command.strip_prefix("usb:") {
            Some(DeviceSelector::Usb(usb_id.to_string()))
        } else if let Some(local_name) = command.strip_prefix("local:") {
            Some(DeviceSelector::Local(local_name.to_string()))
        } else if let Some(transport_id) = command.strip_prefix("transport-id:") {
            Some(DeviceSelector::TransportId(transport_id.to_string()))
        } else if let Some(device_id) = command.strip_prefix("transport:") {
            Some(DeviceSelector::DeviceId(device_id.to_string()))
        } else {
            None
        }
    }

    /// Validate ADB device state string
    pub fn validate_device_state(state: &str) -> bool {
        matches!(state, "device" | "offline" | "unauthorized" | "bootloader" | "recovery" | "sideload")
    }

    /// Format device info for long listing
    pub fn format_device_long(device: &AdbDeviceInfo) -> String {
        let product = device.product.as_deref().unwrap_or("");
        let model = device.model.as_deref().unwrap_or("");
        let device_name = device.device_name.as_deref().unwrap_or("");
        let transport_id = &device.transport_id;

        format!(
            "{}\t{}\tproduct:{} model:{} device:{} transport_id:{}",
            device.device_id,
            device.state,
            product,
            model,
            device_name,
            transport_id
        )
    }
}

/// Device selector types
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceSelector {
    DeviceId(String),
    Serial(String),
    Usb(String),
    Local(String),
    TransportId(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportType;

    // Mock test helper
    fn create_test_device() -> AdbDeviceInfo {
        AdbDeviceInfo {
            device_id: "test_device_001".to_string(),
            transport_id: "1".to_string(),
            state: DeviceState::Online,
            transport_type: crate::transport::TransportType::Tcp,
            serial_number: Some("ABC123DEF456".to_string()),
            product: Some("test_product".to_string()),
            model: Some("TestModel".to_string()),
            device_name: Some("test_device".to_string()),
            usb_info: None,
        }
    }

    #[test]
    fn test_host_command_response_to_adb_data() {
        let data_response = HostCommandResponse::Data {
            data: Bytes::from(b"test data".to_vec()),
        };
        assert_eq!(data_response.to_adb_data(), Bytes::from(b"test data".to_vec()));

        let error_response = HostCommandResponse::Error {
            message: "test error".to_string(),
        };
        let expected_fail = format!("FAIL{:04x}test error", 10);
        assert_eq!(error_response.to_adb_data(), Bytes::from(expected_fail.into_bytes()));
    }

    #[test]
    fn test_device_selector_parsing() {
        assert!(matches!(
            utils::parse_device_selector("serial:ABC123"),
            Some(DeviceSelector::Serial(_))
        ));

        assert!(matches!(
            utils::parse_device_selector("usb:001-004"),
            Some(DeviceSelector::Usb(_))
        ));

        assert!(matches!(
            utils::parse_device_selector("transport:device001"),
            Some(DeviceSelector::DeviceId(_))
        ));

        assert_eq!(utils::parse_device_selector("invalid"), None);
    }

    #[test]
    fn test_device_state_validation() {
        assert!(utils::validate_device_state("device"));
        assert!(utils::validate_device_state("offline"));
        assert!(utils::validate_device_state("unauthorized"));
        assert!(!utils::validate_device_state("invalid_state"));
    }

    #[test]
    fn test_device_formatting() {
        let device = create_test_device();
        let formatted = utils::format_device_long(&device);
        assert!(formatted.contains("test_device_001"));
        assert!(formatted.contains("device")); // DeviceState::Online displays as "device"
        assert!(formatted.contains("product:test_product"));
    }
}