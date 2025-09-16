use async_trait::async_trait;
use crate::host_services::{HostService, HostServiceResponse};
use crate::protocol::error::ProtocolError;
use crate::server::session::ClientSessionInfo;
use crate::server::device_registry::{DeviceRegistry, DeviceState};
use std::sync::Arc;

/// Host service for listing available devices
/// Implements the "host:list" service that returns all registered devices
#[derive(Debug)]
pub struct HostListService {
    device_registry: Arc<DeviceRegistry>,
}

impl HostListService {
    /// Create a new host list service
    pub fn new(device_registry: Arc<DeviceRegistry>) -> Self {
        Self {
            device_registry,
        }
    }

    /// Format device information for host:list response
    /// Format: <device_id> <status> <systemtype> <model> <version>
    fn format_device_line(device_id: &str, state: &DeviceState, systemtype: &str, model: &str, version: &str) -> String {
        let status = match state {
            DeviceState::Connected => "device",
            DeviceState::Connecting => "device", // Show as device if actively connecting
            DeviceState::Discovered | DeviceState::Offline => "offline",
            DeviceState::Disconnecting => "offline",
            DeviceState::Error { .. } => "offline",
        };

        format!("{} {} {} {} {}", device_id, status, systemtype, model, version)
    }

    /// Generate the full device list response
    async fn generate_device_list(&self) -> Result<String, ProtocolError> {
        let devices = self.device_registry.list_devices().map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to list devices from registry".to_string(),
            }
        })?;

        let mut lines = Vec::new();

        for device in devices {
            // Extract metadata for formatting
            let systemtype = device.metadata.properties
                .get("systemtype")
                .map(|s| s.as_str())
                .unwrap_or("unknown");

            let model = device.metadata.model
                .as_deref()
                .unwrap_or("unknown");

            let version = device.metadata.os_version
                .as_deref()
                .unwrap_or("unknown");

            let line = Self::format_device_line(
                &device.device_id,
                &device.state,
                systemtype,
                model,
                version,
            );

            lines.push(line);
        }

        // Join with newlines and add trailing newline if devices exist
        let result = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };

        Ok(result)
    }
}

#[async_trait]
impl HostService for HostListService {
    fn service_name(&self) -> &str {
        "host:list"
    }

    async fn handle(&self, _session: &ClientSessionInfo, _args: &str) -> Result<HostServiceResponse, ProtocolError> {
        // host:list doesn't use args parameter
        match self.generate_device_list().await {
            Ok(device_list) => Ok(HostServiceResponse::okay_str(&device_list)),
            Err(e) => {
                tracing::error!("Failed to generate device list: {}", e);
                Err(ProtocolError::HostServiceError {
                    service: "host:list".to_string(),
                    message: format!("Failed to list devices: {}", e),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::device_registry::{DeviceInfo, DeviceMetadata, DeviceCapabilities, DeviceState};
    use std::net::SocketAddr;
    use std::time::Instant;

    fn create_test_session() -> ClientSessionInfo {
        use crate::server::session::{ClientInfo, SessionState, SessionStats, ClientCapabilities};
        use std::collections::HashMap;

        ClientSessionInfo {
            session_id: "test-session".to_string(),
            client_info: ClientInfo {
                connection_id: "tcp://127.0.0.1:12345→127.0.0.1:5555".to_string(),
                identity: Some("test-client".to_string()),
                protocol_version: 1,
                max_data_size: 1024 * 1024,
                capabilities: ClientCapabilities::default(),
            },
            state: SessionState::Active,
            streams: HashMap::new(),
            stats: SessionStats::default(),
            established_at: Instant::now(),
            last_activity: Instant::now(),
        }
    }

    fn create_test_device(id: &str, state: DeviceState, systemtype: &str, model: &str, version: &str) -> DeviceInfo {
        let mut metadata = DeviceMetadata::default();
        metadata.model = Some(model.to_string());
        metadata.os_version = Some(version.to_string());
        metadata.properties.insert("systemtype".to_string(), systemtype.to_string());

        DeviceInfo {
            device_id: id.to_string(),
            name: format!("Test Device {}", id),
            address: "127.0.0.1:5555".parse::<SocketAddr>().unwrap(),
            state,
            capabilities: DeviceCapabilities::default(),
            metadata,
            last_seen: Instant::now(),
            stats: crate::server::device_registry::DeviceStats::default(),
        }
    }

    #[test]
    fn test_host_list_service_creation() {
        let registry = Arc::new(DeviceRegistry::new());
        let service = HostListService::new(registry);
        assert_eq!(service.service_name(), "host:list");
    }

    #[test]
    fn test_format_device_line() {
        let line = HostListService::format_device_line(
            "tcp:device001",
            &DeviceState::Connected,
            "tizen",
            "MyBoard",
            "v1.2",
        );
        assert_eq!(line, "tcp:device001 device tizen MyBoard v1.2");

        let line_offline = HostListService::format_device_line(
            "usb:device123",
            &DeviceState::Offline,
            "seret",
            "DevBoard",
            "v2.0",
        );
        assert_eq!(line_offline, "usb:device123 offline seret DevBoard v2.0");
    }

    #[tokio::test]
    async fn test_empty_device_list() {
        let registry = Arc::new(DeviceRegistry::new());
        let service = HostListService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                let response = String::from_utf8(data).unwrap();
                assert!(response.is_empty()); // Empty list for no devices
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[tokio::test]
    async fn test_device_list_with_devices() {
        let registry = Arc::new(DeviceRegistry::new());

        // Add test devices
        let device1 = create_test_device("tcp:custom001", DeviceState::Offline, "tizen", "MyBoard", "v1.2");
        let device2 = create_test_device("usb:device123", DeviceState::Connected, "seret", "DevBoard", "v2.0");

        registry.register_device(device1).unwrap();
        registry.register_device(device2).unwrap();

        let service = HostListService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                let response = String::from_utf8(data).unwrap();

                // Should contain both devices with proper formatting
                assert!(response.contains("tcp:custom001 offline tizen MyBoard v1.2"));
                assert!(response.contains("usb:device123 device seret DevBoard v2.0"));
                assert!(response.ends_with('\n')); // Trailing newline
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[tokio::test]
    async fn test_device_state_mapping() {
        // Test different device states are properly mapped
        assert_eq!(
            HostListService::format_device_line("test", &DeviceState::Connected, "type", "model", "ver"),
            "test device type model ver"
        );

        assert_eq!(
            HostListService::format_device_line("test", &DeviceState::Connecting, "type", "model", "ver"),
            "test device type model ver"
        );

        assert_eq!(
            HostListService::format_device_line("test", &DeviceState::Offline, "type", "model", "ver"),
            "test offline type model ver"
        );

        assert_eq!(
            HostListService::format_device_line("test", &DeviceState::Discovered, "type", "model", "ver"),
            "test offline type model ver"
        );

        assert_eq!(
            HostListService::format_device_line("test", &DeviceState::Disconnecting, "type", "model", "ver"),
            "test offline type model ver"
        );

        assert_eq!(
            HostListService::format_device_line("test", &DeviceState::Error { message: "test error".to_string() }, "type", "model", "ver"),
            "test offline type model ver"
        );
    }

    #[tokio::test]
    async fn test_device_with_missing_metadata() {
        let registry = Arc::new(DeviceRegistry::new());

        // Create device with minimal metadata
        let mut device = create_test_device("test:device", DeviceState::Connected, "unknown", "unknown", "unknown");
        device.metadata.model = None;
        device.metadata.os_version = None;
        device.metadata.properties.clear();

        registry.register_device(device).unwrap();

        let service = HostListService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                let response = String::from_utf8(data).unwrap();
                assert_eq!(response, "test:device device unknown unknown unknown\n");
            }
            _ => panic!("Expected Okay response"),
        }
    }
}