use anyhow::Result;
use dbgif_protocol::ascii;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info};

/// ASCII protocol client implementation
pub struct AsciiClient {
    stream: TcpStream,
}

impl AsciiClient {
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }

    /// Send ASCII command
    pub async fn send_command(&mut self, cmd: &str) -> Result<()> {
        debug!("Sending ASCII command: {}", cmd);
        let encoded = ascii::encode_request(cmd);
        self.stream.write_all(&encoded).await?;
        Ok(())
    }

    /// Receive ASCII response
    pub async fn receive_response(&mut self) -> Result<(bool, String)> {
        let mut buffer = vec![0u8; 4096];
        let size = self.stream.read(&mut buffer).await?;

        if size > 0 {
            let (success, data) = ascii::decode_response(&buffer[..size])?;
            let response = String::from_utf8_lossy(&data).to_string();
            debug!("Received response: success={}, data={}", success, response);
            Ok((success, response))
        } else {
            Err(anyhow::anyhow!("Connection closed"))
        }
    }

    /// Send STRM data
    pub async fn send_strm(&mut self, stream_id: u8, data: &[u8]) -> Result<()> {
        debug!("Sending STRM data: stream_id={}, size={}", stream_id, data.len());
        let encoded = ascii::encode_strm(stream_id, data);
        self.stream.write_all(&encoded).await?;
        Ok(())
    }

    /// Receive STRM data
    pub async fn receive_strm(&mut self) -> Result<(u8, Vec<u8>)> {
        let mut buffer = vec![0u8; 4096];
        let size = self.stream.read(&mut buffer).await?;

        if size > 0 {
            let (stream_id, data) = ascii::decode_strm(&buffer[..size])?;
            debug!("Received STRM: stream_id={}, size={}", stream_id, data.len());
            Ok((stream_id, data))
        } else {
            Err(anyhow::anyhow!("Connection closed"))
        }
    }

    /// Test host:version command
    pub async fn test_host_version(&mut self) -> Result<String> {
        info!("Testing host:version");
        self.send_command("host:version").await?;
        let (success, response) = self.receive_response().await?;

        if success {
            info!("✅ host:version successful: {}", response);
            Ok(response)
        } else {
            Err(anyhow::anyhow!("host:version failed: {}", response))
        }
    }

    /// Test host:list command
    pub async fn test_host_list(&mut self) -> Result<Vec<String>> {
        info!("Testing host:list");
        self.send_command("host:list").await?;
        let (success, response) = self.receive_response().await?;

        if success {
            let devices: Vec<String> = response.lines()
                .filter(|line| !line.is_empty())
                .map(|s| s.to_string())
                .collect();
            info!("✅ host:list successful: {} devices", devices.len());
            Ok(devices)
        } else {
            Err(anyhow::anyhow!("host:list failed: {}", response))
        }
    }

    /// Test host:connect command
    pub async fn test_host_connect(&mut self, ip: &str, port: u16) -> Result<()> {
        info!("Testing host:connect to {}:{}", ip, port);
        let command = format!("host:connect:{}:{}", ip, port);
        self.send_command(&command).await?;
        let (success, response) = self.receive_response().await?;

        if success {
            info!("✅ host:connect successful: {}", response);
            Ok(())
        } else {
            Err(anyhow::anyhow!("host:connect failed: {}", response))
        }
    }

    /// Open a service on the selected device
    pub async fn open_service(&mut self, service: &str) -> Result<()> {
        info!("Opening service: {}", service);
        self.send_command(service).await?;
        let (success, response) = self.receive_response().await?;

        if success {
            info!("✅ Service '{}' opened successfully: {}", service, response);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to open service '{}': {}", service, response))
        }
    }
}