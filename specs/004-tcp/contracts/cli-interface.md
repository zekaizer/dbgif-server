# CLI Interface Contract

## Command Structure

### Basic Usage
```bash
dbgif-test-client [OPTIONS] [SUBCOMMAND]
```

### Global Options
- `--server <ADDRESS>`: 서버 주소 (기본값: localhost:5037)
- `--timeout <SECONDS>`: 연결 타임아웃 (기본값: 5)
- `--format <FORMAT>`: 출력 형식 [text|json] (기본값: text)
- `--verbose, -v`: 상세 로깅 활성화
- `--help, -h`: 도움말 출력
- `--version, -V`: 버전 정보 출력

## Subcommands

### ping
서버 기본 연결성 테스트

**Usage**: `dbgif-test-client ping`

**Expected Output (text)**:
```
Testing connection to localhost:5037...
✓ TCP connection established (23ms)
✓ CNXN handshake completed (45ms)
✓ Server responded to ping (12ms)

Result: PASSED (total: 80ms)
```

**Expected Output (json)**:
```json
{
  "test_type": "ping",
  "server": "localhost:5037",
  "status": "passed",
  "duration_ms": 80,
  "details": {
    "tcp_connection_ms": 23,
    "handshake_ms": 45,
    "ping_response_ms": 12
  }
}
```

### host-commands
기본 host 명령어 테스트

**Usage**: `dbgif-test-client host-commands`

**Expected Output (text)**:
```
Testing host commands...
✓ host:version -> "4.4.3" (34ms)
✓ host:devices -> "List of 0 devices" (28ms)
✓ host:track-devices -> "OK" (31ms)

Result: PASSED (3/3 commands successful)
```

### multi-connect
다중 연결 테스트

**Usage**:
```bash
dbgif-test-client multi-connect --count 5 --mode [sequential|concurrent]
```

**Options**:
- `--count <N>`: 연결 개수 (기본값: 3, 최대: 10)
- `--mode <MODE>`: 연결 방식 [sequential|concurrent] (기본값: concurrent)

**Expected Output (text)**:
```
Testing 5 concurrent connections...
Connection 1: ✓ PASSED (89ms)
Connection 2: ✓ PASSED (92ms)
Connection 3: ✓ PASSED (88ms)
Connection 4: ✓ PASSED (95ms)
Connection 5: ✓ PASSED (91ms)

Result: PASSED (5/5 connections successful)
Average duration: 91ms
```

## Exit Codes

- `0`: 모든 테스트 성공
- `1`: 일부 또는 모든 테스트 실패
- `2`: 명령줄 인자 오류
- `3`: 서버 연결 불가능

## Error Handling

### Network Errors
```
Error: Failed to connect to localhost:5037
Cause: Connection refused (os error 111)

Suggestion: Ensure DBGIF server is running on port 5037
```

### Protocol Errors
```
Error: Invalid server response
Cause: Checksum mismatch in CNXN message
Details: Expected 0x12345678, got 0x87654321
```

### Timeout Errors
```
Error: Operation timed out
Cause: No response within 5 seconds
Suggestion: Check network connectivity or increase --timeout
```

## Environment Variables

- `DBGIF_SERVER`: 기본 서버 주소 override
- `DBGIF_TIMEOUT`: 기본 타임아웃 override
- `RUST_LOG`: 로그 레벨 설정

## Configuration File Support

**Scope**: 개인프로젝트 특성상 설정파일 지원하지 않음
**Rationale**: CLI 옵션으로 충분, 오버엔지니어링 지양