# Contributing to DBGIF Server

DBGIF Server는 멀티 트랜스포트 ADB 프로토콜 대몬입니다. 기여해주셔서 감사합니다!

## Development Process

### Prerequisites
- Rust 1.75+
- Git
- Hardware for testing (선택사항):
  - PL25A1 USB bridge cable
  - Android device with USB debugging
  - Network-connected ADB device

### Setup
```bash
git clone <repository-url>
cd dbgif-server
cargo build
cargo test
```

## Code Style

### Formatting
```bash
# 자동 포맷팅 적용
cargo fmt

# 포맷팅 검사
cargo fmt -- --check
```

### Linting
```bash
# Clippy 검사
cargo clippy -- -D warnings

# 모든 검사
cargo clippy --all-targets --all-features -- -D warnings
```

## Testing Strategy

### TDD (Test-Driven Development) - 필수
1. **RED**: 테스트 작성 및 실패 확인
2. **GREEN**: 최소한의 구현으로 테스트 통과
3. **REFACTOR**: 코드 정리 및 최적화

### Test Categories
- **Contract Tests** (`tests/contract/`): 인터페이스 계약 검증
- **Integration Tests** (`tests/integration/`): 실제 하드웨어 연동 테스트
- **E2E Tests** (`tests/e2e/`): 전체 시나리오 테스트
- **Unit Tests** (`tests/unit/`): 개별 함수/모듈 테스트

### Running Tests
```bash
# 전체 테스트
cargo test

# 특정 카테고리
cargo test --test test_transport_trait
cargo test integration

# 하드웨어 테스트 (선택사항)
cargo test --features hardware-tests
```

## Code Organization

### Transport Layer
- `src/transport/mod.rs`: Transport trait 정의
- `src/transport/tcp.rs`: TCP transport 구현
- `src/transport/usb_device.rs`: USB device transport 구현
- `src/transport/usb_bridge.rs`: USB bridge transport 구현

### Protocol Layer
- `src/protocol/`: ADB 프로토콜 메시지 처리
- `src/session/`: ADB 세션 및 스트림 관리

### Server Layer
- `src/server/`: TCP 서버 및 클라이언트 핸들링
- `src/daemon.rs`: 메인 대몬 로직

## Cross-Platform Support

### Windows Considerations
- WinUSB driver requirements for USB bridge
- Windows-specific USB path handling
- Conditional compilation with `#[cfg(windows)]`

### Linux Considerations
- udev rules for USB device permissions
- Linux-specific USB path handling
- Conditional compilation with `#[cfg(unix)]`

## Pull Request Process

### Before Submitting
1. 모든 테스트 통과: `cargo test`
2. 포맷팅 적용: `cargo fmt`
3. Lint 검사: `cargo clippy -- -D warnings`
4. 문서 업데이트 (필요시)

### PR Description
- 변경사항 설명
- 테스트 시나리오
- 하드웨어 요구사항 (해당시)
- Breaking changes 여부

### Review Criteria
- TDD 프로세스 준수
- 코드 스타일 일관성
- 크로스 플랫폼 호환성
- 성능 영향 분석
- 문서 완성도

## Error Handling

### Fail-Fast Principle
- 자동 재시도 없음
- 명확한 에러 메시지
- 적절한 에러 타입 사용

### Error Types
```rust
use anyhow::{Context, Result};

// Good
fn connect_device() -> Result<()> {
    transport.connect()
        .context("Failed to connect to USB device")?;
    Ok(())
}

// Avoid
fn connect_device() -> bool {
    match transport.connect() {
        Ok(_) => true,
        Err(_) => false, // 에러 정보 손실
    }
}
```

## Performance Guidelines

### USB Performance
- 256KB buffer sizes for bulk transfers
- Zero-copy operations where possible
- Asynchronous I/O with tokio

### Memory Management
- Buffer pooling for frequent allocations
- Avoid cloning large data structures
- Use `Bytes` for efficient buffer sharing

## Documentation

### Code Documentation
```rust
/// Creates a new TCP transport for the specified host and port.
///
/// # Arguments
/// * `host` - Target hostname or IP address
/// * `port` - Target port number (typically 5555 for ADB)
///
/// # Returns
/// A configured TCP transport ready for connection
///
/// # Examples
/// ```
/// let transport = TcpTransport::new("192.168.1.100".to_string(), 5555);
/// ```
pub fn new(host: String, port: u16) -> Self {
```

### Commit Messages
```
feat(transport): add USB bridge transport implementation

- Implement PL25A1 specific vendor commands
- Add connection state monitoring
- Support Windows and Linux platforms

Closes #123
```

## Debugging

### Logging Levels
```bash
# Debug 모든 모듈
RUST_LOG=debug cargo run

# Transport만 trace
RUST_LOG=dbgif_server::transport=trace cargo run

# 특정 모듈만
RUST_LOG=dbgif_server::transport::usb_bridge=debug cargo run
```

### Hardware Debugging
- USB 연결 상태 확인: `lsusb` (Linux), Device Manager (Windows)
- 네트워크 연결 확인: `telnet <host> 5555`
- ADB 호환성 테스트: `adb connect <host>:5555`

## Release Process

### Version Numbering
- MAJOR.MINOR.PATCH (Semantic Versioning)
- Breaking changes increment MAJOR
- New features increment MINOR
- Bug fixes increment PATCH

### Release Checklist
- [ ] All tests passing
- [ ] Documentation updated
- [ ] Changelog updated
- [ ] Version number bumped
- [ ] Cross-platform testing completed

## Getting Help

- **Issues**: GitHub Issues에서 버그 리포트 및 기능 요청
- **Discussions**: 일반적인 질문 및 아이디어 논의
- **Documentation**: `docs/` 디렉토리의 기술 문서 참조

기여해주셔서 감사합니다! 🦀