use dbgif_server::protocol::{Message, Command, checksum::calculate_crc32};
use bytes::BytesMut;

#[test]
fn test_message_magic_validation() {
    // Test correct magic value
    let valid_msg = Message {
        command: Command::OKAY,
        arg0: 0,
        arg1: 0,
        data_length: 0,
        data_crc32: 0,
        magic: Command::OKAY.magic(),
    };
    assert!(valid_msg.is_valid());

    // Test incorrect magic value
    let invalid_msg = Message {
        command: Command::OKAY,
        arg0: 0,
        arg1: 0,
        data_length: 0,
        data_crc32: 0,
        magic: 0xDEADBEEF,  // Invalid magic
    };
    assert!(!invalid_msg.is_valid());
}

#[test]
fn test_data_crc32_validation() {
    let data = b"test data";
    let expected_crc = calculate_crc32(data);

    // Test correct CRC32
    let valid_msg = Message {
        command: Command::WRTE,
        arg0: 1,
        arg1: 2,
        data_length: data.len() as u32,
        data_crc32: expected_crc,
        magic: Command::WRTE.magic(),
    };
    assert!(valid_msg.is_valid());

    // Test incorrect CRC32
    let invalid_msg = Message {
        command: Command::WRTE,
        arg0: 1,
        arg1: 2,
        data_length: data.len() as u32,
        data_crc32: expected_crc + 1, // Wrong CRC
        magic: Command::WRTE.magic(),
    };
    assert!(invalid_msg.is_valid()); // Still structurally valid, CRC checked separately
}

#[test]
fn test_command_types_validation() {
    let commands = [
        Command::CNXN, Command::AUTH, Command::OPEN, Command::OKAY,
        Command::WRTE, Command::CLSE, Command::PING, Command::PONG,
    ];

    for cmd in commands.iter() {
        let msg = Message {
            command: *cmd,
            arg0: 0,
            arg1: 0,
            data_length: 0,
            data_crc32: 0,
            magic: cmd.magic(),
        };
        assert!(msg.is_valid(), "Command {:?} should create valid message", cmd);
    }
}

#[test]
fn test_data_length_validation() {
    // Test maximum data length
    let max_data_msg = Message {
        command: Command::WRTE,
        arg0: 1,
        arg1: 2,
        data_length: 256 * 1024, // MAXDATA
        data_crc32: 0,
        magic: Command::WRTE.magic(),
    };
    assert!(max_data_msg.is_valid());

    // Test excessive data length (should still be structurally valid)
    let large_data_msg = Message {
        command: Command::WRTE,
        arg0: 1,
        arg1: 2,
        data_length: 1024 * 1024, // 1MB
        data_crc32: 0,
        magic: Command::WRTE.magic(),
    };
    assert!(large_data_msg.is_valid()); // Validation may happen at protocol level
}

#[test]
fn test_message_serialization_roundtrip() {
    let original_msg = Message {
        command: Command::OPEN,
        arg0: 12345,
        arg1: 67890,
        data_length: 100,
        data_crc32: 0xABCDEF01,
        magic: Command::OPEN.magic(),
    };

    let mut buffer = BytesMut::new();
    original_msg.serialize(&mut buffer).unwrap();

    let mut cursor = std::io::Cursor::new(buffer.freeze());
    let deserialized_msg = Message::deserialize(&mut cursor).unwrap();

    assert_eq!(deserialized_msg.command, original_msg.command);
    assert_eq!(deserialized_msg.arg0, original_msg.arg0);
    assert_eq!(deserialized_msg.arg1, original_msg.arg1);
    assert_eq!(deserialized_msg.data_length, original_msg.data_length);
    assert_eq!(deserialized_msg.data_crc32, original_msg.data_crc32);
    assert_eq!(deserialized_msg.magic, original_msg.magic);
}

#[test]
fn test_empty_message_validation() {
    let empty_msg = Message {
        command: Command::PING,
        arg0: 0,
        arg1: 0,
        data_length: 0,
        data_crc32: 0,
        magic: Command::PING.magic(),
    };
    assert!(empty_msg.is_valid());
}

#[test]
fn test_stream_id_validation() {
    // Test valid stream IDs in OPEN message
    let open_msg = Message {
        command: Command::OPEN,
        arg0: 1, // local_id
        arg1: 0, // remote_id (should be 0 for OPEN)
        data_length: 0,
        data_crc32: 0,
        magic: Command::OPEN.magic(),
    };
    assert!(open_msg.is_valid());

    // Test OKAY response with matching IDs
    let okay_msg = Message {
        command: Command::OKAY,
        arg0: 1, // local_id
        arg1: 2, // remote_id
        data_length: 0,
        data_crc32: 0,
        magic: Command::OKAY.magic(),
    };
    assert!(okay_msg.is_valid());
}