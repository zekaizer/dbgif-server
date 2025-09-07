/// Utility for displaying binary data in hex dump format
use std::fmt::Write;

/// Print hex dump to stdout with label
pub fn hex_dump(label: &str, data: &[u8], max_bytes: Option<usize>) {
    println!("{}", hex_dump_string(label, data, max_bytes));
}

/// Generate hex dump string without printing
pub fn hex_dump_string(label: &str, data: &[u8], max_bytes: Option<usize>) -> String {
    let limit = max_bytes.unwrap_or(256);
    let mut output = String::new();

    writeln!(output, "=== {} ({} bytes) ===", label, data.len()).unwrap();

    let display_bytes = limit.min(data.len());

    for (i, chunk) in data[..display_bytes].chunks(16).enumerate() {
        // Address part
        write!(output, "{:08X}: ", i * 16).unwrap();

        // Hex part (16 bytes per line, padded if needed)
        for (j, byte) in chunk.iter().enumerate() {
            write!(output, "{:02X}", byte).unwrap();
            if j == 7 {
                // Extra space in the middle for readability
                output.push(' ');
            }
            output.push(' ');
        }

        // Pad remaining space if chunk is less than 16 bytes
        for j in chunk.len()..16 {
            if j == 7 {
                output.push(' ');
            }
            output.push_str("   ");
        }

        // ASCII part
        output.push_str(" |");
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                output.push(*byte as char);
            } else {
                output.push('.');
            }
        }
        output.push('|');
        output.push('\n');
    }

    if data.len() > limit {
        writeln!(output, "... ({} more bytes hidden) ...", data.len() - limit).unwrap();
    }

    output
}

/// Generate compact hex string for tracing (no formatting)
pub fn hex_string_compact(data: &[u8], max_bytes: Option<usize>) -> String {
    let limit = max_bytes.unwrap_or(64);
    let display_bytes = limit.min(data.len());

    let mut hex = data[..display_bytes]
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ");

    if data.len() > limit {
        hex.push_str(&format!(" ... ({} more)", data.len() - limit));
    }

    hex
}

/// Format bytes with optional ASCII representation for single line output
pub fn format_bytes_inline(data: &[u8], max_bytes: Option<usize>) -> String {
    let limit = max_bytes.unwrap_or(32);
    let display_bytes = limit.min(data.len());

    let hex = hex_string_compact(data, Some(display_bytes));
    let ascii = data[..display_bytes]
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            }
        })
        .collect::<String>();

    if data.len() > limit {
        format!("{} |{}...| ({} total)", hex, &ascii, data.len())
    } else {
        format!("{} |{}|", hex, ascii)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_dump_string() {
        let data = b"Hello, World!\x00\x01\x02\x03";
        let result = hex_dump_string("TEST", data, None);

        assert!(result.contains("=== TEST"));
        assert!(result.contains("48 65 6C 6C 6F")); // "Hello"
        assert!(result.contains("Hello, World!"));
        assert!(result.contains("00000000:"));
    }

    #[test]
    fn test_hex_string_compact() {
        let data = b"ABCD";
        let result = hex_string_compact(data, None);
        assert_eq!(result, "41 42 43 44");
    }

    #[test]
    fn test_format_bytes_inline() {
        let data = b"Hi\x00\xff";
        let result = format_bytes_inline(data, None);
        assert!(result.contains("48 69 00 FF"));
        assert!(result.contains("|Hi..|"));
    }

    #[test]
    fn test_truncation() {
        let data: Vec<u8> = (0..100).collect();
        let result = hex_dump_string("LONG", &data, Some(32));
        assert!(result.contains("(68 more bytes hidden)"));
    }

    #[test]
    fn test_empty_data() {
        let data = b"";
        let result = hex_dump_string("EMPTY", data, None);
        assert!(result.contains("=== EMPTY (0 bytes) ==="));
    }

    #[test]
    fn test_sixteen_byte_boundary() {
        let data: Vec<u8> = (0..17).collect(); // 17 bytes to test line wrapping
        let result = hex_dump_string("WRAP", &data, None);

        // Should have two lines of hex output
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.len() >= 3); // Header + 2 data lines
        assert!(lines[1].starts_with("00000000:"));
        assert!(lines[2].starts_with("00000010:"));
    }
}
