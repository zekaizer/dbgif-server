#![cfg(feature = "std")]

use dbgif_protocol_core::*;

#[test]
fn test_single_byte_streaming() {
    let mut parser = MessageDecoder::new();

    // Create a CNXN message
    let header = MessageBuilder::new()
        .command(Command::CNXN)
        .arg0(0x01000000)
        .arg1(256 * 1024)
        .data_info(0, 0)
        .build();

    let mut buffer = [0u8; MessageHeader::SIZE];
    header.serialize(&mut buffer);

    // Feed one byte at a time
    for i in 0..MessageHeader::SIZE {
        let (result, consumed) = parser.feed(&buffer[i..i + 1]);

        if i < MessageHeader::SIZE - 1 {
            assert_eq!(result, DecodeResult::NeedMoreData);
            assert_eq!(consumed, 1);
        } else {
            assert_eq!(result, DecodeResult::Complete);
            assert_eq!(consumed, 1);
        }
    }

    // Verify parsed header
    let parsed_header = parser.get_header().unwrap();
    assert_eq!(parsed_header.command, Command::CNXN as u32);
    assert_eq!(parsed_header.arg0, 0x01000000);
    assert_eq!(parsed_header.arg1, 256 * 1024);
}

#[test]
fn test_message_with_data() {
    let mut parser = MessageDecoder::new();

    let test_data = b"test:device:v1.0";
    let mut encoder = MessageEncoder::new();
    let encoded = encoder.encode_with_data(Command::OPEN, 1, 0, test_data).unwrap();
    let crc = MessageHeader::deserialize(
        encoded[..MessageHeader::SIZE].try_into().unwrap()
    ).unwrap().data_crc32;

    // Create OPEN message with data
    let header = MessageBuilder::new()
        .command(Command::OPEN)
        .arg0(1)
        .arg1(0)
        .data_info(test_data.len() as u32, crc)
        .build();

    let mut message = Vec::new();
    let mut header_buf = [0u8; MessageHeader::SIZE];
    header.serialize(&mut header_buf);
    message.extend_from_slice(&header_buf);
    message.extend_from_slice(test_data);

    // Feed entire message
    let (result, consumed) = parser.feed(&message);
    assert_eq!(result, DecodeResult::Complete);
    assert_eq!(consumed, message.len());

    let parsed_header = parser.get_header().unwrap();
    assert_eq!(parsed_header.data_length, test_data.len() as u32);
    assert_eq!(parsed_header.data_crc32, crc);
}

#[test]
fn test_chunked_streaming() {
    let mut parser = MessageDecoder::new();

    let test_data = b"shell:";
    let mut encoder = MessageEncoder::new();
    let encoded = encoder.encode_with_data(Command::OPEN, 42, 0, test_data).unwrap();
    let crc = MessageHeader::deserialize(
        encoded[..MessageHeader::SIZE].try_into().unwrap()
    ).unwrap().data_crc32;

    let header = MessageBuilder::new()
        .command(Command::OPEN)
        .arg0(42)
        .arg1(0)
        .data_info(test_data.len() as u32, crc)
        .build();

    let mut message = Vec::new();
    let mut header_buf = [0u8; MessageHeader::SIZE];
    header.serialize(&mut header_buf);
    message.extend_from_slice(&header_buf);
    message.extend_from_slice(test_data);

    // Feed in 3 chunks
    let chunk1 = &message[0..10];
    let chunk2 = &message[10..24];
    let chunk3 = &message[24..];

    let (result1, consumed1) = parser.feed(chunk1);
    assert_eq!(result1, DecodeResult::NeedMoreData);
    assert_eq!(consumed1, 10);

    let (result2, consumed2) = parser.feed(chunk2);
    assert_eq!(result2, DecodeResult::NeedMoreData);
    assert_eq!(consumed2, 14);

    let (result3, consumed3) = parser.feed(chunk3);
    assert_eq!(result3, DecodeResult::Complete);
    assert_eq!(consumed3, chunk3.len());
}

#[test]
fn test_invalid_magic() {
    let mut parser = MessageDecoder::new();

    let mut header = MessageBuilder::new()
        .command(Command::WRTE)
        .arg0(1)
        .arg1(2)
        .build();

    // Corrupt magic
    header.magic = 0x12345678;

    let mut buffer = [0u8; MessageHeader::SIZE];
    header.serialize(&mut buffer);

    let (result, _) = parser.feed(&buffer);
    assert!(matches!(result, DecodeResult::Error { code: DecodeError::InvalidMagic }));
}

#[test]
fn test_crc_mismatch() {
    let mut parser = MessageDecoder::new();

    let test_data = b"corrupted";

    let header = MessageBuilder::new()
        .command(Command::WRTE)
        .arg0(1)
        .arg1(2)
        .data_info(test_data.len() as u32, 0xDEADBEEF) // Wrong CRC
        .build();

    let mut message = Vec::new();
    let mut header_buf = [0u8; MessageHeader::SIZE];
    header.serialize(&mut header_buf);
    message.extend_from_slice(&header_buf);
    message.extend_from_slice(test_data);

    let (result, _) = parser.feed(&message);
    assert!(matches!(result, DecodeResult::Error { code: DecodeError::CrcMismatch }));
}

#[test]
fn test_parser_reset() {
    let mut parser = MessageDecoder::new();

    // First message
    let header1 = MessageBuilder::new()
        .command(Command::PING)
        .build();

    let mut buffer1 = [0u8; MessageHeader::SIZE];
    header1.serialize(&mut buffer1);

    let (result1, _) = parser.feed(&buffer1);
    assert_eq!(result1, DecodeResult::Complete);

    // Reset parser
    parser.reset();

    // Second message
    let header2 = MessageBuilder::new()
        .command(Command::PONG)
        .build();

    let mut buffer2 = [0u8; MessageHeader::SIZE];
    header2.serialize(&mut buffer2);

    let (result2, _) = parser.feed(&buffer2);
    assert_eq!(result2, DecodeResult::Complete);

    let parsed = parser.get_header().unwrap();
    assert_eq!(parsed.command, Command::PONG as u32);
}

#[test]
fn test_data_accumulator() {
    let mut parser = MessageDecoder::new().with_data_accumulator();

    let test_data = b"accumulator test data";
    let mut encoder = MessageEncoder::new();
    let encoded = encoder.encode_with_data(Command::WRTE, 100, 200, test_data).unwrap();
    let crc = MessageHeader::deserialize(
        encoded[..MessageHeader::SIZE].try_into().unwrap()
    ).unwrap().data_crc32;

    let header = MessageBuilder::new()
        .command(Command::WRTE)
        .arg0(100)
        .arg1(200)
        .data_info(test_data.len() as u32, crc)
        .build();

    let mut message = Vec::new();
    let mut header_buf = [0u8; MessageHeader::SIZE];
    header.serialize(&mut header_buf);
    message.extend_from_slice(&header_buf);
    message.extend_from_slice(test_data);

    let (result, _) = parser.feed(&message);
    assert_eq!(result, DecodeResult::Complete);

    // Check accumulated data
    let accumulated = parser.get_accumulated_data();
    assert!(accumulated.is_some());
    assert_eq!(accumulated.unwrap(), test_data);
}

#[test]
fn test_multiple_messages_in_buffer() {
    let mut parser = MessageDecoder::new();

    // Create two messages
    let msg1 = MessageBuilder::new()
        .command(Command::PING)
        .build();

    let msg2 = MessageBuilder::new()
        .command(Command::PONG)
        .build();

    let mut buffer = Vec::new();
    let mut buf1 = [0u8; MessageHeader::SIZE];
    let mut buf2 = [0u8; MessageHeader::SIZE];
    msg1.serialize(&mut buf1);
    msg2.serialize(&mut buf2);
    buffer.extend_from_slice(&buf1);
    buffer.extend_from_slice(&buf2);

    // Parse first message
    let (result1, consumed1) = parser.feed(&buffer);
    assert_eq!(result1, DecodeResult::Complete);
    assert_eq!(consumed1, MessageHeader::SIZE);

    // Reset and parse second message
    parser.reset();
    let (result2, consumed2) = parser.feed(&buffer[consumed1..]);
    assert_eq!(result2, DecodeResult::Complete);
    assert_eq!(consumed2, MessageHeader::SIZE);
}

#[test]
fn test_state_transitions() {
    let mut parser = MessageDecoder::new();

    assert_eq!(parser.get_state(), DecoderState::WaitingForHeader);

    // Feed partial header
    let header = MessageBuilder::new()
        .command(Command::CNXN)
        .build();

    let mut buffer = [0u8; MessageHeader::SIZE];
    header.serialize(&mut buffer);

    let (_, _) = parser.feed(&buffer[..10]);
    assert!(matches!(parser.get_state(), DecoderState::AccumulatingHeader { received: 10 }));

    // Complete header
    let (_, _) = parser.feed(&buffer[10..]);
    assert_eq!(parser.get_state(), DecoderState::MessageComplete);
}

#[test]
fn test_bytes_tracking() {
    let mut parser = MessageDecoder::new();

    let test_data = b"tracking test";
    let mut encoder = MessageEncoder::new();
    let encoded = encoder.encode_with_data(Command::WRTE, 1, 2, test_data).unwrap();
    let crc = MessageHeader::deserialize(
        encoded[..MessageHeader::SIZE].try_into().unwrap()
    ).unwrap().data_crc32;

    let header = MessageBuilder::new()
        .command(Command::WRTE)
        .arg0(1)
        .arg1(2)
        .data_info(test_data.len() as u32, crc)
        .build();

    let mut message = Vec::new();
    let mut header_buf = [0u8; MessageHeader::SIZE];
    header.serialize(&mut header_buf);
    message.extend_from_slice(&header_buf);
    message.extend_from_slice(test_data);

    // Initially need header
    assert_eq!(parser.bytes_needed(), MessageHeader::SIZE);
    assert_eq!(parser.bytes_received(), 0);

    // Feed header
    let (_, _) = parser.feed(&message[..MessageHeader::SIZE]);
    assert_eq!(parser.bytes_needed(), test_data.len());
    assert_eq!(parser.bytes_received(), MessageHeader::SIZE);

    // Feed data
    let (_, _) = parser.feed(&message[MessageHeader::SIZE..]);
    assert_eq!(parser.bytes_needed(), 0);
    assert_eq!(parser.bytes_received(), message.len());
}