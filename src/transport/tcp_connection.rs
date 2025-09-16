use async_trait::async_trait;
use crate::transport::connection::{TransportError, TransportResult, Connection};
use crate::transport::{TransportConfig};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;

/// TCP connection implementation
pub struct TcpConnection {
    stream: TcpStream,
    local_addr: SocketAddr,
    remote_addr: SocketAddr,
    #[allow(dead_code)]
    config: TransportConfig,
    closed: bool,
    max_message_size: usize,
}

impl TcpConnection {
    /// Create a new TCP connection
    pub fn new(stream: TcpStream, config: TransportConfig) -> TransportResult<Self> {
        let local_addr = stream.local_addr().map_err(|e| {
            TransportError::Other { message: format!("Failed to get local address: {}", e) }
        })?;

        let remote_addr = stream.peer_addr().map_err(|e| {
            TransportError::Other { message: format!("Failed to get remote address: {}", e) }
        })?;

        // Configure TCP options
        if config.tcp_nodelay {
            if let Err(e) = stream.set_nodelay(true) {
                tracing::warn!("Failed to set TCP_NODELAY: {}", e);
            }
        }

        let max_message_size = config.max_data_size;

        Ok(Self {
            stream,
            local_addr,
            remote_addr,
            config,
            closed: false,
            max_message_size,
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
    pub fn from_stream(stream: TcpStream) -> TransportResult<Self> {
        Self::new(stream, TransportConfig::default())
    }

    /// Get local address
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Get remote address
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

#[async_trait]
impl Connection for TcpConnection {
    async fn send_bytes(&mut self, data: &[u8]) -> TransportResult<()> {
        if self.closed {
            return Err(TransportError::ConnectionClosed);
        }

        use tokio::io::AsyncWriteExt;
        self.stream.write_all(data).await.map_err(|e| {
            TransportError::IoError(e)
        })
    }

    async fn receive_bytes(&mut self, buffer: &mut [u8]) -> TransportResult<Option<usize>> {
        if self.closed {
            return Err(TransportError::ConnectionClosed);
        }

        use tokio::io::AsyncReadExt;
        match self.stream.read(buffer).await {
            Ok(0) => {
                // EOF - connection closed by peer
                self.closed = true;
                Ok(None)
            }
            Ok(n) => Ok(Some(n)),
            Err(e) => Err(TransportError::IoError(e)),
        }
    }

    async fn send_bytes_timeout(&mut self, data: &[u8], timeout: Duration) -> TransportResult<()> {
        if self.closed {
            return Err(TransportError::ConnectionClosed);
        }

        use tokio::io::AsyncWriteExt;
        tokio::time::timeout(timeout, self.stream.write_all(data))
            .await
            .map_err(|_| TransportError::Timeout { timeout_ms: timeout.as_millis() as u64 })?
            .map_err(|e| TransportError::IoError(e))
    }

    async fn receive_bytes_timeout(&mut self, buffer: &mut [u8], timeout: Duration) -> TransportResult<Option<usize>> {
        if self.closed {
            return Err(TransportError::ConnectionClosed);
        }

        use tokio::io::AsyncReadExt;
        let result = tokio::time::timeout(timeout, self.stream.read(buffer))
            .await
            .map_err(|_| TransportError::Timeout { timeout_ms: timeout.as_millis() as u64 })?;

        match result {
            Ok(0) => {
                self.closed = true;
                Ok(None)
            }
            Ok(n) => Ok(Some(n)),
            Err(e) => Err(TransportError::IoError(e)),
        }
    }

    async fn close(&mut self) -> TransportResult<()> {
        if !self.closed {
            use tokio::io::AsyncWriteExt;
            let _ = self.stream.shutdown().await;
            self.closed = true;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> TransportResult<()> {
        self.close().await
    }

    fn is_connected(&self) -> bool {
        !self.closed
    }

    fn connection_id(&self) -> String {
        format!("tcp://{}→{}", self.local_addr, self.remote_addr)
    }

    async fn flush(&mut self) -> TransportResult<()> {
        use tokio::io::AsyncWriteExt;
        self.stream.flush().await.map_err(|e| {
            TransportError::IoError(e)
        })
    }

    fn max_message_size(&self) -> usize {
        self.max_message_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};

    async fn create_test_connection_pair() -> TransportResult<(TcpConnection, TcpConnection)> {
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

        assert!(!client_conn.connection_id().is_empty());
        assert!(client_conn.connection_id().starts_with("tcp://"));
    }

    #[tokio::test]
    async fn test_connection_id() {
        let (conn, _) = create_test_connection_pair().await.unwrap();
        let connection_id = conn.connection_id();

        assert!(connection_id.starts_with("tcp://"));
        assert!(connection_id.contains("→"));
    }

    #[tokio::test]
    async fn test_basic_send_receive() {
        let (mut client, mut server) = create_test_connection_pair().await.unwrap();

        let test_data = b"Hello, World!";
        client.send_bytes(test_data).await.unwrap();

        let mut buffer = [0u8; 1024];
        let n = server.receive_bytes(&mut buffer).await.unwrap().unwrap();

        assert_eq!(n, test_data.len());
        assert_eq!(&buffer[..n], test_data);
    }

    #[tokio::test]
    async fn test_connection_close() {
        let (mut client, mut server) = create_test_connection_pair().await.unwrap();

        client.close().await.unwrap();
        assert!(!client.is_connected());

        // Server should detect the closed connection
        let mut buffer = [0u8; 1024];
        let result = server.receive_bytes(&mut buffer).await.unwrap();
        assert!(result.is_none()); // EOF
    }

    #[tokio::test]
    async fn test_max_message_size() {
        let (conn, _) = create_test_connection_pair().await.unwrap();
        assert!(conn.max_message_size() > 0);
    }
}