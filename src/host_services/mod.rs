pub mod list;
pub mod device;
pub mod version;
pub mod features;

use async_trait::async_trait;
use crate::protocol::error::ProtocolError;
use crate::server::session::ClientSessionInfo;
use std::collections::HashMap;

/// Host service abstraction for built-in server services
/// Provides functionality like device listing, version info, etc.
#[async_trait]
pub trait HostService: Send + Sync + std::fmt::Debug {
    /// Get the service name (e.g., "host:list", "host:version")
    fn service_name(&self) -> &str;

    /// Handle a service request from a client session
    /// Returns response data or error
    async fn handle(&self, session: &ClientSessionInfo, args: &str) -> Result<HostServiceResponse, ProtocolError>;
}

/// Response from a host service
#[derive(Debug, Clone)]
pub enum HostServiceResponse {
    /// Success with optional data payload
    Okay(Vec<u8>),
    /// Failure with error message (triggers CLSE)
    Close(String),
}

impl HostServiceResponse {
    /// Create a success response with string data
    pub fn okay_str(data: &str) -> Self {
        Self::Okay(data.as_bytes().to_vec())
    }

    /// Create a success response with no data
    pub fn okay_empty() -> Self {
        Self::Okay(Vec::new())
    }

    /// Create a failure response
    pub fn close(message: &str) -> Self {
        Self::Close(message.to_string())
    }
}

/// Registry for managing host services
#[derive(Debug)]
pub struct HostServiceRegistry {
    services: HashMap<String, Box<dyn HostService>>,
}

impl HostServiceRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    /// Register a new host service
    pub fn register<S: HostService + 'static>(&mut self, service: S) {
        let service_name = service.service_name().to_string();
        self.services.insert(service_name, Box::new(service));
    }

    /// Get a host service by name
    pub fn get_service(&self, service_name: &str) -> Option<&dyn HostService> {
        self.services.get(service_name).map(|s| s.as_ref())
    }

    /// Handle a host service request
    pub async fn handle_service(&self, service_name: &str, session: &ClientSessionInfo, args: &str) -> Result<HostServiceResponse, ProtocolError> {
        if let Some(service) = self.get_service(service_name) {
            service.handle(session, args).await
        } else {
            Err(ProtocolError::HostServiceError {
                service: service_name.to_string(),
                message: "Service not found".to_string(),
            })
        }
    }

    /// Check if a service name is a host service
    pub fn is_host_service(service_name: &str) -> bool {
        service_name.starts_with("host:")
    }

    /// List all registered service names
    pub fn list_services(&self) -> Vec<&str> {
        self.services.keys().map(|k| k.as_str()).collect()
    }

    /// Get the number of registered services
    pub fn service_count(&self) -> usize {
        self.services.len()
    }

    /// Remove a service by name
    pub fn remove_service(&mut self, service_name: &str) -> bool {
        self.services.remove(service_name).is_some()
    }

    /// Clear all services
    pub fn clear(&mut self) {
        self.services.clear();
    }
}

impl Default for HostServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::session::ClientSessionInfo;

    // Mock host service for testing
    #[derive(Debug)]
    struct MockHostService {
        name: String,
        response: HostServiceResponse,
    }

    impl MockHostService {
        fn new(name: &str, response: HostServiceResponse) -> Self {
            Self {
                name: name.to_string(),
                response,
            }
        }
    }

    #[async_trait]
    impl HostService for MockHostService {
        fn service_name(&self) -> &str {
            &self.name
        }

        async fn handle(&self, _session: &ClientSessionInfo, _args: &str) -> Result<HostServiceResponse, ProtocolError> {
            Ok(self.response.clone())
        }
    }

    fn create_test_session() -> ClientSessionInfo {
        use crate::server::session::{ClientInfo, SessionState, SessionStats, ClientCapabilities};
        use std::collections::HashMap;
        use std::net::SocketAddr;

        ClientSessionInfo {
            session_id: "test-session".to_string(),
            client_info: ClientInfo {
                address: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
                identity: Some("test-client".to_string()),
                protocol_version: 1,
                max_data_size: 1024 * 1024,
                capabilities: ClientCapabilities::default(),
            },
            state: SessionState::Active,
            streams: HashMap::new(),
            stats: SessionStats::default(),
            established_at: std::time::Instant::now(),
            last_activity: std::time::Instant::now(),
        }
    }

    #[test]
    fn test_host_service_registry_creation() {
        let registry = HostServiceRegistry::new();
        assert_eq!(registry.service_count(), 0);
        assert!(registry.list_services().is_empty());
    }

    #[test]
    fn test_service_registration() {
        let mut registry = HostServiceRegistry::new();
        let service = MockHostService::new("host:test", HostServiceResponse::okay_empty());

        registry.register(service);
        assert_eq!(registry.service_count(), 1);
        assert!(registry.get_service("host:test").is_some());
        assert!(registry.list_services().contains(&"host:test"));
    }

    #[tokio::test]
    async fn test_service_handling() {
        let mut registry = HostServiceRegistry::new();
        let service = MockHostService::new("host:test", HostServiceResponse::okay_str("test response"));

        registry.register(service);

        let session = create_test_session();
        let result = registry.handle_service("host:test", &session, "").await;

        assert!(result.is_ok());
        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                assert_eq!(String::from_utf8(data).unwrap(), "test response");
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[tokio::test]
    async fn test_service_not_found() {
        let registry = HostServiceRegistry::new();
        let session = create_test_session();

        let result = registry.handle_service("host:nonexistent", &session, "").await;
        assert!(result.is_err());

        match result.unwrap_err() {
            ProtocolError::HostServiceError { service, message } => {
                assert_eq!(service, "host:nonexistent");
                assert_eq!(message, "Service not found");
            }
            _ => panic!("Expected HostServiceError"),
        }
    }

    #[test]
    fn test_is_host_service() {
        assert!(HostServiceRegistry::is_host_service("host:list"));
        assert!(HostServiceRegistry::is_host_service("host:version"));
        assert!(HostServiceRegistry::is_host_service("host:device:123"));
        assert!(!HostServiceRegistry::is_host_service("shell"));
        assert!(!HostServiceRegistry::is_host_service("tcp:5555"));
        assert!(!HostServiceRegistry::is_host_service(""));
    }

    #[test]
    fn test_service_removal() {
        let mut registry = HostServiceRegistry::new();
        let service = MockHostService::new("host:test", HostServiceResponse::okay_empty());

        registry.register(service);
        assert_eq!(registry.service_count(), 1);

        assert!(registry.remove_service("host:test"));
        assert_eq!(registry.service_count(), 0);
        assert!(!registry.remove_service("host:nonexistent"));
    }

    #[test]
    fn test_host_service_response_helpers() {
        let ok_str = HostServiceResponse::okay_str("hello");
        match ok_str {
            HostServiceResponse::Okay(data) => {
                assert_eq!(String::from_utf8(data).unwrap(), "hello");
            }
            _ => panic!("Expected Okay response"),
        }

        let ok_empty = HostServiceResponse::okay_empty();
        match ok_empty {
            HostServiceResponse::Okay(data) => {
                assert!(data.is_empty());
            }
            _ => panic!("Expected Okay response"),
        }

        let close = HostServiceResponse::close("error message");
        match close {
            HostServiceResponse::Close(msg) => {
                assert_eq!(msg, "error message");
            }
            _ => panic!("Expected Close response"),
        }
    }
}