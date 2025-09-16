#[allow(unused_imports)]
use crate::protocol::commands::AdbCommand;
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
    pub fn serialize(&self) -> Vec<u8> {
        // TODO: Implement serialization
        unimplemented!()
    }

    #[allow(unused_variables)]
    pub fn deserialize(_data: &[u8]) -> Result<Self, MessageError> {
        // TODO: Implement deserialization
        unimplemented!()
    }

    pub fn is_valid_magic(&self) -> bool {
        // TODO: Implement magic validation
        unimplemented!()
    }

    #[allow(unused_variables)]
    pub fn new_cnxn(_version: u32, _maxdata: u32, _system_identity: Vec<u8>) -> Self {
        // TODO: Implement CNXN message creation
        unimplemented!()
    }
}