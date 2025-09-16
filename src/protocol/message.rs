use crate::protocol::commands::AdbCommand;
use crate::protocol::crc::calculate_crc32;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct AdbMessage {
    pub command: u32,
    pub arg0: u32,
    pub arg1: u32,
    pub data_length: u32,
    pub data_crc32: u32,
    pub magic: u32,
    pub data: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum MessageError {
    #[error("Invalid message format")]
    InvalidFormat,
    #[error("Invalid magic number")]
    InvalidMagic,
    #[error("Data length mismatch")]
    DataLengthMismatch,
}

impl AdbMessage {
    /// Serialize message to binary format (24-byte header + data)
    /// Format: command(4) + arg0(4) + arg1(4) + data_length(4) + data_crc32(4) + magic(4) + data(variable)
    /// All fields are little-endian
    pub fn serialize(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(24 + self.data.len());

        // 24-byte header (all little-endian)
        buffer.extend_from_slice(&self.command.to_le_bytes());
        buffer.extend_from_slice(&self.arg0.to_le_bytes());
        buffer.extend_from_slice(&self.arg1.to_le_bytes());
        buffer.extend_from_slice(&self.data_length.to_le_bytes());
        buffer.extend_from_slice(&self.data_crc32.to_le_bytes());
        buffer.extend_from_slice(&self.magic.to_le_bytes());

        // Variable-length data payload
        buffer.extend_from_slice(&self.data);

        buffer
    }

    /// Deserialize binary data to AdbMessage
    /// Validates header size and data length
    pub fn deserialize(data: &[u8]) -> Result<Self, MessageError> {
        if data.len() < 24 {
            return Err(MessageError::InvalidFormat);
        }

        // Parse 24-byte header (all little-endian)
        let command = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let arg0 = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let arg1 = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let data_length = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let data_crc32 = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let magic = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);

        // Validate data length
        let expected_total_length = 24 + data_length as usize;
        if data.len() != expected_total_length {
            return Err(MessageError::DataLengthMismatch);
        }

        // Extract payload data
        let payload = if data_length > 0 {
            data[24..].to_vec()
        } else {
            Vec::new()
        };

        let message = AdbMessage {
            command,
            arg0,
            arg1,
            data_length,
            data_crc32,
            magic,
            data: payload,
        };

        // Validate magic number
        if !message.is_valid_magic() {
            return Err(MessageError::InvalidMagic);
        }

        Ok(message)
    }

    /// Validate magic number (should be bitwise NOT of command)
    pub fn is_valid_magic(&self) -> bool {
        self.magic == !self.command
    }

    /// Create a new CNXN (connection) message
    pub fn new_cnxn(version: u32, maxdata: u32, system_identity: Vec<u8>) -> Self {
        let data_length = system_identity.len() as u32;
        let data_crc32 = calculate_crc32(&system_identity);
        let command = AdbCommand::CNXN as u32;

        AdbMessage {
            command,
            arg0: version,
            arg1: maxdata,
            data_length,
            data_crc32,
            magic: !command, // Magic is bitwise NOT of command
            data: system_identity,
        }
    }

    /// Create a new OKAY message
    pub fn new_okay(arg0: u32, arg1: u32) -> Self {
        let command = AdbCommand::OKAY as u32;

        AdbMessage {
            command,
            arg0,
            arg1,
            data_length: 0,
            data_crc32: 0,
            magic: !command,
            data: Vec::new(),
        }
    }

    /// Create a new OPEN message
    pub fn new_open(local_id: u32, service_name: Vec<u8>) -> Self {
        let data_length = service_name.len() as u32;
        let data_crc32 = calculate_crc32(&service_name);
        let command = AdbCommand::OPEN as u32;

        AdbMessage {
            command,
            arg0: local_id,
            arg1: 0, // Remote ID assigned by server
            data_length,
            data_crc32,
            magic: !command,
            data: service_name,
        }
    }

    /// Create a new WRTE (write) message
    pub fn new_wrte(local_id: u32, remote_id: u32, data: Vec<u8>) -> Self {
        let data_length = data.len() as u32;
        let data_crc32 = calculate_crc32(&data);
        let command = AdbCommand::WRTE as u32;

        AdbMessage {
            command,
            arg0: local_id,
            arg1: remote_id,
            data_length,
            data_crc32,
            magic: !command,
            data,
        }
    }

    /// Create a new CLSE (close) message
    pub fn new_clse(local_id: u32, remote_id: u32) -> Self {
        let command = AdbCommand::CLSE as u32;

        AdbMessage {
            command,
            arg0: local_id,
            arg1: remote_id,
            data_length: 0,
            data_crc32: 0,
            magic: !command,
            data: Vec::new(),
        }
    }

    /// Validate data CRC32
    pub fn is_valid_crc(&self) -> bool {
        if self.data.is_empty() && self.data_crc32 == 0 {
            return true; // Empty data with CRC 0 is valid
        }
        calculate_crc32(&self.data) == self.data_crc32
    }

    /// Get header size (always 24 bytes)
    pub const fn header_size() -> usize {
        24
    }
}