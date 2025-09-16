use crate::host_services::{HostServiceRegistry, HostServiceResponse};
use crate::protocol::commands::AdbCommand;
use crate::protocol::error::{ProtocolError, ProtocolResult};
use crate::protocol::message::AdbMessage;
use crate::server::session::ClientSessionInfo;
use crate::server::state::ServerState;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Command dispatcher for routing ADB protocol messages
/// Handles incoming messages and routes them to appropriate handlers
#[derive(Debug)]
pub struct MessageDispatcher {
    /// Server state for accessing global information
    server_state: Arc<ServerState>,
}

impl MessageDispatcher {
    /// Create a new message dispatcher
    pub fn new(server_state: Arc<ServerState>) -> Self {
        Self { server_state }
    }

    /// Dispatch an incoming message to the appropriate handler
    pub async fn dispatch_message(
        &self,
        session: &ClientSessionInfo,
        message: AdbMessage,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        let command = match AdbCommand::from_u32(message.command) {
            Some(cmd) => cmd,
            None => {
                warn!("Unknown command received: 0x{:08X}", message.command);
                return Err(ProtocolError::UnknownCommand { command: message.command });
            }
        };

        debug!(
            "Dispatching message: session={}, command={:?}, arg0={}, arg1={}",
            session.session_id,
            command,
            message.arg0,
            message.arg1
        );

        // Update statistics first
        self.server_state.stats.message_processed(
            (24 + message.data.len()) as u64
        );

        let responses = match command {
            AdbCommand::CNXN => self.handle_connect(session, message).await?,
            AdbCommand::OPEN => self.handle_open(session, message).await?,
            AdbCommand::OKAY => self.handle_okay(session, message).await?,
            AdbCommand::WRTE => self.handle_write(session, message).await?,
            AdbCommand::CLSE => self.handle_close(session, message).await?,
            AdbCommand::PING => self.handle_ping(session, message).await?,
            AdbCommand::PONG => self.handle_pong(session, message).await?,
        };

        Ok(responses)
    }

    /// Handle CNXN (connection establishment) command
    async fn handle_connect(
        &self,
        session: &ClientSessionInfo,
        message: AdbMessage,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        info!("Handling CNXN for session {}", session.session_id);

        // Validate protocol version
        let protocol_version = message.arg0;
        let max_data_size = message.arg1;

        if protocol_version != 0x01000000 {
            error!(
                "Unsupported protocol version: 0x{:08X}",
                protocol_version
            );
            return Err(ProtocolError::UnsupportedVersion {
                version: protocol_version,
            });
        }

        // Parse client identifier
        let client_identifier = String::from_utf8(message.data.clone()).map_err(|_| {
            ProtocolError::InvalidMessage("Invalid UTF-8 in client identifier".to_string())
        })?;

        debug!(
            "Client connection: id={}, max_data_size={}",
            client_identifier, max_data_size
        );

        // Respond with server banner
        let server_banner = format!(
            "server:dbgif-server v{};",
            env!("CARGO_PKG_VERSION")
        );

        let response = AdbMessage::new_cnxn(
            protocol_version,
            self.server_state.config.connection_timeout.as_secs() as u32 * 1000, // timeout in ms
            server_banner.into_bytes(),
        );

        Ok(vec![response])
    }

    /// Handle OPEN (stream opening) command
    async fn handle_open(
        &self,
        session: &ClientSessionInfo,
        message: AdbMessage,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        let local_stream_id = message.arg0;
        let service_name = String::from_utf8(message.data.clone()).map_err(|_| {
            ProtocolError::InvalidMessage("Invalid UTF-8 in service name".to_string())
        })?;

        debug!(
            "Opening stream: session={}, local_id={}, service={}",
            session.session_id, local_stream_id, service_name
        );

        // Check if it's a host service
        if HostServiceRegistry::is_host_service(&service_name) {
            self.handle_host_service_open(session, local_stream_id, service_name)
                .await
        } else {
            // Device service - forward to selected device
            self.handle_device_service_open(session, local_stream_id, service_name)
                .await
        }
    }

    /// Handle host service OPEN request
    async fn handle_host_service_open(
        &self,
        session: &ClientSessionInfo,
        local_stream_id: u32,
        service_name: String,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        info!(
            "Handling host service: session={}, service={}",
            session.session_id, service_name
        );

        // Allocate remote stream ID
        let remote_stream_id = self.server_state.next_stream_id();

        // Extract service name and arguments
        let (service_base, service_args) = if let Some(pos) = service_name.find(':') {
            let mut parts = service_name[pos + 1..].splitn(2, ':');
            let base = parts.next().unwrap_or("");
            let args = parts.next().unwrap_or("");
            (format!("host:{}", base), args)
        } else {
            (service_name.clone(), "")
        };

        // Handle special case for host:device service
        let final_service_name = if service_base == "host:device" && !service_args.is_empty() {
            service_base
        } else {
            service_name.clone()
        };

        let host_services = self.server_state.host_services.read().unwrap();
        match host_services.handle_service(&final_service_name, session, service_args).await {
            Ok(HostServiceResponse::Okay(data)) => {
                let mut responses = Vec::new();

                // Send OKAY to establish stream
                responses.push(AdbMessage::new_okay(
                    remote_stream_id,
                    local_stream_id,
                ));

                // Send data if available
                if !data.is_empty() {
                    responses.push(AdbMessage::new_wrte(
                        remote_stream_id,
                        local_stream_id,
                        data,
                    ));
                }

                // Close the stream (host services are one-shot)
                responses.push(AdbMessage::new_clse(
                    remote_stream_id,
                    local_stream_id,
                ));

                Ok(responses)
            }
            Ok(HostServiceResponse::Close(_reason)) => {
                // Service failed - send CLSE
                Ok(vec![AdbMessage::new_clse(
                    remote_stream_id,
                    local_stream_id,
                )])
            }
            Err(e) => {
                error!("Host service error: {}", e);
                Ok(vec![AdbMessage::new_clse(
                    remote_stream_id,
                    local_stream_id,
                )])
            }
        }
    }

    /// Handle device service OPEN request (forward to device)
    async fn handle_device_service_open(
        &self,
        _session: &ClientSessionInfo,
        local_stream_id: u32,
        service_name: String,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        warn!(
            "Device service forwarding not implemented yet: service={}",
            service_name
        );

        // For now, just close the stream
        let remote_stream_id = self.server_state.next_stream_id();
        Ok(vec![AdbMessage::new_clse(
            remote_stream_id,
            local_stream_id,
        )])
    }

    /// Handle OKAY (acknowledgment) command
    async fn handle_okay(
        &self,
        session: &ClientSessionInfo,
        message: AdbMessage,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        debug!(
            "Handling OKAY: session={}, local_id={}, remote_id={}",
            session.session_id, message.arg0, message.arg1
        );

        // OKAY is typically a response, so we don't generate new responses
        // Just log for debugging
        Ok(Vec::new())
    }

    /// Handle WRTE (write data) command
    async fn handle_write(
        &self,
        session: &ClientSessionInfo,
        message: AdbMessage,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        debug!(
            "Handling WRTE: session={}, local_id={}, remote_id={}, data_len={}",
            session.session_id, message.arg0, message.arg1, message.data.len()
        );

        // For device streams, forward to device
        // For now, just log and don't forward
        warn!("WRTE forwarding not implemented yet");
        Ok(Vec::new())
    }

    /// Handle CLSE (close stream) command
    async fn handle_close(
        &self,
        session: &ClientSessionInfo,
        message: AdbMessage,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        debug!(
            "Handling CLSE: session={}, local_id={}, remote_id={}",
            session.session_id, message.arg0, message.arg1
        );

        // Clean up stream state
        // For now, just acknowledge the close
        Ok(vec![AdbMessage::new_clse(
            message.arg1, // swap stream IDs
            message.arg0,
        )])
    }

    /// Handle PING command
    async fn handle_ping(
        &self,
        session: &ClientSessionInfo,
        message: AdbMessage,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        debug!(
            "Handling PING: session={}, connect_id=0x{:08X}, token=0x{:08X}",
            session.session_id, message.arg0, message.arg1
        );

        // Respond with PONG
        let mut pong = AdbMessage::new_pong();
        pong.arg0 = message.arg0; // connect_id
        pong.arg1 = message.arg1; // token
        Ok(vec![pong])
    }

    /// Handle PONG command
    async fn handle_pong(
        &self,
        session: &ClientSessionInfo,
        message: AdbMessage,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        debug!(
            "Handling PONG: session={}, connect_id=0x{:08X}, token=0x{:08X}",
            session.session_id, message.arg0, message.arg1
        );

        // PONG is typically a response to PING, so no further action needed
        Ok(Vec::new())
    }

    /// Get dispatcher statistics
    pub fn stats(&self) -> DispatcherStats {
        let server_stats = self.server_state.stats.snapshot();
        DispatcherStats {
            messages_processed: server_stats.messages_processed,
            bytes_processed: server_stats.bytes_transferred,
            errors_count: server_stats.errors_count,
            active_sessions: server_stats.active_sessions,
            active_streams: server_stats.active_streams,
        }
    }
}

/// Dispatcher statistics
#[derive(Debug, Clone)]
pub struct DispatcherStats {
    pub messages_processed: u64,
    pub bytes_processed: u64,
    pub errors_count: u64,
    pub active_sessions: u64,
    pub active_streams: u64,
}

impl DispatcherStats {
    /// Calculate error rate as percentage
    pub fn error_rate(&self) -> f64 {
        if self.messages_processed == 0 {
            0.0
        } else {
            (self.errors_count as f64 / self.messages_processed as f64) * 100.0
        }
    }

    /// Calculate average message size in bytes
    pub fn avg_message_size(&self) -> f64 {
        if self.messages_processed == 0 {
            0.0
        } else {
            self.bytes_processed as f64 / self.messages_processed as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_services::list::HostListService;
    use crate::host_services::version::HostVersionService;
    use crate::host_services::features::HostFeaturesService;
    use crate::server::state::{ServerState, ServerConfig};
    use crate::server::session::{ClientInfo, SessionState, SessionStats, ClientCapabilities, ClientSessionInfo};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Instant;

    fn create_test_session() -> ClientSessionInfo {
        ClientSessionInfo {
            session_id: "test-session".to_string(),
            client_info: ClientInfo {
                connection_id: "tcp://127.0.0.1:12345->127.0.0.1:5555".to_string(),
                identity: Some("test-client".to_string()),
                protocol_version: 0x01000000,
                max_data_size: 1024 * 1024,
                capabilities: ClientCapabilities::default(),
            },
            state: SessionState::Active,
            streams: HashMap::new(),
            stats: SessionStats::default(),
            established_at: Instant::now(),
            last_activity: Instant::now(),
        }
    }

    fn create_test_dispatcher() -> MessageDispatcher {
        let config = ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap());
        let server_state = Arc::new(ServerState::new(config));

        // Register host services
        {
            let mut host_services = server_state.host_services.write().unwrap();
            let device_registry = Arc::clone(&server_state.device_registry);
            host_services.register(HostListService::new(device_registry));
            host_services.register(HostVersionService::new());
            host_services.register(HostFeaturesService::new());
        }

        MessageDispatcher::new(server_state)
    }

    #[tokio::test]
    async fn test_dispatcher_creation() {
        let dispatcher = create_test_dispatcher();
        let stats = dispatcher.stats();
        assert_eq!(stats.messages_processed, 0);
        assert_eq!(stats.active_sessions, 0);
    }

    #[tokio::test]
    async fn test_cnxn_message_handling() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let cnxn_message = AdbMessage::new_cnxn(
            0x01000000,
            1024 * 1024,
            b"client:test:test-client v1.0;".to_vec(),
        );

        let responses = dispatcher.dispatch_message(&session, cnxn_message).await.unwrap();
        assert_eq!(responses.len(), 1);

        let response = &responses[0];
        assert_eq!(response.command, AdbCommand::CNXN as u32);
        assert_eq!(response.arg0, 0x01000000);
    }

    #[tokio::test]
    async fn test_invalid_protocol_version() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let cnxn_message = AdbMessage::new_cnxn(
            0x02000000, // Invalid version
            1024 * 1024,
            b"client:test:test-client v1.0;".to_vec(),
        );

        let result = dispatcher.dispatch_message(&session, cnxn_message).await;
        assert!(result.is_err());

        if let Err(ProtocolError::UnsupportedVersion { version }) = result {
            assert_eq!(version, 0x02000000);
        } else {
            panic!("Expected UnsupportedVersion error");
        }
    }

    #[tokio::test]
    async fn test_host_version_service() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let open_message = AdbMessage::new_open(
            1, // local stream ID
            b"host:version".to_vec(),
        );

        let responses = dispatcher.dispatch_message(&session, open_message).await.unwrap();

        // Should get OKAY, WRTE, and CLSE
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0].command, AdbCommand::OKAY as u32);
        assert_eq!(responses[1].command, AdbCommand::WRTE as u32);
        assert_eq!(responses[2].command, AdbCommand::CLSE as u32);
    }

    #[tokio::test]
    async fn test_host_list_service() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let open_message = AdbMessage::new_open(
            2, // local stream ID
            b"host:list".to_vec(),
        );

        let responses = dispatcher.dispatch_message(&session, open_message).await.unwrap();

        // Should get OKAY, WRTE (possibly empty), and CLSE
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0].command, AdbCommand::OKAY as u32);
        assert_eq!(responses[1].command, AdbCommand::WRTE as u32);
        assert_eq!(responses[2].command, AdbCommand::CLSE as u32);
    }

    #[tokio::test]
    async fn test_ping_pong() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let mut ping_message = AdbMessage::new_ping();
        ping_message.arg0 = 0x12345678; // connect_id
        ping_message.arg1 = 0xABCDEF01; // token

        let responses = dispatcher.dispatch_message(&session, ping_message).await.unwrap();
        assert_eq!(responses.len(), 1);

        let pong = &responses[0];
        assert_eq!(pong.command, AdbCommand::PONG as u32);
        assert_eq!(pong.arg0, 0x12345678);
        assert_eq!(pong.arg1, 0xABCDEF01);
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let mut unknown_message = AdbMessage::new_ping();
        unknown_message.command = 0xDEADBEEF; // Unknown command
        unknown_message.magic = !0xDEADBEEF;

        let result = dispatcher.dispatch_message(&session, unknown_message).await;
        assert!(result.is_err());

        if let Err(ProtocolError::UnknownCommand { command }) = result {
            assert_eq!(command, 0xDEADBEEF);
        } else {
            panic!("Expected UnknownCommand error");
        }
    }

    #[tokio::test]
    async fn test_device_service_not_implemented() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let open_message = AdbMessage::new_open(
            3,
            b"shell:".to_vec(),
        );

        let responses = dispatcher.dispatch_message(&session, open_message).await.unwrap();

        // Should get CLSE with not implemented message
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].command, AdbCommand::CLSE as u32);

        let error_msg = String::from_utf8(responses[0].data.clone()).unwrap();
        assert!(error_msg.contains("not implemented"));
    }

    #[tokio::test]
    async fn test_close_stream() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let close_message = AdbMessage::new_clse(
            1, // local stream ID
            100, // remote stream ID
        );

        let responses = dispatcher.dispatch_message(&session, close_message).await.unwrap();
        assert_eq!(responses.len(), 1);

        let response = &responses[0];
        assert_eq!(response.command, AdbCommand::CLSE as u32);
        assert_eq!(response.arg0, 100); // swapped
        assert_eq!(response.arg1, 1);   // swapped
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let dispatcher = create_test_dispatcher();
        let session = create_test_session();

        let initial_stats = dispatcher.stats();
        assert_eq!(initial_stats.messages_processed, 0);

        // Process a message
        let ping_message = AdbMessage::new_ping();

        dispatcher.dispatch_message(&session, ping_message).await.unwrap();

        let updated_stats = dispatcher.stats();
        assert_eq!(updated_stats.messages_processed, 1);
        assert!(updated_stats.bytes_processed >= 24);
    }
}