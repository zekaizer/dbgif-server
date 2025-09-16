# dbgif Protocol Specification

## Overview

dbgif uses an ADB-like protocol for communication between clients and the server. The protocol operates over TCP connections and provides a simple command-response interface.

## Message Format

All messages follow the ADB packet structure:

```
struct Message {
    command: u32,     // Command code (little-endian)
    arg0: u32,        // First argument (little-endian)
    arg1: u32,        // Second argument (little-endian)
    data_length: u32, // Length of data payload (little-endian)
    data_crc32: u32,  // CRC32 of data payload (little-endian)
    magic: u32,       // Magic number (~command, bitwise NOT)
    data: [u8],       // Variable length data payload
}
```

Total header size: 24 bytes

## Commands

### CNXN (0x4E584E43)
Connection establishment from client to server.

**Arguments:**
- arg0: Protocol version (0x01000000)
- arg1: Maximum message length

**Data:** Device identifier string

**Response:** CNXN with server information

### OPEN (0x4E45504F)
Open a new stream for communication.

**Arguments:**
- arg0: Local stream ID
- arg1: 0 (remote stream ID unknown at this point)

**Data:** Service name (e.g., "shell:", "tcp:8080")

**Response:** OKAY (success) or CLSE (failure)

**Note:** OPEN is sent to initiate a new stream. The sender allocates a local stream ID but doesn't know the remote stream ID yet. The remote side will provide its stream ID in the OKAY response.

### OKAY (0x59414B4F)
Success acknowledgment.

**Arguments:**
- arg0: Local stream ID (responder's own stream ID)
- arg1: Remote stream ID (original sender's stream ID from OPEN)

**Data:** None

**Note:** When responding to OPEN, the receiver allocates its own stream ID (arg0) and includes the sender's stream ID from the OPEN command (arg1). This establishes the bidirectional stream mapping.

### WRTE (0x45545257)
Write data to an open stream.

**Arguments:**
- arg0: Local stream ID
- arg1: Remote stream ID

**Data:** Payload to write

**Response:** OKAY (acknowledgment)

### CLSE (0x45534C43)
Close an open stream.

**Arguments:**
- arg0: Local stream ID
- arg1: Remote stream ID

**Data:** None

### PING (0x474E4950)
Ping request to test connection.

**Arguments:**
- arg0: Connect ID (from device's ro.connect.id banner property)
- arg1: Random token for this ping

**Data:** None

**Response:** PONG with same connect ID and token

### PONG (0x474E4F50)
Ping response.

**Arguments:**
- arg0: Connect ID (from PING)
- arg1: Random token (from PING)

**Data:** None

## Connection Flow

### Client-Server Connection
1. Client connects to server via TCP
2. Client sends CNXN with device info
3. Server responds with CNXN
4. Client can now send OPEN/PING commands
5. For each OPEN, server responds with OKAY/CLSE
6. Data exchange via WRTE/OKAY pairs
7. Stream closed with CLSE

### Device-Daemon Connection Flow
1. Server initiates connection to device daemon via TCP/USB
2. Server sends CNXN with b"RESET\x00" to reset device state
3. Device daemon sends CNXN with device identifier and capabilities
4. Server responds with CNXN acknowledgment (arg0 contains protocol version)
5. Server registers device in internal device list
6. Device daemon can now receive forwarded commands from clients
7. Device maintains connection via periodic PING/PONG (every 1 seconds, 3 second timeout)
8. On disconnect, server removes device from available list

### Three-way Communication Flow
When a client wants to communicate with a device:

1. **Client Request**: Client sends OPEN to server with service name
2. **Device Lookup**: Server finds target device by ID/capabilities
3. **Forward Request**: Server forwards OPEN to device daemon
4. **Device Response**: Device daemon responds with OKAY/CLSE
5. **Relay Response**: Server relays device response to client
6. **Stream Establishment**: If successful, bidirectional stream is created
7. **Data Forwarding**: All WRTE commands are forwarded between client and device
8. **Stream Closure**: Either side can close with CLSE, forwarded to other side

```
Client ----OPEN(arg0=1, arg1=0)--------> Server ----OPEN(arg0=1, arg1=0)--------> Device
Client <---OKAY(arg0=1, arg1=100)------- Server <---OKAY(arg0=100, arg1=1)------- Device
Client ----WRTE(arg0=1, arg1=100)------> Server ----WRTE(arg0=1, arg1=100)------> Device
Device ----OKAY(arg0=100, arg1=1)------> Server ----OKAY(arg0=100, arg1=1)------> Client
Client ----CLSE(arg0=1, arg1=100)------> Server ----CLSE(arg0=1, arg1=100)------> Device
```

## Device Registration

When a device daemon connects, it must provide identification and capability information in the CNXN command:

**CNXN Data Format for Devices:**
```
<systemtype>:<device_serialno>:<banner>
```

**Example:**
```
tizen:custom001:banner ro.product.model=MyBoard;ro.build.version=v1.2;ro.connect.id=0x12345678;
```

**Fields:**
- `systemtype`: Device system type (e.g., tizen, seret)
- `device_serialno`: Unique device serial number
- `banner`: Properties string in format "banner ro.property1=value;...;ro.propertyx=value;"

**Common Banner Properties:**
- `ro.product.model`: Device model name
- `ro.build.version`: Firmware/software version
- `ro.connect.id`: Connection identifier (32-bit value, used in PING/PONG)

## Stream Forwarding

The server maintains stream mapping between clients and devices:

**Stream ID Management:**
- Each client session has its own stream ID space (1-65535)
- Each device connection has its own stream ID space (1-65535)
- Multiple clients can use the same stream IDs (e.g., Client A stream 1, Client B stream 1)
- Server maintains mapping: (ClientSession, ClientStreamID) ↔ (Device, DeviceStreamID)
- Server forwards messages without modifying stream IDs
- Stream ID 0 is reserved and not used
- Stream IDs are reused after CLSE confirmation from both sides
- No ID conflicts due to independent ID spaces per session

**Stream Mapping (not translation):**
- Server maintains bidirectional mapping table
- Maps (ClientSession, ClientStreamID) to (Device, DeviceStreamID)
- Messages are forwarded without ID modification
- OKAY responses preserve original local and remote IDs
- Example: Client1 stream 1 ↔ Device stream 100, Client2 stream 1 ↔ Device stream 200

**Multi-client Support:**
- Multiple clients can connect to the same device simultaneously
- Each client session has completely independent stream ID space
- Client A stream 1 and Client B stream 1 are different streams
- Server maps each (ClientSession, StreamID) pair to corresponding device stream
- Server ensures proper isolation between client sessions
- Example: ClientA:1 → Device:100, ClientB:1 → Device:200

**Forwarding Rules:**
1. OPEN: Server finds target device and creates new stream mapping
2. WRTE: Server forwards data using mapped stream IDs
3. OKAY: Server forwards acknowledgments to appropriate endpoints
4. CLSE: Server closes both client and device streams, removes mapping

## Error Handling

- Invalid commands are ignored
- Failed operations return CLSE
- Connection drops terminate all streams
- CRC32 validation on data payloads
- Network timeouts result in connection termination
- Malformed packets are discarded without response

## Concurrency and Error Recovery

**Concurrent Operations:**
- Multiple OPEN commands from same client are processed sequentially
- Each stream operates independently once established
- Device processing order is not guaranteed
- Stream ID conflicts are prevented by server allocation

**Error Recovery:**
- Failed WRTE requires stream closure and re-opening
- Partial data transmission results in CLSE
- Connection drops invalidate all associated streams
- Stream state mismatches are resolved by sending CLSE

**Connection Lifecycle:**
- Normal disconnect: Client/Device sends appropriate close commands
- Abnormal disconnect: Server detects timeout and cleans up resources
- Reconnection: Previous streams are invalidated, new handshake required

## Differences from ADB

- AUTH command not implemented (no authentication)
- PING/PONG added for connection testing
- Simplified service discovery
- No encryption support
