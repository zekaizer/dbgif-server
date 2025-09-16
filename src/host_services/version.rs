use async_trait::async_trait;
use crate::host_services::{HostService, HostServiceResponse};
use crate::protocol::error::ProtocolError;
use crate::server::session::ClientSessionInfo;

/// Host service for server version information
/// Implements the "host:version" service that returns server version
#[derive(Debug)]
pub struct HostVersionService {
    version_string: String,
}

impl HostVersionService {
    /// Create a new host version service with default version
    pub fn new() -> Self {
        // Get version from Cargo.toml at compile time
        let version = env!("CARGO_PKG_VERSION");
        let name = env!("CARGO_PKG_NAME");

        Self {
            version_string: format!("{} {}", name, version),
        }
    }

    /// Create a new host version service with custom version string
    pub fn with_version(version_string: String) -> Self {
        Self {
            version_string,
        }
    }

    /// Get the version string
    pub fn version_string(&self) -> &str {
        &self.version_string
    }
}

impl Default for HostVersionService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HostService for HostVersionService {
    fn service_name(&self) -> &str {
        "host:version"
    }

    async fn handle(&self, _session: &ClientSessionInfo, _args: &str) -> Result<HostServiceResponse, ProtocolError> {
        // host:version doesn't use args parameter
        Ok(HostServiceResponse::okay_str(&self.version_string))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

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
            established_at: Instant::now(),
            last_activity: Instant::now(),
        }
    }

    #[test]
    fn test_host_version_service_creation() {
        let service = HostVersionService::new();
        assert_eq!(service.service_name(), "host:version");

        // Version should contain package name and version
        let version = service.version_string();
        assert!(version.contains("dbgif-protocol")); // Package name from Cargo.toml
        assert!(!version.is_empty());
    }

    #[test]
    fn test_custom_version_string() {
        let custom_version = "dbgif-server 2.0.0-beta";
        let service = HostVersionService::with_version(custom_version.to_string());

        assert_eq!(service.version_string(), custom_version);
    }

    #[tokio::test]
    async fn test_version_service_response() {
        let service = HostVersionService::new();
        let session = create_test_session();

        let result = service.handle(&session, "").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                let response = String::from_utf8(data).unwrap();
                assert!(!response.is_empty());
                assert!(response.contains("dbgif-protocol")); // Should contain package name
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[tokio::test]
    async fn test_version_service_ignores_args() {
        let service = HostVersionService::new();
        let session = create_test_session();

        // Should work the same regardless of args
        let result1 = service.handle(&session, "").await.unwrap();
        let result2 = service.handle(&session, "some random args").await.unwrap();

        match (result1, result2) {
            (HostServiceResponse::Okay(data1), HostServiceResponse::Okay(data2)) => {
                assert_eq!(data1, data2); // Should be identical
            }
            _ => panic!("Expected Okay responses"),
        }
    }

    #[test]
    fn test_default_implementation() {
        let service1 = HostVersionService::new();
        let service2 = HostVersionService::default();

        assert_eq!(service1.version_string(), service2.version_string());
    }

    #[tokio::test]
    async fn test_custom_version_response() {
        let custom_version = "test-server 3.1.4";
        let service = HostVersionService::with_version(custom_version.to_string());
        let session = create_test_session();

        let result = service.handle(&session, "").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                let response = String::from_utf8(data).unwrap();
                assert_eq!(response, custom_version);
            }
            _ => panic!("Expected Okay response"),
        }
    }
}