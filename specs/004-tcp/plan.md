# Implementation Plan: TCP Test Client

**Branch**: `004-tcp` | **Date**: 2025-09-16 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/004-tcp/spec.md`

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
TCP 테스트 클라이언트를 개발하여 DBGIF 서버의 기본 동작을 검증한다. 클라이언트는 포트 5037로 연결하여 CNXN 핸드셰이크, 기본 host 명령어 실행, 다중 연결 테스트를 수행하고 결과를 콘솔에 출력한다. 추가로 서버에 echo service transport를 구현하여 에이징 테스트(장시간 연결 유지, 반복 데이터 송수신)를 지원한다. 개인 프로젝트이므로 단순하고 실용적인 접근을 취한다.

## Technical Context
**Language/Version**: Rust 1.75+ (edition 2021)
**Primary Dependencies**: tokio (async runtime), bytes, clap (CLI)
**Storage**: N/A (stateless test client)
**Testing**: cargo test
**Target Platform**: Linux/Windows (cross-platform)
**Project Type**: single (CLI application)
**Performance Goals**: 연결 테스트 <1초, 동시 연결 10개 이하, 에이징 테스트 (최대 1시간)
**Constraints**: 메모리 사용량 최소화, 외부 의존성 최소화 (개인 프로젝트)
**Scale/Scope**: 단일 바이너리, 기본 프로토콜 검증 + echo service transport (오버엔지니어링 지양)
**Echo Service**: TCP echo transport로 패킷 송수신 에이징 테스트 지원

## Constitution Check
*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

**Simplicity**:
- Projects: 1 (CLI test client only)
- Using framework directly? YES (tokio, clap directly)
- Single data model? YES (간단한 메시지/결과 구조체)
- Avoiding patterns? YES (no Repository - 직접 TCP 통신)

**Architecture**:
- EVERY feature as library? PARTIAL (test-client lib + CLI wrapper + echo transport)
- Libraries listed: test-client (TCP 연결 및 프로토콜 검증), echo-transport (에이징 테스트용)
- CLI per library: dbgif-test-client --help/--version/--format/--aging
- Library docs: 간단한 README.md (개인프로젝트)

**Testing (NON-NEGOTIABLE)**:
- RED-GREEN-Refactor cycle enforced? YES
- Git commits show tests before implementation? YES
- Order: Contract→Integration→E2E→Unit strictly followed? YES
- Real dependencies used? YES (실제 TCP 연결)
- Integration tests for: TCP 프로토콜 통신, 서버 응답 검증, echo service 에이징 테스트
- FORBIDDEN: Implementation before test, skipping RED phase

**Observability**:
- Structured logging included? YES (tracing)
- Frontend logs → backend? N/A
- Error context sufficient? YES

**Versioning**:
- Version number assigned? 0.1.0 (기존 프로젝트 버전)
- BUILD increments on every change? YES
- Breaking changes handled? N/A (새 기능)

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

**Structure Decision**: Option 1 (Single project) - CLI 테스트 도구로 단순 구조 사용

## Echo Service Transport Design

### Purpose
- **Aging Tests**: 장시간 연결 유지 및 반복 데이터 송수신으로 메모리 누수, 연결 안정성 검증
- **Load Testing**: 다양한 패킷 크기와 빈도로 서버 부하 테스트
- **Protocol Validation**: 실제 데이터 교환을 통한 DBGIF 프로토콜 검증

### Implementation Strategy
1. **Server Side**:
   - `src/transport/echo_transport.rs` - echo 전용 transport 구현
   - TCP 포트 5038 (5037과 분리하여 기본 서버 기능과 독립적 운영)
   - 받은 데이터를 그대로 송신자에게 반환하는 단순 echo 서비스

2. **Client Side**:
   - `dbgif-test-client aging` 명령어 추가
   - 설정 가능한 옵션: 패킷 크기, 전송 간격, 테스트 지속 시간
   - 실시간 통계: 송수신 패킷 수, 응답 시간, 에러율

### Configuration Options
```bash
dbgif-test-client aging [OPTIONS]
  --duration <SECONDS>     테스트 지속 시간 (기본: 60초, 최대: 3600초)
  --packet-size <BYTES>    패킷 크기 (기본: 1024, 범위: 64-65536)
  --interval <MS>          전송 간격 밀리초 (기본: 100ms)
  --connections <COUNT>    동시 연결 수 (기본: 1, 최대: 10)
  --echo-port <PORT>       Echo 서비스 포트 (기본: 5038)
```

### Test Scenarios
1. **Memory Leak Test**: 1시간 동안 1초마다 1KB 패킷 송수신
2. **High Frequency Test**: 10ms 간격으로 64바이트 패킷을 10분간 송수신
3. **Large Packet Test**: 64KB 패킷을 30초 간격으로 30분간 송수신
4. **Multi-Connection Test**: 5개 연결로 동시에 각기 다른 패킷 크기 테스트

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
   - Run `/scripts/bash/update-agent-context.sh claude` for your AI assistant
   - If exists: Add only NEW tech from current plan
   - Preserve manual additions between markers
   - Update recent changes (keep last 3)
   - Keep under 150 lines for token efficiency
   - Output to repository root

**Output**: data-model.md, /contracts/*, failing tests, quickstart.md, agent-specific file

## Phase 2: Task Planning Approach
*This section describes what the /tasks command will do - DO NOT execute during /plan*

**Task Generation Strategy**:
- Load `/templates/tasks-template.md` as base
- Generate tasks from Phase 1 design docs (contracts, data model, quickstart)
- Each contract → contract test task [P]
- Each entity → model creation task [P]
- Each user story → integration test task
- Implementation tasks to make tests pass
- Echo service transport tasks:
  - Server-side echo transport implementation
  - Client-side aging command implementation
  - Echo service integration tests

**Ordering Strategy**:
- TDD order: Tests before implementation 
- Dependency order: Models before services before UI
- Mark [P] for parallel execution (independent files)

**Estimated Output**: 30-35 numbered, ordered tasks in tasks.md (기존 32개 + echo service 관련 3-5개)

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
- [x] Phase 3: Basic tasks completed (T001-T032)
- [ ] Phase 3.6: Echo service extension tasks (/tasks command for new tasks)
- [ ] Phase 4: Echo service implementation complete
- [ ] Phase 5: Echo service validation and aging tests passed

**Gate Status**:
- [x] Initial Constitution Check: PASS
- [x] Post-Design Constitution Check: PASS
- [x] All NEEDS CLARIFICATION resolved
- [x] Complexity deviations documented (N/A - no deviations)
- [ ] Echo service requirements integrated

---
*Based on Constitution v2.1.1 - See `/memory/constitution.md`*