use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;

/// Transport layer errors
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Connection timeout: operation took longer than {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("Connection closed unexpectedly")]
    ConnectionClosed,

    #[error("Transport error: {message}")]
    Other { message: String },

    #[error("Invalid address: {address}")]
    InvalidAddress { address: String },

    #[error("Connection failed: {reason}")]
    ConnectionFailed { reason: String },

    #[error("Data size exceeded: {size} bytes (max: {max_size})")]
    DataSizeExceeded { size: usize, max_size: usize },
}

/// Result type for transport operations
pub type TransportResult<T> = Result<T, TransportError>;

/// Connection abstraction for different transport types
/// Provides async read/write operations at byte level
#[async_trait]
pub trait Connection: Send + Sync {
    /// Send raw bytes over the connection
    async fn send_bytes(&mut self, data: &[u8]) -> TransportResult<()>;

    /// Receive raw bytes from the connection
    /// Returns None if connection is closed gracefully
    async fn receive_bytes(&mut self, buffer: &mut [u8]) -> TransportResult<Option<usize>>;

    /// Send bytes with timeout
    async fn send_bytes_timeout(&mut self, data: &[u8], timeout: Duration) -> TransportResult<()>;

    /// Receive bytes with timeout
    async fn receive_bytes_timeout(&mut self, buffer: &mut [u8], timeout: Duration) -> TransportResult<Option<usize>>;

    /// Close the connection gracefully
    async fn close(&mut self) -> TransportResult<()>;

    /// Forcefully shutdown the connection
    async fn shutdown(&mut self) -> TransportResult<()>;

    /// Check if the connection is still active
    fn is_connected(&self) -> bool;

    /// Check if the connection is closed
    fn is_closed(&self) -> bool {
        !self.is_connected()
    }

    /// Get connection identifier for logging and debugging
    fn connection_id(&self) -> String;

    /// Flush any pending write buffers
    async fn flush(&mut self) -> TransportResult<()>;

    /// Get the maximum message size for this connection
    fn max_message_size(&self) -> usize;
}





