use super::{Transport, TransportType, ConnectionInfo, DeviceInfo, TransportFactory};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// TCP transport for connecting to remote ADB daemons
pub struct TcpTransport {
    device_id: String,
    host: String,
    port: u16,
    stream: Option<TcpStream>,
    connection_timeout: Duration,
}

impl TcpTransport {
    pub fn new(device_id: String, host: String, port: u16) -> Self {
        Self {
            device_id,
            host,
            port,
            stream: None,
            connection_timeout: Duration::from_secs(5),
        }
    }

    pub fn with_timeout(device_id: String, host: String, port: u16, timeout: Duration) -> Self {
        Self {
            device_id,
            host,
            port,
            stream: None,
            connection_timeout: timeout,
        }
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn connect(&mut self) -> Result<()> {
        let stream = tokio::time::timeout(
            self.connection_timeout,
            TcpStream::connect(format!("{}:{}", self.host, self.port))
        ).await??;

        self.stream = Some(stream);
        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> Result<()> {
        match &mut self.stream {
            Some(stream) => {
                stream.write_all(data).await?;
                Ok(())
            }
            None => Err(anyhow::anyhow!("Not connected")),
        }
    }

    async fn receive(&mut self, buffer: &mut [u8]) -> Result<usize> {
        match &mut self.stream {
            Some(stream) => {
                let bytes_read = stream.read(buffer).await?;
                Ok(bytes_read)
            }
            None => Err(anyhow::anyhow!("Not connected")),
        }
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.shutdown().await;
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn device_id(&self) -> String {
        self.device_id.clone()
    }

    fn display_name(&self) -> String {
        format!("TCP Transport ({}:{})", self.host, self.port)
    }

    fn max_transfer_size(&self) -> usize {
        64 * 1024 // 64KB for TCP
    }

    fn connection_info(&self) -> ConnectionInfo {
        ConnectionInfo::Tcp {
            host: self.host.clone(),
            port: self.port,
        }
    }
}

/// Factory for discovering and creating TCP transports
pub struct TcpTransportFactory {
    pub scan_ports: Vec<u16>,
    pub scan_subnets: Vec<String>,
    pub connection_timeout: Duration,
}

impl TcpTransportFactory {
    pub fn new() -> Self {
        Self {
            scan_ports: vec![5555], // Default ADB port
            scan_subnets: vec!["127.0.0.1".to_string()],
            connection_timeout: Duration::from_secs(5),
        }
    }

    pub fn with_config(ports: Vec<u16>, subnets: Vec<String>, timeout: Duration) -> Self {
        Self {
            scan_ports: ports,
            scan_subnets: subnets,
            connection_timeout: timeout,
        }
    }

    async fn probe_host_port(&self, host: &str, port: u16) -> Option<DeviceInfo> {
        // Try to connect to see if there's an ADB daemon
        let connect_result = tokio::time::timeout(
            self.connection_timeout,
            TcpStream::connect(format!("{}:{}", host, port))
        ).await;

        if let Ok(Ok(stream)) = connect_result {
            // Connection successful, assume it's an ADB daemon
            let _ = stream;
            Some(DeviceInfo {
                device_id: format!("tcp:{}:{}", host, port),
                display_name: format!("ADB over TCP ({}:{})", host, port),
                transport_type: TransportType::Tcp,
                connection_info: ConnectionInfo::Tcp {
                    host: host.to_string(),
                    port,
                },
                capabilities: vec!["tcp".to_string(), "adb".to_string()],
            })
        } else {
            None
        }
    }
}

#[async_trait]
impl TransportFactory for TcpTransportFactory {
    async fn discover_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        // Probe all host:port combinations
        for subnet in &self.scan_subnets {
            for &port in &self.scan_ports {
                if let Some(device) = self.probe_host_port(subnet, port).await {
                    devices.push(device);
                }
            }
        }

        Ok(devices)
    }

    async fn create_transport(&self, device_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
        match &device_info.connection_info {
            ConnectionInfo::Tcp { host, port } => {
                let transport = TcpTransport::with_timeout(
                    device_info.device_id.clone(),
                    host.clone(),
                    *port,
                    self.connection_timeout,
                );
                Ok(Box::new(transport))
            }
            _ => Err(anyhow::anyhow!("Invalid connection info for TCP transport")),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn factory_name(&self) -> String {
        "TCP Transport Factory".to_string()
    }

    fn is_available(&self) -> bool {
        true // TCP is always available
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn start_mock_server(port: u16) -> TcpListener {
        TcpListener::bind(format!("127.0.0.1:{}", port)).await.unwrap()
    }

    #[tokio::test]
    async fn test_tcp_transport_connect_disconnect() {
        let listener = start_mock_server(0).await;
        let port = listener.local_addr().unwrap().port();

        // Accept connections in background
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let _ = stream;
            }
        });

        let mut transport = TcpTransport::new(
            format!("tcp:127.0.0.1:{}", port),
            "127.0.0.1".to_string(),
            port,
        );

        assert!(!transport.is_connected());

        transport.connect().await.unwrap();
        assert!(transport.is_connected());

        transport.disconnect().await.unwrap();
        assert!(!transport.is_connected());
    }

    #[tokio::test]
    async fn test_tcp_transport_send_receive() {
        let listener = start_mock_server(0).await;
        let port = listener.local_addr().unwrap().port();

        // Echo server
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    while let Ok(n) = stream.read(&mut buffer).await {
                        if n == 0 { break; }
                        let _ = stream.write_all(&buffer[..n]).await;
                    }
                });
            }
        });

        let mut transport = TcpTransport::new(
            format!("tcp:127.0.0.1:{}", port),
            "127.0.0.1".to_string(),
            port,
        );

        transport.connect().await.unwrap();

        // Test send/receive
        let test_data = b"hello world";
        transport.send(test_data).await.unwrap();

        let mut buffer = vec![0u8; 64];
        let len = transport.receive(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..len], test_data);

        transport.disconnect().await.unwrap();
    }

    #[tokio::test]
    async fn test_tcp_factory_discovery() {
        let listener = start_mock_server(0).await;
        let port = listener.local_addr().unwrap().port();

        // Accept connections
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let _ = stream;
            }
        });

        let factory = TcpTransportFactory::with_config(
            vec![port],
            vec!["127.0.0.1".to_string()],
            Duration::from_secs(1),
        );

        let devices = factory.discover_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].transport_type, TransportType::Tcp);
        assert_eq!(devices[0].device_id, format!("tcp:127.0.0.1:{}", port));

        // Test transport creation
        let transport = factory.create_transport(&devices[0]).await.unwrap();
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[tokio::test]
    async fn test_tcp_factory_no_devices() {
        let factory = TcpTransportFactory::with_config(
            vec![65432], // Very unlikely port
            vec!["192.0.2.1".to_string()], // RFC 5737 TEST-NET-1 address (should not exist)
            Duration::from_millis(50), // Shorter timeout
        );

        let devices = factory.discover_devices().await.unwrap();
        assert_eq!(devices.len(), 0);
    }
}