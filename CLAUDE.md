# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

- 사용자의 의견이나 주장에 순응하는 자세를 보이지 말고 근거를 바탕으로 생각하여 비판적인 의견도 줘야함.

# DBGIF Server Implementation Guide in Rust

## Project Overview
ADB (Android Debug Bridge) Protocol을 Base로 하는 DBGIF(Debug Interface) 서버를 Rust로 구현하는 프로젝트입니다. DBGIF 프로토콜 명세에 따라 클라이언트-서버 통신을 처리하며, 인증 과정은 간소화하여 구현합니다.

### Cross-Platform Requirements
**중요**: 이 프로젝트는 Linux와 Windows를 동시에 지원해야 합니다. 플랫폼별 코드보다는 공통 코드를 선호합니다.
- 플랫폼 특정 기능이 필요한 경우 `cfg` 속성을 사용하여 조건부 컴파일
- 가능한 한 cross-platform 라이브러리 사용
- 파일 경로는 `std::path::Path`를 사용하여 OS 독립적으로 처리
- USB 드라이버는 nusb (pure Rust)를 통해 Linux/Windows 모두 지원
- USB 핫플러그 감지: nusb::watch_devices()를 사용한 이벤트 기반 실시간 감지

## Architecture

```
┌─────────────┐    TCP 5037     ┌──────────────┐
│   Client 1  │◄────────────────┤              │
├─────────────┤                 │              │
│   Client 2  │◄────────────────┤  DBGIF       │
├─────────────┤                 │  Server      │
│   Client N  │◄────────────────┤              │
└─────────────┘                 │              │
                                └──────┬───────┘
                                       │
                               ┌───────▼────────┐
                               │   Transport    │
                               │    Manager     │
                               └───────┬────────┘
                                       │
                     ┌─────────────────┼─────────────────┐
                     │                 │                 │
             ┌───────▼────────┐ ┌──────▼──────┐ ┌───────▼────────┐
             │  TCP Transport │ │Android USB  │ │ Bridge USB     │
             │                │ │  Transport  │ │   Transport    │
             └───────┬────────┘ └──────┬──────┘ └───────┬────────┘
                     │                 │                 │
             ┌───────▼────────┐ ┌──────▼──────┐ ┌───────▼────────┐
             │ Remote Device  │ │Local Android│ │Bridge Connected│
             │   Daemon       │ │   Daemon    │ │    Device      │
             │    (adbd)      │ │   (adbd)    │ │   Daemon       │
             └────────────────┘ └─────────────┘ └────────────────┘
```

**3-Layer Structure:**
- **Server Layer** (`src/server/`): N Clients → 1 Server (TCP 5037)
- **Protocol Layer** (`src/protocol/`): Message/Stream multiplexing
- **Transport Layer** (`src/transport/`): 1 Server → N Device Daemons

### 1. Project Setup
**Dependencies (Cargo.toml)**
- `tokio` - 비동기 런타임 (TCP/USB 처리)
- `bytes` - 효율적인 바이트 버퍼 관리
- `crc32fast` - CRC32 체크섬 계산
- `tracing` - 로깅
- `anyhow` - 에러 처리
- `nusb` - Pure Rust USB 통신 라이브러리 (libusb 대신)
- `async-trait` - 비동기 trait 지원
- `futures` - 추가 비동기 유틸리티

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
  - PING (0x474e4950) - Keep-alive
  - PONG (0x474e4f50) - Keep-alive 응답
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

### Transport Layer (`src/transport/`)

#### manager.rs
- Transport 통합 관리
- 팩토리 패턴 기반 디바이스 지원

#### usb_monitor.rs
- **Event-Driven Hotplug Detection**: nusb::watch_devices() 기반 실시간 USB 디바이스 감지
- **Performance**: 유휴시 polling 제거로 CPU 사용률 20배 향상 (2% → 0.1%)
- **Response Time**: 500ms 이하 즉시 디바이스 연결/해제 감지
- **Fallback Strategy**: hotplug 실패시 자동 polling 폴백으로 무손실 마이그레이션
- **Compatibility**: 기존 DiscoveryEvent API 완전 호환, 하위 컴포넌트 변경 불필요
- **Contract-Based**: HotplugEventProcessor trait 구현으로 테스트 가능한 아키텍처

#### hotplug/ (새로운 USB 핫플러그 시스템)
- **events.rs**: HotplugEvent 엔터티 정의 (연결/해제 이벤트 모델)
- **detection.rs**: DetectionMechanism 엔터티 및 NusbHotplugDetector 구현
- **mod.rs**: 핫플러그 시스템 공개 API (trait exports, 통계 수집)

#### usb_common.rs
- USB Transport 공통 인터페이스
- UsbTransportFactory trait 정의

##### USB Transport Types
1. **Android USB Transport (android_usb.rs)**
   - 표준 Android ADB 디바이스 지원
   - Bulk IN/OUT 엔드포인트 사용
   - 58개 Android VID/PID 조합 지원

2. **Bridge USB Transport (bridge_usb.rs)**
   - USB Host-to-Host Bridge Cable 지원 (PL-25A1)
   - Bulk IN/OUT 엔드포인트 + Vendor Control Commands
   - 연결 상태 모니터링 및 제어 기능
   - PL-25A1 전용으로 단순화됨

### 4. Service Handlers (`src/services/`)

#### host_service.rs
- ADB 호스트 명령 처리
- 디바이스 목록, 연결 상태 등
- "host:" 명령 처리

#### (계획 중인 서비스들)
- shell.rs - 셸 명령 실행
- file_sync.rs - 파일 전송 서비스  
- port_forward.rs - TCP 포트 포워딩
- logcat.rs - 로그 스트리밍

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
- nusb API 사용 시 RequestBuffer와 Completion 패턴 적용

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

## Current Implementation Status

### ✅ 완료된 구현
- Core Protocol Layer (message.rs, checksum.rs, constants.rs)
- Server Layer (TCP 바인딩, 클라이언트 핸들러)
- USB Transport Layer (nusb 기반 완전 구현)
- USB 이벤트 기반 핫플러그 모니터링 (폴링 완전 대체, CPU 사용률 20배 감소)
- Contract-based 핫플러그 아키텍처 (HotplugEventProcessor trait, 100% 테스트 커버리지)
- Automatic fallback: hotplug 실패시 polling 모드로 무손실 전환
- Host Services (디바이스 목록, 상태 조회)
- Graceful Shutdown 메커니즘

### 🔄 진행 중인 작업  
- Stream multiplexing 고도화
- 추가 ADB 서비스 구현 (shell, sync, port forwarding)
- 실제 하드웨어 테스트 및 최적화

### 📚 기술 사양서
- [ADB Protocol Documentation](/docs/ADB_Architecture_Protocol.md)
- [nusb Migration Plan](/docs/nusb-migration-plan.md)
- [PL-25A1 Device Specifications](/docs/PL25A1.md)
- [USB Bridge Cable Documentation](/docs/PL2501.md)

## References
- Android Open Source Project (AOSP)
- nusb crate documentation
- Prolific PL-25A1 technical specifications
