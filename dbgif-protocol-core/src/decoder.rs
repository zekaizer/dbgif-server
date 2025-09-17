use crate::crc::Crc32State;
use crate::error::DecodeError;
use crate::header::MessageHeader;

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub enum DecoderState {
    WaitingForHeader,
    AccumulatingHeader { received: u8 },
    WaitingForData,
    AccumulatingData { received: u32 },
    MessageComplete,
    Error { code: DecodeError },
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub enum DecodeResult {
    NeedMoreData,
    Complete,
    Error { code: DecodeError },
}

pub struct MessageDecoder {
    state: DecoderState,
    header_buffer: [u8; MessageHeader::SIZE],
    header_bytes_received: u8,
    current_header: MessageHeader,
    data_bytes_received: u32,
    crc_hasher: Crc32State,
    // Data accumulator for collecting message data
    #[cfg(feature = "std")]
    data_accumulator: Option<Vec<u8>>,
    #[cfg(not(feature = "std"))]
    data_accumulator: Option<&'static mut [u8]>,
    data_accumulated: usize,
}

impl MessageDecoder {
    pub fn new() -> Self {
        Self {
            state: DecoderState::WaitingForHeader,
            header_buffer: [0; MessageHeader::SIZE],
            header_bytes_received: 0,
            current_header: MessageHeader::new(),
            data_bytes_received: 0,
            crc_hasher: Crc32State::new(),
            #[cfg(feature = "std")]
            data_accumulator: None,
            #[cfg(not(feature = "std"))]
            data_accumulator: None,
            data_accumulated: 0,
        }
    }

    #[cfg(feature = "std")]
    pub fn with_data_accumulator(mut self) -> Self {
        self.data_accumulator = Some(Vec::new());
        self
    }

    #[cfg(not(feature = "std"))]
    pub fn with_static_buffer(mut self, buffer: &'static mut [u8]) -> Self {
        self.data_accumulator = Some(buffer);
        self.data_accumulated = 0;
        self
    }

    pub fn reset(&mut self) {
        self.state = DecoderState::WaitingForHeader;
        self.header_bytes_received = 0;
        self.data_bytes_received = 0;
        self.crc_hasher.reset();
        self.data_accumulated = 0;
        #[cfg(feature = "std")]
        if let Some(ref mut acc) = self.data_accumulator {
            acc.clear();
        }
    }

    pub fn get_accumulated_data(&self) -> Option<&[u8]> {
        #[cfg(feature = "std")]
        {
            self.data_accumulator.as_deref()
        }
        #[cfg(not(feature = "std"))]
        {
            self.data_accumulator.as_ref().map(|buf| &buf[..self.data_accumulated])
        }
    }

    pub fn feed(&mut self, data: &[u8]) -> (DecodeResult, usize) {
        let mut consumed = 0;
        let mut remaining = data;

        loop {
            match self.state {
                DecoderState::WaitingForHeader | DecoderState::AccumulatingHeader { .. } => {
                    let needed = (MessageHeader::SIZE - self.header_bytes_received as usize).min(remaining.len());

                    let start = self.header_bytes_received as usize;
                    self.header_buffer[start..start + needed].copy_from_slice(&remaining[..needed]);

                    self.header_bytes_received += needed as u8;
                    consumed += needed;
                    remaining = &remaining[needed..];

                    if self.header_bytes_received == MessageHeader::SIZE as u8 {
                        // Parse header
                        match MessageHeader::deserialize(&self.header_buffer) {
                            Ok(header) => {
                                self.current_header = header;
                                if header.data_length > 0 {
                                    self.state = DecoderState::WaitingForData;
                                    self.data_bytes_received = 0;
                                    self.crc_hasher.reset();
                                } else {
                                    // No data, message complete
                                    if header.data_crc32 == 0 {
                                        self.state = DecoderState::MessageComplete;
                                        return (DecodeResult::Complete, consumed);
                                    } else {
                                        self.state = DecoderState::Error { code: DecodeError::CrcMismatch };
                                        return (DecodeResult::Error { code: DecodeError::CrcMismatch }, consumed);
                                    }
                                }
                            }
                            Err(e) => {
                                self.state = DecoderState::Error { code: e };
                                return (DecodeResult::Error { code: e }, consumed);
                            }
                        }
                    } else {
                        self.state = DecoderState::AccumulatingHeader { received: self.header_bytes_received };
                        if remaining.is_empty() {
                            return (DecodeResult::NeedMoreData, consumed);
                        }
                    }
                }

                DecoderState::WaitingForData | DecoderState::AccumulatingData { .. } => {
                    let needed = ((self.current_header.data_length - self.data_bytes_received) as usize).min(remaining.len());

                    // Update CRC incrementally
                    self.crc_hasher.update(&remaining[..needed]);

                    // Accumulate data if accumulator is enabled
                    #[cfg(feature = "std")]
                    if let Some(ref mut accumulator) = self.data_accumulator {
                        accumulator.extend_from_slice(&remaining[..needed]);
                    }

                    #[cfg(not(feature = "std"))]
                    if let Some(buffer) = self.data_accumulator.as_mut() {
                        let available = buffer.len().saturating_sub(self.data_accumulated);
                        let to_copy = needed.min(available);
                        if to_copy > 0 {
                            buffer[self.data_accumulated..self.data_accumulated + to_copy]
                                .copy_from_slice(&remaining[..to_copy]);
                            self.data_accumulated += to_copy;
                        }
                    }

                    self.data_bytes_received += needed as u32;
                    consumed += needed;
                    remaining = &remaining[needed..];

                    if self.data_bytes_received == self.current_header.data_length {
                        // Verify CRC
                        let calculated_crc = self.crc_hasher.finalize();
                        if calculated_crc == self.current_header.data_crc32 {
                            self.state = DecoderState::MessageComplete;
                            return (DecodeResult::Complete, consumed);
                        } else {
                            self.state = DecoderState::Error { code: DecodeError::CrcMismatch };
                            return (DecodeResult::Error { code: DecodeError::CrcMismatch }, consumed);
                        }
                    } else {
                        self.state = DecoderState::AccumulatingData { received: self.data_bytes_received };
                        if remaining.is_empty() {
                            return (DecodeResult::NeedMoreData, consumed);
                        }
                    }
                }

                DecoderState::MessageComplete => {
                    return (DecodeResult::Complete, consumed);
                }

                DecoderState::Error { code } => {
                    return (DecodeResult::Error { code }, consumed);
                }
            }
        }
    }

    pub fn get_state(&self) -> DecoderState {
        self.state
    }

    pub fn get_header(&self) -> Option<&MessageHeader> {
        match self.state {
            DecoderState::WaitingForData | DecoderState::AccumulatingData { .. } | DecoderState::MessageComplete => {
                Some(&self.current_header)
            }
            _ => None,
        }
    }

    pub fn bytes_needed(&self) -> usize {
        match self.state {
            DecoderState::WaitingForHeader | DecoderState::AccumulatingHeader { .. } => {
                MessageHeader::SIZE - self.header_bytes_received as usize
            }
            DecoderState::WaitingForData | DecoderState::AccumulatingData { .. } => {
                (self.current_header.data_length - self.data_bytes_received) as usize
            }
            _ => 0,
        }
    }

    pub fn bytes_received(&self) -> usize {
        match self.state {
            DecoderState::WaitingForHeader | DecoderState::AccumulatingHeader { .. } => {
                self.header_bytes_received as usize
            }
            DecoderState::WaitingForData | DecoderState::AccumulatingData { .. } => {
                MessageHeader::SIZE + self.data_bytes_received as usize
            }
            DecoderState::MessageComplete => {
                MessageHeader::SIZE + self.current_header.data_length as usize
            }
            _ => 0,
        }
    }
}

