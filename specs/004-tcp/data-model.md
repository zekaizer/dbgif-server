# Data Model: TCP Test Client

## Core Entities

### TestSession
테스트 실행 세션을 나타내는 엔터티

**Fields**:
- `id: String` - 세션 고유 식별자
- `start_time: chrono::DateTime<Utc>` - 테스트 시작 시간
- `end_time: Option<chrono::DateTime<Utc>>` - 테스트 종료 시간
- `server_address: String` - 대상 서버 주소 (기본: localhost:5037)
- `test_type: TestType` - 테스트 유형
- `results: Vec<TestResult>` - 개별 테스트 결과 목록

**State Transitions**:
```
Created → Running → Completed | Failed
```

**Validation Rules**:
- `start_time`은 필수값
- `server_address`는 유효한 TCP 주소 형식
- `results`는 테스트 완료 후에만 채워짐

### TestType
테스트 유형을 정의하는 열거형

**Variants**:
- `Single` - 단일 연결 테스트
- `Sequential(u32)` - 순차 연결 테스트 (연결 횟수)
- `Concurrent(u32)` - 동시 연결 테스트 (동시 연결 수)

### TestResult
개별 테스트 결과를 나타내는 엔터티

**Fields**:
- `test_name: String` - 테스트 명칭
- `status: TestStatus` - 테스트 상태
- `duration: chrono::Duration` - 실행 시간
- `error_message: Option<String>` - 실패 시 오류 메시지
- `protocol_messages: Vec<ProtocolExchange>` - 프로토콜 메시지 교환 내역
- `connection_id: u32` - 연결 식별자 (다중 연결 시)

**Validation Rules**:
- `test_name`은 빈 문자열 불가
- `duration`은 0 이상
- `error_message`는 Failed 상태일 때만 Some

### TestStatus
테스트 상태를 나타내는 열거형

**Variants**:
- `Pending` - 대기 중
- `Running` - 실행 중
- `Passed` - 성공
- `Failed` - 실패
- `Timeout` - 타임아웃

### ProtocolExchange
프로토콜 메시지 교환을 나타내는 엔터티

**Fields**:
- `direction: MessageDirection` - 메시지 방향
- `message_type: String` - 메시지 타입 (CNXN, OPEN, WRTE 등)
- `timestamp: chrono::DateTime<Utc>` - 메시지 시간
- `payload_size: usize` - 페이로드 크기
- `checksum_valid: Option<bool>` - 체크섬 유효성 (수신 메시지만)

**Relationships**:
- `TestResult`에 여러 개 포함됨

### MessageDirection
메시지 방향을 나타내는 열거형

**Variants**:
- `Sent` - 클라이언트 → 서버
- `Received` - 서버 → 클라이언트

## Simplified Protocol Reuse

### 기존 Protocol 모듈 활용
- `crate::protocol::message::Message` - 직접 사용
- `crate::protocol::message::Command` - 직접 사용
- `crate::protocol::checksum` - 직접 사용

**Benefits**:
- 코드 중복 제거
- 프로토콜 일관성 보장
- 유지보수 용이성

## Data Flow

### Test Execution Flow
```
1. TestSession 생성 (Created 상태)
2. 각 테스트별 TestResult 생성 (Pending 상태)
3. TestSession을 Running으로 전환
4. 개별 테스트 실행:
   - TestResult를 Running으로 전환
   - TCP 연결 및 프로토콜 교환
   - ProtocolExchange 기록
   - TestResult 완료 (Passed/Failed/Timeout)
5. 모든 테스트 완료 후 TestSession 완료

### Error Handling
- 네트워크 오류: TestResult.status = Failed, error_message 기록
- 타임아웃: TestResult.status = Timeout, duration 기록
- 프로토콜 오류: 체크섬 불일치 등을 ProtocolExchange에 기록

## Serialization Support

### JSON Output (--format json)
모든 엔터티는 serde를 통한 JSON 직렬화 지원:
- `TestSession` → JSON 결과 파일 생성 가능
- 디버깅 및 외부 툴 연동 지원

### Console Output (기본)
구조화된 콘솔 출력:
- 테스트 진행상황 실시간 표시
- 컬러 코딩으로 상태 구분
- 요약 통계 제공

## Constraints (개인프로젝트 특성)

### Simplified Design
- 복잡한 ORM 패턴 사용 안 함
- 단순한 struct/enum 조합
- 메모리 내 데이터 처리만 (영속성 없음)

### Minimal Dependencies
- 기존 프로젝트 dependencies 최대한 활용
- 새로운 의존성 추가 최소화
- 표준 라이브러리 우선 사용