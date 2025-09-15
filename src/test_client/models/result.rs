use std::time::Duration;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TestResult {
    Success {
        #[serde(with = "duration_as_millis")]
        duration: Duration,
        events: Vec<String>,
        performance_metrics: Option<PerformanceMetrics>,
    },
    Failure {
        error: String,
        #[serde(with = "duration_as_millis")]
        duration: Duration,
        performance_metrics: Option<PerformanceMetrics>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerformanceMetrics {
    #[serde(with = "duration_as_millis")]
    pub connection_time: Duration,
    #[serde(with = "duration_as_millis")]
    pub handshake_time: Duration,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub connection_count: u32,
    pub successful_connections: u32,
    pub failed_connections: u32,
}

mod duration_as_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_millis().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u128::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis as u64))
    }
}

impl TestResult {
    pub fn is_success(&self) -> bool {
        matches!(self, TestResult::Success { .. })
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, TestResult::Failure { .. })
    }

    pub fn duration(&self) -> Duration {
        match self {
            TestResult::Success { duration, .. } => *duration,
            TestResult::Failure { duration, .. } => *duration,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            TestResult::Success { .. } => None,
            TestResult::Failure { error, .. } => Some(error),
        }
    }

    pub fn events(&self) -> &[String] {
        match self {
            TestResult::Success { events, .. } => events,
            TestResult::Failure { .. } => &[],
        }
    }

    pub fn performance_metrics(&self) -> Option<&PerformanceMetrics> {
        match self {
            TestResult::Success { performance_metrics, .. } => performance_metrics.as_ref(),
            TestResult::Failure { performance_metrics, .. } => performance_metrics.as_ref(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn summary(&self) -> String {
        match self {
            TestResult::Success { duration, events, performance_metrics } => {
                let mut summary = format!("SUCCESS in {:.2}ms ({} events)",
                                        duration.as_millis(), events.len());
                if let Some(perf) = performance_metrics {
                    summary.push_str(&format!(" | Conn: {:.2}ms, Handshake: {:.2}ms",
                                            perf.connection_time.as_millis(),
                                            perf.handshake_time.as_millis()));
                    if perf.connection_count > 1 {
                        summary.push_str(&format!(", Connections: {}/{}",
                                                perf.successful_connections, perf.connection_count));
                    }
                }
                summary
            }
            TestResult::Failure { error, duration, performance_metrics } => {
                let mut summary = format!("FAILED in {:.2}ms: {}",
                                        duration.as_millis(), error);
                if let Some(perf) = performance_metrics {
                    summary.push_str(&format!(" | Conn: {:.2}ms, Handshake: {:.2}ms",
                                            perf.connection_time.as_millis(),
                                            perf.handshake_time.as_millis()));
                }
                summary
            }
        }
    }
}