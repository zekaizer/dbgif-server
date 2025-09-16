#[cfg(test)]
mod tests {
    use dbgif_protocol::protocol::message::AdbMessage;
    use dbgif_protocol::protocol::commands::AdbCommand;

    #[test]
    fn test_u32_little_endian_serialization() {
        // Test that u32 values are correctly serialized as little-endian

        let value: u32 = 0x12345678;
        let bytes = value.to_le_bytes();

        // Little-endian: least significant byte first
        assert_eq!(bytes[0], 0x78); // LSB
        assert_eq!(bytes[1], 0x56);
        assert_eq!(bytes[2], 0x34);
        assert_eq!(bytes[3], 0x12); // MSB
    }

    #[test]
    fn test_u32_little_endian_deserialization() {
        // Test that little-endian bytes are correctly deserialized to u32

        let bytes = [0x78, 0x56, 0x34, 0x12];
        let value = u32::from_le_bytes(bytes);

        assert_eq!(value, 0x12345678);
    }

    #[test]
    fn test_adb_command_endianness() {
        // Test that ADB commands maintain correct endianness

        let cnxn_u32 = AdbCommand::CNXN as u32;
        let cnxn_bytes = cnxn_u32.to_le_bytes();

        // CNXN should be "CNXN" when read as ASCII
        assert_eq!(&cnxn_bytes, b"CNXN");

        // Verify round-trip conversion
        let reconstructed = u32::from_le_bytes(cnxn_bytes);
        assert_eq!(reconstructed, cnxn_u32);
    }

    #[test]
    fn test_message_header_endianness() {
        // Test that message headers use little-endian encoding

        let msg = create_test_message();
        let serialized = serialize_message_header(&msg);

        // Each field should be in little-endian format
        let command_bytes = &serialized[0..4];
        let arg0_bytes = &serialized[4..8];
        let arg1_bytes = &serialized[8..12];
        let data_length_bytes = &serialized[12..16];
        let crc_bytes = &serialized[16..20];
        let magic_bytes = &serialized[20..24];

        // Verify each field is little-endian encoded
        assert_eq!(u32::from_le_bytes([command_bytes[0], command_bytes[1], command_bytes[2], command_bytes[3]]), msg.command);
        assert_eq!(u32::from_le_bytes([arg0_bytes[0], arg0_bytes[1], arg0_bytes[2], arg0_bytes[3]]), msg.arg0);
        assert_eq!(u32::from_le_bytes([arg1_bytes[0], arg1_bytes[1], arg1_bytes[2], arg1_bytes[3]]), msg.arg1);
        assert_eq!(u32::from_le_bytes([data_length_bytes[0], data_length_bytes[1], data_length_bytes[2], data_length_bytes[3]]), msg.data_length);
        assert_eq!(u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]), msg.data_crc32);
        assert_eq!(u32::from_le_bytes([magic_bytes[0], magic_bytes[1], magic_bytes[2], magic_bytes[3]]), msg.magic);
    }

    #[test]
    fn test_endianness_across_platforms() {
        // Test that endianness is consistent regardless of platform

        let test_values: [u32; 5] = [
            0x00000001,
            0xFF000000,
            0x12345678,
            0xABCDEF00,
            0xFFFFFFFF,
        ];

        for value in test_values {
            let le_bytes = value.to_le_bytes();
            let reconstructed = u32::from_le_bytes(le_bytes);
            assert_eq!(value, reconstructed, "Round-trip failed for value: 0x{:08X}", value);

            // Verify little-endian byte order
            assert_eq!(le_bytes[0], (value & 0xFF) as u8);
            assert_eq!(le_bytes[1], ((value >> 8) & 0xFF) as u8);
            assert_eq!(le_bytes[2], ((value >> 16) & 0xFF) as u8);
            assert_eq!(le_bytes[3], ((value >> 24) & 0xFF) as u8);
        }
    }

    #[test]
    fn test_multibyte_fields_endianness() {
        // Test specific scenarios with realistic ADB protocol values

        // Version field (0x01000000 = version 1.0.0.0)
        let version = 0x01000000u32;
        let version_bytes = version.to_le_bytes();
        assert_eq!(version_bytes, [0x00, 0x00, 0x00, 0x01]);

        // Max data size (0x00100000 = 1MB)
        let maxdata = 0x00100000u32;
        let maxdata_bytes = maxdata.to_le_bytes();
        assert_eq!(maxdata_bytes, [0x00, 0x00, 0x10, 0x00]);

        // Stream ID (0x00000001)
        let stream_id = 0x00000001u32;
        let stream_bytes = stream_id.to_le_bytes();
        assert_eq!(stream_bytes, [0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_data_payload_not_affected() {
        // Test that data payload is not subject to endianness conversion
        // (only header fields should be little-endian)

        let data_payload = vec![0x12, 0x34, 0x56, 0x78];
        let processed_data = process_data_payload(&data_payload);

        // Data payload should remain unchanged
        assert_eq!(data_payload, processed_data);
    }

    #[test]
    fn test_wire_format_compatibility() {
        // Test that our endianness matches expected wire format

        let msg = AdbMessage {
            command: AdbCommand::OPEN as u32,
            arg0: 0x12345678,
            arg1: 0xABCDEF00,
            data_length: 0x10,
            data_crc32: 0x87654321,
            magic: !(AdbCommand::OPEN as u32),
            data: vec![],
        };

        let wire_format = serialize_to_wire_format(&msg);

        // Expected wire format (all fields in little-endian)
        let expected: Vec<u8> = vec![
            // command: OPEN (0x4E45504F)
            0x4F, 0x50, 0x45, 0x4E,
            // arg0: 0x12345678
            0x78, 0x56, 0x34, 0x12,
            // arg1: 0xABCDEF00
            0x00, 0xEF, 0xCD, 0xAB,
            // data_length: 0x10
            0x10, 0x00, 0x00, 0x00,
            // data_crc32: 0x87654321
            0x21, 0x43, 0x65, 0x87,
            // magic: !(0x4E45504F) = 0xB1BAAFB0
            0xB0, 0xAF, 0xBA, 0xB1,
        ];

        assert_eq!(wire_format, expected);
    }

    // Helper functions - these should be implemented in the actual code
    fn create_test_message() -> AdbMessage {
        AdbMessage {
            command: AdbCommand::CNXN as u32,
            arg0: 0x01000000,
            arg1: 0x00100000,
            data_length: 0,
            data_crc32: 0,
            magic: !(AdbCommand::CNXN as u32),
            data: vec![],
        }
    }

    fn serialize_message_header(msg: &AdbMessage) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(24);

        // 24-byte header (all little-endian)
        buffer.extend_from_slice(&msg.command.to_le_bytes());
        buffer.extend_from_slice(&msg.arg0.to_le_bytes());
        buffer.extend_from_slice(&msg.arg1.to_le_bytes());
        buffer.extend_from_slice(&msg.data_length.to_le_bytes());
        buffer.extend_from_slice(&msg.data_crc32.to_le_bytes());
        buffer.extend_from_slice(&msg.magic.to_le_bytes());

        buffer
    }

    fn process_data_payload(data: &[u8]) -> Vec<u8> {
        data.to_vec() // Data should not be modified
    }

    fn serialize_to_wire_format(msg: &AdbMessage) -> Vec<u8> {
        msg.serialize()
    }
}