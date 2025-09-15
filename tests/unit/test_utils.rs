use dbgif_server::*;
use std::time::{Duration, Instant};

#[cfg(test)]
mod protocol_utils_tests {
    use super::*;
    use dbgif_server::protocol::{AdbMessage, Command};

    #[test]
    fn test_message_magic_calculation() {
        // Test CNXN command
        let cnxn_magic = Command::CNXN as u32;
        let expected_magic = !cnxn_magic;

        let message = AdbMessage::new(
            Command::CNXN,
            0x01000000,
            256 * 1024,
            bytes::Bytes::from("test"),
        );

        // Magic should be calculated correctly
        assert_eq!(message.magic, expected_magic);
    }

    #[test]
    fn test_message_data_length() {
        let test_data = "Hello, ADB!";
        let message = AdbMessage::new(
            Command::WRTE,
            1,
            2,
            bytes::Bytes::from(test_data),
        );

        assert_eq!(message.data_length as usize, test_data.len());
        assert_eq!(message.data.len(), test_data.len());
    }

    #[test]
    fn test_message_checksum() {
        let test_data = "test checksum data";
        let message = AdbMessage::new(
            Command::WRTE,
            1,
            2,
            bytes::Bytes::from(test_data),
        );

        // Checksum should be calculated for the data
        assert!(message.data_checksum != 0);
    }

    #[test]
    fn test_command_variants() {
        // Test all command variants exist
        let commands = vec![
            Command::CNXN,
            Command::AUTH,
            Command::OPEN,
            Command::OKAY,
            Command::CLSE,
            Command::WRTE,
        ];

        for cmd in commands {
            let magic = !(cmd as u32);
            assert!(magic != 0);
        }
    }
}

#[cfg(test)]
mod checksum_utils_tests {
    use super::*;
    use dbgif_server::protocol::checksum;

    #[test]
    fn test_crc32_empty_data() {
        let empty_data = &[];
        let checksum = checksum::calculate_crc32(empty_data);
        assert_eq!(checksum, 0);
    }

    #[test]
    fn test_crc32_known_data() {
        let test_data = b"hello world";
        let checksum = checksum::calculate_crc32(test_data);

        // CRC32 should be deterministic
        let checksum2 = checksum::calculate_crc32(test_data);
        assert_eq!(checksum, checksum2);
        assert!(checksum != 0);
    }

    #[test]
    fn test_crc32_different_data() {
        let data1 = b"hello";
        let data2 = b"world";

        let checksum1 = checksum::calculate_crc32(data1);
        let checksum2 = checksum::calculate_crc32(data2);

        // Different data should produce different checksums
        assert_ne!(checksum1, checksum2);
    }

    #[test]
    fn test_crc32_incremental() {
        let full_data = b"hello world";
        let part1 = b"hello ";
        let part2 = b"world";

        let full_checksum = checksum::calculate_crc32(full_data);

        // Incremental calculation should match
        let mut incremental_checksum = 0;
        incremental_checksum = checksum::update_crc32(incremental_checksum, part1);
        incremental_checksum = checksum::update_crc32(incremental_checksum, part2);

        assert_eq!(full_checksum, incremental_checksum);
    }
}

#[cfg(test)]
mod transport_utils_tests {
    use super::*;
    use dbgif_server::transport::TransportType;

    #[test]
    fn test_transport_type_variants() {
        let transport_types = vec![
            TransportType::Tcp,
            TransportType::UsbDevice,
            TransportType::UsbBridge,
        ];

        for transport_type in transport_types {
            // Each transport type should have a meaningful string representation
            let type_str = format!("{:?}", transport_type);
            assert!(!type_str.is_empty());
        }
    }

    #[test]
    fn test_transport_type_equality() {
        assert_eq!(TransportType::Tcp, TransportType::Tcp);
        assert_eq!(TransportType::UsbDevice, TransportType::UsbDevice);
        assert_eq!(TransportType::UsbBridge, TransportType::UsbBridge);

        assert_ne!(TransportType::Tcp, TransportType::UsbDevice);
        assert_ne!(TransportType::UsbDevice, TransportType::UsbBridge);
    }
}

#[cfg(test)]
mod configuration_utils_tests {
    use super::*;
    use dbgif_server::config::DaemonConfig;

    #[test]
    fn test_default_config() {
        let config = DaemonConfig::default();

        // Verify default values
        assert!(config.server.max_connections > 0);
        assert!(!config.server.bind_address.ip().is_unspecified() ||
                config.server.bind_address.port() != 0);

        // Transport should have at least one enabled by default
        assert!(config.transport.enable_tcp ||
                config.transport.enable_usb_device ||
                config.transport.enable_usb_bridge);
    }

    #[test]
    fn test_development_config() {
        let config = DaemonConfig::development();

        // Development config should enable more verbose logging
        assert_eq!(config.logging.level, "debug");

        // Should be more permissive
        assert!(config.server.max_connections >= 50);
    }

    #[test]
    fn test_production_config() {
        let config = DaemonConfig::production();

        // Production config should be more conservative
        assert_eq!(config.logging.level, "info");
        assert!(config.server.graceful_shutdown);
        assert!(config.server.shutdown_timeout.as_secs() > 0);
    }

    #[test]
    fn test_config_validation() {
        let mut config = DaemonConfig::default();

        // Valid config should pass validation
        assert!(config.validate().is_ok());

        // Invalid config should fail validation
        config.server.max_connections = 0;
        assert!(config.validate().is_err());
    }
}

#[cfg(test)]
mod logging_utils_tests {
    use super::*;
    use dbgif_server::logging::LoggingConfig;

    #[test]
    fn test_logging_config_default() {
        let config = LoggingConfig::default();

        assert_eq!(config.level, "info");
        assert_eq!(config.format, "compact");
        assert!(config.enable_console);
        assert!(!config.enable_file);
    }

    #[test]
    fn test_logging_config_development() {
        let config = LoggingConfig::development();

        assert_eq!(config.level, "debug");
        assert_eq!(config.format, "pretty");
        assert!(config.enable_console);
        assert!(config.enable_file);
        assert!(config.include_spans);
        assert!(config.include_line_numbers);
    }

    #[test]
    fn test_logging_config_production() {
        let config = LoggingConfig::production();

        assert_eq!(config.level, "info");
        assert!(!config.enable_console); // Production typically logs to files
        assert!(config.enable_file);
        assert!(!config.include_spans); // Less verbose in production
    }
}

#[cfg(test)]
mod utility_functions_tests {
    use super::*;

    #[test]
    fn test_byte_conversion_utilities() {
        // Test common byte conversion scenarios
        let bytes_1kb = 1024u64;
        let bytes_1mb = bytes_1kb * 1024;
        let bytes_1gb = bytes_1mb * 1024;

        assert_eq!(bytes_1kb, 1024);
        assert_eq!(bytes_1mb, 1048576);
        assert_eq!(bytes_1gb, 1073741824);
    }

    #[test]
    fn test_timeout_utilities() {
        let short_timeout = Duration::from_millis(100);
        let medium_timeout = Duration::from_secs(5);
        let long_timeout = Duration::from_secs(30);

        assert!(short_timeout < medium_timeout);
        assert!(medium_timeout < long_timeout);

        // Test timeout boundary conditions
        assert!(short_timeout.as_millis() == 100);
        assert!(medium_timeout.as_secs() == 5);
        assert!(long_timeout.as_secs() == 30);
    }

    #[test]
    fn test_performance_measurement() {
        let start_time = Instant::now();

        // Simulate some work
        std::thread::sleep(Duration::from_millis(10));

        let elapsed = start_time.elapsed();
        assert!(elapsed >= Duration::from_millis(10));
        assert!(elapsed < Duration::from_millis(100)); // Should be well under 100ms
    }

    #[test]
    fn test_string_utilities() {
        // Test common string operations used in the codebase
        let device_id = "device_12345";
        let client_id = "client_abcdef";

        let session_id = format!("{}:{}", client_id, device_id);
        assert_eq!(session_id, "client_abcdef:device_12345");

        // Test parsing
        let parts: Vec<&str> = session_id.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], client_id);
        assert_eq!(parts[1], device_id);
    }

    #[test]
    fn test_buffer_management() {
        // Test buffer size calculations
        let max_data_size = 256 * 1024; // 256KB
        let header_size = 24; // ADB message header
        let total_buffer_size = max_data_size + header_size;

        assert_eq!(max_data_size, 262144);
        assert_eq!(total_buffer_size, 262168);

        // Test buffer allocation
        let mut buffer = vec![0u8; total_buffer_size];
        buffer[0] = 0x4e; // First byte of CNXN command
        assert_eq!(buffer[0], 0x4e);
        assert_eq!(buffer.len(), total_buffer_size);
    }

    #[test]
    fn test_error_handling_utilities() {
        // Test common error scenarios
        let error_msg = "Connection timeout";
        let detailed_error = format!("Transport error: {}", error_msg);

        assert!(detailed_error.contains("Transport error"));
        assert!(detailed_error.contains(error_msg));
    }
}

#[cfg(test)]
mod async_utilities_tests {
    use super::*;
    use tokio::time::{sleep, timeout};

    #[tokio::test]
    async fn test_async_timeout() {
        // Test successful operation within timeout
        let result = timeout(Duration::from_millis(100), async {
            sleep(Duration::from_millis(10)).await;
            "success"
        }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_async_timeout_exceeded() {
        // Test operation that exceeds timeout
        let result = timeout(Duration::from_millis(10), async {
            sleep(Duration::from_millis(100)).await;
            "should not complete"
        }).await;

        assert!(result.is_err()); // Should timeout
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        // Test multiple concurrent operations
        let start_time = Instant::now();

        let (result1, result2, result3) = tokio::join!(
            async { sleep(Duration::from_millis(10)).await; "task1" },
            async { sleep(Duration::from_millis(10)).await; "task2" },
            async { sleep(Duration::from_millis(10)).await; "task3" }
        );

        let elapsed = start_time.elapsed();

        assert_eq!(result1, "task1");
        assert_eq!(result2, "task2");
        assert_eq!(result3, "task3");

        // All three 10ms tasks should complete in roughly 10ms when run concurrently
        assert!(elapsed < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_channel_communication() {
        use tokio::sync::mpsc;

        let (tx, mut rx) = mpsc::channel::<String>(10);

        // Send some messages
        tx.send("message1".to_string()).await.unwrap();
        tx.send("message2".to_string()).await.unwrap();
        tx.send("message3".to_string()).await.unwrap();

        // Close sender
        drop(tx);

        // Receive all messages
        let mut messages = Vec::new();
        while let Some(msg) = rx.recv().await {
            messages.push(msg);
        }

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0], "message1");
        assert_eq!(messages[1], "message2");
        assert_eq!(messages[2], "message3");
    }
}