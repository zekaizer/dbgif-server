# Data Model: DBGIF Server (ADB-style Protocol)

**Date**: 2025-09-16
**Version**: 1.0.0
**Context**: Core data structures for ADB-style DBGIF protocol implementation
**Reference**: docs/protocol.md

## Core Entities

### 1. ADB Message

**Purpose**: Primary communication unit following ADB packet structure

**Structure**:
```rust
pub struct AdbMessage {
    pub command: u32,     // Command code (little-endian)
    pub arg0: u32,        // First argument (little-endian)
    pub arg1: u32,        // Second argument (little-endian)
    pub data_length: u32, // Length of data payload (little-endian)
    pub data_crc32: u32,  // CRC32 of data payload (little-endian)
    pub magic: u32,       // Magic number (~command, bitwise NOT)
    pub data: Vec<u8>,    // Variable length data payload
}

impl AdbMessage {
    pub const HEADER_SIZE: usize = 24;

    pub fn new(command: AdbCommand, arg0: u32, arg1: u32, data: Vec<u8>) -> Self;
    pub fn serialize(&self) -> Vec<u8>;
    pub fn deserialize(bytes: &[u8]) -> Result<Self, ProtocolError>;
    pub fn validate_crc(&self) -> bool;
}

#[repr(u32)]
pub enum AdbCommand {
    CNXN = 0x4E584E43,  // Connection establishment
    OPEN = 0x4E45504F,  // Open new stream
    OKAY = 0x59414B4F,  // Success acknowledgment
    WRTE = 0x45545257,  // Write data to stream
    CLSE = 0x45534C43,  // Close stream
    PING = 0x474E4950,  // Ping request
    PONG = 0x474E4F50,  // Ping response
}
```

**Validation Rules**:
- magic must equal !command (bitwise NOT)
- data_length must match actual data.len()
- data_crc32 must match computed CRC32 of data
- command must be valid AdbCommand value

### 2. Client Session

**Purpose**: Manages state for individual client connections

**Structure**:
```rust
pub struct ClientSession {
    pub session_id: SessionId,
    pub connection: Box<dyn Connection>,
    pub selected_device: Option<DeviceId>,
    pub streams: HashMap<StreamId, StreamInfo>,
    pub client_info: ClientInfo,
    pub created_at: Instant,
    pub last_activity: Instant,
}

pub type SessionId = u64;
pub type StreamId = u32;
pub type DeviceId = String; // Format: "tcp:custom001" or "usb:device123"

pub struct ClientInfo {
    pub system_type: String,     // From CNXN data
    pub serial_no: String,       // From CNXN data
    pub banner: HashMap<String, String>, // Banner properties
    pub peer_addr: String,       // Client network address
}

pub struct StreamInfo {
    pub local_id: StreamId,      // Client's stream ID
    pub remote_id: StreamId,     // Device's stream ID
    pub device_id: DeviceId,     // Target device
    pub service_name: String,    // Requested service
    pub state: StreamState,
    pub created_at: Instant,
}

pub enum StreamState {
    Opening,     // OPEN sent, waiting for OKAY/CLSE
    Active,      // OKAY received, ready for WRTE
    Closing,     // CLSE sent, waiting for confirmation
    Closed,      // Stream closed and cleaned up
}
```

**Validation Rules**:
- session_id must be unique across server instance
- stream IDs in range 1-65535 (0 reserved)
- selected_device must exist in device registry
- stream mapping must be bidirectional

### 3. Device Registration

**Purpose**: Manages discovered and connected devices

**Structure**:
```rust
pub struct DeviceRegistry {
    pub devices: HashMap<DeviceId, DeviceInfo>,
    pub connections: HashMap<DeviceId, DeviceConnection>,
}

pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub transport_type: TransportType,
    pub status: DeviceStatus,
    pub system_type: Option<String>,  // From device CNXN
    pub model: Option<String>,        // From banner properties
    pub version: Option<String>,      // From banner properties
    pub banner: HashMap<String, String>,
    pub discovery_info: TransportInfo,
    pub discovered_at: Instant,
    pub last_seen: Option<Instant>,
}

#[derive(Clone, Copy)]
pub enum TransportType {
    Tcp,
    Usb,
}

#[derive(Clone, Copy)]
pub enum DeviceStatus {
    Offline,    // Discovered but not connected
    Device,     // Connected and ready
}

pub enum TransportInfo {
    Tcp { host: String, port: u16 },
    Usb { vendor_id: u16, product_id: u16, serial: String },
}

pub struct DeviceConnection {
    pub device_id: DeviceId,
    pub connection: Box<dyn Connection>,
    pub streams: HashMap<StreamId, StreamMapping>,
    pub connect_id: u32,         // For PING/PONG
    pub last_ping: Instant,
    pub ping_token: u32,
    pub connected_at: Instant,
}

pub struct StreamMapping {
    pub device_stream_id: StreamId,
    pub client_session_id: SessionId,
    pub client_stream_id: StreamId,
}
```

**Validation Rules**:
- device_id format: "<transport>:<serial>"
- status transitions: Offline ↔ Device only
- connect_id must be unique per device
- stream mappings must be consistent

### 4. Host Services

**Purpose**: Built-in server services accessible via OPEN command

**Structure**:
```rust
pub trait HostService: Send + Sync {
    fn service_name(&self) -> &str;
    async fn handle(&self, session: &ClientSession, args: &str) -> Result<HostServiceResponse, HostServiceError>;
}

pub enum HostServiceResponse {
    Okay(Vec<u8>),   // Success with optional data
    Close(String),   // Failure with error message
}

pub struct HostListService;    // Implements "host:list"
pub struct HostDeviceService;  // Implements "host:device:<id>"
pub struct HostVersionService; // Implements "host:version"
pub struct HostFeaturesService; // Implements "host:features"

pub struct HostServiceRegistry {
    services: HashMap<String, Box<dyn HostService>>,
}

impl HostServiceRegistry {
    pub fn register<S: HostService + 'static>(&mut self, service: S);
    pub fn handle_service(&self, service_name: &str, session: &ClientSession) -> Option<&dyn HostService>;
    pub fn is_host_service(service_name: &str) -> bool {
        service_name.starts_with("host:")
    }
}
```

**Validation Rules**:
- Service names must start with "host:"
- host:device service must validate device_id format
- host:list must return current device registry state

### 5. Transport Layer

**Purpose**: Abstraction for different transport mechanisms (TCP, USB)

**Structure**:
```rust
pub trait Transport: Send + Sync {
    type Connection: Connection;
    type Error: std::error::Error + Send + Sync + 'static;

    async fn listen(&mut self) -> Result<(), Self::Error>;
    async fn accept(&mut self) -> Result<Self::Connection, Self::Error>;
    async fn connect(&mut self, info: &TransportInfo) -> Result<Self::Connection, Self::Error>;
    async fn discover_devices(&mut self) -> Result<Vec<DeviceInfo>, Self::Error>;
    async fn shutdown(&mut self) -> Result<(), Self::Error>;
}

pub trait Connection: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn read_message(&mut self) -> Result<AdbMessage, Self::Error>;
    async fn write_message(&mut self, msg: &AdbMessage) -> Result<(), Self::Error>;
    async fn close(&mut self) -> Result<(), Self::Error>;
    fn peer_info(&self) -> String;
    fn is_connected(&self) -> bool;
}

// TCP Implementation
pub struct TcpTransport {
    pub bind_addr: SocketAddr,
    pub listener: Option<TcpListener>,
    pub discovery_config: TcpDiscoveryConfig,
}

pub struct TcpConnection {
    pub stream: TcpStream,
    pub peer_addr: SocketAddr,
    pub read_buffer: BytesMut,
}

pub struct TcpDiscoveryConfig {
    pub device_ports: Vec<u16>,   // Ports to scan for devices
    pub discovery_interval: Duration,
}
```

**Validation Rules**:
- Messages must follow ADB packet format exactly
- CRC32 validation on all data payloads
- Little-endian byte order for header fields
- Connection must handle partial reads/writes

### 6. Server State

**Purpose**: Global server state and coordination

**Structure**:
```rust
pub struct ServerState {
    pub config: ServerConfig,
    pub client_sessions: HashMap<SessionId, ClientSession>,
    pub device_registry: DeviceRegistry,
    pub host_services: HostServiceRegistry,
    pub session_counter: AtomicU64,
    pub stream_counter: AtomicU64,
    pub stats: ServerStats,
    pub start_time: Instant,
}

pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub max_connections: usize,
    pub connection_timeout: Duration,
    pub ping_interval: Duration,
    pub ping_timeout: Duration,
    pub device_discovery_interval: Duration,
    pub tcp_discovery_ports: Vec<u16>,
}

pub struct ServerStats {
    pub total_sessions: AtomicU64,
    pub active_sessions: AtomicU64,
    pub total_streams: AtomicU64,
    pub active_streams: AtomicU64,
    pub messages_processed: AtomicU64,
    pub bytes_transferred: AtomicU64,
    pub errors_count: AtomicU64,
}
```

**Validation Rules**:
- max_connections must be > 0 and <= 1000
- active sessions/streams must not exceed maximums
- All timeouts must be > 0
- Counters must be monotonic increasing

### 7. Error Types

**Purpose**: Structured error handling across the system

**Structure**:
```rust
#[derive(thiserror::Error, Debug)]
pub enum DbgifError {
    #[error("Protocol error: {message}")]
    Protocol { message: String },

    #[error("Transport error: {source}")]
    Transport {
        #[from]
        source: Box<dyn std::error::Error + Send + Sync>
    },

    #[error("Invalid message: {reason}")]
    InvalidMessage { reason: String },

    #[error("Stream error: {stream_id} - {reason}")]
    Stream { stream_id: StreamId, reason: String },

    #[error("Device error: {device_id} - {reason}")]
    Device { device_id: DeviceId, reason: String },

    #[error("Host service error: {service} - {reason}")]
    HostService { service: String, reason: String },

    #[error("Connection limit exceeded: {current}/{max}")]
    ConnectionLimit { current: usize, max: usize },

    #[error("CRC validation failed")]
    CrcMismatch,

    #[error("Internal server error: {message}")]
    Internal { message: String },
}

impl DbgifError {
    pub fn to_clse_reason(&self) -> String {
        match self {
            Self::Protocol { message } => format!("protocol: {}", message),
            Self::InvalidMessage { reason } => format!("invalid: {}", reason),
            Self::Stream { reason, .. } => format!("stream: {}", reason),
            Self::Device { reason, .. } => format!("device: {}", reason),
            Self::HostService { reason, .. } => format!("host: {}", reason),
            Self::ConnectionLimit { .. } => "connection limit".to_string(),
            Self::CrcMismatch => "crc error".to_string(),
            _ => "internal error".to_string(),
        }
    }
}
```

**Validation Rules**:
- All errors must be serializable for logging
- Protocol errors should map to CLSE responses
- CRC errors must not be ignored
- Internal errors indicate bugs that need fixing

## Data Relationships

```
Server
├── ServerState (1)
│   ├── client_sessions: HashMap<SessionId, ClientSession> (1:N)
│   ├── device_registry: DeviceRegistry (1:1)
│   └── host_services: HostServiceRegistry (1:1)
│
├── ClientSession (N)
│   ├── streams: HashMap<StreamId, StreamInfo> (1:N)
│   └── selected_device: Option<DeviceId> (1:0..1)
│
├── DeviceRegistry (1)
│   ├── devices: HashMap<DeviceId, DeviceInfo> (1:N)
│   └── connections: HashMap<DeviceId, DeviceConnection> (1:0..N)
│
└── DeviceConnection (0..N)
    └── streams: HashMap<StreamId, StreamMapping> (1:N)
```

## Message Flow Patterns

### Connection Establishment
```
Client -> Server: CNXN(version, max_length, "client_info")
Server -> Client: CNXN(version, max_length, "server_info")
```

### Host Service Access
```
Client -> Server: OPEN(local_id, 0, "host:list")
Server -> Client: OKAY(server_id, local_id)
Server -> Client: WRTE(server_id, local_id, device_list_data)
Client -> Server: OKAY(local_id, server_id)
```

### Device Communication (after selection)
```
Client -> Server: OPEN(local_id, 0, "shell:")
Server -> Device: OPEN(local_id, 0, "shell:")
Device -> Server: OKAY(device_id, local_id)
Server -> Client: OKAY(device_id, local_id)
Client -> Server: WRTE(local_id, device_id, "command")
Server -> Device: WRTE(local_id, device_id, "command")
```

## Serialization Details

### ADB Message Serialization
- All multi-byte integers in little-endian format
- Header: 24 bytes fixed size
- Data: variable length, may be empty
- CRC32: Computed over data payload only
- Magic: Always equals !command

### String Encoding
- UTF-8 encoding for all text data
- Null-terminated for banner properties
- Length-prefixed for service names

## Performance Characteristics

**Memory Usage**:
- AdbMessage: ~50 bytes + data size
- ClientSession: ~2KB + streams
- DeviceConnection: ~1KB + streams
- Maximum memory: ~200MB for 100 clients with full streams

**Concurrency Model**:
- Shared state protected by Arc<RwLock<>>
- Per-session async tasks
- Per-device connection tasks
- Lock-free atomic counters for statistics

## Future Extensions

**Planned Additions**:
- USB transport implementation using nusb 2.0
- Device authentication (AUTH command)
- Compression for large data transfers
- Metrics collection and monitoring endpoints
- Configuration hot-reload