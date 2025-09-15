use super::{AdbMessage as Message, Command};
use anyhow::{Result, Context};
use bytes::{Bytes, BytesMut};
use std::io::Write;

/// ADB message serializer for converting messages to byte streams
pub struct MessageSerializer {
    output_buffer: BytesMut,
    max_buffer_size: usize,
}

impl MessageSerializer {
    pub fn new() -> Self {
        Self {
            output_buffer: BytesMut::new(),
            max_buffer_size: 1024 * 1024, // 1MB max buffer
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            output_buffer: BytesMut::with_capacity(capacity),
            max_buffer_size: capacity * 2,
        }
    }

    /// Serialize a message and add it to the output buffer
    pub fn serialize_message(&mut self, message: &Message) -> Result<()> {
        let serialized = message.serialize()
            .context("Failed to serialize message")?;

        // Check buffer size limits
        if self.output_buffer.len() + serialized.len() > self.max_buffer_size {
            return Err(anyhow::anyhow!(
                "Output buffer would exceed maximum size: {} + {} > {}",
                self.output_buffer.len(),
                serialized.len(),
                self.max_buffer_size
            ));
        }

        self.output_buffer.extend_from_slice(&serialized);
        Ok(())
    }

    /// Serialize multiple messages in batch
    pub fn serialize_batch(&mut self, messages: &[Message]) -> Result<()> {
        for message in messages {
            self.serialize_message(message)?;
        }
        Ok(())
    }

    /// Get the serialized data and clear the buffer
    pub fn take_data(&mut self) -> Bytes {
        self.output_buffer.split().freeze()
    }

    /// Get the serialized data without clearing the buffer
    pub fn get_data(&self) -> Bytes {
        self.output_buffer.clone().freeze()
    }

    /// Clear the output buffer
    pub fn clear(&mut self) {
        self.output_buffer.clear();
    }

    /// Get current buffer size
    pub fn buffer_size(&self) -> usize {
        self.output_buffer.len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.output_buffer.is_empty()
    }

    /// Get remaining capacity before hitting max buffer size
    pub fn remaining_capacity(&self) -> usize {
        self.max_buffer_size.saturating_sub(self.output_buffer.len())
    }
}

impl Default for MessageSerializer {
    fn default() -> Self {
        Self::new()
    }
}

/// Stream-based message serializer for continuous serialization
pub struct StreamSerializer {
    serializer: MessageSerializer,
    message_count: usize,
    total_bytes_serialized: usize,
}

impl StreamSerializer {
    pub fn new() -> Self {
        Self {
            serializer: MessageSerializer::new(),
            message_count: 0,
            total_bytes_serialized: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            serializer: MessageSerializer::with_capacity(capacity),
            message_count: 0,
            total_bytes_serialized: 0,
        }
    }

    /// Serialize a message and update statistics
    pub fn serialize_message(&mut self, message: &Message) -> Result<()> {
        let initial_size = self.serializer.buffer_size();
        self.serializer.serialize_message(message)?;

        let bytes_added = self.serializer.buffer_size() - initial_size;
        self.message_count += 1;
        self.total_bytes_serialized += bytes_added;

        Ok(())
    }

    /// Serialize messages and return the data immediately
    pub fn serialize_and_take(&mut self, messages: &[Message]) -> Result<Bytes> {
        for message in messages {
            self.serialize_message(message)?;
        }
        Ok(self.serializer.take_data())
    }

    /// Get serialization statistics
    pub fn get_stats(&self) -> SerializerStats {
        SerializerStats {
            message_count: self.message_count,
            total_bytes_serialized: self.total_bytes_serialized,
            current_buffer_size: self.serializer.buffer_size(),
            remaining_capacity: self.serializer.remaining_capacity(),
        }
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.message_count = 0;
        self.total_bytes_serialized = 0;
    }

    /// Take data and reset statistics
    pub fn take_data_and_reset(&mut self) -> Bytes {
        let data = self.serializer.take_data();
        self.reset_stats();
        data
    }
}

impl Default for StreamSerializer {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about serializer performance
#[derive(Debug, Clone)]
pub struct SerializerStats {
    pub message_count: usize,
    pub total_bytes_serialized: usize,
    pub current_buffer_size: usize,
    pub remaining_capacity: usize,
}

/// Utility functions for message serialization
pub mod utils {
    use super::*;

    /// Serialize a single message to bytes
    pub fn serialize_single(message: &Message) -> Result<Bytes> {
        let serialized = message.serialize()?;
        Ok(Bytes::from(serialized))
    }

    /// Serialize multiple messages to a single byte buffer
    pub fn serialize_multiple(messages: &[Message]) -> Result<Bytes> {
        let mut total_size = 0;
        let mut serialized_messages = Vec::new();

        // Pre-serialize all messages to calculate total size
        for message in messages {
            let serialized = message.serialize()?;
            total_size += serialized.len();
            serialized_messages.push(serialized);
        }

        // Combine all serialized messages
        let mut output = BytesMut::with_capacity(total_size);
        for serialized in serialized_messages {
            output.extend_from_slice(&serialized);
        }

        Ok(output.freeze())
    }

    /// Serialize message directly to a writer
    pub fn serialize_to_writer<W: Write>(message: &Message, writer: &mut W) -> Result<usize> {
        let serialized = message.serialize()?;
        writer.write_all(&serialized)
            .context("Failed to write serialized message")?;
        Ok(serialized.len())
    }

    /// Create a message response pair (command + OKAY response)
    pub fn create_response_pair(
        original_command: Command,
        local_id: u32,
        remote_id: u32,
        response_data: Bytes,
    ) -> Result<(Message, Message)> {
        // Create the main response message
        let response = Message::new(original_command, local_id, remote_id, response_data);

        // Create the OKAY acknowledgment
        let okay = Message::new(Command::OKAY, local_id, remote_id, Bytes::new());

        Ok((response, okay))
    }

    /// Serialize a command-response pair
    pub fn serialize_response_pair(
        original_command: Command,
        local_id: u32,
        remote_id: u32,
        response_data: Bytes,
    ) -> Result<Bytes> {
        let (response, okay) = create_response_pair(original_command, local_id, remote_id, response_data)?;
        serialize_multiple(&[response, okay])
    }

    /// Calculate the serialized size of a message without actually serializing
    pub fn calculate_serialized_size(message: &Message) -> usize {
        Message::HEADER_SIZE + message.payload.len()
    }

    /// Calculate total size for multiple messages
    pub fn calculate_batch_size(messages: &[Message]) -> usize {
        messages.iter()
            .map(calculate_serialized_size)
            .sum()
    }
}

/// Builder pattern for creating and serializing message sequences
pub struct MessageSequenceBuilder {
    messages: Vec<Message>,
    max_messages: usize,
}

impl MessageSequenceBuilder {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            max_messages: 100, // Reasonable default
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            messages: Vec::with_capacity(capacity),
            max_messages: capacity,
        }
    }

    /// Add a message to the sequence
    pub fn add_message(mut self, message: Message) -> Result<Self> {
        if self.messages.len() >= self.max_messages {
            return Err(anyhow::anyhow!(
                "Message sequence exceeds maximum length: {}",
                self.max_messages
            ));
        }
        self.messages.push(message);
        Ok(self)
    }

    /// Add a CNXN message
    pub fn add_connect(self, version: u32, max_data: u32, system_identity: &str) -> Result<Self> {
        let message = Message::new(
            Command::CNXN,
            version,
            max_data,
            Bytes::from(system_identity.as_bytes().to_vec()),
        );
        self.add_message(message)
    }

    /// Add an AUTH message
    pub fn add_auth(self, auth_type: u32, auth_data: Bytes) -> Result<Self> {
        let message = Message::new(Command::AUTH, auth_type, 0, auth_data);
        self.add_message(message)
    }

    /// Add an OKAY message
    pub fn add_okay(self, local_id: u32, remote_id: u32) -> Result<Self> {
        let message = Message::new(Command::OKAY, local_id, remote_id, Bytes::new());
        self.add_message(message)
    }

    /// Add an OPEN message
    pub fn add_open(self, local_id: u32, destination: &str) -> Result<Self> {
        let message = Message::new(
            Command::OPEN,
            local_id,
            0,
            Bytes::from(destination.as_bytes().to_vec()),
        );
        self.add_message(message)
    }

    /// Add a WRTE message
    pub fn add_write(self, local_id: u32, remote_id: u32, data: Bytes) -> Result<Self> {
        let message = Message::new(Command::WRTE, local_id, remote_id, data);
        self.add_message(message)
    }

    /// Add a CLSE message
    pub fn add_close(self, local_id: u32, remote_id: u32) -> Result<Self> {
        let message = Message::new(Command::CLSE, local_id, remote_id, Bytes::new());
        self.add_message(message)
    }

    /// Build and serialize the message sequence
    pub fn build_and_serialize(self) -> Result<Bytes> {
        utils::serialize_multiple(&self.messages)
    }

    /// Get the messages without serializing
    pub fn build(self) -> Vec<Message> {
        self.messages
    }

    /// Get the number of messages in the sequence
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if the sequence is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

impl Default for MessageSequenceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Command;

    #[test]
    fn test_message_serializer_single() {
        let mut serializer = MessageSerializer::new();

        let message = Message::new(
            Command::CNXN,
            0x01000000,
            256 * 1024,
            Bytes::from("test_system"),
        );

        serializer.serialize_message(&message).unwrap();

        let data = serializer.take_data();
        assert!(!data.is_empty());
        assert_eq!(data.len(), Message::HEADER_SIZE + "test_system".len());
    }

    #[test]
    fn test_message_serializer_batch() {
        let mut serializer = MessageSerializer::new();

        let messages = vec![
            Message::new(Command::CNXN, 1, 0, Bytes::from("msg1")),
            Message::new(Command::OKAY, 2, 1, Bytes::new()),
            Message::new(Command::WRTE, 2, 1, Bytes::from("data")),
        ];

        serializer.serialize_batch(&messages).unwrap();

        let data = serializer.take_data();
        let expected_size = messages.iter()
            .map(|m| Message::HEADER_SIZE + m.payload.len())
            .sum::<usize>();

        assert_eq!(data.len(), expected_size);
    }

    #[test]
    fn test_stream_serializer_stats() {
        let mut stream_serializer = StreamSerializer::new();

        let message1 = Message::new(Command::PING, 0, 0, Bytes::new());
        let message2 = Message::new(Command::PONG, 0, 0, Bytes::new());

        stream_serializer.serialize_message(&message1).unwrap();
        stream_serializer.serialize_message(&message2).unwrap();

        let stats = stream_serializer.get_stats();
        assert_eq!(stats.message_count, 2);
        assert_eq!(stats.total_bytes_serialized, Message::HEADER_SIZE * 2);
    }

    #[test]
    fn test_utils_serialize_multiple() {
        let messages = vec![
            Message::new(Command::OPEN, 1, 0, Bytes::from("shell:")),
            Message::new(Command::OKAY, 0, 1, Bytes::new()),
        ];

        let serialized = utils::serialize_multiple(&messages).unwrap();

        let expected_size = (Message::HEADER_SIZE + "shell:".len()) + Message::HEADER_SIZE;
        assert_eq!(serialized.len(), expected_size);
    }

    #[test]
    fn test_message_sequence_builder() {
        let sequence = MessageSequenceBuilder::new()
            .add_connect(0x01000000, 256 * 1024, "test_device")
            .unwrap()
            .add_okay(1, 0)
            .unwrap()
            .add_open(2, "shell:")
            .unwrap()
            .build();

        assert_eq!(sequence.len(), 3);
        assert_eq!(sequence[0].command, Command::CNXN);
        assert_eq!(sequence[1].command, Command::OKAY);
        assert_eq!(sequence[2].command, Command::OPEN);
    }

    #[test]
    fn test_utils_response_pair() {
        let (response, okay) = utils::create_response_pair(
            Command::OPEN,
            1,
            2,
            Bytes::from("response_data"),
        ).unwrap();

        assert_eq!(response.command, Command::OPEN);
        assert_eq!(response.arg0, 1);
        assert_eq!(response.arg1, 2);
        assert_eq!(response.payload, Bytes::from("response_data"));

        assert_eq!(okay.command, Command::OKAY);
        assert_eq!(okay.arg0, 1);
        assert_eq!(okay.arg1, 2);
        assert_eq!(okay.payload, Bytes::new());
    }

    #[test]
    fn test_serializer_buffer_limits() {
        let mut serializer = MessageSerializer::with_capacity(100);

        // Create a message that will exceed the buffer limit
        let large_data = vec![0u8; 200];
        let message = Message::new(Command::WRTE, 1, 2, Bytes::from(large_data));

        let result = serializer.serialize_message(&message);
        assert!(result.is_err());
    }
}