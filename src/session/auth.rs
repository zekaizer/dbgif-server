use crate::protocol::{AdbMessage as Message, Command};
use anyhow::Result;
use bytes::Bytes;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Simplified ADB authentication handler
///
/// Per requirements: "인증 과정은 간소화하여 구현합니다"
/// This implementation auto-accepts all authentication attempts for simplicity
pub struct AuthHandler {
    auth_enabled: bool,
    auto_accept: bool,
    challenge_timeout: Duration,
}

impl AuthHandler {
    /// Create new auth handler with default simplified settings
    pub fn new() -> Self {
        Self {
            auth_enabled: false, // Disabled by default for simplicity
            auto_accept: true,   // Auto-accept when enabled
            challenge_timeout: Duration::from_secs(30),
        }
    }

    /// Create auth handler with custom settings
    pub fn with_config(auth_enabled: bool, auto_accept: bool, timeout: Duration) -> Self {
        Self {
            auth_enabled,
            auto_accept,
            challenge_timeout: timeout,
        }
    }

    /// Create an AUTH challenge message (simplified)
    pub fn create_auth_challenge(&self) -> Message {
        if !self.auth_enabled {
            // If auth is disabled, create a dummy challenge that will be auto-accepted
            return Message::new(
                Command::AUTH,
                AuthType::Token as u32,
                0,
                Bytes::from("no_auth_required"),
            );
        }

        // Simple timestamp-based challenge
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let challenge_data = format!("simplified_challenge_{}", timestamp);

        Message::new(
            Command::AUTH,
            AuthType::Token as u32,
            0,
            Bytes::from(challenge_data.as_bytes().to_vec()),
        )
    }

    /// Process incoming AUTH response (simplified - always accepts)
    pub fn process_auth_response(&self, message: &Message) -> Result<AuthResult> {
        if message.command != Command::AUTH {
            return Err(anyhow::anyhow!(
                "Expected AUTH message, got {:?}",
                message.command
            ));
        }

        let auth_type = AuthType::try_from(message.arg0)?;
        let auth_data = &message.payload;

        tracing::debug!(
            "Processing AUTH response: type={:?}, data_len={}",
            auth_type,
            auth_data.len()
        );

        if !self.auth_enabled {
            // Auth disabled - always accept
            tracing::info!("Authentication bypassed (disabled)");
            return Ok(AuthResult::Success {
                method: auth_type,
                message: "Authentication disabled".to_string(),
            });
        }

        if self.auto_accept {
            // Auto-accept mode - always accept
            tracing::info!("Authentication auto-accepted (simplified mode)");
            return Ok(AuthResult::Success {
                method: auth_type,
                message: "Auto-accepted in simplified mode".to_string(),
            });
        }

        // In a real implementation, this would validate the auth response
        // For now, we accept based on basic checks
        match auth_type {
            AuthType::Token => {
                if auth_data.is_empty() {
                    Ok(AuthResult::Failed {
                        reason: "Empty auth token".to_string(),
                    })
                } else {
                    Ok(AuthResult::Success {
                        method: auth_type,
                        message: "Token accepted (simplified validation)".to_string(),
                    })
                }
            }
            AuthType::Signature => {
                // In real ADB, this would verify RSA signature
                // For simplified mode, we just check if data is present
                if auth_data.len() >= 32 {
                    Ok(AuthResult::Success {
                        method: auth_type,
                        message: "Signature accepted (simplified validation)".to_string(),
                    })
                } else {
                    Ok(AuthResult::Failed {
                        reason: "Invalid signature format".to_string(),
                    })
                }
            }
            AuthType::RsaPublicKey => {
                // In real ADB, this would validate RSA public key
                // For simplified mode, we just check basic format
                if auth_data.len() >= 64 {
                    Ok(AuthResult::Success {
                        method: auth_type,
                        message: "RSA key accepted (simplified validation)".to_string(),
                    })
                } else {
                    Ok(AuthResult::Failed {
                        reason: "Invalid RSA key format".to_string(),
                    })
                }
            }
        }
    }

    /// Check if authentication is required
    pub fn is_auth_required(&self) -> bool {
        self.auth_enabled
    }

    /// Get challenge timeout
    pub fn challenge_timeout(&self) -> Duration {
        self.challenge_timeout
    }

    /// Create CNXN response after successful auth
    pub fn create_connection_response(&self, system_identity: &str) -> Message {
        Message::new(
            Command::CNXN,
            0x01000000, // ADB version
            256 * 1024, // Max data size
            Bytes::from(system_identity.as_bytes().to_vec()),
        )
    }
}

impl Default for AuthHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// ADB authentication types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AuthType {
    Token = 1,
    Signature = 2,
    RsaPublicKey = 3,
}

impl TryFrom<u32> for AuthType {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(AuthType::Token),
            2 => Ok(AuthType::Signature),
            3 => Ok(AuthType::RsaPublicKey),
            _ => Err(anyhow::anyhow!("Invalid auth type: {}", value)),
        }
    }
}

/// Result of authentication processing
#[derive(Debug, Clone)]
pub enum AuthResult {
    Success {
        method: AuthType,
        message: String,
    },
    Failed {
        reason: String,
    },
    RequiresChallenge {
        challenge_message: Message,
    },
}

/// Authentication state tracker
#[derive(Debug, Clone, PartialEq)]
pub enum AuthState {
    /// No authentication required
    NotRequired,
    /// Waiting for initial auth response
    WaitingForResponse,
    /// Authentication completed successfully
    Authenticated { method: AuthType },
    /// Authentication failed
    Failed { reason: String },
}

/// Stateful authentication manager
pub struct AuthManager {
    handler: AuthHandler,
    state: AuthState,
    challenge_sent_at: Option<std::time::Instant>,
}

impl AuthManager {
    pub fn new(handler: AuthHandler) -> Self {
        let initial_state = if handler.is_auth_required() {
            AuthState::WaitingForResponse
        } else {
            AuthState::NotRequired
        };

        Self {
            handler,
            state: initial_state,
            challenge_sent_at: None,
        }
    }

    /// Start authentication process
    pub fn start_auth(&mut self) -> Option<Message> {
        if !self.handler.is_auth_required() {
            self.state = AuthState::NotRequired;
            return None;
        }

        self.state = AuthState::WaitingForResponse;
        self.challenge_sent_at = Some(std::time::Instant::now());

        Some(self.handler.create_auth_challenge())
    }

    /// Process auth response and update state
    pub fn process_response(&mut self, message: &Message) -> Result<Option<Message>> {
        // Check timeout
        if let Some(sent_at) = self.challenge_sent_at {
            if sent_at.elapsed() > self.handler.challenge_timeout() {
                self.state = AuthState::Failed {
                    reason: "Authentication timeout".to_string(),
                };
                return Err(anyhow::anyhow!("Authentication timeout"));
            }
        }

        match &self.state {
            AuthState::WaitingForResponse => {
                match self.handler.process_auth_response(message)? {
                    AuthResult::Success { method, .. } => {
                        self.state = AuthState::Authenticated { method };
                        // Return CNXN response to complete handshake
                        Ok(Some(self.handler.create_connection_response("adb_server")))
                    }
                    AuthResult::Failed { reason } => {
                        self.state = AuthState::Failed { reason: reason.clone() };
                        Err(anyhow::anyhow!("Authentication failed: {}", reason))
                    }
                    AuthResult::RequiresChallenge { challenge_message } => {
                        // Send another challenge
                        self.challenge_sent_at = Some(std::time::Instant::now());
                        Ok(Some(challenge_message))
                    }
                }
            }
            AuthState::NotRequired => {
                Err(anyhow::anyhow!("Authentication not required"))
            }
            AuthState::Authenticated { .. } => {
                Err(anyhow::anyhow!("Already authenticated"))
            }
            AuthState::Failed { reason } => {
                Err(anyhow::anyhow!("Authentication failed: {}", reason))
            }
        }
    }

    /// Get current authentication state
    pub fn state(&self) -> &AuthState {
        &self.state
    }

    /// Check if authentication is completed
    pub fn is_authenticated(&self) -> bool {
        matches!(self.state, AuthState::Authenticated { .. } | AuthState::NotRequired)
    }

    /// Check if authentication failed
    pub fn is_failed(&self) -> bool {
        matches!(self.state, AuthState::Failed { .. })
    }
}

/// Utility functions for authentication
pub mod utils {
    use super::*;

    /// Create a simplified auth bypass message
    pub fn create_bypass_auth() -> Message {
        Message::new(
            Command::AUTH,
            AuthType::Token as u32,
            0,
            Bytes::from("bypass"),
        )
    }

    /// Check if auth data looks like a valid token
    pub fn is_valid_token_format(data: &[u8]) -> bool {
        // Very basic validation - just check length
        data.len() >= 4 && data.len() <= 256
    }

    /// Check if auth data looks like a valid signature
    pub fn is_valid_signature_format(data: &[u8]) -> bool {
        // Basic signature validation - typically 256 bytes for RSA-2048
        data.len() >= 32 && data.len() <= 512
    }

    /// Generate a simple challenge for testing
    pub fn generate_test_challenge() -> Bytes {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Bytes::from(format!("test_challenge_{}", timestamp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_handler_disabled() {
        let handler = AuthHandler::new();
        assert!(!handler.is_auth_required());

        // Create dummy challenge
        let challenge = handler.create_auth_challenge();
        assert_eq!(challenge.command, Command::AUTH);
    }

    #[test]
    fn test_auth_handler_enabled() {
        let handler = AuthHandler::with_config(true, true, Duration::from_secs(30));
        assert!(handler.is_auth_required());

        let challenge = handler.create_auth_challenge();
        assert_eq!(challenge.command, Command::AUTH);
        assert_eq!(challenge.arg0, AuthType::Token as u32);
    }

    #[test]
    fn test_auth_response_processing() {
        let handler = AuthHandler::with_config(true, true, Duration::from_secs(30));

        // Test with valid token
        let auth_message = Message::new(
            Command::AUTH,
            AuthType::Token as u32,
            0,
            Bytes::from("valid_token"),
        );

        let result = handler.process_auth_response(&auth_message).unwrap();
        match result {
            AuthResult::Success { method, .. } => {
                assert_eq!(method, AuthType::Token);
            }
            _ => panic!("Expected success"),
        }
    }

    #[test]
    fn test_auth_manager_flow() {
        let handler = AuthHandler::with_config(true, true, Duration::from_secs(30));
        let mut manager = AuthManager::new(handler);

        // Start auth
        let challenge = manager.start_auth();
        assert!(challenge.is_some());
        assert_eq!(manager.state(), &AuthState::WaitingForResponse);

        // Process response
        let auth_response = Message::new(
            Command::AUTH,
            AuthType::Token as u32,
            0,
            Bytes::from("test_token"),
        );

        let response = manager.process_response(&auth_response).unwrap();
        assert!(response.is_some()); // Should get CNXN response
        assert!(manager.is_authenticated());
    }

    #[test]
    fn test_auth_types() {
        assert_eq!(AuthType::try_from(1).unwrap(), AuthType::Token);
        assert_eq!(AuthType::try_from(2).unwrap(), AuthType::Signature);
        assert_eq!(AuthType::try_from(3).unwrap(), AuthType::RsaPublicKey);
        assert!(AuthType::try_from(99).is_err());
    }

    #[test]
    fn test_auth_utils() {
        assert!(utils::is_valid_token_format(b"test"));
        assert!(!utils::is_valid_token_format(b""));

        let challenge = utils::generate_test_challenge();
        assert!(!challenge.is_empty());

        let bypass = utils::create_bypass_auth();
        assert_eq!(bypass.command, Command::AUTH);
    }

    #[test]
    fn test_auth_disabled_flow() {
        let handler = AuthHandler::new(); // Auth disabled by default
        let mut manager = AuthManager::new(handler);

        // Should start in NotRequired state
        assert_eq!(manager.state(), &AuthState::NotRequired);
        assert!(manager.is_authenticated());

        // Should not send challenge
        let challenge = manager.start_auth();
        assert!(challenge.is_none());
    }
}