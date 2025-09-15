# Phase 1: Data Model & Entities

## Core Entities

### Transport Connection
Represents a generalized connection to any transport type (TCP, USB Device, USB Bridge).

**Fields**:
- `transport_type`: TransportType - Type of transport (TCP/USB Device/USB Bridge)
- `device_id`: String - Unique identifier for this connection
- `connection_state`: ConnectionState - Current connection status
- `transport_config`: TransportConfig - Transport-specific configuration
- `last_activity`: Instant - Timestamp of last communication
- `error_count`: u32 - Number of consecutive errors

**Transport-Specific Configurations**:
- **TCP Config**: `host: String, port: u16`
- **USB Device Config**: `vid: u16, pid: u16, serial: Option<String>`
- **USB Bridge Config**: `vid: u16, pid: u16, connector_info: BridgeConnectorInfo`

**State Transitions**:
```
Disconnected → Connecting → Connected → Disconnecting → Disconnected
```

**Validation Rules**:
- Transport type must be valid enum value
- Device ID must be unique within daemon instance
- Connection state must be valid before data transfer
- Transport-specific validation (VID/PID for USB, host/port for TCP)

### ADB Session
Encapsulates an ADB protocol communication session.

**Fields**:
- `session_id`: Uuid - Unique session identifier
- `version`: u32 - ADB protocol version (0x01000000)
- `max_data_size`: u32 - Maximum data payload size (256KB)
- `auth_state`: AuthState - Authentication status
- `banner`: String - Connection banner string
- `features`: Vec<String> - Supported ADB features
- `streams`: HashMap<u32, DataStream> - Active data streams

**State Transitions**:
```
Connecting → Authenticating → Connected → Disconnecting → Disconnected
```

**Validation Rules**:
- Version must match supported ADB protocol versions
- Max data size must not exceed 256KB
- Banner string must be valid UTF-8
- Stream IDs must be unique within session

### Data Stream
Individual multiplexed communication channel within an ADB session.

**Fields**:
- `local_id`: u32 - Local stream identifier
- `remote_id`: u32 - Remote stream identifier
- `service`: String - Service name (e.g., "shell", "sync")
- `state`: StreamState - Current stream status
- `send_buffer`: VecDeque<Bytes> - Outgoing message queue
- `receive_buffer`: VecDeque<Bytes> - Incoming message queue
- `window_size`: u32 - Flow control window size

**State Transitions**:
```
Closed → Opening → Open → Closing → Closed
```

**Validation Rules**:
- Local ID must be assigned by local host
- Remote ID assigned after OKAY response
- Service name must be valid ASCII
- Buffer sizes must not exceed memory limits

### ADB Message
Protocol message structure for ADB communication.

**Fields**:
- `command`: Command - Message type (CNXN, AUTH, OPEN, etc.)
- `arg0`: u32 - First argument (context-dependent)
- `arg1`: u32 - Second argument (context-dependent)
- `data_length`: u32 - Payload data length
- `data_checksum`: u32 - CRC32 checksum of payload
- `magic`: u32 - Command verification (bitwise NOT of command)
- `payload`: Bytes - Message payload data

**Message Types**:
```rust
pub enum Command {
    CNXN = 0x4e584e43,
    AUTH = 0x48545541,
    OPEN = 0x4e45504f,
    OKAY = 0x59414b4f,
    WRTE = 0x45545257,
    CLSE = 0x45534c43,
    PING = 0x474e4950,
    PONG = 0x474e4f50,
}
```

**Validation Rules**:
- Magic field must equal bitwise NOT of command
- Data checksum must match CRC32 of payload
- Data length must match payload size
- Command must be valid enum value

### Device Discovery Info
Information about discovered devices across all transport types.

**Fields**:
- `device_id`: String - Unique device identifier
- `transport_type`: TransportType - How to connect to this device
- `display_name`: String - Human-readable device name
- `connection_info`: ConnectionInfo - Transport-specific connection details
- `device_state`: DeviceState - Current availability status
- `capabilities`: Vec<String> - Supported features (if known)

**Connection Info Variants**:
- **TCP**: `host: String, port: u16`
- **USB Device**: `vid: u16, pid: u16, serial: String, path: String`
- **USB Bridge**: `vid: u16, pid: u16, path: String, bridge_type: String`

**Device States**:
- `Available` - Device detected and ready for connection
- `Connected` - Currently connected to this daemon
- `Busy` - Connected to another ADB instance
- `Unauthorized` - Requires user authorization
- `Offline` - Previously seen but not currently available

**Validation Rules**:
- Device ID must be unique across all transport types
- Connection info must match transport type
- Display name must be valid UTF-8
- Capabilities must be valid feature strings

### Transport Error
Structured error types for transport layer failures.

**Fields**:
- `error_type`: ErrorType - Category of error
- `message`: String - Human-readable error description
- `context`: HashMap<String, String> - Additional error context
- `timestamp`: Instant - When error occurred
- `recoverable`: bool - Whether error allows retry

**Error Types**:
```rust
pub enum ErrorType {
    UsbCommunication,
    ProtocolViolation,
    ConnectionLost,
    ChecksumMismatch,
    Timeout,
    HardwareFailure,
}
```

**Validation Rules**:
- Error type must be valid enum value
- Message must be non-empty
- Context keys must be valid identifiers
- Timestamp must be recent (within reasonable bounds)

## Relationships

### Session ↔ Streams
- One ADB session contains multiple data streams
- Streams share session authentication and protocol version
- Stream lifecycle managed by session state

### Connection ↔ Session
- One USB bridge connection supports one ADB session
- Session cannot exist without active connection
- Connection loss terminates all associated streams

### Messages ↔ Streams
- Messages are routed to specific streams via stream IDs
- CNXN/AUTH messages operate at session level
- OPEN/WRTE/CLSE messages operate at stream level

### Transport ↔ Connection
- Transport trait abstracts connection implementation
- PL25A1 transport implements USB-specific connection logic
- Connection state reflects transport-specific status

## Data Flow

### Connection Establishment
1. USB device detection and enumeration
2. PL25A1 vendor command initialization
3. Connection status monitoring via interrupt endpoint
4. ADB session handshake (CNXN ↔ CNXN)

### Stream Communication
1. Stream creation (OPEN → OKAY)
2. Data transfer (WRTE ↔ OKAY)
3. Stream termination (CLSE)
4. Multiplexed routing based on stream IDs

### Error Propagation
1. Transport errors bubble up to connection level
2. Protocol errors terminate affected streams
3. Connection errors terminate entire session
4. All errors logged with structured context