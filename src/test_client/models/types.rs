use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TestType {
    Connection,
    Ping,
    HostCommands,
    MultiConnect,
    Protocol,
    Integration,
}

impl TestType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TestType::Connection => "connection",
            TestType::Ping => "ping",
            TestType::HostCommands => "host_commands",
            TestType::MultiConnect => "multi_connect",
            TestType::Protocol => "protocol",
            TestType::Integration => "integration",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            TestType::Connection => "Connection establishment test",
            TestType::Ping => "Ping command test",
            TestType::HostCommands => "Host commands test",
            TestType::MultiConnect => "Multi-connection test",
            TestType::Protocol => "Protocol behavior test",
            TestType::Integration => "Integration test",
        }
    }
}

impl std::fmt::Display for TestType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TestStatus {
    Pending,
    Running,
    Success,
    Failed,
    Timeout,
    Cancelled,
}

impl TestStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TestStatus::Pending => "pending",
            TestStatus::Running => "running",
            TestStatus::Success => "success",
            TestStatus::Failed => "failed",
            TestStatus::Timeout => "timeout",
            TestStatus::Cancelled => "cancelled",
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self,
            TestStatus::Success |
            TestStatus::Failed |
            TestStatus::Timeout |
            TestStatus::Cancelled
        )
    }

    pub fn is_successful(&self) -> bool {
        matches!(self, TestStatus::Success)
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            TestStatus::Pending => "⏳",
            TestStatus::Running => "🔄",
            TestStatus::Success => "✅",
            TestStatus::Failed => "❌",
            TestStatus::Timeout => "⏰",
            TestStatus::Cancelled => "🚫",
        }
    }
}

impl std::fmt::Display for TestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}