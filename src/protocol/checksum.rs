use crc32fast::Hasher;

pub fn calculate(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

pub fn verify(data: &[u8], expected: u32) -> bool {
    calculate(data) == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_checksum() {
        let checksum = calculate(&[]);
        assert_eq!(checksum, 0);
    }

    #[test]
    fn test_simple_checksum() {
        let data = b"hello world";
        let checksum = calculate(data);
        assert!(verify(data, checksum));
    }

    #[test]
    fn test_verify_mismatch() {
        let data = b"hello world";
        assert!(!verify(data, 0x12345678));
    }
}
