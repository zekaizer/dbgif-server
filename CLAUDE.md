# DBGIF Server - Claude Code Context

**Project**: Debug Interface (DBGIF) Protocol Server (ADB-like)
**Language**: Rust
**Architecture**: Async TCP server with ADB-like protocol implementation (NOT fully ADB-compatible)

## Current Development

**Active Feature**: 001-dbgif-server
**Status**: Implementation planning updated for ADB-like protocol (Phase 1 complete)
**Target**: ADB-like DBGIF server with host services, test client, and device simulation (NOT fully ADB-compatible)

## Project Structure

```
dbgif-server/
├── src/
│   ├── protocol/          # DBGIF protocol implementation
│   ├── transport/         # Transport abstraction (TCP + future USB)
│   ├── server/            # Main server logic
│   └── bin/
│       ├── dbgif-server.rs
│       ├── dbgif-test-client.rs
│       └── tcp-device-test-server.rs
├── tests/
│   ├── contract/          # Protocol contract tests
│   ├── integration/       # End-to-end tests
│   └── unit/             # Unit tests
└── specs/001-dbgif-server/
    ├── spec.md           # Feature specification
    ├── plan.md           # Implementation plan
    ├── research.md       # Technology research
    ├── data-model.md     # Data structures
    ├── quickstart.md     # User guide
    └── contracts/        # Protocol specifications
```

## Key Technologies

- **Runtime**: tokio (async/await)
- **Protocol**: ADB-like binary protocol (24-byte header + variable data, NOT fully ADB-compatible)
- **Serialization**: Custom message format inspired by ADB with CRC32 validation
- **Transport**: TCP (tokio::net::TcpListener) with USB abstraction
- **Future**: nusb 2.0 for USB support
- **CRC Validation**: crc32fast for data integrity
- **Error Handling**: thiserror
- **Logging**: tracing + tracing-subscriber
- **CLI**: clap with derive features
- **Testing**: cargo test + tokio-test

## Architecture Overview

```
DBGIF Client ↔ DBGIF Server ↔ TCP Transport ↔ Device (DBGIF daemon)
```

**Core Components**:
1. **ADB-like Protocol**: 24-byte header (little-endian) + CRC32 validation (NOT fully ADB-compatible)
2. **Host Services**: Built-in server services (list, device, version, features)
3. **Device Management**: Lazy connection with discovery and registration
4. **Stream Forwarding**: Multi-client stream multiplexing
5. **Transport Layer**: TCP implementation with USB abstraction for future

## Protocol Specification (ADB-like, NOT fully compatible)

**Message Format** (exactly 24 bytes header):
```
struct AdbMessage {
    command: u32,     // Command code (little-endian)
    arg0: u32,        // First argument (little-endian)
    arg1: u32,        // Second argument (little-endian)
    data_length: u32, // Data payload length (little-endian)
    data_crc32: u32,  // CRC32 of data payload (little-endian)
    magic: u32,       // Magic number (!command, bitwise NOT)
    data: Vec<u8>,    // Variable length data payload
}
```

**Key Commands**:
- CNXN (0x4E584E43): Connection establishment handshake
- OPEN (0x4E45504F): Stream opening with service name
- OKAY (0x59414B4F): Success acknowledgment
- WRTE (0x45545257): Data transfer with stream IDs
- CLSE (0x45534C43): Stream closure
- PING/PONG (0x474E4950/0x474E4F50): Connection health monitoring

**Host Services**:
- host:list - Device discovery and listing
- host:device:<id> - Device selection
- host:version - Server version information
- host:features - Server capabilities

**Performance Targets**:
- 100 concurrent client connections
- Stream multiplexing with independent ID spaces
- <100ms response latency
- ADB-like protocol (custom implementation, NOT fully ADB-compatible)

## Development Principles

**Constitution Compliance**:
- ✓ Library-first architecture (protocol, transport, server libs)
- ✓ CLI interfaces for each component
- ✓ TDD with contract → integration → unit testing
- ✓ Simple, direct implementations (no unnecessary patterns)
- ✓ Structured logging and error handling

**Testing Strategy**:
1. Contract tests for protocol messages
2. Integration tests with real TCP connections
3. End-to-end tests with full client-server-device chain
4. Performance tests for 100 concurrent connections

## Current State

**Completed (Phase 1)**:
- ✓ Feature specification with clear requirements
- ✓ Technology research and decisions
- ✓ Data model design
- ✓ Protocol contract specification
- ✓ Quickstart guide and testing approach

**Next Steps (Phase 2)**:
- Generate detailed implementation tasks
- Create failing contract tests
- Implement core protocol library
- Build transport abstraction
- Develop server application

## Key Files to Know

- `specs/001-dbgif-server/spec.md`: Feature requirements
- `specs/001-dbgif-server/data-model.md`: Core data structures
- `specs/001-dbgif-server/contracts/protocol-spec.md`: Protocol definition
- `specs/001-dbgif-server/quickstart.md`: Testing and usage guide

## Testing Commands

```bash
# Build all components
cargo build --release

# Run contract tests
cargo test --test protocol_contracts

# Run integration tests
cargo test --test integration_tests

# Start test environment
./target/release/tcp-device-test-server --port 5557 &
./target/release/dbgif-server --port 5555 --device-port 5557 &
./target/release/dbgif-test-client --server 127.0.0.1:5555 --test basic
```

## Recent Changes

**2025-09-16**: Updated for ADB-like protocol (NOT fully ADB-compatible)
- Updated feature specification to use existing ADB-like protocol from docs/protocol.md
- Completed technology research (Rust + tokio + ADB-like protocol + CRC32)
- Redesigned data model for ADB-like message format and stream management
- Specified ADB-like protocol with 24-byte header + CRC32 validation (custom implementation)
- Updated test architecture for DBGIF client + server + device daemon simulation
- Added host services for device discovery and management (not in standard ADB)

## Notes

- Start simple with TCP, abstract for future USB support
- Focus on basic tracing operations initially
- Emphasize clean separation between protocol, transport, and server logic
- DBGIF protocol version 1.0.0 (ADB-like but NOT fully compatible)
- Use structured logging for debugging and observability