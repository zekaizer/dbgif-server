# dbgif Protocol Specification

**Version:** 1.0.0
**Last Updated:** 2025-01-17

## Overview

dbgif uses a dual-protocol architecture inspired by ADB for communication between components:
- **Client ↔ Server**: ASCII-based text protocol for simplicity and debugging
- **Server ↔ Device**: Binary protocol with 24-byte headers for efficiency

This design allows clients to use simple text commands while maintaining efficient binary communication with devices.

## Architecture

```
┌──────────┐     ASCII Protocol      ┌──────────┐     Binary Protocol     ┌──────────┐
│  Client  │ ◄──────────────────────► │  Server  │ ◄─────────────────────► │  Device  │
└──────────┘   (text commands)        └──────────┘   (24-byte headers)     └──────────┘
```

The server acts as a protocol bridge:
- Receives ASCII commands from clients
- Translates to binary messages for devices
- Multiplexes streams between multiple clients and devices
- Handles host services directly without device communication

## Client-Server Protocol (ASCII)

The Client-Server protocol uses a simple ASCII-based format similar to ADB's client protocol.

### Request Format

```
[4-byte hex length][request-string]
```

- **Length**: 4-byte hexadecimal string representing the length of the request (e.g., "000c" for 12 bytes)
- **Request**: ASCII string containing the command

**Examples:**
```
"000chost:version"        # 12-byte request for version
"000ahost:list"           # 10-byte request for device list
"0005shell:"              # 5-byte request for shell service
```

### Response Format

Responses use type-first pattern with length before data:

```
Success: ["OKAY"][4-byte hex length][response-data]
Failure: ["FAIL"][4-byte hex length][error-message]
```

- **Status**: "OKAY" for success, "FAIL" for error (4 bytes, fixed position)
- **Length**: Length of response data only (not including "OKAY"/"FAIL")
- **Data**: Optional response data or error message

**Examples:**
```
"OKAY0000"                              # Success with no data
"OKAY0010dbgif-server 1.0"              # Success with 16-byte data
"FAIL0011device not found"              # Failure with 17-byte error message
```

### Host Services

Host services are handled directly by the server without device communication.

#### host:version
Returns the server version.

**Request:** `"000chost:version"`
**Response:** `"OKAY"` + Length + version string

**Example:**
```
C: "000chost:version"
S: "OKAY0010dbgif-server 1.0"  # OKAY + 0x10 (16 decimal) byte version string
```

#### host:list / host:devices
Lists all registered devices with their status.

**Request:** `"000ahost:list"` or `"000dhost:devices"`
**Response:** `"OKAY"` + Length + device list

**Device List Format:**
```
<device_id>\t<status>\t<systemtype>\t<model>\t<version>\n
```

**Status Values:**
- `device`: Device is connected and ready
- `offline`: Device is registered but not connected
- `authorizing`: Device is in authorization process (future)

**Example:**
```
C: "000ahost:list"
S: "OKAY0050tcp:custom001\toffline\ttizen\tMyBoard\tv1.2\nusb:device123\tdevice\tseret\tDevBoard\tv2.0\n"  # 0x50 (80 decimal) byte device list
```

#### host:transport:<device_id>
Selects a target device for subsequent commands. All following non-host commands will be forwarded to this device.

**Request:** `"001ahost:transport:tcp:custom001"`
**Response:** `"OKAY"` + Length (success) or `"FAIL"` + Length + error

**Example:**
```
C: "001ahost:transport:tcp:custom001"
S: "OKAY0000"                           # Device selected, no data
C: "0005shell:"                         # This will create a stream
```

#### host:connect:<ip>:<port>
Connects directly to a device at the specified IPv4 address and port. This creates a dynamic connection without requiring pre-registration.

**Security Restriction:** For security reasons, only localhost (127.0.0.1) connections are allowed.

**Request:** `"0019host:connect:127.0.0.1:5557"`
**Response:** `"OKAY"` + Length (success) or `"FAIL"` + Length + error

**Example:**
```
C: "0019host:connect:127.0.0.1:5557"
S: "OKAY0000"                           # Device registered, no data
C: "0005shell:"                         # This will create a stream
```

**Error Cases:**
- Invalid IP address: `"FAIL0011invalid address"`
- Non-localhost IP: `"FAIL001donly localhost connections allowed"`
- Registration failed: `"FAIL0013registration failed"`
- Port out of range: `"FAIL000dinvalid port"`

#### host:features
Lists server capabilities and supported features.

**Request:** `"000dhost:features"`
**Response:** `"OKAY"` + Length + feature list (one per line)

**Example:**
```
C: "000dhost:features"
S: "OKAY0034lazy-connection\nmulti-client\nping-pong\ndirect-connect\n"  # 0x34 (52 decimal) byte feature list
```

### Local Services

Local services are forwarded to the selected device after transport selection. Service requests automatically create streams,
and the server responds with the assigned stream ID.

**Important:** First select a device using `host:transport`, then request services directly.

#### shell[:<command>]
Opens a shell session or executes a command.

**Request:** `"0005shell:"` or `"0011shell:ls -la"`
**Response:** `"OKAY000201"` (stream ID "01" assigned)
**Data Exchange:** Use STRM messages for commands and responses

#### tcp:<port>
Creates a TCP forwarding tunnel.

**Request:** `"0008tcp:5555"`
**Response:** `"OKAY000202"` (stream ID "02" assigned)
**Data Exchange:** Use STRM messages for TCP data forwarding

#### sync:
Initiates file synchronization protocol.

**Request:** `"0005sync:"`
**Response:** `"OKAY000203"` (stream ID "03" assigned)
**Data Exchange:** Use STRM messages for sync protocol messages

### Streaming Services (Multiplexed)

Services that require continuous data exchange use multiplexed streaming with explicit stream IDs:

#### Stream Message Format

```
["STRM"][2-byte hex stream-id][6-byte hex length][data]
```

- **"STRM"**: Stream data marker (4 bytes, fixed)
- **Stream ID**: 2-byte hex stream identifier (e.g., "01", "FF")
- **Length**: 6-byte hex for data length (supports up to 16MB)
- **Data**: Actual payload (can be empty for stream closure)

**Special case:** Zero-length STRM closes the stream:
```
"STRM01000000"  # Close stream 01 (no data)
```

#### Example: Shell Session

1. **Setup and Stream Creation:**
   ```
   C: "001ahost:transport:device_id"
   S: "OKAY0000"                    # Transport selected

   C: "0005shell:"                  # Request shell service
   S: "OKAY000201"                  # Stream 01 assigned
   ```

2. **Multiplexed Data Exchange:**
   ```
   C: "STRM0100000Als -la\n"       # Send 0x0A (10 decimal) bytes: "ls -la\n" on stream 01
   S: "STRM01000035total 48\ndrwxr-xr-x 2 user user...\n"  # Response

   C: "STRM01000005pwd\n"           # Send "pwd\n" on stream 01
   S: "STRM0100000B/home/user\n"   # Response 0x0B (11 decimal) bytes: "/home/user\n"
   ```

3. **Stream Termination:**
   ```
   C: "STRM01000000"                # Close stream 01 (zero-length)
   S: "OKAY0000"                    # Stream closed
   ```

#### Multiple Concurrent Streams

The multiplexed format allows multiple services to run simultaneously:

```
C: "0005shell:"                     # Request shell service
S: "OKAY000201"                     # Stream 01 assigned

C: "0008tcp:8080"                   # Request TCP forward
S: "OKAY000202"                     # Stream 02 assigned

C: "STRM0100000Als -la\n"           # 0x0A (10 decimal) bytes shell command on stream 01
C: "STRM02000012GET / HTTP/1.1\r\n" # HTTP request on stream 02

S: "STRM01000035total 48\n..."      # Shell response on stream 01
S: "STRM02000026HTTP/1.1 200 OK..." # HTTP response on stream 02
```

**Benefits:**
- Clear stream boundaries and identification
- Support for concurrent operations
- Consistent message format
- Easy to parse and debug
- No ambiguity about which stream data belongs to

## Server-Device Protocol (Binary)

The Server-Device protocol uses a binary format with 24-byte headers for efficiency.

### Message Format

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

### Commands

#### CNXN (0x4E584E43)
Connection establishment between server and device.

**Arguments:**
- arg0: Protocol version (0x01000000)
- arg1: Maximum message length

**Data:** System identifier string

**Response:** CNXN with device information

**CNXN Data Format:**
```
<systemtype>:<device_serialno>:<banner>
```

**Example:**
```
tizen:custom001:ro.product.model=MyBoard;ro.build.version=v1.2;ro.connect.id=0x12345678;
```

**Note:** The device includes its unique `ro.connect.id` in the banner during the
CNXN handshake. This ID will be used for all PING/PONG health checks.

#### OPEN (0x4E45504F)
Open a new stream for communication.

**Arguments:**
- arg0: Local stream ID
- arg1: 0 (remote stream ID unknown at this point)

**Data:** Service name (e.g., "shell:", "tcp:8080")

**Response:** OKAY (success) or CLSE (failure)

#### OKAY (0x59414B4F)
Success acknowledgment.

**Arguments:**
- arg0: Local stream ID (responder's own stream ID)
- arg1: Remote stream ID (original sender's stream ID from OPEN)

**Data:** None

#### WRTE (0x45545257)
Write data to an open stream.

**Arguments:**
- arg0: Local stream ID
- arg1: Remote stream ID

**Data:** Payload to write

**Response:** OKAY (acknowledgment)

#### CLSE (0x45534C43)
Close an open stream.

**Arguments:**
- arg0: Local stream ID
- arg1: Remote stream ID

**Data:** None

#### PING (0x474E4950)
Ping request to test connection.

**Arguments:**
- arg0: Connect ID (from device's ro.connect.id in CNXN banner)
- arg1: Random token for this ping

**Data:** None

**Response:** PONG with same connect ID and token

**Note:** The connect_id is provided by the device during the second CNXN message
in the banner field (e.g., ro.connect.id=0x12345678). The device uses this ID
for all subsequent PING messages to identify itself.

#### PONG (0x474E4F50)
Ping response.

**Arguments:**
- arg0: Connect ID (from PING)
- arg1: Random token (from PING)

**Data:** None

## Protocol Bridge Flow

The server bridges between ASCII and binary protocols:

### Example: Shell Service with STRM

1. **Client selects device (ASCII):**
   ```
   C→S: "001ahost:transport:tcp:device001"
   S→C: "OKAY0000"                          # Device selected
   ```

2. **Client requests shell service:**
   ```
   C→S: "0005shell:"                        # Request shell service
   ```

3. **Server opens stream with device (Binary):**
   ```
   S→D: OPEN(local_id=501, remote_id=0, data="shell:")
   D→S: OKAY(local_id=100, remote_id=501)
   ```

4. **Server acknowledges with stream ID:**
   ```
   S→C: "OKAY000201"                        # Stream 01 assigned
   ```

5. **Data exchange using STRM format:**
   ```
   # Client sends command
   C→S: "STRM01000003ls\n"                  # STRM + stream 01 + 3 bytes + "ls\n"

   # Server forwards to device
   S→D: WRTE(local_id=501, remote_id=100, data="ls\n")
   D→S: OKAY(local_id=100, remote_id=501)

   # Device sends response
   D→S: WRTE(local_id=100, remote_id=501, data="file1\nfile2\n")
   S→D: OKAY(local_id=501, remote_id=100)

   # Server forwards to client
   S→C: "STRM0100000Efile1\nfile2\n"        # STRM + stream 01 + 0x0E (14 decimal) bytes + output
   ```

6. **Stream closure:**
   ```
   C→S: "STRM01000000"                      # Zero-length STRM closes stream
   S→D: CLSE(local_id=501, remote_id=100)
   D→S: CLSE(local_id=100, remote_id=501)
   S→C: "OKAY0000"                          # Stream closed
   ```

### Example: Device List

1. **Client requests device list (ASCII):**
   ```
   C→S: "000ahost:list"
   ```

2. **Server processes internally (no device communication):**
   ```
   Server checks internal device registry
   ```

3. **Server responds (ASCII):**
   ```
   S→C: "OKAY0028tcp:dev1\tdevice\ttizen\tBoard\tv1.0\n"  # OKAY + 0x28 (40 decimal) byte device list
   ```

## Device Discovery and Registration

Devices are discovered through transport layer mechanisms:

### TCP Transport Discovery
- Devices advertise on configured network ports
- Server detects and registers as "offline"
- Connection established on-demand when client selects device

### USB Transport Discovery
- Server enumerates USB devices
- Compatible devices registered as "offline"
- Serial numbers extracted from USB descriptors

### Registration States
- **offline**: Device discovered but not connected
- **device**: Device connected and ready
- **authorizing**: Device pending authorization (future)

## Connection Management

### Client-Server Connection Flow

Unlike the binary protocol, client-server uses a stateless ASCII protocol:

1. **TCP Connection:**
   - Client connects to server on port 5037 (default)
   - No handshake required (stateless protocol)
   - Connection persists for session duration

2. **Command Processing:**
   ```
   Client                    Server
     |                         |
     |--TCP Connect:5037------>|
     |                         |
     |--"000chost:version"---->|
     |<--"OKAY00051.0.0"------| # OKAY + 0x05 (5 decimal) byte version
     |                         |
     |--"000ahost:list"------->|
     |<--"OKAY002c..."---------|  # OKAY + device list
     |                         |
   ```

3. **Transport Selection (Stateful):**
   ```
   C: "001ahost:transport:tcp:device001"
   S: "OKAY0000"
   # Server now routes all non-host commands to device001
   ```

### Server-Device Connection Flow (Lazy)

The server uses lazy connection with binary protocol:

1. **Device Discovery Phase:**
   ```
   Device                    Server
     |                         |
     |  (Advertises on 5557)   |
     |<--Server discovers-------|
     |                         |
     |  Status: "offline"      |
     |  No active connection   |
   ```

2. **On-Demand Connection (First Stream Request):**
   ```
   Client → Server: "001ahost:transport:tcp:device001"
   Server: Mark device001 as selected for this client
   Client → Server: "0005shell:"  # Request shell service

   Server → Device: [Connect via transport]
   Server → Device: CNXN(version=0x01000000, maxdata=256K, "RESET\x00")
   Device → Server: CNXN(version=0x01000000, maxdata=256K, "tizen:device001:banner ro.product.model=MyBoard;ro.build.version=v1.0;ro.connect.id=0x12345678;")
   Server → Device: CNXN(version=0x01000000, maxdata=256K, "host::ready")

   Server: Update device status to "device"
   Server → Device: OPEN(local_id=1, remote_id=0, "shell:")
   Device → Server: OKAY(local_id=100, remote_id=1)

   Server → Client: "OKAY000201"  # Stream 01 assigned
   ```

3. **Connection Maintenance:**
   ```
   Every 1 second:
   Device → Server: PING(connect_id=0x12345678, token=random)
   Server → Device: PONG(connect_id=0x12345678, token=same)

   If no PONG within 3 seconds:
   Device: Close connection, retry
   Server: Mark device as "offline"
   ```

### Three-Way Stream Flow

Complete flow from client through server to device:

1. **Stream Establishment:**
   ```
   # Client has already selected device via host:transport

   Client → Server: "0005shell:"          # Request shell service

   Server internal:
   - Allocate server-side stream ID (e.g., 501)
   - Map client_stream_01 ↔ server_stream_501

   Server → Device: OPEN(local_id=501, remote_id=0, "shell:")
   Device → Server: OKAY(local_id=100, remote_id=501)

   Server internal:
   - Map server_stream_501 ↔ device_stream_100
   - Complete mapping: client_01 ↔ server_501 ↔ device_100

   Server → Client: "OKAY000201"                # Stream 01 assigned
   ```

2. **Data Transfer:**
   ```
   # Write from client
   Client → Server: "STRM0100000Cls -la\n"      # STRM format

   Server internal:
   - Lookup: client_01 → server_501 → device_100

   Server → Device: WRTE(local_id=501, remote_id=100, "ls -la\n")
   Device → Server: OKAY(local_id=100, remote_id=501)

   # Response from device
   Device → Server: WRTE(local_id=100, remote_id=501, "total 48\n...")
   Server → Device: OKAY(local_id=501, remote_id=100)

   Server internal:
   - Lookup: device_100 → server_501 → client_01

   Server → Client: "STRM01000035total 48\n..." # STRM format  # 0x35 (53 decimal) bytes
   ```

3. **Stream Closure:**
   ```
   Client → Server: "STRM01000000"  # Zero-length STRM closes stream

   Server → Device: CLSE(local_id=501, remote_id=100)
   Device → Server: CLSE(local_id=100, remote_id=501)

   Server internal:
   - Remove mapping for client_01 ↔ server_501 ↔ device_100

   Server → Client: "OKAY0000"
   ```

### Multi-Client Multiplexing

Multiple clients can connect to the same device:

```
Client A                Server              Device
   |                      |                   |
   |--host:transport----->|                   |
   |--"0005shell:"------->|--OPEN(501)------->|
   |                      |<--OKAY(100,501)---|
   |<--"OKAY000201"--------|                   |
                          |
Client B                  |
   |                      |                   |
   |--host:transport----->|                   |
   |--"0005shell:"------->|--OPEN(502)------->|
   |                      |<--OKAY(101,502)---|
   |<--"OKAY000201"--------|                   |

Stream Mapping:
- ClientA:01 ↔ Server:501 ↔ Device:100
- ClientB:01 ↔ Server:502 ↔ Device:101
```

### Connection State Management

**Client Session State:**
- Selected device (via host:transport)
- Active streams (00-FF, max 256)
- Connection socket

**Device Connection State:**
- Connection status (offline/device)
- Active streams with all clients
- Last ping timestamp
- Device properties (from CNXN)

**Stream Mapping Table:**
```
| Client | C.Stream | S.Stream | Device | D.Stream |
|--------|----------|----------|--------|----------|
| A      | 01       | 501      | dev001 | 100      |
| A      | 02       | 502      | dev001 | 101      |
| B      | 01       | 503      | dev001 | 102      |
| B      | 02       | 504      | dev002 | 100      |
```

## Error Handling

### Client-Server Errors
- Invalid command format: `"FAIL"` + Length + error message
- Device not found: `"FAIL0011device not found"`
- Service unavailable: `"FAIL0013service unavailable"`

### Server-Device Errors
- Connection failure: Device marked "offline"
- Invalid message: Connection terminated
- CRC32 mismatch: Message discarded
- Stream errors: CLSE sent to both endpoints

### Recovery
- Client reconnection: New session, must reselect device
- Device reconnection: Existing streams invalidated
- Partial writes: Stream must be closed and reopened

## Performance Considerations

### Optimizations
- Lazy device connections (connect on-demand)
- Stream multiplexing for concurrent operations
- Connection pooling for multiple clients
- Efficient binary protocol for device communication

### Limits
- Maximum request size: 64KB (4-byte hex = FFFF)
- Maximum STRM data size: 16MB (6-byte hex = FFFFFF)
- Stream IDs per session: 256 (00-FF in 2-byte hex)
- Concurrent clients: 100 (default)
- Device connections: 16 (default)

## Quick Reference

### Message Formats Summary

| Type | Format | Example |
|------|--------|---------|
| **Request** | `[4-byte hex length][command]` | `"000chost:version"` |
| **Response (OK)** | `"OKAY"[4-byte hex length][data]` | `"OKAY0010server 1.0"` |
| **Response (Fail)** | `"FAIL"[4-byte hex length][error]` | `"FAIL0005error"` |
| **Stream Data** | `"STRM"[2-byte id][6-byte length][data]` | `"STRM01000003ls\n"` |
| **Stream Close** | `"STRM"[2-byte id]"000000"` | `"STRM01000000"` |

### Common Commands

| Command | Description | Response |
|---------|-------------|----------|
| `host:version` | Get server version | `"OKAY"` + version |
| `host:list` | List all devices | `"OKAY"` + device list |
| `host:transport:<id>` | Select device | `"OKAY0000"` |
| `host:connect:<ip>:<port>` | Connect to IP:port | `"OKAY0000"` |
| `host:features` | Get server features | `"OKAY"` + features |
| `shell:` | Open shell stream | `"OKAY0002"` + stream ID |
| `tcp:<port>` | TCP port forward | `"OKAY0002"` + stream ID |
| `sync:` | File sync | `"OKAY0002"` + stream ID |

## Differences from Standard ADB

### Added Features
- PING/PONG for connection health monitoring
- Lazy connection establishment
- Simplified device discovery
- CRC32 validation on all messages

### Removed Features
- AUTH command (no authentication yet)
- SYNC.TXT protocol (simplified file transfer)
- USB protocol details (abstracted)
- Encryption support (not implemented)

### Protocol Differences
- Dual-protocol architecture (ASCII + Binary)
- Simplified host service protocol
- Direct ASCII responses (no A_WRTE wrapping)
- No authentication requirement