use super::{AdbMessage as Message, Command};
use anyhow::{Result, Context};
use bytes::{Bytes, BytesMut, Buf};
use std::io::Cursor;

/// ADB message parser for handling incoming data streams
pub struct MessageParser {
    buffer: BytesMut,
    expected_header_size: usize,
    expected_payload_size: Option<usize>,
}

impl MessageParser {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
            expected_header_size: Message::HEADER_SIZE,
            expected_payload_size: None,
        }
    }

    /// Add incoming data to the parser buffer
    pub fn feed_data(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Try to parse the next complete message from the buffer
    pub fn try_parse_message(&mut self) -> Result<Option<Message>> {
        // Check if we have enough data for header
        if self.buffer.len() < self.expected_header_size {
            return Ok(None);
        }

        // If we haven't determined payload size yet, parse header
        if self.expected_payload_size.is_none() {
            let header_data = &self.buffer[..self.expected_header_size];
            let payload_size = self.extract_payload_size(header_data)?;
            self.expected_payload_size = Some(payload_size);
        }

        let payload_size = self.expected_payload_size.unwrap();
        let total_message_size = self.expected_header_size + payload_size;

        // Check if we have the complete message
        if self.buffer.len() < total_message_size {
            return Ok(None);
        }

        // Extract the complete message
        let message_data = self.buffer.split_to(total_message_size);
        let (header_data, payload_data) = message_data.split_at(self.expected_header_size);

        // Parse the message
        let message = Message::deserialize(header_data, Bytes::from(payload_data.to_vec()))?;

        // Reset for next message
        self.expected_payload_size = None;

        Ok(Some(message))
    }

    /// Extract payload size from header data
    fn extract_payload_size(&self, header_data: &[u8]) -> Result<usize> {
        if header_data.len() != Message::HEADER_SIZE {
            return Err(anyhow::anyhow!(
                "Invalid header size: expected {}, got {}",
                Message::HEADER_SIZE,
                header_data.len()
            ));
        }

        let mut cursor = Cursor::new(header_data);

        // Skip command (4 bytes)
        cursor.advance(4);

        // Skip arg0 (4 bytes)
        cursor.advance(4);

        // Skip arg1 (4 bytes)
        cursor.advance(4);

        // Read data_length (4 bytes) - this is the payload size
        let data_length = cursor.get_u32_le() as usize;

        // Validate payload size
        if data_length > Message::MAX_PAYLOAD_SIZE {
            return Err(anyhow::anyhow!(
                "Payload size too large: {} > {}",
                data_length,
                Message::MAX_PAYLOAD_SIZE
            ));
        }

        Ok(data_length)
    }

    /// Get the current buffer size (for debugging)
    pub fn buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// Clear the internal buffer (useful for error recovery)
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
        self.expected_payload_size = None;
    }

    /// Check if the parser is in a valid state
    pub fn is_ready_for_next_message(&self) -> bool {
        self.expected_payload_size.is_none()
    }

    /// Peek at the next command type without consuming data
    pub fn peek_next_command(&self) -> Result<Option<Command>> {
        if self.buffer.len() < 4 {
            return Ok(None);
        }

        let mut cursor = Cursor::new(&self.buffer[..4]);
        let command_value = cursor.get_u32_le();

        match Command::try_from(command_value) {
            Ok(command) => Ok(Some(command)),
            Err(_) => Err(anyhow::anyhow!("Invalid command value: 0x{:08x}", command_value)),
        }
    }
}

impl Default for MessageParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Stream-based message parser for continuous parsing
pub struct StreamParser {
    parser: MessageParser,
    messages: Vec<Message>,
}

impl StreamParser {
    pub fn new() -> Self {
        Self {
            parser: MessageParser::new(),
            messages: Vec::new(),
        }
    }

    /// Process incoming data and return all complete messages
    pub fn process_data(&mut self, data: &[u8]) -> Result<Vec<Message>> {
        self.parser.feed_data(data);
        self.messages.clear();

        // Parse all available messages
        while let Some(message) = self.parser.try_parse_message()? {
            self.messages.push(message);
        }

        Ok(self.messages.clone())
    }

    /// Get parsing statistics
    pub fn get_stats(&self) -> ParserStats {
        ParserStats {
            buffer_size: self.parser.buffer_size(),
            is_ready: self.parser.is_ready_for_next_message(),
            messages_parsed: self.messages.len(),
        }
    }
}

impl Default for StreamParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about parser performance
#[derive(Debug, Clone)]
pub struct ParserStats {
    pub buffer_size: usize,
    pub is_ready: bool,
    pub messages_parsed: usize,
}

/// Utility functions for message parsing
pub mod utils {
    use super::*;

    /// Parse a single complete message from bytes
    pub fn parse_single_message(data: &[u8]) -> Result<Message> {
        if data.len() < Message::HEADER_SIZE {
            return Err(anyhow::anyhow!(
                "Data too short for ADB message header: {} < {}",
                data.len(),
                Message::HEADER_SIZE
            ));
        }

        let (header_data, payload_data) = data.split_at(Message::HEADER_SIZE);
        Message::deserialize(header_data, Bytes::from(payload_data.to_vec()))
    }

    /// Validate message header without parsing payload
    pub fn validate_header(header: &[u8]) -> Result<(Command, u32, u32, u32)> {
        if header.len() != Message::HEADER_SIZE {
            return Err(anyhow::anyhow!("Invalid header size"));
        }

        let mut cursor = Cursor::new(header);

        let command_value = cursor.get_u32_le();
        let arg0 = cursor.get_u32_le();
        let arg1 = cursor.get_u32_le();
        let data_length = cursor.get_u32_le();
        let _data_checksum = cursor.get_u32_le();
        let actual_magic = cursor.get_u32_le();

        let command = Command::try_from(command_value)
            .context("Invalid command value")?;

        // Validate magic value
        let expected_magic = !command_value;

        if actual_magic != expected_magic {
            return Err(anyhow::anyhow!(
                "Invalid magic value: expected 0x{:08x}, got 0x{:08x}",
                expected_magic,
                actual_magic
            ));
        }

        Ok((command, arg0, arg1, data_length))
    }

    /// Calculate expected message size from header
    pub fn calculate_message_size(header: &[u8]) -> Result<usize> {
        let (_, _, _, data_length) = validate_header(header)?;
        Ok(Message::HEADER_SIZE + data_length as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Command;

    #[test]
    fn test_message_parser_single_message() {
        let mut parser = MessageParser::new();

        // Create a test message
        let test_message = Message::new(
            Command::CNXN,
            0x12345678,
            0x87654321,
            Bytes::from("test payload"),
        );

        let serialized = test_message.serialize().unwrap();

        // Feed data to parser
        parser.feed_data(&serialized);

        // Parse message
        let parsed = parser.try_parse_message().unwrap();
        assert!(parsed.is_some());

        let parsed_message = parsed.unwrap();
        assert_eq!(parsed_message.command, Command::CNXN);
        assert_eq!(parsed_message.arg0, 0x12345678);
        assert_eq!(parsed_message.arg1, 0x87654321);
        assert_eq!(parsed_message.payload, Bytes::from("test payload"));
    }

    #[test]
    fn test_message_parser_partial_data() {
        let mut parser = MessageParser::new();

        // Create a test message
        let test_message = Message::new(
            Command::OKAY,
            0,
            0,
            Bytes::from("hello"),
        );

        let serialized = test_message.serialize().unwrap();

        // Feed data in parts
        let mid_point = serialized.len() / 2;
        parser.feed_data(&serialized[..mid_point]);

        // Should not parse yet
        let result = parser.try_parse_message().unwrap();
        assert!(result.is_none());

        // Feed rest of data
        parser.feed_data(&serialized[mid_point..]);

        // Now should parse
        let parsed = parser.try_parse_message().unwrap();
        assert!(parsed.is_some());

        let parsed_message = parsed.unwrap();
        assert_eq!(parsed_message.command, Command::OKAY);
        assert_eq!(parsed_message.payload, Bytes::from("hello"));
    }

    #[test]
    fn test_stream_parser_multiple_messages() {
        let mut stream_parser = StreamParser::new();

        // Create multiple test messages
        let msg1 = Message::new(Command::CNXN, 1, 0, Bytes::from("msg1"));
        let msg2 = Message::new(Command::OKAY, 2, 0, Bytes::from("msg2"));

        let mut combined_data = Vec::new();
        combined_data.extend_from_slice(&msg1.serialize().unwrap());
        combined_data.extend_from_slice(&msg2.serialize().unwrap());

        // Process combined data
        let messages = stream_parser.process_data(&combined_data).unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].command, Command::CNXN);
        assert_eq!(messages[0].payload, Bytes::from("msg1"));
        assert_eq!(messages[1].command, Command::OKAY);
        assert_eq!(messages[1].payload, Bytes::from("msg2"));
    }

    #[test]
    fn test_peek_next_command() {
        let mut parser = MessageParser::new();

        // Create test data with PING command
        let test_message = Message::new(Command::PING, 0, 0, Bytes::new());
        let serialized = test_message.serialize().unwrap();

        parser.feed_data(&serialized);

        // Peek at command without consuming
        let peeked_command = parser.peek_next_command().unwrap();
        assert_eq!(peeked_command, Some(Command::PING));

        // Verify we can still parse the full message
        let parsed = parser.try_parse_message().unwrap();
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().command, Command::PING);
    }

    #[test]
    fn test_utils_parse_single_message() {
        let test_message = Message::new(
            Command::WRTE,
            0x11111111,
            0x22222222,
            Bytes::from("test data"),
        );

        let serialized = test_message.serialize().unwrap();
        let parsed = utils::parse_single_message(&serialized).unwrap();

        assert_eq!(parsed.command, Command::WRTE);
        assert_eq!(parsed.arg0, 0x11111111);
        assert_eq!(parsed.arg1, 0x22222222);
        assert_eq!(parsed.payload, Bytes::from("test data"));
    }

    #[test]
    fn test_utils_validate_header() {
        let test_message = Message::new(Command::CLSE, 0x99999999, 0x11111111, Bytes::new());
        let serialized = test_message.serialize().unwrap();
        let header = &serialized[..Message::HEADER_SIZE];

        let (command, arg0, arg1, data_length) = utils::validate_header(header).unwrap();

        assert_eq!(command, Command::CLSE);
        assert_eq!(arg0, 0x99999999);
        assert_eq!(arg1, 0x11111111);
        assert_eq!(data_length, 0);
    }
}