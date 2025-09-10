use anyhow::Result;
use async_trait::async_trait;
use bytes::{Buf, BytesMut};
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, error};

use super::{ConnectionStatus, Transport, TransportType};

pub struct TcpTransport {
    client_id: String,
    stream: Option<TcpStream>,
    is_connected: bool,
}

impl TcpTransport {
    pub fn new(client_id: String, stream: TcpStream) -> Self {
        Self {
            client_id,
            stream: Some(stream),
            is_connected: true,
        }
    }

    pub fn new_disconnected(client_id: String) -> Self {
        Self {
            client_id,
            stream: None,
            is_connected: false,
        }
    }

    async fn read_bytes_internal(&mut self) -> Result<Option<Vec<u8>>> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Stream not connected"))?;

        // Read header (24 bytes)
        let mut header = [0u8; 24];

        match stream.read_exact(&mut header).await {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                self.is_connected = false;
                return Ok(None); // Client disconnected
            }
            Err(e) => {
                self.is_connected = false;
                return Err(e.into());
            }
        }

        // Parse header to get data length
        let mut buf = BytesMut::from(&header[..]);
        let _command = buf.get_u32_le();
        let _arg0 = buf.get_u32_le();
        let _arg1 = buf.get_u32_le();
        let data_length = buf.get_u32_le();

        // Read data payload if present
        let mut full_data = Vec::with_capacity(24 + data_length as usize);
        full_data.extend_from_slice(&header);

        if data_length > 0 {
            let mut data = vec![0u8; data_length as usize];
            match stream.read_exact(&mut data).await {
                Ok(_) => full_data.extend_from_slice(&data),
                Err(e) => {
                    self.is_connected = false;
                    return Err(e.into());
                }
            }
        }

        Ok(Some(full_data))
    }

    async fn send_bytes_internal(&mut self, data: &[u8]) -> Result<()> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Stream not connected"))?;

        match stream.write_all(data).await {
            Ok(_) => {
                debug!("TCP client {} sent {} bytes", self.client_id, data.len());
                Ok(())
            }
            Err(e) => {
                self.is_connected = false;
                error!(
                    "Failed to send data to TCP client {}: {}",
                    self.client_id, e
                );
                Err(e.into())
            }
        }
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        self.send_bytes_internal(data).await
    }

    async fn receive(&mut self) -> Result<Vec<u8>> {
        match self.read_bytes_internal().await? {
            Some(data) => Ok(data),
            None => Err(anyhow::anyhow!("Connection closed")),
        }
    }

    async fn connect(&mut self) -> Result<ConnectionStatus> {
        if self.stream.is_some() {
            self.is_connected = true;
            Ok(ConnectionStatus::Ready)
        } else {
            Err(anyhow::anyhow!(
                "Cannot reconnect TCP transport - no stream available"
            ))
        }
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.shutdown().await;
        }
        self.is_connected = false;
        debug!("TCP client {} disconnected", self.client_id);
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        self.is_connected && self.stream.is_some()
    }

    fn device_id(&self) -> &str {
        &self.client_id
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    async fn health_check(&self) -> Result<()> {
        if self.is_connected && self.stream.is_some() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("TCP transport is not connected"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn test_tcp_transport_creation() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            // Just accept and close
            drop(stream);
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let transport = TcpTransport::new("test_client".to_string(), stream);

        assert!(transport.is_connected().await);
        assert_eq!(transport.device_id(), "test_client");
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[tokio::test]
    async fn test_tcp_transport_disconnected() {
        let transport = TcpTransport::new_disconnected("test_client".to_string());

        assert!(!transport.is_connected().await);
        assert!(transport.health_check().await.is_err());
    }
}
