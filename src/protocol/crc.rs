use crc32fast::Hasher;

/// Calculate CRC32 checksum for given data
/// Uses IEEE 802.3 CRC32 polynomial (same as used in ADB protocol)
pub fn calculate_crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Validate data against expected CRC32 checksum
pub fn validate_crc32(data: &[u8], expected_crc: u32) -> bool {
    let calculated_crc = calculate_crc32(data);
    calculated_crc == expected_crc
}

/// Calculate CRC32 for multiple data chunks
/// Useful for streaming data or when data is split across buffers
pub fn calculate_crc32_chunks(chunks: &[&[u8]]) -> u32 {
    let mut hasher = Hasher::new();
    for chunk in chunks {
        hasher.update(chunk);
    }
    hasher.finalize()
}

/// Validate multiple data chunks against expected CRC32
pub fn validate_crc32_chunks(chunks: &[&[u8]], expected_crc: u32) -> bool {
    let calculated_crc = calculate_crc32_chunks(chunks);
    calculated_crc == expected_crc
}

/// CRC32 hasher for incremental calculation
/// Useful when processing data in streams
pub struct Crc32Hasher {
    hasher: Hasher,
}

impl Crc32Hasher {
    /// Create a new CRC32 hasher
    pub fn new() -> Self {
        Self {
            hasher: Hasher::new(),
        }
    }

    /// Update hasher with new data
    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    /// Get the current CRC32 value without finalizing
    pub fn clone_finalize(&self) -> u32 {
        self.hasher.clone().finalize()
    }

    /// Finalize and get the CRC32 value (consumes the hasher)
    pub fn finalize(self) -> u32 {
        self.hasher.finalize()
    }

    /// Reset the hasher to initial state
    pub fn reset(&mut self) {
        self.hasher = Hasher::new();
    }

    /// Validate current state against expected CRC32
    pub fn validate(&self, expected_crc: u32) -> bool {
        self.clone_finalize() == expected_crc
    }
}

impl Default for Crc32Hasher {
    fn default() -> Self {
        Self::new()
    }
}

/// Known CRC32 values for common data patterns (useful for testing)
pub mod known_values {
    /// CRC32 of empty data
    pub const EMPTY: u32 = 0x00000000;

    /// CRC32 of "123456789"
    pub const TEST_STRING: u32 = 0xCBF43926;

    /// CRC32 of single null byte
    pub const NULL_BYTE: u32 = 0xD202EF8D;

    /// CRC32 of "CNXN" (connection command)
    pub const CNXN_COMMAND: u32 = 0x7C54F71A;

    /// CRC32 of "host:version"
    pub const HOST_VERSION: u32 = 0x8B5C2F3E;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_crc32_empty() {
        assert_eq!(calculate_crc32(b""), known_values::EMPTY);
    }

    #[test]
    fn test_calculate_crc32_known_value() {
        assert_eq!(calculate_crc32(b"123456789"), known_values::TEST_STRING);
    }

    #[test]
    fn test_validate_crc32_success() {
        let data = b"test data";
        let crc = calculate_crc32(data);
        assert!(validate_crc32(data, crc));
    }

    #[test]
    fn test_validate_crc32_failure() {
        let data = b"test data";
        let wrong_crc = 0x12345678;
        assert!(!validate_crc32(data, wrong_crc));
    }

    #[test]
    fn test_crc32_chunks() {
        let data = b"hello world";
        let chunks = [&data[..5], &data[5..]]; // Split "hello" and " world"

        let whole_crc = calculate_crc32(data);
        let chunks_crc = calculate_crc32_chunks(&chunks);

        assert_eq!(whole_crc, chunks_crc);
    }

    #[test]
    fn test_incremental_hasher() {
        let mut hasher = Crc32Hasher::new();
        hasher.update(b"hello");
        hasher.update(b" ");
        hasher.update(b"world");

        let incremental_crc = hasher.finalize();
        let direct_crc = calculate_crc32(b"hello world");

        assert_eq!(incremental_crc, direct_crc);
    }

    #[test]
    fn test_hasher_reset() {
        let mut hasher = Crc32Hasher::new();
        hasher.update(b"some data");

        hasher.reset();
        hasher.update(b"123456789");

        assert_eq!(hasher.finalize(), known_values::TEST_STRING);
    }

    #[test]
    fn test_hasher_clone_finalize() {
        let mut hasher = Crc32Hasher::new();
        hasher.update(b"123456789");

        let crc1 = hasher.clone_finalize();
        let crc2 = hasher.clone_finalize();
        let crc3 = hasher.finalize();

        assert_eq!(crc1, crc2);
        assert_eq!(crc2, crc3);
        assert_eq!(crc1, known_values::TEST_STRING);
    }
}