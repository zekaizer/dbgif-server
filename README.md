# dbgif-server
dbgif server written in rust

## PL-25A1 Loopback Test

동일 호스트에서 PL-25A1 케이블 양쪽을 연결하여 전체 프로토콜 스택을 테스트하는 예제입니다.

### 필요사항
- PL-25A1 USB Host-to-Host Bridge Cable
- 2개의 USB 포트 (케이블 양쪽을 동일 호스트에 연결)

### 실행 방법

#### 기본 사용법
```bash
# 기본 에코 테스트 (60초)
cargo run --example pl25a1_loopback

# 도움말 보기
cargo run --example pl25a1_loopback -- --help
```

#### 다양한 에이징 테스트 패턴

```bash
# 대용량 데이터 연속 전송 (5분)
cargo run --example pl25a1_loopback -- --pattern bulk --duration 300 --size 65536

# 랜덤 크기 데이터 전송 (무한, Ctrl+C로 중단)
cargo run --example pl25a1_loopback -- --pattern random --duration 0 --size 8192

# 버스트 패턴 (고처리량/대기 반복)
cargo run --example pl25a1_loopback -- --pattern burst --duration 600

# 양방향 동시 전송 시뮬레이션
cargo run --example pl25a1_loopback -- --pattern bidirectional --duration 180

# 최대 처리량 스트레스 테스트
cargo run --example pl25a1_loopback -- --pattern stress --duration 3600

# 전송 간격 지연을 두고 테스트
cargo run --example pl25a1_loopback -- --pattern bulk --delay 100

# CSV 로그 출력
cargo run --example pl25a1_loopback -- --pattern random --csv-log > test_results.csv

# 상세 디버그 로그
RUST_LOG=debug cargo run --example pl25a1_loopback -- --pattern stress
```

#### 테스트 패턴 설명

- **echo**: 기본 에코 테스트 (고정 크기)
- **bulk**: 대용량 데이터 연속 전송
- **random**: 랜덤 크기 데이터 전송 (1바이트~최대크기)
- **burst**: 고처리량(10초)/대기(5초) 반복 패턴
- **bidirectional**: 양방향 전송 시뮬레이션
- **stress**: 최대 처리량 스트레스 테스트

### 테스트 구조
```
Transport Layer A ←→ Protocol Layer A ←→ Test Logic ←→ Protocol Layer B ←→ Transport Layer B
      ↓                                                                               ↓
  USB Port A                                                                   USB Port B
      └─────────────────────── PL-25A1 Bridge Cable ───────────────────────────────┘
```

### 예상 출력

#### 기본 에코 테스트
```
[INFO] PL-25A1 Loopback Aging Test Application
[INFO] ======================================
[INFO] Test Pattern: Echo
[INFO] Duration: 60 seconds
[INFO] Data Size: 1024 bytes
[INFO] Found 2 PL-25A1 devices
[INFO] Side A: pl25a1_1:2 (Connector ID: true)  
[INFO] Side B: pl25a1_1:3 (Connector ID: false)
[INFO] ✓ Handshake completed successfully
[INFO] Running Echo Test Pattern...
[INFO] [STATS] Time: 5s | Sent: 2 MB | Throughput: 0 MB/s | Transfers: 1250 | Errors: 0 | Latency: min=850μs avg=950μs max=1200μs
[INFO] Completed 3000 echo transfers
[INFO] === FINAL PERFORMANCE SUMMARY ===
[INFO] Total Duration: 60.1s
[INFO] Total Bytes Sent: 3 MB
[INFO] Average Throughput: 0 MB/s
[INFO] Total Transfers: 3000
[INFO] Success Rate: 100.00%
[INFO] ✓ All tests completed successfully!
```

#### 스트레스 테스트
```
[INFO] Running Stress Test Pattern (Maximum Throughput)...
[INFO] [STATS] Time: 5s | Sent: 50 MB | Throughput: 10 MB/s | Transfers: 800 | Errors: 2 | Latency: min=2ms avg=6ms max=15ms
[INFO] [STATS] Time: 10s | Sent: 120 MB | Throughput: 12 MB/s | Transfers: 1850 | Errors: 5 | Latency: min=2ms avg=5ms max=20ms
```

#### CSV 로그 출력
```
timestamp_ms,bytes_sent,bytes_received,latency_us
1250,1024,1024,850
2100,2048,2048,920
3500,4096,4096,1100
```
