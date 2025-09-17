# DBGIF Server - Claude Code Context

**Project**: Debug Interface (DBGIF) Protocol Server (ADB-like, NOT fully compatible)
**Language**: Rust | **Architecture**: Async TCP server

## Critical Development Guidelines

### ⚠️ Rust Borrowing System Requirements
**CRITICAL**: All Rust implementations MUST properly handle:
- **Ownership**: Clear ownership patterns, avoid unnecessary cloning
- **Borrowing**: Use references (`&T`, `&mut T`) appropriately
- **Lifetimes**: Explicit lifetimes when required, especially in structs/traits
- **Arc/Mutex**: Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for shared state in async contexts
- **Send + Sync**: Ensure types crossing thread boundaries implement required traits
- **Error Handling**: Use `Result<T, E>` consistently, avoid `.unwrap()` in production code

## Project Structure
```
workspace-root/
├── dbgif-protocol/        # Protocol implementation (messages, commands, CRC)
├── dbgif-transport/       # Transport abstraction (TCP now, USB future)
├── dbgif-server/          # Server with host_services & main logic
├── dbgif-integrated-test/ # Test client & device simulator
└── specs/001-dbgif-server/# Specifications & documentation
```

## Tech Stack & Protocol

**Core**: tokio async runtime | **Protocol**: ADB-like 24-byte header + CRC32
**Libraries**: crc32fast, thiserror, tracing, clap
**Message Format**:
```rust
struct AdbMessage {
    command: u32,     // LE: CNXN, OPEN, OKAY, WRTE, CLSE, PING/PONG
    arg0: u32, arg1: u32, data_length: u32, data_crc32: u32,
    magic: u32,       // !command validation
    data: Vec<u8>,
}
```

**Host Services**: `host:list`, `host:device:<id>`, `host:version`, `host:features`
**Targets**: 100 concurrent clients, <100ms latency, stream multiplexing

## Development Status

**Phase 1 Complete**: Specs, research, data model, protocol contracts, quickstart
**Phase 2 Active**: Implementation with workspace modules

## Testing & Commands
```bash
cargo build --release && cargo test --workspace
./target/release/dbgif-server --port 5555
./target/release/dbgif-integrated-test --server 127.0.0.1:5555 --test all
```

## Key Files
- `specs/001-dbgif-server/spec.md`: Requirements
- `specs/001-dbgif-server/data-model.md`: Data structures
- `specs/001-dbgif-server/contracts/protocol-spec.md`: Protocol
- `specs/001-dbgif-server/quickstart.md`: Usage guide

## Recent Updates (2025-09-17)
- ✓ Workspace restructuring with separate crates
- ✓ ADB-like protocol implementation (custom, not fully compatible)
- ✓ Host services for device management
- ✓ Test compilation fixes

## Architecture Principles
- Library-first design with CLI interfaces
- TDD: contract → integration → unit tests
- Simple, direct implementations
- Clean protocol/transport/server separation
- Structured logging throughout

**Note**: Protocol v1.0.0 - ADB-inspired but NOT fully ADB-compatible