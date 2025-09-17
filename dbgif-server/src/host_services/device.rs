use async_trait::async_trait;
use crate::host_services::{HostService, HostServiceResponse};
use dbgif_protocol::error::ProtocolError;
use crate::server::session::ClientSessionInfo;
use crate::server::device_registry::{DeviceRegistry, DeviceState};
use std::sync::Arc;

/// Host service for device selection
/// Implements the "host:device:<device_id>" service that selects a target device
#[derive(Debug)]
pub struct HostDeviceService {
    device_registry: Arc<DeviceRegistry>,
}

impl HostDeviceService {
    /// Create a new host device service
    pub fn new(device_registry: Arc<DeviceRegistry>) -> Self {
        Self {
            device_registry,
        }
    }

    /// Extract device ID from service arguments
    /// The service name format is "host:device:<device_id>"
    /// Args should contain the device_id part after "host:device:"
    fn extract_device_id(args: &str) -> Result<&str, ProtocolError> {
        if args.is_empty() {
            return Err(ProtocolError::HostServiceError {
                service: "host:device".to_string(),
                message: "Device ID required".to_string(),
            });
        }

        // args contains the device_id
        Ok(args)
    }

    /// Validate device ID format
    /// Device IDs should follow the format: <transport>:<identifier>
    /// Examples: tcp:custom001, usb:device123
    fn validate_device_id(device_id: &str) -> Result<(), ProtocolError> {
        if device_id.is_empty() {
            return Err(ProtocolError::HostServiceError {
                service: "host:device".to_string(),
                message: "Device ID cannot be empty".to_string(),
            });
        }

        // Check for basic format: should contain at least one colon
        if !device_id.contains(':') {
            return Err(ProtocolError::HostServiceError {
                service: "host:device".to_string(),
                message: format!("Invalid device ID format: '{}'. Expected format: <transport>:<identifier>", device_id),
            });
        }

        // Additional validation could be added here for specific transport types
        Ok(())
    }

    /// Check if device is available for selection
    fn is_device_available(state: &DeviceState) -> bool {
        match state {
            DeviceState::Connected => true,
            DeviceState::Discovered => true, // Can select discovered devices (lazy connection)
            DeviceState::Connecting => false, // Wait for connection to complete
            DeviceState::Disconnecting => false,
            DeviceState::Offline => false,
            DeviceState::Error { .. } => false,
        }
    }

    /// Select a device for the session
    async fn select_device(&self, device_id: &str) -> Result<HostServiceResponse, ProtocolError> {
        // Validate device ID format
        Self::validate_device_id(device_id)?;

        // Check if device exists in registry
        let device_info = self.device_registry.get_device(device_id).map_err(|_| {
            ProtocolError::InternalError {
                message: "Failed to access device registry".to_string(),
            }
        })?;

        match device_info {
            Some(device) => {
                // Check if device is available for selection
                if Self::is_device_available(&device.state) {
                    // Device selection successful
                    // Note: The actual session state update would be handled by the server
                    // This service just validates the device selection request
                    tracing::info!("Device '{}' selected successfully", device_id);
                    Ok(HostServiceResponse::okay_empty())
                } else {
                    // Device exists but not available
                    let reason = match device.state {
                        DeviceState::Connecting => "device is connecting",
                        DeviceState::Disconnecting => "device is disconnecting",
                        DeviceState::Offline => "device is offline",
                        DeviceState::Error { ref message } => &format!("device error: {}", message),
                        _ => "device not available",
                    };

                    Ok(HostServiceResponse::close(reason))
                }
            }
            None => {
                // Device not found
                Ok(HostServiceResponse::close("device not found"))
            }
        }
    }
}

#[async_trait]
impl HostService for HostDeviceService {
    fn service_name(&self) -> &str {
        "host:device"
    }

    async fn handle(&self, _session: &ClientSessionInfo, args: &str) -> Result<HostServiceResponse, ProtocolError> {
        // Extract device ID from args
        let device_id = Self::extract_device_id(args)?;

        // Attempt to select the device
        self.select_device(device_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::device_registry::{DeviceInfo, DeviceMetadata, DeviceCapabilities};
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

    fn create_test_device(id: &str, state: DeviceState) -> DeviceInfo {
        DeviceInfo {
            device_id: id.to_string(),
            name: format!("Test Device {}", id),
            address: "127.0.0.1:5555".parse::<SocketAddr>().unwrap(),
            state,
            capabilities: DeviceCapabilities::default(),
            metadata: DeviceMetadata::default(),
            last_seen: Instant::now(),
            stats: crate::server::device_registry::DeviceStats::default(),
        }
    }

    #[test]
    fn test_host_device_service_creation() {
        let registry = Arc::new(DeviceRegistry::new());
        let service = HostDeviceService::new(registry);
        assert_eq!(service.service_name(), "host:device");
    }

    #[test]
    fn test_extract_device_id() {
        assert_eq!(HostDeviceService::extract_device_id("tcp:custom001").unwrap(), "tcp:custom001");
        assert_eq!(HostDeviceService::extract_device_id("usb:device123").unwrap(), "usb:device123");

        assert!(HostDeviceService::extract_device_id("").is_err());
    }

    #[test]
    fn test_validate_device_id() {
        // Valid device IDs
        assert!(HostDeviceService::validate_device_id("tcp:custom001").is_ok());
        assert!(HostDeviceService::validate_device_id("usb:device123").is_ok());
        assert!(HostDeviceService::validate_device_id("transport:very:long:id").is_ok());

        // Invalid device IDs
        assert!(HostDeviceService::validate_device_id("").is_err());
        assert!(HostDeviceService::validate_device_id("no_colon").is_err());
    }

    #[test]
    fn test_is_device_available() {
        assert!(HostDeviceService::is_device_available(&DeviceState::Connected));
        assert!(HostDeviceService::is_device_available(&DeviceState::Discovered));

        assert!(!HostDeviceService::is_device_available(&DeviceState::Connecting));
        assert!(!HostDeviceService::is_device_available(&DeviceState::Disconnecting));
        assert!(!HostDeviceService::is_device_available(&DeviceState::Offline));
        assert!(!HostDeviceService::is_device_available(&DeviceState::Error {
            message: "test error".to_string()
        }));
    }

    #[tokio::test]
    async fn test_select_connected_device() {
        let registry = Arc::new(DeviceRegistry::new());
        let device = create_test_device("tcp:test001", DeviceState::Connected);
        registry.register_device(device).unwrap();

        let service = HostDeviceService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "tcp:test001").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(_) => {
                // Success - device was available
            }
            _ => panic!("Expected Okay response for connected device"),
        }
    }

    #[tokio::test]
    async fn test_select_discovered_device() {
        let registry = Arc::new(DeviceRegistry::new());
        let device = create_test_device("usb:dev123", DeviceState::Discovered);
        registry.register_device(device).unwrap();

        let service = HostDeviceService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "usb:dev123").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(_) => {
                // Success - discovered devices can be selected (lazy connection)
            }
            _ => panic!("Expected Okay response for discovered device"),
        }
    }

    #[tokio::test]
    async fn test_select_offline_device() {
        let registry = Arc::new(DeviceRegistry::new());
        let device = create_test_device("tcp:offline", DeviceState::Offline);
        registry.register_device(device).unwrap();

        let service = HostDeviceService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "tcp:offline").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Close(reason) => {
                assert_eq!(reason, "device is offline");
            }
            _ => panic!("Expected Close response for offline device"),
        }
    }

    #[tokio::test]
    async fn test_select_nonexistent_device() {
        let registry = Arc::new(DeviceRegistry::new());
        let service = HostDeviceService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "tcp:nonexistent").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Close(reason) => {
                assert_eq!(reason, "device not found");
            }
            _ => panic!("Expected Close response for nonexistent device"),
        }
    }

    #[tokio::test]
    async fn test_select_device_with_error_state() {
        let registry = Arc::new(DeviceRegistry::new());
        let device = create_test_device("tcp:error", DeviceState::Error {
            message: "connection failed".to_string()
        });
        registry.register_device(device).unwrap();

        let service = HostDeviceService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "tcp:error").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Close(reason) => {
                assert!(reason.contains("device error: connection failed"));
            }
            _ => panic!("Expected Close response for device in error state"),
        }
    }

    #[tokio::test]
    async fn test_invalid_device_id() {
        let registry = Arc::new(DeviceRegistry::new());
        let service = HostDeviceService::new(registry);
        let session = create_test_session();

        // Empty device ID
        let result = service.handle(&session, "").await;
        assert!(result.is_err());

        // Invalid format
        let result = service.handle(&session, "no_colon_here").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connecting_device() {
        let registry = Arc::new(DeviceRegistry::new());
        let device = create_test_device("tcp:connecting", DeviceState::Connecting);
        registry.register_device(device).unwrap();

        let service = HostDeviceService::new(registry);
        let session = create_test_session();

        let result = service.handle(&session, "tcp:connecting").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Close(reason) => {
                assert_eq!(reason, "device is connecting");
            }
            _ => panic!("Expected Close response for connecting device"),
        }
    }
}