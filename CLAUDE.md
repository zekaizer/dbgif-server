# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# DBGIF Server Implementation Guide in Rust

## Project Overview
ADB (Android Debug Bridge) Protocol을 Base로 하는 DBGIF(Debug Interface) 서버를 Rust로 구현하는 프로젝트입니다. DBGIF 프로토콜 명세에 따라 클라이언트-서버 통신을 처리하며, 인증 과정은 간소화하여 구현합니다.

## Architecture

### 1. Project Setup
**Dependencies (Cargo.toml)**
- `tokio` - 비동기 런타임 (TCP/USB 처리)
- `bytes` - 효율적인 바이트 버퍼 관리
- `crc32fast` - CRC32 체크섬 계산
- `tracing` - 로깅
- `anyhow` - 에러 처리

### 2. Core Protocol Module (`src/protocol/`)

#### message.rs
- DBGIF 메시지 구조체 (24바이트 헤더)
- Command enum 정의:
  - CNXN (0x4e584e43) - 연결
  - AUTH (0x48545541) - 인증 (간소화 처리)
  - OPEN (0x4e45504f) - 스트림 열기
  - OKAY (0x59414b4f) - 확인 응답
  - WRTE (0x45545257) - 데이터 전송
  - CLSE (0x45534c43) - 스트림 종료
  - Ping (PING: 0x474e4950) - Keep-alive
  - Pong (PONG: 0x474e4f50) - Keep-alive 응답
- 메시지 직렬화/역직렬화
- Magic value 검증 (command의 비트 NOT 연산)

#### constants.rs
- `MAXDATA`: 256KB (256 * 1024)
- `VERSION`: 0x01000000
- `DEFAULT_PORT`: 5037

#### checksum.rs
- CRC32 체크섬 구현
- 데이터 무결성 검증

### 3. Connection Management (`src/connection/`)

#### server.rs
- TCP 서버 (포트 5037 바인딩)
- 클라이언트 연결 수락
- 비동기 연결 처리

#### client_handler.rs
- 다중 클라이언트 동시 처리
- 메시지 라우팅
- 연결 생명주기 관리

#### stream.rs
- 스트림 멀티플렉싱
- local_id/remote_id 매핑
- 스트림별 버퍼 관리

#### usb.rs
- USB 통신 레이어 (libusb-rs 사용)
- USB 트랜잭션 처리
- 헤더와 데이터 분리 전송

##### USB Transport Types
1. **기본 USB Transport**
   - Bulk IN/OUT 엔드포인트 사용
   - 표준 ADB USB 통신

2. **USB Host-to-Host Bridge Cable Transport**
   - USB Host-to-Host Bridge Cable을 통한 직접 연결
   - Bulk IN/OUT 엔드포인트 + Interrupt IN 엔드포인트 사용
   - Interrupt IN을 통해 상대편 연결 상태 모니터링
   - 연결 상태 변화 감지 및 실시간 알림

### 4. Service Handlers (`src/services/`)

#### shell.rs
- 셸 명령 실행
- 명령 형식: "shell:command"
- 출력 스트리밍

#### file_sync.rs
- 파일 전송 서비스
- 명령 형식: "sync:"
- 파일 업로드/다운로드

#### port_forward.rs
- TCP 포트 포워딩
- 명령 형식: "tcp:port"
- 로컬/리모트 포트 매핑

#### logcat.rs
- 로그 스트리밍
- 명령 형식: "shell:logcat"
- 실시간 로그 전송

### 5. State Machine (`src/state/`)

#### Connection States
```
Disconnected → Connecting → Connected
```

#### Stream States
```
Closed → Opening → Open → Closing
```

#### 상태 전이 규칙
- OPEN 후 OKAY 응답 대기
- WRTE 후 OKAY 응답 대기
- CLSE 수신 시 스트림 정리

### 6. Main Application (`src/main.rs`)
- 서버 초기화 및 시작
- 이벤트 루프 실행
- 시그널 처리 (graceful shutdown)
- 로깅 설정

## Protocol Flow

### Connection Handshake (인증 간소화)
1. Client → Server: CNXN 메시지
2. Server → Client: AUTH TOKEN (무시/스킵 가능)
3. Client → Server: AUTH SIGNATURE (검증 없이 통과)
4. Server → Client: CNXN 응답

### Stream Communication
1. Client → Server: OPEN (서비스 요청)
2. Server → Client: OKAY (스트림 준비 완료)
3. Client ↔ Server: WRTE (데이터 교환)
4. Either → Other: CLSE (스트림 종료)

## Implementation Notes

### 메시지 처리 주의사항
- 모든 메시지는 little-endian 형식
- USB 통신 시 헤더와 데이터 분리 전송 필수
- 최대 메시지 크기: 256KB
- CNXN/AUTH 메시지는 4096 바이트 제한

### 에러 처리
- 잘못된 magic value 검증
- 체크섬 불일치 감지
- 스트림 ID 충돌 방지
- 연결 타임아웃 처리

## Testing Strategy

### Unit Tests
- 메시지 직렬화/역직렬화
- 체크섬 계산
- 상태 전이 로직

### Integration Tests
- 전체 핸드셰이크 시퀀스
- 다중 스트림 처리
- 서비스 핸들러 동작

### Mock Client
- 프로토콜 준수 테스트
- 성능 벤치마크
- 에러 시나리오 테스트

## Development Commands
```bash
# Build
cargo build

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run

# Format code
cargo fmt

# Lint
cargo clippy
```

## References
- [ADB Protocol Documentation](/docs/ADB_Architecture_Protocol.md)
- Android Open Source Project (AOSP)
- ADB Protocol Internals
