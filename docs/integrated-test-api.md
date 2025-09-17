# DBGIF Integrated Test - API Contracts and Interfaces

## Overview

This document defines the API contracts and interfaces for the integrated test binary modules. All interfaces are designed to be async-first and use Result types for error handling.

## Core Traits and Interfaces

### 1. TestClient Trait

```rust
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::net::TcpStream;

/// Trait for test client operations
#[async_trait]
pub trait TestClient: Send + Sync {
    /// Connect to DBGIF server
    async fn connect(&mut self, addr: SocketAddr) -> Result<(), TestError>;

    /// Send ASCII command
    async fn send_command(&mut self, command: &str) -> Result<CommandResponse, TestError>;

    /// Send STRM data
    async fn send_strm(&mut self, stream_id: u8, data: &[u8]) -> Result<(), TestError>;

    /// Receive STRM data
    async fn receive_strm(&mut self) -> Result<(u8, Vec<u8>), TestError>;

    /// List connected devices
    async fn list_devices(&mut self) -> Result<Vec<DeviceInfo>, TestError>;

    /// Connect to specific device via host:connect
    async fn connect_to_device(&mut self, ip: &str, port: u16) -> Result<(), TestError>;

    /// Get connection status
    fn is_connected(&self) -> bool;

    /// Disconnect from server
    async fn disconnect(&mut self) -> Result<(), TestError>;
}

/// Command response from server
#[derive(Debug, Clone)]
pub struct CommandResponse {
    pub success: bool,
    pub data: String,
    pub raw_bytes: Vec<u8>,
}

/// Device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub model: String,
    pub state: DeviceState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceState {
    Online,
    Offline,
    Unauthorized,
}
```

### 2. DeviceServer Trait

```rust
/// Trait for embedded device server
#[async_trait]
pub trait DeviceServer: Send + Sync {
    /// Start the device server
    async fn start(&mut self) -> Result<u16, TestError>;

    /// Stop the device server
    async fn stop(&mut self) -> Result<(), TestError>;

    /// Get server status
    fn status(&self) -> DeviceServerStatus;

    /// Get the port server is listening on
    fn port(&self) -> Option<u16>;

    /// Health check
    async fn health_check(&self) -> Result<bool, TestError>;

    /// Get device configuration
    fn config(&self) -> &DeviceConfig;

    /// Update device configuration (requires restart)
    async fn update_config(&mut self, config: DeviceConfig) -> Result<(), TestError>;
}

/// Device server configuration
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    pub device_id: String,
    pub device_model: String,
    pub port: Option<u16>,  // None for auto-allocation
    pub capabilities: Vec<String>,
    pub latency_ms: u64,
    pub error_rate: f64,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            device_id: "test-device-001".to_string(),
            device_model: "DBGIF-TestDevice".to_string(),
            port: None,
            capabilities: vec!["shell".to_string(), "file".to_string()],
            latency_ms: 0,
            error_rate: 0.0,
        }
    }
}

/// Device server status
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceServerStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}
```

### 3. Scenario Trait

```rust
/// Trait for test scenarios
#[async_trait]
pub trait Scenario: Send + Sync {
    /// Get scenario name
    fn name(&self) -> &str;

    /// Get scenario description
    fn description(&self) -> &str;

    /// Setup before scenario execution
    async fn setup(&mut self, context: &mut ScenarioContext) -> Result<(), TestError>;

    /// Execute the scenario
    async fn execute(&mut self, context: &mut ScenarioContext) -> Result<ScenarioResult, TestError>;

    /// Teardown after scenario execution
    async fn teardown(&mut self, context: &mut ScenarioContext) -> Result<(), TestError>;

    /// Validate scenario results
    fn validate(&self, result: &ScenarioResult) -> ValidationResult;
}

/// Scenario execution context
pub struct ScenarioContext {
    pub client: Box<dyn TestClient>,
    pub devices: Vec<Box<dyn DeviceServer>>,
    pub config: ScenarioConfig,
    pub metadata: HashMap<String, String>,
}

/// Scenario configuration
#[derive(Debug, Clone)]
pub struct ScenarioConfig {
    pub timeout: Duration,
    pub retry_count: u32,
    pub parallel_execution: bool,
    pub fail_fast: bool,
}

/// Scenario execution result
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub name: String,
    pub success: bool,
    pub duration: Duration,
    pub steps: Vec<StepResult>,
    pub metrics: HashMap<String, f64>,
}

/// Individual step result
#[derive(Debug, Clone)]
pub struct StepResult {
    pub name: String,
    pub success: bool,
    pub duration: Duration,
    pub error: Option<String>,
    pub data: Option<serde_json::Value>,
}

/// Validation result
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub passed: bool,
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Clone)]
pub struct Assertion {
    pub name: String,
    pub passed: bool,
    pub expected: String,
    pub actual: String,
    pub message: Option<String>,
}
```

### 4. ProcessManager Trait

```rust
/// Trait for managing embedded processes
#[async_trait]
pub trait ProcessManager: Send + Sync {
    /// Spawn a new managed process
    async fn spawn<F, Fut>(&mut self, name: String, task: F) -> Result<ProcessHandle, TestError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), TestError>> + Send + 'static;

    /// Stop a specific process
    async fn stop(&mut self, handle: ProcessHandle) -> Result<(), TestError>;

    /// Stop all processes
    async fn stop_all(&mut self) -> Result<(), TestError>;

    /// Get process status
    fn status(&self, handle: &ProcessHandle) -> ProcessStatus;

    /// List all processes
    fn list(&self) -> Vec<ProcessInfo>;

    /// Wait for all processes to complete
    async fn wait_all(&mut self) -> Result<(), TestError>;
}

/// Handle to a managed process
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ProcessHandle(pub Uuid);

/// Process status
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// Process information
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub handle: ProcessHandle,
    pub name: String,
    pub status: ProcessStatus,
    pub started_at: Instant,
    pub stopped_at: Option<Instant>,
}
```

## Error Types

```rust
use thiserror::Error;

/// Main error type for test operations
#[derive(Error, Debug)]
pub enum TestError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Device error: {0}")]
    Device(String),

    #[error("Scenario error: {0}")]
    Scenario(String),

    #[error("Timeout after {0:?}")]
    Timeout(Duration),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Process error: {0}")]
    Process(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Other error: {0}")]
    Other(String),
}

/// Result type alias
pub type TestResult<T> = Result<T, TestError>;
```

## Implementation Modules

### Client Module (`dbgif-integrated-test/src/client/mod.rs`)

```rust
use dbgif_protocol::protocol::ascii;
use std::net::SocketAddr;
use tokio::net::TcpStream;

pub struct DefaultTestClient {
    connection: Option<TcpStream>,
    server_addr: SocketAddr,
    timeout: Duration,
}

impl DefaultTestClient {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self {
            connection: None,
            server_addr,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl TestClient for DefaultTestClient {
    // Implementation of trait methods...
}
```

### Device Module (`dbgif-integrated-test/src/device/mod.rs`)

```rust
use dbgif_protocol::protocol::{AdbMessage, AdbCommand};
use tokio::task::JoinHandle;

pub struct EmbeddedDeviceServer {
    config: DeviceConfig,
    status: DeviceServerStatus,
    handle: Option<JoinHandle<()>>,
    port: Option<u16>,
}

impl EmbeddedDeviceServer {
    pub fn new(config: DeviceConfig) -> Self {
        Self {
            config,
            status: DeviceServerStatus::Stopped,
            handle: None,
            port: None,
        }
    }
}

#[async_trait]
impl DeviceServer for EmbeddedDeviceServer {
    // Implementation of trait methods...
}
```

### Scenario Module (`dbgif-integrated-test/src/scenarios/mod.rs`)

```rust
/// Basic connection test scenario
pub struct BasicConnectionScenario {
    name: String,
    device_count: usize,
}

impl BasicConnectionScenario {
    pub fn new(device_count: usize) -> Self {
        Self {
            name: "basic_connection".to_string(),
            device_count,
        }
    }
}

#[async_trait]
impl Scenario for BasicConnectionScenario {
    // Implementation of trait methods...
}

/// Scenario factory
pub struct ScenarioFactory;

impl ScenarioFactory {
    pub fn create(name: &str) -> Result<Box<dyn Scenario>, TestError> {
        match name {
            "basic_connection" => Ok(Box::new(BasicConnectionScenario::new(1))),
            "multi_device" => Ok(Box::new(MultiDeviceScenario::new(3))),
            "performance" => Ok(Box::new(PerformanceScenario::new())),
            _ => Err(TestError::Scenario(format!("Unknown scenario: {}", name))),
        }
    }

    pub fn list() -> Vec<&'static str> {
        vec!["basic_connection", "multi_device", "performance", "stress_test"]
    }
}
```

### Process Module (`dbgif-integrated-test/src/process/mod.rs`)

```rust
pub struct DefaultProcessManager {
    processes: HashMap<ProcessHandle, ProcessEntry>,
}

struct ProcessEntry {
    name: String,
    handle: JoinHandle<Result<(), TestError>>,
    status: ProcessStatus,
    started_at: Instant,
}

impl DefaultProcessManager {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
        }
    }
}

#[async_trait]
impl ProcessManager for DefaultProcessManager {
    // Implementation of trait methods...
}
```

## Usage Examples

### Creating and Running a Basic Test

```rust
use dbgif_integrated_test::*;

#[tokio::main]
async fn main() -> Result<(), TestError> {
    // Create client
    let mut client = DefaultTestClient::new("127.0.0.1:5555".parse()?);

    // Create device server
    let mut device = EmbeddedDeviceServer::new(DeviceConfig::default());

    // Start device server
    let device_port = device.start().await?;

    // Connect client to DBGIF server
    client.connect("127.0.0.1:5555".parse()?).await?;

    // Connect to device via host:connect
    client.connect_to_device("127.0.0.1", device_port).await?;

    // Run tests
    let devices = client.list_devices().await?;
    assert_eq!(devices.len(), 1);

    // Cleanup
    device.stop().await?;
    client.disconnect().await?;

    Ok(())
}
```

### Running a Scenario

```rust
use dbgif_integrated_test::*;

#[tokio::main]
async fn main() -> Result<(), TestError> {
    // Create scenario
    let mut scenario = ScenarioFactory::create("basic_connection")?;

    // Setup context
    let mut context = ScenarioContext {
        client: Box::new(DefaultTestClient::new("127.0.0.1:5555".parse()?)),
        devices: vec![Box::new(EmbeddedDeviceServer::new(DeviceConfig::default()))],
        config: ScenarioConfig {
            timeout: Duration::from_secs(60),
            retry_count: 3,
            parallel_execution: false,
            fail_fast: true,
        },
        metadata: HashMap::new(),
    };

    // Run scenario
    scenario.setup(&mut context).await?;
    let result = scenario.execute(&mut context).await?;
    scenario.teardown(&mut context).await?;

    // Validate results
    let validation = scenario.validate(&result);
    if !validation.passed {
        eprintln!("Scenario failed validation:");
        for assertion in validation.assertions {
            if !assertion.passed {
                eprintln!("  - {}: expected {}, got {}",
                    assertion.name, assertion.expected, assertion.actual);
            }
        }
        return Err(TestError::Validation("Scenario validation failed".into()));
    }

    println!("Scenario completed successfully in {:?}", result.duration);
    Ok(())
}
```

### Using Process Manager

```rust
use dbgif_integrated_test::*;

#[tokio::main]
async fn main() -> Result<(), TestError> {
    let mut manager = DefaultProcessManager::new();

    // Spawn device servers
    let handles: Vec<ProcessHandle> = vec![];
    for i in 0..3 {
        let handle = manager.spawn(
            format!("device-{}", i),
            || async {
                let mut device = EmbeddedDeviceServer::new(DeviceConfig {
                    device_id: format!("device-{}", i),
                    ..Default::default()
                });
                device.start().await?;
                // Keep running...
                tokio::time::sleep(Duration::from_secs(60)).await;
                device.stop().await?;
                Ok(())
            }
        ).await?;
        handles.push(handle);
    }

    // Check status
    for handle in &handles {
        let status = manager.status(handle);
        println!("Process {:?} status: {:?}", handle, status);
    }

    // Stop all
    manager.stop_all().await?;

    Ok(())
}
```

## Testing the Interfaces

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mockall::*;

    // Mock client for testing
    mock! {
        TestClientMock {}

        #[async_trait]
        impl TestClient for TestClientMock {
            async fn connect(&mut self, addr: SocketAddr) -> Result<(), TestError>;
            async fn send_command(&mut self, command: &str) -> Result<CommandResponse, TestError>;
            // ... other methods
        }
    }

    #[tokio::test]
    async fn test_client_interface() {
        let mut mock = MockTestClientMock::new();
        mock.expect_connect()
            .returning(|_| Ok(()));
        mock.expect_send_command()
            .returning(|_| Ok(CommandResponse {
                success: true,
                data: "OK".to_string(),
                raw_bytes: vec![],
            }));

        // Use mock in tests
        let result = mock.connect("127.0.0.1:5555".parse().unwrap()).await;
        assert!(result.is_ok());
    }
}
```

## Next Steps

1. Implement the core traits in their respective modules
2. Create mock implementations for testing
3. Add integration tests for each interface
4. Document edge cases and error handling
5. Add performance benchmarks for critical paths