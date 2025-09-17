use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AsciiProtocolError {
    #[error("Invalid message format")]
    InvalidFormat,
    #[error("Invalid length field")]
    InvalidLength,
    #[error("Invalid STRM format")]
    InvalidStrmFormat,
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, AsciiProtocolError>;

pub fn encode_request(command: &str) -> Vec<u8> {
    let command_bytes = command.as_bytes();
    let length = command_bytes.len();
    let hex_length = format!("{:04x}", length);

    let mut result = Vec::with_capacity(4 + length);
    result.extend_from_slice(hex_length.as_bytes());
    result.extend_from_slice(command_bytes);
    result
}

pub fn decode_response(data: &[u8]) -> Result<(bool, Vec<u8>)> {
    if data.len() < 8 {
        return Err(AsciiProtocolError::InvalidFormat);
    }

    let status = &data[0..4];
    let success = match status {
        b"OKAY" => true,
        b"FAIL" => false,
        _ => return Err(AsciiProtocolError::InvalidFormat),
    };

    let length_str = std::str::from_utf8(&data[4..8])
        .map_err(|_| AsciiProtocolError::InvalidLength)?;
    let length = u32::from_str_radix(length_str, 16)
        .map_err(|_| AsciiProtocolError::InvalidLength)? as usize;

    if data.len() < 8 + length {
        return Err(AsciiProtocolError::InvalidFormat);
    }

    let payload = data[8..8 + length].to_vec();
    Ok((success, payload))
}

pub fn encode_strm(stream_id: u8, data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(12 + data.len());

    result.extend_from_slice(b"STRM");

    // Stream ID (2 hex bytes)
    let hex_stream_id = format!("{:02x}", stream_id);
    result.extend_from_slice(hex_stream_id.as_bytes());

    // Length (6 hex bytes)
    let hex_length = format!("{:06x}", data.len());
    result.extend_from_slice(hex_length.as_bytes());

    result.extend_from_slice(data);

    result
}

pub fn decode_strm(data: &[u8]) -> Result<(u8, Vec<u8>)> {
    if data.len() < 12 {
        return Err(AsciiProtocolError::InvalidStrmFormat);
    }

    let header = &data[0..4];
    if header != b"STRM" {
        return Err(AsciiProtocolError::InvalidStrmFormat);
    }

    // Stream ID (2 hex bytes)
    let stream_id_str = std::str::from_utf8(&data[4..6])
        .map_err(|_| AsciiProtocolError::InvalidStrmFormat)?;
    let stream_id = u8::from_str_radix(stream_id_str, 16)
        .map_err(|_| AsciiProtocolError::InvalidStrmFormat)?;

    // Length (6 hex bytes)
    let length_str = std::str::from_utf8(&data[6..12])
        .map_err(|_| AsciiProtocolError::InvalidLength)?;
    let length = u32::from_str_radix(length_str, 16)
        .map_err(|_| AsciiProtocolError::InvalidLength)? as usize;

    if data.len() < 12 + length {
        return Err(AsciiProtocolError::InvalidStrmFormat);
    }

    let payload = data[12..12 + length].to_vec();

    Ok((stream_id, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_request() {
        let cmd = "host:version";
        let encoded = encode_request(cmd);
        assert_eq!(&encoded[0..4], b"000c");
        assert_eq!(&encoded[4..], b"host:version");
    }

    #[test]
    fn test_decode_response_okay() {
        let data = b"OKAY0005hello";
        let (success, payload) = decode_response(data).unwrap();
        assert!(success);
        assert_eq!(payload, b"hello");
    }

    #[test]
    fn test_decode_response_fail() {
        let data = b"FAIL0005error";
        let (success, payload) = decode_response(data).unwrap();
        assert!(!success);
        assert_eq!(payload, b"error");
    }

    #[test]
    fn test_encode_strm() {
        let stream_id = 0x42;
        let data = b"test data";
        let encoded = encode_strm(stream_id, data);

        assert_eq!(&encoded[0..4], b"STRM");
        assert_eq!(&encoded[4..6], b"42"); // hex stream_id
        assert_eq!(&encoded[6..12], b"000009"); // 9 bytes data = 0x000009
        assert_eq!(&encoded[12..], b"test data");
    }

    #[test]
    fn test_decode_strm() {
        let data = b"STRM42000009test data"; // STRM + stream_id(2) + length(6) + data
        let (stream_id, payload) = decode_strm(data).unwrap();
        assert_eq!(stream_id, 0x42);
        assert_eq!(payload, b"test data");
    }
}