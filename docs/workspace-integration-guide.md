# DBGIF Integrated Test - Workspace Integration Guide

## Overview

This guide explains how `dbgif-integrated-test` is structured as a separate workspace member and how it integrates with the main `dbgif-server` and `dbgif-protocol` crates.

## Workspace Architecture

```
┌─────────────────────────────────────┐
│        Workspace Root               │
├─────────────────────────────────────┤
│                                     │
│  ┌──────────────────────────────┐  │
│  │   dbgif-protocol (library)   │  │  ← Shared protocol implementation
│  └──────────────────────────────┘  │
│            ▲         ▲              │
│            │         │              │
│  ┌─────────┴───┐ ┌──┴────────────┐ │
│  │dbgif-server │ │dbgif-integrated│ │
│  │  (binary)   │ │-test (lib+bin) │ │
│  └─────────────┘ └───────────────┘ │
│                                     │
└─────────────────────────────────────┘
```

## Key Benefits of Workspace Separation

### 1. **Clean Dependencies**
- `dbgif-integrated-test` depends on `dbgif-protocol` but not `dbgif-server`
- Avoids circular dependencies
- Clear separation of concerns

### 2. **Independent Versioning**
```toml
# Each crate can have its own version
dbgif-server = "1.0.0"
dbgif-protocol = "1.0.0"
dbgif-integrated-test = "0.1.0"  # Can evolve independently
```

### 3. **Parallel Development**
- Teams can work on test infrastructure without affecting server code
- Changes to test framework don't trigger server rebuilds
- Faster compilation times during development

### 4. **Reusability**
- Test framework can be used by external projects
- Can be published as separate crate if needed
- Other workspace members can depend on it for their tests

## Integration Points

### 1. Protocol Library Usage

`dbgif-integrated-test` uses the protocol library directly:

```rust
// dbgif-integrated-test/src/client/ascii.rs
use dbgif_protocol::protocol::ascii;
use dbgif_protocol::protocol::message::AdbMessage;

pub async fn send_command(stream: &mut TcpStream, cmd: &str) -> Result<()> {
    let encoded = ascii::encode_request(cmd);
    stream.write_all(&encoded).await?;
    Ok(())
}
```

### 2. Server Testing

The integrated test connects to the running server as a black-box:

```rust
// dbgif-integrated-test/src/scenarios/basic.rs
pub async fn test_server_connection() -> Result<()> {
    // Start embedded device server
    let device = EmbeddedDeviceServer::new(DeviceConfig::default());
    let device_port = device.start().await?;

    // Connect to actual dbgif-server (running separately)
    let mut client = TestClient::new("127.0.0.1:5555".parse()?);
    client.connect().await?;

    // Use host:connect to link server to device
    client.connect_to_device("127.0.0.1", device_port).await?;

    // Run tests...
}
```

### 3. Embedded Device Server

The device server runs in-process within the test framework:

```rust
// dbgif-integrated-test/src/device/server.rs
impl EmbeddedDeviceServer {
    pub async fn start(&mut self) -> Result<u16> {
        let port = find_available_port()?;
        let config = self.config.clone();

        // Spawn as tokio task, not separate process
        self.handle = Some(tokio::spawn(async move {
            run_device_server(config, port).await
        }));

        self.port = Some(port);
        Ok(port)
    }
}
```

## Build and Run Instructions

### 1. Build Everything
```bash
# From workspace root
cargo build --workspace

# Or build specific members
cargo build -p dbgif-integrated-test
```

### 2. Run Integrated Tests
```bash
# Simple all-in-one test
cargo run -p dbgif-integrated-test -- all-in-one

# With specific binary
cargo run --bin dbgif-integrated-test -- all-in-one

# From the crate directory
cd dbgif-integrated-test
cargo run -- all-in-one
```

### 3. Run Tests
```bash
# Run all workspace tests
cargo test --workspace

# Run only integrated test crate tests
cargo test -p dbgif-integrated-test

# Run with logging
RUST_LOG=debug cargo test -p dbgif-integrated-test
```

## Development Workflow

### 1. Making Changes to Protocol
When updating the protocol library:
```bash
# 1. Make changes in dbgif-protocol
cd dbgif-protocol
# ... edit files ...

# 2. Rebuild dependent crates
cargo build -p dbgif-server -p dbgif-integrated-test

# 3. Run tests to verify
cargo test --workspace
```

### 2. Adding New Test Scenarios
```bash
# 1. Create new scenario in integrated test
cd dbgif-integrated-test
vim src/scenarios/my_scenario.rs

# 2. Register in scenario factory
vim src/scenarios/mod.rs

# 3. Test locally
cargo run -- scenario my_scenario
```

### 3. Debugging Integration Issues
```bash
# Run with verbose logging
RUST_LOG=dbgif_integrated_test=trace cargo run -- all-in-one

# Run specific components
# Terminal 1: Start device server only
cargo run -- device-server --port 5557

# Terminal 2: Run client tests
cargo run -- test --device-port 5557
```

## Common Patterns

### 1. Test Fixture Setup
```rust
// dbgif-integrated-test/src/fixtures.rs
pub struct TestFixture {
    pub client: TestClient,
    pub devices: Vec<EmbeddedDeviceServer>,
    pub server_addr: SocketAddr,
}

impl TestFixture {
    pub async fn new(device_count: usize) -> Result<Self> {
        // Setup code...
    }

    pub async fn teardown(self) -> Result<()> {
        // Cleanup code...
    }
}
```

### 2. Parallel Test Execution
```rust
// Run multiple scenarios in parallel
let scenarios = vec!["basic", "advanced", "performance"];
let handles: Vec<_> = scenarios
    .into_iter()
    .map(|name| {
        tokio::spawn(async move {
            ScenarioFactory::create(name)?.execute().await
        })
    })
    .collect();

let results = futures::future::join_all(handles).await;
```

### 3. Port Management
```rust
// dbgif-integrated-test/src/utils/ports.rs
pub struct PortManager {
    base_port: u16,
    allocated: HashSet<u16>,
}

impl PortManager {
    pub fn allocate(&mut self) -> Result<u16> {
        // Find next available port...
    }

    pub fn release(&mut self, port: u16) {
        self.allocated.remove(&port);
    }
}
```

## Troubleshooting

### Issue: Cannot find dbgif-protocol
**Solution**: Ensure workspace member paths are correct in Cargo.toml
```toml
[dependencies]
dbgif-protocol = { path = "../dbgif-protocol" }
```

### Issue: Port conflicts during tests
**Solution**: Use dynamic port allocation
```rust
let port = portpicker::pick_unused_port()
    .ok_or_else(|| anyhow!("No ports available"))?;
```

### Issue: Embedded device server not responding
**Solution**: Check health before testing
```rust
device.start().await?;
tokio::time::sleep(Duration::from_millis(100)).await;
assert!(device.health_check().await?);
```

## Future Enhancements

1. **Test Discovery**: Automatic discovery of test scenarios from directory
2. **Parallel Execution**: Run multiple device servers in parallel
3. **Performance Metrics**: Built-in benchmarking and reporting
4. **Docker Support**: Containerized test environments
5. **Remote Testing**: Connect to remote DBGIF servers

## Conclusion

The workspace separation of `dbgif-integrated-test` provides:
- Clean architecture with clear boundaries
- Independent evolution of test infrastructure
- Better maintainability and reusability
- Simplified CI/CD integration

This design allows the test framework to grow independently while maintaining tight integration with the protocol library and providing comprehensive testing capabilities for the DBGIF server.