# dbgif-server

A personal project implementing an ADB-like protocol server for connecting to custom devices (non-Android). Written in Rust.

## Features

- TCP-based device connections (similar to ADB)
- USB host-to-host bridge cable support (e.g., PL-25A1) for devices with USB host ports only
- Custom protocol implementation for non-Android device debugging and interface
- Cross-platform support (Windows and Linux)
- Asynchronous I/O using Tokio
- USB communication via nusb 2.0
- Transport abstraction layer for unified connection handling

## Architecture

ADB-like structure with centralized server:

```
Client (N) <--TCP--> Server (1) <--Transport Layer--> Device (N)
                                        |
                                        +-- TCP Transport
                                        +-- USB Transport (nusb)
```

- **Server**: Central hub managing device connections and client sessions
- **Transport Layer**: Unified interface for TCP and USB communications
- **Multi-client**: Multiple clients can share access to the same device
- **Multi-device**: Server can handle multiple connected devices simultaneously

## Protocol Commands

The protocol uses ADB-like command codes:

| Command | Hex Value | ASCII | Description |
|---------|-----------|-------|-------------|
| CNXN | 0x4E584E43 | "CNXN" | Connection establishment |
| OPEN | 0x4E45504F | "OPEN" | Open stream |
| OKAY | 0x59414B4F | "OKAY" | Success response |
| WRTE | 0x45545257 | "WRTE" | Write data |
| CLSE | 0x45534C43 | "CLSE" | Close stream |
| AUTH | 0x48545541 | "AUTH" | Authentication (not used in dbgif) |
| PING | 0x474E4950 | "PING" | Ping request (added in dbgif) |
| PONG | 0x474E4F50 | "PONG" | Ping response (added in dbgif) |
