use super::{TestResult, TestType, TestStatus, PerformanceMetrics};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct TestSession {
    test_name: Option<String>,
    start_time: Option<Instant>,
    end_time: Option<Instant>,
    events: Vec<(Instant, String)>,
    test_type: TestType,
    status: TestStatus,
    connection_start: Option<Instant>,
    connection_time: Duration,
    handshake_start: Option<Instant>,
    handshake_time: Duration,
    bytes_sent: u64,
    bytes_received: u64,
    connection_count: u32,
    successful_connections: u32,
    failed_connections: u32,
}

impl Default for TestSession {
    fn default() -> Self {
        Self::new()
    }
}

impl TestSession {
    pub fn new() -> Self {
        Self {
            test_name: None,
            start_time: None,
            end_time: None,
            events: Vec::new(),
            test_type: TestType::Connection,
            status: TestStatus::Pending,
            connection_start: None,
            connection_time: Duration::ZERO,
            handshake_start: None,
            handshake_time: Duration::ZERO,
            bytes_sent: 0,
            bytes_received: 0,
            connection_count: 0,
            successful_connections: 0,
            failed_connections: 0,
        }
    }

    pub fn new_with_type(test_type: TestType) -> Self {
        Self {
            test_name: None,
            start_time: None,
            end_time: None,
            events: Vec::new(),
            test_type,
            status: TestStatus::Pending,
            connection_start: None,
            connection_time: Duration::ZERO,
            handshake_start: None,
            handshake_time: Duration::ZERO,
            bytes_sent: 0,
            bytes_received: 0,
            connection_count: 0,
            successful_connections: 0,
            failed_connections: 0,
        }
    }

    pub fn start_test(&mut self, name: &str) {
        self.test_name = Some(name.to_string());
        self.start_time = Some(Instant::now());
        self.status = TestStatus::Running;
        self.events.clear();
        self.record_event("test_started");
    }

    pub fn record_event(&mut self, event: &str) {
        let timestamp = Instant::now();
        self.events.push((timestamp, event.to_string()));
    }

    pub fn end_test(&mut self) {
        self.end_time = Some(Instant::now());
        self.record_event("test_completed");
        if self.status == TestStatus::Running {
            self.status = TestStatus::Success;
        }
    }

    pub fn fail_test(&mut self, error: &str) {
        self.end_time = Some(Instant::now());
        self.status = TestStatus::Failed;
        self.record_event(&format!("test_failed: {}", error));
    }

    pub fn timeout_test(&mut self) {
        self.end_time = Some(Instant::now());
        self.status = TestStatus::Timeout;
        self.record_event("test_timeout");
    }

    pub fn get_result(&self) -> TestResult {
        let duration = match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => end.duration_since(start),
            (Some(start), None) => start.elapsed(),
            _ => Duration::from_secs(0),
        };

        let events = self.events.iter()
            .map(|(_, event)| event.clone())
            .collect();

        let performance_metrics = if self.connection_time > Duration::ZERO ||
                                   self.handshake_time > Duration::ZERO ||
                                   self.connection_count > 0 {
            Some(self.get_performance_metrics())
        } else {
            None
        };

        match self.status {
            TestStatus::Success => TestResult::Success {
                duration,
                events,
                performance_metrics
            },
            TestStatus::Failed | TestStatus::Timeout => {
                let error = if self.status == TestStatus::Timeout {
                    "Test timed out".to_string()
                } else {
                    self.events.iter()
                        .find(|(_, event)| event.starts_with("test_failed:"))
                        .map(|(_, event)| event.clone())
                        .unwrap_or_else(|| "Test failed".to_string())
                };
                TestResult::Failure {
                    error,
                    duration,
                    performance_metrics
                }
            }
            _ => TestResult::Failure {
                error: "Test never completed".to_string(),
                duration,
                performance_metrics,
            }
        }
    }

    pub fn get_status(&self) -> TestStatus {
        self.status
    }

    pub fn get_test_name(&self) -> Option<&str> {
        self.test_name.as_deref()
    }

    pub fn get_duration(&self) -> Duration {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => end.duration_since(start),
            (Some(start), None) => start.elapsed(),
            _ => Duration::from_secs(0),
        }
    }

    pub fn get_events(&self) -> &[(Instant, String)] {
        &self.events
    }

    // Performance tracking methods
    pub fn start_connection_timing(&mut self) {
        self.connection_start = Some(Instant::now());
    }

    pub fn end_connection_timing(&mut self) {
        if let Some(start) = self.connection_start {
            self.connection_time = start.elapsed();
        }
    }

    pub fn start_handshake_timing(&mut self) {
        self.handshake_start = Some(Instant::now());
    }

    pub fn end_handshake_timing(&mut self) {
        if let Some(start) = self.handshake_start {
            self.handshake_time = start.elapsed();
        }
    }

    pub fn record_bytes_sent(&mut self, bytes: u64) {
        self.bytes_sent += bytes;
    }

    pub fn record_bytes_received(&mut self, bytes: u64) {
        self.bytes_received += bytes;
    }

    pub fn increment_connection_count(&mut self) {
        self.connection_count += 1;
    }

    pub fn increment_successful_connections(&mut self) {
        self.successful_connections += 1;
    }

    pub fn increment_failed_connections(&mut self) {
        self.failed_connections += 1;
    }

    pub fn get_performance_metrics(&self) -> PerformanceMetrics {
        PerformanceMetrics {
            connection_time: self.connection_time,
            handshake_time: self.handshake_time,
            bytes_sent: self.bytes_sent,
            bytes_received: self.bytes_received,
            connection_count: self.connection_count,
            successful_connections: self.successful_connections,
            failed_connections: self.failed_connections,
        }
    }
}