# Simple Testing: USB Hotplug

## 기본 테스트

### 1. 서버 실행하고 USB 꽂기/빼기
```bash
# 서버 실행
cargo run

# 로그 확인
tail -f target/debug/dbgif-server.log

# USB 디바이스 꽂기/빼기 하면서 로그 보기
# "Device connected" / "Device disconnected" 메시지 확인
```

### 2. 디바이스 목록 확인
```bash
# 연결된 디바이스 확인
curl http://localhost:5037/host:devices

# USB 꽂기/빼기 후 다시 확인해서 변화 확인
```

### 3. 성능 확인
```bash
# CPU 사용률 확인 (폴링 없으면 낮아야 함)
top -p $(pgrep dbgif-server)
```

## 예상 결과
- USB 꽂으면 즉시 로그에 나타남 (1초 기다릴 필요 없음)
- USB 빼면 즉시 로그에 나타남
- 아무것도 안할 때 CPU 사용률 낮음

## 문제 발생시
- 로그에 "Hotplug 안됨, 폴링 계속" 나오면 → 권한 문제일 수 있음
- Linux에서 안되면 → udev 규칙 확인
- 그래도 안되면 → 기존 폴링 방식으로 작동함 (문제없음)

끝.