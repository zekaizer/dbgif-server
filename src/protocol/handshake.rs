use super::{AdbMessage as Message, Command};
use anyhow::{Result, Context};
use bytes::Bytes;
use std::time::{Duration, Instant};

/// ADB protocol handshake handler for connection establishment
pub struct HandshakeHandler {
    version: u32,
    max_data_size: u32,
    system_identity: String,
    auth_required: bool,
    handshake_timeout: Duration,
}

impl HandshakeHandler {
    pub fn new(system_identity: String) -> Self {
        Self {
            version: 0x01000000, // ADB version 1.0
            max_data_size: 256 * 1024, // 256KB max payload
            system_identity,
            auth_required: false, // Simplified auth as per requirements
            handshake_timeout: Duration::from_secs(10),
        }
    }

    pub fn with_config(
        system_identity: String,
        version: u32,
        max_data_size: u32,
        auth_required: bool,
        timeout: Duration,
    ) -> Self {
        Self {
            version,
            max_data_size,
            system_identity,
            auth_required,
            handshake_timeout: timeout,
        }
    }

    /// Create initial CNXN message for client handshake
    pub fn create_connect_message(&self) -> Message {
        Message::new(
            Command::CNXN,
            self.version,
            self.max_data_size,
            Bytes::from(self.system_identity.as_bytes().to_vec()),
        )
    }

    /// Process incoming CNXN message and create appropriate response
    pub fn process_connect_message(&self, message: &Message) -> Result<HandshakeResponse> {
        if message.command != Command::CNXN {
            return Err(anyhow::anyhow!(
                "Expected CNXN message, got {:?}",
                message.command
            ));
        }

        let client_version = message.arg0;
        let client_max_data = message.arg1;
        let client_identity = String::from_utf8_lossy(&message.payload);

        tracing::info!(
            "Received CNXN from client: version=0x{:08x}, max_data={}, identity='{}'",
            client_version,
            client_max_data,
            client_identity
        );

        // Negotiate parameters
        let negotiated_max_data = std::cmp::min(self.max_data_size, client_max_data);

        if self.auth_required {
            // Request authentication
            let auth_message = Message::new(
                Command::AUTH,
                1, // AUTH_TOKEN type
                0,
                Bytes::from("auth_required"),
            );

            Ok(HandshakeResponse::AuthRequired {
                auth_message,
                negotiated_version: self.version,
                negotiated_max_data,
                client_identity: client_identity.to_string(),
            })
        } else {
            // Skip auth - create CNXN response
            let response = Message::new(
                Command::CNXN,
                self.version,
                negotiated_max_data,
                Bytes::from(self.system_identity.as_bytes().to_vec()),
            );

            Ok(HandshakeResponse::Connected {
                response_message: response,
                negotiated_version: self.version,
                negotiated_max_data,
                client_identity: client_identity.to_string(),
            })
        }
    }

    /// Process AUTH message (simplified - always accept)
    pub fn process_auth_message(&self, message: &Message) -> Result<Message> {
        if message.command != Command::AUTH {
            return Err(anyhow::anyhow!(
                "Expected AUTH message, got {:?}",
                message.command
            ));
        }

        tracing::debug!("Processing AUTH message (simplified mode - auto-accept)");

        // In simplified mode, respond with CNXN to complete handshake
        Ok(Message::new(
            Command::CNXN,
            self.version,
            self.max_data_size,
            Bytes::from(self.system_identity.as_bytes().to_vec()),
        ))
    }

    /// Validate handshake completion
    pub fn validate_handshake_complete(
        &self,
        client_version: u32,
        server_version: u32,
    ) -> Result<HandshakeResult> {
        // Check version compatibility
        let client_major = (client_version >> 24) & 0xFF;
        let server_major = (server_version >> 24) & 0xFF;

        if client_major != server_major {
            return Err(anyhow::anyhow!(
                "Version mismatch: client major {} != server major {}",
                client_major,
                server_major
            ));
        }

        let result = HandshakeResult {
            negotiated_version: std::cmp::min(client_version, server_version),
            max_data_size: self.max_data_size,
            auth_completed: !self.auth_required,
            connection_established: true,
        };

        tracing::info!(
            "Handshake completed: version=0x{:08x}, max_data={}",
            result.negotiated_version,
            result.max_data_size
        );

        Ok(result)
    }

    /// Create a handshake timeout error
    pub fn create_timeout_error(&self) -> anyhow::Error {
        anyhow::anyhow!(
            "Handshake timeout after {:?}",
            self.handshake_timeout
        )
    }

    /// Get handshake timeout duration
    pub fn timeout(&self) -> Duration {
        self.handshake_timeout
    }
}

/// Result of processing a CNXN message
#[derive(Debug, Clone)]
pub enum HandshakeResponse {
    /// Authentication required before connection
    AuthRequired {
        auth_message: Message,
        negotiated_version: u32,
        negotiated_max_data: u32,
        client_identity: String,
    },
    /// Connection established (auth skipped or completed)
    Connected {
        response_message: Message,
        negotiated_version: u32,
        negotiated_max_data: u32,
        client_identity: String,
    },
}

/// Final result of handshake process
#[derive(Debug, Clone)]
pub struct HandshakeResult {
    pub negotiated_version: u32,
    pub max_data_size: u32,
    pub auth_completed: bool,
    pub connection_established: bool,
}

/// State machine for tracking handshake progress
#[derive(Debug, Clone, PartialEq)]
pub enum HandshakeState {
    /// Waiting for initial CNXN message
    WaitingForConnect,
    /// Waiting for AUTH response
    WaitingForAuth {
        negotiated_version: u32,
        negotiated_max_data: u32,
        client_identity: String,
    },
    /// Handshake completed successfully
    Connected {
        negotiated_version: u32,
        negotiated_max_data: u32,
        client_identity: String,
    },
    /// Handshake failed
    Failed(String),
}

/// Stateful handshake manager
pub struct HandshakeManager {
    handler: HandshakeHandler,
    state: HandshakeState,
    start_time: Option<Instant>,
}

impl HandshakeManager {
    pub fn new(handler: HandshakeHandler) -> Self {
        Self {
            handler,
            state: HandshakeState::WaitingForConnect,
            start_time: None,
        }
    }

    /// Start the handshake process
    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
        self.state = HandshakeState::WaitingForConnect;
    }

    /// Process incoming message and update state
    pub fn process_message(&mut self, message: &Message) -> Result<Option<Message>> {
        // Check timeout
        if let Some(start_time) = self.start_time {
            if start_time.elapsed() > self.handler.timeout() {
                self.state = HandshakeState::Failed("Timeout".to_string());
                return Err(self.handler.create_timeout_error());
            }
        }

        match &self.state {
            HandshakeState::WaitingForConnect => {
                if message.command != Command::CNXN {
                    let error = format!("Expected CNXN, got {:?}", message.command);
                    self.state = HandshakeState::Failed(error.clone());
                    return Err(anyhow::anyhow!(error));
                }

                match self.handler.process_connect_message(message)? {
                    HandshakeResponse::AuthRequired {
                        auth_message,
                        negotiated_version,
                        negotiated_max_data,
                        client_identity,
                    } => {
                        self.state = HandshakeState::WaitingForAuth {
                            negotiated_version,
                            negotiated_max_data,
                            client_identity,
                        };
                        Ok(Some(auth_message))
                    }
                    HandshakeResponse::Connected {
                        response_message,
                        negotiated_version,
                        negotiated_max_data,
                        client_identity,
                    } => {
                        self.state = HandshakeState::Connected {
                            negotiated_version,
                            negotiated_max_data,
                            client_identity,
                        };
                        Ok(Some(response_message))
                    }
                }
            }
            HandshakeState::WaitingForAuth { .. } => {
                if message.command != Command::AUTH {
                    let error = format!("Expected AUTH, got {:?}", message.command);
                    self.state = HandshakeState::Failed(error.clone());
                    return Err(anyhow::anyhow!(error));
                }

                let response = self.handler.process_auth_message(message)?;

                // Extract previous negotiation results
                if let HandshakeState::WaitingForAuth {
                    negotiated_version,
                    negotiated_max_data,
                    client_identity,
                } = &self.state
                {
                    self.state = HandshakeState::Connected {
                        negotiated_version: *negotiated_version,
                        negotiated_max_data: *negotiated_max_data,
                        client_identity: client_identity.clone(),
                    };
                }

                Ok(Some(response))
            }
            HandshakeState::Connected { .. } => {
                Err(anyhow::anyhow!("Handshake already completed"))
            }
            HandshakeState::Failed(error) => {
                Err(anyhow::anyhow!("Handshake failed: {}", error))
            }
        }
    }

    /// Get current handshake state
    pub fn state(&self) -> &HandshakeState {
        &self.state
    }

    /// Check if handshake is completed
    pub fn is_completed(&self) -> bool {
        matches!(self.state, HandshakeState::Connected { .. })
    }

    /// Check if handshake failed
    pub fn is_failed(&self) -> bool {
        matches!(self.state, HandshakeState::Failed(_))
    }

    /// Get handshake result if completed
    pub fn get_result(&self) -> Option<HandshakeResult> {
        match &self.state {
            HandshakeState::Connected {
                negotiated_version,
                negotiated_max_data,
                ..
            } => Some(HandshakeResult {
                negotiated_version: *negotiated_version,
                max_data_size: *negotiated_max_data,
                auth_completed: true,
                connection_established: true,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_handler_creation() {
        let handler = HandshakeHandler::new("test_system".to_string());

        assert_eq!(handler.version, 0x01000000);
        assert_eq!(handler.max_data_size, 256 * 1024);
        assert_eq!(handler.system_identity, "test_system");
        assert!(!handler.auth_required);
    }

    #[test]
    fn test_create_connect_message() {
        let handler = HandshakeHandler::new("test_device".to_string());
        let message = handler.create_connect_message();

        assert_eq!(message.command, Command::CNXN);
        assert_eq!(message.arg0, 0x01000000);
        assert_eq!(message.arg1, 256 * 1024);
        assert_eq!(message.payload, Bytes::from("test_device"));
    }

    #[test]
    fn test_process_connect_no_auth() {
        let handler = HandshakeHandler::new("server".to_string());

        let client_message = Message::new(
            Command::CNXN,
            0x01000000,
            128 * 1024,
            Bytes::from("client"),
        );

        let response = handler.process_connect_message(&client_message).unwrap();

        match response {
            HandshakeResponse::Connected {
                response_message,
                negotiated_version,
                negotiated_max_data,
                client_identity,
            } => {
                assert_eq!(response_message.command, Command::CNXN);
                assert_eq!(negotiated_version, 0x01000000);
                assert_eq!(negotiated_max_data, 128 * 1024); // Min of server and client
                assert_eq!(client_identity, "client");
            }
            _ => panic!("Expected Connected response"),
        }
    }

    #[test]
    fn test_process_connect_with_auth() {
        let handler = HandshakeHandler::with_config(
            "server".to_string(),
            0x01000000,
            256 * 1024,
            true, // Auth required
            Duration::from_secs(10),
        );

        let client_message = Message::new(
            Command::CNXN,
            0x01000000,
            128 * 1024,
            Bytes::from("client"),
        );

        let response = handler.process_connect_message(&client_message).unwrap();

        match response {
            HandshakeResponse::AuthRequired {
                auth_message,
                negotiated_version,
                negotiated_max_data,
                client_identity,
            } => {
                assert_eq!(auth_message.command, Command::AUTH);
                assert_eq!(negotiated_version, 0x01000000);
                assert_eq!(negotiated_max_data, 128 * 1024);
                assert_eq!(client_identity, "client");
            }
            _ => panic!("Expected AuthRequired response"),
        }
    }

    #[test]
    fn test_handshake_manager_flow() {
        let handler = HandshakeHandler::new("server".to_string());
        let mut manager = HandshakeManager::new(handler);

        manager.start();
        assert_eq!(manager.state(), &HandshakeState::WaitingForConnect);

        // Send CNXN message
        let client_message = Message::new(
            Command::CNXN,
            0x01000000,
            128 * 1024,
            Bytes::from("client"),
        );

        let response = manager.process_message(&client_message).unwrap();
        assert!(response.is_some());
        assert!(manager.is_completed());

        let result = manager.get_result().unwrap();
        assert_eq!(result.negotiated_version, 0x01000000);
        assert_eq!(result.max_data_size, 128 * 1024);
        assert!(result.connection_established);
    }

    #[test]
    fn test_version_validation() {
        let handler = HandshakeHandler::new("test".to_string());

        // Compatible versions (same major)
        let result = handler.validate_handshake_complete(0x01000001, 0x01000002);
        assert!(result.is_ok());

        // Incompatible versions (different major)
        let result = handler.validate_handshake_complete(0x01000001, 0x02000001);
        assert!(result.is_err());
    }
}