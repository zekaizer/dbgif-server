#![cfg(feature = "ffi")]

use dbgif_protocol_core::*;

#[test]
fn test_ffi_wrapper_matches_rust_api() {
    // Test that FFI wrapper produces same results as Rust API

    // Create test message
    let test_data = b"test:device:v1.0";
    let mut encoder = MessageEncoder::new();
    let encoded = encoder.encode_with_data(Command::CNXN, 0x01000000, 256 * 1024, test_data).unwrap();
    let crc = MessageHeader::deserialize(
        encoded[..MessageHeader::SIZE].try_into().unwrap()
    ).unwrap().data_crc32;

    let header = MessageBuilder::new()
        .command(Command::CNXN)
        .arg0(0x01000000)
        .arg1(256 * 1024)
        .data_info(test_data.len() as u32, crc)
        .build();

    let mut message = Vec::new();
    let mut header_buf = [0u8; MessageHeader::SIZE];
    header.serialize(&mut header_buf);
    message.extend_from_slice(&header_buf);
    message.extend_from_slice(test_data);

    // Test with Rust API
    let mut rust_parser = MessageDecoder::new().with_data_accumulator();
    let (rust_result, rust_consumed) = rust_parser.feed(&message);

    // Test with FFI API
    unsafe {
        let mut ffi_buffer = vec![0u8; 512];
        let ffi_parser = ffi::dbgif_decoder_init(ffi_buffer.as_mut_ptr(), ffi_buffer.len());

        let mut ffi_consumed = 0;
        let ffi_result = ffi::dbgif_decode_bytes(
            ffi_parser,
            message.as_ptr(),
            message.len(),
            &mut ffi_consumed,
        );

        // Compare results
        assert_eq!(rust_consumed, ffi_consumed);
        assert_eq!(rust_result, DecodeResult::Complete);
        assert_eq!(ffi_result, ffi::DbgifDecodeResult::MessageComplete);

        // Compare parsed headers
        let rust_header = rust_parser.get_header().unwrap();
        let mut ffi_header = MessageHeader::new();
        let result = ffi::dbgif_get_message(
            ffi_parser,
            &mut ffi_header,
            ptr::null_mut(),
            ptr::null_mut(),
        );

        assert_eq!(result, 0);
        assert_eq!(rust_header.command, ffi_header.command);
        assert_eq!(rust_header.arg0, ffi_header.arg0);
        assert_eq!(rust_header.arg1, ffi_header.arg1);
        assert_eq!(rust_header.data_length, ffi_header.data_length);
        assert_eq!(rust_header.data_crc32, ffi_header.data_crc32);

        // Compare data
        let rust_data = rust_parser.get_accumulated_data();
        let mut ffi_data_ptr: *const u8 = ptr::null();
        let mut ffi_data_len = 0;

        ffi::dbgif_get_message(
            ffi_parser,
            &mut ffi_header,
            &mut ffi_data_ptr,
            &mut ffi_data_len,
        );

        assert_eq!(rust_data.unwrap().len(), ffi_data_len);
        if ffi_data_len > 0 {
            let ffi_data = slice::from_raw_parts(ffi_data_ptr, ffi_data_len);
            assert_eq!(rust_data.unwrap(), ffi_data);
        }
    }
}

#[test]
fn test_ffi_streaming_behavior() {
    // Test that FFI wrapper handles streaming correctly
    let header = MessageBuilder::new()
        .command(Command::PING)
        .build();

    let mut message = [0u8; MessageHeader::SIZE];
    header.serialize(&mut message);

    unsafe {
        let mut buffer = vec![0u8; 256];
        let parser = ffi::dbgif_decoder_init(buffer.as_mut_ptr(), buffer.len());

        // Feed byte by byte
        for i in 0..MessageHeader::SIZE {
            let mut consumed = 0;
            let result = ffi::dbgif_decode_bytes(
                parser,
                &message[i],
                1,
                &mut consumed,
            );

            assert_eq!(consumed, 1);

            if i < MessageHeader::SIZE - 1 {
                assert_eq!(result, ffi::DbgifDecodeResult::NeedMoreData);
            } else {
                assert_eq!(result, ffi::DbgifDecodeResult::MessageComplete);
            }
        }
    }
}

#[test]
fn test_ffi_error_handling() {
    // Test that FFI wrapper handles errors correctly
    let mut bad_header = MessageBuilder::new()
        .command(Command::WRTE)
        .build();
    bad_header.magic = 0xDEADBEEF; // Invalid magic

    let mut message = [0u8; MessageHeader::SIZE];
    bad_header.serialize(&mut message);

    unsafe {
        let mut buffer = vec![0u8; 256];
        let parser = ffi::dbgif_decoder_init(buffer.as_mut_ptr(), buffer.len());

        let mut consumed = 0;
        let result = ffi::dbgif_decode_bytes(
            parser,
            message.as_ptr(),
            message.len(),
            &mut consumed,
        );

        assert_eq!(result, ffi::DbgifDecodeResult::ErrorInvalidMagic);
    }
}

#[test]
fn test_ffi_reset() {
    // Test that FFI reset works correctly
    unsafe {
        let mut buffer = vec![0u8; 256];
        let parser = ffi::dbgif_decoder_init(buffer.as_mut_ptr(), buffer.len());

        // First message
        let msg1 = MessageBuilder::new()
            .command(Command::PING)
            .build();
        let mut buf1 = [0u8; MessageHeader::SIZE];
        msg1.serialize(&mut buf1);

        let mut consumed = 0;
        let result1 = ffi::dbgif_decode_bytes(
            parser,
            buf1.as_ptr(),
            buf1.len(),
            &mut consumed,
        );
        assert_eq!(result1, ffi::DbgifDecodeResult::MessageComplete);

        // Reset
        ffi::dbgif_decoder_reset(parser);

        // Second message
        let msg2 = MessageBuilder::new()
            .command(Command::PONG)
            .build();
        let mut buf2 = [0u8; MessageHeader::SIZE];
        msg2.serialize(&mut buf2);

        let result2 = ffi::dbgif_decode_bytes(
            parser,
            buf2.as_ptr(),
            buf2.len(),
            &mut consumed,
        );
        assert_eq!(result2, ffi::DbgifDecodeResult::MessageComplete);

        // Check it's the second message
        let mut header = MessageHeader::new();
        ffi::dbgif_get_message(parser, &mut header, ptr::null_mut(), ptr::null_mut());
        assert_eq!(header.command, Command::PONG as u32);
    }
}

#[test]
fn test_ffi_status() {
    // Test that FFI status reporting works
    unsafe {
        let mut buffer = vec![0u8; 256];
        let parser = ffi::dbgif_decoder_init(buffer.as_mut_ptr(), buffer.len());

        let mut status = ffi::DbgifDecodeStatus {
            state: 0,
            bytes_needed: 0,
            bytes_received: 0,
            expected_crc: 0,
            current_crc: 0,
        };

        // Initial status
        ffi::dbgif_get_status(parser, &mut status);
        assert_eq!(status.state, 0); // WaitingForHeader
        assert_eq!(status.bytes_needed, MessageHeader::SIZE);
        assert_eq!(status.bytes_received, 0);

        // Feed partial header
        let header = MessageBuilder::new()
            .command(Command::CNXN)
            .build();
        let mut message = [0u8; MessageHeader::SIZE];
        header.serialize(&mut message);

        let mut consumed = 0;
        ffi::dbgif_decode_bytes(parser, message.as_ptr(), 10, &mut consumed);

        ffi::dbgif_get_status(parser, &mut status);
        assert_eq!(status.state, 1); // AccumulatingHeader
        assert_eq!(status.bytes_needed, MessageHeader::SIZE - 10);
        assert_eq!(status.bytes_received, 10);
    }
}

#[test]
fn test_ffi_encoder() {
    // Test encoder through FFI
    unsafe {
        let mut buffer = vec![0u8; 512];
        let encoder = ffi::dbgif_encoder_init(buffer.as_mut_ptr(), buffer.len());
        assert!(!encoder.is_null());

        // Encode a PING message
        let mut output: *const u8 = ptr::null();
        let mut output_len: usize = 0;

        let result = ffi::dbgif_encode_message(
            encoder,
            Command::PING as u32,
            0,
            0,
            ptr::null(),
            0,
            &mut output,
            &mut output_len,
        );

        assert_eq!(result, 0);
        assert!(!output.is_null());
        assert_eq!(output_len, MessageHeader::SIZE);

        // Verify the encoded message
        let encoded_data = slice::from_raw_parts(output, output_len);
        let header = MessageHeader::deserialize(
            encoded_data[..MessageHeader::SIZE].try_into().unwrap()
        ).unwrap();

        assert_eq!(header.command, Command::PING as u32);
        assert_eq!(header.magic, !(Command::PING as u32));
    }
}

#[test]
fn test_ffi_header_serialization() {
    // Test header serialization through FFI
    let rust_header = MessageBuilder::new()
        .command(Command::OPEN)
        .arg0(123)
        .arg1(456)
        .data_info(789, 0xABCDEF00)
        .build();

    unsafe {
        let mut ffi_header = MessageHeader::new();
        ffi::dbgif_create_header(
            Command::OPEN as u32,
            123,
            456,
            789,
            0xABCDEF00,
            &mut ffi_header,
        );

        // Serialize both
        let mut rust_buffer = [0u8; MessageHeader::SIZE];
        let mut ffi_buffer = [0u8; MessageHeader::SIZE];

        rust_header.serialize(&mut rust_buffer);
        ffi::dbgif_serialize_header(&ffi_header, ffi_buffer.as_mut_ptr());

        assert_eq!(rust_buffer, ffi_buffer);

        // Test deserialization
        let mut deserialized = MessageHeader::new();
        let result = ffi::dbgif_deserialize_header(
            ffi_buffer.as_ptr(),
            &mut deserialized,
        );

        assert_eq!(result, 0);
        assert_eq!(deserialized.command, rust_header.command);
        assert_eq!(deserialized.arg0, rust_header.arg0);
        assert_eq!(deserialized.arg1, rust_header.arg1);
        assert_eq!(deserialized.data_length, rust_header.data_length);
        assert_eq!(deserialized.data_crc32, rust_header.data_crc32);
    }
}

use core::ptr;
use core::slice;