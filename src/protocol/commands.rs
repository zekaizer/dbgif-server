#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdbCommand {
    /// Connection establishment message
    CNXN = 0x4E584E43,  // "CNXN" - ASCII representation
    /// Success acknowledgment message
    OKAY = 0x59414B4F,  // "OKAY" - ASCII representation
    /// Stream opening message
    OPEN = 0x4E45504F,  // "OPEN" - ASCII representation
    /// Data transfer message
    WRTE = 0x45545257,  // "WRTE" - ASCII representation
    /// Stream close message
    CLSE = 0x45534C43,  // "CLSE" - ASCII representation
    /// Connection health check request
    PING = 0x474E4950,  // "PING" - ASCII representation
    /// Connection health check response
    PONG = 0x474E4F50,  // "PONG" - ASCII representation
}

impl AdbCommand {
    /// Convert u32 value to AdbCommand enum
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x4E584E43 => Some(AdbCommand::CNXN),
            0x59414B4F => Some(AdbCommand::OKAY),
            0x4E45504F => Some(AdbCommand::OPEN),
            0x45545257 => Some(AdbCommand::WRTE),
            0x45534C43 => Some(AdbCommand::CLSE),
            0x474E4950 => Some(AdbCommand::PING),
            0x474E4F50 => Some(AdbCommand::PONG),
            _ => None,
        }
    }

    /// Get ASCII string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            AdbCommand::CNXN => "CNXN",
            AdbCommand::OKAY => "OKAY",
            AdbCommand::OPEN => "OPEN",
            AdbCommand::WRTE => "WRTE",
            AdbCommand::CLSE => "CLSE",
            AdbCommand::PING => "PING",
            AdbCommand::PONG => "PONG",
        }
    }

    /// Get ASCII bytes representation (4 bytes)
    pub fn as_bytes(&self) -> [u8; 4] {
        (*self as u32).to_le_bytes()
    }

    /// Check if command expects a response
    pub fn expects_response(&self) -> bool {
        match self {
            AdbCommand::CNXN => true,   // Expects CNXN response
            AdbCommand::OPEN => true,   // Expects OKAY or CLSE
            AdbCommand::WRTE => true,   // Expects OKAY
            AdbCommand::PING => true,   // Expects PONG
            AdbCommand::OKAY => false,  // Response message
            AdbCommand::CLSE => false,  // May trigger CLSE response but not required
            AdbCommand::PONG => false,  // Response message
        }
    }

    /// Check if command carries data payload
    pub fn carries_data(&self) -> bool {
        match self {
            AdbCommand::CNXN => true,   // System identity string
            AdbCommand::OPEN => true,   // Service name
            AdbCommand::WRTE => true,   // Data payload
            AdbCommand::OKAY => false,  // No data
            AdbCommand::CLSE => false,  // No data
            AdbCommand::PING => false,  // No data
            AdbCommand::PONG => false,  // No data
        }
    }

    /// Check if command is connection-level (vs stream-level)
    pub fn is_connection_level(&self) -> bool {
        match self {
            AdbCommand::CNXN => true,   // Connection establishment
            AdbCommand::PING => true,   // Connection health check
            AdbCommand::PONG => true,   // Connection health response
            AdbCommand::OKAY => false,  // Can be both connection and stream level
            AdbCommand::OPEN => false,  // Stream level
            AdbCommand::WRTE => false,  // Stream level
            AdbCommand::CLSE => false,  // Stream level
        }
    }

    /// Get the expected magic number (bitwise NOT of command)
    pub fn magic(&self) -> u32 {
        !(*self as u32)
    }

    /// Validate if given magic matches this command
    pub fn is_valid_magic(&self, magic: u32) -> bool {
        magic == self.magic()
    }

    /// Get all valid command values
    pub fn all_commands() -> [AdbCommand; 7] {
        [
            AdbCommand::CNXN,
            AdbCommand::OKAY,
            AdbCommand::OPEN,
            AdbCommand::WRTE,
            AdbCommand::CLSE,
            AdbCommand::PING,
            AdbCommand::PONG,
        ]
    }

    /// Check if command is valid in context
    pub fn is_valid_in_context(&self, is_connected: bool, _has_streams: bool) -> bool {
        match self {
            AdbCommand::CNXN => !is_connected, // Only valid when not connected
            AdbCommand::PING | AdbCommand::PONG => is_connected, // Only valid when connected
            AdbCommand::OPEN => is_connected, // Only valid when connected
            AdbCommand::OKAY | AdbCommand::WRTE | AdbCommand::CLSE => {
                is_connected // Valid when connected (stream commands)
            }
        }
    }
}

impl std::fmt::Display for AdbCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<AdbCommand> for u32 {
    fn from(cmd: AdbCommand) -> Self {
        cmd as u32
    }
}

impl TryFrom<u32> for AdbCommand {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        AdbCommand::from_u32(value).ok_or(())
    }
}