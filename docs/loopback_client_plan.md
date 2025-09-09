# DBGIF Generic Loopback Client Implementation Plan

## 프로젝트 개요

DBGIF 서버에 TCP로 연결하여 등록된 모든 종류의 디바이스(PL-25A1, Android USB, TCP 등)와 루프백 테스트를 수행하는 범용 클라이언트를 구현합니다.

## 목표

- **범용성**: 모든 Transport 타입에서 동작
- **표준 호환성**: DBGIF/ADB 프로토콜 준수
- **성능 측정**: throughput, latency, 에러율 통계
- **사용 편의성**: 직관적인 CLI 인터페이스

## 아키텍처 설계

### 전체 구조

```
┌─────────────────┐    TCP:5037     ┌──────────────┐
│  Loopback       │◄────────────────┤ DBGIF Server │
│  Client         │                 │              │
└─────────────────┘                 └──────┬───────┘
                                           │
                                   ┌───────▼────────┐
                                   │  Transport     │
                                   │  Manager       │
                                   └───────┬────────┘
                                           │
                     ┌─────────────────────┼─────────────────────┐
                     │                     │                     │
             ┌───────▼────────┐   ┌───────▼────────┐   ┌───────▼────────┐
             │  PL-25A1       │   │  Android USB   │   │  TCP/Remote    │
             │  Transport     │   │  Transport     │   │  Transport     │
             └────────────────┘   └────────────────┘   └────────────────┘
```

### 모듈 구조

```
examples/
├── loopback_client.rs           # 메인 애플리케이션
└── common/
    ├── mod.rs                   # 공통 모듈 선언
    ├── client.rs                # DBGIF 클라이언트 기본 기능
    ├── loopback.rs              # 루프백 테스트 로직
    ├── stats.rs                 # 성능 통계 관리
    └── protocol.rs              # 프로토콜 헬퍼 함수
```

## 상세 구현 계획

### 1. Command Line Interface

#### 기본 구조
```rust
#[derive(Parser)]
#[command(name = "loopback_client")]
#[command(about = "Generic DBGIF loopback test client")]
struct Args {
    /// DBGIF server address
    #[arg(long, default_value = "127.0.0.1:5037")]
    server: String,
    
    /// List available devices and exit
    #[arg(long, short)]
    list: bool,
    
    /// Device A identifier
    #[arg(long, short = 'a')]
    device_a: Option<String>,
    
    /// Device B identifier (for dual-device tests)
    #[arg(long, short = 'b')]
    device_b: Option<String>,
    
    /// Test duration in seconds (0 = infinite)
    #[arg(long, default_value = "60")]
    duration: u64,
    
    /// Data size per transfer in bytes
    #[arg(long, default_value = "1024")]
    size: usize,
    
    /// Delay between transfers in milliseconds
    #[arg(long, default_value = "100")]
    delay: u64,
    
    /// Test pattern
    #[arg(long, default_value = "echo")]
    pattern: TestPattern,
    
    /// Number of concurrent streams
    #[arg(long, default_value = "1")]
    streams: usize,
    
    /// Enable CSV output
    #[arg(long)]
    csv: bool,
    
    /// Verbose logging
    #[arg(long, short)]
    verbose: bool,
}
```

#### 테스트 패턴
```rust
#[derive(Debug, Clone, ValueEnum)]
enum TestPattern {
    /// Simple echo test (데이터 송신 후 동일 데이터 수신)
    Echo,
    /// Bulk transfer (대용량 데이터 전송)
    Bulk,
    /// Random size transfers (가변 크기 데이터)
    Random,
    /// Bidirectional (양방향 동시 전송)
    Bidirectional,
    /// Stress test (최대 성능 테스트)
    Stress,
}
```

### 2. Core Components

#### A. DbgifClient (common/client.rs)

**역할**: DBGIF 서버와의 기본 통신 담당

```rust
pub struct DbgifClient {
    server_addr: String,
}

impl DbgifClient {
    // 기본 연결 및 핸드셰이크
    pub async fn connect(&self) -> Result<TcpStream>;
    
    // 디바이스 목록 조회
    pub async fn list_devices(&self) -> Result<Vec<DeviceInfo>>;
    
    // 특정 디바이스 선택
    pub async fn select_device(&self, device_id: &str) -> Result<TcpStream>;
    
    // 스트림 열기 (서비스 요청)
    pub async fn open_stream(&self, stream: &mut TcpStream, service: &str) -> Result<u32>;
    
    // 메시지 송수신 헬퍼
    pub async fn send_message(&self, stream: &mut TcpStream, msg: &Message) -> Result<()>;
    pub async fn receive_message(&self, stream: &mut TcpStream) -> Result<Message>;
    
    // 스트림 데이터 송수신
    pub async fn write_stream_data(&self, stream: &mut TcpStream, local_id: u32, remote_id: u32, data: &[u8]) -> Result<()>;
    pub async fn read_stream_data(&self, stream: &mut TcpStream) -> Result<(u32, u32, Vec<u8>)>;
}
```

**프로토콜 플로우**:
1. TCP 연결
2. CNXN 핸드셰이크
3. host:devices 또는 host:transport 요청
4. 스트림 기반 데이터 통신

#### B. LoopbackTest (common/loopback.rs)

**역할**: 실제 루프백 테스트 로직 구현

```rust
pub struct LoopbackTest {
    client: DbgifClient,
    stats: Arc<TestStats>,
    config: TestConfig,
}

impl LoopbackTest {
    // 단일 디바이스 에코 테스트
    pub async fn run_single_device(&mut self, device_id: &str) -> Result<()>;
    
    // 양방향 디바이스 테스트
    pub async fn run_dual_device(&mut self, device_a: &str, device_b: &str) -> Result<()>;
    
    // 스트레스 테스트 (다중 스트림)
    pub async fn run_stress_test(&mut self, device_id: &str) -> Result<()>;
    
    // 단일 라운드트립 테스트
    async fn test_roundtrip(&self, stream: &mut TcpStream, data: &[u8]) -> Result<Duration>;
    
    // 에코 서비스 설정 (서버측)
    async fn setup_echo_service(&self, stream: &mut TcpStream) -> Result<()>;
    
    // 테스트 데이터 생성
    fn generate_test_data(&self, size: usize) -> Vec<u8>;
}
```

**테스트 시나리오**:

1. **단일 디바이스 에코**:
   ```
   Client -> Server -> Device -> Server -> Client
   ```

2. **양방향 디바이스**:
   ```
   Client -> Server -> Device A -> Device B -> Server -> Client
   ```

3. **다중 스트림**:
   ```
   Client (Stream1) -> Server -> Device
   Client (Stream2) -> Server -> Device
   Client (StreamN) -> Server -> Device
   ```

#### C. TestStats (common/stats.rs)

**역할**: 성능 통계 수집 및 리포팅

```rust
pub struct TestStats {
    start_time: Instant,
    total_transfers: AtomicU64,
    successful_transfers: AtomicU64,
    total_bytes: AtomicU64,
    error_count: AtomicU32,
    latency_histogram: Mutex<BTreeMap<u64, u64>>,
    csv_output: bool,
}

impl TestStats {
    pub fn new(csv_output: bool) -> Self;
    
    // 성공 기록
    pub fn record_success(&self, bytes: usize, latency: Duration);
    
    // 에러 기록
    pub fn record_error(&self, error_type: ErrorType);
    
    // 실시간 통계 출력
    pub fn print_current_stats(&self);
    
    // 최종 요약 출력
    pub fn print_final_summary(&self);
    
    // CSV 로그 출력
    pub fn log_csv_entry(&self, timestamp: u64, bytes: usize, latency: Duration, success: bool);
    
    // 히스토그램 계산
    pub fn calculate_percentiles(&self) -> LatencyPercentiles;
}
```

**측정 항목**:
- **처리량**: 초당 전송 바이트 수
- **지연시간**: min/avg/max/p95/p99
- **성공률**: 성공/실패 비율
- **에러 분석**: 에러 유형별 통계

### 3. 프로토콜 구현

#### A. DBGIF 핸드셰이크

```
1. Client -> Server: CNXN (VERSION, MAXDATA, "host::loopback_client")
2. Server -> Client: CNXN (확인)
```

#### B. 디바이스 목록 조회

```
1. Client -> Server: OPEN (local_id=1, "host:devices")
2. Server -> Client: OKAY (local_id=1, remote_id=X)
3. Server -> Client: WRTE (device_list_data)
4. Server -> Client: CLSE
```

#### C. 디바이스 선택

```
1. Client -> Server: OPEN (local_id=2, "host:transport:<device_id>")
2. Server -> Client: OKAY (local_id=2, remote_id=Y)
   // 이제 해당 디바이스와 직접 통신 가능
```

#### D. 루프백 테스트

```
1. Client -> Server: OPEN (local_id=3, "shell:echo")
2. Server -> Device: OPEN (forward)
3. Device -> Server: OKAY
4. Server -> Client: OKAY
5. Client -> Server: WRTE (test_data)
6. Server -> Device: WRTE (forward)
7. Device -> Server: WRTE (echo_data)
8. Server -> Client: WRTE (forward)
9. Client: 데이터 검증 및 지연시간 측정
```

### 4. 에러 처리 및 복구

#### 에러 유형
```rust
#[derive(Debug, Clone)]
enum TestError {
    ConnectionFailed(String),
    DeviceNotFound(String),
    DeviceOffline(String),
    ProtocolError(String),
    DataMismatch { expected: usize, received: usize },
    Timeout(Duration),
    NetworkError(String),
}
```

#### 복구 전략
1. **연결 에러**: 자동 재연결 시도 (최대 3회)
2. **디바이스 오프라인**: 다른 디바이스로 fallback
3. **데이터 불일치**: 에러 로그 후 계속 진행
4. **타임아웃**: 재시도 후 에러 카운트

### 5. 사용 시나리오

#### 기본 사용법

```bash
# 디바이스 목록 확인
cargo run --example loopback_client -- --list
# 출력:
# Available devices:
#   pl25a1_1:3      device
#   pl25a1_1:4      device
#   android_ABC123  device

# 자동 디바이스 선택으로 기본 테스트
cargo run --example loopback_client

# 특정 디바이스로 60초 테스트
cargo run --example loopback_client -- \
    --device-a pl25a1_1:3 \
    --duration 60 \
    --size 4096

# 두 디바이스 간 양방향 테스트
cargo run --example loopback_client -- \
    --device-a pl25a1_1:3 \
    --device-b pl25a1_1:4 \
    --pattern bidirectional

# 스트레스 테스트 (다중 스트림)
cargo run --example loopback_client -- \
    --device-a android_ABC123 \
    --pattern stress \
    --streams 4 \
    --size 65536 \
    --delay 0

# CSV 출력으로 성능 분석
cargo run --example loopback_client -- \
    --csv --duration 300 > performance.csv
```

#### 원격 서버 테스트

```bash
# 원격 DBGIF 서버 테스트
cargo run --example loopback_client -- \
    --server 192.168.1.100:5037 \
    --device-a remote_device_1 \
    --duration 600

# 고성능 네트워크 테스트
cargo run --example loopback_client -- \
    --server 10.0.0.50:5037 \
    --pattern bulk \
    --size 1048576 \
    --streams 8
```

### 6. 성능 최적화

#### 메모리 최적화
- 대용량 데이터 전송 시 스트리밍 처리
- 버퍼 재사용으로 메모리 할당 최소화
- 통계 데이터 효율적 저장

#### 네트워크 최적화
- TCP_NODELAY 설정으로 지연시간 최소화
- 배치 전송으로 처리량 향상
- 연결 풀링으로 오버헤드 감소

#### 동시성 최적화
- 다중 스트림 병렬 처리
- 비동기 I/O 활용
- 백그라운드 통계 수집

### 7. 확장성 고려사항

#### 새로운 테스트 패턴 추가
- 플러그인 아키텍처 고려
- 테스트 패턴별 설정 파일
- 커스텀 데이터 생성기

#### 다양한 출력 형식
- JSON 형식 통계 출력
- Prometheus 메트릭 연동
- 실시간 대시보드 연동

#### 클러스터 테스트
- 다중 서버 동시 테스트
- 분산 부하 생성
- 중앙집중식 결과 수집

### 8. 구현 우선순위

**Phase 1 - 기본 기능** ✅ **완료됨 (2025-09-09)**:
1. ✅ CLI 구조 및 기본 설정
2. ✅ DBGIF 클라이언트 연결 (타임아웃, 재시도 로직 포함)
3. ✅ 디바이스 목록 조회 (빈 목록 처리 포함)
4. ✅ 단일 디바이스 에코 테스트 (shell:echo 서비스)

**Phase 2 - 핵심 기능** ⚪ **다음 단계**:
5. ✅ 성능 통계 수집 (이미 구현됨)
6. ✅ CSV 출력 기능 (이미 구현됨)
7. ⚪ 다양한 테스트 패턴 구현 (Bulk, Random, Bidirectional, Stress)
8. ✅ 에러 처리 및 복구 (이미 구현됨)

**Phase 3 - 고급 기능**:
9. 양방향 디바이스 테스트
10. 다중 스트림 지원
11. 스트레스 테스트
12. 성능 최적화

**Phase 4 - 확장 기능**:
13. 원격 서버 지원
14. 고급 통계 분석
15. 설정 파일 지원
16. 플러그인 아키텍처

### 9. 테스트 검증

#### 단위 테스트
- 각 모듈별 단위 테스트
- 프로토콜 메시지 파싱 테스트
- 통계 계산 로직 테스트

#### 통합 테스트
- 실제 DBGIF 서버와 연동 테스트
- 다양한 Transport 타입 테스트
- 장시간 안정성 테스트

#### 성능 테스트
- 벤치마크 비교
- 메모리 사용량 프로파일링
- 네트워크 대역폭 효율성 측정

이 계획을 통해 DBGIF 서버의 모든 기능을 검증하고, 실제 운영 환경에서의 성능과 안정성을 확인할 수 있는 강력한 테스트 도구를 구현할 수 있습니다.

## 10. 구현 진행 상황

### Phase 1 완료 현황 (2025-09-09)

**구현 완료된 핵심 기능들:**
- **통합된 단일 파일 구조**: `examples/loopback_client.rs`에 모든 기능을 통합하여 단순성과 유지보수성 향상
- **견고한 연결 관리**: 타임아웃, 재시도, 에러 복구 로직 완전 구현
- **실시간 성능 모니터링**: 처리량, 지연시간, 성공률 실시간 추적
- **유연한 출력 형식**: 콘솔 출력 및 CSV 형식 지원
- **완전한 프로토콜 준수**: DBGIF/ADB 프로토콜 정확한 구현

**테스트 검증 완료:**
- ✅ 서버 연결 및 핸드셰이크
- ✅ 디바이스 목록 조회 (빈 목록 포함)
- ✅ 에러 처리 및 타임아웃
- ✅ 통계 수집 및 보고

**다음 단계 (Phase 2):**
- 다양한 테스트 패턴 구현 (Bulk, Random, Bidirectional, Stress)
- 실제 USB 디바이스와의 통합 테스트
- 성능 최적화 및 벤치마킹