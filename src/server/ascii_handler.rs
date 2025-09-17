use crate::protocol::ascii;
use crate::server::state::ServerState;
use crate::server::stream_forwarder::StreamForwarder;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Stream data buffer for a single stream
#[derive(Clone)]
struct StreamBuffer {
    /// Buffered data waiting to be sent to device
    pending_data: Vec<u8>,
    /// Service name for this stream
    service_name: String,
    /// Whether the stream is active
    active: bool,
}

/// ASCII protocol handler for client connections
pub struct AsciiHandler {
    /// Server state
    server_state: Arc<ServerState>,
    /// Stream forwarder for device communication
    stream_forwarder: Arc<StreamForwarder>,
    /// Active stream mappings with buffers (client_stream_id -> (device_stream_id, buffer))
    stream_mappings: Arc<RwLock<HashMap<u8, (u32, StreamBuffer)>>>,
    /// Selected device for this session
    selected_device: Arc<RwLock<Option<String>>>,
    /// Session ID
    session_id: String,
}

impl AsciiHandler {
    /// Create a new ASCII protocol handler
    pub fn new(
        server_state: Arc<ServerState>,
        stream_forwarder: Arc<StreamForwarder>,
        session_id: String,
    ) -> Self {
        Self {
            server_state,
            stream_forwarder,
            stream_mappings: Arc::new(RwLock::new(HashMap::new())),
            selected_device: Arc::new(RwLock::new(None)),
            session_id,
        }
    }

    /// Handle a client connection with ASCII protocol
    pub async fn handle_connection(&self, mut stream: TcpStream) -> Result<(), Box<dyn std::error::Error>> {
        let peer_addr = stream.peer_addr()?;
        info!("Handling ASCII connection from {} (session: {})", peer_addr, self.session_id);

        loop {
            // Read request length (4 hex bytes)
            let mut len_buf = [0u8; 4];
            match stream.read_exact(&mut len_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("Client {} disconnected", peer_addr);
                    break;
                }
                Err(e) => {
                    error!("Error reading from client {}: {}", peer_addr, e);
                    break;
                }
            }

            // Parse length
            let len_str = std::str::from_utf8(&len_buf)?;
            let length = u32::from_str_radix(len_str, 16)? as usize;

            if length > 65536 {
                error!("Request too large from {}: {} bytes", peer_addr, length);
                break;
            }

            // Read request data
            let mut request_buf = vec![0u8; length];
            stream.read_exact(&mut request_buf).await?;

            let request = String::from_utf8_lossy(&request_buf);
            debug!("Request from {}: '{}'", peer_addr, request);

            // Process request and send response
            let response = self.process_request(&request).await;
            stream.write_all(&response).await?;
            stream.flush().await?;
        }

        // Clean up temporary devices for this session
        self.cleanup_temporary_devices().await;

        info!("ASCII connection from {} closed", peer_addr);
        Ok(())
    }

    /// Process an ASCII request and generate response
    async fn process_request(&self, request: &str) -> Vec<u8> {
        // Handle STRM messages
        if request.starts_with("STRM") {
            return self.handle_strm_message(request.as_bytes()).await;
        }

        // Handle host services
        if request.starts_with("host:") {
            // Handle async transport selection
            if request.starts_with("host:transport:") {
                let device_id = request.strip_prefix("host:transport:").unwrap_or("");
                return self.handle_transport_selection(device_id).await;
            }
            // Handle async direct connection
            if request.starts_with("host:connect:") {
                let address = request.strip_prefix("host:connect:").unwrap_or("");
                return self.handle_direct_connection(address).await;
            }
            // Other host services are synchronous
            return self.handle_host_service(request);
        }

        // Handle local services (shell, tcp, sync)
        if request.starts_with("shell:") || request == "shell" {
            return self.handle_shell_service(request).await;
        }

        if request.starts_with("tcp:") {
            return self.handle_tcp_service(request).await;
        }

        if request.starts_with("sync:") || request == "sync" {
            return self.handle_sync_service(request).await;
        }

        // Unknown command
        warn!("Unknown command: {}", request);
        self.create_fail_response("Unknown command")
    }

    /// Handle host service requests
    fn handle_host_service(&self, request: &str) -> Vec<u8> {
        let (service_name, args) = if let Some(pos) = request.find(':') {
            let (name, rest) = request.split_at(pos + 1);
            if rest.contains(':') {
                // Format like "host:transport:device_id"
                let parts: Vec<&str> = request.splitn(3, ':').collect();
                if parts.len() >= 3 {
                    (format!("{}:{}", parts[0], parts[1]), parts[2].to_string())
                } else {
                    (name.to_string(), rest.to_string())
                }
            } else {
                (request.to_string(), String::new())
            }
        } else {
            (request.to_string(), String::new())
        };

        debug!("Host service: '{}', args: '{}'", service_name, args);

        // Handle special transport selection - this needs to be async, so we return early
        if service_name == "host:transport" && !args.is_empty() {
            // For transport selection, we'll handle it separately
            return self.create_fail_response("transport selection requires async");
        }

        // For simple host services, we can handle them synchronously
        // These are typically just information queries
        match service_name.as_str() {
            "host:version" => {
                self.create_okay_response(b"dbgif-server 1.0")
            }
            "host:features" => {
                self.create_okay_response(b"lazy-connection\nmulti-client\nping-pong\n")
            }
            "host:list" | "host:devices" => {
                // Get device list from registry
                let device_registry = &self.server_state.device_registry;
                let mut device_list = String::new();

                // List all devices with their actual status
                if let Ok(devices) = device_registry.list_devices() {
                    for device in devices {
                        // Map device state to status string
                        let status = match device.state {
                            crate::server::device_registry::DeviceState::Connected => "device",
                            crate::server::device_registry::DeviceState::Discovered => "offline",
                            crate::server::device_registry::DeviceState::Connecting => "connecting",
                            crate::server::device_registry::DeviceState::Disconnecting => "disconnecting",
                            crate::server::device_registry::DeviceState::Offline => "offline",
                            crate::server::device_registry::DeviceState::Error { .. } => "error",
                        };

                        // Get device metadata
                        let system_type = device.metadata.properties.get("system_type")
                            .map(|s| s.as_str())
                            .unwrap_or("unknown");
                        let model = device.metadata.model.as_deref()
                            .unwrap_or(device.name.as_str());
                        let version = device.metadata.os_version.as_deref()
                            .unwrap_or("1.0");

                        device_list.push_str(&format!("{}\t{}\t{}\t{}\t{}\n",
                                                      device.device_id, status, system_type, model, version));
                    }
                }

                self.create_okay_response(device_list.as_bytes())
            }
            _ => {
                warn!("Unknown host service: {}", service_name);
                self.create_fail_response("unknown host service")
            }
        }
    }

    /// Handle device transport selection
    async fn handle_transport_selection(&self, device_id: &str) -> Vec<u8> {
        info!("Selecting transport for device: {}", device_id);

        // Check if device exists
        let device_registry = &self.server_state.device_registry;

        if device_registry.get_device(device_id).is_ok() {
            // Store selected device
            let mut selected = self.selected_device.write().await;
            *selected = Some(device_id.to_string());

            info!("Device '{}' selected for session {}", device_id, self.session_id);
            self.create_okay_response(&[])
        } else {
            warn!("Device '{}' not found", device_id);
            self.create_fail_response("device not found")
        }
    }

    /// Handle direct connection to IP:port
    async fn handle_direct_connection(&self, address: &str) -> Vec<u8> {
        info!("Attempting direct connection to: {}", address);

        // Parse IP:port from address
        let (ip, port) = match self.parse_ip_port(address) {
            Ok((ip, port)) => (ip, port),
            Err(error) => {
                warn!("Invalid address format '{}': {}", address, error);
                return self.create_fail_response("invalid address");
            }
        };

        // Validate port range
        if port == 0 {
            warn!("Invalid port: {}", port);
            return self.create_fail_response("invalid port");
        }

        // Create a unique device ID for this direct connection
        let device_id = format!("direct:{}:{}", ip, port);

        // Check if device already exists
        let device_registry = &self.server_state.device_registry;
        if device_registry.get_device(&device_id).is_ok() {
            // Device already exists, just select it
            let mut selected = self.selected_device.write().await;
            *selected = Some(device_id.clone());
            info!("Device '{}' already exists, selected for session {}", device_id, self.session_id);
            return self.create_okay_response(&[]);
        }

        // Register as temporary device
        match self.register_direct_device(&ip, port, &device_id).await {
            Ok(()) => {
                // Store selected device
                let mut selected = self.selected_device.write().await;
                *selected = Some(device_id.clone());

                info!("Successfully registered temporary device '{}' for session {} ({}:{})", device_id, self.session_id, ip, port);
                self.create_okay_response(&[])
            }
            Err(error) => {
                warn!("Failed to register device {}:{}: {}", ip, port, error);
                self.create_fail_response("registration failed")
            }
        }
    }

    /// Parse IPv4:port from address string with localhost restriction
    fn parse_ip_port(&self, address: &str) -> Result<(String, u16), String> {
        // Parse IPv4 addresses: 127.0.0.1:5557
        if let Some(colon_pos) = address.rfind(':') {
            let ip = address[..colon_pos].to_string();
            let port_str = &address[colon_pos + 1..];
            let port = port_str.parse::<u16>()
                .map_err(|_| "invalid port number")?;

            // Basic IPv4 validation
            if ip.split('.').count() == 4 {
                for part in ip.split('.') {
                    if part.parse::<u8>().is_err() {
                        return Err("invalid IPv4 address".to_string());
                    }
                }

                // Security: Only allow localhost connections
                if ip != "127.0.0.1" {
                    return Err("only localhost connections allowed".to_string());
                }

                return Ok((ip, port));
            } else {
                return Err("only IPv4 addresses supported".to_string());
            }
        }

        Err("missing port number".to_string())
    }

    /// Register direct device without connection testing
    async fn register_direct_device(&self, ip: &str, port: u16, device_id: &str) -> Result<(), String> {
        use std::net::SocketAddr;
        use crate::server::device_registry::{DeviceInfo, DeviceState, DeviceCapabilities, DeviceMetadata, DeviceStats};
        use std::collections::HashMap;

        // Parse socket address for validation
        let socket_addr: SocketAddr = format!("{}:{}", ip, port)
            .parse()
            .map_err(|_| "invalid address format")?;

        // Create temporary device info
        let mut properties = HashMap::new();
        properties.insert("temporary".to_string(), "true".to_string());
        properties.insert("session_id".to_string(), self.session_id.clone());

        let device_info = DeviceInfo {
            device_id: device_id.to_string(),
            name: format!("Direct TCP {}:{}", ip, port),
            address: socket_addr,
            state: DeviceState::Discovered, // Will be connected on-demand
            capabilities: DeviceCapabilities::default(),
            metadata: DeviceMetadata {
                model: Some("Direct TCP Connection".to_string()),
                manufacturer: Some("DBGIF".to_string()),
                serial_number: Some(format!("tcp-{}-{}", ip, port)),
                os_version: None,
                properties,
            },
            last_seen: std::time::Instant::now(),
            stats: DeviceStats::default(),
        };

        // Register in device registry
        let device_registry = &self.server_state.device_registry;
        device_registry.register_device(device_info)
            .map_err(|e| format!("failed to register device: {}", e))?;

        info!("Successfully registered temporary device '{}' at {}:{}", device_id, ip, port);

        Ok(())
    }

    /// Cleanup temporary devices registered for this session
    async fn cleanup_temporary_devices(&self) {
        let device_registry = &self.server_state.device_registry;
        let all_devices = match device_registry.list_devices() {
            Ok(devices) => devices,
            Err(e) => {
                warn!("Failed to list devices for cleanup: {}", e);
                return;
            }
        };

        for device_info in all_devices {
            // Check if this is a temporary device for our session
            if let Some(temporary) = device_info.metadata.properties.get("temporary") {
                if temporary == "true" {
                    if let Some(session_id) = device_info.metadata.properties.get("session_id") {
                        if session_id == &self.session_id {
                            // Remove temporary device
                            match device_registry.unregister_device(&device_info.device_id) {
                                Ok(_) => {
                                    info!("Cleaned up temporary device '{}' for session {}", device_info.device_id, self.session_id);
                                }
                                Err(e) => {
                                    warn!("Failed to cleanup temporary device '{}': {}", device_info.device_id, e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Handle shell service
    async fn handle_shell_service(&self, request: &str) -> Vec<u8> {
        let selected_device = self.selected_device.read().await.clone();

        if selected_device.is_none() {
            return self.create_fail_response("no device selected");
        }

        let device_id = selected_device.unwrap();

        // Extract command from request if provided
        let service_name = if request == "shell" || request == "shell:" {
            "shell:".to_string()
        } else {
            request.to_string()
        };

        // Allocate stream ID with service name
        let stream_id = self.allocate_stream_id_with_service(service_name.clone()).await;

        // Open stream with the stream forwarder (no upstream_connection needed - registry handles it)
        let result = self.stream_forwarder.open_stream(
            self.session_id.clone(),
            stream_id as u32,
            stream_id as u32,  // Use same ID for both local and remote
            format!("{}:{}", device_id, service_name),
            None, // StreamForwarder will create connection from device registry
        ).await;

        match result {
            Ok(_) => {
                info!("Opened shell service with stream ID: {:02x} for device: {}", stream_id, device_id);
                // Return stream ID in response
                let response_data = format!("{:02x}", stream_id);
                self.create_okay_response(response_data.as_bytes())
            }
            Err(e) => {
                warn!("Failed to open shell stream: {}", e);
                self.create_fail_response("failed to open stream")
            }
        }
    }

    /// Handle TCP service
    async fn handle_tcp_service(&self, request: &str) -> Vec<u8> {
        let selected_device = self.selected_device.read().await.clone();

        if selected_device.is_none() {
            return self.create_fail_response("no device selected");
        }

        let device_id = selected_device.unwrap();

        let port = request.strip_prefix("tcp:").unwrap_or("");
        if port.is_empty() {
            return self.create_fail_response("invalid port");
        }

        // Allocate stream ID with TCP service name
        let stream_id = self.allocate_stream_id_with_service(format!("tcp:{}", port)).await;

        // Open stream with the stream forwarder (registry handles connection)
        let result = self.stream_forwarder.open_stream(
            self.session_id.clone(),
            stream_id as u32,
            stream_id as u32,
            format!("{}:{}", device_id, request),
            None, // StreamForwarder will create connection from device registry
        ).await;

        match result {
            Ok(_) => {
                info!("Opened TCP service on port {} with stream ID: {:02x} for device: {}", port, stream_id, device_id);
                let response_data = format!("{:02x}", stream_id);
                self.create_okay_response(response_data.as_bytes())
            }
            Err(e) => {
                warn!("Failed to open TCP stream: {}", e);
                self.create_fail_response("failed to open stream")
            }
        }
    }

    /// Handle sync service
    async fn handle_sync_service(&self, _request: &str) -> Vec<u8> {
        let selected_device = self.selected_device.read().await.clone();

        if selected_device.is_none() {
            return self.create_fail_response("no device selected");
        }

        let device_id = selected_device.unwrap();

        // Allocate stream ID with sync service
        let stream_id = self.allocate_stream_id_with_service("sync:".to_string()).await;

        // Open stream with the stream forwarder (registry handles connection)
        let result = self.stream_forwarder.open_stream(
            self.session_id.clone(),
            stream_id as u32,
            stream_id as u32,
            format!("{}:sync:", device_id),
            None, // StreamForwarder will create connection from device registry
        ).await;

        match result {
            Ok(_) => {
                info!("Opened sync service with stream ID: {:02x} for device: {}", stream_id, device_id);
                let response_data = format!("{:02x}", stream_id);
                self.create_okay_response(response_data.as_bytes())
            }
            Err(e) => {
                warn!("Failed to open sync stream: {}", e);
                self.create_fail_response("failed to open stream")
            }
        }
    }

    /// Handle STRM messages
    async fn handle_strm_message(&self, data: &[u8]) -> Vec<u8> {
        // Parse STRM message
        match ascii::decode_strm(data) {
            Ok((stream_id, payload)) => {
                if payload.is_empty() {
                    // Zero-length STRM closes the stream
                    info!("Closing stream {:02x}", stream_id);

                    // Close stream in the forwarder
                    let result = self.stream_forwarder.close_stream(
                        self.session_id.clone(),
                        stream_id as u32,
                        stream_id as u32,
                        "Client closed stream".to_string(),
                    ).await;

                    if let Err(e) = result {
                        warn!("Failed to close stream {:02x}: {}", stream_id, e);
                    }

                    self.close_stream(stream_id).await;
                    self.create_okay_response(&[])
                } else {
                    // Forward data to device
                    debug!("STRM message for stream {:02x}: {} bytes", stream_id, payload.len());

                    // Buffer the data for this stream
                    let result = {
                        let mut mappings = self.stream_mappings.write().await;
                        if let Some((device_stream_id, buffer)) = mappings.get_mut(&stream_id) {
                            // Add data to stream buffer
                            buffer.pending_data.extend_from_slice(&payload);

                            info!("Buffered {} bytes for stream {:02x} (device stream {}, total buffered: {} bytes)",
                                  payload.len(), stream_id, device_stream_id, buffer.pending_data.len());

                            // Flow control: if buffer is getting large, apply back-pressure
                            const MAX_BUFFER_SIZE: usize = 65536;  // 64KB threshold
                            if buffer.pending_data.len() > MAX_BUFFER_SIZE {
                                error!("Stream {:02x} buffer overflow: {} bytes exceeds limit",
                                       stream_id, buffer.pending_data.len());

                                // Deactivate stream to prevent more data
                                buffer.active = false;

                                // Clear excess data to prevent memory issues
                                if buffer.pending_data.len() > MAX_BUFFER_SIZE * 2 {
                                    warn!("Truncating stream {:02x} buffer from {} to {} bytes",
                                          stream_id, buffer.pending_data.len(), MAX_BUFFER_SIZE);
                                    buffer.pending_data.truncate(MAX_BUFFER_SIZE);
                                }

                                // Return error response for flow control
                                return self.create_fail_response("stream buffer overflow - flow control activated");
                            }

                            Ok(*device_stream_id)
                        } else {
                            Err("stream not found")
                        }
                    };

                    match result {
                        Ok(_device_stream_id) => {
                            // Process the buffered data immediately if device is connected
                            if let Some(device_id) = self.selected_device.read().await.as_ref() {
                                // Check if device is actually connected
                                let device_connected = self.server_state.device_registry
                                    .get_device(device_id)
                                    .ok()
                                    .and_then(|opt_dev| opt_dev)
                                    .map(|dev| dev.state == crate::server::device_registry::DeviceState::Connected)
                                    .unwrap_or(false);

                                if device_connected {
                                    // Get and clear the buffer
                                    let data_to_send = {
                                        let mut mappings = self.stream_mappings.write().await;
                                        if let Some((_, buffer)) = mappings.get_mut(&stream_id) {
                                            if !buffer.pending_data.is_empty() {
                                                let data = buffer.pending_data.clone();
                                                buffer.pending_data.clear();
                                                Some(data)
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    };

                                    if let Some(data) = data_to_send {
                                        info!("Sending {} bytes to device {} for stream {:02x}",
                                              data.len(), device_id, stream_id);

                                        // Forward data to device using stream forwarder
                                        // Get device stream ID from mappings
                                        let device_stream_id_to_use = {
                                            let mappings = self.stream_mappings.read().await;
                                            mappings.get(&stream_id).map(|(id, _)| *id)
                                        };

                                        if let Some(dev_stream_id) = device_stream_id_to_use {
                                            let client_stream_id = stream_id as u32;

                                            // Use stream forwarder to send data downstream to device
                                            match self.stream_forwarder.forward_downstream(
                                                self.session_id.clone(),
                                                client_stream_id,
                                                dev_stream_id,
                                                data,
                                            ).await {
                                                Ok(messages) => {
                                                    info!("Successfully forwarded data to device, {} messages generated",
                                                          messages.len());
                                                }
                                                Err(e) => {
                                                    error!("Failed to forward data to device: {}", e);
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    debug!("Device {} not connected, data buffered for stream {:02x}",
                                           device_id, stream_id);
                                }
                            }

                            self.create_okay_response(&[])
                        }
                        Err(e) => {
                            warn!("Stream {:02x} not found in mappings", stream_id);
                            self.create_fail_response(e)
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Invalid STRM message: {}", e);
                self.create_fail_response("invalid STRM format")
            }
        }
    }

    /// Allocate a new stream ID with service name
    async fn allocate_stream_id_with_service(&self, service_name: String) -> u8 {
        let mut mappings = self.stream_mappings.write().await;

        // Find an unused stream ID (1-255)
        for id in 1u8..=255 {
            if !mappings.contains_key(&id) {
                // Create stream buffer
                let buffer = StreamBuffer {
                    pending_data: Vec::with_capacity(4096),
                    service_name,
                    active: true,
                };
                // Reserve this ID with buffer
                mappings.insert(id, (id as u32, buffer));
                return id;
            }
        }

        // If all IDs are in use, reuse ID 1 (shouldn't happen in practice)
        warn!("All stream IDs in use, reusing ID 1");
        1
    }


    /// Close a stream
    async fn close_stream(&self, stream_id: u8) {
        let mut mappings = self.stream_mappings.write().await;
        if let Some((device_stream_id, mut buffer)) = mappings.remove(&stream_id) {
            buffer.active = false;
            let buffered_bytes = buffer.pending_data.len();
            if buffered_bytes > 0 {
                warn!("Closing stream {:02x} with {} buffered bytes (service: {})",
                      stream_id, buffered_bytes, buffer.service_name);
            }
            debug!("Closed stream {:02x} (device stream: {}, service: {})",
                   stream_id, device_stream_id, buffer.service_name);
        }
    }

    /// Create an OKAY response
    fn create_okay_response(&self, data: &[u8]) -> Vec<u8> {
        let mut response = Vec::with_capacity(8 + data.len());
        response.extend_from_slice(b"OKAY");

        let length = format!("{:04x}", data.len());
        response.extend_from_slice(length.as_bytes());
        response.extend_from_slice(data);

        response
    }

    /// Create a FAIL response
    fn create_fail_response(&self, message: &str) -> Vec<u8> {
        let mut response = Vec::with_capacity(8 + message.len());
        response.extend_from_slice(b"FAIL");

        let length = format!("{:04x}", message.len());
        response.extend_from_slice(length.as_bytes());
        response.extend_from_slice(message.as_bytes());

        response
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_okay_response() {
        let handler = create_test_handler();
        let response = handler.create_okay_response(b"test");
        assert_eq!(&response[0..4], b"OKAY");
        assert_eq!(&response[4..8], b"0004");
        assert_eq!(&response[8..], b"test");
    }

    #[test]
    fn test_fail_response() {
        let handler = create_test_handler();
        let response = handler.create_fail_response("error");
        assert_eq!(&response[0..4], b"FAIL");
        assert_eq!(&response[4..8], b"0005");
        assert_eq!(&response[8..], b"error");
    }

    fn create_test_handler() -> AsciiHandler {
        use crate::server::state::ServerConfig;

        let config = ServerConfig::default();
        let server_state = Arc::new(ServerState::new(config));
        let stream_forwarder = Arc::new(StreamForwarder::new(Arc::clone(&server_state)));

        AsciiHandler::new(
            server_state,
            stream_forwarder,
            "test-session".to_string(),
        )
    }
}