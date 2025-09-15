use std::time::Duration;
use dbgif_server::test_client::{
    TestSession, TestResult, TestType, TestStatus, PerformanceMetrics
};

#[cfg(test)]
mod test_session_tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = TestSession::new();
        assert_eq!(session.get_status(), TestStatus::Pending);
        assert!(session.get_test_name().is_none());
        assert_eq!(session.get_duration(), Duration::ZERO);
        assert_eq!(session.get_events().len(), 0);
    }

    #[test]
    fn test_session_with_type() {
        let session = TestSession::new_with_type(TestType::Ping);
        assert_eq!(session.get_status(), TestStatus::Pending);
    }

    #[test]
    fn test_session_lifecycle() {
        let mut session = TestSession::new_with_type(TestType::Ping);

        // Start test
        session.start_test("test-ping");
        assert_eq!(session.get_status(), TestStatus::Running);
        assert_eq!(session.get_test_name(), Some("test-ping"));
        assert_eq!(session.get_events().len(), 1); // test_started event

        // Record some events
        session.record_event("connection_established");
        session.record_event("handshake_completed");
        assert_eq!(session.get_events().len(), 3);

        // End test successfully
        session.end_test();
        assert_eq!(session.get_status(), TestStatus::Success);
        assert_eq!(session.get_events().len(), 4); // test_completed event added

        // Verify duration
        assert!(session.get_duration() > Duration::ZERO);
    }

    #[test]
    fn test_session_failure() {
        let mut session = TestSession::new();
        session.start_test("test-fail");

        session.fail_test("Connection timeout");
        assert_eq!(session.get_status(), TestStatus::Failed);

        let result = session.get_result();
        assert!(!result.is_success());
        assert!(result.error().is_some());
    }

    #[test]
    fn test_session_timeout() {
        let mut session = TestSession::new();
        session.start_test("test-timeout");

        session.timeout_test();
        assert_eq!(session.get_status(), TestStatus::Timeout);

        let result = session.get_result();
        assert!(!result.is_success());
        assert_eq!(result.error(), Some("Test timed out"));
    }

    #[test]
    fn test_performance_metrics_tracking() {
        let mut session = TestSession::new();
        session.start_test("test-performance");

        // Test connection timing
        session.start_connection_timing();
        std::thread::sleep(Duration::from_millis(1)); // Small delay
        session.end_connection_timing();

        // Test handshake timing
        session.start_handshake_timing();
        std::thread::sleep(Duration::from_millis(1)); // Small delay
        session.end_handshake_timing();

        // Test counters
        session.increment_connection_count();
        session.increment_successful_connections();
        session.record_bytes_sent(100);
        session.record_bytes_received(200);

        session.end_test();
        let result = session.get_result();

        let metrics = result.performance_metrics().unwrap();
        assert!(metrics.connection_time > Duration::ZERO);
        assert!(metrics.handshake_time > Duration::ZERO);
        assert_eq!(metrics.connection_count, 1);
        assert_eq!(metrics.successful_connections, 1);
        assert_eq!(metrics.failed_connections, 0);
        assert_eq!(metrics.bytes_sent, 100);
        assert_eq!(metrics.bytes_received, 200);
    }

    #[test]
    fn test_multi_connection_metrics() {
        let mut session = TestSession::new();
        session.start_test("test-multi");

        // Simulate multiple connections
        for i in 0..3 {
            session.increment_connection_count();
            if i < 2 {
                session.increment_successful_connections();
            } else {
                session.increment_failed_connections();
            }
        }

        session.end_test();
        let result = session.get_result();

        let metrics = result.performance_metrics().unwrap();
        assert_eq!(metrics.connection_count, 3);
        assert_eq!(metrics.successful_connections, 2);
        assert_eq!(metrics.failed_connections, 1);
    }
}

#[cfg(test)]
mod test_result_tests {
    use super::*;

    #[test]
    fn test_result_success() {
        let result = TestResult::Success {
            duration: Duration::from_millis(100),
            events: vec!["test_started".to_string(), "test_completed".to_string()],
            performance_metrics: None,
        };

        assert!(result.is_success());
        assert!(!result.is_failure());
        assert_eq!(result.duration(), Duration::from_millis(100));
        assert_eq!(result.error(), None);
        assert_eq!(result.events().len(), 2);
        assert!(result.performance_metrics().is_none());

        let summary = result.summary();
        assert!(summary.contains("SUCCESS"));
        assert!(summary.contains("100ms"));
        assert!(summary.contains("2 events"));
    }

    #[test]
    fn test_result_success_with_metrics() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(5),
            handshake_time: Duration::from_millis(95),
            bytes_sent: 1024,
            bytes_received: 2048,
            connection_count: 1,
            successful_connections: 1,
            failed_connections: 0,
        };

        let result = TestResult::Success {
            duration: Duration::from_millis(100),
            events: vec!["test_started".to_string()],
            performance_metrics: Some(metrics),
        };

        assert!(result.is_success());
        assert!(result.performance_metrics().is_some());

        let summary = result.summary();
        assert!(summary.contains("Conn: 5ms"));
        assert!(summary.contains("Handshake: 95ms"));
    }

    #[test]
    fn test_result_failure() {
        let result = TestResult::Failure {
            error: "Connection failed".to_string(),
            duration: Duration::from_millis(50),
            performance_metrics: None,
        };

        assert!(!result.is_success());
        assert!(result.is_failure());
        assert_eq!(result.duration(), Duration::from_millis(50));
        assert_eq!(result.error(), Some("Connection failed"));
        assert_eq!(result.events().len(), 0);

        let summary = result.summary();
        assert!(summary.contains("FAILED"));
        assert!(summary.contains("50ms"));
        assert!(summary.contains("Connection failed"));
    }

    #[test]
    fn test_result_json_serialization() {
        let result = TestResult::Success {
            duration: Duration::from_millis(123),
            events: vec!["event1".to_string(), "event2".to_string()],
            performance_metrics: None,
        };

        let json = result.to_json().unwrap();
        assert!(json.contains("Success"));
        assert!(json.contains("123")); // Duration in milliseconds
        assert!(json.contains("event1"));
        assert!(json.contains("event2"));
    }

    #[test]
    fn test_result_with_multi_connection_metrics() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(10),
            handshake_time: Duration::from_millis(50),
            bytes_sent: 0,
            bytes_received: 0,
            connection_count: 5,
            successful_connections: 4,
            failed_connections: 1,
        };

        let result = TestResult::Success {
            duration: Duration::from_millis(200),
            events: vec!["multi_test".to_string()],
            performance_metrics: Some(metrics),
        };

        let summary = result.summary();
        assert!(summary.contains("Connections: 4/5"));
        assert!(summary.contains("Conn: 10ms"));
        assert!(summary.contains("Handshake: 50ms"));
    }
}

#[cfg(test)]
mod performance_metrics_tests {
    use super::*;

    #[test]
    fn test_performance_metrics_creation() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(10),
            handshake_time: Duration::from_millis(90),
            bytes_sent: 1000,
            bytes_received: 2000,
            connection_count: 3,
            successful_connections: 2,
            failed_connections: 1,
        };

        assert_eq!(metrics.connection_time, Duration::from_millis(10));
        assert_eq!(metrics.handshake_time, Duration::from_millis(90));
        assert_eq!(metrics.bytes_sent, 1000);
        assert_eq!(metrics.bytes_received, 2000);
        assert_eq!(metrics.connection_count, 3);
        assert_eq!(metrics.successful_connections, 2);
        assert_eq!(metrics.failed_connections, 1);
    }

    #[test]
    fn test_performance_metrics_json_serialization() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(5),
            handshake_time: Duration::from_millis(95),
            bytes_sent: 500,
            bytes_received: 1500,
            connection_count: 1,
            successful_connections: 1,
            failed_connections: 0,
        };

        let json = serde_json::to_string(&metrics).unwrap();
        assert!(json.contains("\"connection_time\":5"));
        assert!(json.contains("\"handshake_time\":95"));
        assert!(json.contains("\"bytes_sent\":500"));
        assert!(json.contains("\"bytes_received\":1500"));
        assert!(json.contains("\"connection_count\":1"));
        assert!(json.contains("\"successful_connections\":1"));
        assert!(json.contains("\"failed_connections\":0"));
    }

    #[test]
    fn test_performance_metrics_equality() {
        let metrics1 = PerformanceMetrics {
            connection_time: Duration::from_millis(10),
            handshake_time: Duration::from_millis(90),
            bytes_sent: 100,
            bytes_received: 200,
            connection_count: 1,
            successful_connections: 1,
            failed_connections: 0,
        };

        let metrics2 = PerformanceMetrics {
            connection_time: Duration::from_millis(10),
            handshake_time: Duration::from_millis(90),
            bytes_sent: 100,
            bytes_received: 200,
            connection_count: 1,
            successful_connections: 1,
            failed_connections: 0,
        };

        let metrics3 = PerformanceMetrics {
            connection_time: Duration::from_millis(20), // Different
            handshake_time: Duration::from_millis(90),
            bytes_sent: 100,
            bytes_received: 200,
            connection_count: 1,
            successful_connections: 1,
            failed_connections: 0,
        };

        assert_eq!(metrics1, metrics2);
        assert_ne!(metrics1, metrics3);
    }
}