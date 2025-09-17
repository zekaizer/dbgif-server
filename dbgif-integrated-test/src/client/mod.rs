pub mod ascii;
pub mod connection;

use anyhow::Result;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{info, debug};

pub use ascii::AsciiClient;

pub struct TestClient {
    server_addr: SocketAddr,
    stream: Option<TcpStream>,
    ascii_client: Option<AsciiClient>,
    timeout_duration: Duration,
}

impl TestClient {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self {
            server_addr,
            stream: None,
            ascii_client: None,
            timeout_duration: Duration::from_secs(10),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout_duration = timeout;
        self
    }

    pub async fn connect(&mut self) -> Result<()> {
        info!("Connecting to {}", self.server_addr);
        let stream = timeout(self.timeout_duration, TcpStream::connect(self.server_addr)).await??;
        self.ascii_client = Some(AsciiClient::new(stream));
        info!("Connected successfully");
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        self.stream = None;
        self.ascii_client = None;
        debug!("Disconnected");
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.ascii_client.is_some()
    }

    pub fn ascii(&mut self) -> Result<&mut AsciiClient> {
        self.ascii_client.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))
    }

    /// Test host services
    pub async fn test_host_services(&mut self) -> Result<()> {
        let version = self.ascii()?.test_host_version().await?;
        info!("Server version: {}", version);

        let devices = self.ascii()?.test_host_list().await?;
        info!("Connected devices: {:?}", devices);

        Ok(())
    }

    /// Connect to a device via host:connect
    pub async fn connect_device(&mut self, device_ip: &str, device_port: u16) -> Result<()> {
        self.ascii()?.test_host_connect(device_ip, device_port).await?;
        Ok(())
    }

    /// Open a service on the connected device
    pub async fn open_device_service(&mut self, service: &str) -> Result<()> {
        self.ascii()?.open_service(service).await?;
        Ok(())
    }
}