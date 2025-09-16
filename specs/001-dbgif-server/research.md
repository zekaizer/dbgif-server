# Research: DBGIF Server Development

**Date**: 2025-09-16
**Status**: Updated for ADB-style Protocol
**Context**: Research for implementing DBGIF protocol server with existing ADB-style protocol specification

## Technology Decisions

### 1. Rust Async Runtime Selection

**Decision**: Use tokio as the primary async runtime

**Rationale**:
- Most mature and widely adopted async runtime in Rust ecosystem
- Excellent performance for network I/O operations
- Comprehensive ecosystem with TCP server primitives
- Built-in support for concurrent connection handling
- Strong integration with other networking crates

**Alternatives considered**:
- async-std: Good alternative but smaller ecosystem
- smol: Lightweight but less feature-complete for server applications

### 2. TCP Server Implementation

**Decision**: Use tokio::net::TcpListener with async connection handling

**Rationale**:
- Native tokio support for TCP operations
- Built-in backpressure handling
- Efficient connection pooling and management
- Easy to implement connection limits (100 concurrent)

**Key patterns**:
- Spawn individual tasks per connection
- Use Arc<RwLock<ServerState>> for server state management
- Implement graceful shutdown with tokio::signal

### 3. Transport Abstraction Strategy

**Decision**: Create transport abstraction trait with TCP implementation first, USB support later

**Rationale**:
- Allows future USB implementation without changing core logic
- Clean separation of concerns
- Can test with TCP before adding USB complexity
- nusb 2.0 integration can be added later

**Architecture**:
```rust
trait Transport {
    async fn listen(&mut self) -> Result<(), TransportError>;
    async fn accept(&mut self) -> Result<Box<dyn Connection>, TransportError>;
    async fn discover_devices(&mut self) -> Result<Vec<DeviceInfo>, TransportError>;
}

trait Connection {
    async fn read_message(&mut self) -> Result<AdbMessage, ConnectionError>;
    async fn write_message(&mut self, msg: &AdbMessage) -> Result<(), ConnectionError>;
    fn peer_info(&self) -> String;
}
```

### 4. Protocol Implementation

**Decision**: ADB-style binary protocol as specified in docs/protocol.md

**Rationale**:
- Well-established protocol design
- Proven scalability and reliability
- Clear command/response semantics
- Built-in stream multiplexing support
- CRC32 data validation

**Message format** (24-byte header + variable data):
```rust
struct AdbMessage {
    command: u32,     // Command code (little-endian)
    arg0: u32,        // First argument (little-endian)
    arg1: u32,        // Second argument (little-endian)
    data_length: u32, // Length of data payload (little-endian)
    data_crc32: u32,  // CRC32 of data payload (little-endian)
    magic: u32,       // Magic number (~command, bitwise NOT)
    data: Vec<u8>,    // Variable length data payload
}
```

### 5. Command Processing Architecture

**Decision**: Command-based message dispatch with async handlers

**Rationale**:
- ADB protocol defines specific commands (CNXN, OPEN, WRTE, CLSE, PING, PONG)
- Each command has distinct handling logic
- Host services require special processing
- Stream multiplexing needs efficient routing

**Command handlers**:
```rust
enum AdbCommand {
    CNXN = 0x4E584E43,
    OPEN = 0x4E45504F,
    OKAY = 0x59414B4F,
    WRTE = 0x45545257,
    CLSE = 0x45534C43,
    PING = 0x474E4950,
    PONG = 0x474E4F50,
}
```

### 6. Stream Management

**Decision**: Bidirectional stream mapping without ID translation

**Rationale**:
- Multiple clients can use same stream IDs (independent namespaces)
- Server maintains (ClientSession, StreamID) ↔ (Device, StreamID) mapping
- No ID modification during forwarding
- Clean session isolation

**Stream state**:
- Each client session has independent stream ID space (1-65535)
- Server maps client streams to device streams
- Stream lifecycle: OPEN → OKAY → WRTE/OKAY cycles → CLSE

### 7. Device Management

**Decision**: Lazy connection with device discovery and registration

**Rationale**:
- Devices can be discovered without immediate connection
- "offline" status for discovered but unconnected devices
- On-demand connection when client selects device
- Efficient resource usage

**Device lifecycle**:
1. Discovery: Transport layer finds devices → "offline" status
2. Registration: Server maintains device list
3. Selection: Client uses "host:device:<id>" to select target
4. Connection: Server connects to device when needed → "device" status
5. Usage: Client commands forwarded to selected device

### 8. Host Services Implementation

**Decision**: Built-in server services accessible via OPEN command

**Rationale**:
- Clean protocol integration (uses same OPEN mechanism)
- "host:" prefix distinguishes server services from device services
- Extensible for future server features

**Core services**:
- host:list - List available devices
- host:device:<id> - Select target device
- host:version - Server version info
- host:features - Server capabilities

### 9. Error Handling

**Decision**: Use thiserror for structured error types with protocol-specific errors

**Rationale**:
- ADB protocol uses CLSE for operation failures
- Need structured errors for different failure modes
- Integration with tracing for observability
- Clear error context for debugging

### 10. Configuration Management

**Decision**: Use clap for CLI arguments with serde for config files

**Rationale**:
- Standard CLI parsing for Rust
- Server needs configurable ports and connection limits
- Device transport configuration
- Logging level control

### 11. Testing Strategy

**Decision**: Multi-level testing matching ADB protocol structure

**Rationale**:
- Protocol message serialization/deserialization tests
- Command handler unit tests
- Stream management integration tests
- Full client-server-device chain tests

**Test components**:
- dbgif-test-client: Simulates ADB client with various test scenarios
- tcp-device-test-server: Simulates ADB device daemon
- Integration tests: Full three-way communication flow

### 12. Performance Considerations

**Decision**: Async stream forwarding with connection limits

**Rationale**:
- Stream multiplexing requires efficient async forwarding
- Connection limits prevent resource exhaustion
- CRC32 validation on all data payloads
- Lazy device connections reduce overhead

## Implementation Priorities

1. **Phase 1**: Basic TCP transport and protocol parsing
2. **Phase 2**: Server with connection management
3. **Phase 3**: Test client and device test server
4. **Phase 4**: Integration testing and performance validation
5. **Phase 5**: USB transport abstraction (future)

## Risk Assessment

**Low Risk**:
- TCP implementation (well-established patterns)
- Basic protocol parsing (standard Rust techniques)
- Testing infrastructure (tokio-test ecosystem)

**Medium Risk**:
- Connection limit handling under load
- Graceful shutdown with active connections
- Error propagation across async boundaries

**Mitigation Strategies**:
- Start with simple connection counting
- Use tokio::signal for shutdown coordination
- Comprehensive error type hierarchy with context

## Dependencies Analysis

**Core Dependencies**:
- tokio = { version = "1.0", features = ["full"] }
- serde = { version = "1.0", features = ["derive"] }
- thiserror = "1.0"
- tracing = "0.1"
- tracing-subscriber = "0.3"
- clap = { version = "4.0", features = ["derive"] }

**Test Dependencies**:
- tokio-test = "0.4"
- criterion = "0.5" (for performance benchmarks)

**Future Dependencies**:
- nusb = "0.2" (when adding USB support)

## Success Criteria Validation

✅ **100 concurrent connections**: Use tokio::sync::Semaphore
✅ **Sub-100ms response**: Async I/O with minimal processing
✅ **Basic tracing**: Message logging with tracing crate
✅ **Protocol v1.0.0**: Version field in message header
✅ **Simple error reporting**: Structured error types with context
✅ **Cross-platform**: Pure Rust with tokio (no platform-specific code)
