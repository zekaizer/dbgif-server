use bytes::{Buf, BufMut, Bytes, BytesMut};
use anyhow::{Result, bail};
use super::constants::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Connect,
    Open,
    Okay,
    Write,
    Close,
    Auth,
    Ping,
    Pong,
}

impl Command {
    pub fn to_u32(&self) -> u32 {
        match self {
            Command::Connect => CNXN,
            Command::Open => OPEN,
            Command::Okay => OKAY,
            Command::Write => WRTE,
            Command::Close => CLSE,
            Command::Auth => AUTH,
            Command::Ping => PING,
            Command::Pong => PONG,
        }
    }

    pub fn from_u32(value: u32) -> Result<Self> {
        match value {
            CNXN => Ok(Command::Connect),
            OPEN => Ok(Command::Open),
            OKAY => Ok(Command::Okay),
            WRTE => Ok(Command::Write),
            CLSE => Ok(Command::Close),
            AUTH => Ok(Command::Auth),
            PING => Ok(Command::Ping),
            PONG => Ok(Command::Pong),
            _ => bail!("Unknown command: 0x{:08x}", value),
        }
    }

    pub fn magic(&self) -> u32 {
        match self {
            Command::Connect => CNXN_MAGIC,
            Command::Open => OPEN_MAGIC,
            Command::Okay => OKAY_MAGIC,
            Command::Write => WRTE_MAGIC,
            Command::Close => CLSE_MAGIC,
            Command::Auth => AUTH_MAGIC,
            Command::Ping => PING_MAGIC,
            Command::Pong => PONG_MAGIC,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub command: Command,
    pub arg0: u32,
    pub arg1: u32,
    pub data: Bytes,
}

impl Message {
    pub fn new(command: Command, arg0: u32, arg1: u32, data: impl Into<Bytes>) -> Self {
        Self {
            command,
            arg0,
            arg1,
            data: data.into(),
        }
    }

    pub fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(24 + self.data.len());
        
        // Header (24 bytes)
        buf.put_u32_le(self.command.to_u32());
        buf.put_u32_le(self.arg0);
        buf.put_u32_le(self.arg1);
        buf.put_u32_le(self.data.len() as u32);
        buf.put_u32_le(self.checksum());
        buf.put_u32_le(self.command.magic());
        
        // Data payload
        buf.extend_from_slice(&self.data);
        
        buf.freeze()
    }

    pub fn deserialize(mut data: impl Buf) -> Result<Self> {
        if data.remaining() < 24 {
            bail!("Message too short: {} bytes", data.remaining());
        }

        let command_raw = data.get_u32_le();
        let arg0 = data.get_u32_le();
        let arg1 = data.get_u32_le();
        let data_length = data.get_u32_le();
        let data_checksum = data.get_u32_le();
        let magic = data.get_u32_le();

        let command = Command::from_u32(command_raw)?;
        
        // Verify magic value
        if magic != command.magic() {
            bail!("Invalid magic value: expected 0x{:08x}, got 0x{:08x}", 
                  command.magic(), magic);
        }

        // Read payload data
        if data.remaining() < data_length as usize {
            bail!("Insufficient data: expected {} bytes, got {}", 
                  data_length, data.remaining());
        }

        let mut payload = vec![0u8; data_length as usize];
        data.copy_to_slice(&mut payload);
        let payload = Bytes::from(payload);

        let message = Message {
            command,
            arg0,
            arg1,
            data: payload,
        };

        // Verify checksum
        if message.checksum() != data_checksum {
            bail!("Checksum mismatch: expected 0x{:08x}, got 0x{:08x}", 
                  message.checksum(), data_checksum);
        }

        Ok(message)
    }

    fn checksum(&self) -> u32 {
        super::checksum::calculate(&self.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_conversion() {
        assert_eq!(Command::Connect.to_u32(), CNXN);
        assert_eq!(Command::from_u32(CNXN).unwrap(), Command::Connect);
    }

    #[test]
    fn test_magic_values() {
        assert_eq!(Command::Connect.magic(), !CNXN);
        assert_eq!(Command::Open.magic(), !OPEN);
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message::new(Command::Connect, 0x01000000, 0x00040000, &b"test"[..]);
        let serialized = msg.serialize();
        
        assert_eq!(serialized.len(), 24 + 4); // header + data
        
        let deserialized = Message::deserialize(&serialized[..]).unwrap();
        assert_eq!(deserialized.command, Command::Connect);
        assert_eq!(deserialized.arg0, 0x01000000);
        assert_eq!(deserialized.arg1, 0x00040000);
        assert_eq!(deserialized.data, Bytes::from(&b"test"[..]));
    }

    #[test]
    fn test_empty_message() {
        let msg = Message::new(Command::Okay, 1, 2, Bytes::new());
        let serialized = msg.serialize();
        let deserialized = Message::deserialize(&serialized[..]).unwrap();
        
        assert_eq!(deserialized.command, Command::Okay);
        assert_eq!(deserialized.data.len(), 0);
    }

    #[test]
    fn test_invalid_command() {
        let result = Command::from_u32(0xdeadbeef);
        assert!(result.is_err());
    }

    #[test]
    fn test_ping_pong_commands() {
        // Test PING command
        assert_eq!(Command::Ping.to_u32(), PING);
        assert_eq!(Command::from_u32(PING).unwrap(), Command::Ping);
        assert_eq!(Command::Ping.magic(), PING_MAGIC);

        // Test PONG command
        assert_eq!(Command::Pong.to_u32(), PONG);
        assert_eq!(Command::from_u32(PONG).unwrap(), Command::Pong);
        assert_eq!(Command::Pong.magic(), PONG_MAGIC);
    }

    #[test]
    fn test_ping_pong_messages() {
        // Test PING message
        let ping_msg = Message::new(Command::Ping, 0, 0, Bytes::new());
        let serialized = ping_msg.serialize();
        let deserialized = Message::deserialize(&serialized[..]).unwrap();
        assert_eq!(deserialized.command, Command::Ping);

        // Test PONG message
        let pong_msg = Message::new(Command::Pong, 1, 2, &b"keepalive"[..]);
        let serialized = pong_msg.serialize();
        let deserialized = Message::deserialize(&serialized[..]).unwrap();
        assert_eq!(deserialized.command, Command::Pong);
        assert_eq!(deserialized.data, Bytes::from(&b"keepalive"[..]));
    }
}