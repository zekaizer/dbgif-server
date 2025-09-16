use async_trait::async_trait;
use crate::protocol::error::{ProtocolError, ProtocolResult};
use crate::transport::{Transport, TransportListener, TransportConfig, TransportStats};
use crate::transport::tcp_connection::TcpConnection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

/// TCP transport implementation
pub struct TcpTransport {
    config: TransportConfig,
    stats: Arc<Mutex<TransportStats>>,
}

impl TcpTransport {
    /// Create a new TCP transport with default configuration
    pub fn new() -> Self {
        Self {
            config: TransportConfig::default(),
            stats: Arc::new(Mutex::new(TransportStats::new())),
        }
    }

    /// Create a new TCP transport with custom configuration
    pub fn with_config(config: TransportConfig) -> Self {
        Self {
            config,
            stats: Arc::new(Mutex::new(TransportStats::new())),
        }
    }

    /// Get transport statistics
    pub fn stats(&self) -> TransportStats {
        self.stats.lock().unwrap().clone()
    }

    /// Reset transport statistics
    pub fn reset_stats(&self) {
        *self.stats.lock().unwrap() = TransportStats::new();
    }
}

impl Default for TcpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for TcpTransport {
    type Connection = TcpConnection;

    async fn listen(&self, addr: SocketAddr) -> ProtocolResult<Box<dyn TransportListener<Connection = Self::Connection>>> {
        let listener = TcpListener::bind(addr).await.map_err(|e| {
            ProtocolError::TransportError(format!("Failed to bind TCP listener to {}: {}", addr, e))
        })?;

        // Update stats
        {
            let _stats = self.stats.lock().unwrap();
            // Note: We don't have a direct stat for listeners, but we can track it if needed
        }

        Ok(Box::new(TcpTransportListener {
            listener,
            config: self.config.clone(),
            stats: Arc::clone(&self.stats),
        }))
    }

    async fn connect(&self, addr: SocketAddr) -> ProtocolResult<Self::Connection> {
        self.connect_timeout(addr, self.config.connect_timeout).await
    }

    async fn connect_timeout(&self, addr: SocketAddr, timeout: Duration) -> ProtocolResult<Self::Connection> {
        let stream = tokio::time::timeout(timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| ProtocolError::Timeout { timeout_ms: timeout.as_millis() as u64 })?
            .map_err(|e| ProtocolError::TransportError(format!("Failed to connect to {}: {}", addr, e)))?;

        // Configure socket options
        if self.config.tcp_nodelay {
            if let Err(e) = stream.set_nodelay(true) {
                tracing::warn!("Failed to set TCP_NODELAY: {}", e);
            }
        }

        // Update stats
        {
            let mut stats = self.stats.lock().unwrap();
            stats.connection_initiated();
        }

        let connection = TcpConnection::new(stream, self.config.clone())?;
        Ok(connection)
    }

    fn transport_type(&self) -> &'static str {
        "tcp"
    }

    fn supports_address(&self, addr: &SocketAddr) -> bool {
        // TCP transport supports all IP addresses
        addr.ip().is_ipv4() || addr.ip().is_ipv6()
    }

    fn max_data_size(&self) -> usize {
        self.config.max_data_size
    }

    fn default_timeout(&self) -> Duration {
        self.config.connect_timeout
    }
}

/// TCP transport listener implementation
pub struct TcpTransportListener {
    listener: TcpListener,
    config: TransportConfig,
    stats: Arc<Mutex<TransportStats>>,
}

#[async_trait]
impl TransportListener for TcpTransportListener {
    type Connection = TcpConnection;

    async fn accept(&mut self) -> ProtocolResult<(Self::Connection, SocketAddr)> {
        let (stream, addr) = self.listener.accept().await.map_err(|e| {
            ProtocolError::TransportError(format!("Failed to accept TCP connection: {}", e))
        })?;

        // Configure socket options
        if self.config.tcp_nodelay {
            if let Err(e) = stream.set_nodelay(true) {
                tracing::warn!("Failed to set TCP_NODELAY on accepted connection: {}", e);
            }
        }

        // Update stats
        {
            let mut stats = self.stats.lock().unwrap();
            stats.connection_accepted();
        }

        let connection = TcpConnection::new(stream, self.config.clone())?;
        Ok((connection, addr))
    }

    fn local_addr(&self) -> ProtocolResult<SocketAddr> {
        self.listener.local_addr().map_err(|e| {
            ProtocolError::TransportError(format!("Failed to get local address: {}", e))
        })
    }

    async fn close(&mut self) -> ProtocolResult<()> {
        // TcpListener doesn't have an explicit close method
        // Dropping it will close the socket
        Ok(())
    }

    fn is_active(&self) -> bool {
        // For TcpListener, we consider it active if we can get the local address
        self.listener.local_addr().is_ok()
    }
}

