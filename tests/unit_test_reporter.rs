use std::time::Duration;
use dbgif_server::test_client::{TestResult, PerformanceMetrics};

#[cfg(test)]
mod result_formatting_tests {
    use super::*;

    #[test]
    fn test_success_result_summary_basic() {
        let result = TestResult::Success {
            duration: Duration::from_millis(150),
            events: vec![
                "test_started".to_string(),
                "connection_established".to_string(),
                "handshake_completed".to_string(),
                "test_completed".to_string(),
            ],
            performance_metrics: None,
        };

        let summary = result.summary();
        assert!(summary.contains("SUCCESS"));
        assert!(summary.contains("150ms"));
        assert!(summary.contains("4 events"));
        // Should not contain performance info when metrics are None
        assert!(!summary.contains("Conn:"));
        assert!(!summary.contains("Handshake:"));
    }

    #[test]
    fn test_success_result_summary_with_metrics() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(10),
            handshake_time: Duration::from_millis(85),
            bytes_sent: 1024,
            bytes_received: 2048,
            connection_count: 1,
            successful_connections: 1,
            failed_connections: 0,
        };

        let result = TestResult::Success {
            duration: Duration::from_millis(95),
            events: vec!["event1".to_string(), "event2".to_string()],
            performance_metrics: Some(metrics),
        };

        let summary = result.summary();
        assert!(summary.contains("SUCCESS in 95ms (2 events)"));
        assert!(summary.contains("Conn: 10ms"));
        assert!(summary.contains("Handshake: 85ms"));
        // Should not show connections count for single connection
        assert!(!summary.contains("Connections:"));
    }

    #[test]
    fn test_success_result_summary_with_multi_connections() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(20),
            handshake_time: Duration::from_millis(100),
            bytes_sent: 0,
            bytes_received: 0,
            connection_count: 5,
            successful_connections: 4,
            failed_connections: 1,
        };

        let result = TestResult::Success {
            duration: Duration::from_millis(500),
            events: vec!["multi_test".to_string()],
            performance_metrics: Some(metrics),
        };

        let summary = result.summary();
        assert!(summary.contains("SUCCESS in 500ms (1 events)"));
        assert!(summary.contains("Conn: 20ms"));
        assert!(summary.contains("Handshake: 100ms"));
        assert!(summary.contains("Connections: 4/5"));
    }

    #[test]
    fn test_failure_result_summary_basic() {
        let result = TestResult::Failure {
            error: "Connection timeout".to_string(),
            duration: Duration::from_millis(5000),
            performance_metrics: None,
        };

        let summary = result.summary();
        assert!(summary.contains("FAILED"));
        assert!(summary.contains("5000ms"));
        assert!(summary.contains("Connection timeout"));
        assert!(!summary.contains("Conn:"));
        assert!(!summary.contains("Handshake:"));
    }

    #[test]
    fn test_failure_result_summary_with_metrics() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(15),
            handshake_time: Duration::from_millis(0), // Handshake never completed
            bytes_sent: 0,
            bytes_received: 0,
            connection_count: 1,
            successful_connections: 0,
            failed_connections: 1,
        };

        let result = TestResult::Failure {
            error: "Handshake failed: protocol error".to_string(),
            duration: Duration::from_millis(2000),
            performance_metrics: Some(metrics),
        };

        let summary = result.summary();
        assert!(summary.contains("FAILED in 2000ms"));
        assert!(summary.contains("Handshake failed: protocol error"));
        assert!(summary.contains("Conn: 15ms"));
        assert!(summary.contains("Handshake: 0ms"));
    }

    #[test]
    fn test_json_output_success_basic() {
        let result = TestResult::Success {
            duration: Duration::from_millis(123),
            events: vec!["start".to_string(), "end".to_string()],
            performance_metrics: None,
        };

        let json = result.to_json().unwrap();

        // Verify JSON structure
        assert!(json.contains("\"Success\""));
        assert!(json.contains("\"duration\": 123"));
        assert!(json.contains("\"events\""));
        assert!(json.contains("\"start\""));
        assert!(json.contains("\"end\""));
        assert!(json.contains("\"performance_metrics\": null"));

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["Success"].is_object());
    }

    #[test]
    fn test_json_output_success_with_metrics() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(5),
            handshake_time: Duration::from_millis(95),
            bytes_sent: 1000,
            bytes_received: 2000,
            connection_count: 2,
            successful_connections: 2,
            failed_connections: 0,
        };

        let result = TestResult::Success {
            duration: Duration::from_millis(100),
            events: vec!["test".to_string()],
            performance_metrics: Some(metrics),
        };

        let json = result.to_json().unwrap();

        // Verify performance metrics are included
        assert!(json.contains("\"performance_metrics\""));
        assert!(json.contains("\"connection_time\": 5"));
        assert!(json.contains("\"handshake_time\": 95"));
        assert!(json.contains("\"bytes_sent\": 1000"));
        assert!(json.contains("\"bytes_received\": 2000"));
        assert!(json.contains("\"connection_count\": 2"));
        assert!(json.contains("\"successful_connections\": 2"));
        assert!(json.contains("\"failed_connections\": 0"));

        // Verify it parses correctly
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let perf = &parsed["Success"]["performance_metrics"];
        assert_eq!(perf["connection_time"], 5);
        assert_eq!(perf["handshake_time"], 95);
        assert_eq!(perf["connection_count"], 2);
    }

    #[test]
    fn test_json_output_failure() {
        let result = TestResult::Failure {
            error: "Network unreachable".to_string(),
            duration: Duration::from_millis(1000),
            performance_metrics: None,
        };

        let json = result.to_json().unwrap();

        assert!(json.contains("\"Failure\""));
        assert!(json.contains("\"error\": \"Network unreachable\""));
        assert!(json.contains("\"duration\": 1000"));
        assert!(json.contains("\"performance_metrics\": null"));

        // Verify it parses correctly
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["Failure"]["error"], "Network unreachable");
        assert_eq!(parsed["Failure"]["duration"], 1000);
    }

    #[test]
    fn test_duration_formatting_precision() {
        // Test various duration values to ensure consistent formatting
        let test_cases = vec![
            (Duration::from_millis(0), "0ms"),
            (Duration::from_millis(1), "1ms"),
            (Duration::from_millis(10), "10ms"),
            (Duration::from_millis(100), "100ms"),
            (Duration::from_millis(1000), "1000ms"),
            (Duration::from_millis(1500), "1500ms"),
        ];

        for (duration, expected_ms) in test_cases {
            let result = TestResult::Success {
                duration,
                events: vec![],
                performance_metrics: None,
            };

            let summary = result.summary();
            assert!(summary.contains(&format!("SUCCESS in {}", expected_ms)));
        }
    }

    #[test]
    fn test_performance_metrics_zero_values() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::ZERO,
            handshake_time: Duration::ZERO,
            bytes_sent: 0,
            bytes_received: 0,
            connection_count: 0,
            successful_connections: 0,
            failed_connections: 0,
        };

        let result = TestResult::Success {
            duration: Duration::from_millis(100),
            events: vec!["test".to_string()],
            performance_metrics: Some(metrics),
        };

        let summary = result.summary();
        assert!(summary.contains("Conn: 0ms"));
        assert!(summary.contains("Handshake: 0ms"));
        // Should not show connections when count is 0
        assert!(!summary.contains("Connections:"));
    }

    #[test]
    fn test_event_count_formatting() {
        let test_cases = vec![
            (vec![], "0 events"),
            (vec!["event1".to_string()], "1 events"),
            (vec!["event1".to_string(), "event2".to_string()], "2 events"),
        ];

        for (events, expected_text) in test_cases {
            let result = TestResult::Success {
                duration: Duration::from_millis(100),
                events,
                performance_metrics: None,
            };

            let summary = result.summary();
            assert!(summary.contains(expected_text));
        }
    }

    #[test]
    fn test_error_message_formatting() {
        let error_messages = vec![
            "Simple error",
            "Error with: special characters!",
            "Multi-line\nerror\nmessage",
            "Error with \"quotes\" and 'apostrophes'",
        ];

        for error_msg in error_messages {
            let result = TestResult::Failure {
                error: error_msg.to_string(),
                duration: Duration::from_millis(100),
                performance_metrics: None,
            };

            let summary = result.summary();
            assert!(summary.contains("FAILED"));
            assert!(summary.contains(error_msg));

            // Verify JSON also handles the error message correctly
            let json = result.to_json().unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed["Failure"]["error"], error_msg);
        }
    }

    #[test]
    fn test_large_duration_formatting() {
        // Test very large durations
        let result = TestResult::Success {
            duration: Duration::from_millis(999999),
            events: vec!["long_test".to_string()],
            performance_metrics: None,
        };

        let summary = result.summary();
        assert!(summary.contains("SUCCESS in 999999ms"));

        let json = result.to_json().unwrap();
        assert!(json.contains("\"duration\": 999999"));
    }

    #[test]
    fn test_json_pretty_printing() {
        let result = TestResult::Success {
            duration: Duration::from_millis(100),
            events: vec!["test".to_string()],
            performance_metrics: None,
        };

        let json = result.to_json().unwrap();

        // Pretty printed JSON should have newlines and indentation
        assert!(json.contains("{\n"));
        assert!(json.contains("  "));

        // Should be readable
        let lines: Vec<&str> = json.lines().collect();
        assert!(lines.len() > 1);
    }
}

#[cfg(test)]
mod cli_output_tests {
    use super::*;

    #[test]
    fn test_result_accessor_methods() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(10),
            handshake_time: Duration::from_millis(90),
            bytes_sent: 500,
            bytes_received: 1500,
            connection_count: 1,
            successful_connections: 1,
            failed_connections: 0,
        };

        // Test success result accessors
        let success_result = TestResult::Success {
            duration: Duration::from_millis(100),
            events: vec!["event1".to_string(), "event2".to_string()],
            performance_metrics: Some(metrics.clone()),
        };

        assert!(success_result.is_success());
        assert!(!success_result.is_failure());
        assert_eq!(success_result.duration(), Duration::from_millis(100));
        assert_eq!(success_result.error(), None);
        assert_eq!(success_result.events().len(), 2);
        assert_eq!(success_result.events()[0], "event1");
        assert!(success_result.performance_metrics().is_some());

        // Test failure result accessors
        let failure_result = TestResult::Failure {
            error: "Test error".to_string(),
            duration: Duration::from_millis(50),
            performance_metrics: Some(metrics),
        };

        assert!(!failure_result.is_success());
        assert!(failure_result.is_failure());
        assert_eq!(failure_result.duration(), Duration::from_millis(50));
        assert_eq!(failure_result.error(), Some("Test error"));
        assert_eq!(failure_result.events().len(), 0);
        assert!(failure_result.performance_metrics().is_some());
    }

    #[test]
    fn test_performance_metrics_accessor() {
        let metrics = PerformanceMetrics {
            connection_time: Duration::from_millis(15),
            handshake_time: Duration::from_millis(85),
            bytes_sent: 1024,
            bytes_received: 2048,
            connection_count: 3,
            successful_connections: 2,
            failed_connections: 1,
        };

        let result = TestResult::Success {
            duration: Duration::from_millis(100),
            events: vec![],
            performance_metrics: Some(metrics),
        };

        let perf = result.performance_metrics().unwrap();
        assert_eq!(perf.connection_time, Duration::from_millis(15));
        assert_eq!(perf.handshake_time, Duration::from_millis(85));
        assert_eq!(perf.bytes_sent, 1024);
        assert_eq!(perf.bytes_received, 2048);
        assert_eq!(perf.connection_count, 3);
        assert_eq!(perf.successful_connections, 2);
        assert_eq!(perf.failed_connections, 1);
    }

    #[test]
    fn test_result_without_metrics() {
        let result = TestResult::Success {
            duration: Duration::from_millis(100),
            events: vec!["test".to_string()],
            performance_metrics: None,
        };

        assert!(result.performance_metrics().is_none());

        let summary = result.summary();
        // Should not contain performance metrics info
        assert!(!summary.contains("Conn:"));
        assert!(!summary.contains("Handshake:"));
        assert!(!summary.contains("Connections:"));
    }
}