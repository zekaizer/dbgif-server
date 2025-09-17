use crate::commands::Command;
use crate::error::DecodeError;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MessageHeader {
    pub command: u32,
    pub arg0: u32,
    pub arg1: u32,
    pub data_length: u32,
    pub data_crc32: u32,
    pub magic: u32,
}

// Builder for creating messages
pub struct MessageBuilder {
    header: MessageHeader,
}

impl MessageBuilder {
    pub const fn new() -> Self {
        Self {
            header: MessageHeader::new(),
        }
    }

    pub fn command(mut self, cmd: crate::commands::Command) -> Self {
        self.header.command = cmd.as_u32();
        self.header.magic = cmd.magic();
        self
    }

    pub fn arg0(mut self, value: u32) -> Self {
        self.header.arg0 = value;
        self
    }

    pub fn arg1(mut self, value: u32) -> Self {
        self.header.arg1 = value;
        self
    }

    pub fn data_info(mut self, length: u32, crc32: u32) -> Self {
        self.header.data_length = length;
        self.header.data_crc32 = crc32;
        self
    }

    pub fn build(self) -> MessageHeader {
        self.header
    }
}

impl MessageHeader {
    pub const SIZE: usize = 24;

    pub const fn new() -> Self {
        Self {
            command: 0,
            arg0: 0,
            arg1: 0,
            data_length: 0,
            data_crc32: 0,
            magic: 0,
        }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..4].copy_from_slice(&self.command.to_le_bytes());
        buffer[4..8].copy_from_slice(&self.arg0.to_le_bytes());
        buffer[8..12].copy_from_slice(&self.arg1.to_le_bytes());
        buffer[12..16].copy_from_slice(&self.data_length.to_le_bytes());
        buffer[16..20].copy_from_slice(&self.data_crc32.to_le_bytes());
        buffer[20..24].copy_from_slice(&self.magic.to_le_bytes());
    }

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Result<Self, DecodeError> {
        let command = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let arg0 = u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        let arg1 = u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);
        let data_length = u32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]);
        let data_crc32 = u32::from_le_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
        let magic = u32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);

        // Validate magic
        if magic != !command {
            return Err(DecodeError::InvalidMagic);
        }

        // Validate command
        if Command::from_u32(command).is_none() {
            return Err(DecodeError::InvalidCommand);
        }

        Ok(Self {
            command,
            arg0,
            arg1,
            data_length,
            data_crc32,
            magic,
        })
    }

    pub fn is_valid_magic(&self) -> bool {
        self.magic == !self.command
    }

    pub fn get_command(&self) -> Option<Command> {
        Command::from_u32(self.command)
    }
}