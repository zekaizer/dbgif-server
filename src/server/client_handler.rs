use crate::protocol::{AdbMessage, Command};
use crate::transport::TransportManager;
use crate::session::AdbSessionManager;
use crate::server::host_commands::{HostCommandProcessor, HostCommandResponse};
use anyhow::{Result, Context};
use bytes::Bytes;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, error, info, warn};
use std::collections::HashMap;

/// Handles individual client connections and ADB command processing
pub struct ClientHandler {
    client_id: String,
    stream: TcpStream,
    peer_addr: SocketAddr,
    transport_manager: Arc<TransportManager>,
    session_manager: Arc<AdbSessionManager>,
    host_processor: HostCommandProcessor,

    // Client state
    is_authenticated: bool,
    selected_device: Option<String>,
    active_streams: HashMap<u32, StreamInfo>,
    next_stream_id: u32,
}

#[derive(Debug, Clone)]
struct StreamInfo {
    local_id: u32,
    remote_id: u32,
    service_name: String,
    is_host_service: bool,
}

impl ClientHandler {
    pub fn new(
        client_id: String,
        stream: TcpStream,
        transport_manager: Arc<TransportManager>,
        session_manager: Arc<AdbSessionManager>,
    ) -> Self {
        let peer_addr = stream.peer_addr().unwrap_or_else(|_| {
            "unknown".parse().unwrap()
        });

        Self {
            client_id,
            stream,
            peer_addr,
            host_processor: HostCommandProcessor::new(transport_manager.clone()),
            transport_manager,
            session_manager,
            is_authenticated: false,
            selected_device: None,
            active_streams: HashMap::new(),
            next_stream_id: 1,
        }
    }

    /// Main client handling loop
    pub async fn handle(mut self) -> Result<()> {
        info!("Starting client handler for {}: {}", self.client_id, self.peer_addr);

        loop {
            match self.receive_message().await {
                Ok(message) => {
                    debug!(
                        "Client {} received: {:?} (arg0={}, arg1={})",
                        self.client_id, message.command, message.arg0, message.arg1
                    );

                    if let Err(e) = self.handle_message(message).await {
                        error!("Error handling message for {}: {}", self.client_id, e);
                        break;
                    }
                }
                Err(e) => {
                    info!("Client {} disconnected: {}", self.client_id, e);
                    break;
                }
            }
        }

        // Cleanup on disconnect
        self.cleanup().await?;
        Ok(())
    }

    /// Receive ADB message from client
    async fn receive_message(&mut self) -> Result<AdbMessage> {
        // Read exactly 24 bytes for header
        let mut header_buf = [0u8; AdbMessage::HEADER_SIZE];
        self.stream.read_exact(&mut header_buf).await
            .context("Failed to read message header")?;

        // Parse header to determine payload size
        let data_length = u32::from_le_bytes([
            header_buf[12], header_buf[13], header_buf[14], header_buf[15]
        ]);

        if data_length > AdbMessage::MAX_PAYLOAD_SIZE as u32 {
            return Err(anyhow::anyhow!(
                "Payload too large: {} bytes (max {})",
                data_length,
                AdbMessage::MAX_PAYLOAD_SIZE
            ));
        }

        // Read payload if present
        let mut full_data = Vec::with_capacity(AdbMessage::HEADER_SIZE + data_length as usize);
        full_data.extend_from_slice(&header_buf);

        if data_length > 0 {
            let mut payload_buf = vec![0u8; data_length as usize];
            self.stream.read_exact(&mut payload_buf).await
                .context("Failed to read message payload")?;
            full_data.extend_from_slice(&payload_buf);
        }

        // Deserialize complete message
        let header = &full_data[..AdbMessage::HEADER_SIZE];
        let payload = if data_length > 0 {
            Bytes::from(full_data[AdbMessage::HEADER_SIZE..].to_vec())
        } else {
            Bytes::new()
        };

        AdbMessage::deserialize(header, payload)
            .context("Failed to deserialize ADB message")
    }

    /// Send ADB message to client
    async fn send_message(&mut self, message: &AdbMessage) -> Result<()> {
        let data = message.serialize().context("Failed to serialize message")?;
        self.stream.write_all(&data).await
            .context("Failed to send message")?;

        debug!(
            "Client {} sent: {:?} (payload_len={})",
            self.client_id, message.command, message.payload.len()
        );

        Ok(())
    }

    /// Handle incoming ADB message
    async fn handle_message(&mut self, message: AdbMessage) -> Result<()> {
        match message.command {
            Command::CNXN => self.handle_connect(message).await,
            Command::AUTH => self.handle_auth(message).await,
            Command::OPEN => self.handle_open(message).await,
            Command::WRTE => self.handle_write(message).await,
            Command::CLSE => self.handle_close(message).await,
            Command::PING => self.handle_ping(message).await,
            Command::OKAY => self.handle_okay(message).await,
            Command::PONG => self.handle_pong(message).await,
        }
    }

    /// Handle CNXN (Connect) message
    async fn handle_connect(&mut self, message: AdbMessage) -> Result<()> {
        info!("Client {} requesting connection", self.client_id);

        // Extract client version and system identity
        let client_version = message.arg0;
        let max_data = message.arg1;
        let system_identity = String::from_utf8_lossy(&message.payload);

        debug!(
            "Client {}: version=0x{:08x}, max_data={}, identity='{}'",
            self.client_id, client_version, max_data, system_identity
        );

        // According to simplified auth requirement - skip AUTH and go straight to connected
        let response = AdbMessage::new(
            Command::CNXN,
            0x01000000, // Server ADB version
            256 * 1024, // Max data size we support
            Bytes::from(b"device::\0".to_vec()), // Server identity
        );

        self.send_message(&response).await?;
        self.is_authenticated = true;

        info!("Client {} connected successfully (simplified auth)", self.client_id);
        Ok(())
    }

    /// Handle AUTH (Authentication) message - simplified to always accept
    async fn handle_auth(&mut self, _message: AdbMessage) -> Result<()> {
        debug!("Client {} sent AUTH - accepting (simplified mode)", self.client_id);

        // Send CNXN response to complete handshake
        let response = AdbMessage::new(
            Command::CNXN,
            0x01000000,
            256 * 1024,
            Bytes::from(b"device::\0".to_vec()),
        );

        self.send_message(&response).await?;
        self.is_authenticated = true;

        Ok(())
    }

    /// Handle OPEN (Open Stream) message
    async fn handle_open(&mut self, message: AdbMessage) -> Result<()> {
        if !self.is_authenticated {
            warn!("Client {} tried to open stream before authentication", self.client_id);
            return Err(anyhow::anyhow!("Not authenticated"));
        }

        let remote_id = message.arg0;
        let service_name = String::from_utf8_lossy(&message.payload).trim_end_matches('\0').to_string();

        debug!(
            "Client {} opening stream for service: '{}'",
            self.client_id, service_name
        );

        // Check if this is a host service
        if service_name.starts_with("host:") {
            self.handle_host_service_open(remote_id, service_name).await
        } else {
            self.handle_device_service_open(remote_id, service_name).await
        }
    }

    /// Handle host service OPEN requests
    async fn handle_host_service_open(&mut self, remote_id: u32, service_name: String) -> Result<()> {
        let local_id = self.next_stream_id;
        self.next_stream_id += 1;

        let stream_info = StreamInfo {
            local_id,
            remote_id,
            service_name: service_name.clone(),
            is_host_service: true,
        };

        self.active_streams.insert(local_id, stream_info);

        // Execute host command using processor
        match self.host_processor.execute(&service_name).await {
            Ok(response) => self.handle_host_response(local_id, remote_id, response).await,
            Err(e) => {
                error!("Host command failed: {}", e);
                self.send_host_error(local_id, remote_id, &format!("host command error: {}", e)).await
            }
        }
    }

    /// Handle device service OPEN requests
    async fn handle_device_service_open(&mut self, remote_id: u32, service_name: String) -> Result<()> {
        // Ensure we have a selected device
        let device_id = match &self.selected_device {
            Some(id) => id.clone(),
            None => {
                let local_id = self.next_stream_id;
                self.next_stream_id += 1;
                self.send_host_error(local_id, remote_id, "no device selected").await?;
                return Ok(());
            }
        };

        debug!(
            "Client {} forwarding service '{}' to device '{}'",
            self.client_id, service_name, device_id
        );

        // For now, just send OKAY - session manager integration will be completed later
        // TODO: Implement proper session manager integration
        let local_id = self.next_stream_id;
        self.next_stream_id += 1;

        let stream_info = StreamInfo {
            local_id,
            remote_id,
            service_name,
            is_host_service: false,
        };

        self.active_streams.insert(local_id, stream_info);

        // Send OKAY to client
        let okay_response = AdbMessage::new(
            Command::OKAY,
            local_id,
            remote_id,
            Bytes::new(),
        );

        self.send_message(&okay_response).await?;
        debug!("Stream opened successfully: local_id={}", local_id);

        Ok(())
    }

    /// Handle host command response
    async fn handle_host_response(&mut self, local_id: u32, remote_id: u32, response: HostCommandResponse) -> Result<()> {
        match response {
            HostCommandResponse::Data { data } => {
                // Send OKAY first
                let okay_response = AdbMessage::new(
                    Command::OKAY,
                    local_id,
                    remote_id,
                    Bytes::new(),
                );
                self.send_message(&okay_response).await?;

                // Send data
                if !data.is_empty() {
                    let data_message = AdbMessage::new(
                        Command::WRTE,
                        local_id,
                        remote_id,
                        data,
                    );
                    self.send_message(&data_message).await?;
                }

                // Close stream
                let close_message = AdbMessage::new(
                    Command::CLSE,
                    local_id,
                    remote_id,
                    Bytes::new(),
                );
                self.send_message(&close_message).await?;
                self.active_streams.remove(&local_id);
            }
            HostCommandResponse::Error { message } => {
                self.send_host_error(local_id, remote_id, &message).await?;
            }
            HostCommandResponse::TransportSelected { device_id, .. } => {
                self.selected_device = Some(device_id.clone());

                // Send OKAY to confirm transport selection
                let okay_response = AdbMessage::new(
                    Command::OKAY,
                    local_id,
                    remote_id,
                    Bytes::new(),
                );
                self.send_message(&okay_response).await?;

                info!("Client {} selected device: {}", self.client_id, device_id);
            }
            HostCommandResponse::Streaming { initial_data } => {
                // Send OKAY first
                let okay_response = AdbMessage::new(
                    Command::OKAY,
                    local_id,
                    remote_id,
                    Bytes::new(),
                );
                self.send_message(&okay_response).await?;

                // Send initial data
                if !initial_data.is_empty() {
                    let data_message = AdbMessage::new(
                        Command::WRTE,
                        local_id,
                        remote_id,
                        initial_data,
                    );
                    self.send_message(&data_message).await?;
                }

                // Keep stream open for streaming
                // TODO: Implement actual streaming logic
            }
            HostCommandResponse::Kill => {
                // Send OKAY to acknowledge
                let okay_response = AdbMessage::new(
                    Command::OKAY,
                    local_id,
                    remote_id,
                    Bytes::new(),
                );
                self.send_message(&okay_response).await?;

                info!("Client {} requested server shutdown", self.client_id);
                // TODO: Signal server shutdown
            }
        }

        Ok(())
    }

    /// Send host service error response
    async fn send_host_error(&mut self, local_id: u32, remote_id: u32, error_msg: &str) -> Result<()> {
        let fail_data = format!("FAIL{:04x}{}", error_msg.len(), error_msg);
        let error_message = AdbMessage::new(
            Command::WRTE,
            local_id,
            remote_id,
            Bytes::from(fail_data.into_bytes()),
        );
        self.send_message(&error_message).await?;

        // Close the stream
        let close_message = AdbMessage::new(
            Command::CLSE,
            local_id,
            remote_id,
            Bytes::new(),
        );
        self.send_message(&close_message).await?;
        self.active_streams.remove(&local_id);

        Ok(())
    }

    /// Handle WRTE (Write) message
    async fn handle_write(&mut self, message: AdbMessage) -> Result<()> {
        let local_id = message.arg0;
        let remote_id = message.arg1;

        if let Some(stream_info) = self.active_streams.get(&remote_id) {
            if stream_info.is_host_service {
                // Host services don't typically receive write data after opening
                debug!("Ignoring write to host service stream {}", remote_id);

                // Send OKAY acknowledgment
                let okay_response = AdbMessage::new(
                    Command::OKAY,
                    remote_id,
                    local_id,
                    Bytes::new(),
                );
                self.send_message(&okay_response).await?;
            } else {
                // Forward to device
                if let Some(device_id) = &self.selected_device {
                    // TODO: Forward write data to device through session manager
                    debug!("Forwarding write data to device {}: {} bytes", device_id, message.payload.len());

                    // For now, just send OKAY
                    let okay_response = AdbMessage::new(
                        Command::OKAY,
                        remote_id,
                        local_id,
                        Bytes::new(),
                    );
                    self.send_message(&okay_response).await?;
                }
            }
        } else {
            warn!("Write to unknown stream: {}", remote_id);
        }

        Ok(())
    }

    /// Handle CLSE (Close) message
    async fn handle_close(&mut self, message: AdbMessage) -> Result<()> {
        let remote_id = message.arg1;

        if let Some(stream_info) = self.active_streams.remove(&remote_id) {
            debug!("Closing stream {}: {}", remote_id, stream_info.service_name);

            if !stream_info.is_host_service {
                // TODO: Close stream on device side through session manager
                if let Some(device_id) = &self.selected_device {
                    debug!("Closing device stream for device {}", device_id);
                }
            }
        }

        Ok(())
    }

    /// Handle PING message
    async fn handle_ping(&mut self, message: AdbMessage) -> Result<()> {
        debug!("Client {} sent PING", self.client_id);

        let pong_response = AdbMessage::new(
            Command::PONG,
            message.arg1,
            message.arg0,
            message.payload.clone(),
        );

        self.send_message(&pong_response).await?;
        Ok(())
    }

    /// Handle OKAY message
    async fn handle_okay(&mut self, _message: AdbMessage) -> Result<()> {
        debug!("Client {} sent OKAY", self.client_id);
        // OKAY messages are typically acknowledgments - no specific action needed
        Ok(())
    }

    /// Handle PONG message
    async fn handle_pong(&mut self, _message: AdbMessage) -> Result<()> {
        debug!("Client {} sent PONG", self.client_id);
        Ok(())
    }

    /// Cleanup when client disconnects
    async fn cleanup(&mut self) -> Result<()> {
        info!("Cleaning up client {}", self.client_id);

        // Close all active streams
        for (stream_id, stream_info) in &self.active_streams {
            debug!("Cleaning up stream {}: {}", stream_id, stream_info.service_name);

            if !stream_info.is_host_service {
                if let Some(device_id) = &self.selected_device {
                    // TODO: Clean up device streams through session manager
                    debug!("Would close device stream {} for device {}", stream_id, device_id);
                }
            }
        }

        self.active_streams.clear();

        // Release device ownership if any
        if let Some(device_id) = &self.selected_device {
            if let Err(e) = self.transport_manager.release_device(device_id, self.client_id.clone()).await {
                warn!("Failed to release device {}: {}", device_id, e);
            }
        }

        Ok(())
    }
}