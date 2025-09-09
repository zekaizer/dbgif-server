# PL-25A1
VID, PID: 0x067b 0x25a1

## Contoll transfer vendor type

### Vendor IN
 - 0xF7: ??, 2byte, 어떤 상태 or 피쳐를 나타내는것 같음, [0x06, 0x80], 0xF8(Vendor OUT)과 연관된 것으로 보임
 - 0xFB: 2byte, local(first byte), remote(second byte) state
   - bits
     - DISCONNECTED(0x02): 물리적 커넥션 상태
     - READY(0x04): 장치가 열거되어 준비 완료
     - CONNNECTOR_ID(0x08): 케이블의 2개의 커넥터중 어느쪽 인지알려주는 비트로 보임, 케이블 위치에 따라 고정된값

### Vendor OUT
 - 0xF8: ??, 2byte, Vendor IN 0xF7과 연관됨. 정확한 비트필드의 의미를 알고싶고 사용 케이스도 알고싶음.
 - 0xF9: power off
 - 0xFA: reset

## Bulk IN/OUT
실제 데이터 전송에 사용하는 ep

## Interrupt IN
아직 어떤 기능인지 확인 안됨, Vendor 0xF7, 0xF8과 관련이 있다고 추정중. 대기해도 아무것도 들어오지 않음.
어떻게 사용하면 되는건지 궁금함.