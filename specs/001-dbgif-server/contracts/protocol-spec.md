# DBGIF Protocol Contract Specification (ADB-like)

**Version**: 1.0.0
**Date**: 2025-09-16
**Transport**: TCP (with USB abstraction for future)
**Reference**: docs/protocol.md

## Protocol Overview

DBGIF uses an ADB-like protocol for communication between clients and the server. This is **NOT** fully ADB-compatible but uses similar message structure and concepts. The protocol operates over TCP connections and provides a simple command-response interface with stream multiplexing.

**Architecture**:
```
Client ↔ DBGIF Server ↔ TCP Transport ↔ Device
```

## Message Format

All messages follow the ADB packet structure with exactly 24 bytes header + variable data:

```rust
struct AdbMessage {
    command: u32,     // Command code (little-endian)
    arg0: u32,        // First argument (little-endian)
    arg1: u32,        // Second argument (little-endian)
    data_length: u32, // Length of data payload (little-endian)
    data_crc32: u32,  // CRC32 of data payload (little-endian)
    magic: u32,       // Magic number (~command, bitwise NOT)
    data: [u8],       // Variable length data payload
}
```

**Header Layout** (24 bytes):
```
+-------+-------+-------+-------+-------+-------+-------+-------+
| CMD   | CMD   | CMD   | CMD   | ARG0  | ARG0  | ARG0  | ARG0  |
| (u32 little-endian)           | (u32 little-endian)           |
+-------+-------+-------+-------+-------+-------+-------+-------+
| ARG1  | ARG1  | ARG1  | ARG1  | DLEN  | DLEN  | DLEN  | DLEN  |
| (u32 little-endian)           | (u32 little-endian)           |
+-------+-------+-------+-------+-------+-------+-------+-------+
| CRC32 | CRC32 | CRC32 | CRC32 | MAGIC | MAGIC | MAGIC | MAGIC |
| (u32 little-endian)           | (u32 little-endian)           |
+-------+-------+-------+-------+-------+-------+-------+-------+
|                    DATA (variable length)                     |
+-------+-------+-------+-------+-------+-------+-------+-------+
```

## Commands

### CNXN (0x4E584E43)
Connection establishment between client and server.

**Direction**: Bidirectional (handshake)

**Arguments**:
- arg0: Protocol version (0x01000000)
- arg1: Maximum message length

**Data**: Device/client identifier string

**Client → Server Example**:
```
CNXN(0x01000000, 0x00100000, "client:debugger:debug-client v1.0;")
```

**Server → Client Response**:
```
CNXN(0x01000000, 0x00100000, "server:dbgif-server v1.0.0;")
```

### OPEN (0x4E45504F)
Open a new stream for communication.

**Direction**: Client → Server (or Server → Device)

**Arguments**:
- arg0: Local stream ID (1-65535)
- arg1: 0 (remote stream ID unknown)

**Data**: Service name

**Service Types**:
- Host services: "host:list", "host:device:<id>", "host:version", "host:features"
- Device services: "shell:", "tcp:8080", etc. (forwarded to selected device)

**Example**:
```
OPEN(1, 0, "host:list")
OPEN(2, 0, "host:device:tcp:custom001")
OPEN(3, 0, "shell:")
```

### OKAY (0x59414B4F)
Success acknowledgment and stream establishment.

**Direction**: Server → Client (or Device → Server)

**Arguments**:
- arg0: Local stream ID (responder's stream ID)
- arg1: Remote stream ID (from OPEN command)

**Data**: None (or optional success data)

**Example**:
```
OKAY(100, 1)  // Server stream 100 ↔ Client stream 1
```

### WRTE (0x45545257)
Write data to an established stream.

**Direction**: Bidirectional

**Arguments**:
- arg0: Local stream ID
- arg1: Remote stream ID

**Data**: Payload to write

**Example**:
```
WRTE(1, 100, "ls -la\n")
WRTE(100, 1, "total 42\ndrwxr-xr-x ...\n")
```

### CLSE (0x45534C43)
Close an established stream.

**Direction**: Bidirectional

**Arguments**:
- arg0: Local stream ID
- arg1: Remote stream ID

**Data**: None (or optional error reason)

**Example**:
```
CLSE(1, 100)
CLSE(100, 1)
```

### PING (0x474E4950)
Ping request to test connection.

**Direction**: Client → Server → Device

**Arguments**:
- arg0: Connect ID (from device's banner)
- arg1: Random token for this ping

**Data**: None

**Example**:
```
PING(0x12345678, 0xABCDEF01)
```

### PONG (0x474E4F50)
Ping response.

**Direction**: Device → Server → Client

**Arguments**:
- arg0: Connect ID (from PING)
- arg1: Random token (from PING)

**Data**: None

**Example**:
```
PONG(0x12345678, 0xABCDEF01)
```

## Host Services

The server provides special host services that clients can access using the OPEN command.

### host:list
Lists all available devices with their status.

**Request**:
```
OPEN(1, 0, "host:list")
```

**Response**:
```
OKAY(100, 1)
WRTE(100, 1, "tcp:custom001 offline tizen MyBoard v1.2\nusb:device123 device seret DevBoard v2.0\n")
```

**Response Format**:
```
<device_id> <status> <systemtype> <model> <version>
<device_id> <status> <systemtype> <model> <version>
...
```

**Device Status Values**:
- `device`: Connected and ready
- `offline`: Discovered but not connected

### host:device:<device_id>
Selects a target device for subsequent operations.

**Request**:
```
OPEN(2, 0, "host:device:tcp:custom001")
```

**Success Response**:
```
OKAY(101, 2)
```

**Failure Response**:
```
CLSE(0, 2, "device not found")
```

### host:version
Returns server version information.

**Request**:
```
OPEN(3, 0, "host:version")
```

**Response**:
```
OKAY(102, 3)
WRTE(102, 3, "dbgif-server 1.0.0")
```

### host:features
Lists server capabilities.

**Request**:
```
OPEN(4, 0, "host:features")
```

**Response**:
```
OKAY(103, 4)
WRTE(103, 4, "lazy-connection\nmulti-client\nping-pong\nhost-services\n")
```

**Standard Features**:
- `lazy-connection`: Devices connected on-demand
- `multi-client`: Multiple clients can use same device
- `ping-pong`: Connection health monitoring
- `host-services`: Built-in server services

## Protocol Flow Examples

### Complete Session Example

**1. Connection Establishment**:
```
Client → Server: CNXN(0x01000000, 0x00100000, "client:debugger:test-client v1.0;")
Server → Client: CNXN(0x01000000, 0x00100000, "server:dbgif-server v1.0.0;")
```

**2. Device Discovery**:
```
Client → Server: OPEN(1, 0, "host:list")
Server → Client: OKAY(100, 1)
Server → Client: WRTE(100, 1, "tcp:custom001 offline tizen MyBoard v1.2\n")
Client → Server: OKAY(1, 100)
Client → Server: CLSE(1, 100)
Server → Client: CLSE(100, 1)
```

**3. Device Selection**:
```
Client → Server: OPEN(2, 0, "host:device:tcp:custom001")
Server → Client: OKAY(101, 2)
Client → Server: CLSE(2, 101)
Server → Client: CLSE(101, 2)
```

**4. Device Communication**:
```
Client → Server: OPEN(3, 0, "shell:")
Server → Device: OPEN(3, 0, "shell:")  [Server connects to device if needed]
Device → Server: OKAY(200, 3)
Server → Client: OKAY(200, 3)

Client → Server: WRTE(3, 200, "ls -la\n")
Server → Device: WRTE(3, 200, "ls -la\n")
Device → Server: OKAY(200, 3)
Server → Client: OKAY(200, 3)

Device → Server: WRTE(200, 3, "total 42\ndrwxr-xr-x ...\n")
Server → Client: WRTE(200, 3, "total 42\ndrwxr-xr-x ...\n")
Client → Server: OKAY(3, 200)
Server → Device: OKAY(3, 200)

Client → Server: CLSE(3, 200)
Server → Device: CLSE(3, 200)
Device → Server: CLSE(200, 3)
Server → Client: CLSE(200, 3)
```

## Device Discovery and Registration

### TCP Device Discovery
Devices are discovered through network scanning without immediate connection:

**Discovery Process**:
1. Server scans configured TCP ports for device availability
2. Devices advertise their presence (basic info exchange)
3. Server registers device as "offline" with basic information
4. Connection established only when client selects device

**Device ID Format**: `tcp:<serial_number>`

### Device Registration Format
When devices connect (after client selection), they provide full information:

**CNXN Data Format**:
```
<systemtype>:<device_serialno>:<banner>
```

**Example**:
```
tizen:custom001:banner ro.product.model=MyBoard;ro.build.version=v1.2;ro.connect.id=0x12345678;
```

**Banner Properties**:
- `ro.product.model`: Device model name
- `ro.build.version`: Firmware/software version
- `ro.connect.id`: Connection ID for PING/PONG (32-bit hex)

## Stream Management

### Stream ID Allocation
- Client stream IDs: 1-65535 (per client session)
- Server stream IDs: Allocated dynamically per response
- Device stream IDs: Allocated by device for each connection
- Stream ID 0: Reserved, never used

### Stream Lifecycle
1. **Opening**: Client sends OPEN → Server responds OKAY/CLSE
2. **Active**: Bidirectional WRTE/OKAY exchanges
3. **Closing**: Either side sends CLSE → Other side responds CLSE
4. **Cleanup**: Stream mapping removed, IDs can be reused

### Multi-client Support
Each client session has independent stream ID space:
- Client A stream 1 and Client B stream 1 are different
- Server maintains separate mappings for each client
- Device sees different stream IDs for each client connection

## Error Handling

### Protocol-level Errors
- **Invalid command**: Message ignored, no response
- **CRC mismatch**: Message discarded
- **Invalid stream ID**: CLSE with error reason
- **Stream already exists**: CLSE with "stream exists"

### Service-level Errors
- **Service not found**: CLSE with "service not found"
- **Device not selected**: CLSE with "no device selected"
- **Device unavailable**: CLSE with "device offline"
- **Permission denied**: CLSE with "permission denied"

### Connection Errors
- **Connection timeout**: Automatic cleanup and notification
- **Device disconnect**: All client streams receive CLSE
- **Transport error**: Affected connections marked offline

## Security Considerations

### Current Implementation
- No authentication (AUTH command not implemented)
- No encryption (plain TCP)
- Local network access only
- Device discovery limited to configured ports

### Future Security Enhancements
- Device authentication via AUTH command
- TLS encryption for transport security
- Access control for host services
- Rate limiting and connection quotas

## Performance Requirements

### Throughput
- Support 100 concurrent client connections
- Multiple streams per client (up to 1000 per session)
- Sub-100ms response latency for control messages
- Efficient stream data forwarding

### Resource Management
- Connection pooling for device connections
- Lazy device connection establishment
- Automatic cleanup of inactive streams
- Memory-efficient message buffering

## Compliance Notes

### ADB-like Design
- Message format inspired by ADB structure
- Similar command semantics where applicable
- Stream multiplexing concept adapted
- Magic number validation pattern adopted

### Key Differences from Standard ADB
- **NOT fully ADB-compatible** - custom implementation
- AUTH command not implemented (no authentication)
- PING/PONG added for connection monitoring
- Host services for device management (not in standard ADB)
- Lazy device connection model (optimization)
- Multi-client device sharing (enhancement)
- Custom host service prefix ("host:")
- Different device discovery mechanism