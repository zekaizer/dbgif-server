# Quickstart: TCP Test Client

## Prerequisites

### 1. DBGIF Server Running
서버가 실행 중이어야 합니다:
```bash
# 서버 시작
cd /path/to/dbgif-server
cargo run

# 다른 터미널에서 서버 상태 확인
netstat -ln | grep 5037
```

### 2. Build Test Client
```bash
# 테스트 클라이언트 빌드
cargo build --bin dbgif-test-client

# 또는 실행과 동시에 빌드
cargo run --bin dbgif-test-client -- --help
```

## Basic Usage

### 1. Server Connectivity Test
가장 기본적인 연결 테스트:
```bash
./target/debug/dbgif-test-client ping
```

**Expected Output**:
```
Testing connection to localhost:5037...
✓ TCP connection established (23ms)
✓ CNXN handshake completed (45ms)
✓ Server responded to ping (12ms)

Result: PASSED (total: 80ms)
```

### 2. Host Commands Test
서버의 기본 명령어 테스트:
```bash
./target/debug/dbgif-test-client host-commands
```

**Expected Output**:
```
Testing host commands...
✓ host:version -> "0.1.0" (34ms)
✓ host:devices -> "List of 0 devices" (28ms)
✓ host:track-devices -> "OK" (31ms)

Result: PASSED (3/3 commands successful)
```

### 3. Multi-Connection Test
다중 연결 부하 테스트:
```bash
# 동시 3개 연결
./target/debug/dbgif-test-client multi-connect --count 3 --mode concurrent

# 순차 5개 연결
./target/debug/dbgif-test-client multi-connect --count 5 --mode sequential
```

**Expected Output**:
```
Testing 3 concurrent connections...
Connection 1: ✓ PASSED (89ms)
Connection 2: ✓ PASSED (92ms)
Connection 3: ✓ PASSED (88ms)

Result: PASSED (3/3 connections successful)
Average duration: 90ms
```

## Advanced Options

### JSON Output Format
스크립트나 외부 툴에서 사용할 수 있는 JSON 출력:
```bash
./target/debug/dbgif-test-client ping --format json
```

**Output**:
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

### Custom Server Address
다른 서버에 대한 테스트:
```bash
./target/debug/dbgif-test-client ping --server 192.168.1.100:5037
```

### Verbose Logging
상세한 프로토콜 로그 출력:
```bash
RUST_LOG=debug ./target/debug/dbgif-test-client ping --verbose
```

## Validation Scenarios

### 1. Server Not Running
```bash
./target/debug/dbgif-test-client ping
```
**Expected Error**:
```
Error: Failed to connect to localhost:5037
Cause: Connection refused (os error 111)

Suggestion: Ensure DBGIF server is running on port 5037
Exit code: 3
```

### 2. Protocol Error Simulation
서버 응답이 잘못된 경우:
```
Error: Invalid server response
Cause: Checksum mismatch in CNXN message
Details: Expected 0x12345678, got 0x87654321
Exit code: 1
```

### 3. Timeout Test
네트워크 지연 시뮬레이션:
```bash
./target/debug/dbgif-test-client ping --timeout 1
```

## Integration Test Validation

### Test Suite Execution
모든 기능을 한번에 테스트:
```bash
# 기본 테스트 시퀀스
./target/debug/dbgif-test-client ping && \
./target/debug/dbgif-test-client host-commands && \
./target/debug/dbgif-test-client multi-connect --count 3

echo "All tests completed with exit code: $?"
```

### CI/CD Integration Example
```bash
#!/bin/bash
# test-server.sh

# 서버 시작
cargo run &
SERVER_PID=$!

# 서버 준비 대기
sleep 2

# 테스트 실행
TESTS_PASSED=0

if ./target/debug/dbgif-test-client ping --format json | jq -r '.status' | grep -q "passed"; then
    echo "✓ Connectivity test PASSED"
    ((TESTS_PASSED++))
else
    echo "✗ Connectivity test FAILED"
fi

if ./target/debug/dbgif-test-client host-commands --format json | jq -r '.status' | grep -q "passed"; then
    echo "✓ Host commands test PASSED"
    ((TESTS_PASSED++))
else
    echo "✗ Host commands test FAILED"
fi

if ./target/debug/dbgif-test-client multi-connect --count 5 --format json | jq -r '.status' | grep -q "passed"; then
    echo "✓ Multi-connection test PASSED"
    ((TESTS_PASSED++))
else
    echo "✗ Multi-connection test FAILED"
fi

# 서버 종료
kill $SERVER_PID

# 결과 보고
if [ $TESTS_PASSED -eq 3 ]; then
    echo "All integration tests PASSED"
    exit 0
else
    echo "Some tests FAILED ($TESTS_PASSED/3 passed)"
    exit 1
fi
```

## Performance Baselines

### Expected Performance
- **Single Connection**: < 100ms 총 소요시간
- **Host Commands**: 각 명령어 < 50ms
- **Multi-Connection (3개)**: < 200ms 총 소요시간
- **Memory Usage**: < 10MB RSS

### Performance Regression Detection
```bash
# 성능 벤치마크 실행
for i in {1..10}; do
    time ./target/debug/dbgif-test-client ping --format json | jq -r '.duration_ms'
done
```

## Troubleshooting

### Common Issues
1. **서버가 응답하지 않음**: 서버 프로세스 상태 확인
2. **권한 오류**: 포트 바인딩 권한 확인
3. **의존성 오류**: `cargo check` 실행하여 빌드 문제 확인

### Debug Information
```bash
# 상세 로깅으로 실행
RUST_LOG=trace ./target/debug/dbgif-test-client ping --verbose

# 네트워크 패킷 캡처 (선택사항)
sudo tcpdump -i lo port 5037
```