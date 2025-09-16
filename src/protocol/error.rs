use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("Invalid message format")]
    InvalidMessage,
    #[error("CRC validation failed")]
    CrcValidationFailed,
    #[error("Unknown command: {0}")]
    UnknownCommand(u32),
    #[error("Connection error: {0}")]
    ConnectionError(String),
}