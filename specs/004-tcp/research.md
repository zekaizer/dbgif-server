# Research: TCP Test Client

## Overview
DBGIF 서버 검증용 TCP 테스트 클라이언트 개발을 위한 기술 조사 결과

## Technology Decisions

### 1. CLI Framework Choice
**Decision**: clap v4.4 with derive features
**Rationale**:
- 이미 프로젝트에 포함됨 (dependency overhead 없음)
- derive 매크로로 간단한 CLI 구성 가능
- --help, --version 자동 생성
**Alternatives considered**: structopt (deprecated), argh (too minimal)

### 2. Async Runtime
**Decision**: tokio (기존 dependency 활용)
**Rationale**:
- 프로젝트에서 이미 사용 중
- TCP 연결 및 다중 연결 테스트에 적합
- 비동기 I/O로 동시 연결 테스트 효율적 처리
**Alternatives considered**: async-std (불필요한 추가 의존성)

### 3. Protocol Implementation
**Decision**: 기존 protocol 모듈 재사용
**Rationale**:
- src/protocol/message.rs 이미 DBGIF 메시지 구조 구현됨
- src/protocol/checksum.rs CRC32 검증 로직 존재
- 코드 중복 방지, 일관성 유지
**Alternatives considered**: 별도 구현 (코드 중복 발생)

### 4. Error Handling
**Decision**: anyhow crate 활용
**Rationale**:
- 프로젝트에서 이미 사용 중
- 간단한 에러 체인 및 컨텍스트 제공
- CLI 애플리케이션에 적합
**Alternatives considered**: thiserror (복잡도 불필요)

### 5. Logging Strategy
**Decision**: tracing crate
**Rationale**:
- 기존 프로젝트와 일관성
- 구조화된 로깅 지원
- 개발 디버깅 및 테스트 결과 추적에 유용
**Alternatives considered**: log crate (기능 부족)

## Implementation Patterns

### TCP Connection Pattern
```rust
// 기존 코드베이스의 패턴 활용
use tokio::net::TcpStream;
use crate::protocol::message::Message;

// 단순한 연결 → 핸드셰이크 → 명령어 → 결과 패턴
```

### Test Result Reporting
**Decision**: 구조화된 콘솔 출력
**Rationale**:
- 개인 프로젝트 특성상 파일 저장 불필요
- 컬러 출력으로 시각적 구분
- JSON 포맷 옵션 제공 (--format json)

### Multi-Connection Testing
**Decision**: tokio::join! 매크로 활용
**Rationale**:
- 동시 연결 테스트를 위한 간단한 패턴
- 각 연결별 독립적 결과 수집
- 오버엔지니어링 지양

## Architecture Decisions

### Library Structure
```
src/
├── test_client/          # 새로운 모듈
│   ├── mod.rs
│   ├── connection.rs     # TCP 연결 및 프로토콜 처리
│   ├── tests.rs         # 개별 테스트 로직
│   └── reporter.rs      # 결과 출력
└── bin/
    └── dbgif-test-client.rs  # CLI 엔트리포인트
```

### Protocol Reuse Strategy
- 기존 `src/protocol/` 모듈 전체 재사용
- 새로운 protocol 코드 작성 안 함
- Message, Command enum 직접 활용

## Performance Considerations

### Connection Limits
**Decision**: 최대 10개 동시 연결
**Rationale**:
- 개인 프로젝트 범위에 적합
- 서버 과부하 방지
- 테스트 목적으로 충분

### Timeout Settings
**Decision**: 연결 타임아웃 5초, 응답 타임아웃 3초
**Rationale**:
- 로컬 테스트 환경 고려
- 네트워크 지연 허용
- 테스트 실행 시간 합리적 유지

## Testing Strategy

### Unit Tests
- 개별 함수별 단위 테스트
- mock TCP stream 활용 금지 (실제 연결 테스트)

### Integration Tests
- 실제 DBGIF 서버와 통신 테스트
- 프로토콜 호환성 검증
- 에러 시나리오 처리 확인

### Contract Tests
- 메시지 직렬화/역직렬화 검증
- 체크섬 계산 정확성 확인
- 핸드셰이크 시퀀스 준수

## Scope Limitations (개인프로젝트)

### 제외되는 기능들
- 복잡한 설정 파일 지원
- 플러그인 아키텍처
- 고급 성능 지표 수집
- 실시간 모니터링 대시보드
- 분산 테스트 지원

### 포함되는 핵심 기능만
- TCP 연결 테스트
- CNXN 핸드셰이크 검증
- 기본 host 명령어 실행
- 다중 연결 테스트
- 명확한 성공/실패 결과 출력

## Resolved Unknowns
모든 NEEDS CLARIFICATION 항목이 해결됨:
- 기본 명령어: host:version, host:devices 등 디바이스 불필요한 명령어들
- 결과 저장: 콘솔 출력만 (파일 저장 불필요)
- 테스트 실행 방식: 다중 연결 지원 (동시 또는 순차)