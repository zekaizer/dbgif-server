// ADB Protocol message contract definitions
// Defines the structure and validation rules for ADB messages

use bytes::Bytes;
use std::collections::HashMap;

/// ADB Protocol message structure (24-byte header + payload)
#[derive(Debug, Clone)]
pub struct AdbMessage {
    pub command: Command,
    pub arg0: u32,
    pub arg1: u32,
    pub data_length: u32,
    pub data_checksum: u32,
    pub magic: u32,
    pub payload: Bytes,
}

/// ADB command types as defined in protocol specification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Command {
    CNXN = 0x4e584e43,  // Connect
    AUTH = 0x48545541,  // Authentication
    OPEN = 0x4e45504f,  // Open stream
    OKAY = 0x59414b4f,  // Acknowledgment
    WRTE = 0x45545257,  // Write data
    CLSE = 0x45534c43,  // Close stream
    PING = 0x474e4950,  // Ping
    PONG = 0x474e4f50,  // Pong response
}

/// Authentication types for AUTH messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    Token = 1,      // Server sends random token
    Signature = 2,  // Client sends signed response
    RsaPublicKey = 3, // Client sends RSA public key
}

/// Stream states for connection tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Closed,
    Opening,
    Open,
    Closing,
}

/// Connection session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Authenticating,
    Connected,
    Disconnecting,
}

impl AdbMessage {
    /// Protocol constants
    pub const HEADER_SIZE: usize = 24;
    pub const MAX_DATA_SIZE: u32 = 256 * 1024; // 256KB
    pub const VERSION: u32 = 0x01000000;

    /// Create new message with automatic magic calculation
    pub fn new(command: Command, arg0: u32, arg1: u32, payload: Bytes) -> Self {
        let data_length = payload.len() as u32;
        let data_checksum = crc32fast::hash(&payload);
        let magic = !(command as u32);

        Self {
            command,
            arg0,
            arg1,
            data_length,
            data_checksum,
            magic,
            payload,
        }
    }

    /// Validate message integrity
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Check magic value
        if self.magic != !(self.command as u32) {
            return Err(ValidationError::InvalidMagic);
        }

        // Check data length
        if self.data_length != self.payload.len() as u32 {
            return Err(ValidationError::DataLengthMismatch);
        }

        // Check maximum data size
        if self.data_length > Self::MAX_DATA_SIZE {
            return Err(ValidationError::DataTooLarge);
        }

        // Check data checksum
        let calculated_checksum = crc32fast::hash(&self.payload);
        if self.data_checksum != calculated_checksum {
            return Err(ValidationError::ChecksumMismatch);
        }

        Ok(())
    }

    /// Serialize message to bytes (header + payload)
    pub fn serialize(&self) -> Bytes {
        let mut buffer = Vec::with_capacity(Self::HEADER_SIZE + self.payload.len());

        // Write header (little-endian)
        buffer.extend_from_slice(&(self.command as u32).to_le_bytes());
        buffer.extend_from_slice(&self.arg0.to_le_bytes());
        buffer.extend_from_slice(&self.arg1.to_le_bytes());
        buffer.extend_from_slice(&self.data_length.to_le_bytes());
        buffer.extend_from_slice(&self.data_checksum.to_le_bytes());
        buffer.extend_from_slice(&self.magic.to_le_bytes());

        // Write payload
        buffer.extend_from_slice(&self.payload);

        Bytes::from(buffer)
    }

    /// Deserialize message from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self, ValidationError> {
        if data.len() < Self::HEADER_SIZE {
            return Err(ValidationError::InsufficientData);
        }

        // Parse header
        let command = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let arg0 = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let arg1 = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let data_length = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let data_checksum = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let magic = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);

        // Validate total message length
        let total_length = Self::HEADER_SIZE + data_length as usize;
        if data.len() < total_length {
            return Err(ValidationError::InsufficientData);
        }

        // Extract payload
        let payload = if data_length > 0 {
            Bytes::copy_from_slice(&data[Self::HEADER_SIZE..total_length])
        } else {
            Bytes::new()
        };

        // Convert command to enum
        let command = Command::from_u32(command)
            .ok_or(ValidationError::InvalidCommand)?;

        let message = Self {
            command,
            arg0,
            arg1,
            data_length,
            data_checksum,
            magic,
            payload,
        };

        // Validate message
        message.validate()?;

        Ok(message)
    }
}

impl Command {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x4e584e43 => Some(Command::CNXN),
            0x48545541 => Some(Command::AUTH),
            0x4e45504f => Some(Command::OPEN),
            0x59414b4f => Some(Command::OKAY),
            0x45545257 => Some(Command::WRTE),
            0x45534c43 => Some(Command::CLSE),
            0x474e4950 => Some(Command::PING),
            0x474e4f50 => Some(Command::PONG),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidMagic,
    DataLengthMismatch,
    DataTooLarge,
    ChecksumMismatch,
    InsufficientData,
    InvalidCommand,
}

// Contract tests would verify:
// 1. Message serialization/deserialization round-trip
// 2. Checksum validation catches corruption
// 3. Magic value validation works correctly
// 4. Maximum data size limits enforced
// 5. Invalid commands rejected properly