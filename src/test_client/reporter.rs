use anyhow::Result;
use serde_json::Value;

pub struct Reporter;

impl Reporter {
    pub fn new() -> Self {
        Self
    }

    pub fn report_console(&self, _message: &str) -> Result<()> {
        // Stub implementation - will be implemented in Phase 3.3
        Err(anyhow::anyhow!("Not implemented yet"))
    }

    pub fn report_json(&self, _data: &Value) -> Result<()> {
        // Stub implementation - will be implemented in Phase 3.3
        Err(anyhow::anyhow!("Not implemented yet"))
    }
}