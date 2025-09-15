# Quickstart: TCP Test Client

## Prerequisites

### 1. DBGIF Server Running
서버가 실행 중이어야 합니다:
```bash
# 서버 시작
cd /path/to/dbgif-server
cargo run --bin dbgif-server

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

**Expected Output (basic)**:
```
# Silent execution unless there's an error
# Use --verbose for detailed output
```

**Expected Output (verbose mode)**:
```bash
./target/debug/dbgif-test-client ping --verbose
```
```
🔄 Starting ping test to localhost:5037 (timeout: 5s)
✅ Connected to localhost:5037 in 0ms
✅ Handshake completed in 102ms
📊 Test result: SUCCESS in 102ms (5 events) | Conn: 0ms, Handshake: 101ms
```

### 2. Host Commands Test
서버의 기본 명령어 테스트:
```bash
./target/debug/dbgif-test-client host-commands
```

**Expected Output (basic)**:
```
# Silent execution unless there's an error
# Use --verbose for detailed output
```

**Expected Output (verbose mode)**:
```bash
./target/debug/dbgif-test-client host-commands --verbose
```
```
🔄 Testing host commands on localhost:5037
✅ Host commands test: connection and handshake successful
📊 Test result: SUCCESS in 102ms (6 events)
```

### 3. Multi-Connection Test
다중 연결 부하 테스트:
```bash
# 동시 2개 연결 (기본값: 3개)
./target/debug/dbgif-test-client multi-connect --count 2

# 최대 10개 연결까지 지원
./target/debug/dbgif-test-client multi-connect --count 5
```

**Expected Output (basic)**:
```
# Silent execution unless there's an error
# Use --verbose for detailed output
```

**Expected Output (verbose mode)**:
```bash
./target/debug/dbgif-test-client multi-connect --count 2 --verbose
```
```
🔄 Testing 2 concurrent connections to localhost:5037
✅ Connection 1 completed in 204ms
✅ Connection 0 completed in 204ms
📊 Multi-connect results:
  Total connections: 2
  Successful: 2 (100.0%)
  Failed: 0
  Total time: 204ms
  Average per connection: 204.00ms
📊 Test result: SUCCESS in 204ms (5 events)
```

## Advanced Options

### JSON Output Format
스크립트나 외부 툴에서 사용할 수 있는 JSON 출력:
```bash
./target/debug/dbgif-test-client ping --json
```

**Output**:
```json
{
  "Success": {
    "duration": 102,
    "events": [
      "test_started",
      "connection_established",
      "handshake_completed",
      "connection_closed",
      "test_completed"
    ],
    "performance_metrics": {
      "connection_time": 0,
      "handshake_time": 102,
      "bytes_sent": 0,
      "bytes_received": 0,
      "connection_count": 1,
      "successful_connections": 1,
      "failed_connections": 0
    }
  }
}
```

### Custom Server Address
다른 서버에 대한 테스트:
```bash
./target/debug/dbgif-test-client ping --host 192.168.1.100 --port 5037
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
cargo run --bin dbgif-server &
SERVER_PID=$!

# 서버 준비 대기
sleep 2

# 테스트 실행
TESTS_PASSED=0

# Test ping
if env RUST_LOG=off ./target/debug/dbgif-test-client ping --json | jq -e '.Success' > /dev/null 2>&1; then
    echo "✓ Connectivity test PASSED"
    ((TESTS_PASSED++))
else
    echo "✗ Connectivity test FAILED"
fi

# Test host-commands
if env RUST_LOG=off ./target/debug/dbgif-test-client host-commands --json | jq -e '.Success' > /dev/null 2>&1; then
    echo "✓ Host commands test PASSED"
    ((TESTS_PASSED++))
else
    echo "✗ Host commands test FAILED"
fi

# Test multi-connect
if env RUST_LOG=off ./target/debug/dbgif-test-client multi-connect --count 3 --json | jq -e '.Success' > /dev/null 2>&1; then
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
- **Single Connection**: < 1000ms (실제: ~100ms)
- **Host Commands**: < 1000ms (실제: ~100ms)
- **Multi-Connection (2개)**: < 1000ms (실제: ~200ms)
- **Memory Usage**: < 10MB RSS
- **All operations**: Meet <1s baseline requirement

### Performance Regression Detection
```bash
# 성능 벤치마크 실행
for i in {1..10}; do
    env RUST_LOG=off time ./target/debug/dbgif-test-client ping --json | jq -r '.Success.duration'
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

## CLI Options Reference

### Global Options
- `--verbose`, `-v`: Enable detailed output with progress indicators
- `--json`, `-j`: Output results in JSON format for automation
- `--help`, `-h`: Show help information
- `--version`, `-V`: Show version information

### Command-Specific Options

#### ping
- `--host <HOST>`: Target host (default: localhost)
- `--port <PORT>`: Target port (default: 5037)
- `--timeout <TIMEOUT>`: Timeout in seconds (default: 5)

#### host-commands
- `--host <HOST>`: Target host (default: localhost)
- `--port <PORT>`: Target port (default: 5037)

#### multi-connect
- `--host <HOST>`: Target host (default: localhost)
- `--port <PORT>`: Target port (default: 5037)
- `--count <COUNT>`: Number of connections, max 10 (default: 3)

## Exit Codes
- `0`: Success
- `1`: Test failure or protocol error
- `2`: Invalid arguments
- `3`: Connection error (server not running)