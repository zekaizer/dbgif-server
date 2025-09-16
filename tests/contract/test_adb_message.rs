#[cfg(test)]
mod tests {
    use dbgif_protocol::protocol::message::AdbMessage;
    use dbgif_protocol::protocol::commands::AdbCommand;

    #[test]
    fn test_adb_message_header_size() {
        // ADB message header must be exactly 24 bytes
        assert_eq!(std::mem::size_of::<AdbMessage>(), 24);
    }

    #[test]
    fn test_adb_message_serialization() {
        let msg = AdbMessage {
            command: AdbCommand::CNXN as u32,
            arg0: 0x01000000,
            arg1: 0x1000,
            data_length: 4,
            data_crc32: 0x12345678,
            magic: !(AdbCommand::CNXN as u32),
            data: b"test".to_vec(),
        };

        let serialized = msg.serialize();

        // Header should be 24 bytes + 4 bytes data
        assert_eq!(serialized.len(), 28);

        // Test little-endian encoding
        assert_eq!(&serialized[0..4], &(AdbCommand::CNXN as u32).to_le_bytes());
        assert_eq!(&serialized[4..8], &0x01000000u32.to_le_bytes());
        assert_eq!(&serialized[8..12], &0x1000u32.to_le_bytes());
        assert_eq!(&serialized[12..16], &4u32.to_le_bytes());
        assert_eq!(&serialized[16..20], &0x12345678u32.to_le_bytes());
        assert_eq!(&serialized[20..24], &(!(AdbCommand::CNXN as u32)).to_le_bytes());
        assert_eq!(&serialized[24..], b"test");
    }

    #[test]
    fn test_adb_message_deserialization() {
        let mut data = Vec::new();
        data.extend_from_slice(&(AdbCommand::OKAY as u32).to_le_bytes());
        data.extend_from_slice(&0x12u32.to_le_bytes());
        data.extend_from_slice(&0x34u32.to_le_bytes());
        data.extend_from_slice(&6u32.to_le_bytes());
        data.extend_from_slice(&0xABCDEF00u32.to_le_bytes());
        data.extend_from_slice(&(!(AdbCommand::OKAY as u32)).to_le_bytes());
        data.extend_from_slice(b"hello!");

        let msg = AdbMessage::deserialize(&data).unwrap();

        assert_eq!(msg.command, AdbCommand::OKAY as u32);
        assert_eq!(msg.arg0, 0x12);
        assert_eq!(msg.arg1, 0x34);
        assert_eq!(msg.data_length, 6);
        assert_eq!(msg.data_crc32, 0xABCDEF00);
        assert_eq!(msg.magic, !(AdbCommand::OKAY as u32));
        assert_eq!(msg.data, b"hello!");
    }

    #[test]
    fn test_adb_message_magic_validation() {
        let msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: 1,
            arg1: 2,
            data_length: 0,
            data_crc32: 0,
            magic: !(AdbCommand::WRTE as u32), // Correct magic
            data: vec![],
        };

        assert!(msg.is_valid_magic());

        let invalid_msg = AdbMessage {
            command: AdbCommand::WRTE as u32,
            arg0: 1,
            arg1: 2,
            data_length: 0,
            data_crc32: 0,
            magic: 0x12345678, // Wrong magic
            data: vec![],
        };

        assert!(!invalid_msg.is_valid_magic());
    }

    #[test]
    fn test_cnxn_message_format() {
        // Test CNXN message specific format
        let cnxn_msg = AdbMessage::new_cnxn(
            0x01000000, // version
            0x100000,   // maxdata
            b"device::dbgif-test".to_vec()
        );

        assert_eq!(cnxn_msg.command, AdbCommand::CNXN as u32);
        assert_eq!(cnxn_msg.arg0, 0x01000000);
        assert_eq!(cnxn_msg.arg1, 0x100000);
        assert_eq!(cnxn_msg.data, b"device::dbgif-test");
        assert!(cnxn_msg.is_valid_magic());
    }
}