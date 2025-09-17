# DBGIF Server Quickstart Guide (ADB-like Protocol)

**Version**: 1.0.0
**Date**: 2025-09-16
**Target**: Developers implementing or testing ADB-like DBGIF protocol (NOT fully ADB-compatible)

## Overview

This guide walks you through setting up and testing the DBGIF server with ADB-like protocol support:
1. **DBGIF Server** - ADB-like protocol server with host services (NOT fully ADB-compatible)
2. **DBGIF Integrated Test** - Comprehensive test framework with built-in device simulation

**Architecture**:
```
DBGIF Client ↔ DBGIF Server ↔ TCP Transport ↔ TCP Device (DBGIF daemon)
```

## Prerequisites

- Rust (latest stable version)
- TCP ports 5555, 5556, 5557 available
- Terminal access for multiple processes
- Basic understanding of message-based protocols

## Quick Start (5 minutes)

### 1. Build the Project
```bash
# Clone and build
git clone <repository>
cd dbgif-server
cargo build --release

# Verify builds
ls target/release/
# Should see: dbgif-server, dbgif-integrated-test
```

### 2. Start the DBGIF Server
```bash
# Terminal 1: Start DBGIF protocol server
./target/release/dbgif-server --port 5555 --discovery-ports 5557 --max-connections 100 --verbose

# Expected output:
# DBGIF Server v1.0.0 (ADB-like Protocol)
# Listening on 0.0.0.0:5555
# Device discovery ports: [5557]
# Max connections: 100
# Host services: [host:list, host:device, host:version, host:features]
# Discovering devices...
# Found device: tcp:custom001 (offline)
# Server ready for connections
```

### 3. Run Integrated Tests (Includes Device Simulation)
```bash
# Terminal 2: Run comprehensive tests with automatic device simulation
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test basic

# Expected output:
# DBGIF Integrated Test v1.0.0 (ADB-like Protocol)
# Connecting to 127.0.0.1:5555...
# ✓ TCP connection established
# ✓ CNXN handshake successful
# ✓ Protocol version negotiated (0x01000000)
# ✓ Server identification received
# Test completed successfully
```

## Detailed Testing

### Connection and Handshake Test
```bash
# Test ADB protocol connection establishment
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test connection

# Tests:
# - TCP connection to server
# - CNXN message exchange
# - Protocol version negotiation
# - Client/server identification
# - Connection graceful closure
```

### Host Services Test
```bash
# Test server's built-in host services
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test host-services

# Tests:
# - host:version (server version info)
# - host:features (server capabilities)
# - host:list (device discovery and listing)
# - host:device:<id> (device selection)
# - Invalid host service handling
```

### Device Selection and Communication Test
```bash
# Test device selection and command forwarding
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test device-communication

# Tests:
# - Device discovery via host:list
# - Device selection via host:device:tcp:custom001
# - Stream opening to selected device (shell: service)
# - Command execution through stream
# - Bidirectional data exchange
# - Stream closure
```

### Stream Multiplexing Test
```bash
# Test multiple concurrent streams
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test streams

# Tests:
# - Multiple OPEN commands with different stream IDs
# - Concurrent WRTE operations on different streams
# - Independent stream lifecycle management
# - Stream ID collision avoidance
# - Proper OKAY/CLSE responses for each stream
```

### Multi-Client Test
```bash
# Test multiple clients accessing same device
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test multi-client --concurrent 3

# Tests:
# - 3 clients connecting simultaneously
# - Each client selecting same device
# - Independent stream spaces per client
# - Concurrent device access
# - No interference between client sessions
```

### Performance Test
```bash
# Test server performance under load
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test performance --connections 50

# Tests:
# - 50 concurrent client connections
# - ADB message throughput measurement
# - Response latency (<100ms target)
# - Memory usage monitoring
# - Stream multiplexing efficiency
```

### Protocol Compliance Test
```bash
# Test ADB protocol format compliance
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test protocol-compliance

# Tests:
# - 24-byte header format validation
# - Little-endian field encoding
# - CRC32 data payload validation
# - Magic number verification (!command)
# - Invalid message handling
# - Error response format (CLSE with reason)
```

## Configuration Options

### DBGIF Server Options
```bash
./target/release/dbgif-server --help

# Key options:
--port 5555                    # Server listening port
--discovery-ports 5557,5558    # Ports to scan for devices
--max-connections 100          # Maximum concurrent client connections
--connection-timeout 30        # Client connection timeout in seconds
--device-discovery-interval 10 # Device discovery interval in seconds
--ping-interval 1              # Device ping interval in seconds
--ping-timeout 3               # Device ping timeout in seconds
--log-level info              # Logging level (error|warn|info|debug|trace)
--config /path/to/config.toml  # Configuration file
```

### Test Client Options
```bash
./target/release/dbgif-integrated-test --help

# Key options:
--server 127.0.0.1:5555       # DBGIF server address
--test basic                  # Test suite to run
--device-id tcp:custom001     # Target device for device tests
--concurrent 1                # Number of concurrent connections
--streams 5                   # Number of streams per connection
--iterations 10               # Test iterations
--timeout 10                  # Test timeout in seconds
--output json                 # Output format (text|json|adb-wire)
--protocol-version 0x01000000 # ADB protocol version
```

### TCP Device Test Server Options
```bash
./target/release/dbgif-integrated-test (device mode) --help

# Key options:
--port 5557                   # Listening port
--device-id tcp:custom001     # Device identifier
--system-type tizen           # Device system type
--model MyBoard               # Device model name
--version v1.2                # Device version
--connect-id 0x12345678       # Connection ID for PING/PONG
--services shell:,tcp:8080    # Supported services (comma-separated)
--response-delay-ms 10        # Simulated processing delay
```

## Expected Results

### Successful Basic Test Output
```
DBGIF Test Client v1.0.0 (ADB Protocol)
Target server: 127.0.0.1:5555
Test suite: basic

[INFO] Starting ADB connection test...
✓ TCP connection established (2ms)
✓ CNXN message sent (version: 0x01000000, max_len: 0x00100000)
✓ CNXN response received (server: dbgif-server v1.0.0)
✓ ADB protocol handshake successful

[INFO] Starting host services test...
✓ host:version - "dbgif-server 1.0.0"
✓ host:features - "lazy-connection\nmulti-client\nping-pong\nhost-services"
✓ host:list - "tcp:custom001 offline tizen MyBoard v1.2"

Test Results:
- Connection latency: 2ms ✓
- Protocol handshake: 15ms ✓
- Host service response: 25ms ✓
- ADB message format: compliant ✓
- CRC32 validation: passed ✓

All tests passed successfully!
```

### Device Communication Test Output
```
[INFO] Testing device communication flow...
✓ Device selection: host:device:tcp:custom001
✓ Device status: offline → device (lazy connection)
✓ Stream opening: OPEN(1, 0, "shell:")
✓ Stream established: OKAY(200, 1)
✓ Command execution: WRTE(1, 200, "echo hello")
✓ Response received: WRTE(200, 1, "hello\n")
✓ Stream closure: CLSE(1, 200) → CLSE(200, 1)

Stream Lifecycle: OPEN → OKAY → WRTE → OKAY → CLSE → CLSE ✓
Device forwarding: client(1) ↔ server ↔ device(200) ✓
Message integrity: All CRC32 validations passed ✓
```

### Performance Test Metrics
```
Performance Test Results (50 concurrent connections):
- Connection establishment: 45ms avg (target: <100ms) ✓
- CNXN handshake time: 12ms avg ✓
- Host service response: 8ms avg ✓
- Stream throughput: 1,250 streams/sec ✓
- Message throughput: 5,500 messages/sec ✓
- Memory usage: 125MB (stable) ✓
- CPU usage: 35% (avg) ✓
- Zero message corruption ✓
- Zero connection drops ✓
```

## Troubleshooting

### Connection Issues
```bash
# Check if ports are available
netstat -tulpn | grep -E ':(5555|5556|5557)'

# Test basic TCP connectivity
telnet 127.0.0.1 5555
telnet 127.0.0.1 5557

# Verify ADB message format
./target/release/dbgif-integrated-test --test protocol-compliance --verbose
```

### Protocol Issues
```bash
# Enable debug logging for message traces
./target/release/dbgif-server --log-level debug

# Check message format compliance
./target/release/dbgif-integrated-test --output adb-wire --verbose

# Common issues:
# - Incorrect CRC32 calculation
# - Wrong byte order (must be little-endian)
# - Invalid magic number (!command)
# - Malformed stream ID usage
```

### Performance Issues
```bash
# Monitor system resources
top -p $(pgrep dbgif-server)

# Check connection limits
ulimit -n  # Should be > 1024 for 100 connections

# Increase limits if needed
ulimit -n 4096

# Profile memory usage
./target/release/dbgif-server --log-level trace | grep -i memory
```

## Development Workflow

### 1. Protocol Development
```bash
# Run protocol compliance tests during development
cargo test --test protocol_tests

# Test ADB message serialization
cargo test --test adb_message_tests

# Validate CRC32 implementation
cargo test --test crc32_tests
```

### 2. Integration Testing
```bash
# Run full ADB protocol integration tests
cargo test --test integration_tests

# Test with real device simulation
./scripts/run-integration-tests.sh
```

### 3. Performance Validation
```bash
# Benchmark message throughput
cargo bench --bench adb_throughput

# Test concurrent connection handling
./target/release/dbgif-integrated-test --test performance --connections 100
```

## ADB Protocol Compliance

### Message Format Compliance
- ✅ 24-byte header with exact field layout
- ✅ Little-endian encoding for all multi-byte fields
- ✅ CRC32 validation on data payloads
- ✅ Magic number verification (!command)
- ✅ Proper command code constants

### Command Semantics Compliance
- ✅ CNXN: Bidirectional handshake with version negotiation
- ✅ OPEN: Stream establishment with service name
- ✅ OKAY: Success acknowledgment with stream mapping
- ✅ WRTE: Data transfer with acknowledgment requirement
- ✅ CLSE: Stream closure with optional error reason
- ✅ PING/PONG: Connection health monitoring

### Stream Management Compliance
- ✅ Independent stream ID spaces per session
- ✅ Bidirectional stream mapping without ID translation
- ✅ Proper stream lifecycle: OPEN → OKAY → WRTE → CLSE
- ✅ Multi-client device access support
- ✅ Stream multiplexing with isolation

## Next Steps

1. **Explore Device Services**: Test different service types (shell:, tcp:port)
2. **Multi-Device Setup**: Configure multiple TCP devices
3. **Custom Services**: Implement custom device services
4. **Load Testing**: Scale up to 100 concurrent connections
5. **USB Support**: Implement USB transport layer (future)
6. **Authentication**: Add AUTH command support (future)

## Support

- **Protocol Reference**: See `docs/protocol.md` for detailed ADB protocol specification
- **API Documentation**: Check `docs/api/` for library APIs
- **Examples**: Review `examples/` for usage patterns
- **Issues**: Report bugs via GitHub issues
- **Community**: Join discussions in project forums