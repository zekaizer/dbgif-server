use crate::protocol::{AdbMessage as Message, Command};
use anyhow::Result;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock, Mutex};
use std::time::{Duration, Instant};

/// Manages multiple concurrent ADB streams within a session
pub struct StreamMultiplexer {
    streams: Arc<RwLock<HashMap<u32, Arc<AdbStream>>>>,
    next_local_id: Arc<Mutex<u32>>,
    max_concurrent_streams: usize,
    stream_timeout: Duration,

    // Event channel for stream lifecycle events
    event_sender: mpsc::UnboundedSender<StreamEvent>,
}

impl StreamMultiplexer {
    pub fn new(event_sender: mpsc::UnboundedSender<StreamEvent>) -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
            next_local_id: Arc::new(Mutex::new(1)), // Start from 1, 0 is reserved
            max_concurrent_streams: 50,
            stream_timeout: Duration::from_secs(300), // 5 minutes
            event_sender,
        }
    }

    pub fn with_config(
        event_sender: mpsc::UnboundedSender<StreamEvent>,
        max_concurrent_streams: usize,
        stream_timeout: Duration,
    ) -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
            next_local_id: Arc::new(Mutex::new(1)),
            max_concurrent_streams,
            stream_timeout,
            event_sender,
        }
    }

    /// Create a new stream for a service
    pub async fn create_stream(&self, service_name: String) -> Result<Arc<AdbStream>> {
        // Check stream limits
        {
            let streams = self.streams.read().await;
            if streams.len() >= self.max_concurrent_streams {
                return Err(anyhow::anyhow!(
                    "Maximum concurrent streams reached: {}",
                    self.max_concurrent_streams
                ));
            }
        }

        // Allocate new local ID
        let local_id = {
            let mut next_id = self.next_local_id.lock().await;
            let id = *next_id;
            *next_id = next_id.wrapping_add(1);
            if *next_id == 0 {
                *next_id = 1; // Skip 0
            }
            id
        };

        // Create stream
        let stream = Arc::new(AdbStream::new(
            local_id,
            service_name.clone(),
            self.stream_timeout,
        ));

        // Store stream
        {
            let mut streams = self.streams.write().await;
            streams.insert(local_id, Arc::clone(&stream));
        }

        // Send event
        let _ = self.event_sender.send(StreamEvent::StreamCreated {
            local_id,
            service_name,
        });

        tracing::debug!("Created stream {} for service: {}", local_id, stream.service_name);

        Ok(stream)
    }

    /// Get an existing stream by local ID
    pub async fn get_stream(&self, local_id: u32) -> Option<Arc<AdbStream>> {
        let streams = self.streams.read().await;
        streams.get(&local_id).cloned()
    }

    /// Process incoming OPEN message and create stream
    pub async fn process_open_message(&self, message: &Message) -> Result<(Arc<AdbStream>, Message)> {
        if message.command != Command::OPEN {
            return Err(anyhow::anyhow!("Expected OPEN message, got {:?}", message.command));
        }

        let remote_id = message.arg0;
        let service_name = String::from_utf8_lossy(&message.payload).to_string();

        // Create new stream
        let stream = self.create_stream(service_name.clone()).await?;

        // Set remote ID
        stream.set_remote_id(remote_id).await?;

        // Create OKAY response
        let okay_response = Message::new(
            Command::OKAY,
            stream.local_id,
            remote_id,
            Bytes::new(),
        );

        tracing::info!(
            "Opened stream {} -> {} for service: {}",
            stream.local_id,
            remote_id,
            service_name
        );

        Ok((stream, okay_response))
    }

    /// Process incoming WRTE message
    pub async fn process_write_message(&self, message: &Message) -> Result<Message> {
        if message.command != Command::WRTE {
            return Err(anyhow::anyhow!("Expected WRTE message, got {:?}", message.command));
        }

        let remote_id = message.arg0;
        let local_id = message.arg1;

        let stream = self.get_stream(local_id).await
            .ok_or_else(|| anyhow::anyhow!("Stream not found: {}", local_id))?;

        // Verify stream is connected
        if stream.get_state().await != StreamState::Connected {
            return Err(anyhow::anyhow!(
                "Stream {} not in connected state",
                local_id
            ));
        }

        // Send data to stream
        stream.receive_data(message.payload.clone()).await?;

        // Create OKAY response
        let okay_response = Message::new(
            Command::OKAY,
            local_id,
            remote_id,
            Bytes::new(),
        );

        tracing::debug!(
            "Received {} bytes on stream {} -> {}",
            message.payload.len(),
            local_id,
            remote_id
        );

        Ok(okay_response)
    }

    /// Process incoming CLSE message
    pub async fn process_close_message(&self, message: &Message) -> Result<Option<Message>> {
        if message.command != Command::CLSE {
            return Err(anyhow::anyhow!("Expected CLSE message, got {:?}", message.command));
        }

        let remote_id = message.arg0;
        let local_id = message.arg1;

        if let Some(stream) = self.get_stream(local_id).await {
            // Close stream
            stream.close().await?;

            // Remove from active streams
            {
                let mut streams = self.streams.write().await;
                streams.remove(&local_id);
            }

            // Send event
            let _ = self.event_sender.send(StreamEvent::StreamClosed {
                local_id,
                remote_id,
            });

            tracing::info!("Closed stream {} -> {}", local_id, remote_id);

            // Create CLSE response
            let close_response = Message::new(
                Command::CLSE,
                local_id,
                remote_id,
                Bytes::new(),
            );

            Ok(Some(close_response))
        } else {
            tracing::warn!("Attempt to close non-existent stream: {}", local_id);
            Ok(None)
        }
    }

    /// Send data through a stream
    pub async fn send_stream_data(&self, local_id: u32, data: Bytes) -> Result<Message> {
        let stream = self.get_stream(local_id).await
            .ok_or_else(|| anyhow::anyhow!("Stream not found: {}", local_id))?;

        // Verify stream is connected
        if stream.get_state().await != StreamState::Connected {
            return Err(anyhow::anyhow!(
                "Stream {} not in connected state",
                local_id
            ));
        }

        let remote_id = stream.get_remote_id().await
            .ok_or_else(|| anyhow::anyhow!("Stream {} has no remote ID", local_id))?;

        // Update stream activity
        stream.update_activity().await;

        // Create WRTE message
        let write_message = Message::new(
            Command::WRTE,
            local_id,
            remote_id,
            data.clone(),
        );

        tracing::debug!(
            "Sending {} bytes on stream {} -> {}",
            data.len(),
            local_id,
            remote_id
        );

        Ok(write_message)
    }

    /// Close a stream
    pub async fn close_stream(&self, local_id: u32) -> Result<Option<Message>> {
        if let Some(stream) = self.get_stream(local_id).await {
            let remote_id = stream.get_remote_id().await;

            // Close stream
            stream.close().await?;

            // Remove from active streams
            {
                let mut streams = self.streams.write().await;
                streams.remove(&local_id);
            }

            // Send event
            if let Some(remote_id) = remote_id {
                let _ = self.event_sender.send(StreamEvent::StreamClosed {
                    local_id,
                    remote_id,
                });

                // Create CLSE message
                let close_message = Message::new(
                    Command::CLSE,
                    local_id,
                    remote_id,
                    Bytes::new(),
                );

                tracing::info!("Initiated close for stream {} -> {}", local_id, remote_id);
                Ok(Some(close_message))
            } else {
                tracing::warn!("Closing stream {} with no remote ID", local_id);
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Get all active streams
    pub async fn get_all_streams(&self) -> Vec<Arc<AdbStream>> {
        let streams = self.streams.read().await;
        streams.values().cloned().collect()
    }

    /// Cleanup expired streams
    pub async fn cleanup_expired_streams(&self) -> Result<usize> {
        let now = Instant::now();
        let mut expired_streams = Vec::new();

        // Find expired streams
        {
            let streams = self.streams.read().await;
            for (local_id, stream) in streams.iter() {
                if stream.is_expired(now) {
                    expired_streams.push(*local_id);
                }
            }
        }

        // Close expired streams
        for local_id in &expired_streams {
            if let Err(e) = self.close_stream(*local_id).await {
                tracing::warn!("Failed to close expired stream {}: {}", local_id, e);
            }
        }

        let count = expired_streams.len();
        if count > 0 {
            tracing::info!("Cleaned up {} expired streams", count);
        }

        Ok(count)
    }

    /// Get multiplexer statistics
    pub async fn get_stats(&self) -> MultiplexerStats {
        let streams = self.streams.read().await;
        let active_count = streams.len();

        let mut connected_count = 0;
        let mut opening_count = 0;

        for stream in streams.values() {
            match stream.get_state().await {
                StreamState::Connected => connected_count += 1,
                StreamState::Opening => opening_count += 1,
                _ => {}
            }
        }

        MultiplexerStats {
            active_streams: active_count,
            connected_streams: connected_count,
            opening_streams: opening_count,
            max_concurrent_streams: self.max_concurrent_streams,
            stream_timeout: self.stream_timeout,
        }
    }
}

/// Individual ADB stream within a session
pub struct AdbStream {
    pub local_id: u32,
    pub service_name: String,
    remote_id: Arc<Mutex<Option<u32>>>,
    state: Arc<Mutex<StreamState>>,
    created_at: Instant,
    last_activity: Arc<Mutex<Instant>>,
    timeout: Duration,

    // Data channels
    data_sender: mpsc::UnboundedSender<Bytes>,
    data_receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<Bytes>>>>,
}

impl AdbStream {
    pub fn new(local_id: u32, service_name: String, timeout: Duration) -> Self {
        let (data_sender, data_receiver) = mpsc::unbounded_channel();
        let now = Instant::now();

        Self {
            local_id,
            service_name,
            remote_id: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(StreamState::Opening)),
            created_at: now,
            last_activity: Arc::new(Mutex::new(now)),
            timeout,
            data_sender,
            data_receiver: Arc::new(Mutex::new(Some(data_receiver))),
        }
    }

    /// Set the remote stream ID (called when OPEN is processed)
    pub async fn set_remote_id(&self, remote_id: u32) -> Result<()> {
        let mut remote_id_guard = self.remote_id.lock().await;
        if remote_id_guard.is_some() {
            return Err(anyhow::anyhow!(
                "Remote ID already set for stream {}",
                self.local_id
            ));
        }

        *remote_id_guard = Some(remote_id);

        // Update state to connected
        let mut state = self.state.lock().await;
        *state = StreamState::Connected;

        self.update_activity().await;

        tracing::debug!(
            "Stream {} connected with remote ID {}",
            self.local_id,
            remote_id
        );

        Ok(())
    }

    /// Get the remote stream ID
    pub async fn get_remote_id(&self) -> Option<u32> {
        *self.remote_id.lock().await
    }

    /// Get current stream state
    pub async fn get_state(&self) -> StreamState {
        *self.state.lock().await
    }

    /// Receive data on this stream
    pub async fn receive_data(&self, data: Bytes) -> Result<()> {
        self.data_sender.send(data)
            .map_err(|_| anyhow::anyhow!("Stream data channel closed"))?;

        self.update_activity().await;
        Ok(())
    }

    /// Take the data receiver (for stream processing)
    pub async fn take_data_receiver(&self) -> Option<mpsc::UnboundedReceiver<Bytes>> {
        self.data_receiver.lock().await.take()
    }

    /// Close the stream
    pub async fn close(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        *state = StreamState::Closed;

        tracing::debug!("Stream {} closed", self.local_id);
        Ok(())
    }

    /// Check if stream is expired
    pub fn is_expired(&self, current_time: Instant) -> bool {
        current_time.duration_since(self.created_at) > self.timeout
    }

    /// Update last activity timestamp
    pub async fn update_activity(&self) {
        let mut last_activity = self.last_activity.lock().await;
        *last_activity = Instant::now();
    }

    /// Get stream information
    pub async fn get_info(&self) -> StreamInfo {
        let state = *self.state.lock().await;
        let remote_id = *self.remote_id.lock().await;
        let last_activity = *self.last_activity.lock().await;

        StreamInfo {
            local_id: self.local_id,
            remote_id,
            service_name: self.service_name.clone(),
            state,
            created_at: self.created_at,
            last_activity,
            timeout: self.timeout,
        }
    }
}

/// ADB stream state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Opening,
    Connected,
    Closing,
    Closed,
}

/// Stream lifecycle events
#[derive(Debug, Clone)]
pub enum StreamEvent {
    StreamCreated {
        local_id: u32,
        service_name: String,
    },
    StreamConnected {
        local_id: u32,
        remote_id: u32,
        service_name: String,
    },
    StreamClosed {
        local_id: u32,
        remote_id: u32,
    },
    StreamError {
        local_id: u32,
        error: String,
    },
}

/// Stream information for monitoring
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub local_id: u32,
    pub remote_id: Option<u32>,
    pub service_name: String,
    pub state: StreamState,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub timeout: Duration,
}

/// Multiplexer statistics
#[derive(Debug, Clone)]
pub struct MultiplexerStats {
    pub active_streams: usize,
    pub connected_streams: usize,
    pub opening_streams: usize,
    pub max_concurrent_streams: usize,
    pub stream_timeout: Duration,
}

/// Create event channel for stream events
pub fn create_stream_event_channel() -> (mpsc::UnboundedSender<StreamEvent>, mpsc::UnboundedReceiver<StreamEvent>) {
    mpsc::unbounded_channel()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_multiplexer_creation() {
        let (sender, _receiver) = create_stream_event_channel();
        let multiplexer = StreamMultiplexer::new(sender);

        let stats = multiplexer.get_stats().await;
        assert_eq!(stats.active_streams, 0);
        assert_eq!(stats.max_concurrent_streams, 50);
    }

    #[tokio::test]
    async fn test_stream_creation() {
        let (sender, _receiver) = create_stream_event_channel();
        let multiplexer = StreamMultiplexer::new(sender);

        let stream = multiplexer.create_stream("shell:".to_string()).await.unwrap();

        assert_eq!(stream.local_id, 1);
        assert_eq!(stream.service_name, "shell:");

        let state = stream.get_state().await;
        assert_eq!(state, StreamState::Opening);
    }

    #[tokio::test]
    async fn test_open_message_processing() {
        let (sender, _receiver) = create_stream_event_channel();
        let multiplexer = StreamMultiplexer::new(sender);

        let open_message = Message::new(
            Command::OPEN,
            42, // remote_id
            0,
            Bytes::from("shell:"),
        );

        let (stream, response) = multiplexer.process_open_message(&open_message).await.unwrap();

        assert_eq!(stream.local_id, 1);
        assert_eq!(stream.service_name, "shell:");
        assert_eq!(stream.get_remote_id().await, Some(42));

        assert_eq!(response.command, Command::OKAY);
        assert_eq!(response.arg0, 1); // local_id
        assert_eq!(response.arg1, 42); // remote_id
    }

    #[tokio::test]
    async fn test_write_message_processing() {
        let (sender, _receiver) = create_stream_event_channel();
        let multiplexer = StreamMultiplexer::new(sender);

        // First create and connect a stream
        let open_message = Message::new(
            Command::OPEN,
            42,
            0,
            Bytes::from("shell:"),
        );

        let (stream, _) = multiplexer.process_open_message(&open_message).await.unwrap();

        // Now send data
        let write_message = Message::new(
            Command::WRTE,
            42, // remote_id
            stream.local_id, // local_id
            Bytes::from("echo hello"),
        );

        let response = multiplexer.process_write_message(&write_message).await.unwrap();

        assert_eq!(response.command, Command::OKAY);
        assert_eq!(response.arg0, stream.local_id);
        assert_eq!(response.arg1, 42);
    }

    #[tokio::test]
    async fn test_stream_data_flow() {
        let stream = AdbStream::new(
            1,
            "test:".to_string(),
            Duration::from_secs(60),
        );

        // Set remote ID
        stream.set_remote_id(42).await.unwrap();
        assert_eq!(stream.get_state().await, StreamState::Connected);

        // Receive data
        let test_data = Bytes::from("test data");
        stream.receive_data(test_data.clone()).await.unwrap();

        // Take receiver and verify data
        if let Some(mut receiver) = stream.take_data_receiver().await {
            let received = receiver.recv().await.unwrap();
            assert_eq!(received, test_data);
        } else {
            panic!("Failed to get data receiver");
        }
    }
}