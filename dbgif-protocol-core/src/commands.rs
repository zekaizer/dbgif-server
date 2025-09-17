#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    CNXN = 0x4E584E43,  // "CNXN"
    OKAY = 0x59414B4F,  // "OKAY"
    OPEN = 0x4E45504F,  // "OPEN"
    WRTE = 0x45545257,  // "WRTE"
    CLSE = 0x45534C43,  // "CLSE"
    PING = 0x474E4950,  // "PING"
    PONG = 0x474E4F50,  // "PONG"
}

impl Command {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x4E584E43 => Some(Command::CNXN),
            0x59414B4F => Some(Command::OKAY),
            0x4E45504F => Some(Command::OPEN),
            0x45545257 => Some(Command::WRTE),
            0x45534C43 => Some(Command::CLSE),
            0x474E4950 => Some(Command::PING),
            0x474E4F50 => Some(Command::PONG),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> u32 {
        *self as u32
    }

    pub fn magic(&self) -> u32 {
        !self.as_u32()
    }

    pub fn carries_data(&self) -> bool {
        matches!(self, Command::CNXN | Command::OPEN | Command::WRTE)
    }
}