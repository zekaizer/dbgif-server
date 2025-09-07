# Android Debug Bridge (ADB) 아키텍처 및 프로토콜 분석

## 1. ADB 아키텍처 및 작동 원리

### 1.1 ADB 개요
Android Debug Bridge (ADB)는 Android 디바이스와 통신하기 위한 다목적 명령줄 도구입니다. 앱 설치, 디버깅 등 다양한 디바이스 작업을 수행할 수 있으며, Unix 셸에 대한 접근을 제공합니다.

### 1.2 클라이언트-서버 아키텍처
ADB는 3개의 주요 컴포넌트로 구성된 클라이언트-서버 프로그램입니다:

1. **클라이언트 (Client)**
   - 명령을 전송하는 역할
   - 개발 머신에서 실행
   - adb 명령어를 통해 사용자와 상호작용

2. **서버 (Server)** 
   - 클라이언트와 데몬 간 통신을 관리
   - 개발 머신에서 실행
   - TCP 포트 5037에 바인딩되어 클라이언트 명령 수신

3. **데몬 (adbd)**
   - Android 디바이스에서 명령을 실행
   - 백그라운드 프로세스로 실행
   - 실제 디바이스 작업 수행

### 1.3 통신 방식
- **USB 연결**: USB 케이블을 통한 직접 연결
- **TCP 연결**: Wi-Fi를 통한 네트워크 연결 (포트 5555)
- **서버 포트**: 모든 ADB 클라이언트는 포트 5037을 사용하여 서버와 통신

### 1.4 연결 프로세스
1. ADB 서버 시작 시 로컬 TCP 포트 5037에 바인딩
2. 클라이언트가 서버에 명령 전송
3. 서버가 적절한 디바이스의 adbd와 통신
4. adbd가 명령 실행 후 결과 반환

### 1.5 보안 메커니즘
- **USB 디버깅 활성화**: 개발자 옵션에서 명시적 활성화 필요
- **RSA 인증**: Android 4.2.2+ (API level 17)부터 RSA 핑거프린트 확인 대화상자 표시
- **사용자 승인**: 디바이스 사용자의 명시적 승인 없이는 디버깅 불가

## 2. ADB 프로토콜 메시지 포맷 (CNXN, AUTH, OPEN, WRTE 등)

### 2.1 기본 메시지 구조
모든 ADB 메시지는 24바이트 헤더로 구성되며, little-endian 형식의 6개 32비트 정수로 이루어집니다:

```
+----------------+----------------+----------------+----------------+
|    Command     |     Arg0       |     Arg1       |  Data Length   |
|   (4 bytes)    |   (4 bytes)    |   (4 bytes)    |   (4 bytes)    |
+----------------+----------------+----------------+----------------+
| Data Checksum  |     Magic      |           Data Payload         |
|   (4 bytes)    |   (4 bytes)    |        (Variable Length)       |
+----------------+----------------+--------------------------------+
```

### 2.2 헤더 필드 설명
1. **Command**: 메시지 타입을 나타내는 4바이트 값
2. **Arg0**: 첫 번째 인자 (명령에 따라 용도 변경)
3. **Arg1**: 두 번째 인자 (명령에 따라 용도 변경)
4. **Data Length**: 데이터 페이로드의 길이
5. **Data Checksum**: 데이터의 CRC32 체크섬
6. **Magic**: 명령의 비트 NOT 연산 결과 (검증용)

### 2.3 주요 명령어 및 헥스 값

#### CNXN (Connect) - 0x4e584e43
연결 설정 명령
- **용도**: 클라이언트와 디바이스 간 연결 초기화
- **Arg0**: 프로토콜 버전 (VERSION = 0x01000000)
- **Arg1**: 최대 데이터 크기 (MAX_ADB_DATA)
- **Data**: "host::[banner]\0" 형식의 문자열

```
CNXN Message Example:
Command: 0x4e584e43 (CNXN)
Arg0: 0x01000000 (Version)
Arg1: 0x00040000 (Max data size: 256KB)
Data: "host::features=shell_v2,cmd,stat_v2,ls_v2,fixed_push_mkdir"
```

#### AUTH (Authentication) - 0x48545541
인증 메시지
- **용도**: RSA 키 기반 인증 수행
- **타입별 구분**:
  - Type 1 (TOKEN): 서버가 클라이언트에게 랜덤 데이터 전송
  - Type 2 (SIGNATURE): 클라이언트가 서명된 데이터 전송
  - Type 3 (RSAPUBLICKEY): 클라이언트가 RSA 공개키 전송

```
AUTH Message Example:
Command: 0x48545541 (AUTH)
Arg0: 0x00000001 (TOKEN type)
Arg1: 0x00000000
Data: [20 bytes random token]
```

#### OPEN (Open Stream) - 0x4e45504f
스트림 열기 명령
- **용도**: 새로운 통신 스트림 생성
- **Arg0**: local_id (송신자 스트림 ID)
- **Arg1**: 0 (초기값)
- **Data**: "서비스:옵션" 형식

```
OPEN Message Examples:
1. Shell command: "shell:ls -al"
2. File sync: "sync:"
3. Port forwarding: "tcp:8080"
4. Logcat: "shell:logcat"
```

#### OKAY (Ready/Acknowledge) - 0x59414b4f
준비/확인 응답
- **용도**: 메시지 수신 확인 및 스트림 준비 완료 신호
- **Arg0**: local_id
- **Arg1**: remote_id
- **Data**: 일반적으로 비어있음

```
OKAY Message Example:
Command: 0x59414b4f (OKAY)
Arg0: 0x00000001 (local_id)
Arg1: 0x00000002 (remote_id)
Data: (empty)
```

#### WRTE (Write Data) - 0x45545257
데이터 전송 명령
- **용도**: 스트림을 통한 실제 데이터 전송
- **Arg0**: local_id
- **Arg1**: remote_id
- **Data**: 전송할 실제 데이터

```
WRTE Message Example:
Command: 0x45545257 (WRTE)
Arg0: 0x00000001 (local_id)
Arg1: 0x00000002 (remote_id)
Data: "total 64\ndrwxrwxr-x 2 user user 4096 Jan 1 12:00 dir1\n"
```

#### CLSE (Close Stream) - 0x45534c43
스트림 종료 명령
- **용도**: 열린 스트림 종료
- **Arg0**: local_id
- **Arg1**: remote_id
- **Data**: 일반적으로 비어있음

### 2.4 메시지 흐름 및 프로토콜 규칙

#### 연결 핸드셰이크 순서
1. 클라이언트 → 서버: CNXN 메시지 전송
2. 서버 → 클라이언트: AUTH TOKEN 메시지 전송
3. 클라이언트 → 서버: AUTH SIGNATURE 메시지 전송
4. 서버 → 클라이언트: CNXN 메시지로 연결 확인

#### 스트림 통신 규칙
1. OPEN 또는 WRTE 전송 후 OKAY 응답 대기 필수
2. 스트림 멀티플렉싱을 위한 local_id/remote_id 사용
3. 최대 메시지 데이터 크기: 256KB (MAXDATA = 256 * 1024)
4. CNXN과 AUTH 메시지는 4096바이트로 제한

#### USB 구현 주의사항
- USB 연결 시 메시지 헤더와 데이터를 별도 USB 트랜잭션으로 전송 필요
- 단일 패킷으로 헤더+데이터 동시 전송 불가

### 2.5 보안 고려사항
- **암호화 부재**: ADB 통신은 기본적으로 암호화되지 않음
- **LAN 사용 시 주의**: 민감한 정보 노출 위험
- **인증 필수**: Android 4.2.2+ 에서 RSA 핑거프린트 확인 필요
- **권한 관리**: 명시적 사용자 승인 없이는 디바이스 접근 불가

## 참고 자료
- Android Open Source Project (AOSP) - ADB Protocol Documentation
- Google Python-ADB Implementation
- Synacktiv - Diving into ADB Protocol Internals
- Android Developers Documentation - Android Debug Bridge