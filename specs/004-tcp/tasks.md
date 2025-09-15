# Tasks: TCP Test Client

**Input**: Design documents from `/specs/004-tcp/`
**Prerequisites**: plan.md (required), research.md, data-model.md, contracts/

## Execution Flow (main)
```
1. Load plan.md from feature directory
   → Extract: Rust, tokio, clap, CLI structure
2. Load optional design documents:
   → data-model.md: TestSession, TestResult, TestType entities
   → contracts/: CLI interface + protocol behavior contracts
   → quickstart.md: ping, host-commands, multi-connect scenarios
3. Generate tasks by category:
   → Setup: project structure, binary setup
   → Tests: contract tests for CLI commands and protocol
   → Core: data models, connection logic, CLI commands
   → Integration: protocol compliance, error handling
   → Polish: unit tests, performance validation
4. Apply task rules:
   → Different modules = [P] parallel execution
   → Tests before implementation (TDD)
   → Models before services before CLI
5. SUCCESS (32 tasks ready for execution)
```

## Format: `[ID] [P?] Description`
- **[P]**: Can run in parallel (different files, no dependencies)
- Include exact file paths in descriptions

## Path Conventions
- **Single project**: `src/`, `tests/` at repository root
- Binary location: `src/bin/dbgif-test-client.rs`
- Test client library: `src/test_client/`

## Phase 3.1: Setup
- [ ] T001 Create test client module structure in `src/test_client/` with mod.rs
- [ ] T002 Add CLI binary entry point in `src/bin/dbgif-test-client.rs` with clap integration
- [ ] T003 [P] Update `Cargo.toml` with new binary configuration and ensure clap is available
- [ ] T004 [P] Configure workspace for test client with proper module exports in `src/lib.rs`

## Phase 3.2: Tests First (TDD) ⚠️ MUST COMPLETE BEFORE 3.3
**CRITICAL: These tests MUST be written and MUST FAIL before ANY implementation**
- [ ] T005 [P] Contract test for CLI ping command in `tests/contract/test_cli_ping.rs`
- [ ] T006 [P] Contract test for CLI host-commands in `tests/contract/test_cli_host.rs`
- [ ] T007 [P] Contract test for CLI multi-connect in `tests/contract/test_cli_multi.rs`
- [ ] T008 [P] Protocol behavior test for CNXN handshake in `tests/contract/test_protocol_handshake.rs`
- [ ] T009 [P] Protocol behavior test for message validation in `tests/contract/test_protocol_validation.rs`
- [ ] T010 [P] Integration test for single connection flow in `tests/integration/test_single_connection.rs`
- [ ] T011 [P] Integration test for concurrent connections in `tests/integration/test_concurrent_connections.rs`
- [ ] T012 [P] Integration test for error scenarios in `tests/integration/test_error_handling.rs`

## Phase 3.3: Core Implementation (ONLY after tests are failing)
- [ ] T013 [P] TestSession model in `src/test_client/models/session.rs`
- [ ] T014 [P] TestResult model in `src/test_client/models/result.rs`
- [ ] T015 [P] TestType and TestStatus enums in `src/test_client/models/types.rs`
- [ ] T016 [P] ProtocolExchange model in `src/test_client/models/protocol.rs`
- [ ] T017 Connection manager in `src/test_client/connection.rs` (uses existing protocol module)
- [ ] T018 Test executor logic in `src/test_client/executor.rs`
- [ ] T019 Result reporter in `src/test_client/reporter.rs` (console + JSON output)
- [ ] T020 CLI ping command implementation in `src/test_client/commands/ping.rs`
- [ ] T021 CLI host-commands implementation in `src/test_client/commands/host.rs`
- [ ] T022 CLI multi-connect implementation in `src/test_client/commands/multi.rs`
- [ ] T023 CLI argument parsing and routing in `src/test_client/cli.rs`

## Phase 3.4: Integration
- [ ] T024 Protocol compliance validation using existing `src/protocol/` modules
- [ ] T025 Error handling and timeout management in connection logic
- [ ] T026 Structured logging integration with tracing crate
- [ ] T027 JSON output formatting for automation scripts
- [ ] T028 Performance measurement and reporting (connection times, throughput)

## Phase 3.5: Polish
- [ ] T029 [P] Unit tests for data models in `tests/unit/test_models.rs`
- [ ] T030 [P] Unit tests for result formatting in `tests/unit/test_reporter.rs`
- [ ] T031 [P] Performance validation against baseline (<1s for basic tests)
- [ ] T032 [P] Update quickstart.md with actual usage examples and validate all scenarios

## Dependencies
- Setup (T001-T004) before everything else
- Tests (T005-T012) before implementation (T013-T023)
- Models (T013-T016) before services (T017-T019)
- Core commands (T020-T022) before CLI routing (T023)
- Core implementation before integration (T024-T028)
- Integration before polish (T029-T032)

## Parallel Example: Contract Tests Phase
```bash
# Launch T005-T009 together (different contract files):
Task: "Contract test for CLI ping command in tests/contract/test_cli_ping.rs"
Task: "Contract test for CLI host-commands in tests/contract/test_cli_host.rs"
Task: "Contract test for CLI multi-connect in tests/contract/test_cli_multi.rs"
Task: "Protocol behavior test for CNXN handshake in tests/contract/test_protocol_handshake.rs"
Task: "Protocol behavior test for message validation in tests/contract/test_protocol_validation.rs"
```

## Parallel Example: Data Models Phase
```bash
# Launch T013-T016 together (different model files):
Task: "TestSession model in src/test_client/models/session.rs"
Task: "TestResult model in src/test_client/models/result.rs"
Task: "TestType and TestStatus enums in src/test_client/models/types.rs"
Task: "ProtocolExchange model in src/test_client/models/protocol.rs"
```

## Parallel Example: CLI Commands Phase
```bash
# Launch T020-T022 together (different command files):
Task: "CLI ping command implementation in src/test_client/commands/ping.rs"
Task: "CLI host-commands implementation in src/test_client/commands/host.rs"
Task: "CLI multi-connect implementation in src/test_client/commands/multi.rs"
```

## Notes
- [P] tasks = different files, no shared dependencies
- Verify all tests fail before implementing (RED phase of TDD)
- Commit after each completed task
- Reuse existing `src/protocol/` modules for DBGIF message handling
- Keep implementation simple (개인 프로젝트, 오버엔지니어링 지양)
- Maximum 10 concurrent connections for multi-connect tests

## Task Generation Rules Applied

1. **From CLI Interface Contract**:
   - ping command → T005 (contract test), T020 (implementation)
   - host-commands → T006 (contract test), T021 (implementation)
   - multi-connect → T007 (contract test), T022 (implementation)

2. **From Protocol Behavior Contract**:
   - CNXN handshake → T008 (contract test), T024 (validation)
   - Message validation → T009 (contract test), T024 (validation)

3. **From Data Model**:
   - TestSession → T013 (model)
   - TestResult → T014 (model)
   - TestType/TestStatus → T015 (enums)
   - ProtocolExchange → T016 (model)

4. **From Quickstart Scenarios**:
   - Single connection → T010 (integration test)
   - Concurrent connections → T011 (integration test)
   - Error handling → T012 (integration test)

## Validation Checklist ✓
- [x] All CLI contracts have corresponding tests (T005-T007)
- [x] All protocol contracts have tests (T008-T009)
- [x] All entities have model tasks (T013-T016)
- [x] All tests come before implementation (Phase 3.2 → 3.3)
- [x] Parallel tasks are truly independent (different files)
- [x] Each task specifies exact file path
- [x] No [P] task modifies same file as another [P] task
- [x] TDD order enforced: RED (tests) → GREEN (implementation) → REFACTOR