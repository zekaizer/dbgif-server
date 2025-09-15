# Phase 0: Research & Technical Decisions

## Generalized Transport Abstraction

**Decision**: Design pluggable transport system supporting TCP, USB Device, and USB Bridge equally
**Rationale**:
- Extensible architecture allows adding new transport types
- No USB Bridge-specific coupling in core daemon logic
- Each transport type can optimize for its specific characteristics
- Unified ADB protocol layer works across all transports

**Transport Types Supported**:
1. **TCP Transport**: Network-based ADB connections (port 5555)
2. **USB Device Transport**: Direct Android USB device connections (Android VID/PIDs)
3. **USB Bridge Transport**: PL25A1 and similar USB bridge cables

## USB Communication Strategy

**Decision**: Use nusb crate for all USB-based transports (Device + Bridge)
**Rationale**:
- Pure Rust implementation provides cross-platform Windows/Linux support
- No external dependencies on libusb C library
- Better async/await integration with tokio
- Type-safe USB descriptors and endpoints
- Single USB stack for both direct devices and bridge cables

**Alternatives considered**:
- rusb: Requires libusb C dependency, complicates cross-platform deployment
- Separate USB libraries per transport: Increases complexity and binary size
- Platform-specific USB APIs: Would break cross-platform compatibility

## ADB Protocol Implementation

**Decision**: Implement minimal ADB protocol subset focused on CNXN, AUTH, OPEN, WRTE, CLSE, OKAY messages
**Rationale**:
- Sufficient for basic debugging and data transfer use cases
- Simplified authentication without RSA verification meets personal project requirements
- Message-based design allows extension for additional ADB features

**Alternatives considered**:
- Full ADB protocol: Overkill for personal toy project, RSA auth unnecessary
- Custom protocol: Would lose compatibility with existing ADB tools

## Transport Abstraction Design

**Decision**: Create async trait-based transport abstraction with factory pattern for transport discovery
**Rationale**:
- Unified interface across TCP, USB Device, and USB Bridge transports
- Factory pattern enables automatic device discovery and transport selection
- Separates protocol logic from transport-specific implementation
- Enables testing with mock transports and easy extension

**Transport trait interface**:
```rust
#[async_trait]
pub trait Transport {
    async fn connect(&mut self) -> Result<()>;
    async fn send(&mut self, data: &[u8]) -> Result<()>;
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<usize>;
    async fn disconnect(&mut self) -> Result<()>;
    fn is_connected(&self) -> bool;
    fn transport_type(&self) -> TransportType;
    fn device_id(&self) -> String;
}

pub enum TransportType {
    Tcp,
    UsbDevice,
    UsbBridge,
}
```

**Transport Factory**:
```rust
#[async_trait]
pub trait TransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>>;
    async fn create_transport(&self, device: &DeviceInfo) -> Result<Box<dyn Transport>>;
    fn transport_type(&self) -> TransportType;
}
```

## Cross-Platform USB Driver Strategy

**Decision**: Use nusb's built-in Windows/Linux support with conditional compilation for platform-specific optimizations
**Rationale**:
- nusb handles platform differences internally
- WinUSB on Windows, usbfs on Linux automatically selected
- Minimal platform-specific code required

**Platform considerations**:
- Windows: Requires WinUSB driver for PL25A1 (may need manual installation)
- Linux: Uses kernel usbfs, works out-of-box with appropriate permissions

## Performance Optimization Approach

**Decision**: Implement zero-copy buffer management using bytes crate with pre-allocated buffer pools
**Rationale**:
- Minimizes memory allocations in hot path
- USB 2.0 bulk transfers can achieve near-theoretical 480 Mbps
- Reduce latency for real-time debugging scenarios

**Key optimizations**:
- Buffer pooling for large transfers (256KB chunks)
- Separate header/payload transfers as required by nusb
- Async streaming for concurrent read/write operations

## Error Handling Strategy

**Decision**: Fail-fast approach using anyhow for error context with structured error types
**Rationale**:
- Matches user requirement for no automatic retry
- Simplified debugging with rich error context
- Clear error propagation through async call stack

**Error categories**:
- Transport errors: USB communication failures
- Protocol errors: Malformed ADB messages
- Connection errors: Bridge cable disconnection
- Validation errors: Checksum mismatches

## Testing Strategy

**Decision**: Multi-tier testing with hardware-in-loop integration tests
**Rationale**:
- Contract tests ensure transport trait compliance
- Integration tests validate actual PL25A1 hardware communication
- Unit tests cover protocol message parsing/serialization

**Test structure**:
- Contract tests: Mock transport implementations
- Integration tests: Real PL25A1 hardware (CI environment dependent)
- Unit tests: Protocol message handling, checksums
- End-to-end tests: Full ADB session scenarios

## Concurrency Model

**Decision**: Use tokio's async runtime with separate tasks for read/write operations
**Rationale**:
- Full-duplex communication over single USB bridge
- Stream multiplexing requires concurrent message handling
- Graceful shutdown on disconnect events

**Task architecture**:
- Connection manager: Monitor USB bridge status
- Read task: Process incoming messages from bridge
- Write task: Send outgoing messages to bridge
- Client handlers: Manage individual ADB client connections

## State Management

**Decision**: Minimal state tracking using atomic counters and async channels
**Rationale**:
- Stateless design preferred for personal project simplicity
- Stream IDs managed with atomic counters
- Connection state communicated via channels

**State components**:
- Connection status: Connected/Disconnected atomic bool
- Stream ID allocation: Atomic counter for local stream IDs
- Message routing: HashMap of active streams
- Client sessions: Vec of active client connections