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

## Host Services

The server provides special host services that clients can access using the OPEN command. These services are handled by the server itself rather than being forwarded to devices.

### Service Types

#### host:list
Lists all available devices registered with the server with detailed information.

**Usage:** Client sends OPEN with data "host:list"

**Response:** Server responds with OKAY, then sends device list via WRTE

**Response Format (in WRTE data):**
```
<device_id> <status> <systemtype> <model> <version>
<device_id> <status> <systemtype> <model> <version>
...
```

**Device Status:**
- `device`: Device is connected and ready
- `offline`: Device is registered but not connected

**Example Session:**
```
Client -> Server: OPEN(1, 0, "host:list")
Server -> Client: OKAY(1, 100)
Server -> Client: WRTE(1, 100, "tcp:custom001 offline tizen MyBoard v1.2\nusb:device123 device seret DevBoard v2.0\n")
Client -> Server: OKAY(100, 1)
```

#### host:device:<device_id>
Selects a target device for the current client session. All subsequent OPEN commands will be forwarded to this device.

**Usage:** Client sends OPEN with data "host:device:tcp:custom001"

**Response:**
- OKAY: Device selected successfully
- CLSE: Device not found or unavailable

**Note:** Once a device is selected, all non-host OPEN commands from this client session will be automatically forwarded to the selected device until the client disconnects or selects a different device.

#### host:version
Returns server version information.

**Usage:** Client sends OPEN with data "host:version"

**Response:** Server responds with OKAY, then sends version via WRTE

**Example Session:**
```
Client -> Server: OPEN(1, 0, "host:version")
Server -> Client: OKAY(1, 100)
Server -> Client: WRTE(1, 100, "dbgif-server 1.0.0")
Client -> Server: OKAY(100, 1)
```

#### host:features
Lists server capabilities and supported features.

**Usage:** Client sends OPEN with data "host:features"

**Response:** Server responds with OKAY, then sends feature list via WRTE

**Example Session:**
```
Client -> Server: OPEN(1, 0, "host:features")
Server -> Client: OKAY(1, 100)
Server -> Client: WRTE(1, 100, "lazy-connection\nmulti-client\nping-pong\n")
Client -> Server: OKAY(100, 1)
```

## Device Discovery

Devices are discovered through transport layer mechanisms without requiring immediate connection:

### TCP Transport Discovery
- Devices advertise their availability on configured network ports
- Server detects advertisement and registers device as "offline"
- Device information is obtained through advertisement protocol

### USB Transport Discovery
- Server enumerates USB devices using device descriptors
- Compatible devices are registered as "offline"
- Device serial numbers are extracted from USB descriptors

### Device Registration
When discovered, devices are registered with:
- **Device ID**: `<transport>:<serial>` format (e.g., `tcp:custom001`, `usb:device123`)
- **Status**: Initially "offline"
- **Transport Info**: Connection details for later use
- **Basic Info**: Model, version if available from discovery

When a device daemon connects and provides full information, the registration is updated with:
- **System Type**: Device system type (e.g., tizen, seret)
- **Banner Properties**: Device capabilities and version info
- **CNXN Data Format**: `<systemtype>:<device_serialno>:<banner>`

**Example CNXN Data:**
```
tizen:custom001:banner ro.product.model=MyBoard;ro.build.version=v1.2;ro.connect.id=0x12345678;
```

**Common Banner Properties:**
- `ro.product.model`: Device model name
- `ro.build.version`: Firmware/software version
- `ro.connect.id`: Connection identifier (32-bit value, used in PING/PONG)

## Connection Flow

### Client-Server Connection
1. Client connects to server via TCP
2. Client sends CNXN with device info
3. Server responds with CNXN
4. Client can now send OPEN/PING commands or host services
5. For each OPEN, server responds with OKAY/CLSE
6. Data exchange via WRTE/OKAY pairs
7. Stream closed with CLSE

### Client Device Selection Flow
When a client wants to communicate with a specific device:

1. **Device Discovery**: Client sends OPEN with "host:list"
2. **Device Selection**: Client sends OPEN with "host:device:<device_id>"
3. **Service Access**: Client sends OPEN with desired service (e.g., "shell:", "tcp:8080")
4. **Automatic Forwarding**: Server forwards all subsequent OPEN commands to selected device
5. **Session Persistence**: Device selection remains active until client disconnects or selects another device

**Example Session:**
```
Client -> Server: OPEN(1, 0, "host:list")
Server -> Client: OKAY(1, 100) + device list data
Client -> Server: OPEN(2, 0, "host:device:tcp:custom001")
Server -> Client: OKAY(2, 101) (device selected)
Client -> Server: OPEN(3, 0, "shell:")
Server -> Device: OPEN(3, 0, "shell:") (forwarded to tcp:custom001)
```

### Server-Device Connection Flow
The server uses lazy connection establishment to optimize resource usage:

1. **Device Discovery**: Transport layer discovers devices and registers them as "offline"
2. **Device Registration**: Server maintains device list without immediate connection
3. **Client Device Selection**: Client uses "host:device:<device_id>" to select target device
4. **On-Demand Connection**: When client sends first non-host OPEN request after device selection:
   - Server initiates connection to selected device via appropriate transport
   - Server sends CNXN with b"RESET\x00" to reset device state
   - Device daemon sends CNXN with device identifier and capabilities
   - Server responds with CNXN acknowledgment (arg0 contains protocol version)
   - Device status changes from "offline" to "device"
5. **Connection Reuse**: Subsequent client requests use existing device connection
6. **Connection Maintenance**: Device maintains connection via periodic PING/PONG (every 1 second, 3 second timeout)
7. **Multi-Client Support**: Multiple clients can select and use the same device simultaneously
8. **Cleanup**: On disconnect, device status returns to "offline" and server removes from active connections

### Three-way Communication Flow
When a client wants to communicate with a device (after device selection):

1. **Client Request**: Client sends OPEN to server with service name
2. **Device Validation**: Server uses previously selected device from `host:device` command
3. **Connection Check**: Server ensures device is connected (connects if offline)
4. **Forward Request**: Server forwards OPEN to selected device daemon
5. **Device Response**: Device daemon responds with OKAY/CLSE
6. **Relay Response**: Server relays device response to client
7. **Stream Establishment**: If successful, bidirectional stream is created
8. **Data Forwarding**: All WRTE commands are forwarded between client and device
9. **Stream Closure**: Either side can close with CLSE, forwarded to other side

**Error Cases:**
- No device selected: Server responds with CLSE
- Selected device unavailable: Server responds with CLSE
- Device connection fails: Server responds with CLSE

```
Client ----OPEN(arg0=1, arg1=0)--------> Server ----OPEN(arg0=1, arg1=0)--------> Device
Client <---OKAY(arg0=1, arg1=100)------- Server <---OKAY(arg0=100, arg1=1)------- Device
Client ----WRTE(arg0=1, arg1=100)------> Server ----WRTE(arg0=1, arg1=100)------> Device
Device ----OKAY(arg0=100, arg1=1)------> Server ----OKAY(arg0=100, arg1=1)------> Client
Client ----CLSE(arg0=1, arg1=100)------> Server ----CLSE(arg0=1, arg1=100)------> Device
```


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

**General Error Cases:**
- Invalid commands are ignored
- Failed operations return CLSE
- Connection drops terminate all streams
- CRC32 validation on data payloads
- Network timeouts result in connection termination
- Malformed packets are discarded without response

**Device Selection Errors:**
- OPEN without device selection: Server responds with CLSE
- Invalid device ID in `host:device`: Server responds with CLSE
- Selected device unavailable: Server responds with CLSE on service OPEN

**Connection Errors:**
- Device connection failure: Server responds with CLSE, device remains "offline"
- Device disconnect during operation: All active streams receive CLSE
- Transport errors: Affected device marked "offline", clients notified via CLSE

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
