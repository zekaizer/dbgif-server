use anyhow::Result;
use bytes::{Buf, Bytes, BytesMut};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, error, info};

use crate::protocol::host_commands::HostCommand;
use crate::protocol::stream::StreamManager;
use crate::protocol::{Command, Message, VERSION};
use crate::services::HostService;
use crate::transport::TransportManager;

#[derive(Debug, Clone, PartialEq)]
pub enum ClientState {
    Disconnected,
    Connecting,
    Connected,
}

/// Stream mapping between client-server and server-device
#[derive(Debug, Clone)]
struct StreamMapping {
    client_local_id: u32,
    client_remote_id: u32,
    device_local_id: u32,
    device_remote_id: u32,
}

pub struct ClientHandler {
    client_id: u32,
    stream: TcpStream,
    state: ClientState,
    stream_manager: StreamManager,
    host_service: HostService,
    target_device_id: Option<String>,
    transport_manager: Arc<TransportManager>,
    /// Maps client local_id to device stream information
    stream_mappings: HashMap<u32, StreamMapping>,
    /// Counter for generating device-side local_ids
    device_stream_counter: u32,
}

impl ClientHandler {
    pub fn new(
        client_id: u32,
        stream: TcpStream,
        transport_manager: Arc<TransportManager>,
    ) -> Self {
        Self {
            client_id,
            stream,
            state: ClientState::Connecting,
            stream_manager: StreamManager::new(),
            host_service: HostService::new(transport_manager.clone()),
            target_device_id: None,
            transport_manager,
            stream_mappings: HashMap::new(),
            device_stream_counter: 1,
        }
    }

    pub async fn handle(&mut self) -> Result<()> {
        info!("Starting client handler for client {}", self.client_id);

        loop {
            match self.receive_message().await {
                Ok(message) => {
                    debug!(
                        "Client {} received message: {:?}",
                        self.client_id, message.command
                    );

                    if let Err(e) = self.handle_message(message).await {
                        error!(
                            "Error handling message for client {}: {}",
                            self.client_id, e
                        );
                        break;
                    }
                }
                Err(e) => {
                    error!(
                        "Error reading message from client {}: {}",
                        self.client_id, e
                    );
                    break;
                }
            }
        }

        self.state = ClientState::Disconnected;
        self.stream_manager.close_all_streams();

        // Disconnect client stream
        if let Err(e) = self.stream.shutdown().await {
            error!(
                "Failed to disconnect stream for client {}: {}",
                self.client_id, e
            );
        }

        Ok(())
    }

    async fn send_message(&mut self, message: Message) -> Result<()> {
        let data = message.serialize();
        self.stream.write_all(&data).await?;
        debug!(
            "Client {} sent message: {:?}",
            self.client_id, message.command
        );
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<Message> {
        // Read header (24 bytes)
        let mut header = [0u8; 24];

        match self.stream.read_exact(&mut header).await {
            Ok(_) => {},
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                return Err(anyhow::anyhow!("Client disconnected"));
            }
            Err(e) => {
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
        let mut full_message = BytesMut::with_capacity(24 + data_length as usize);
        full_message.extend_from_slice(&header);

        if data_length > 0 {
            let mut data = vec![0u8; data_length as usize];
            self.stream.read_exact(&mut data).await?;
            full_message.extend_from_slice(&data);
        }

        let message = Message::deserialize(full_message.freeze())?;
        Ok(message)
    }

    async fn handle_message(&mut self, message: Message) -> Result<()> {
        match message.command {
            Command::Connect => self.handle_connect(message).await,
            Command::Auth => self.handle_auth(message).await,
            Command::Open => self.handle_open(message).await,
            Command::Write => self.handle_write(message).await,
            Command::Close => self.handle_close(message).await,
            Command::Ping => self.handle_ping(message).await,
            Command::Okay => self.handle_okay(message).await,
            Command::Pong => self.handle_pong(message).await,
        }
    }

    /// Handle messages from device and relay to appropriate client
    pub async fn handle_device_message(&mut self, device_id: &str, device_message: Message) -> Result<()> {
        debug!(
            "Client {} received device message from {}: {:?}",
            self.client_id, device_id, device_message.command
        );

        // Find the stream mapping for this device message
        let mapping = self.stream_mappings.iter()
            .find(|(_, m)| m.device_local_id == device_message.arg1)
            .map(|(client_local_id, m)| (*client_local_id, m.clone()));

        if let Some((client_local_id, mapping)) = mapping {
            // Convert device stream IDs back to client stream IDs
            let client_message = Message::new(
                device_message.command,
                mapping.client_local_id,
                mapping.client_remote_id,
                device_message.data,
            );

            // Update device_remote_id if this is an OKAY response to OPEN
            if device_message.command == Command::Okay && mapping.device_remote_id == 0 {
                if let Some(mapping_mut) = self.stream_mappings.get_mut(&client_local_id) {
                    mapping_mut.device_remote_id = device_message.arg0;
                    debug!(
                        "Updated device_remote_id for client stream {} to {}",
                        client_local_id, device_message.arg0
                    );
                }
            }

            debug!(
                "Relaying device message to client {}: device_stream={}:{} -> client_stream={}:{}",
                self.client_id,
                device_message.arg1, device_message.arg0,
                client_message.arg1, client_message.arg0
            );

            self.send_message(client_message).await?;

            // Clean up stream mapping if this is a close message
            if device_message.command == Command::Close {
                self.stream_mappings.remove(&client_local_id);
                debug!(
                    "Removed stream mapping for client stream {}",
                    client_local_id
                );
            }
        } else {
            error!(
                "No stream mapping found for device message from {}: device_stream={}",
                device_id, device_message.arg1
            );
        }

        Ok(())
    }

    async fn handle_connect(&mut self, _message: Message) -> Result<()> {
        info!("Client {} initiated CNXN handshake", self.client_id);

        // Simplified auth - skip auth token request and go straight to connected
        let response = Message::new(
            Command::Connect,
            VERSION,
            4096, // Max message size for CNXN
            "device::".as_bytes(),
        );

        self.send_message(response).await?;
        self.state = ClientState::Connected;

        info!("Client {} connected successfully", self.client_id);
        Ok(())
    }

    async fn handle_auth(&mut self, _message: Message) -> Result<()> {
        // Simplified auth - always accept
        debug!(
            "Client {} sent AUTH, accepting without verification",
            self.client_id
        );

        let response = Message::new(Command::Connect, VERSION, 4096, "device::".as_bytes());

        self.send_message(response).await?;
        self.state = ClientState::Connected;

        Ok(())
    }

    async fn handle_open(&mut self, message: Message) -> Result<()> {
        let remote_id = message.arg0;
        let local_id = self.stream_manager.create_stream(remote_id);

        // Extract service name from data
        let service_name = String::from_utf8_lossy(&message.data).to_string();

        debug!(
            "Client {} opened stream {} for service: {}",
            self.client_id, local_id, service_name
        );

        // Check if this is a host command - only these are handled locally
        if service_name.starts_with("host:") {
            if let Ok(host_command) = HostCommand::parse(&service_name) {
                return self
                    .handle_host_command(local_id, remote_id, host_command)
                    .await;
            }
        }

        // All non-host services should be relayed to target device
        self.relay_to_device(message, local_id).await?;
        Ok(())
    }

    async fn handle_host_command(
        &mut self,
        local_id: u32,
        remote_id: u32,
        command: HostCommand,
    ) -> Result<()> {
        use crate::services::host_service::HostServiceResponse;

        debug!(
            "Client {} executing host command: {:?}",
            self.client_id, command
        );

        match self.host_service.execute_command(command).await {
            Ok(response) => {
                match response {
                    HostServiceResponse::SingleResponse(data) => {
                        // Send OKAY first
                        let okay_msg =
                            Message::new(Command::Okay, local_id, remote_id, Bytes::new());
                        self.send_message(okay_msg).await?;

                        // Send data if not empty
                        if !data.is_empty() {
                            let data_msg = Message::new(Command::Write, local_id, remote_id, data);
                            self.send_message(data_msg).await?;
                        }

                        // Close stream
                        let close_msg =
                            Message::new(Command::Close, local_id, remote_id, Bytes::new());
                        self.send_message(close_msg).await?;
                    }
                    HostServiceResponse::TransportSelected(device_id) => {
                        // Set target device for subsequent commands
                        self.target_device_id = Some(device_id);

                        // Send OKAY to confirm transport selection
                        let okay_msg =
                            Message::new(Command::Okay, local_id, remote_id, Bytes::new());
                        self.send_message(okay_msg).await?;
                    }
                    HostServiceResponse::StreamingResponse(_rx) => {
                        // TODO: Implement streaming for track-devices
                        // For now, just send OKAY
                        let okay_msg =
                            Message::new(Command::Okay, local_id, remote_id, Bytes::new());
                        self.send_message(okay_msg).await?;
                    }
                }
            }
            Err(e) => {
                error!("Host command failed for client {}: {}", self.client_id, e);

                // Send error response
                let error_data = format!("FAIL{:04x}{}", e.to_string().len(), e);
                let fail_msg =
                    Message::new(Command::Write, local_id, remote_id, Bytes::from(error_data));
                self.send_message(fail_msg).await?;

                // Close stream
                let close_msg = Message::new(Command::Close, local_id, remote_id, Bytes::new());
                self.send_message(close_msg).await?;
            }
        }

        Ok(())
    }

    async fn handle_write(&mut self, message: Message) -> Result<()> {
        let remote_id = message.arg0;
        let local_id = message.arg1;

        debug!(
            "Client {} received WRTE for stream {}:{}",
            self.client_id, local_id, remote_id
        );

        // For now, just send OKAY back
        let response = Message::new(Command::Okay, local_id, remote_id, Bytes::new());

        self.send_message(response).await?;
        Ok(())
    }

    async fn handle_close(&mut self, message: Message) -> Result<()> {
        let remote_id = message.arg0;
        let local_id = message.arg1;

        if let Some(mut stream) = self.stream_manager.remove_stream(local_id) {
            stream.close();
            debug!(
                "Client {} closed stream {}:{}",
                self.client_id, local_id, remote_id
            );
        }

        Ok(())
    }

    async fn handle_ping(&mut self, message: Message) -> Result<()> {
        debug!("Client {} sent PING", self.client_id);

        let response = Message::new(
            Command::Pong,
            message.arg0,
            message.arg1,
            message.data.clone(),
        );

        self.send_message(response).await?;
        Ok(())
    }

    async fn handle_okay(&mut self, _message: Message) -> Result<()> {
        debug!("Client {} sent OKAY", self.client_id);
        Ok(())
    }

    async fn handle_pong(&mut self, _message: Message) -> Result<()> {
        debug!("Client {} sent PONG", self.client_id);
        Ok(())
    }

    /// Relay message to target device
    async fn relay_to_device(&mut self, message: Message, client_local_id: u32) -> Result<()> {
        let target_device_id = match self.target_device_id.clone() {
            Some(device_id) => device_id,
            None => {
                error!("No target device selected for client {}", self.client_id);
                let error_data = format!("FAIL{:04x}No target device selected", 24);
                let fail_msg = Message::new(Command::Write, client_local_id, message.arg0, Bytes::from(error_data));
                self.send_message(fail_msg).await?;
                
                let close_msg = Message::new(Command::Close, client_local_id, message.arg0, Bytes::new());
                self.send_message(close_msg).await?;
                return Ok(());
            }
        };

        let command = message.command;
        let arg0 = message.arg0;
        let data = message.data.clone();
        
        // Create stream mapping for OPEN messages
        if command == Command::Open {
            let device_local_id = self.device_stream_counter;
            self.device_stream_counter += 1;
            
            let mapping = StreamMapping {
                client_local_id,
                client_remote_id: arg0,
                device_local_id,
                device_remote_id: 0, // Will be set when device responds
            };
            
            self.stream_mappings.insert(client_local_id, mapping);
            
            // Create new message with device stream ID
            let device_message = Message::new(
                command,
                device_local_id,
                0, // Initial remote_id for device
                data,
            );
            
            debug!(
                "Client {} relaying OPEN to device {}: client_stream={} -> device_stream={}",
                self.client_id, target_device_id, client_local_id, device_local_id
            );
            
            // Send to device and wait for response
            match self.transport_manager.send_message(&target_device_id, &device_message).await {
                Ok(()) => {
                    // Wait for device response
                    match self.transport_manager.receive_message(&target_device_id).await {
                        Ok(device_response) => {
                            // Handle the device response using our device message handler
                            if let Err(e) = self.handle_device_message(&target_device_id, device_response).await {
                                error!("Failed to handle device response for client {}: {}", self.client_id, e);
                                
                                // Send error and cleanup on failure
                                self.stream_mappings.remove(&client_local_id);
                                let error_data = format!("FAIL{:04x}{}", e.to_string().len(), e);
                                let fail_msg = Message::new(Command::Write, client_local_id, arg0, Bytes::from(error_data));
                                self.send_message(fail_msg).await?;
                                
                                let close_msg = Message::new(Command::Close, client_local_id, arg0, Bytes::new());
                                self.send_message(close_msg).await?;
                            }
                        }
                        Err(e) => {
                            error!("Failed to receive response from device {}: {}", target_device_id, e);
                            
                            // Remove failed stream mapping
                            self.stream_mappings.remove(&client_local_id);
                            
                            let error_data = format!("FAIL{:04x}{}", e.to_string().len(), e);
                            let fail_msg = Message::new(Command::Write, client_local_id, arg0, Bytes::from(error_data));
                            self.send_message(fail_msg).await?;
                            
                            let close_msg = Message::new(Command::Close, client_local_id, arg0, Bytes::new());
                            self.send_message(close_msg).await?;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to send message to device {}: {}", target_device_id, e);
                    
                    // Remove failed stream mapping
                    self.stream_mappings.remove(&client_local_id);
                    
                    let error_data = format!("FAIL{:04x}{}", e.to_string().len(), e);
                    let fail_msg = Message::new(Command::Write, client_local_id, arg0, Bytes::from(error_data));
                    self.send_message(fail_msg).await?;
                    
                    let close_msg = Message::new(Command::Close, client_local_id, arg0, Bytes::new());
                    self.send_message(close_msg).await?;
                }
            }
        } else {
            // For non-OPEN messages, find existing stream mapping
            if let Some(mapping) = self.stream_mappings.get(&client_local_id) {
                debug!(
                    "Client {} relaying {:?} to device {}: client_stream={} -> device_stream={}",
                    self.client_id, command, target_device_id, client_local_id, mapping.device_local_id
                );
                
                let device_message = Message::new(
                    command,
                    mapping.device_local_id,
                    mapping.device_remote_id,
                    data,
                );
                
                // Send to device and wait for response
                match self.transport_manager.send_message(&target_device_id, &device_message).await {
                    Ok(()) => {
                        // For WRITE/CLOSE messages, wait for device response
                        if command == Command::Write || command == Command::Close {
                            match self.transport_manager.receive_message(&target_device_id).await {
                                Ok(device_response) => {
                                    if let Err(e) = self.handle_device_message(&target_device_id, device_response).await {
                                        error!("Failed to handle device response for client {}: {}", self.client_id, e);
                                        let error_data = format!("FAIL{:04x}{}", e.to_string().len(), e);
                                        let fail_msg = Message::new(Command::Write, client_local_id, arg0, Bytes::from(error_data));
                                        self.send_message(fail_msg).await?;
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to receive response from device {}: {}", target_device_id, e);
                                    let error_data = format!("FAIL{:04x}{}", e.to_string().len(), e);
                                    let fail_msg = Message::new(Command::Write, client_local_id, arg0, Bytes::from(error_data));
                                    self.send_message(fail_msg).await?;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to send message to device {}: {}", target_device_id, e);
                        let error_data = format!("FAIL{:04x}{}", e.to_string().len(), e);
                        let fail_msg = Message::new(Command::Write, client_local_id, arg0, Bytes::from(error_data));
                        self.send_message(fail_msg).await?;
                    }
                }
            } else {
                error!("No stream mapping found for client stream {}", client_local_id);
                let error_data = format!("FAIL{:04x}Stream not found", 17);
                let fail_msg = Message::new(Command::Write, client_local_id, arg0, Bytes::from(error_data));
                self.send_message(fail_msg).await?;
            }
        }
        
        Ok(())
    }
}
