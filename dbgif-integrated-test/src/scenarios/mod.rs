pub mod basic;
pub mod connection;
pub mod advanced;
pub mod performance;

use anyhow::Result;
use async_trait::async_trait;

pub use basic::{BasicConnectionScenario, HostServicesScenario, MultiDeviceScenario};
pub use connection::{
    ConnectionHandshakeScenario,
    ErrorHandlingScenario,
    StreamMultiplexingScenario,
    TimeoutScenario,
};
pub use advanced::{
    DeviceReconnectionScenario,
    ConcurrentStreamScenario,
    FailureRecoveryScenario,
    CrossDeviceScenario,
};
pub use performance::{
    ThroughputScenario,
    LatencyScenario,
    ConnectionLimitScenario,
    MemoryLeakScenario,
};

pub struct ScenarioManager {
    scenarios: Vec<Box<dyn Scenario>>,
}

#[async_trait]
pub trait Scenario: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self) -> Result<()>;
}

impl ScenarioManager {
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
        }
    }

    pub fn add_scenario(&mut self, scenario: Box<dyn Scenario>) {
        self.scenarios.push(scenario);
    }

    pub async fn run_scenario(&self, name: &str) -> Result<()> {
        let scenario = self.scenarios
            .iter()
            .find(|s| s.name() == name)
            .ok_or_else(|| anyhow::anyhow!("Scenario '{}' not found", name))?;

        scenario.execute().await
    }

    pub fn list_scenarios(&self) -> Vec<String> {
        self.scenarios.iter().map(|s| s.name().to_string()).collect()
    }
}