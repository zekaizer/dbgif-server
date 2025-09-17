use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TestConfig {
    #[serde(default)]
    pub version: String,
    pub server: ServerConfig,
    pub devices: DevicePoolConfig,
    pub scenarios: ScenarioConfig,
    pub logging: LoggingConfig,
    pub test: TestSettings,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            server: ServerConfig::default(),
            devices: DevicePoolConfig::default(),
            scenarios: ScenarioConfig::default(),
            logging: LoggingConfig::default(),
            test: TestSettings::default(),
        }
    }
}

/// Server connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_server_address")]
    pub address: String,
    #[serde(default = "default_timeout", with = "duration_secs")]
    pub timeout: Duration,
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
    #[serde(default = "default_retry_delay", with = "duration_millis")]
    pub retry_delay: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: default_server_address(),
            timeout: default_timeout(),
            retry_count: default_retry_count(),
            retry_delay: default_retry_delay(),
        }
    }
}

/// Device pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePoolConfig {
    #[serde(default = "default_device_count")]
    pub count: usize,
    #[serde(default = "default_base_port")]
    pub base_port: u16,
    #[serde(default = "default_startup_delay", with = "duration_millis")]
    pub startup_delay: Duration,
    #[serde(default)]
    pub device_prefix: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl Default for DevicePoolConfig {
    fn default() -> Self {
        Self {
            count: default_device_count(),
            base_port: default_base_port(),
            startup_delay: default_startup_delay(),
            device_prefix: "test-device".to_string(),
            capabilities: vec!["shell".to_string(), "file".to_string()],
        }
    }
}

/// Scenario configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioConfig {
    #[serde(default = "default_scenario")]
    pub default: String,
    #[serde(default = "default_scenario_timeout", with = "duration_secs")]
    pub timeout: Duration,
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(default)]
    pub parameters: ScenarioParameters,
}

impl Default for ScenarioConfig {
    fn default() -> Self {
        Self {
            default: default_scenario(),
            timeout: default_scenario_timeout(),
            enabled: Vec::new(),
            disabled: Vec::new(),
            parameters: ScenarioParameters::default(),
        }
    }
}

/// Scenario-specific parameters
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScenarioParameters {
    #[serde(default)]
    pub throughput_data_size: usize,
    #[serde(default)]
    pub throughput_duration_secs: u64,
    #[serde(default)]
    pub latency_iterations: usize,
    #[serde(default)]
    pub connection_limit_max: usize,
    #[serde(default)]
    pub memory_leak_iterations: usize,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
    #[serde(default = "default_color")]
    pub color: bool,
    #[serde(default)]
    pub show_target: bool,
    #[serde(default)]
    pub show_thread_ids: bool,
    #[serde(default)]
    pub show_line_numbers: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            color: default_color(),
            show_target: true,
            show_thread_ids: false,
            show_line_numbers: false,
        }
    }
}

/// General test settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSettings {
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
    #[serde(default = "default_retry_delay", with = "duration_millis")]
    pub retry_delay: Duration,
    #[serde(default)]
    pub fail_fast: bool,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default)]
    pub cleanup_on_failure: bool,
}

impl Default for TestSettings {
    fn default() -> Self {
        Self {
            retry_count: default_retry_count(),
            retry_delay: default_retry_delay(),
            fail_fast: false,
            parallel: false,
            cleanup_on_failure: true,
        }
    }
}

// Default value functions
fn default_server_address() -> String {
    "127.0.0.1:5555".to_string()
}

fn default_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_retry_count() -> u32 {
    3
}

fn default_retry_delay() -> Duration {
    Duration::from_millis(1000)
}

fn default_device_count() -> usize {
    2
}

fn default_base_port() -> u16 {
    5557
}

fn default_startup_delay() -> Duration {
    Duration::from_millis(100)
}

fn default_scenario() -> String {
    "basic_flow".to_string()
}

fn default_scenario_timeout() -> Duration {
    Duration::from_secs(60)
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_color() -> bool {
    true
}

// Serde helpers for Duration
mod duration_secs {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

impl TestConfig {
    /// Load configuration from file
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = if content.trim().starts_with('{') {
            serde_json::from_str(&content)?
        } else {
            serde_yaml::from_str(&content)?
        };

        config.validate()?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let content = if path.extension().and_then(|s| s.to_str()) == Some("json") {
            serde_json::to_string_pretty(self)?
        } else {
            serde_yaml::to_string(self)?
        };

        std::fs::write(path, content)?;
        Ok(())
    }

    /// Load from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(addr) = std::env::var("DBGIF_TEST_SERVER") {
            config.server.address = addr;
        }

        if let Ok(count) = std::env::var("DBGIF_TEST_DEVICES") {
            if let Ok(n) = count.parse() {
                config.devices.count = n;
            }
        }

        if let Ok(level) = std::env::var("DBGIF_TEST_LOG_LEVEL") {
            config.logging.level = level;
        }

        if let Ok(scenario) = std::env::var("DBGIF_TEST_SCENARIO") {
            config.scenarios.default = scenario;
        }

        config
    }

    /// Merge with environment variables (env takes precedence)
    pub fn merge_env(mut self) -> Self {
        if let Ok(addr) = std::env::var("DBGIF_TEST_SERVER") {
            self.server.address = addr;
        }

        if let Ok(count) = std::env::var("DBGIF_TEST_DEVICES") {
            if let Ok(n) = count.parse() {
                self.devices.count = n;
            }
        }

        if let Ok(level) = std::env::var("DBGIF_TEST_LOG_LEVEL") {
            self.logging.level = level;
        }

        self
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate server address
        if self.server.address.is_empty() {
            return Err(anyhow::anyhow!("Server address cannot be empty"));
        }

        // Validate timeout
        if self.server.timeout.as_secs() == 0 {
            return Err(anyhow::anyhow!("Server timeout must be greater than 0"));
        }

        // Validate device count
        if self.devices.count > 100 {
            return Err(anyhow::anyhow!("Device count cannot exceed 100"));
        }

        // Validate log level
        match self.logging.level.as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {}
            _ => return Err(anyhow::anyhow!("Invalid log level: {}", self.logging.level)),
        }

        Ok(())
    }

    /// Create example configuration file
    pub fn create_example(path: impl AsRef<Path>) -> Result<()> {
        let example = Self {
            version: "1.0".to_string(),
            server: ServerConfig {
                address: "127.0.0.1:5555".to_string(),
                timeout: Duration::from_secs(30),
                retry_count: 3,
                retry_delay: Duration::from_millis(1000),
            },
            devices: DevicePoolConfig {
                count: 3,
                base_port: 5557,
                startup_delay: Duration::from_millis(100),
                device_prefix: "example-device".to_string(),
                capabilities: vec!["shell".to_string(), "file".to_string(), "debug".to_string()],
            },
            scenarios: ScenarioConfig {
                default: "basic_flow".to_string(),
                timeout: Duration::from_secs(60),
                enabled: vec!["basic_connection".to_string(), "host_services".to_string()],
                disabled: vec!["memory_leak".to_string()],
                parameters: ScenarioParameters {
                    throughput_data_size: 4096,
                    throughput_duration_secs: 10,
                    latency_iterations: 100,
                    connection_limit_max: 50,
                    memory_leak_iterations: 1000,
                },
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "pretty".to_string(),
                color: true,
                show_target: true,
                show_thread_ids: false,
                show_line_numbers: false,
            },
            test: TestSettings {
                retry_count: 3,
                retry_delay: Duration::from_millis(1000),
                fail_fast: false,
                parallel: true,
                cleanup_on_failure: true,
            },
        };

        example.save(path)?;
        Ok(())
    }
}