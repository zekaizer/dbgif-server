# Tasks: DBGIF Server Development

**Input**: Design documents from `/specs/001-dbgif-server/`
**Prerequisites**: plan.md (required), research.md, data-model.md, contracts/

## Execution Flow (main)
```
1. Load plan.md from feature directory
   → If not found: ERROR "No implementation plan found"
   → Extract: tech stack, libraries, structure
2. Load optional design documents:
   → data-model.md: Extract entities → model tasks
   → contracts/: Each file → contract test task
   → research.md: Extract decisions → setup tasks
3. Generate tasks by category:
   → Setup: project init, dependencies, linting
   → Tests: contract tests, integration tests
   → Core: models, services, CLI commands
   → Integration: DB, middleware, logging
   → Polish: unit tests, performance, docs
4. Apply task rules:
   → Different files = mark [P] for parallel
   → Same file = sequential (no [P])
   → Tests before implementation (TDD)
5. Number tasks sequentially (T001, T002...)
6. Generate dependency graph
7. Create parallel execution examples
8. Validate task completeness:
   → All contracts have tests?
   → All entities have models?
   → All endpoints implemented?
9. Return: SUCCESS (tasks ready for execution)
```

## Format: `[ID] [P?] Description`
- **[P]**: Can run in parallel (different files, no dependencies)
- Include exact file paths in descriptions

## Path Conventions
- **Single project**: `src/`, `tests/` at repository root
- Paths shown below follow Rust workspace structure

## Phase 3.1: Setup
- [x] T001 Create Rust workspace structure with Cargo.toml and crate organization
- [x] T002 Initialize dependencies: tokio, crc32fast, thiserror, tracing, clap, serde
- [x] T003 [P] Configure clippy and rustfmt in .cargo/config.toml
- ~~[ ] T004 [P] Setup CI workflow in .github/workflows/rust.yml~~

## Phase 3.2: Tests First (TDD) ⚠️ MUST COMPLETE BEFORE 3.3
**CRITICAL: These tests MUST be written and MUST FAIL before ANY implementation**
- [x] T005 [P] ADB message serialization contract test in tests/contract/test_adb_message.rs
- [x] T006 [P] CRC32 validation contract test in tests/contract/test_crc32.rs
- [x] T007 [P] Command enum contract test in tests/contract/test_commands.rs
- [x] T008 [P] Little-endian encoding contract test in tests/contract/test_endianness.rs
- [x] T009 [P] TCP transport contract test in tests/contract/test_tcp_transport.rs
- [x] T010 [P] Host service registry contract test in tests/contract/test_host_services.rs
- [x] T011 [P] Integration test: CNXN handshake in tests/integration/test_connection.rs
- [x] T012 [P] Integration test: host services in tests/integration/test_host_services.rs
- [x] T013 [P] Integration test: stream lifecycle in tests/integration/test_streams.rs
- [x] T014 [P] Integration test: device forwarding in tests/integration/test_device_forwarding.rs
- [x] T015 [P] Integration test: multi-client access in tests/integration/test_multi_client.rs
- [x] T016 [P] Performance test: 100 connections in tests/integration/test_performance.rs

## Phase 3.3: Core Implementation (ONLY after tests are failing)
- [x] T017 [P] AdbMessage struct with serialization in src/protocol/message.rs
- [x] T018 [P] AdbCommand enum with constants in src/protocol/commands.rs
- [x] T019 [P] CRC32 validation implementation in src/protocol/crc.rs
- [x] T020 [P] ProtocolError types in src/protocol/error.rs
- [x] T021 [P] Transport trait definition in src/transport/mod.rs
- [x] T022 [P] Connection trait definition in src/transport/connection.rs
- [x] T023 [P] TcpTransport implementation in src/transport/tcp.rs
- [x] T024 [P] TcpConnection implementation in src/transport/tcp_connection.rs
- [ ] T025 [P] DeviceRegistry struct in src/server/device_registry.rs
- [ ] T026 [P] ClientSession management in src/server/session.rs
- [ ] T027 [P] StreamInfo and mapping in src/server/stream.rs
- [ ] T028 [P] HostService trait in src/host_services/mod.rs
- [ ] T029 [P] HostListService implementation in src/host_services/list.rs
- [ ] T030 [P] HostDeviceService implementation in src/host_services/device.rs
- [ ] T031 [P] HostVersionService implementation in src/host_services/version.rs
- [ ] T032 [P] HostFeaturesService implementation in src/host_services/features.rs
- [ ] T033 ServerState and configuration in src/server/state.rs
- [ ] T034 Command dispatch and message routing in src/server/dispatcher.rs
- [ ] T035 Connection lifecycle management in src/server/connection_manager.rs
- [ ] T036 Stream forwarding logic in src/server/stream_forwarder.rs
- [ ] T037 Device discovery and lazy connection in src/server/device_manager.rs

## Phase 3.4: Applications
- [ ] T038 DBGIF server binary with CLI in src/bin/dbgif-server.rs
- [ ] T039 Test client binary with CLI in src/bin/dbgif-test-client.rs
- [ ] T040 TCP device test server in src/bin/tcp-device-test-server.rs
- [ ] T041 Configuration parsing and validation in src/config.rs
- [ ] T042 Logging and tracing setup in src/logging.rs
- [ ] T043 Error handling and graceful shutdown in src/server/shutdown.rs

## Phase 3.5: Integration
- [ ] T044 Wire up host services to server in src/server/mod.rs
- [ ] T045 Implement protocol message handling in src/server/message_handler.rs
- [ ] T046 Add connection limits and backpressure in src/server/limits.rs
- [ ] T047 Implement PING/PONG heartbeat in src/server/heartbeat.rs
- [ ] T048 Device discovery background task in src/server/discovery.rs

## Phase 3.6: Polish
- [ ] T049 [P] Unit tests for serialization in tests/unit/test_serialization.rs
- [ ] T050 [P] Unit tests for host services in tests/unit/test_host_services.rs
- [ ] T051 [P] Unit tests for stream management in tests/unit/test_streams.rs
- [ ] T052 [P] Unit tests for device registry in tests/unit/test_device_registry.rs
- [ ] T053 [P] Benchmark message throughput in benches/message_throughput.rs
- [ ] T054 [P] Benchmark concurrent connections in benches/connection_performance.rs
- [ ] T055 [P] Update project README.md with usage examples
- [ ] T056 [P] Generate API documentation with cargo doc
- [ ] T057 Run quickstart.md test scenarios end-to-end
- [ ] T058 Code cleanup and remove duplication

## Dependencies
**Critical TDD Dependencies**:
- Tests (T005-T016) MUST complete and FAIL before implementation (T017-T048)
- Contract tests must validate exact ADB protocol format

**Implementation Dependencies**:
- T017-T020 (protocol) blocks T021-T024 (transport)
- T021-T024 (transport) blocks T025-T032 (server core)
- T025-T032 (server core) blocks T033-T037 (server logic)
- T017-T037 (core) blocks T038-T043 (applications)
- T038-T043 (applications) blocks T044-T048 (integration)
- T044-T048 (integration) blocks T049-T058 (polish)

**Specific Blocks**:
- T025 (DeviceRegistry) blocks T037 (DeviceManager)
- T026 (ClientSession) blocks T035 (ConnectionManager)
- T027 (StreamInfo) blocks T036 (StreamForwarder)
- T028 (HostService trait) blocks T029-T032 (service implementations)

## Parallel Example
```bash
# Launch contract tests together (T005-T010):
Task: "ADB message serialization contract test in tests/contract/test_adb_message.rs"
Task: "CRC32 validation contract test in tests/contract/test_crc32.rs"
Task: "Command enum contract test in tests/contract/test_commands.rs"
Task: "Little-endian encoding contract test in tests/contract/test_endianness.rs"
Task: "TCP transport contract test in tests/contract/test_tcp_transport.rs"
Task: "Host service registry contract test in tests/contract/test_host_services.rs"

# Launch core protocol implementation together (T017-T020):
Task: "AdbMessage struct with serialization in src/protocol/message.rs"
Task: "AdbCommand enum with constants in src/protocol/commands.rs"
Task: "CRC32 validation implementation in src/protocol/crc.rs"
Task: "ProtocolError types in src/protocol/error.rs"
```

## Test Scenarios from Quickstart
1. **Connection Test**: CNXN handshake with protocol version negotiation
2. **Host Services Test**: host:version, host:features, host:list, host:device
3. **Device Communication Test**: device selection and command forwarding
4. **Stream Multiplexing Test**: multiple concurrent streams per client
5. **Multi-Client Test**: multiple clients accessing same device
6. **Performance Test**: 100 concurrent connections with sub-100ms latency
7. **Protocol Compliance Test**: ADB format validation and error handling

## Notes
- [P] tasks = different files, can run in parallel
- ADB-like protocol: NOT fully ADB-compatible (custom implementation)
- Stream IDs: independent per client session (1-65535)
- Magic number: always !command (bitwise NOT)
- CRC32: computed only over data payload
- Little-endian: all multi-byte fields in headers
- Host services: custom "host:" prefix (not in standard ADB)
- Lazy connections: devices connected only when selected

## Task Generation Rules Applied

1. **From Protocol Contracts**:
   - ADB message format → T005 (serialization test)
   - CRC32 validation → T006 (CRC test)
   - Command constants → T007 (enum test)
   - Byte order → T008 (endianness test)

2. **From Data Model Entities**:
   - AdbMessage → T017 (message struct)
   - ClientSession → T026 (session management)
   - DeviceRegistry → T025 (device registry)
   - StreamInfo → T027 (stream mapping)
   - HostService → T028-T032 (service implementations)

3. **From Host Services**:
   - host:list → T029 (list service)
   - host:device → T030 (device service)
   - host:version → T031 (version service)
   - host:features → T032 (features service)

4. **From Test Scenarios**:
   - Connection handshake → T011 (CNXN test)
   - Host services → T012 (services test)
   - Stream lifecycle → T013 (streams test)
   - Device forwarding → T014 (forwarding test)
   - Multi-client → T015 (multi-client test)
   - Performance → T016 (performance test)

## Validation Checklist ✓
- [x] All protocol contracts have corresponding tests (T005-T010)
- [x] All data model entities have implementation tasks (T017-T032)
- [x] All tests come before implementation (TDD enforced)
- [x] Parallel tasks are truly independent (different files)
- [x] Each task specifies exact file path
- [x] No task modifies same file as another [P] task
- [x] Dependencies properly mapped
- [x] Test scenarios from quickstart covered