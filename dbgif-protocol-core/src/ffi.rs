use crate::error::{DecodeError, StateError};
use crate::header::MessageHeader;
use crate::decoder::{DecodeResult, MessageDecoder};
use crate::encoder::MessageEncoder;
use crate::commands::Command;
use crate::state::{ProtocolStateMachine, ConnectionState};
use core::mem;
use core::ptr;
use core::slice;

// FFI-safe decoder handle - now just a thin wrapper
#[repr(C)]
pub struct DbgifDecoder {
    decoder: MessageDecoder,
}

// FFI-safe encoder handle
#[repr(C)]
pub struct DbgifEncoder {
    encoder: MessageEncoder,
}

// FFI-safe parse result
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DbgifDecodeResult {
    NeedMoreData = 0,
    MessageComplete = 1,
    ErrorInvalidMagic = -1,
    ErrorInvalidCommand = -2,
    ErrorCrcMismatch = -3,
    ErrorBufferTooSmall = -4,
    ErrorInvalidState = -5,
}

impl From<DecodeResult> for DbgifDecodeResult {
    fn from(result: DecodeResult) -> Self {
        match result {
            DecodeResult::NeedMoreData => DbgifDecodeResult::NeedMoreData,
            DecodeResult::Complete => DbgifDecodeResult::MessageComplete,
            DecodeResult::Error { code } => match code {
                DecodeError::InvalidMagic => DbgifDecodeResult::ErrorInvalidMagic,
                DecodeError::InvalidCommand => DbgifDecodeResult::ErrorInvalidCommand,
                DecodeError::CrcMismatch => DbgifDecodeResult::ErrorCrcMismatch,
                DecodeError::BufferTooSmall => DbgifDecodeResult::ErrorBufferTooSmall,
                DecodeError::InvalidState => DbgifDecodeResult::ErrorInvalidState,
            },
        }
    }
}

// FFI-safe status structure
#[repr(C)]
pub struct DbgifDecodeStatus {
    pub state: u32,
    pub bytes_needed: usize,
    pub bytes_received: usize,
    pub expected_crc: u32,
    pub current_crc: u32,
}

// Initialize parser with working buffer
#[no_mangle]
pub unsafe extern "C" fn dbgif_decoder_init(
    working_buffer: *mut u8,
    buffer_size: usize,
) -> *mut DbgifDecoder {
    if working_buffer.is_null() || buffer_size < mem::size_of::<DbgifDecoder>() {
        return ptr::null_mut();
    }

    let parser_ptr = working_buffer as *mut DbgifDecoder;
    let parser = &mut *parser_ptr;

    // Initialize the parser
    #[cfg(feature = "std")]
    {
        parser.decoder = MessageDecoder::new().with_data_accumulator();
    }

    #[cfg(not(feature = "std"))]
    {
        // For no_std, use the remaining buffer for data storage
        let parser_size = mem::size_of::<DbgifDecoder>();
        if buffer_size > parser_size + 256 {
            // Ensure we have enough space for data
            let data_buffer = working_buffer.add(parser_size);
            let data_buffer_size = buffer_size - parser_size;

            // Create a static reference (unsafe but necessary for no_std)
            let static_buffer = slice::from_raw_parts_mut(data_buffer, data_buffer_size);
            let static_ref = &mut *(static_buffer as *mut [u8]);

            parser.decoder = MessageDecoder::new().with_static_buffer(static_ref);
        } else {
            parser.decoder = MessageDecoder::new();
        }
    }

    parser_ptr
}

// Reset parser state
#[no_mangle]
pub unsafe extern "C" fn dbgif_decoder_reset(parser: *mut DbgifDecoder) {
    if parser.is_null() {
        return;
    }

    let parser = &mut *parser;
    parser.decoder.reset();
}

// Feed bytes to parser - now just a thin wrapper
#[no_mangle]
pub unsafe extern "C" fn dbgif_decode_bytes(
    parser: *mut DbgifDecoder,
    data: *const u8,
    len: usize,
    consumed: *mut usize,
) -> DbgifDecodeResult {
    if parser.is_null() || data.is_null() {
        return DbgifDecodeResult::ErrorInvalidState;
    }

    let decoder = &mut (*parser).decoder;
    let data_slice = slice::from_raw_parts(data, len);

    // Simply call the Rust API and convert the result
    let (result, bytes_consumed) = decoder.feed(data_slice);

    if !consumed.is_null() {
        *consumed = bytes_consumed;
    }

    result.into()
}

// Get completed message - retrieves from parser's accumulator
#[no_mangle]
pub unsafe extern "C" fn dbgif_get_message(
    parser: *mut DbgifDecoder,
    header: *mut MessageHeader,
    data: *mut *const u8,
    data_len: *mut usize,
) -> i32 {
    if parser.is_null() || header.is_null() {
        return -1;
    }

    let parser = &*parser;

    if let Some(msg_header) = parser.decoder.get_header() {
        *header = *msg_header;

        if !data.is_null() && !data_len.is_null() {
            if let Some(accumulated_data) = parser.decoder.get_accumulated_data() {
                *data = accumulated_data.as_ptr();
                *data_len = accumulated_data.len();
            } else {
                *data = ptr::null();
                *data_len = 0;
            }
        }

        0
    } else {
        -1
    }
}

// Get parser status
#[no_mangle]
pub unsafe extern "C" fn dbgif_get_status(
    parser: *mut DbgifDecoder,
    status: *mut DbgifDecodeStatus,
) {
    if parser.is_null() || status.is_null() {
        return;
    }

    let parser = &*parser;
    let state = parser.decoder.get_state();

    (*status).state = match state {
        crate::decoder::DecoderState::WaitingForHeader => 0,
        crate::decoder::DecoderState::AccumulatingHeader { .. } => 1,
        crate::decoder::DecoderState::WaitingForData => 2,
        crate::decoder::DecoderState::AccumulatingData { .. } => 3,
        crate::decoder::DecoderState::MessageComplete => 4,
        crate::decoder::DecoderState::Error { .. } => 5,
    };

    (*status).bytes_needed = parser.decoder.bytes_needed();
    (*status).bytes_received = parser.decoder.bytes_received();

    if let Some(header) = parser.decoder.get_header() {
        (*status).expected_crc = header.data_crc32;
    } else {
        (*status).expected_crc = 0;
    }

    // Current CRC would need to be exposed from parser if needed
    (*status).current_crc = 0;
}

// Helper function to create messages
#[no_mangle]
pub unsafe extern "C" fn dbgif_create_header(
    command: u32,
    arg0: u32,
    arg1: u32,
    data_length: u32,
    data_crc32: u32,
    header: *mut MessageHeader,
) -> i32 {
    if header.is_null() {
        return -1;
    }

    (*header).command = command;
    (*header).arg0 = arg0;
    (*header).arg1 = arg1;
    (*header).data_length = data_length;
    (*header).data_crc32 = data_crc32;
    (*header).magic = !command;

    0
}

// Serialize header to buffer
#[no_mangle]
pub unsafe extern "C" fn dbgif_serialize_header(
    header: *const MessageHeader,
    buffer: *mut u8,
) -> i32 {
    if header.is_null() || buffer.is_null() {
        return -1;
    }

    let header = &*header;
    let buffer_array = slice::from_raw_parts_mut(buffer, MessageHeader::SIZE);

    if let Ok(buffer_ref) = buffer_array.try_into() {
        header.serialize(buffer_ref);
        0
    } else {
        -1
    }
}

// Deserialize header from buffer
#[no_mangle]
pub unsafe extern "C" fn dbgif_deserialize_header(
    buffer: *const u8,
    header: *mut MessageHeader,
) -> i32 {
    if buffer.is_null() || header.is_null() {
        return -1;
    }

    let buffer_slice = slice::from_raw_parts(buffer, MessageHeader::SIZE);

    if let Ok(buffer_array) = <&[u8; MessageHeader::SIZE]>::try_from(buffer_slice) {
        match MessageHeader::deserialize(buffer_array) {
            Ok(parsed_header) => {
                *header = parsed_header;
                0
            }
            Err(e) => e.as_code(),
        }
    } else {
        -1
    }
}

// Initialize encoder with working buffer
#[no_mangle]
pub unsafe extern "C" fn dbgif_encoder_init(
    working_buffer: *mut u8,
    buffer_size: usize,
) -> *mut DbgifEncoder {
    if working_buffer.is_null() || buffer_size < mem::size_of::<DbgifEncoder>() {
        return ptr::null_mut();
    }

    let encoder_ptr = working_buffer as *mut DbgifEncoder;
    let encoder = &mut *encoder_ptr;

    // Initialize the encoder
    #[cfg(feature = "std")]
    {
        encoder.encoder = MessageEncoder::new();
    }

    #[cfg(not(feature = "std"))]
    {
        // For no_std, use the remaining buffer for output storage
        let encoder_size = mem::size_of::<DbgifEncoder>();
        if buffer_size > encoder_size + 256 {
            let output_buffer = working_buffer.add(encoder_size);
            let output_buffer_size = buffer_size - encoder_size;

            // Create a static reference (unsafe but necessary for no_std)
            let static_buffer = slice::from_raw_parts_mut(output_buffer, output_buffer_size);
            let static_ref = &mut *(static_buffer as *mut [u8]);

            encoder.encoder = MessageEncoder::with_static_buffer(static_ref);
        } else {
            return ptr::null_mut();
        }
    }

    encoder_ptr
}

// Encode a message
#[no_mangle]
pub unsafe extern "C" fn dbgif_encode_message(
    encoder: *mut DbgifEncoder,
    command: u32,
    arg0: u32,
    arg1: u32,
    data: *const u8,
    data_len: usize,
    output: *mut *const u8,
    output_len: *mut usize,
) -> i32 {
    if encoder.is_null() || output.is_null() || output_len.is_null() {
        return -1;
    }

    let encoder = &mut *encoder;

    // Convert command
    let cmd = match command {
        x if x == Command::CNXN as u32 => Command::CNXN,
        x if x == Command::OPEN as u32 => Command::OPEN,
        x if x == Command::OKAY as u32 => Command::OKAY,
        x if x == Command::WRTE as u32 => Command::WRTE,
        x if x == Command::CLSE as u32 => Command::CLSE,
        x if x == Command::PING as u32 => Command::PING,
        x if x == Command::PONG as u32 => Command::PONG,
        _ => return -2, // Invalid command
    };

    let data_slice = if !data.is_null() && data_len > 0 {
        slice::from_raw_parts(data, data_len)
    } else {
        &[]
    };

    match encoder.encoder.encode_with_data(cmd, arg0, arg1, data_slice) {
        Ok(encoded) => {
            *output = encoded.as_ptr();
            *output_len = encoded.len();
            0
        }
        Err(_) => -3,
    }
}

// Clear encoder for reuse
#[no_mangle]
pub unsafe extern "C" fn dbgif_encoder_clear(encoder: *mut DbgifEncoder) {
    if encoder.is_null() {
        return;
    }

    let encoder = &mut *encoder;
    encoder.encoder.clear();
}

// Get encoder capacity
#[no_mangle]
pub unsafe extern "C" fn dbgif_encoder_capacity(encoder: *mut DbgifEncoder) -> usize {
    if encoder.is_null() {
        return 0;
    }

    let encoder = &*encoder;
    encoder.encoder.capacity()
}

// FFI-safe state machine handle
#[repr(C)]
pub struct DbgifStateMachine {
    machine: ProtocolStateMachine,
}

// Initialize state machine
#[no_mangle]
pub unsafe extern "C" fn dbgif_state_machine_new() -> *mut DbgifStateMachine {
    let machine = Box::new(DbgifStateMachine {
        machine: ProtocolStateMachine::new(),
    });
    Box::into_raw(machine)
}

// Free state machine
#[no_mangle]
pub unsafe extern "C" fn dbgif_state_machine_free(machine: *mut DbgifStateMachine) {
    if !machine.is_null() {
        let _ = Box::from_raw(machine);
    }
}

// Validate if command can be sent
#[no_mangle]
pub unsafe extern "C" fn dbgif_state_validate_send(
    machine: *mut DbgifStateMachine,
    command: u32,
    arg0: u32,
    arg1: u32,
) -> i32 {
    if machine.is_null() {
        return StateError::InvalidTransition.as_code();
    }

    let machine = &*machine;

    // Convert command
    let cmd = match command {
        x if x == Command::CNXN as u32 => Command::CNXN,
        x if x == Command::OPEN as u32 => Command::OPEN,
        x if x == Command::OKAY as u32 => Command::OKAY,
        x if x == Command::WRTE as u32 => Command::WRTE,
        x if x == Command::CLSE as u32 => Command::CLSE,
        x if x == Command::PING as u32 => Command::PING,
        x if x == Command::PONG as u32 => Command::PONG,
        _ => return StateError::InvalidTransition.as_code(),
    };

    match machine.machine.can_send(cmd, arg0, arg1) {
        Ok(()) => 0,
        Err(e) => e.as_code(),
    }
}

// Update state after sending
#[no_mangle]
pub unsafe extern "C" fn dbgif_state_on_send(
    machine: *mut DbgifStateMachine,
    command: u32,
    arg0: u32,
    arg1: u32,
) -> i32 {
    if machine.is_null() {
        return StateError::InvalidTransition.as_code();
    }

    let machine = &mut *machine;

    // Convert command
    let cmd = match command {
        x if x == Command::CNXN as u32 => Command::CNXN,
        x if x == Command::OPEN as u32 => Command::OPEN,
        x if x == Command::OKAY as u32 => Command::OKAY,
        x if x == Command::WRTE as u32 => Command::WRTE,
        x if x == Command::CLSE as u32 => Command::CLSE,
        x if x == Command::PING as u32 => Command::PING,
        x if x == Command::PONG as u32 => Command::PONG,
        _ => return StateError::InvalidTransition.as_code(),
    };

    match machine.machine.on_send(cmd, arg0, arg1) {
        Ok(()) => 0,
        Err(e) => e.as_code(),
    }
}

// Update state after receiving
#[no_mangle]
pub unsafe extern "C" fn dbgif_state_on_receive(
    machine: *mut DbgifStateMachine,
    command: u32,
    arg0: u32,
    arg1: u32,
) -> i32 {
    if machine.is_null() {
        return StateError::InvalidTransition.as_code();
    }

    let machine = &mut *machine;

    // Convert command
    let cmd = match command {
        x if x == Command::CNXN as u32 => Command::CNXN,
        x if x == Command::OPEN as u32 => Command::OPEN,
        x if x == Command::OKAY as u32 => Command::OKAY,
        x if x == Command::WRTE as u32 => Command::WRTE,
        x if x == Command::CLSE as u32 => Command::CLSE,
        x if x == Command::PING as u32 => Command::PING,
        x if x == Command::PONG as u32 => Command::PONG,
        _ => return StateError::InvalidTransition.as_code(),
    };

    match machine.machine.on_receive(cmd, arg0, arg1) {
        Ok(()) => 0,
        Err(e) => e.as_code(),
    }
}

// Get connection state
#[no_mangle]
pub unsafe extern "C" fn dbgif_state_get_connection(
    machine: *mut DbgifStateMachine,
) -> i32 {
    if machine.is_null() {
        return -1;
    }

    let machine = &*machine;

    match machine.machine.connection_state() {
        ConnectionState::Disconnected => 0,
        ConnectionState::Connecting { .. } => 1,
        ConnectionState::Connected { .. } => 2,
        ConnectionState::Error { .. } => 3,
    }
}

// Check if connected
#[no_mangle]
pub unsafe extern "C" fn dbgif_state_is_connected(
    machine: *mut DbgifStateMachine,
) -> i32 {
    if machine.is_null() {
        return 0;
    }

    let machine = &*machine;
    if machine.machine.is_connected() { 1 } else { 0 }
}

// Reset state machine
#[no_mangle]
pub unsafe extern "C" fn dbgif_state_reset(
    machine: *mut DbgifStateMachine,
) {
    if machine.is_null() {
        return;
    }

    let machine = &mut *machine;
    machine.machine.reset();
}