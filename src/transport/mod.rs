pub mod connection;
pub mod tcp;
pub mod tcp_connection;

use async_trait::async_trait;
use crate::protocol::error::ProtocolResult;
use std::net::SocketAddr;
use std::time::Duration;

pub use connection::Connection;
pub use tcp::TcpTransport;
pub use tcp_connection::TcpConnection;

/// Transport layer abstraction for different connection types
/// Supports TCP initially, with USB abstraction for future expansion
#[async_trait]
pub trait Transport: Send + Sync {
    /// The connection type this transport creates
    type Connection: Connection;

    /// Start listening for incoming connections on the specified address
    async fn listen(&self, addr: SocketAddr) -> ProtocolResult<Box<dyn TransportListener<Connection = Self::Connection>>>;

    /// Connect to a remote address (for client-side connections)
    async fn connect(&self, addr: SocketAddr) -> ProtocolResult<Self::Connection>;

    /// Connect with timeout
    async fn connect_timeout(&self, addr: SocketAddr, timeout: Duration) -> ProtocolResult<Self::Connection>;

    /// Get the transport type name for logging
    fn transport_type(&self) -> &'static str;

    /// Check if the transport supports the given address
    fn supports_address(&self, addr: &SocketAddr) -> bool;

    /// Get maximum data size supported by this transport
    fn max_data_size(&self) -> usize;

    /// Get connection timeout for this transport
    fn default_timeout(&self) -> Duration;
}

/// Listener abstraction for accepting incoming connections
#[async_trait]
pub trait TransportListener: Send + Sync {
    /// The connection type this listener accepts
    type Connection: Connection;

    /// Accept a new incoming connection
    async fn accept(&mut self) -> ProtocolResult<(Self::Connection, SocketAddr)>;

    /// Get the local address this listener is bound to
    fn local_addr(&self) -> ProtocolResult<SocketAddr>;

    /// Close the listener
    async fn close(&mut self) -> ProtocolResult<()>;

    /// Check if the listener is still active
    fn is_active(&self) -> bool;
}

/// Transport configuration for different connection types
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Maximum number of concurrent connections
    pub max_connections: usize,

    /// Default connection timeout
    pub connect_timeout: Duration,

    /// Keep-alive interval for connections
    pub keepalive_interval: Option<Duration>,

    /// Maximum data size per message
    pub max_data_size: usize,

    /// Buffer size for reading/writing
    pub buffer_size: usize,

    /// Enable TCP nodelay (disable Nagle's algorithm)
    pub tcp_nodelay: bool,

    /// SO_REUSEADDR setting for listeners
    pub reuse_addr: bool,

    /// Backlog size for listeners
    pub listen_backlog: u32,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            max_connections: 100,
            connect_timeout: Duration::from_secs(10),
            keepalive_interval: Some(Duration::from_secs(30)),
            max_data_size: 1024 * 1024, // 1MB
            buffer_size: 64 * 1024, // 64KB
            tcp_nodelay: true,
            reuse_addr: true,
            listen_backlog: 128,
        }
    }
}

/// Transport statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct TransportStats {
    /// Total connections accepted
    pub connections_accepted: u64,

    /// Total connections initiated
    pub connections_initiated: u64,

    /// Current active connections
    pub active_connections: u64,

    /// Total bytes sent
    pub bytes_sent: u64,

    /// Total bytes received
    pub bytes_received: u64,

    /// Connection errors
    pub connection_errors: u64,

    /// Read errors
    pub read_errors: u64,

    /// Write errors
    pub write_errors: u64,

    /// Timeout errors
    pub timeout_errors: u64,
}

impl TransportStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new connection accepted
    pub fn connection_accepted(&mut self) {
        self.connections_accepted += 1;
        self.active_connections += 1;
    }

    /// Record a new connection initiated
    pub fn connection_initiated(&mut self) {
        self.connections_initiated += 1;
        self.active_connections += 1;
    }

    /// Record a connection closed
    pub fn connection_closed(&mut self) {
        if self.active_connections > 0 {
            self.active_connections -= 1;
        }
    }

    /// Record bytes sent
    pub fn bytes_sent(&mut self, count: u64) {
        self.bytes_sent += count;
    }

    /// Record bytes received
    pub fn bytes_received(&mut self, count: u64) {
        self.bytes_received += count;
    }

    /// Record a connection error
    pub fn connection_error(&mut self) {
        self.connection_errors += 1;
    }

    /// Record a read error
    pub fn read_error(&mut self) {
        self.read_errors += 1;
    }

    /// Record a write error
    pub fn write_error(&mut self) {
        self.write_errors += 1;
    }

    /// Record a timeout error
    pub fn timeout_error(&mut self) {
        self.timeout_errors += 1;
    }
}

/// Address types supported by transports
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportAddress {
    /// TCP/IP socket address
    Tcp(SocketAddr),
    /// USB device address (for future expansion)
    Usb { vendor_id: u16, product_id: u16, serial: Option<String> },
    /// Unix domain socket (for testing)
    Unix(String),
}

impl TransportAddress {
    /// Check if this is a TCP address
    pub fn is_tcp(&self) -> bool {
        matches!(self, TransportAddress::Tcp(_))
    }

    /// Check if this is a USB address
    pub fn is_usb(&self) -> bool {
        matches!(self, TransportAddress::Usb { .. })
    }

    /// Check if this is a Unix socket address
    pub fn is_unix(&self) -> bool {
        matches!(self, TransportAddress::Unix(_))
    }

    /// Get TCP socket address if available
    pub fn as_tcp(&self) -> Option<&SocketAddr> {
        match self {
            TransportAddress::Tcp(addr) => Some(addr),
            _ => None,
        }
    }

    /// Get USB device info if available
    pub fn as_usb(&self) -> Option<(u16, u16, &Option<String>)> {
        match self {
            TransportAddress::Usb { vendor_id, product_id, serial } => Some((*vendor_id, *product_id, serial)),
            _ => None,
        }
    }

    /// Get Unix socket path if available
    pub fn as_unix(&self) -> Option<&str> {
        match self {
            TransportAddress::Unix(path) => Some(path),
            _ => None,
        }
    }
}

impl From<SocketAddr> for TransportAddress {
    fn from(addr: SocketAddr) -> Self {
        TransportAddress::Tcp(addr)
    }
}

impl std::fmt::Display for TransportAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportAddress::Tcp(addr) => write!(f, "tcp://{}", addr),
            TransportAddress::Usb { vendor_id, product_id, serial } => {
                if let Some(serial) = serial {
                    write!(f, "usb://{}:{}:{}", vendor_id, product_id, serial)
                } else {
                    write!(f, "usb://{}:{}", vendor_id, product_id)
                }
            }
            TransportAddress::Unix(path) => write!(f, "unix://{}", path),
        }
    }
}