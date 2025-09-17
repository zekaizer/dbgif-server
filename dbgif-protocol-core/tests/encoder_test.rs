#![cfg(feature = "std")]

use dbgif_protocol_core::*;

#[test]
fn test_encoder_simple_messages() {
    let mut encoder = MessageEncoder::new();

    // Test PING
    let result = encoder.encode_simple(Command::PING, 0, 0).unwrap();
    assert_eq!(result.len(), MessageHeader::SIZE);

    let header = MessageHeader::deserialize(
        result[..MessageHeader::SIZE].try_into().unwrap()
    ).unwrap();
    assert_eq!(header.command, Command::PING as u32);
    assert_eq!(header.magic, !(Command::PING as u32));

    // Test PONG
    encoder.clear();
    let result = encoder.encode_simple(Command::PONG, 0, 0).unwrap();
    assert_eq!(result.len(), MessageHeader::SIZE);

    let header = MessageHeader::deserialize(
        result[..MessageHeader::SIZE].try_into().unwrap()
    ).unwrap();
    assert_eq!(header.command, Command::PONG as u32);
}

#[test]
fn test_encoder_with_data() {
    let mut encoder = MessageEncoder::new();

    let test_data = b"test:device:v1.0";
    let result = encoder.encode_with_data(
        Command::CNXN,
        0x01000000,
        256 * 1024,
        test_data
    ).unwrap();

    assert_eq!(result.len(), MessageHeader::SIZE + test_data.len());

    // Verify header
    let header = MessageHeader::deserialize(
        result[..MessageHeader::SIZE].try_into().unwrap()
    ).unwrap();

    assert_eq!(header.command, Command::CNXN as u32);
    assert_eq!(header.arg0, 0x01000000);
    assert_eq!(header.arg1, 256 * 1024);
    assert_eq!(header.data_length, test_data.len() as u32);
    // CRC is calculated internally by encoder
    assert_ne!(header.data_crc32, 0); // Should have a valid CRC

    // Verify data
    assert_eq!(&result[MessageHeader::SIZE..], test_data);
}

#[test]
fn test_encoder_decoder_roundtrip() {
    let mut encoder = MessageEncoder::new();
    let mut decoder = MessageDecoder::new().with_data_accumulator();

    // Encode a message
    let test_data = b"shell:";
    let encoded = encoder.encode_with_data(
        Command::OPEN,
        42,
        0,
        test_data
    ).unwrap();

    // Decode it
    let (result, consumed) = decoder.feed(encoded);
    assert_eq!(result, DecodeResult::Complete);
    assert_eq!(consumed, encoded.len());

    // Verify it matches
    let header = decoder.get_header().unwrap();
    assert_eq!(header.command, Command::OPEN as u32);
    assert_eq!(header.arg0, 42);
    assert_eq!(header.arg1, 0);

    let data = decoder.get_accumulated_data().unwrap();
    assert_eq!(data, test_data);
}

#[test]
fn test_encoder_clear_and_reuse() {
    let mut encoder = MessageEncoder::new();

    // First message
    let msg1 = encoder.encode_simple(Command::PING, 0, 0).unwrap();
    let len1 = msg1.len();

    // Clear and encode second message
    encoder.clear();
    let msg2_data = b"longer test data";
    let msg2 = encoder.encode_with_data(
        Command::WRTE,
        1,
        2,
        msg2_data
    ).unwrap();

    assert_ne!(len1, msg2.len());
    assert_eq!(msg2.len(), MessageHeader::SIZE + msg2_data.len());
}

#[test]
fn test_message_factory_cnxn() {
    let (header, data) = MessageFactory::cnxn(
        0x01000001,
        512 * 1024,
        "host:features"
    );

    assert_eq!(header.command, Command::CNXN as u32);
    assert_eq!(header.arg0, 0x01000001);
    assert_eq!(header.arg1, 512 * 1024);
    assert_eq!(data, b"host:features");
    // CRC is calculated internally by MessageFactory
    assert_ne!(header.data_crc32, 0); // Should have a valid CRC
}

#[test]
fn test_message_factory_open() {
    let (header, data) = MessageFactory::open(123, "tcp:5555");

    assert_eq!(header.command, Command::OPEN as u32);
    assert_eq!(header.arg0, 123);
    assert_eq!(header.arg1, 0);
    assert_eq!(data, b"tcp:5555");
    // CRC is calculated internally by MessageFactory
    assert_ne!(header.data_crc32, 0); // Should have a valid CRC
}

#[test]
fn test_message_factory_wrte() {
    let test_data = b"Hello from client";
    let (header, data) = MessageFactory::wrte(100, 200, test_data);

    assert_eq!(header.command, Command::WRTE as u32);
    assert_eq!(header.arg0, 100);
    assert_eq!(header.arg1, 200);
    assert_eq!(data, test_data);
    // CRC is calculated internally by MessageFactory
    assert_ne!(header.data_crc32, 0); // Should have a valid CRC
}

#[test]
fn test_message_factory_control_messages() {
    // OKAY
    let okay = MessageFactory::okay(1, 2);
    assert_eq!(okay.command, Command::OKAY as u32);
    assert_eq!(okay.arg0, 1);
    assert_eq!(okay.arg1, 2);
    assert_eq!(okay.data_length, 0);

    // CLSE
    let clse = MessageFactory::clse(3, 4);
    assert_eq!(clse.command, Command::CLSE as u32);
    assert_eq!(clse.arg0, 3);
    assert_eq!(clse.arg1, 4);
    assert_eq!(clse.data_length, 0);

    // PING
    let ping = MessageFactory::ping();
    assert_eq!(ping.command, Command::PING as u32);
    assert_eq!(ping.data_length, 0);

    // PONG
    let pong = MessageFactory::pong();
    assert_eq!(pong.command, Command::PONG as u32);
    assert_eq!(pong.data_length, 0);
}

#[test]
fn test_encode_header_validation() {
    let mut encoder = MessageEncoder::new();

    // Create header with wrong data length
    let header = MessageBuilder::new()
        .command(Command::WRTE)
        .arg0(1)
        .arg1(2)
        .data_info(10, 0x12345678) // Claims 10 bytes
        .build();

    let test_data = b"short"; // Only 5 bytes

    // Should fail due to length mismatch
    let result = encoder.encode_header(&header, test_data);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DecodeError::InvalidState));
}

#[test]
fn test_encode_header_crc_validation() {
    let mut encoder = MessageEncoder::new();

    let test_data = b"test data";
    let wrong_crc = 0xDEADBEEF;

    let header = MessageBuilder::new()
        .command(Command::WRTE)
        .arg0(1)
        .arg1(2)
        .data_info(test_data.len() as u32, wrong_crc)
        .build();

    // Should fail due to CRC mismatch
    let result = encoder.encode_header(&header, test_data);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DecodeError::CrcMismatch));
}

#[test]
fn test_encoder_capacity() {
    let encoder = MessageEncoder::new();

    // Initial capacity should be 0 or some default
    let _initial_capacity = encoder.capacity();

    let mut encoder = MessageEncoder::new();
    encoder.encode_with_data(
        Command::WRTE,
        1,
        2,
        b"test"
    ).unwrap();

    // After encoding, capacity should be at least the message size
    assert!(encoder.capacity() >= MessageHeader::SIZE + 4);
}