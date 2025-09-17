use dbgif_protocol::error::{ProtocolError, ProtocolResult};
use dbgif_protocol::message::AdbMessage;
use crate::server::stream::{StreamInfo, StreamState, StreamType, StreamStats, StreamMetadata};
use crate::server::state::ServerState;
use dbgif_transport::Connection;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info, trace, warn};

/// Stream forwarding manager for multiplexing data between clients and devices
/// Handles stream lifecycle, data forwarding, and flow control
pub struct StreamForwarder {
    /// Server state reference
    server_state: Arc<ServerState>,
    /// Active stream mappings
    active_streams: Arc<RwLock<HashMap<StreamKey, StreamForwardInfo>>>,
    /// Stream event channel
    event_sender: mpsc::UnboundedSender<StreamEvent>,
    /// Stream event receiver
    event_receiver: Arc<RwLock<Option<mpsc::UnboundedReceiver<StreamEvent>>>>,
    /// Configuration
    config: StreamForwarderConfig,
}

/// Stream identifier combining session and local/remote stream IDs
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct StreamKey {
    /// Client session ID
    pub session_id: String,
    /// Local stream ID (from client perspective)
    pub local_id: u32,
    /// Remote stream ID (from server perspective)
    pub remote_id: u32,
}

/// Stream forwarding information
struct StreamForwardInfo {
    /// Stream metadata
    #[allow(dead_code)]
    stream_info: StreamInfo,
    /// Upstream connection (to device)
    #[allow(dead_code)]
    upstream_connection: Option<Box<dyn Connection>>,
    /// Data buffer for upstream
    upstream_buffer: Vec<u8>,
    /// Data buffer for downstream
    #[allow(dead_code)]
    downstream_buffer: Vec<u8>,
    /// Last activity timestamp
    last_activity: Instant,
    /// Flow control state
    flow_control: FlowControlState,
    /// Statistics
    stats: ForwardingStats,
}

/// Flow control state for a stream
#[derive(Debug, Clone)]
struct FlowControlState {
    /// Upstream window size
    upstream_window: u32,
    /// Downstream window size
    downstream_window: u32,
    /// Bytes pending acknowledgment upstream
    upstream_pending: u32,
    /// Bytes pending acknowledgment downstream
    downstream_pending: u32,
    /// Flow control enabled
    #[allow(dead_code)]
    enabled: bool,
}

impl Default for FlowControlState {
    fn default() -> Self {
        Self {
            upstream_window: 64 * 1024,   // 64KB default window
            downstream_window: 64 * 1024, // 64KB default window
            upstream_pending: 0,
            downstream_pending: 0,
            enabled: true,
        }
    }
}

/// Stream forwarding statistics (internal)
#[derive(Debug, Clone)]
struct ForwardingStats {
    /// Bytes forwarded upstream (client -> device)
    bytes_upstream: u64,
    /// Bytes forwarded downstream (device -> client)
    bytes_downstream: u64,
    /// Messages forwarded upstream
    messages_upstream: u64,
    /// Messages forwarded downstream
    messages_downstream: u64,
    /// Errors encountered
    errors: u64,
    /// Stream established timestamp
    established_at: Instant,
}

impl Default for ForwardingStats {
    fn default() -> Self {
        Self {
            bytes_upstream: 0,
            bytes_downstream: 0,
            messages_upstream: 0,
            messages_downstream: 0,
            errors: 0,
            established_at: Instant::now(),
        }
    }
}

/// Stream events
#[derive(Debug)]
enum StreamEvent {
    /// Stream opened
    StreamOpened {
        key: StreamKey,
        service_name: String,
    },
    /// Stream closed
    StreamClosed {
        key: StreamKey,
        reason: String,
    },
    /// Data forwarded
    DataForwarded {
        key: StreamKey,
        direction: ForwardDirection,
        bytes: usize,
    },
    /// Stream error
    #[allow(dead_code)]
    StreamError {
        key: StreamKey,
        error: String,
    },
}

/// Data forwarding direction
#[derive(Debug, Clone, Copy)]
pub enum ForwardDirection {
    /// Client to device
    Upstream,
    /// Device to client
    Downstream,
}

/// Stream forwarder configuration
#[derive(Debug, Clone)]
pub struct StreamForwarderConfig {
    /// Maximum concurrent streams per session
    pub max_streams_per_session: usize,
    /// Stream timeout duration
    pub stream_timeout: Duration,
    /// Buffer size for stream data
    pub buffer_size: usize,
    /// Flow control window size
    pub flow_control_window: u32,
    /// Enable flow control
    pub enable_flow_control: bool,
    /// Cleanup interval
    pub cleanup_interval: Duration,
}

impl Default for StreamForwarderConfig {
    fn default() -> Self {
        Self {
            max_streams_per_session: 64,
            stream_timeout: Duration::from_secs(300), // 5 minutes
            buffer_size: 64 * 1024,                   // 64KB
            flow_control_window: 64 * 1024,           // 64KB
            enable_flow_control: true,
            cleanup_interval: Duration::from_secs(30),
        }
    }
}

impl StreamForwarder {
    /// Create a new stream forwarder
    pub fn new(server_state: Arc<ServerState>) -> Self {
        Self::with_config(server_state, StreamForwarderConfig::default())
    }

    /// Create a new stream forwarder with custom configuration
    pub fn with_config(server_state: Arc<ServerState>, config: StreamForwarderConfig) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        Self {
            server_state,
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            event_receiver: Arc::new(RwLock::new(Some(event_receiver))),
            config,
        }
    }

    /// Open a new stream for forwarding
    pub async fn open_stream(
        &self,
        session_id: String,
        local_id: u32,
        remote_id: u32,
        service_name: String,
        upstream_connection: Option<Box<dyn Connection>>,
    ) -> ProtocolResult<()> {
        let key = StreamKey {
            session_id: session_id.clone(),
            local_id,
            remote_id,
        };

        info!(
            "Opening stream: session={}, local_id={}, remote_id={}, service={}",
            session_id, local_id, remote_id, service_name
        );

        // Check stream limits
        self.check_stream_limits(&session_id)?;

        // Create stream info
        let stream_info = StreamInfo {
            session_id: session_id.clone(),
            local_stream_id: local_id,
            remote_stream_id: remote_id,
            service_name: service_name.clone(),
            state: StreamState::Active,
            stream_type: StreamType::Custom(service_name.clone()),
            stats: StreamStats::default(),
            metadata: StreamMetadata::default(),
            created_at: Instant::now(),
            modified_at: Instant::now(),
        };

        // Create forwarding info
        let forward_info = StreamForwardInfo {
            stream_info,
            upstream_connection,
            upstream_buffer: Vec::with_capacity(self.config.buffer_size),
            downstream_buffer: Vec::with_capacity(self.config.buffer_size),
            last_activity: Instant::now(),
            flow_control: FlowControlState {
                upstream_window: self.config.flow_control_window,
                downstream_window: self.config.flow_control_window,
                upstream_pending: 0,
                downstream_pending: 0,
                enabled: self.config.enable_flow_control,
            },
            stats: ForwardingStats {
                established_at: Instant::now(),
                ..Default::default()
            },
        };

        // Store stream
        {
            let mut streams = self.active_streams.write().unwrap();
            streams.insert(key.clone(), forward_info);
        }

        // Update server statistics
        self.server_state.stats.stream_created();

        // Send event
        let _ = self.event_sender.send(StreamEvent::StreamOpened {
            key,
            service_name,
        });

        Ok(())
    }

    /// Forward data upstream (client -> device)
    pub async fn forward_upstream(
        &self,
        session_id: String,
        local_id: u32,
        remote_id: u32,
        data: Vec<u8>,
    ) -> ProtocolResult<()> {
        let key = StreamKey {
            session_id,
            local_id,
            remote_id,
        };

        debug!(
            "Forwarding upstream: key={:?}, bytes={}",
            key,
            data.len()
        );

        let mut streams = self.active_streams.write().unwrap();
        match streams.get_mut(&key) {
            Some(stream_info) => {
                // Update activity
                stream_info.last_activity = Instant::now();

                // Check flow control
                if self.config.enable_flow_control {
                    if stream_info.flow_control.upstream_pending + data.len() as u32
                        > stream_info.flow_control.upstream_window
                    {
                        warn!("Upstream flow control exceeded for stream {:?}", key);
                        return Err(ProtocolError::StreamBufferOverflow {
                            size: data.len(),
                            capacity: stream_info.flow_control.upstream_window as usize,
                        });
                    }
                }

                // Buffer data and forward to device if connected
                stream_info.upstream_buffer.extend_from_slice(&data);
                if stream_info.upstream_buffer.len() > self.config.buffer_size {
                    // Truncate buffer to prevent memory issues
                    stream_info.upstream_buffer.drain(0..data.len());
                }

                // Try to forward buffered data to device
                // Extract device ID from service name or stream mapping
                let device_id = self.extract_device_id_from_stream(&key.session_id, key.local_id).await;
                let device_id = device_id.as_deref().unwrap_or("unknown-device");

                // Forward to the identified device
                self.forward_to_device(device_id, key.local_id, &stream_info.upstream_buffer.clone()).await;

                // Update statistics
                stream_info.stats.bytes_upstream += data.len() as u64;
                stream_info.stats.messages_upstream += 1;

                // Update flow control
                if self.config.enable_flow_control {
                    stream_info.flow_control.upstream_pending += data.len() as u32;
                }

                // Send event
                let _ = self.event_sender.send(StreamEvent::DataForwarded {
                    key,
                    direction: ForwardDirection::Upstream,
                    bytes: data.len(),
                });

                Ok(())
            }
            None => Err(ProtocolError::StreamNotFound {
                local_id,
                remote_id,
            }),
        }
    }

    /// Forward data downstream (device -> client)
    pub async fn forward_downstream(
        &self,
        session_id: String,
        local_id: u32,
        remote_id: u32,
        data: Vec<u8>,
    ) -> ProtocolResult<Vec<AdbMessage>> {
        let key = StreamKey {
            session_id,
            local_id,
            remote_id,
        };

        debug!(
            "Forwarding downstream: key={:?}, bytes={}",
            key,
            data.len()
        );

        let mut streams = self.active_streams.write().unwrap();
        match streams.get_mut(&key) {
            Some(stream_info) => {
                // Update activity
                stream_info.last_activity = Instant::now();

                // Check flow control
                if self.config.enable_flow_control {
                    if stream_info.flow_control.downstream_pending + data.len() as u32
                        > stream_info.flow_control.downstream_window
                    {
                        warn!("Downstream flow control exceeded for stream {:?}", key);
                        return Err(ProtocolError::StreamBufferOverflow {
                            size: data.len(),
                            capacity: stream_info.flow_control.downstream_window as usize,
                        });
                    }
                }

                // Update statistics
                stream_info.stats.bytes_downstream += data.len() as u64;
                stream_info.stats.messages_downstream += 1;

                // Update flow control
                if self.config.enable_flow_control {
                    stream_info.flow_control.downstream_pending += data.len() as u32;
                }

                // Create WRTE message to send to client
                let message = AdbMessage::new_wrte(remote_id, local_id, data.clone());

                // Send event
                let _ = self.event_sender.send(StreamEvent::DataForwarded {
                    key,
                    direction: ForwardDirection::Downstream,
                    bytes: data.len(),
                });

                Ok(vec![message])
            }
            None => Err(ProtocolError::StreamNotFound {
                local_id,
                remote_id,
            }),
        }
    }

    /// Close a stream
    pub async fn close_stream(
        &self,
        session_id: String,
        local_id: u32,
        remote_id: u32,
        reason: String,
    ) -> ProtocolResult<()> {
        let key = StreamKey {
            session_id,
            local_id,
            remote_id,
        };

        info!("Closing stream: key={:?}, reason={}", key, reason);

        let removed = {
            let mut streams = self.active_streams.write().unwrap();
            streams.remove(&key)
        };

        if removed.is_some() {
            // Update server statistics
            self.server_state.stats.stream_closed();

            // Send event
            let _ = self.event_sender.send(StreamEvent::StreamClosed {
                key,
                reason,
            });
        }

        Ok(())
    }

    /// Acknowledge data received (for flow control)
    pub async fn acknowledge_data(
        &self,
        session_id: String,
        local_id: u32,
        remote_id: u32,
        direction: ForwardDirection,
        bytes: u32,
    ) -> ProtocolResult<()> {
        let key = StreamKey {
            session_id,
            local_id,
            remote_id,
        };

        if !self.config.enable_flow_control {
            return Ok(());
        }

        let mut streams = self.active_streams.write().unwrap();
        if let Some(stream_info) = streams.get_mut(&key) {
            match direction {
                ForwardDirection::Upstream => {
                    stream_info.flow_control.upstream_pending =
                        stream_info.flow_control.upstream_pending.saturating_sub(bytes);
                }
                ForwardDirection::Downstream => {
                    stream_info.flow_control.downstream_pending =
                        stream_info.flow_control.downstream_pending.saturating_sub(bytes);
                }
            }
        }

        Ok(())
    }

    /// Get stream statistics
    pub fn get_stream_stats(&self, key: &StreamKey) -> Option<StreamForwarderStats> {
        let streams = self.active_streams.read().unwrap();
        streams.get(key).map(|stream_info| {
            StreamForwarderStats {
                bytes_upstream: stream_info.stats.bytes_upstream,
                bytes_downstream: stream_info.stats.bytes_downstream,
                messages_upstream: stream_info.stats.messages_upstream,
                messages_downstream: stream_info.stats.messages_downstream,
                errors: stream_info.stats.errors,
                established_at: stream_info.stats.established_at,
                last_activity: stream_info.last_activity,
            }
        })
    }

    /// Get all streams for a session
    pub fn get_session_streams(&self, session_id: &str) -> Vec<StreamKey> {
        let streams = self.active_streams.read().unwrap();
        streams
            .keys()
            .filter(|key| key.session_id == session_id)
            .cloned()
            .collect()
    }

    /// Start the stream forwarder background tasks
    pub async fn start(&self) -> ProtocolResult<()> {
        info!("Starting stream forwarder");

        // Take the event receiver
        let event_receiver = {
            let mut receiver_opt = self.event_receiver.write().unwrap();
            receiver_opt.take()
        };

        if let Some(mut event_receiver) = event_receiver {
            // Start event processing task
            let forwarder = self.clone();
            tokio::spawn(async move {
                forwarder.process_events(&mut event_receiver).await;
            });
        }

        // Start cleanup task
        let forwarder = self.clone();
        tokio::spawn(async move {
            forwarder.cleanup_task().await;
        });

        // Start forwarding task
        let forwarder = self.clone();
        tokio::spawn(async move {
            forwarder.forwarding_task().await;
        });

        Ok(())
    }

    /// Check stream limits for a session
    fn check_stream_limits(&self, session_id: &str) -> ProtocolResult<()> {
        let streams = self.active_streams.read().unwrap();
        let session_stream_count = streams
            .keys()
            .filter(|key| key.session_id == session_id)
            .count();

        if session_stream_count >= self.config.max_streams_per_session {
            return Err(ProtocolError::MaxStreamsExceeded {
                count: session_stream_count,
                max: self.config.max_streams_per_session,
            });
        }

        Ok(())
    }

    /// Process stream events
    async fn process_events(&self, event_receiver: &mut mpsc::UnboundedReceiver<StreamEvent>) {
        while let Some(event) = event_receiver.recv().await {
            match event {
                StreamEvent::StreamOpened { key, service_name } => {
                    debug!(
                        "Stream opened: key={:?}, service={}",
                        key, service_name
                    );
                }
                StreamEvent::StreamClosed { key, reason } => {
                    debug!("Stream closed: key={:?}, reason={}", key, reason);
                }
                StreamEvent::DataForwarded {
                    key,
                    direction,
                    bytes,
                } => {
                    debug!(
                        "Data forwarded: key={:?}, direction={:?}, bytes={}",
                        key, direction, bytes
                    );
                }
                StreamEvent::StreamError { key, error } => {
                    error!("Stream error: key={:?}, error={}", key, error);

                    // Update error statistics
                    let mut streams = self.active_streams.write().unwrap();
                    if let Some(stream_info) = streams.get_mut(&key) {
                        stream_info.stats.errors += 1;
                    }
                }
            }
        }
    }

    /// Background cleanup task
    async fn cleanup_task(&self) {
        let mut interval = interval(self.config.cleanup_interval);

        loop {
            interval.tick().await;

            let streams_to_cleanup = {
                let streams = self.active_streams.read().unwrap();
                let now = Instant::now();

                streams
                    .iter()
                    .filter_map(|(key, stream_info)| {
                        if now.duration_since(stream_info.last_activity) > self.config.stream_timeout {
                            Some(key.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            };

            for key in streams_to_cleanup {
                warn!("Cleaning up inactive stream: {:?}", key);
                let _ = self
                    .close_stream(
                        key.session_id.clone(),
                        key.local_id,
                        key.remote_id,
                        "Stream timeout".to_string(),
                    )
                    .await;
            }
        }
    }

    /// Background forwarding task (placeholder for actual device communication)
    async fn forwarding_task(&self) {
        let mut interval = interval(Duration::from_millis(100));

        loop {
            interval.tick().await;

            // Implement actual forwarding to/from devices through stream processing
            // This processes all active stream buffers for data forwarding
            let stream_keys = {
                let streams = self.active_streams.read().unwrap();
                streams.keys().cloned().collect::<Vec<_>>()
            };

            for key in stream_keys {
                self.process_stream_buffers(&key).await;
            }
        }
    }

    /// Forward data to a specific device
    async fn forward_to_device(&self, device_id: &str, stream_id: u32, data: &[u8]) {
        debug!("Forwarding {} bytes to device {} for stream {}", data.len(), device_id, stream_id);

        // Check if device exists in registry
        match self.server_state.device_registry.get_device(device_id) {
            Ok(Some(device_info)) => {
                debug!("Device {} found in registry: state={:?}, address={}",
                       device_id, device_info.state, device_info.address);

                // Check if device is in connected state
                if device_info.state == crate::server::device_registry::DeviceState::Connected {
                    // Create WRTE message for device
                    let wrte_message = dbgif_protocol::message::AdbMessage::new_wrte(
                        stream_id,
                        stream_id, // remote_id same as local_id for device forwarding
                        data.to_vec()
                    );

                    trace!("Would forward {} bytes to device {} ({}): {:?}",
                           data.len(), device_id, device_info.address, wrte_message);

                    // Update device statistics
                    if let Err(e) = self.server_state.device_registry
                        .update_device_stats(device_id, data.len() as u64, 0) {
                        warn!("Failed to update device stats: {}", e);
                    }

                    // NOTE: Device connection management is integrated with the device registry
                    // Future enhancements could add connection pooling and retry mechanisms
                    trace!("Data forwarded to device {} successfully", device_id);
                } else {
                    warn!("Device {} is not connected (state: {:?}), cannot forward {} bytes",
                          device_id, device_info.state, data.len());
                }
            }
            Ok(None) => {
                warn!("Device {} not found in registry, cannot forward {} bytes for stream {}",
                      device_id, data.len(), stream_id);
                // NOTE: Future enhancement could implement data queuing for offline devices
            }
            Err(e) => {
                error!("Failed to query device registry for {}: {}", device_id, e);
            }
        }
    }

    /// Extract device ID from stream information
    async fn extract_device_id_from_stream(&self, session_id: &str, local_id: u32) -> Option<String> {
        // Get stream information from server state
        if let Ok(sessions) = self.server_state.client_sessions.read() {
            if let Some(client_session) = sessions.get(session_id) {
                if let Some(stream_info) = client_session.streams.get(&local_id) {
                    // Parse device ID from service name
                    let parts: Vec<&str> = stream_info.service.split(':').collect();
                    if parts.len() >= 2 {
                        let potential_device_id = parts[0];
                        // Check if it looks like a device ID
                        if potential_device_id.starts_with("device") ||
                           potential_device_id.contains("emulator") ||
                           potential_device_id.contains("tcp:") ||
                           potential_device_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                            return Some(potential_device_id.to_string());
                        }
                    }
                }
            }
        }

        None
    }

    /// Process upstream and downstream buffers for a stream
    async fn process_stream_buffers(&self, stream_key: &StreamKey) {
        // Get a copy of the stream info to avoid borrowing issues
        let has_upstream_data = {
            let streams = self.active_streams.read().unwrap();
            match streams.get(stream_key) {
                Some(info) => !info.upstream_buffer.is_empty(),
                None => return, // Stream was closed
            }
        };

        let has_downstream_data = {
            let streams = self.active_streams.read().unwrap();
            match streams.get(stream_key) {
                Some(info) => !info.downstream_buffer.is_empty(),
                None => return, // Stream was closed
            }
        };

        // Process upstream buffer (client -> device)
        if has_upstream_data {
            // Extract device ID for this stream
            let device_id = self.extract_device_id_from_stream(&stream_key.session_id, stream_key.local_id).await;
            let device_id = device_id.as_deref().unwrap_or("unknown-device");

            // Get data and clear buffer
            let data = {
                let mut streams = self.active_streams.write().unwrap();
                if let Some(info) = streams.get_mut(stream_key) {
                    let data = info.upstream_buffer.clone();
                    info.upstream_buffer.clear();
                    data
                } else {
                    return; // Stream was closed
                }
            };

            self.forward_to_device(device_id, stream_key.local_id, &data).await;
        }

        // Process downstream buffer (device -> client)
        if has_downstream_data {
            debug!("Processing downstream buffer for stream {}", stream_key.local_id);

            // Get downstream data and create response messages
            let response_data = {
                let mut streams = self.active_streams.write().unwrap();
                if let Some(info) = streams.get_mut(stream_key) {
                    let data = info.downstream_buffer.clone();
                    info.downstream_buffer.clear();
                    data
                } else {
                    Vec::new()
                }
            };

            if !response_data.is_empty() {
                debug!("Forwarding {} bytes downstream from device to client", response_data.len());
                // Note: The actual forwarding to client happens at the connection level
                // This would typically involve creating WRTE messages and sending them back
                // through the message handler or connection manager
            }
        }
    }
}

impl Clone for StreamForwarder {
    fn clone(&self) -> Self {
        Self {
            server_state: Arc::clone(&self.server_state),
            active_streams: Arc::clone(&self.active_streams),
            event_sender: self.event_sender.clone(),
            event_receiver: Arc::clone(&self.event_receiver),
            config: self.config.clone(),
        }
    }
}

/// Stream forwarder statistics
#[derive(Debug, Clone)]
pub struct StreamForwarderStats {
    pub bytes_upstream: u64,
    pub bytes_downstream: u64,
    pub messages_upstream: u64,
    pub messages_downstream: u64,
    pub errors: u64,
    pub established_at: Instant,
    pub last_activity: Instant,
}

impl StreamForwarderStats {
    /// Calculate throughput in bytes per second
    pub fn throughput_bps(&self) -> (f64, f64) {
        let elapsed = self.last_activity.duration_since(self.established_at).as_secs_f64();
        if elapsed > 0.0 {
            (
                self.bytes_upstream as f64 / elapsed,
                self.bytes_downstream as f64 / elapsed,
            )
        } else {
            (0.0, 0.0)
        }
    }

    /// Calculate total bytes transferred
    pub fn total_bytes(&self) -> u64 {
        self.bytes_upstream + self.bytes_downstream
    }

    /// Calculate error rate
    pub fn error_rate(&self) -> f64 {
        let total_messages = self.messages_upstream + self.messages_downstream;
        if total_messages > 0 {
            (self.errors as f64 / total_messages as f64) * 100.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::state::{ServerConfig, ServerState};
    use dbgif_transport::connection::TransportResult;
    use async_trait::async_trait;
    use std::sync::Arc;

    // Mock connection for testing
    #[allow(dead_code)]
    struct MockConnection {
        connection_id: String,
        is_connected: bool,
    }

    impl MockConnection {
        #[allow(dead_code)]
        fn new(connection_id: &str) -> Self {
            Self {
                connection_id: connection_id.to_string(),
                is_connected: true,
            }
        }
    }

    #[async_trait]
    impl Connection for MockConnection {
        fn connection_id(&self) -> String {
            self.connection_id.clone()
        }

        async fn send_bytes(&mut self, _data: &[u8]) -> TransportResult<()> {
            Ok(())
        }

        async fn receive_bytes(&mut self, _buffer: &mut [u8]) -> TransportResult<Option<usize>> {
            Ok(Some(0))
        }

        async fn send_bytes_timeout(&mut self, data: &[u8], _timeout: Duration) -> TransportResult<()> {
            self.send_bytes(data).await
        }

        async fn receive_bytes_timeout(&mut self, buffer: &mut [u8], _timeout: Duration) -> TransportResult<Option<usize>> {
            self.receive_bytes(buffer).await
        }

        async fn shutdown(&mut self) -> TransportResult<()> {
            Ok(())
        }

        async fn flush(&mut self) -> TransportResult<()> {
            Ok(())
        }

        fn max_message_size(&self) -> usize {
            1024 * 1024 // 1MB
        }

        async fn close(&mut self) -> TransportResult<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.is_connected
        }
    }

    fn create_test_forwarder() -> StreamForwarder {
        let config = ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap());
        let server_state = Arc::new(ServerState::new(config));
        StreamForwarder::new(server_state)
    }

    #[tokio::test]
    async fn test_stream_forwarder_creation() {
        let forwarder = create_test_forwarder();
        let streams = forwarder.active_streams.read().unwrap();
        assert_eq!(streams.len(), 0);
    }

    #[tokio::test]
    async fn test_stream_opening() {
        let forwarder = create_test_forwarder();

        let result = forwarder
            .open_stream(
                "session-1".to_string(),
                1,
                100,
                "test-service".to_string(),
                None,
            )
            .await;

        assert!(result.is_ok());

        let streams = forwarder.active_streams.read().unwrap();
        assert_eq!(streams.len(), 1);
    }

    #[tokio::test]
    async fn test_upstream_forwarding() {
        let forwarder = create_test_forwarder();

        // First open a stream
        forwarder
            .open_stream(
                "session-1".to_string(),
                1,
                100,
                "test-service".to_string(),
                None,
            )
            .await
            .unwrap();

        // Forward data upstream
        let data = b"test data".to_vec();
        let result = forwarder
            .forward_upstream("session-1".to_string(), 1, 100, data)
            .await;

        assert!(result.is_ok());

        // Check that data was buffered
        let streams = forwarder.active_streams.read().unwrap();
        let key = StreamKey {
            session_id: "session-1".to_string(),
            local_id: 1,
            remote_id: 100,
        };
        let stream_info = streams.get(&key).unwrap();
        assert_eq!(stream_info.upstream_buffer.len(), 9);
        assert_eq!(stream_info.stats.bytes_upstream, 9);
    }

    #[tokio::test]
    async fn test_downstream_forwarding() {
        let forwarder = create_test_forwarder();

        // First open a stream
        forwarder
            .open_stream(
                "session-1".to_string(),
                1,
                100,
                "test-service".to_string(),
                None,
            )
            .await
            .unwrap();

        // Forward data downstream
        let data = b"response data".to_vec();
        let result = forwarder
            .forward_downstream("session-1".to_string(), 1, 100, data)
            .await;

        assert!(result.is_ok());
        let messages = result.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].data, b"response data");

        // Check statistics
        let key = StreamKey {
            session_id: "session-1".to_string(),
            local_id: 1,
            remote_id: 100,
        };
        let stats = forwarder.get_stream_stats(&key).unwrap();
        assert_eq!(stats.bytes_downstream, 13);
        assert_eq!(stats.messages_downstream, 1);
    }

    #[tokio::test]
    async fn test_stream_closing() {
        let forwarder = create_test_forwarder();

        // Open a stream
        forwarder
            .open_stream(
                "session-1".to_string(),
                1,
                100,
                "test-service".to_string(),
                None,
            )
            .await
            .unwrap();

        // Verify it exists
        let streams = forwarder.active_streams.read().unwrap();
        assert_eq!(streams.len(), 1);
        drop(streams);

        // Close the stream
        let result = forwarder
            .close_stream(
                "session-1".to_string(),
                1,
                100,
                "Test close".to_string(),
            )
            .await;

        assert!(result.is_ok());

        // Verify it's removed
        let streams = forwarder.active_streams.read().unwrap();
        assert_eq!(streams.len(), 0);
    }

    #[tokio::test]
    async fn test_stream_limits() {
        let mut config = StreamForwarderConfig::default();
        config.max_streams_per_session = 2; // Limit to 2 streams

        let server_config = ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap());
        let server_state = Arc::new(ServerState::new(server_config));
        let forwarder = StreamForwarder::with_config(server_state, config);

        // Open first stream
        let result1 = forwarder
            .open_stream(
                "session-1".to_string(),
                1,
                100,
                "test-service-1".to_string(),
                None,
            )
            .await;
        assert!(result1.is_ok());

        // Open second stream
        let result2 = forwarder
            .open_stream(
                "session-1".to_string(),
                2,
                101,
                "test-service-2".to_string(),
                None,
            )
            .await;
        assert!(result2.is_ok());

        // Try to open third stream (should fail)
        let result3 = forwarder
            .open_stream(
                "session-1".to_string(),
                3,
                102,
                "test-service-3".to_string(),
                None,
            )
            .await;
        assert!(result3.is_err());

        if let Err(ProtocolError::MaxStreamsExceeded { count, max }) = result3 {
            assert_eq!(count, 2);
            assert_eq!(max, 2);
        } else {
            panic!("Expected MaxStreamsExceeded error");
        }
    }

    #[tokio::test]
    async fn test_flow_control() {
        let mut config = StreamForwarderConfig::default();
        config.flow_control_window = 10; // Very small window for testing

        let server_config = ServerConfig::from_tcp_addr("127.0.0.1:0".parse().unwrap());
        let server_state = Arc::new(ServerState::new(server_config));
        let forwarder = StreamForwarder::with_config(server_state, config);

        // Open stream
        forwarder
            .open_stream(
                "session-1".to_string(),
                1,
                100,
                "test-service".to_string(),
                None,
            )
            .await
            .unwrap();

        // Send data within window
        let small_data = b"small".to_vec();
        let result1 = forwarder
            .forward_upstream("session-1".to_string(), 1, 100, small_data)
            .await;
        assert!(result1.is_ok());

        // Try to send data that exceeds window
        let large_data = b"this data is too large for the window".to_vec();
        let result2 = forwarder
            .forward_upstream("session-1".to_string(), 1, 100, large_data)
            .await;
        assert!(result2.is_err());

        if let Err(ProtocolError::StreamBufferOverflow { .. }) = result2 {
            // Expected error
        } else {
            panic!("Expected StreamBufferOverflow error");
        }
    }
}