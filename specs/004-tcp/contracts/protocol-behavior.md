# Protocol Behavior Contract

## DBGIF Protocol Compliance

### Connection Handshake Sequence

**Expected Flow**:
1. Client → Server: CNXN message
2. Server → Client: AUTH message (skip validation)
3. Client → Server: AUTH response (dummy signature)
4. Server → Client: CNXN ACK

**Contract Requirements**:
- All messages must have valid magic values
- Checksums must be calculated correctly
- Message sizes must not exceed MAXDATA (256KB)
- Handshake must complete within timeout period

### Message Format Validation

**CNXN Message Contract**:
```rust
Message {
    command: Command::CNXN,
    arg0: VERSION,        // 0x01000000
    arg1: MAXDATA,        // 256 * 1024
    data_length: payload_len,
    data_crc32: computed_checksum,
    magic: !command as u32,  // Bitwise NOT of command
}
```

**Response Validation**:
- Server response must echo client's VERSION and MAXDATA
- Magic field must be bitwise NOT of command
- CRC32 must match payload data
- Command field must be valid enum value

### Host Command Contracts

#### host:version
**Request**: OPEN stream with "host:version" service
**Expected Response**: Version string (e.g., "4.4.3")
**Validation**:
- Response must be valid UTF-8 string
- Version format should match semantic versioning pattern
- Response time should be < 1 second

#### host:devices
**Request**: OPEN stream with "host:devices" service
**Expected Response**: Device list or empty list message
**Validation**:
- Response format: tab-separated values or "List of N devices"
- Must handle empty device list gracefully
- Response parsing must not fail on various formats

#### host:track-devices
**Request**: OPEN stream with "host:track-devices" service
**Expected Response**: "OK" acknowledgment
**Validation**:
- Initial OK response required
- Stream should remain open for device events
- Client can close stream after receiving OK

### Error Handling Contracts

#### Connection Failures
**Scenarios**:
- Server not running → Connection refused
- Wrong port → Connection refused
- Network unreachable → Timeout

**Expected Behavior**:
- Clear error messages with context
- Proper error codes (connection vs protocol errors)
- No hanging connections

#### Protocol Violations
**Invalid Magic Value**:
- Detect incorrect magic field
- Report expected vs actual values
- Terminate connection gracefully

**Checksum Mismatch**:
- Validate all incoming message checksums
- Report checksum errors with hex values
- Allow test to continue with other connections

**Malformed Messages**:
- Handle truncated messages
- Detect oversized payloads
- Report specific parsing failures

### Timeout Contracts

#### Connection Timeout
- Default: 5 seconds for initial TCP connection
- Configurable via CLI option
- Clear timeout error messages

#### Response Timeout
- Default: 3 seconds for protocol responses
- Per-message timeout tracking
- Distinguish between connection and response timeouts

#### Handshake Timeout
- Complete handshake within 10 seconds
- Track individual message response times
- Fail fast on handshake timeout

### Concurrent Connection Behavior

#### Resource Management
- Each connection uses independent TCP stream
- No shared state between connections
- Proper cleanup on connection failures

#### Timing Constraints
- Concurrent connections should not interfere
- Total test time should be < max(individual_times) + overhead
- Sequential mode should be sum of individual times

#### Error Isolation
- Failure in one connection doesn't affect others
- Each connection reports independent status
- Overall test succeeds if majority connections pass

### Protocol Message Tracing

#### Message Logging
**Sent Messages**:
```
[SEND] CNXN: version=0x01000000, maxdata=262144, magic=0xb3b2b1b0
```

**Received Messages**:
```
[RECV] AUTH: arg0=1, arg1=0, data_len=20, magic=0xb7b5b4b8
```

#### Timing Information
- Log timestamp for each message
- Calculate and report response times
- Track total handshake duration

### Compliance Verification

#### Mandatory Checks
- [ ] Message format validation
- [ ] Magic value verification
- [ ] Checksum calculation and validation
- [ ] Timeout handling
- [ ] Error reporting
- [ ] Resource cleanup

#### Optional Enhancements
- Message sequence validation
- Protocol version negotiation
- Advanced error diagnostics

**Implementation Note**: 기존 `src/protocol/` 모듈의 구현을 준수하여 서버와의 호환성 보장