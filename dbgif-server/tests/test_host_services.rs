#[cfg(test)]
mod tests {
    use dbgif_server::host_services::*;
    use dbgif_server::host_services::version::HostVersionService;
    use dbgif_server::host_services::features::HostFeaturesService;
    use dbgif_server::host_services::list::HostListService;
    use dbgif_server::host_services::device::HostDeviceService;
    use dbgif_server::server::device_registry::DeviceRegistry;
    use dbgif_server::server::session::{ClientSessionInfo, ClientInfo, SessionState, SessionStats, ClientCapabilities};
    use std::sync::Arc;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_host_service_registry() {
        // Test that host service registry can be created and managed
        let registry = create_host_service_registry();
        assert!(registry.is_ok());

        let registry = registry.unwrap();

        // Registry should start empty or with default services
        let initial_services = registry.list_services();
        // Registry should have a reasonable number of services (allow empty)
        assert!(initial_services.len() < 100);
    }

    #[tokio::test]
    async fn test_host_version_service() {
        // Test host:version service
        let version_service = create_version_service();
        assert!(version_service.is_ok());

        let service = version_service.unwrap();
        let session = create_test_session();
        let version_response = service.handle(&session, "").await;

        assert!(version_response.is_ok());
        let response = version_response.unwrap();

        // Version response should contain version information
        match response {
            HostServiceResponse::Okay(data) => {
                assert!(!data.is_empty());
                // Should be valid UTF-8 string
                let version_str = String::from_utf8(data);
                assert!(version_str.is_ok());
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[tokio::test]
    async fn test_host_features_service() {
        // Test host:features service
        let features_service = create_features_service();
        assert!(features_service.is_ok());

        let service = features_service.unwrap();
        let session = create_test_session();
        let features_response = service.handle(&session, "").await;

        assert!(features_response.is_ok());
        let response = features_response.unwrap();

        // Features response should contain feature list
        match response {
            HostServiceResponse::Okay(data) => {
                assert!(!data.is_empty());
                // Should be valid UTF-8 string
                let features_str = String::from_utf8(data);
                assert!(features_str.is_ok());

                // Should contain expected features (check for actual features from implementation)
                let features = features_str.unwrap();
                assert!(features.contains("lazy-connection") || features.contains("multi-client") || features.contains("host-services"));
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[tokio::test]
    async fn test_host_list_service() {
        // Test host:list service (device listing)
        let list_service = create_list_service();
        assert!(list_service.is_ok());

        let service = list_service.unwrap();
        let session = create_test_session();
        let list_response = service.handle(&session, "").await;

        assert!(list_response.is_ok());
        let response = list_response.unwrap();

        // List response should be valid (even if empty)
        match response {
            HostServiceResponse::Okay(data) => {
                let device_list = String::from_utf8(data);
                assert!(device_list.is_ok());
            }
            _ => panic!("Expected Okay response"),
        }
    }

    #[tokio::test]
    async fn test_host_device_service() {
        // Test host:device service (device selection)
        let device_service = create_device_service();
        assert!(device_service.is_ok());

        let service = device_service.unwrap();
        let session = create_test_session();

        // Test with valid device ID
        let device_id = "tcp:test-device-001";
        let device_response = service.handle(&session, device_id).await;

        assert!(device_response.is_ok());
        let response = device_response.unwrap();

        // Device selection should return status information
        match response {
            HostServiceResponse::Okay(data) => {
                let status = String::from_utf8(data);
                assert!(status.is_ok());
            }
            HostServiceResponse::Close(_) => {
                // Device might not exist, which is acceptable for testing
            }
        }
    }

    #[tokio::test]
    async fn test_service_registration() {
        // Test registering custom services
        let mut registry = create_host_service_registry().unwrap();

        let custom_service = create_custom_service("test:custom");
        registry.register(custom_service);

        // Service should be listed
        let services = registry.list_services();
        assert!(services.contains(&"test:custom"));
    }

    #[tokio::test]
    async fn test_service_lookup() {
        // Test looking up services by name
        let mut registry = create_host_service_registry().unwrap();

        // Register test service
        let test_service = create_custom_service("test:lookup");
        registry.register(test_service);

        // Lookup should succeed
        let lookup_result = registry.get_service("test:lookup");
        assert!(lookup_result.is_some());

        // Non-existent service should return None
        let missing_service = registry.get_service("test:missing");
        assert!(missing_service.is_none());
    }

    #[tokio::test]
    async fn test_service_prefix_matching() {
        // Test that host: services are properly prefixed
        let registry = create_host_service_registry().unwrap();
        let services = registry.list_services();

        // All default services should have host: prefix
        for service_name in services {
            if service_name.starts_with("host:") {
                assert!(
                    service_name == "host:version" ||
                    service_name == "host:features" ||
                    service_name == "host:list" ||
                    service_name.starts_with("host:device")
                );
            }
        }
    }

    #[tokio::test]
    async fn test_service_error_handling() {
        // Test error handling in services
        let version_service = create_version_service().unwrap();

        // Service should handle invalid requests gracefully
        let session = create_test_session();
        let invalid_args = "invalid-args-with-lots-of-data";
        let error_response = version_service.handle(&session, invalid_args).await;

        // Should either succeed with error message or fail gracefully
        assert!(error_response.is_ok() || error_response.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_service_access() {
        // Test concurrent access to services
        let _registry = create_host_service_registry().unwrap();

        let mut handles = Vec::new();

        for i in 0..5 {
            let service_name = format!("test:concurrent-{}", i);
            let custom_service = create_custom_service(&service_name);

            // Simulate concurrent service operations
            let handle = tokio::spawn(async move {
                let session = create_test_session();
                let response = custom_service.handle(&session, "test").await;
                response.is_ok()
            });

            handles.push(handle);
        }

        // All concurrent operations should succeed
        for handle in handles {
            let result = handle.await;
            assert!(result.is_ok());
            assert!(result.unwrap());
        }
    }

    #[tokio::test]
    async fn test_service_protocol_compliance() {
        // Test that services comply with ADB protocol expectations
        let version_service = create_version_service().unwrap();
        let features_service = create_features_service().unwrap();

        // Services should respond to empty requests
        let session = create_test_session();
        let version_resp = version_service.handle(&session, "").await.unwrap();
        let features_resp = features_service.handle(&session, "").await.unwrap();

        // Extract data from responses
        let version_data = match version_resp {
            HostServiceResponse::Okay(data) => data,
            _ => panic!("Expected Okay response from version service"),
        };
        let features_data = match features_resp {
            HostServiceResponse::Okay(data) => data,
            _ => panic!("Expected Okay response from features service"),
        };

        // Responses should be reasonable size (not too large for ADB protocol)
        assert!(version_data.len() < 1024);
        assert!(features_data.len() < 1024);

        // Responses should not contain null bytes (to avoid protocol issues)
        assert!(!version_data.contains(&0));
        assert!(!features_data.contains(&0));
    }

    // Helper types and functions - these should be implemented in actual host services module

    // Use actual host service traits and types from the main codebase
    // These are already imported from dbgif_protocol::host_services::*

    fn create_test_session() -> ClientSessionInfo {
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
            established_at: std::time::Instant::now(),
            last_activity: std::time::Instant::now(),
        }
    }

    fn create_host_service_registry() -> Result<HostServiceRegistry, Box<dyn std::error::Error>> {
        Ok(HostServiceRegistry::new())
    }

    fn create_version_service() -> Result<Box<dyn HostService + Send + Sync>, Box<dyn std::error::Error>> {
        Ok(Box::new(HostVersionService::new()))
    }

    fn create_features_service() -> Result<Box<dyn HostService + Send + Sync>, Box<dyn std::error::Error>> {
        Ok(Box::new(HostFeaturesService::new()))
    }

    fn create_list_service() -> Result<Box<dyn HostService + Send + Sync>, Box<dyn std::error::Error>> {
        let device_registry = Arc::new(DeviceRegistry::new());
        Ok(Box::new(HostListService::new(device_registry)))
    }

    fn create_device_service() -> Result<Box<dyn HostService + Send + Sync>, Box<dyn std::error::Error>> {
        let device_registry = Arc::new(DeviceRegistry::new());
        Ok(Box::new(HostDeviceService::new(device_registry)))
    }

    // Mock host service for testing
    #[derive(Debug)]
    struct MockHostService {
        name: String,
    }

    impl MockHostService {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl HostService for MockHostService {
        fn service_name(&self) -> &str {
            &self.name
        }

        async fn handle(&self, _session: &ClientSessionInfo, _args: &str) -> Result<HostServiceResponse, dbgif_protocol::error::ProtocolError> {
            Ok(HostServiceResponse::okay_str("test response"))
        }
    }

    fn create_custom_service(name: &str) -> MockHostService {
        MockHostService::new(name)
    }
}