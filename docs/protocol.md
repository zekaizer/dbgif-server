# dbgif Protocol Specification

**Version:** 1.0.0
**Last Updated:** 2025-09-30

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

### Protocol Overview

The ASCII protocol supports two distinct message formats on the same connection:

#### Message Format Summary

| Purpose | Format | First 4 Bytes | Length Field | Max Size |
|---------|--------|---------------|--------------|----------|
| **Host Commands** | Length-first | Hex digits (`000c`, `001a`) | 4-byte hex | 64KB |
| **Stream Data** | Type-first | `"STRM"` marker | 6-byte hex | 16MB |
| **Responses** | Type-first | `"OKAY"` or `"FAIL"` | 4-byte hex | 64KB |

#### Parser Disambiguation

First 4 bytes determine message type: `"STRM"` for streams, `"OKAY"`/`"FAIL"` for responses, hex digits for commands. No collision possible.

**Design Rationale:** Host commands use 4-byte length (64KB max, compact), streams use 6-byte length (16MB max, large transfers).

### Request Format

#### Host Command Format
```
[4-byte hex length][command-string]
```

Examples: `"000chost:version"`, `"000ahost:list"`, `"0005shell:"`

**Note:** After service establishment, use Stream Message Format (see "Streaming Services").

### Response Format

```
Success: ["OKAY"][4-byte hex length][response-data]
Failure: ["FAIL"][4-byte hex length][error-message]
```

Examples: `"OKAY0000"`, `"OKAY0010dbgif-server 1.0"`, `"FAIL0011device not found"`

**Stream Assignment:** Service requests return `"OKAY000201"` (length=0x0002, stream ID="01").

### Host Services

Handled by server without device communication.

| Command | Description | Response |
|---------|-------------|----------|
| `host:version` | Server version | `"OKAY"` + version string |
| `host:list`, `host:devices` | List devices | `"OKAY"` + device list (TSV format) |
| `host:transport:<id>` | Select device | `"OKAY0000"` or `"FAIL"` + error |
| `host:connect:<ip>:<port>` | Connect to device (localhost only) | `"OKAY0000"` or `"FAIL"` + error |
| `host:features` | Server capabilities | `"OKAY"` + features (one per line) |

**Device List Format:** `<device_id>\t<status>\t<systemtype>\t<model>\t<version>\n`
**Status:** `device` (ready), `offline` (registered), `authorizing` (future)

### Local Services

Forwarded to device after `host:transport` selection. Returns assigned stream ID.

| Service | Description | Response |
|---------|-------------|----------|
| `shell[:cmd]` | Shell session/command | `"OKAY0002"` + stream ID |
| `tcp:<port>` | TCP forwarding | `"OKAY0002"` + stream ID |
| `sync:` | File sync | `"OKAY0002"` + stream ID |

All data exchange uses STRM messages (see below).

### Streaming Services (Multiplexed)

Services that require continuous data exchange use multiplexed streaming with explicit stream IDs:

#### Stream Message Format

After a service is established (shell:, tcp:, sync:), all data exchange uses the Stream Message Format for multiplexing:

```
["STRM"][2-byte hex stream-id][6-byte hex length][data]
```

- **"STRM"**: Fixed 4-byte ASCII marker (distinguishes from host commands)
- **Stream ID**: 2-byte hex identifier (00-FF, max 256 concurrent streams per session)
- **Length**: 6-byte hex for data length (supports up to 16MB: 0xFFFFFF)
- **Data**: Actual payload (can be empty for stream closure)

**Design Note:** 
- The 6-byte length field enables large file transfers (up to 16MB per message)
- The fixed "STRM" marker prevents parsing ambiguity with host commands
- Stream ID allows multiplexing multiple services on a single connection
- **Full-duplex:** Streams support simultaneous bidirectional data flow (client and device can send/receive independently)

**Special case:** Zero-length STRM closes the stream:
```
"STRM01000000"  # Close stream 01 (no data)
```

**Stream Closure Flows:**

**Client-Initiated Closure:**
1. Client → Server: `"STRM01000000"` (zero-length STRM)
2. Server → Device: `CLSE(local_id, remote_id)` (close stream on device)
3. Device → Server: `CLSE(remote_id, local_id)` (acknowledge closure)
4. Server → Client: `"OKAY0000"` (confirm stream closed)

The server removes the stream mapping only after receiving CLSE confirmation from the device.

**Server-Initiated Closure (Device-driven):**
1. Device → Server: `CLSE(local_id, remote_id)` (device closes stream)
2. Server → Device: `CLSE(remote_id, local_id)` (acknowledge)
3. Server → Client: `"STRM01000000"` (one-way notification, **no response expected**)

When the client receives an unsolicited zero-length STRM, it indicates the device closed 
the stream. The client should clean up local state without sending a response.

#### Example: Shell Session

1. **Setup and Stream Creation:**
   ```
   C: "001ahost:transport:device_id"
   S: "OKAY0000"                    # Transport selected

   C: "0005shell:"                  # Request shell service
   S: "OKAY000201"                  # Stream 01 assigned (length=0x0002, stream="01")
   ```

2. **Multiplexed Data Exchange:**
   ```
   C: "STRM0100000Als -la\n"       # 0x0A (10 decimal) bytes: "ls -la\n" on stream 01
   S: "STRM01000035total 48\ndrwxr-xr-x 2 user user...\n"  # 0x35 (53 decimal) bytes response

   C: "STRM01000005pwd\n"           # 0x05 (5 decimal) bytes: "pwd\n" on stream 01
   S: "STRM0100000B/home/user\n"   # 0x0B (11 decimal) bytes: "/home/user\n"
   ```

3. **Stream Termination:**
   ```
   C: "STRM01000000"                # Close stream 01 (zero-length STRM)
   
   # Server-Device binary protocol
   S→D: CLSE(local_id=501, remote_id=100)
   D→S: CLSE(local_id=100, remote_id=501)
   
   S: "OKAY0000"                    # Stream closed confirmation to client
   ```

#### Multiple Concurrent Streams

The multiplexed format allows multiple services to run simultaneously. Each stream operates independently with full-duplex communication:

```
C: "0005shell:"                     # Request shell service
S: "OKAY000201"                     # Stream 01 assigned

C: "0008tcp:8080"                   # Request TCP forward
S: "OKAY000202"                     # Stream 02 assigned

C: "STRM0100000Als -la\n"           # 0x0A (10 decimal) bytes shell command on stream 01
C: "STRM02000012GET / HTTP/1.1\r\n" # 0x12 (18 decimal) bytes HTTP request on stream 02

S: "STRM01000035total 48\n..."      # 0x35 (53 decimal) bytes shell response on stream 01
S: "STRM02000026HTTP/1.1 200 OK..." # 0x26 (38 decimal) bytes HTTP response on stream 02
```

## Server-Device Protocol (Binary)

24-byte header + variable data, little-endian, ADB-like structure.

### Message Format
```
command(u32) | arg0(u32) | arg1(u32) | data_length(u32) | data_crc32(u32) | magic(u32) | data
```

**Field Descriptions:**
- **command**: Command identifier (4 bytes, little-endian, e.g., 0x4E584E43 for CNXN)
- **arg0**: First argument (4 bytes, command-specific, e.g., local stream ID)
- **arg1**: Second argument (4 bytes, command-specific, e.g., remote stream ID)
- **data_length**: Payload length in bytes (4 bytes, little-endian)
- **data_crc32**: CRC32 checksum of payload (4 bytes, little-endian, 0 if no data)
- **magic**: Validation field (4 bytes, bitwise NOT of command, e.g., ~0x4E584E43)
- **data**: Variable-length payload (0 to data_length bytes)

### Commands

| Command | Code | Arguments | Data | Response |
|---------|------|-----------|------|----------|
| **CNXN** | 0x4E584E43 | version, maxlen | `systemtype:serial:banner` | CNXN |
| **OPEN** | 0x4E45504F | local_id, 0 | service name | OKAY/CLSE |
| **OKAY** | 0x59414B4F | local_id, remote_id | none | - |
| **WRTE** | 0x45545257 | local_id, remote_id | payload | OKAY |
| **CLSE** | 0x45534C43 | local_id, remote_id | none | CLSE (swapped IDs) |
| **PING** | 0x474E4950 | connect_id, token | none | PONG |
| **PONG** | 0x474E4F50 | connect_id, token | none | - |

**Notes:**
- **CNXN banner** includes `ro.connect.id` for PING/PONG identification
- **CLSE** is bidirectional (either side can initiate)

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
   C→S: "STRM01000003ls\n"                  # STRM + stream 01 + 0x03 (3 decimal) bytes

   # Server forwards to device
   S→D: WRTE(local_id=501, remote_id=100, data="ls\n")
   D→S: OKAY(local_id=100, remote_id=501)

   # Device sends response
   D→S: WRTE(local_id=100, remote_id=501, data="file1\nfile2\n")
   S→D: OKAY(local_id=501, remote_id=100)

   # Server forwards to client
   S→C: "STRM0100000Efile1\nfile2\n"        # STRM + stream 01 + 0x0E (14 decimal) bytes
   ```

6. **Stream closure:**
   ```
   C→S: "STRM01000000"                      # Zero-length STRM closes stream
   S→D: CLSE(local_id=501, remote_id=100)
   D→S: CLSE(local_id=100, remote_id=501)
   S→C: "OKAY0000"                          # Stream closed
   ```

## Device Discovery and Registration

**Transport Discovery:**
- **TCP**: Devices advertise on network ports, registered as "offline"
- **USB**: Server enumerates devices, extracts serial numbers

**States:** `offline` (discovered), `device` (connected), `authorizing` (future)

**Lazy Connection:** Actual connection established on-demand when client requests service.

## Connection Management

### Client-Server Connection
- Port 5555 (default), no handshake, stateless ASCII protocol
- `host:transport:<id>` selects device (stateful per session)
- Device switching: new streams to new device, existing streams remain active until closed

### Server-Device Connection (Lazy)

**CNXN Handshake (3-way):**
1. Server → Device: `CNXN("RESET")`
2. Device → Server: `CNXN("systemtype:serial:banner ro.connect.id=0x...")` 
3. Server → Device: `CNXN("host::ready")`

**Health Check:** PING/PONG every 1s (3s timeout → mark offline)

### Stream Flow (Three-Way Mapping)

**Stream Lifecycle:**
1. Client → Server: `"shell:"` (ASCII)
2. Server maps: client_01 ↔ server_501 ↔ device_100
3. Server → Device: `OPEN(501, 0, "shell:")` (binary)
4. Device → Server: `OKAY(100, 501)` (binary)
5. Server → Client: `"OKAY000201"` (ASCII, stream assigned)

**Data Exchange (Full-Duplex):**
- Client STRM → Server WRTE → Device (translate ASCII→binary)
- Device WRTE → Server STRM → Client (translate binary→ASCII)
- Both directions operate independently without blocking each other

**Closure:**
- **Client-initiated:** `STRM00...00` → CLSE handshake → `OKAY0000`
- **Device-initiated:** CLSE handshake → `STRM00...00` (one-way, no client response)

### Multi-Client Multiplexing

Multiple clients to same device: Server maintains separate stream mappings per client.
Example: ClientA:01→Server:501→Device:100, ClientB:01→Server:502→Device:101

## Error Handling

**Client-Server:** `"FAIL"` + length + error message
**Server-Device:** Connection termination (invalid/CRC), CLSE on stream errors
**Closure Errors:** Unexpected closure (device crash) → `STRM00...00` to client, timeout (5s) → force close, non-existent stream → idempotent CLSE

**Recovery:** Client reconnect requires reselect device, device reconnect invalidates streams

## Performance & Limits

**Optimizations:** Lazy connection, stream multiplexing, connection pooling
**Limits:** Request 64KB, STRM 16MB, Streams 256/session, Clients 100, Devices 16 (defaults)

## Quick Reference

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

### Stream Closure Summary

| Initiator | Client Sends | Server Responds | Notes |
|-----------|--------------|-----------------|-------|
| **Client** | `"STRM<id>000000"` | `"OKAY0000"` | Client closes stream |
| **Device** | N/A (receives) | `"STRM<id>000000"` | Server notifies client (no response expected) |

## Differences from Standard ADB

**Added:** PING/PONG, lazy connection, dual-protocol (ASCII+Binary)
**Removed:** AUTH, SYNC.TXT, encryption
**Different:** Simplified host services, direct ASCII responses (no A_WRTE wrapping)