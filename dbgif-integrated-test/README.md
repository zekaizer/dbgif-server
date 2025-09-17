# DBGIF Integrated Test

A comprehensive testing framework for the DBGIF (Debug Interface) protocol server, combining client, device simulation, and scenario management in a single tool.

## Features

- 🚀 **All-in-one testing**: Single command to spawn devices, connect, and run tests
- 🔧 **Device simulation**: Embedded device servers with configurable behavior
- 📊 **Scenario-based testing**: Pre-built and custom test scenarios
- 🎯 **Performance testing**: Throughput, latency, and stress tests
- 📝 **Configuration support**: YAML/JSON configs with environment overrides
- 🎨 **Color-coded logging**: Component-specific debugging with timing info

## Quick Start

```bash
# Run all-in-one test with default settings
cargo run --bin dbgif-integrated-test -- all-in-one

# Run with multiple devices
cargo run --bin dbgif-integrated-test -- all-in-one --devices 3

# Run specific scenario
cargo run --bin dbgif-integrated-test -- scenario basic_connection

# List available scenarios
cargo run --bin dbgif-integrated-test -- list
```

## Installation

Add to your workspace `Cargo.toml`:

```toml
[workspace]
members = [".", "dbgif-integrated-test"]
```

## Usage

### CLI Commands

#### all-in-one
Automatically spawn devices and run tests:
```bash
dbgif-integrated-test all-in-one \
    --devices 3 \
    --scenario multi_device \
    --keep-alive
```

#### device-server
Start standalone device servers:
```bash
dbgif-integrated-test device-server \
    --count 2 \
    --ports 5557,5558 \
    --daemon
```

#### scenario
Run specific test scenarios:
```bash
dbgif-integrated-test scenario throughput_test \
    --config test-config.yaml
```

#### quick
Run quick test suite:
```bash
dbgif-integrated-test quick --skip-slow
```

### Configuration

Create a config file (`test-config.yaml`):

```yaml
version: "1.0"

server:
  address: "127.0.0.1:5555"
  timeout: 30
  retry_count: 3
  retry_delay: 1000

devices:
  count: 3
  base_port: 5557
  startup_delay: 100
  device_prefix: "test-device"
  capabilities:
    - shell
    - file
    - debug

scenarios:
  default: basic_flow
  timeout: 60
  enabled:
    - basic_connection
    - host_services
  disabled:
    - memory_leak

logging:
  level: info
  format: pretty
  color: true
  show_target: true

test:
  retry_count: 3
  retry_delay: 1000
  parallel: true
  cleanup_on_failure: true
```

### Environment Variables

Override configuration with environment variables:

```bash
export DBGIF_TEST_SERVER=127.0.0.1:5555
export DBGIF_TEST_DEVICES=5
export DBGIF_TEST_LOG_LEVEL=debug
export DBGIF_TEST_SCENARIO=performance

dbgif-integrated-test all-in-one
```

## Available Scenarios

### Basic Scenarios
- `basic_connection`: Test connection establishment
- `connection_handshake`: Test handshake protocol
- `error_handling`: Test error conditions
- `stream_multiplexing`: Test multiple streams
- `timeout_test`: Test timeout handling

### Advanced Scenarios
- `device_reconnection`: Test disconnect/reconnect
- `concurrent_streams`: Test concurrent operations
- `failure_recovery`: Test failure scenarios
- `cross_device`: Test multi-device communication
- `multi_device`: Test with multiple devices

### Performance Scenarios
- `throughput_test`: Measure data throughput
- `latency_test`: Measure round-trip latency
- `connection_limit`: Test connection limits
- `memory_leak`: Detect memory leaks

## API Usage

### As a Library

```rust
use dbgif_integrated_test::{
    TestClient,
    EmbeddedDeviceServer,
    ScenarioManager,
    TestConfig,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration
    let config = TestConfig::load("config.yaml")?;

    // Start device server
    let device_config = DeviceConfig {
        device_id: "my-device".to_string(),
        port: None,
        model: Some("TestDevice".to_string()),
        capabilities: None,
    };
    let device = EmbeddedDeviceServer::spawn(device_config).await?;

    // Connect client
    let mut client = TestClient::new("127.0.0.1:5555".parse()?);
    client.connect().await?;
    client.connect_device("127.0.0.1", device.port()).await?;

    // Run tests
    let mut manager = ScenarioManager::new();
    manager.add_scenario(Box::new(BasicConnectionScenario::new(addr)));
    manager.run_scenario("basic_connection").await?;

    Ok(())
}
```

### Custom Scenarios

```rust
use async_trait::async_trait;
use dbgif_integrated_test::scenarios::Scenario;

pub struct MyScenario {
    server_addr: SocketAddr,
}

#[async_trait]
impl Scenario for MyScenario {
    fn name(&self) -> &str {
        "my_custom_scenario"
    }

    async fn execute(&self) -> Result<()> {
        // Your test logic here
        Ok(())
    }
}
```

## Architecture

```
dbgif-integrated-test/
├── src/
│   ├── bin/
│   │   └── dbgif-integrated-test.rs  # CLI entry point
│   ├── client/
│   │   ├── mod.rs                     # Test client
│   │   ├── ascii.rs                   # ASCII protocol
│   │   └── connection.rs              # Connection manager
│   ├── device/
│   │   ├── mod.rs                     # Device server
│   │   ├── server.rs                  # Server implementation
│   │   └── lifecycle.rs               # Lifecycle management
│   ├── scenarios/
│   │   ├── mod.rs                     # Scenario framework
│   │   ├── basic.rs                   # Basic scenarios
│   │   ├── connection.rs              # Connection tests
│   │   ├── advanced.rs                # Advanced scenarios
│   │   └── performance.rs             # Performance tests
│   ├── utils/
│   │   ├── mod.rs                     # Utilities
│   │   ├── ports.rs                   # Port management
│   │   └── logging.rs                 # Logging utilities
│   └── config.rs                      # Configuration
├── tests/                              # Unit tests
└── examples/                           # Usage examples
```

## Development

### Running Tests

```bash
# Run all tests
cargo test --package dbgif-integrated-test

# Run with logging
RUST_LOG=debug cargo test

# Run specific test
cargo test --package dbgif-integrated-test test_basic_connection
```

### Adding New Scenarios

1. Create a new file in `src/scenarios/`
2. Implement the `Scenario` trait
3. Export from `src/scenarios/mod.rs`
4. Add to CLI in `src/bin/dbgif-integrated-test.rs`

### Debugging

Enable debug logging:
```bash
RUST_LOG=dbgif_integrated_test=trace cargo run -- all-in-one --debug
```

Component-specific logging:
```bash
RUST_LOG=dbgif_integrated_test::client=trace cargo run -- all-in-one
```

## Performance Tuning

### Throughput Testing
```bash
dbgif-integrated-test scenario throughput_test \
    --config throughput.yaml
```

Config for throughput testing:
```yaml
scenarios:
  parameters:
    throughput_data_size: 8192
    throughput_duration_secs: 30
```

### Latency Testing
```bash
dbgif-integrated-test scenario latency_test \
    --config latency.yaml
```

### Connection Limits
```bash
dbgif-integrated-test scenario connection_limit \
    --config limits.yaml
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Integration Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable

      - name: Start DBGIF Server
        run: |
          cargo run --bin dbgif-server &
          sleep 5

      - name: Run Integration Tests
        run: |
          cargo run --bin dbgif-integrated-test -- \
            all-in-one --devices 3

      - name: Run Performance Tests
        run: |
          cargo run --bin dbgif-integrated-test -- \
            scenario throughput_test
```

## Troubleshooting

### Common Issues

1. **Port conflicts**: Use `--ports` to specify custom ports
2. **Connection timeouts**: Increase timeout in config
3. **Memory issues**: Reduce device count or iterations
4. **Logging too verbose**: Adjust log level

### Debug Commands

```bash
# Check if server is running
nc -zv 127.0.0.1 5555

# List available ports
netstat -tuln | grep 555

# Monitor resource usage
watch -n 1 'ps aux | grep dbgif'
```

## Contributing

Contributions are welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Add tests for new features
4. Submit a pull request

## License

MIT License - see LICENSE file for details