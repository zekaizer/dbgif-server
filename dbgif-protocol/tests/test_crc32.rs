#[cfg(test)]
mod tests {
    use dbgif_protocol::crc::{calculate_crc32, validate_crc32};

    #[test]
    fn test_crc32_calculation() {
        // Test CRC32 calculation for known data
        let data1 = b"hello";
        let crc1 = calculate_crc32(data1);

        // CRC32 should be deterministic
        assert_eq!(calculate_crc32(data1), crc1);

        // Different data should produce different CRC
        let data2 = b"world";
        let crc2 = calculate_crc32(data2);
        assert_ne!(crc1, crc2);
    }

    #[test]
    fn test_crc32_empty_data() {
        // Empty data should have a specific CRC32 value
        let empty_crc = calculate_crc32(&[]);

        // Standard CRC32 for empty data is 0x00000000
        assert_eq!(empty_crc, 0x00000000);
    }

    #[test]
    fn test_crc32_known_values() {
        // Test against known CRC32 values (IEEE 802.3 polynomial)

        // "123456789" should have CRC32: 0xCBF43926
        let test_data = b"123456789";
        let expected_crc = 0xCBF43926;
        assert_eq!(calculate_crc32(test_data), expected_crc);

        // "A" should have CRC32: 0xD3D99E8B
        let single_char = b"A";
        let expected_single = 0xD3D99E8B;
        assert_eq!(calculate_crc32(single_char), expected_single);
    }

    #[test]
    fn test_crc32_validation_success() {
        let data = b"test data for validation";
        let crc = calculate_crc32(data);

        // Validation should succeed with correct CRC
        assert!(validate_crc32(data, crc));
    }

    #[test]
    fn test_crc32_validation_failure() {
        let data = b"test data for validation";
        let wrong_crc = 0x12345678; // Wrong CRC value

        // Validation should fail with incorrect CRC
        assert!(!validate_crc32(data, wrong_crc));
    }

    #[test]
    fn test_crc32_protocol_data() {
        // Test CRC32 with ADB-like protocol data
        let command_data = [
            0x43, 0x4E, 0x58, 0x4E, // CNXN command (little-endian)
            0x00, 0x00, 0x00, 0x01, // version
            0x00, 0x10, 0x00, 0x00, // maxdata
        ];

        let crc = calculate_crc32(&command_data);

        // CRC should be consistent for the same data
        assert_eq!(calculate_crc32(&command_data), crc);

        // Validation should work
        assert!(validate_crc32(&command_data, crc));
    }

    #[test]
    fn test_crc32_binary_data() {
        // Test with binary data containing all possible byte values
        let binary_data: Vec<u8> = (0..=255).collect();
        let crc = calculate_crc32(&binary_data);

        // Should handle binary data correctly
        assert!(validate_crc32(&binary_data, crc));

        // Changing one byte should invalidate the CRC
        let mut modified_data = binary_data.clone();
        modified_data[128] = modified_data[128].wrapping_add(1);
        assert!(!validate_crc32(&modified_data, crc));
    }

    #[test]
    fn test_crc32_large_data() {
        // Test with larger data to ensure algorithm works for any size
        let large_data = vec![0xAB; 1024]; // 1KB of 0xAB bytes
        let crc = calculate_crc32(&large_data);

        assert!(validate_crc32(&large_data, crc));

        // Different size should produce different CRC
        let different_size = vec![0xAB; 512];
        let different_crc = calculate_crc32(&different_size);
        assert_ne!(crc, different_crc);
    }
}