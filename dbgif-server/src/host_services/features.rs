use async_trait::async_trait;
use crate::host_services::{HostService, HostServiceResponse};
use dbgif_protocol::error::ProtocolError;
use crate::server::session::ClientSessionInfo;

/// Host service for server feature listing
/// Implements the "host:features" service that returns server capabilities
#[derive(Debug)]
pub struct HostFeaturesService {
    features: Vec<String>,
}

impl HostFeaturesService {
    /// Create a new host features service with default features
    pub fn new() -> Self {
        Self {
            features: Self::default_features(),
        }
    }

    /// Create a new host features service with custom features
    pub fn with_features(features: Vec<String>) -> Self {
        Self {
            features,
        }
    }

    /// Get the default DBGIF server features
    fn default_features() -> Vec<String> {
        vec![
            "lazy-connection".to_string(),   // Devices connected on-demand
            "multi-client".to_string(),      // Multiple clients can use same device
            "ping-pong".to_string(),         // Connection health monitoring
            "host-services".to_string(),     // Built-in server services
        ]
    }

    /// Add a feature to the list
    pub fn add_feature(&mut self, feature: String) {
        if !self.features.contains(&feature) {
            self.features.push(feature);
        }
    }

    /// Remove a feature from the list
    pub fn remove_feature(&mut self, feature: &str) -> bool {
        if let Some(pos) = self.features.iter().position(|f| f == feature) {
            self.features.remove(pos);
            true
        } else {
            false
        }
    }

    /// Check if a feature is supported
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.contains(&feature.to_string())
    }

    /// Get all features
    pub fn get_features(&self) -> &[String] {
        &self.features
    }

    /// Generate the features response string
    /// Format: one feature per line with trailing newline
    fn generate_features_string(&self) -> String {
        if self.features.is_empty() {
            String::new()
        } else {
            format!("{}\n", self.features.join("\n"))
        }
    }
}

impl Default for HostFeaturesService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HostService for HostFeaturesService {
    fn service_name(&self) -> &str {
        "host:features"
    }

    async fn handle(&self, _session: &ClientSessionInfo, _args: &str) -> Result<HostServiceResponse, ProtocolError> {
        // host:features doesn't use args parameter
        let features_string = self.generate_features_string();
        Ok(HostServiceResponse::okay_str(&features_string))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn test_host_features_service_creation() {
        let service = HostFeaturesService::new();
        assert_eq!(service.service_name(), "host:features");

        // Should have default features
        let features = service.get_features();
        assert!(!features.is_empty());
        assert!(service.has_feature("lazy-connection"));
        assert!(service.has_feature("multi-client"));
        assert!(service.has_feature("ping-pong"));
        assert!(service.has_feature("host-services"));
    }

    #[test]
    fn test_default_features() {
        let features = HostFeaturesService::default_features();
        let expected = vec![
            "lazy-connection",
            "multi-client",
            "ping-pong",
            "host-services"
        ];

        assert_eq!(features.len(), expected.len());
        for expected_feature in expected {
            assert!(features.contains(&expected_feature.to_string()));
        }
    }

    #[test]
    fn test_custom_features() {
        let custom_features = vec![
            "feature1".to_string(),
            "feature2".to_string(),
            "feature3".to_string(),
        ];

        let service = HostFeaturesService::with_features(custom_features.clone());
        assert_eq!(service.get_features(), &custom_features);

        assert!(service.has_feature("feature1"));
        assert!(service.has_feature("feature2"));
        assert!(service.has_feature("feature3"));
        assert!(!service.has_feature("nonexistent"));
    }

    #[test]
    fn test_add_remove_features() {
        let mut service = HostFeaturesService::new();
        let initial_count = service.get_features().len();

        // Add new feature
        service.add_feature("new-feature".to_string());
        assert_eq!(service.get_features().len(), initial_count + 1);
        assert!(service.has_feature("new-feature"));

        // Adding same feature again should not duplicate
        service.add_feature("new-feature".to_string());
        assert_eq!(service.get_features().len(), initial_count + 1);

        // Remove feature
        assert!(service.remove_feature("new-feature"));
        assert_eq!(service.get_features().len(), initial_count);
        assert!(!service.has_feature("new-feature"));

        // Removing non-existent feature should return false
        assert!(!service.remove_feature("nonexistent"));
    }

    #[test]
    fn test_generate_features_string() {
        // Test with default features
        let service = HostFeaturesService::new();
        let features_string = service.generate_features_string();

        assert!(features_string.contains("lazy-connection"));
        assert!(features_string.contains("multi-client"));
        assert!(features_string.contains("ping-pong"));
        assert!(features_string.contains("host-services"));
        assert!(features_string.ends_with('\n'));

        // Each feature should be on its own line
        let lines: Vec<&str> = features_string.trim().split('\n').collect();
        assert_eq!(lines.len(), 4); // 4 default features
    }

    #[test]
    fn test_empty_features() {
        let service = HostFeaturesService::with_features(vec![]);
        let features_string = service.generate_features_string();
        assert!(features_string.is_empty());
    }

    #[tokio::test]
    async fn test_features_service_response() {
        let service = HostFeaturesService::new();
        let session = create_test_session();

        let result = service.handle(&session, "").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                let response = String::from_utf8(data).unwrap();
                assert!(!response.is_empty());

                // Should contain all default features
                assert!(response.contains("lazy-connection"));
                assert!(response.contains("multi-client"));
                assert!(response.contains("ping-pong"));
                assert!(response.contains("host-services"));
                assert!(response.ends_with('\n'));
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[tokio::test]
    async fn test_features_service_ignores_args() {
        let service = HostFeaturesService::new();
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

    #[tokio::test]
    async fn test_custom_features_response() {
        let custom_features = vec![
            "custom-feature1".to_string(),
            "custom-feature2".to_string(),
        ];
        let service = HostFeaturesService::with_features(custom_features);
        let session = create_test_session();

        let result = service.handle(&session, "").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                let response = String::from_utf8(data).unwrap();
                assert_eq!(response, "custom-feature1\ncustom-feature2\n");
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[test]
    fn test_default_implementation() {
        let service1 = HostFeaturesService::new();
        let service2 = HostFeaturesService::default();

        assert_eq!(service1.get_features(), service2.get_features());
    }

    #[tokio::test]
    async fn test_empty_features_response() {
        let service = HostFeaturesService::with_features(vec![]);
        let session = create_test_session();

        let result = service.handle(&session, "").await;
        assert!(result.is_ok());

        match result.unwrap() {
            HostServiceResponse::Okay(data) => {
                let response = String::from_utf8(data).unwrap();
                assert!(response.is_empty());
            }
            _ => panic!("Expected Okay response"),
        }
    }
}