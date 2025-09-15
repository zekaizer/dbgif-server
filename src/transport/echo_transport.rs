use crate::transport::{Transport, TransportFactory, TransportType, DeviceInfo, ConnectionInfo};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, debug, warn, error};
use std::sync::Arc;
use tokio::sync::Mutex;

const DEFAULT_ECHO_PORT: u16 = 5038;
const MAX_TRANSFER_SIZE: usize = 65536; // 64KB

/// Echo Transport Server
/// Provides a simple TCP echo service for aging tests and load testing
pub struct EchoTransportServer {
    port: u16,
    listener: Option<TcpListener>,
    is_running: Arc<Mutex<bool>>,
}

impl EchoTransportServer {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            listener: None,
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    pub fn default() -> Self {
        Self::new(DEFAULT_ECHO_PORT)
    }

    /// Start the echo server
    pub async fn start(&mut self) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.port);
        info!("Starting Echo Transport Server on {}", addr);

        let listener = TcpListener::bind(&addr).await
            .map_err(|e| anyhow!("Failed to bind echo server to {}: {}", addr, e))?;

        info!("Echo Transport Server listening on {}", addr);

        let is_running = self.is_running.clone();
        *is_running.lock().await = true;

        self.listener = Some(listener);
        Ok(())
    }

    /// Run the echo server (blocking)
    pub async fn run(&mut self) -> Result<()> {
        let listener = self.listener.take()
            .ok_or_else(|| anyhow!("Echo server not started. Call start() first."))?;

        let is_running = self.is_running.clone();

        info!("Echo Transport Server accepting connections");

        loop {
            // Check if we should stop
            if !*is_running.lock().await {
                info!("Echo Transport Server stopping");
                break;
            }

            // Accept new connection
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("Echo server accepted connection from {}", addr);

                    let is_running_clone = is_running.clone();

                    // Handle each connection in a separate task
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(stream, is_running_clone).await {
                            warn!("Error handling echo connection from {}: {}", addr, e);
                        } else {
                            debug!("Echo connection from {} completed successfully", addr);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept echo connection: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }

        Ok(())
    }

    /// Stop the echo server
    pub async fn stop(&mut self) {
        info!("Stopping Echo Transport Server");
        *self.is_running.lock().await = false;
    }

    /// Handle a single echo connection
    async fn handle_connection(mut stream: TcpStream, is_running: Arc<Mutex<bool>>) -> Result<()> {
        let peer_addr = stream.peer_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        debug!("Handling echo connection from {}", peer_addr);

        let mut buffer = vec![0u8; MAX_TRANSFER_SIZE];

        loop {
            // Check if server is still running
            if !*is_running.lock().await {
                debug!("Echo server stopping, closing connection to {}", peer_addr);
                break;
            }

            // Read data from client
            match stream.read(&mut buffer).await {
                Ok(0) => {
                    // Client disconnected
                    debug!("Echo client {} disconnected", peer_addr);
                    break;
                }
                Ok(n) => {
                    // Echo data back to client
                    debug!("Echo server received {} bytes from {}", n, peer_addr);

                    if let Err(e) = stream.write_all(&buffer[..n]).await {
                        warn!("Failed to echo data to {}: {}", peer_addr, e);
                        break;
                    }

                    debug!("Echo server sent {} bytes back to {}", n, peer_addr);
                }
                Err(e) => {
                    warn!("Failed to read from echo client {}: {}", peer_addr, e);
                    break;
                }
            }
        }

        debug!("Echo connection handler for {} completed", peer_addr);
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        // Non-async version for sync contexts
        // Note: This is a best-effort check
        self.listener.is_some()
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

/// Echo Transport Client
/// Connects to echo server for testing
pub struct EchoTransport {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
    device_id: String,
    display_name: String,
}

impl EchoTransport {
    pub fn new(host: String, port: u16) -> Self {
        let device_id = format!("echo_{}_{}", host, port);
        let display_name = format!("Echo Service ({}:{})", host, port);

        Self {
            host,
            port,
            stream: None,
            device_id,
            display_name,
        }
    }

    pub fn default() -> Self {
        Self::new("127.0.0.1".to_string(), DEFAULT_ECHO_PORT)
    }
}

#[async_trait]
impl Transport for EchoTransport {
    async fn connect(&mut self) -> Result<()> {
        let addr = format!("{}:{}", self.host, self.port);
        debug!("Connecting to echo service at {}", addr);

        let stream = TcpStream::connect(&addr).await
            .map_err(|e| anyhow!("Failed to connect to echo service at {}: {}", addr, e))?;

        self.stream = Some(stream);
        info!("Connected to echo service at {}", addr);
        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> Result<()> {
        let stream = self.stream.as_mut()
            .ok_or_else(|| anyhow!("Echo transport not connected"))?;

        stream.write_all(data).await
            .map_err(|e| anyhow!("Failed to send data to echo service: {}", e))?;

        debug!("Sent {} bytes to echo service", data.len());
        Ok(())
    }

    async fn receive(&mut self, buffer: &mut [u8]) -> Result<usize> {
        let stream = self.stream.as_mut()
            .ok_or_else(|| anyhow!("Echo transport not connected"))?;

        let bytes_read = stream.read(buffer).await
            .map_err(|e| anyhow!("Failed to receive data from echo service: {}", e))?;

        debug!("Received {} bytes from echo service", bytes_read);
        Ok(bytes_read)
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            if let Err(e) = stream.shutdown().await {
                warn!("Error shutting down echo transport connection: {}", e);
            }
            info!("Disconnected from echo service");
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Echo
    }

    fn device_id(&self) -> String {
        self.device_id.clone()
    }

    fn display_name(&self) -> String {
        self.display_name.clone()
    }

    fn max_transfer_size(&self) -> usize {
        MAX_TRANSFER_SIZE
    }

    fn connection_info(&self) -> ConnectionInfo {
        ConnectionInfo::Echo {
            host: self.host.clone(),
            port: self.port,
        }
    }
}

/// Echo Transport Factory
/// Creates echo transport instances and manages echo service discovery
pub struct EchoTransportFactory {
    host: String,
    port: u16,
}

impl EchoTransportFactory {
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }

    pub fn default() -> Self {
        Self::new("127.0.0.1".to_string(), DEFAULT_ECHO_PORT)
    }
}

#[async_trait]
impl TransportFactory for EchoTransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        // For echo transport, we create a single "device" representing the echo service
        let device_id = format!("echo_{}_{}", self.host, self.port);
        let display_name = format!("Echo Service ({}:{})", self.host, self.port);

        let device_info = DeviceInfo {
            device_id: device_id.clone(),
            display_name,
            transport_type: TransportType::Echo,
            connection_info: ConnectionInfo::Echo {
                host: self.host.clone(),
                port: self.port,
            },
            capabilities: vec![
                "echo".to_string(),
                "aging_test".to_string(),
                "load_test".to_string(),
            ],
        };

        Ok(vec![device_info])
    }

    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
        if device_info.transport_type != TransportType::Echo {
            return Err(anyhow!("Device is not an echo transport device"));
        }

        match &device_info.connection_info {
            ConnectionInfo::Echo { host, port } => {
                let transport = EchoTransport::new(host.clone(), *port);
                Ok(Box::new(transport))
            }
            _ => Err(anyhow!("Invalid connection info for echo transport")),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Echo
    }

    fn factory_name(&self) -> String {
        "Echo Transport Factory".to_string()
    }

    fn is_available(&self) -> bool {
        // Echo transport is always available as it's a software service
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_echo_transport_server_creation() {
        let server = EchoTransportServer::new(5039);
        assert_eq!(server.port(), 5039);
        assert!(!server.is_running());
    }

    #[tokio::test]
    async fn test_echo_transport_client_creation() {
        let transport = EchoTransport::new("localhost".to_string(), 5039);
        assert_eq!(transport.transport_type(), TransportType::Echo);
        assert!(!transport.is_connected());
        assert_eq!(transport.max_transfer_size(), MAX_TRANSFER_SIZE);
    }

    #[tokio::test]
    async fn test_echo_transport_factory() {
        let factory = EchoTransportFactory::new("localhost".to_string(), 5039);
        assert_eq!(factory.transport_type(), TransportType::Echo);
        assert!(factory.is_available());

        let devices = factory.discover_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].transport_type, TransportType::Echo);
    }

    #[tokio::test]
    async fn test_echo_transport_connection_failure() {
        // Test connection to non-existent echo service
        let mut transport = EchoTransport::new("127.0.0.1".to_string(), 9999);

        let result = timeout(Duration::from_millis(100), transport.connect()).await;
        assert!(result.is_err() || result.unwrap().is_err(),
               "Should fail to connect to non-existent echo service");
    }
}