# DBGIF Ping/Pong Keep-alive 프로토콜

## 개요

DBGIF 프로토콜에서 Ping/Pong 메시지는 연결 상태 모니터링과 keep-alive 메커니즘을 제공합니다. 이는 기존 ADB 프로토콜을 확장하여 장시간 연결 유지 및 연결 상태 감지를 개선합니다.

## 메시지 정의

### PING 메시지 (0x474e4950)
```
Command: 0x474e4950 ("GNIP" in little-endian)
Arg0: sequence_number (시퀀스 번호)
Arg1: timestamp (Unix timestamp, RTT 측정용)
Data: 비어있음 또는 추가 상태 정보
```

### PONG 메시지 (0x474e4f50)
```
Command: 0x474e4f50 ("GNOP" in little-endian)
Arg0: sequence_number (PING과 동일한 시퀀스)
Arg1: timestamp (PING의 timestamp 반환)
Data: 비어있음 또는 디바이스 상태 정보
```

## 통신 시퀀스

### 1. 주기적 Health Check
```
Client/Server                     Daemon
     │                              │
     │────── PING (seq=1) ─────────►│  매 30초마다
     │        + timestamp           │
     │                              │
     │◄────── PONG (seq=1) ─────────│  즉시 응답
     │        + timestamp           │  (RTT 계산 가능)
     │                              │
     │  ⏰ 응답 타임아웃: 5초        │
     │                              │
```

### 2. Idle 연결 관리
```
Client          Server          Daemon
  │               │               │
  │  🕐 30초 idle  │               │
  │               │── PING ──────►│  Server 주도 health check
  │               │◄─ PONG ───────│
  │               │               │
  │  🕐 60초 idle  │               │
  │── PING ──────►│               │  Client도 독립적으로 체크
  │◄─ PONG ───────│               │
  │               │               │
```

### 3. 연결 실패 감지 및 복구
```
Client/Server                     Daemon
     │                              │
     │────── PING (seq=1) ─────────►│
     │         ❌ 응답 없음          │
     │                              │
     │  ⏰ 5초 대기                 │
     │────── PING (seq=2) ─────────►│  재시도 1
     │         ❌ 응답 없음          │
     │                              │
     │  ⏰ 5초 대기                 │
     │────── PING (seq=3) ─────────►│  재시도 2 (마지막)
     │         ❌ 응답 없음          │
     │                              │
     │                              │
     │ 🔴 연결 종료 판정             │
```

### 4. 양방향 Keep-alive
```
Client          Server          Daemon
  │               │               │
  │  30초 idle     │  30초 idle   │
  │── PING ──────►│               │
  │               │── PING ──────►│
  │◄─ PONG ───────│               │
  │               │◄─ PONG ───────│
  │               │               │
  │  연결 상태 OK  │  연결 상태 OK │
```

## 구현 사양

### Keep-alive 매니저 구조
```rust
struct KeepAliveManager {
    // 설정값
    ping_interval: Duration,      // 30초 (기본값)
    pong_timeout: Duration,       // 5초 (기본값)
    max_retries: u32,            // 3회 (기본값)
    idle_threshold: Duration,     // 20초 (기본값)
    
    // 런타임 상태
    last_activity: Instant,
    sequence_counter: AtomicU32,
    pending_pings: HashMap<u32, PingRecord>,
    connection_state: ConnectionHealth,
}

struct PingRecord {
    sent_at: Instant,
    retry_count: u32,
    timeout_handle: Option<AbortHandle>,
}

#[derive(Debug, Clone)]
enum ConnectionHealth {
    Healthy,
    Degraded { rtt: Duration },
    Failing { consecutive_failures: u32 },
    Dead,
}
```

### 백그라운드 태스크 구현
```rust
async fn keep_alive_task(mut manager: KeepAliveManager) {
    let mut interval = tokio::time::interval(manager.ping_interval);
    
    loop {
        tokio::select! {
            _ = interval.tick() => {
                if manager.should_send_ping() {
                    if let Err(e) = manager.send_ping().await {
                        error!("Failed to send ping: {}", e);
                        manager.handle_send_failure().await;
                    }
                }
            }
            
            // Timeout 처리
            _ = manager.check_timeouts() => {
                manager.handle_ping_timeouts().await;
            }
            
            // 종료 신호
            _ = manager.shutdown_rx.recv() => {
                info!("Keep-alive task shutting down");
                break;
            }
        }
    }
}
```

## 설정 매개변수

| 매개변수 | 기본값 | 범위 | 설명 |
|----------|--------|------|------|
| `ping_interval` | 30초 | 10-300초 | Ping 전송 주기 |
| `pong_timeout` | 5초 | 1-30초 | Pong 응답 대기 시간 |
| `max_retries` | 3회 | 1-10회 | 연결 실패 판정 전 재시도 횟수 |
| `idle_threshold` | 20초 | 5-60초 | 비활성 상태에서 Ping 시작 임계값 |

### 권장 설정값 근거
- **30초 Ping 주기**: 일반적인 NAT 타임아웃(60초)의 절반
- **5초 Pong 타임아웃**: 모바일 네트워크 지연 고려
- **3회 재시도**: 일시적 네트워크 문제 허용
- **20초 Idle 임계값**: 불필요한 Ping 트래픽 최소화

## 에러 처리

### 1. Pong 타임아웃
```rust
async fn handle_pong_timeout(&mut self, seq: u32) -> Result<()> {
    if let Some(mut record) = self.pending_pings.remove(&seq) {
        record.retry_count += 1;
        
        if record.retry_count <= self.max_retries {
            // 재시도
            warn!("Ping {} timed out, retrying ({}/{})", 
                  seq, record.retry_count, self.max_retries);
            self.send_ping_retry(seq, record).await?;
        } else {
            // 연결 실패
            error!("Ping {} failed after {} retries", seq, self.max_retries);
            self.connection_state = ConnectionHealth::Dead;
            self.notify_connection_failure().await?;
        }
    }
    Ok(())
}
```

### 2. 시퀀스 불일치
```rust
fn handle_pong(&mut self, message: Message) -> Result<()> {
    let seq = message.arg0;
    let timestamp = message.arg1;
    
    if let Some(record) = self.pending_pings.remove(&seq) {
        let rtt = record.sent_at.elapsed();
        self.update_connection_health(rtt);
        debug!("Received pong for seq {}, RTT: {:?}", seq, rtt);
        Ok(())
    } else {
        warn!("Received unexpected pong with seq {}", seq);
        // 무시하고 계속 진행
        Ok(())
    }
}
```

### 3. 연결 복구
```rust
async fn attempt_reconnection(&mut self) -> Result<()> {
    info!("Attempting connection recovery...");
    
    // 기존 연결 정리
    self.cleanup_connection().await?;
    
    // 새 연결 시도
    for attempt in 1..=3 {
        match self.establish_connection().await {
            Ok(_) => {
                info!("Connection recovered on attempt {}", attempt);
                self.connection_state = ConnectionHealth::Healthy;
                self.reset_keep_alive_state();
                return Ok(());
            }
            Err(e) => {
                warn!("Reconnection attempt {} failed: {}", attempt, e);
                tokio::time::sleep(Duration::from_secs(attempt * 2)).await;
            }
        }
    }
    
    Err(anyhow!("Failed to recover connection after 3 attempts"))
}
```

## 통계 및 모니터링

### RTT 추적
```rust
struct ConnectionStats {
    total_pings_sent: u64,
    total_pongs_received: u64,
    average_rtt: Duration,
    min_rtt: Duration,
    max_rtt: Duration,
    packet_loss_rate: f32,
    connection_uptime: Duration,
}
```

### 로깅 레벨
- **DEBUG**: 모든 Ping/Pong 메시지
- **INFO**: 연결 상태 변화, 복구 성공
- **WARN**: Ping 타임아웃, 재시도
- **ERROR**: 연결 실패, 복구 실패

## 장점

1. **연결 안정성**: 조기 연결 실패 감지
2. **NAT 투과**: NAT 타임아웃 방지로 안정적 연결 유지
3. **성능 모니터링**: RTT 측정으로 네트워크 품질 추적
4. **자동 복구**: 연결 실패 시 자동 재연결 시도
5. **최소 오버헤드**: 24바이트 헤더만 사용
6. **ADB 호환성**: 기존 ADB 프로토콜과 완전 호환

## 구현 고려사항

1. **메모리 관리**: pending_pings 맵의 크기 제한 필요
2. **타이머 정밀도**: 시스템 타이머 해상도 고려
3. **스레드 안전성**: 다중 스레드 환경에서 상태 동기화
4. **설정 가능성**: 환경별 튜닝을 위한 설정 파일 지원

이 keep-alive 메커니즘을 통해 DBGIF 프로토콜의 연결 안정성과 신뢰성을 크게 향상시킬 수 있습니다.