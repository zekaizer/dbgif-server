use crate::commands::Command;
use crate::crc::calculate_crc32;
use crate::error::DecodeError;
use crate::header::{MessageHeader, MessageBuilder};

#[cfg(feature = "std")]
use alloc::vec::Vec;

/// Message encoder for creating and serializing DBGIF protocol messages
pub struct MessageEncoder {
    #[cfg(feature = "std")]
    output_buffer: Vec<u8>,
    #[cfg(not(feature = "std"))]
    output_buffer: &'static mut [u8],
    #[cfg(not(feature = "std"))]
    output_position: usize,
}

impl MessageEncoder {
    /// Create a new encoder with std support
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        Self {
            output_buffer: Vec::new(),
        }
    }

    /// Create a new encoder with a static buffer for no_std
    #[cfg(not(feature = "std"))]
    pub fn with_static_buffer(buffer: &'static mut [u8]) -> Self {
        Self {
            output_buffer: buffer,
            output_position: 0,
        }
    }

    /// Encode a message without data payload
    pub fn encode_simple(
        &mut self,
        command: Command,
        arg0: u32,
        arg1: u32,
    ) -> Result<&[u8], DecodeError> {
        self.encode_with_data(command, arg0, arg1, &[])
    }

    /// Encode a message with data payload
    pub fn encode_with_data(
        &mut self,
        command: Command,
        arg0: u32,
        arg1: u32,
        data: &[u8],
    ) -> Result<&[u8], DecodeError> {
        let data_crc = if data.is_empty() {
            0
        } else {
            calculate_crc32(data)
        };

        let header = MessageBuilder::new()
            .command(command)
            .arg0(arg0)
            .arg1(arg1)
            .data_info(data.len() as u32, data_crc)
            .build();

        self.write_message(&header, data)
    }

    /// Encode a pre-built header with optional data
    pub fn encode_header(
        &mut self,
        header: &MessageHeader,
        data: &[u8],
    ) -> Result<&[u8], DecodeError> {
        // Verify data length and CRC match
        if header.data_length != data.len() as u32 {
            return Err(DecodeError::InvalidState);
        }

        if !data.is_empty() {
            let calculated_crc = calculate_crc32(data);
            if header.data_crc32 != calculated_crc {
                return Err(DecodeError::CrcMismatch);
            }
        }

        self.write_message(header, data)
    }

    /// Internal method to write message to buffer
    fn write_message(
        &mut self,
        header: &MessageHeader,
        data: &[u8],
    ) -> Result<&[u8], DecodeError> {
        let total_size = MessageHeader::SIZE + data.len();

        #[cfg(feature = "std")]
        {
            self.output_buffer.clear();
            self.output_buffer.reserve(total_size);

            // Serialize header
            let mut header_buf = [0u8; MessageHeader::SIZE];
            header.serialize(&mut header_buf);
            self.output_buffer.extend_from_slice(&header_buf);

            // Append data
            if !data.is_empty() {
                self.output_buffer.extend_from_slice(data);
            }

            Ok(&self.output_buffer)
        }

        #[cfg(not(feature = "std"))]
        {
            if total_size > self.output_buffer.len() {
                return Err(DecodeError::BufferTooSmall);
            }

            self.output_position = 0;

            // Serialize header
            let header_slice = &mut self.output_buffer[0..MessageHeader::SIZE];
            if let Ok(header_array) = header_slice.try_into() {
                header.serialize(header_array);
                self.output_position += MessageHeader::SIZE;
            } else {
                return Err(DecodeError::InvalidState);
            }

            // Append data
            if !data.is_empty() {
                let data_start = self.output_position;
                let data_end = data_start + data.len();
                self.output_buffer[data_start..data_end].copy_from_slice(data);
                self.output_position = data_end;
            }

            Ok(&self.output_buffer[..self.output_position])
        }
    }

    /// Get the current encoded message
    pub fn get_encoded(&self) -> &[u8] {
        #[cfg(feature = "std")]
        {
            &self.output_buffer
        }

        #[cfg(not(feature = "std"))]
        {
            &self.output_buffer[..self.output_position]
        }
    }

    /// Clear the encoder for reuse
    pub fn clear(&mut self) {
        #[cfg(feature = "std")]
        {
            self.output_buffer.clear();
        }

        #[cfg(not(feature = "std"))]
        {
            self.output_position = 0;
        }
    }

    /// Get the capacity of the encoder buffer
    pub fn capacity(&self) -> usize {
        #[cfg(feature = "std")]
        {
            self.output_buffer.capacity()
        }

        #[cfg(not(feature = "std"))]
        {
            self.output_buffer.len()
        }
    }
}

#[cfg(feature = "std")]
impl Default for MessageEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper builder for creating messages
#[cfg(feature = "std")]
pub struct MessageFactory;

#[cfg(feature = "std")]
impl MessageFactory {
    /// Create a CNXN message
    pub fn cnxn(version: u32, max_payload: u32, identity: &str) -> (MessageHeader, Vec<u8>) {
        let data = identity.as_bytes().to_vec();
        let crc = calculate_crc32(&data);

        let header = MessageBuilder::new()
            .command(Command::CNXN)
            .arg0(version)
            .arg1(max_payload)
            .data_info(data.len() as u32, crc)
            .build();

        (header, data)
    }

    /// Create an OPEN message
    pub fn open(local_id: u32, destination: &str) -> (MessageHeader, Vec<u8>) {
        let data = destination.as_bytes().to_vec();
        let crc = calculate_crc32(&data);

        let header = MessageBuilder::new()
            .command(Command::OPEN)
            .arg0(local_id)
            .arg1(0)
            .data_info(data.len() as u32, crc)
            .build();

        (header, data)
    }

    /// Create a WRTE message
    pub fn wrte(local_id: u32, remote_id: u32, data: &[u8]) -> (MessageHeader, Vec<u8>) {
        let crc = calculate_crc32(data);

        let header = MessageBuilder::new()
            .command(Command::WRTE)
            .arg0(local_id)
            .arg1(remote_id)
            .data_info(data.len() as u32, crc)
            .build();

        (header, data.to_vec())
    }

    /// Create an OKAY message
    pub fn okay(local_id: u32, remote_id: u32) -> MessageHeader {
        MessageBuilder::new()
            .command(Command::OKAY)
            .arg0(local_id)
            .arg1(remote_id)
            .build()
    }

    /// Create a CLSE message
    pub fn clse(local_id: u32, remote_id: u32) -> MessageHeader {
        MessageBuilder::new()
            .command(Command::CLSE)
            .arg0(local_id)
            .arg1(remote_id)
            .build()
    }

    /// Create a PING message
    pub fn ping() -> MessageHeader {
        MessageBuilder::new()
            .command(Command::PING)
            .build()
    }

    /// Create a PONG message
    pub fn pong() -> MessageHeader {
        MessageBuilder::new()
            .command(Command::PONG)
            .build()
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_encode_simple_message() {
        let mut encoder = MessageEncoder::new();

        let result = encoder.encode_simple(Command::PING, 0, 0).unwrap();

        assert_eq!(result.len(), MessageHeader::SIZE);

        // Verify header
        let header = MessageHeader::deserialize(
            result[..MessageHeader::SIZE].try_into().unwrap()
        ).unwrap();

        assert_eq!(header.command, Command::PING as u32);
        assert_eq!(header.arg0, 0);
        assert_eq!(header.arg1, 0);
        assert_eq!(header.data_length, 0);
        assert_eq!(header.data_crc32, 0);
    }

    #[test]
    fn test_encode_message_with_data() {
        let mut encoder = MessageEncoder::new();
        let test_data = b"Hello, World!";

        let result = encoder.encode_with_data(
            Command::WRTE,
            123,
            456,
            test_data
        ).unwrap();

        assert_eq!(result.len(), MessageHeader::SIZE + test_data.len());

        // Verify header
        let header = MessageHeader::deserialize(
            result[..MessageHeader::SIZE].try_into().unwrap()
        ).unwrap();

        assert_eq!(header.command, Command::WRTE as u32);
        assert_eq!(header.arg0, 123);
        assert_eq!(header.arg1, 456);
        assert_eq!(header.data_length, test_data.len() as u32);
        assert_eq!(header.data_crc32, calculate_crc32(test_data));

        // Verify data
        assert_eq!(&result[MessageHeader::SIZE..], test_data);
    }

    #[test]
    fn test_message_factory() {
        let (header, data) = MessageFactory::cnxn(
            0x01000000,
            256 * 1024,
            "device:test:v1.0"
        );

        assert_eq!(header.command, Command::CNXN as u32);
        assert_eq!(header.arg0, 0x01000000);
        assert_eq!(header.arg1, 256 * 1024);
        assert_eq!(data, b"device:test:v1.0");
        assert_eq!(header.data_crc32, calculate_crc32(&data));
    }
}