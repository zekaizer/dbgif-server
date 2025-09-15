use dbgif_server::protocol::{Message, Command};
use bytes::BytesMut;
use std::io::Cursor;

#[tokio::test]
async fn test_cnxn_handshake_message_format() {
    // Test CNXN message creation and validation
    let cnxn_msg = Message {
        command: Command::CNXN,
        arg0: 0x01000000, // VERSION
        arg1: 256 * 1024,  // MAXDATA
        data_length: 0,
        data_crc32: 0,
        magic: Command::CNXN.magic(),
    };

    // Verify magic value calculation
    assert_eq!(cnxn_msg.magic, 0xb3bcb4bc);
    assert_eq!(cnxn_msg.command as u32, 0x4e584e43);
}

#[tokio::test]
async fn test_cnxn_response_validation() {
    // Test proper CNXN response format
    let response_msg = Message {
        command: Command::CNXN,
        arg0: 0x01000000,
        arg1: 256 * 1024,
        data_length: 0,
        data_crc32: 0,
        magic: Command::CNXN.magic(),
    };

    // Message should be valid
    assert!(response_msg.is_valid());
    assert_eq!(response_msg.data_length, 0);
}

#[tokio::test]
async fn test_auth_token_message() {
    // Test AUTH message format
    let auth_msg = Message {
        command: Command::AUTH,
        arg0: 1, // AUTH_TOKEN
        arg1: 0,
        data_length: 20, // SHA1 size
        data_crc32: 0,
        magic: Command::AUTH.magic(),
    };

    assert_eq!(auth_msg.command as u32, 0x48545541);
    assert_eq!(auth_msg.magic, 0xb7b8babe);
}

#[tokio::test]
async fn test_auth_signature_message() {
    // Test AUTH signature response
    let auth_sig_msg = Message {
        command: Command::AUTH,
        arg0: 2, // AUTH_SIGNATURE
        arg1: 0,
        data_length: 256, // RSA signature size
        data_crc32: 0,
        magic: Command::AUTH.magic(),
    };

    assert_eq!(auth_sig_msg.arg0, 2);
    assert_eq!(auth_sig_msg.data_length, 256);
}

#[tokio::test]
async fn test_handshake_sequence_validation() {
    // Test complete handshake sequence
    let messages = vec![
        // 1. Client sends CNXN
        Message {
            command: Command::CNXN,
            arg0: 0x01000000,
            arg1: 256 * 1024,
            data_length: 0,
            data_crc32: 0,
            magic: Command::CNXN.magic(),
        },
        // 2. Server responds with AUTH (simplified)
        Message {
            command: Command::CNXN,
            arg0: 0x01000000,
            arg1: 256 * 1024,
            data_length: 0,
            data_crc32: 0,
            magic: Command::CNXN.magic(),
        },
    ];

    for msg in messages {
        assert!(msg.is_valid(), "All handshake messages should be valid");
    }
}

#[tokio::test]
async fn test_message_serialization() {
    let msg = Message {
        command: Command::CNXN,
        arg0: 0x01000000,
        arg1: 256 * 1024,
        data_length: 0,
        data_crc32: 0,
        magic: Command::CNXN.magic(),
    };

    let mut buffer = BytesMut::new();
    let result = msg.serialize(&mut buffer);

    assert!(result.is_ok(), "Message serialization should succeed");
    assert_eq!(buffer.len(), 24, "Serialized message should be 24 bytes");
}

#[tokio::test]
async fn test_message_deserialization() {
    let msg = Message {
        command: Command::CNXN,
        arg0: 0x01000000,
        arg1: 256 * 1024,
        data_length: 0,
        data_crc32: 0,
        magic: Command::CNXN.magic(),
    };

    let mut buffer = BytesMut::new();
    msg.serialize(&mut buffer).unwrap();

    let mut cursor = Cursor::new(buffer.freeze());
    let deserialized = Message::deserialize(&mut cursor);

    assert!(deserialized.is_ok(), "Message deserialization should succeed");
    let deserialized_msg = deserialized.unwrap();
    assert_eq!(deserialized_msg.command, msg.command);
    assert_eq!(deserialized_msg.arg0, msg.arg0);
}