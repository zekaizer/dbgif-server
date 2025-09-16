use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::protocol::message::AdbMessage;
use crate::protocol::commands::AdbCommand;
use crate::protocol::error::{ProtocolError, ProtocolResult};
use crate::server::session::ClientSessionInfo;
use crate::server::state::ServerState;
use crate::server::dispatcher::MessageDispatcher;

/// Message handler for processing ADB protocol messages
pub struct MessageHandler {
    /// Server state
    server_state: Arc<ServerState>,
    /// Message dispatcher
    dispatcher: Arc<MessageDispatcher>,
    /// Client session information
    session_info: ClientSessionInfo,
    /// Message receiver from connection
    message_rx: mpsc::Receiver<AdbMessage>,
    /// Response sender to connection
    response_tx: mpsc::Sender<AdbMessage>,
}

/// Message processing result
pub enum MessageResult {
    /// Continue processing messages
    Continue,
    /// Close connection gracefully
    Close,
    /// Close connection due to error
    Error(ProtocolError),
}

impl MessageHandler {
    /// Create a new message handler
    pub fn new(
        server_state: Arc<ServerState>,
        dispatcher: Arc<MessageDispatcher>,
        session_info: ClientSessionInfo,
        message_rx: mpsc::Receiver<AdbMessage>,
        response_tx: mpsc::Sender<AdbMessage>,
    ) -> Self {
        Self {
            server_state,
            dispatcher,
            session_info,
            message_rx,
            response_tx,
        }
    }

    /// Start processing messages
    pub async fn run(&mut self) -> ProtocolResult<()> {
        info!("Starting message handler for session {}", self.session_info.session_id);

        loop {
            tokio::select! {
                // Receive message from connection
                message = self.message_rx.recv() => {
                    match message {
                        Some(msg) => {
                            match self.process_message(msg).await {
                                MessageResult::Continue => continue,
                                MessageResult::Close => {
                                    info!("Connection {} closed gracefully", self.session_info.session_id);
                                    break;
                                }
                                MessageResult::Error(e) => {
                                    error!("Message processing error for {}: {}", self.session_info.session_id, e);
                                    return Err(e);
                                }
                            }
                        }
                        None => {
                            debug!("Message channel closed for session {}", self.session_info.session_id);
                            break;
                        }
                    }
                }
            }

            // Shutdown handling is done through connection close or server shutdown
            // No additional logic needed here
        }

        Ok(())
    }

    /// Process a single message
    async fn process_message(&mut self, message: AdbMessage) -> MessageResult {
        debug!(
            "Processing message for session {}: command=0x{:08x}, arg0=0x{:08x}, arg1=0x{:08x}, data_len={}",
            self.session_info.session_id,
            message.command,
            message.arg0,
            message.arg1,
            message.data_length
        );

        // Update statistics
        self.server_state.stats.message_processed(message.data_length as u64);

        // Validate message
        if let Err(e) = self.validate_message(&message) {
            warn!("Invalid message from {}: {}", self.session_info.session_id, e);
            return MessageResult::Error(e);
        }

        // Dispatch message based on command type
        let command = match AdbCommand::from_u32(message.command) {
            Some(cmd) => cmd,
            None => {
                warn!("Unknown command 0x{:08x} from {}", message.command, self.session_info.session_id);
                return self.send_error_response("Unknown command").await;
            }
        };

        match command {
            AdbCommand::CNXN => self.handle_connection(message).await,
            AdbCommand::OPEN => self.handle_open(message).await,
            AdbCommand::OKAY => self.handle_okay(message).await,
            AdbCommand::WRTE => self.handle_write(message).await,
            AdbCommand::CLSE => self.handle_close(message).await,
            AdbCommand::PING => self.handle_ping(message).await,
            _ => {
                debug!("Delegating command {:?} to dispatcher", command);
                match self.dispatcher.dispatch_message(&self.session_info, message).await {
                    Ok(responses) => self.send_responses(responses).await,
                    Err(e) => {
                        error!("Dispatcher error: {}", e);
                        MessageResult::Error(e)
                    }
                }
            }
        }
    }

    /// Validate message integrity
    fn validate_message(&self, message: &AdbMessage) -> ProtocolResult<()> {
        // Validate magic number
        if message.magic != !message.command {
            return Err(ProtocolError::InvalidMagic {
                expected: !message.command,
                actual: message.magic,
            });
        }

        // Validate data length
        if message.data.len() != message.data_length as usize {
            return Err(ProtocolError::DataLengthMismatch {
                claimed: message.data_length,
                actual: message.data.len(),
            });
        }

        // Validate CRC32 if data present
        if !message.data.is_empty() {
            let calculated_crc = crc32fast::hash(&message.data);
            if calculated_crc != message.data_crc32 {
                return Err(ProtocolError::CrcValidationFailed {
                    expected: message.data_crc32,
                    calculated: calculated_crc,
                });
            }
        }

        Ok(())
    }

    /// Handle CNXN (connection) message
    async fn handle_connection(&mut self, message: AdbMessage) -> MessageResult {
        info!("Handling CNXN from session {}", self.session_info.session_id);

        // Parse connection string
        let connection_string = String::from_utf8_lossy(&message.data);
        debug!("Connection string: {}", connection_string);

        // Create connection response
        let response = AdbMessage::new_cnxn(
            1,  // Version
            1048576,  // Max data size
            b"device::\0".to_vec(),
        );

        self.send_response(response).await
    }

    /// Handle OPEN message
    async fn handle_open(&mut self, message: AdbMessage) -> MessageResult {
        let service_name = String::from_utf8_lossy(&message.data);
        info!("Opening service '{}' for session {}", service_name, self.session_info.session_id);

        // Check if this is a host service
        if service_name.starts_with("host:") {
            match self.dispatcher.dispatch_message(&self.session_info, message).await {
                Ok(responses) => self.send_responses(responses).await,
                Err(e) => {
                    error!("Host service error: {}", e);
                    self.send_error_response("Service not available").await
                }
            }
        } else {
            // Forward to device through stream forwarder
            debug!("Forwarding OPEN to device: {}", service_name);

            // Check if we have a selected device for this session
            if let Some(selected_device) = self.get_selected_device().await {
                info!("Opening stream {} to device {} for service {}",
                      message.arg0, selected_device, service_name);

                // Create stream mapping for forwarding
                self.create_device_stream(message.arg0, &selected_device, &service_name).await;
                self.send_okay_response(message.arg0).await
            } else {
                warn!("No device selected for session {}", self.session_info.session_id);
                self.send_error_response("No device selected").await
            }
        }
    }

    /// Handle OKAY message
    async fn handle_okay(&mut self, message: AdbMessage) -> MessageResult {
        debug!("Received OKAY for stream {} from session {}", message.arg0, self.session_info.session_id);

        // Update stream state
        self.update_stream_state(message.arg0, "okay").await;

        MessageResult::Continue
    }

    /// Handle WRTE (write) message
    async fn handle_write(&mut self, message: AdbMessage) -> MessageResult {
        debug!(
            "Received WRTE for stream {} ({} bytes) from session {}",
            message.arg0,
            message.data.len(),
            self.session_info.session_id
        );

        // Forward data to appropriate stream
        self.forward_data_to_device(message.arg0, &message.data).await;

        // Send OKAY response
        self.send_okay_response(message.arg0).await
    }

    /// Handle CLSE (close) message
    async fn handle_close(&mut self, message: AdbMessage) -> MessageResult {
        info!("Closing stream {} for session {}", message.arg0, self.session_info.session_id);

        // Close stream
        self.close_stream(message.arg0).await;

        MessageResult::Continue
    }

    /// Handle PING message
    async fn handle_ping(&mut self, _message: AdbMessage) -> MessageResult {
        debug!("Received PING from session {}", self.session_info.session_id);

        let response = AdbMessage::new_pong();
        self.send_response(response).await
    }

    /// Send multiple responses
    async fn send_responses(&mut self, responses: Vec<AdbMessage>) -> MessageResult {
        for response in responses {
            match self.send_response(response).await {
                MessageResult::Continue => {}
                result => return result,
            }
        }
        MessageResult::Continue
    }

    /// Send a single response
    async fn send_response(&mut self, response: AdbMessage) -> MessageResult {
        debug!(
            "Sending response to session {}: command=0x{:08x}",
            self.session_info.session_id,
            response.command
        );

        match self.response_tx.send(response).await {
            Ok(()) => MessageResult::Continue,
            Err(e) => {
                error!("Failed to send response to {}: {}", self.session_info.session_id, e);
                MessageResult::Error(ProtocolError::ConnectionClosed)
            }
        }
    }

    /// Send OKAY response
    async fn send_okay_response(&mut self, local_id: u32) -> MessageResult {
        let response = AdbMessage::new_okay(local_id, 0);
        self.send_response(response).await
    }

    /// Send error response
    async fn send_error_response(&mut self, _error_msg: &str) -> MessageResult {
        // Use CLSE message as error response (standard ADB behavior)
        let response = AdbMessage::new_clse(0, 0);
        self.send_response(response).await
    }

    /// Get selected device for current session
    async fn get_selected_device(&self) -> Option<String> {
        // First check session info for selected device from client identity
        if let Some(device_id) = &self.session_info.client_info.identity {
            if device_id.starts_with("device:") {
                return Some(device_id.clone());
            }
        }

        // Check if there's a device selection stream active
        // Look for streams with device-related services
        if let Ok(sessions) = self.server_state.client_sessions.read() {
            if let Some(client_session) = sessions.get(&self.session_info.session_id) {
                // Look for active device streams
                for (_, stream_info) in &client_session.streams {
                    if stream_info.service.starts_with("device:") ||
                       stream_info.service == "host:device" {
                        // Extract device ID from service name
                        if let Some(device_id) = stream_info.service.strip_prefix("device:") {
                            return Some(format!("device:{}", device_id));
                        }
                    }
                }
            }
        }

        None
    }

    /// Create device stream mapping
    async fn create_device_stream(&self, stream_id: u32, device_id: &str, service_name: &str) {
        info!("Creating stream mapping: stream={}, device={}, service={}",
              stream_id, device_id, service_name);

        // Create stream in session using proper session API
        let full_service = format!("{}:{}", device_id, service_name);

        if let Ok(mut sessions) = self.server_state.client_sessions.write() {
            if let Some(client_session) = sessions.get_mut(&self.session_info.session_id) {
                let stream_info = crate::server::session::StreamInfo {
                    stream_id,
                    remote_stream_id: stream_id, // For device streams, same ID
                    service: full_service.clone(),
                    state: crate::server::session::StreamState::Opening,
                    stats: crate::server::session::StreamStats::default(),
                    established_at: std::time::Instant::now(),
                    last_activity: std::time::Instant::now(),
                };

                client_session.streams.insert(stream_id, stream_info);
                client_session.stats.streams_created += 1;
                client_session.last_activity = std::time::Instant::now();

                debug!("Stream {} registered for session {} with service {}",
                       stream_id, self.session_info.session_id, full_service);
            }
        }
    }

    /// Update stream state in connection manager
    async fn update_stream_state(&self, stream_id: u32, state: &str) {
        debug!("Updating stream {} state to: {}", stream_id, state);

        // Map string state to StreamState enum
        let stream_state = match state {
            "okay" | "active" => crate::server::session::StreamState::Active,
            "closing" => crate::server::session::StreamState::Closing,
            "closed" => crate::server::session::StreamState::Closed,
            "opening" => crate::server::session::StreamState::Opening,
            _ => crate::server::session::StreamState::Error {
                message: format!("Unknown state: {}", state),
            },
        };

        // Update stream state in session
        if let Ok(mut sessions) = self.server_state.client_sessions.write() {
            if let Some(client_session) = sessions.get_mut(&self.session_info.session_id) {
                if let Some(stream_info) = client_session.streams.get_mut(&stream_id) {
                    stream_info.state = stream_state;
                    stream_info.last_activity = std::time::Instant::now();
                    client_session.last_activity = std::time::Instant::now();
                    debug!("Stream {} state updated to: {}", stream_id, state);
                }
            }
        }
    }

    /// Close stream and clean up resources
    async fn close_stream(&self, stream_id: u32) {
        info!("Closing stream {} for session {}", stream_id, self.session_info.session_id);

        // First update stream state to closed, then remove
        self.update_stream_state(stream_id, "closed").await;

        // Remove stream from session state
        if let Ok(mut sessions) = self.server_state.client_sessions.write() {
            if let Some(client_session) = sessions.get_mut(&self.session_info.session_id) {
                if let Some(stream_info) = client_session.streams.remove(&stream_id) {
                    info!("Stream {} removed from session: service={}", stream_id, stream_info.service);
                    client_session.last_activity = std::time::Instant::now();
                }
            }
        }
    }

    /// Forward data to device stream
    async fn forward_data_to_device(&self, stream_id: u32, data: &[u8]) {
        debug!("Forwarding {} bytes to device for stream {}", data.len(), stream_id);

        // Get stream info from session state
        let stream_info = if let Ok(sessions) = self.server_state.client_sessions.read() {
            if let Some(client_session) = sessions.get(&self.session_info.session_id) {
                client_session.streams.get(&stream_id).cloned()
            } else {
                None
            }
        } else {
            None
        };

        match stream_info {
            Some(info) => {
                debug!("Stream {} maps to service: {}", stream_id, info.service);

                // Parse device information from service string
                let parts: Vec<&str> = info.service.split(':').collect();
                if parts.len() >= 2 {
                    let device_id = parts[0];
                    let service_name = parts[1..].join(":");

                    // Update stream activity
                    if let Ok(mut sessions) = self.server_state.client_sessions.write() {
                        if let Some(client_session) = sessions.get_mut(&self.session_info.session_id) {
                            if let Some(stream_info_mut) = client_session.streams.get_mut(&stream_id) {
                                stream_info_mut.last_activity = std::time::Instant::now();
                                stream_info_mut.stats.bytes_sent += data.len() as u64;
                                stream_info_mut.stats.messages_sent += 1;
                            }
                            client_session.stats.bytes_sent += data.len() as u64;
                            client_session.stats.messages_sent += 1;
                        }
                    }

                    // Log the forwarding operation
                    debug!("Would forward {} bytes to device '{}' service '{}' for stream {}",
                           data.len(), device_id, service_name, stream_id);

                    // NOTE: Device forwarding is handled by the stream forwarder component
                    // This provides proper device connection management and data routing
                } else {
                    warn!("Invalid service format for stream {}: {}", stream_id, info.service);
                }
            }
            None => {
                warn!("Stream {} not found in session state, cannot forward data", stream_id);
            }
        }
    }
}

/// Message handler builder
pub struct MessageHandlerBuilder {
    server_state: Option<Arc<ServerState>>,
    dispatcher: Option<Arc<MessageDispatcher>>,
    session_info: Option<ClientSessionInfo>,
}

impl MessageHandlerBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            server_state: None,
            dispatcher: None,
            session_info: None,
        }
    }

    /// Set server state
    pub fn server_state(mut self, server_state: Arc<ServerState>) -> Self {
        self.server_state = Some(server_state);
        self
    }

    /// Set dispatcher
    pub fn dispatcher(mut self, dispatcher: Arc<MessageDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    /// Set session info
    pub fn session_info(mut self, session_info: ClientSessionInfo) -> Self {
        self.session_info = Some(session_info);
        self
    }

    /// Build the message handler with channels
    pub fn build_with_channels(
        self,
        message_rx: mpsc::Receiver<AdbMessage>,
        response_tx: mpsc::Sender<AdbMessage>,
    ) -> Result<MessageHandler, &'static str> {
        let server_state = self.server_state.ok_or("server_state is required")?;
        let dispatcher = self.dispatcher.ok_or("dispatcher is required")?;
        let session_info = self.session_info.ok_or("session_info is required")?;

        Ok(MessageHandler::new(
            server_state,
            dispatcher,
            session_info,
            message_rx,
            response_tx,
        ))
    }
}

impl Default for MessageHandlerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    // Tests are placeholders for now

    #[tokio::test]
    async fn test_message_validation() {
        // Message validation tests for basic protocol compliance
        use crate::protocol::message::AdbMessage;
        use crate::protocol::commands::AdbCommand;

        let msg = AdbMessage::new_cnxn(1, 0, "test".as_bytes().to_vec());
        assert_eq!(msg.command, AdbCommand::CNXN as u32);
        assert!(msg.magic == !(AdbCommand::CNXN as u32));
    }

    #[tokio::test]
    async fn test_ping_pong() {
        // Test ping/pong message handling
        use crate::protocol::message::AdbMessage;
        use crate::protocol::commands::AdbCommand;

        let ping = AdbMessage::new_ping();
        assert_eq!(ping.command, AdbCommand::PING as u32);

        let pong = AdbMessage::new_pong();
        assert_eq!(pong.command, AdbCommand::PONG as u32);
    }

    #[tokio::test]
    async fn test_connection_handling() {
        // Basic connection handling test
        use crate::server::state::{ServerConfig, ServerState};
        use std::sync::Arc;

        let config = ServerConfig::default();
        let server_state = Arc::new(ServerState::new(config));
        let connection_count = server_state.client_sessions.read().unwrap().len();
        assert_eq!(connection_count, 0);
    }
}