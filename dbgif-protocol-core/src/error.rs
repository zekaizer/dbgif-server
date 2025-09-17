#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DecodeError {
    InvalidMagic = 1,
    InvalidCommand = 2,
    CrcMismatch = 3,
    BufferTooSmall = 4,
    InvalidState = 5,
}

impl DecodeError {
    pub fn as_code(&self) -> i32 {
        *self as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StateError {
    NotConnected = 10,
    AlreadyConnected = 11,
    InvalidTransition = 12,
    StreamNotFound = 13,
    StreamAlreadyActive = 14,
    TooManyStreams = 15,
}

impl StateError {
    pub fn as_code(&self) -> i32 {
        *self as i32
    }
}