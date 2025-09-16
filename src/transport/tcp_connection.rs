use async_trait::async_trait;
use crate::protocol::error::{ProtocolError, ProtocolResult};
use crate::protocol::message::AdbMessage;
use crate::transport::{TransportConfig};
use crate::transport::connection::{Connection, ConnectionStats, ConnectionMetadata, ConnectionOptions, ConnectionFeature};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;

/// TCP connection implementation
pub struct TcpConnection {
    stream: TcpStream,
    #[allow(dead_code)]
    config: TransportConfig,
    stats: ConnectionStats,
    metadata: ConnectionMetadata,
    options: ConnectionOptions,
    closed: bool,
}

impl TcpConnection {
    /// Create a new TCP connection
    pub fn new(stream: TcpStream, config: TransportConfig) -> ProtocolResult<Self> {
        let local_addr = stream.local_addr().map_err(|e| {
            ProtocolError::TransportError(format!("Failed to get local address: {}", e))
        })?;

        let remote_addr = stream.peer_addr().map_err(|e| {
            ProtocolError::TransportError(format!("Failed to get remote address: {}", e))
        })?;

        let mut metadata = ConnectionMetadata::new("tcp");
        metadata.add_property("local_addr", local_addr.to_string());
        metadata.add_property("remote_addr", remote_addr.to_string());

        let options = ConnectionOptions {
            read_timeout: Some(Duration::from_secs(30)),
            write_timeout: Some(Duration::from_secs(30)),
            keepalive_interval: config.keepalive_interval,
            tcp_nodelay: config.tcp_nodelay,
            read_buffer_size: config.buffer_size,
            write_buffer_size: config.buffer_size,
            max_message_size: config.max_data_size,
            enable_compression: false,
            enable_encryption: false,
        };

        Ok(Self {
            stream,
            config,
            stats: ConnectionStats::new(),
            metadata,
            options,
            closed: false,
        })
    }

    /// Get the underlying TCP stream reference
    pub fn stream(&self) -> &TcpStream {
        &self.stream
    }

    /// Split the connection into read and write halves
    pub fn split(self) -> (tokio::net::tcp::OwnedReadHalf, tokio::net::tcp::OwnedWriteHalf) {
        self.stream.into_split()
    }

    /// Create a new TCP connection from an existing stream with default config
    pub fn from_stream(stream: TcpStream) -> ProtocolResult<Self> {
        Self::new(stream, TransportConfig::default())
    }

    /// Check if the underlying stream supports a specific socket option
    pub fn supports_socket_option(&self, option: TcpSocketOption) -> bool {
        match option {
            TcpSocketOption::NoDelay => true,
            TcpSocketOption::KeepAlive => true,
            TcpSocketOption::Linger => true,
            TcpSocketOption::ReuseAddr => true,
            TcpSocketOption::ReusePort => cfg!(target_os = "linux"),
        }
    }

    /// Configure TCP socket options
    pub fn configure_socket(&self, options: &[TcpSocketOption]) -> ProtocolResult<()> {
        for option in options {
            match option {
                TcpSocketOption::NoDelay => {
                    if let Err(e) = self.stream.set_nodelay(true) {
                        tracing::warn!("Failed to set TCP_NODELAY: {}", e);
                    }
                }
                TcpSocketOption::KeepAlive => {
                    // Note: tokio::net::TcpStream doesn't expose SO_KEEPALIVE directly
                    tracing::debug!("TCP KeepAlive configuration not directly supported");
                }
                _ => {
                    tracing::debug!("Socket option {:?} not implemented", option);
                }
            }
        }
        Ok(())
    }
}

/// TCP socket options that can be configured
#[derive(Debug, Clone, Copy)]
pub enum TcpSocketOption {
    /// TCP_NODELAY - disable Nagle's algorithm
    NoDelay,
    /// SO_KEEPALIVE - enable keep-alive probes
    KeepAlive,
    /// SO_LINGER - control connection close behavior
    Linger,
    /// SO_REUSEADDR - allow address reuse
    ReuseAddr,
    /// SO_REUSEPORT - allow port reuse (Linux only)
    ReusePort,
}

#[async_trait]
impl Connection for TcpConnection {
    async fn send_bytes(&mut self, data: &[u8]) -> ProtocolResult<()> {
        if self.closed {
            return Err(ProtocolError::ConnectionClosed);
        }

        use tokio::io::AsyncWriteExt;

        let write_result = if let Some(timeout) = self.options.write_timeout {
            tokio::time::timeout(timeout, self.stream.write_all(data)).await
                .map_err(|_| ProtocolError::Timeout { timeout_ms: timeout.as_millis() as u64 })?
        } else {
            self.stream.write_all(data).await
        };

        write_result.map_err(|e| {
            self.stats.record_error();
            ProtocolError::IoError(e)
        })?;

        self.stats.record_bytes_sent(data.len() as u64);
        Ok(())
    }

    async fn receive_bytes(&mut self, buffer: &mut [u8]) -> ProtocolResult<Option<usize>> {
        if self.closed {
            return Err(ProtocolError::ConnectionClosed);
        }

        use tokio::io::AsyncReadExt;

        let read_result = if let Some(timeout) = self.options.read_timeout {
            tokio::time::timeout(timeout, self.stream.read(buffer)).await
                .map_err(|_| ProtocolError::Timeout { timeout_ms: timeout.as_millis() as u64 })?
        } else {
            self.stream.read(buffer).await
        };

        match read_result {
            Ok(0) => {
                // EOF - connection closed by peer
                self.closed = true;
                Ok(None)
            }
            Ok(n) => {
                self.stats.record_bytes_received(n as u64);
                Ok(Some(n))
            }
            Err(e) => {
                self.stats.record_error();
                Err(ProtocolError::IoError(e))
            }
        }
    }

    async fn receive_message(&mut self) -> ProtocolResult<Option<AdbMessage>> {
        // Read the 24-byte header first
        let mut header_buf = [0u8; 24];
        let mut bytes_read = 0;

        while bytes_read < 24 {
            match self.receive_bytes(&mut header_buf[bytes_read..]).await? {
                Some(n) => bytes_read += n,
                None => return Ok(None), // Connection closed
            }
        }

        // Parse header to get data length
        let data_length = u32::from_le_bytes([
            header_buf[12], header_buf[13], header_buf[14], header_buf[15]
        ]);

        // Check data size limit
        if data_length as usize > self.options.max_message_size {
            return Err(ProtocolError::DataSizeExceeded {
                size: data_length as usize,
                max_size: self.options.max_message_size,
            });
        }

        // Read data payload if present
        let mut data = Vec::new();
        if data_length > 0 {
            data.resize(data_length as usize, 0);
            let mut bytes_read = 0;

            while bytes_read < data_length as usize {
                match self.receive_bytes(&mut data[bytes_read..]).await? {
                    Some(n) => bytes_read += n,
                    None => {
                        return Err(ProtocolError::ConnectionClosed);
                    }
                }
            }
        }

        // Construct complete message buffer
        let mut message_buf = Vec::with_capacity(24 + data.len());
        message_buf.extend_from_slice(&header_buf);
        message_buf.extend_from_slice(&data);

        // Deserialize the message
        match AdbMessage::deserialize(&message_buf) {
            Ok(message) => {
                self.stats.record_message_received();
                Ok(Some(message))
            }
            Err(e) => Err(ProtocolError::from(e)),
        }
    }

    async fn send_bytes_timeout(&mut self, data: &[u8], timeout: Duration) -> ProtocolResult<()> {
        let original_timeout = self.options.write_timeout;
        self.options.write_timeout = Some(timeout);
        let result = self.send_bytes(data).await;
        self.options.write_timeout = original_timeout;
        result
    }

    async fn receive_bytes_timeout(&mut self, buffer: &mut [u8], timeout: Duration) -> ProtocolResult<Option<usize>> {
        let original_timeout = self.options.read_timeout;
        self.options.read_timeout = Some(timeout);
        let result = self.receive_bytes(buffer).await;
        self.options.read_timeout = original_timeout;
        result
    }

    async fn close(&mut self) -> ProtocolResult<()> {
        if !self.closed {
            use tokio::io::AsyncWriteExt;
            let _ = self.stream.shutdown().await;
            self.closed = true;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> ProtocolResult<()> {
        self.close().await
    }

    fn is_connected(&self) -> bool {
        !self.closed
    }

    fn local_addr(&self) -> ProtocolResult<SocketAddr> {
        self.stream.local_addr().map_err(|e| {
            ProtocolError::TransportError(format!("Failed to get local address: {}", e))
        })
    }

    fn remote_addr(&self) -> ProtocolResult<SocketAddr> {
        self.stream.peer_addr().map_err(|e| {
            ProtocolError::TransportError(format!("Failed to get remote address: {}", e))
        })
    }

    fn stats(&self) -> ConnectionStats {
        let mut stats = self.stats.clone();
        stats.uptime = stats.current_uptime();
        stats
    }

    fn reset_stats(&mut self) {
        self.stats = ConnectionStats::new();
    }

    fn metadata(&self) -> ConnectionMetadata {
        self.metadata.clone()
    }

    async fn set_options(&mut self, options: ConnectionOptions) -> ProtocolResult<()> {
        self.options = options;

        // Apply TCP-specific options
        if let Err(e) = self.stream.set_nodelay(self.options.tcp_nodelay) {
            tracing::warn!("Failed to set TCP_NODELAY: {}", e);
        }

        Ok(())
    }

    fn get_options(&self) -> ConnectionOptions {
        self.options.clone()
    }

    async fn flush(&mut self) -> ProtocolResult<()> {
        use tokio::io::AsyncWriteExt;
        self.stream.flush().await.map_err(|e| {
            self.stats.record_error();
            ProtocolError::IoError(e)
        })
    }

    fn max_message_size(&self) -> usize {
        self.options.max_message_size
    }

    fn supports_feature(&self, feature: ConnectionFeature) -> bool {
        match feature {
            ConnectionFeature::Keepalive => true,
            ConnectionFeature::Compression => self.options.enable_compression,
            ConnectionFeature::Encryption => self.options.enable_encryption,
            ConnectionFeature::Multiplexing => false, // TCP doesn't natively support multiplexing
            ConnectionFeature::Backpressure => true,
            ConnectionFeature::GracefulShutdown => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};

    async fn create_test_connection_pair() -> ProtocolResult<(TcpConnection, TcpConnection)> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();

        let client_conn = TcpConnection::from_stream(client_stream)?;
        let server_conn = TcpConnection::from_stream(server_stream)?;

        Ok((client_conn, server_conn))
    }

    #[tokio::test]
    async fn test_tcp_connection_creation() {
        let (client_conn, server_conn) = create_test_connection_pair().await.unwrap();

        assert!(client_conn.is_connected());
        assert!(server_conn.is_connected());

        assert!(client_conn.local_addr().is_ok());
        assert!(client_conn.remote_addr().is_ok());
    }

    #[tokio::test]
    async fn test_socket_options() {
        let (conn, _) = create_test_connection_pair().await.unwrap();

        assert!(conn.supports_socket_option(TcpSocketOption::NoDelay));
        assert!(conn.supports_socket_option(TcpSocketOption::KeepAlive));

        assert!(conn.configure_socket(&[TcpSocketOption::NoDelay]).is_ok());
    }

    #[tokio::test]
    async fn test_connection_metadata() {
        let (conn, _) = create_test_connection_pair().await.unwrap();

        let metadata = conn.metadata();
        assert_eq!(metadata.connection_type, "tcp");
        assert!(metadata.get_property("local_addr").is_some());
        assert!(metadata.get_property("remote_addr").is_some());
    }

    #[tokio::test]
    async fn test_connection_features() {
        let (conn, _) = create_test_connection_pair().await.unwrap();

        assert!(conn.supports_feature(ConnectionFeature::Keepalive));
        assert!(conn.supports_feature(ConnectionFeature::Backpressure));
        assert!(conn.supports_feature(ConnectionFeature::GracefulShutdown));
        assert!(!conn.supports_feature(ConnectionFeature::Multiplexing));
    }
}