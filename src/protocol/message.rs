use anyhow::Result;
use bytes::Bytes;
use std::io::{Cursor, Write as IoWrite};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Command {
    CNXN = 0x4e584e43, // "CNXN" - Connection
    AUTH = 0x48545541, // "AUTH" - Authentication
    OPEN = 0x4e45504f, // "OPEN" - Open stream
    OKAY = 0x59414b4f, // "OKAY" - Acknowledgment
    WRTE = 0x45545257, // "WRTE" - Write data
    CLSE = 0x45534c43, // "CLSE" - Close stream
    PING = 0x474e4950, // "PING" - Keep-alive
    PONG = 0x474e4f50, // "PONG" - Keep-alive response
}

impl Command {
    /// Create Command from u32 value
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

    /// Convert Command to u32 value
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    /// Get magic value (bitwise NOT of command)
    pub fn magic(self) -> u32 {
        !self.to_u32()
    }
}

impl TryFrom<u32> for Command {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::from_u32(value).ok_or_else(|| anyhow::anyhow!("Invalid command value: 0x{:08x}", value))
    }
}

/// ADB protocol message structure
///
/// ADB messages consist of a 24-byte header followed by optional payload data.
/// All values are in little-endian format.
#[derive(Debug, Clone)]
pub struct AdbMessage {
    /// Command type
    pub command: Command,
    /// First argument (usage varies by command)
    pub arg0: u32,
    /// Second argument (usage varies by command)
    pub arg1: u32,
    /// Length of payload data
    pub data_length: u32,
    /// CRC32 checksum of payload data
    pub data_checksum: u32,
    /// Magic value (should be !command)
    pub magic: u32,
    /// Payload data
    pub payload: Bytes,
}

// Legacy alias for backwards compatibility
pub type Message = AdbMessage;

impl AdbMessage {
    /// ADB message header size in bytes (24 bytes total)
    pub const HEADER_SIZE: usize = 24;

    /// Maximum payload size (256KB)
    pub const MAX_PAYLOAD_SIZE: usize = 256 * 1024;

    /// Create a new ADB message
    pub fn new(command: Command, arg0: u32, arg1: u32, payload: Bytes) -> Self {
        let data_length = payload.len() as u32;
        let data_checksum = if data_length > 0 {
            crc32fast::hash(&payload)
        } else {
            0
        };
        let magic = command.magic();

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

    /// Create a new ADB message without payload
    pub fn new_empty(command: Command, arg0: u32, arg1: u32) -> Self {
        Self::new(command, arg0, arg1, Bytes::new())
    }

    /// Serialize the message to bytes
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut buffer = Vec::with_capacity(24 + self.payload.len());

        // Write 24-byte header
        buffer.write_u32::<LittleEndian>(self.command.to_u32())?;
        buffer.write_u32::<LittleEndian>(self.arg0)?;
        buffer.write_u32::<LittleEndian>(self.arg1)?;
        buffer.write_u32::<LittleEndian>(self.data_length)?;
        buffer.write_u32::<LittleEndian>(self.data_checksum)?;
        buffer.write_u32::<LittleEndian>(self.magic)?;

        // Write payload if present
        if !self.payload.is_empty() {
            buffer.write_all(&self.payload)?;
        }

        Ok(buffer)
    }

    /// Deserialize message from header bytes and payload
    pub fn deserialize(header: &[u8], payload: Bytes) -> Result<Self> {
        if header.len() != 24 {
            return Err(anyhow::anyhow!("Invalid header length: expected 24, got {}", header.len()));
        }

        let mut cursor = Cursor::new(header);

        let command_u32 = cursor.read_u32::<LittleEndian>()?;
        let command = Command::from_u32(command_u32)
            .ok_or_else(|| anyhow::anyhow!("Invalid command: 0x{:08x}", command_u32))?;

        let arg0 = cursor.read_u32::<LittleEndian>()?;
        let arg1 = cursor.read_u32::<LittleEndian>()?;
        let data_length = cursor.read_u32::<LittleEndian>()?;
        let data_checksum = cursor.read_u32::<LittleEndian>()?;
        let magic = cursor.read_u32::<LittleEndian>()?;

        // Validate magic value
        if magic != command.magic() {
            return Err(anyhow::anyhow!(
                "Invalid magic value: expected 0x{:08x}, got 0x{:08x}",
                command.magic(),
                magic
            ));
        }

        // Validate payload length
        if payload.len() != data_length as usize {
            return Err(anyhow::anyhow!(
                "Payload length mismatch: expected {}, got {}",
                data_length,
                payload.len()
            ));
        }

        // Validate checksum if payload is present
        if data_length > 0 {
            let calculated_checksum = crc32fast::hash(&payload);
            if calculated_checksum != data_checksum {
                return Err(anyhow::anyhow!(
                    "Checksum mismatch: expected 0x{:08x}, got 0x{:08x}",
                    data_checksum,
                    calculated_checksum
                ));
            }
        }

        Ok(Self {
            command,
            arg0,
            arg1,
            data_length,
            data_checksum,
            magic,
            payload,
        })
    }

    /// Check if this message is valid
    pub fn is_valid(&self) -> bool {
        // Check magic value
        if self.magic != self.command.magic() {
            return false;
        }

        // Check payload length
        if self.payload.len() != self.data_length as usize {
            return false;
        }

        // Check checksum if payload is present
        if self.data_length > 0 {
            let calculated_checksum = crc32fast::hash(&self.payload);
            if calculated_checksum != self.data_checksum {
                return false;
            }
        }

        true
    }

    /// Get the total message size (header + payload)
    pub fn total_size(&self) -> usize {
        24 + self.payload.len()
    }

    /// Create a CNXN message
    pub fn cnxn(version: u32, maxdata: u32) -> Self {
        Self::new_empty(Command::CNXN, version, maxdata)
    }

    /// Create an AUTH message
    pub fn auth(auth_type: u32, auth_data: Bytes) -> Self {
        Self::new(Command::AUTH, auth_type, 0, auth_data)
    }

    /// Create an OPEN message
    pub fn open(local_id: u32, service: &str) -> Self {
        let service_bytes = service.as_bytes();
        Self::new(Command::OPEN, local_id, 0, Bytes::copy_from_slice(service_bytes))
    }

    /// Create an OKAY message
    pub fn okay(local_id: u32, remote_id: u32) -> Self {
        Self::new_empty(Command::OKAY, local_id, remote_id)
    }

    /// Create a WRTE message
    pub fn wrte(local_id: u32, remote_id: u32, data: Bytes) -> Self {
        Self::new(Command::WRTE, local_id, remote_id, data)
    }

    /// Create a CLSE message
    pub fn clse(local_id: u32, remote_id: u32) -> Self {
        Self::new_empty(Command::CLSE, local_id, remote_id)
    }

    /// Create a PING message
    pub fn ping() -> Self {
        Self::new_empty(Command::PING, 0, 0)
    }

    /// Create a PONG message
    pub fn pong() -> Self {
        Self::new_empty(Command::PONG, 0, 0)
    }

    fn checksum(&self) -> u32 {
        crc32fast::hash(&self.payload)
    }

    /// Generate debug-friendly formatted string
    pub fn debug_format(&self) -> String {
        format!(
            "AdbMessage {{ cmd: {:?} (0x{:08X}), arg0: 0x{:08X}, arg1: 0x{:08X}, data_len: {} }}",
            self.command,
            self.command.to_u32(),
            self.arg0,
            self.arg1,
            self.payload.len()
        )
    }

    /// Generate detailed debug information including raw bytes
    pub fn debug_raw(&self) -> String {
        let raw = self.serialize().unwrap_or_default();
        let mut output = self.debug_format();

        output.push_str("\nHeader (24 bytes):\n");
        if raw.len() >= 24 {
            output.push_str(&format!("{:02x?}", &raw[..24]));
        }

        if !self.payload.is_empty() {
            output.push_str("\nData payload:\n");
            output.push_str(&format!("{:02x?}", &self.payload[..std::cmp::min(256, self.payload.len())]));
        }

        output
    }

    /// Get compact representation for logging
    pub fn debug_compact(&self) -> String {
        let raw = self.serialize().unwrap_or_default();
        format!(
            "{:?}(0x{:08X}) [{} bytes]",
            self.command,
            self.command.to_u32(),
            raw.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_conversion() {
        assert_eq!(Command::from_u32(0x4e584e43), Some(Command::CNXN));
        assert_eq!(Command::CNXN.to_u32(), 0x4e584e43);
        assert_eq!(Command::CNXN.magic(), !0x4e584e43);
        assert_eq!(Command::from_u32(0x12345678), None);
    }

    #[test]
    fn test_message_creation() {
        let msg = AdbMessage::cnxn(0x01000000, 256 * 1024);
        assert_eq!(msg.command, Command::CNXN);
        assert_eq!(msg.arg0, 0x01000000);
        assert_eq!(msg.arg1, 256 * 1024);
        assert_eq!(msg.data_length, 0);
        assert_eq!(msg.magic, Command::CNXN.magic());
        assert!(msg.is_valid());
    }

    #[test]
    fn test_message_with_payload() {
        let payload = Bytes::from("host:devices");
        let msg = AdbMessage::open(1001, "host:devices");

        assert_eq!(msg.command, Command::OPEN);
        assert_eq!(msg.arg0, 1001);
        assert_eq!(msg.data_length, payload.len() as u32);
        assert_eq!(msg.payload, payload);
        assert!(msg.is_valid());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let original = AdbMessage::open(1001, "host:devices");
        let serialized = original.serialize().expect("Failed to serialize");

        // Split header and payload
        let header = &serialized[..24];
        let payload = Bytes::from(serialized[24..].to_vec());

        let deserialized = AdbMessage::deserialize(header, payload).expect("Failed to deserialize");

        assert_eq!(original.command, deserialized.command);
        assert_eq!(original.arg0, deserialized.arg0);
        assert_eq!(original.arg1, deserialized.arg1);
        assert_eq!(original.data_length, deserialized.data_length);
        assert_eq!(original.data_checksum, deserialized.data_checksum);
        assert_eq!(original.magic, deserialized.magic);
        assert_eq!(original.payload, deserialized.payload);
    }

    #[test]
    fn test_invalid_magic() {
        let mut buffer = vec![0u8; 24];
        let mut cursor = Cursor::new(&mut buffer);

        cursor.write_u32::<LittleEndian>(Command::CNXN.to_u32()).unwrap();
        cursor.write_u32::<LittleEndian>(0).unwrap();
        cursor.write_u32::<LittleEndian>(0).unwrap();
        cursor.write_u32::<LittleEndian>(0).unwrap();
        cursor.write_u32::<LittleEndian>(0).unwrap();
        cursor.write_u32::<LittleEndian>(0x12345678).unwrap(); // Invalid magic

        let result = AdbMessage::deserialize(&buffer, Bytes::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_checksum_validation() {
        let payload = Bytes::from("test data");
        let msg = AdbMessage::wrte(1001, 1002, payload.clone());

        // Verify correct checksum
        assert_eq!(msg.data_checksum, crc32fast::hash(&payload));
        assert!(msg.is_valid());

        // Test with wrong checksum
        let mut invalid_msg = msg.clone();
        invalid_msg.data_checksum = 0x12345678;
        assert!(!invalid_msg.is_valid());
    }

    #[test]
    fn test_ping_pong_messages() {
        // Test PING message
        let ping_msg = AdbMessage::ping();
        assert_eq!(ping_msg.command, Command::PING);
        assert_eq!(ping_msg.arg0, 0);
        assert_eq!(ping_msg.arg1, 0);
        assert_eq!(ping_msg.data_length, 0);
        assert!(ping_msg.is_valid());

        // Test PONG message
        let pong_msg = AdbMessage::pong();
        assert_eq!(pong_msg.command, Command::PONG);
        assert_eq!(pong_msg.arg0, 0);
        assert_eq!(pong_msg.arg1, 0);
        assert_eq!(pong_msg.data_length, 0);
        assert!(pong_msg.is_valid());
    }
}
