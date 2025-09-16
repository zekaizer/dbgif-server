---
description: "Implementation plan template for feature development"
scripts:
  sh: .specify/scripts/bash/update-agent-context.sh claude
  ps: .specify/scripts/powershell/update-agent-context.ps1 -AgentType claude
---

# Implementation Plan: DBGIF Server Development

**Branch**: `001-dbgif-server` | **Date**: 2025-09-16 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/home/zekaizer/Workspace/dbgif-server/specs/001-dbgif-server/spec.md`

## Execution Flow (/plan command scope)
```
1. Load feature spec from Input path
   → If not found: ERROR "No feature spec at {path}"
2. Fill Technical Context (scan for NEEDS CLARIFICATION)
   → Detect Project Type from context (web=frontend+backend, mobile=app+api)
   → Set Structure Decision based on project type
3. Evaluate Constitution Check section below
   → If violations exist: Document in Complexity Tracking
   → If no justification possible: ERROR "Simplify approach first"
   → Update Progress Tracking: Initial Constitution Check
4. Execute Phase 0 → research.md
   → If NEEDS CLARIFICATION remain: ERROR "Resolve unknowns"
5. Execute Phase 1 → contracts, data-model.md, quickstart.md, agent-specific template file (e.g., `CLAUDE.md` for Claude Code, `.github/copilot-instructions.md` for GitHub Copilot, or `GEMINI.md` for Gemini CLI).
6. Re-evaluate Constitution Check section
   → If new violations: Refactor design, return to Phase 1
   → Update Progress Tracking: Post-Design Constitution Check
7. Plan Phase 2 → Describe task generation approach (DO NOT create tasks.md)
8. STOP - Ready for /tasks command
```

**IMPORTANT**: The /plan command STOPS at step 7. Phases 2-4 are executed by other commands:
- Phase 2: /tasks command creates tasks.md
- Phase 3-4: Implementation execution (manual or via tools)

## Summary
Implement a DBGIF (Debug Interface) protocol server using ADB-style message format with essential functionality for device debugging and management. The server will handle up to 100 concurrent client connections, provide host services for device discovery and selection, and forward client requests to selected devices through lazy connection establishment. TCP transport implementation first, with USB abstraction for future expansion.

## Technical Context
**Language/Version**: Rust (latest stable)
**Primary Dependencies**: tokio (async runtime), crc32fast (CRC validation), nusb 2.0 (USB support for future)
**Storage**: In-memory device registry and session management (stateless protocol)
**Testing**: cargo test (unit tests), integration tests for ADB protocol compliance
**Target Platform**: Linux/macOS/Windows (cross-platform)
**Project Type**: single (standalone server application)
**Performance Goals**: Handle 100 concurrent connections, stream multiplexing, sub-100ms response time
**Constraints**: ADB protocol compliance, TCP-only transport initially (USB abstracted for future)
**Scale/Scope**: Personal project, ADB-compatible debugging protocol server
**Architecture**: client ↔ dbgif server ↔ device (with host services and lazy device connections)

## Constitution Check
*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

**Simplicity**:
- Projects: 3 (dbgif-server, test-client, tcp-device-test-server)
- Using framework directly? Yes (tokio directly, no wrappers)
- Single data model? Yes (DBGIF protocol messages)
- Avoiding patterns? Yes (direct implementation, no Repository pattern)

**Architecture**:
- EVERY feature as library? Yes (protocol, transport, server, host services libs)
- Libraries listed:
  - dbgif-protocol (ADB message format, serialization/deserialization, CRC validation)
  - dbgif-transport (TCP transport with USB abstraction for future)
  - dbgif-server (server core, session management, stream forwarding)
  - dbgif-host-services (host service implementations: list, device, version, features)
- CLI per library: Yes (--help/--version/--format for each binary)
- Library docs: Yes, llms.txt format planned

**Testing (NON-NEGOTIABLE)**:
- RED-GREEN-Refactor cycle enforced? Yes
- Git commits show tests before implementation? Yes
- Order: Contract→Integration→E2E→Unit strictly followed? Yes
- Real dependencies used? Yes (actual TCP connections)
- Integration tests for: Yes (TCP transport, protocol contracts)
- FORBIDDEN: Implementation before test, skipping RED phase? Enforced

**Observability**:
- Structured logging included? Yes (using tracing crate)
- Frontend logs → backend? N/A (server-only)
- Error context sufficient? Yes (structured error types)

**Versioning**:
- Version number assigned? Yes (DBGIF protocol 1.0.0)
- BUILD increments on every change? Yes
- Breaking changes handled? Yes (protocol versioning plan)

## Project Structure

### Documentation (this feature)
```
specs/[###-feature]/
├── plan.md              # This file (/plan command output)
├── research.md          # Phase 0 output (/plan command)
├── data-model.md        # Phase 1 output (/plan command)
├── quickstart.md        # Phase 1 output (/plan command)
├── contracts/           # Phase 1 output (/plan command)
└── tasks.md             # Phase 2 output (/tasks command - NOT created by /plan)
```

### Source Code (repository root)
```
# Option 1: Single project (DEFAULT)
src/
├── models/
├── services/
├── cli/
└── lib/

tests/
├── contract/
├── integration/
└── unit/

# Option 2: Web application (when "frontend" + "backend" detected)
backend/
├── src/
│   ├── models/
│   ├── services/
│   └── api/
└── tests/

frontend/
├── src/
│   ├── components/
│   ├── pages/
│   └── services/
└── tests/

# Option 3: Mobile + API (when "iOS/Android" detected)
api/
└── [same as backend above]

ios/ or android/
└── [platform-specific structure]
```

**Structure Decision**: [DEFAULT to Option 1 unless Technical Context indicates web/mobile app]

## Phase 0: Outline & Research
1. **Extract unknowns from Technical Context** above:
   - For each NEEDS CLARIFICATION → research task
   - For each dependency → best practices task
   - For each integration → patterns task

2. **Generate and dispatch research agents**:
   ```
   For each unknown in Technical Context:
     Task: "Research {unknown} for {feature context}"
   For each technology choice:
     Task: "Find best practices for {tech} in {domain}"
   ```

3. **Consolidate findings** in `research.md` using format:
   - Decision: [what was chosen]
   - Rationale: [why chosen]
   - Alternatives considered: [what else evaluated]

**Output**: research.md with all NEEDS CLARIFICATION resolved

## Phase 1: Design & Contracts
*Prerequisites: research.md complete*

1. **Extract entities from feature spec** → `data-model.md`:
   - Entity name, fields, relationships
   - Validation rules from requirements
   - State transitions if applicable

2. **Generate API contracts** from functional requirements:
   - For each user action → endpoint
   - Use standard REST/GraphQL patterns
   - Output OpenAPI/GraphQL schema to `/contracts/`

3. **Generate contract tests** from contracts:
   - One test file per endpoint
   - Assert request/response schemas
   - Tests must fail (no implementation yet)

4. **Extract test scenarios** from user stories:
   - Each story → integration test scenario
   - Quickstart test = story validation steps

5. **Update agent file incrementally** (O(1) operation):
   - Run `.specify/scripts/bash/update-agent-context.sh claude` for your AI assistant
   - If exists: Add only NEW tech from current plan
   - Preserve manual additions between markers
   - Update recent changes (keep last 3)
   - Keep under 150 lines for token efficiency
   - Output to repository root

**Output**: data-model.md, /contracts/*, failing tests, quickstart.md, agent-specific file

## Phase 2: Task Planning Approach
*This section describes what the /tasks command will do - DO NOT execute during /plan*

**Task Generation Strategy**:
- Load `.specify/templates/tasks-template.md` as base
- Generate tasks from Phase 1 design docs (contracts, data model, quickstart)
- ADB message format contracts → serialization/deserialization test tasks [P]
- Data model entities → struct definition and validation test tasks [P]
- Transport abstraction → trait definition and TCP implementation tasks
- Host services → service implementation and registration tasks
- Server core → session management, stream forwarding, and device management tasks
- Binary applications → CLI implementation tasks
- Integration scenarios → ADB protocol compliance test tasks

**Specific Task Categories**:

1. **Contract Tests** (TDD Phase - Must Fail First):
   - ADB message serialization/deserialization tests [P]
   - CRC32 validation tests [P]
   - Command enum and magic number tests [P]
   - Little-endian byte order tests [P]
   - Transport layer contract tests [P]

2. **Core Implementation** (Dependency Order):
   - ADB message structs and command enums
   - Message serialization with CRC32 validation
   - Transport trait and TCP implementation
   - Client session and device registry management
   - Stream mapping and forwarding logic
   - Host service trait and implementations

3. **Applications** (After Core):
   - DBGIF server binary with CLI and host services
   - ADB-compatible test client with command suite
   - TCP device test server simulating ADB device daemon

4. **Integration Tests** (After Applications):
   - ADB protocol handshake test (CNXN exchange)
   - Host service tests (list, device, version, features)
   - Stream lifecycle test (OPEN → OKAY → WRTE → CLSE)
   - Device selection and forwarding test
   - Multi-client concurrent access test
   - Performance test (100 connections with stream multiplexing)

**Ordering Strategy**:
- Strict TDD: Contract tests → failing tests → implementation
- Dependency order: Protocol → Transport → Server → Applications
- Mark [P] for parallel execution (independent contract tests)
- Each entity from data-model.md → struct + test task pair

**Testing Verification Strategy**:
- Each contract test MUST fail before implementation
- ADB protocol compliance validation (exact message format)
- Integration tests use real TCP connections (no mocks)
- Performance tests validate 100 concurrent connections with stream multiplexing
- All quickstart scenarios must pass

**Estimated Output**: 40-45 numbered, ordered tasks in tasks.md
- 12 contract test tasks (parallel) - ADB message format compliance
- 18 core implementation tasks (sequential) - protocol, transport, server, host services
- 9 application tasks (after core) - server, client, device simulator
- 8 integration test tasks (after applications) - ADB protocol flows

**Key Dependencies**:
- ADB protocol tests → Protocol implementation
- Transport tests → Transport implementation
- Host service tests → Host service implementation
- Server tests → Server implementation
- All contract tests can run in parallel
- Integration tests require all applications built
- Device simulation requires ADB protocol implementation

**IMPORTANT**: This phase is executed by the /tasks command, NOT by /plan

## Phase 3+: Future Implementation
*These phases are beyond the scope of the /plan command*

**Phase 3**: Task execution (/tasks command creates tasks.md)  
**Phase 4**: Implementation (execute tasks.md following constitutional principles)  
**Phase 5**: Validation (run tests, execute quickstart.md, performance validation)

## Complexity Tracking
*Fill ONLY if Constitution Check has violations that must be justified*

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| [e.g., 4th project] | [current need] | [why 3 projects insufficient] |
| [e.g., Repository pattern] | [specific problem] | [why direct DB access insufficient] |


## Progress Tracking
*This checklist is updated during execution flow*

**Phase Status**:
- [x] Phase 0: Research complete (/plan command)
- [x] Phase 1: Design complete (/plan command)
- [x] Phase 2: Task planning complete (/plan command - describe approach only)
- [ ] Phase 3: Tasks generated (/tasks command)
- [ ] Phase 4: Implementation complete
- [ ] Phase 5: Validation passed

**Gate Status**:
- [x] Initial Constitution Check: PASS
- [x] Post-Design Constitution Check: PASS
- [x] All NEEDS CLARIFICATION resolved
- [x] Complexity deviations documented (none)

---
*Based on Constitution v2.1.1 - See `/memory/constitution.md`*