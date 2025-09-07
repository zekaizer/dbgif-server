use tracing::{info, debug, error};
use bytes::Bytes;
use anyhow::Result;

use crate::protocol::{Message, Command, VERSION};
use crate::protocol::stream::StreamManager;
use crate::transport::Transport;

#[derive(Debug, Clone, PartialEq)]
pub enum ClientState {
    Disconnected,
    Connecting,
    Connected,
}

pub struct ClientHandler {
    client_id: u32,
    transport: Box<dyn Transport + Send>,
    state: ClientState,
    stream_manager: StreamManager,
}

impl ClientHandler {
    pub fn new(client_id: u32, transport: Box<dyn Transport + Send>) -> Self {
        Self {
            client_id,
            transport,
            state: ClientState::Connecting,
            stream_manager: StreamManager::new(),
        }
    }

    pub async fn handle(&mut self) -> Result<()> {
        info!("Starting client handler for client {}", self.client_id);

        loop {
            match self.transport.receive_message().await {
                Ok(message) => {
                    debug!("Client {} received message: {:?}", self.client_id, message.command);
                    
                    if let Err(e) = self.handle_message(message).await {
                        error!("Error handling message for client {}: {}", self.client_id, e);
                        break;
                    }
                }
                Err(e) => {
                    error!("Error reading message from client {}: {}", self.client_id, e);
                    break;
                }
            }
        }

        self.state = ClientState::Disconnected;
        self.stream_manager.close_all_streams();
        
        // Disconnect transport
        if let Err(e) = self.transport.disconnect().await {
            error!("Failed to disconnect transport for client {}: {}", self.client_id, e);
        }
        
        Ok(())
    }

    async fn send_message(&mut self, message: Message) -> Result<()> {
        self.transport.send_message(&message).await?;
        debug!("Client {} sent message: {:?}", self.client_id, message.command);
        Ok(())
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

    async fn handle_connect(&mut self, _message: Message) -> Result<()> {
        info!("Client {} initiated CNXN handshake", self.client_id);
        
        // Simplified auth - skip auth token request and go straight to connected
        let response = Message::new(
            Command::Connect,
            VERSION,
            4096, // Max message size for CNXN
            "device::".as_bytes()
        );
        
        self.send_message(response).await?;
        self.state = ClientState::Connected;
        
        info!("Client {} connected successfully", self.client_id);
        Ok(())
    }

    async fn handle_auth(&mut self, _message: Message) -> Result<()> {
        // Simplified auth - always accept
        debug!("Client {} sent AUTH, accepting without verification", self.client_id);
        
        let response = Message::new(
            Command::Connect,
            VERSION,
            4096,
            "device::".as_bytes()
        );
        
        self.send_message(response).await?;
        self.state = ClientState::Connected;
        
        Ok(())
    }

    async fn handle_open(&mut self, message: Message) -> Result<()> {
        let remote_id = message.arg0;
        let local_id = self.stream_manager.create_stream(remote_id);
        
        // Extract service name from data
        let service_name = String::from_utf8_lossy(&message.data).to_string();
        
        if let Some(stream) = self.stream_manager.get_stream_mut(local_id) {
            stream.set_service(service_name.clone());
            stream.open();
        }
        
        debug!("Client {} opened stream {} for service: {}", 
               self.client_id, local_id, service_name);
        
        // Send OKAY response
        let response = Message::new(
            Command::Okay,
            local_id,
            remote_id,
            Bytes::new()
        );
        
        self.send_message(response).await?;
        Ok(())
    }

    async fn handle_write(&mut self, message: Message) -> Result<()> {
        let remote_id = message.arg0;
        let local_id = message.arg1;
        
        debug!("Client {} received WRTE for stream {}:{}", 
               self.client_id, local_id, remote_id);
        
        // For now, just send OKAY back
        let response = Message::new(
            Command::Okay,
            local_id,
            remote_id,
            Bytes::new()
        );
        
        self.send_message(response).await?;
        Ok(())
    }

    async fn handle_close(&mut self, message: Message) -> Result<()> {
        let remote_id = message.arg0;
        let local_id = message.arg1;
        
        if let Some(mut stream) = self.stream_manager.remove_stream(local_id) {
            stream.close();
            debug!("Client {} closed stream {}:{}", 
                   self.client_id, local_id, remote_id);
        }
        
        Ok(())
    }

    async fn handle_ping(&mut self, message: Message) -> Result<()> {
        debug!("Client {} sent PING", self.client_id);
        
        let response = Message::new(
            Command::Pong,
            message.arg0,
            message.arg1,
            message.data.clone()
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
}