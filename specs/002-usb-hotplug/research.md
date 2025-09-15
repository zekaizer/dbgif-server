# Simple Research: nusb Hotplug

## 핵심 발견사항

### nusb::watch_devices() 기본 사용법
```rust
// 기본 패턴
let watch = nusb::watch_devices()?;
while let Some(event) = watch.next().await {
    match event {
        HotplugEvent::Connected(device) => {
            // 연결됨
        }
        HotplugEvent::Disconnected(id) => {
            // 해제됨
        }
    }
}
```

### 크로스 플랫폼
- Linux: udev 기반, 잘 작동
- Windows: WinUSB 기반, 잘 작동
- 둘 다 동일한 API

### tokio 통합
```rust
// 현재 DiscoveryCoordinator의 discovery_loop() 대신:
tokio::spawn(async move {
    while let Some(event) = hotplug_watch.next().await {
        // 기존 DiscoveryEvent로 변환해서 전송
    }
});
```

### 간단한 폴백
```rust
let watch = match nusb::watch_devices() {
    Ok(w) => Some(w),
    Err(_) => {
        tracing::warn!("Hotplug 안됨, 폴링 계속");
        None
    }
};
```

## 구현 방향
1. `DiscoveryCoordinator::discovery_loop()` 수정
2. 폴링 타이머 대신 hotplug 이벤트 사용
3. 실패시 기존 폴링으로 폴백
4. 끝.