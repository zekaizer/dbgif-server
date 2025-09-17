use crate::commands::Command;
use crate::error::StateError;

#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(not(feature = "std"))]
const MAX_STREAMS: usize = 16;

/// Connection state for the protocol
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// CNXN sent, waiting for response
    Connecting { version: u32, max_payload: u32 },
    /// Fully connected with CNXN handshake complete
    Connected {
        remote_version: u32,
        remote_max_payload: u32,
        connect_id: u32,
    },
    /// Connection error
    Error { code: StateError },
}

/// Stream state for multiplexed channels
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StreamState {
    /// No stream
    Idle,
    /// OPEN sent, waiting for OKAY
    Opening { local_id: u32 },
    /// Stream active, can send WRTE
    Active { local_id: u32, remote_id: u32 },
    /// CLSE sent, waiting for CLSE response
    Closing { local_id: u32, remote_id: u32 },
    /// Stream closed
    Closed,
}

/// Protocol state machine for connection and stream management
pub struct ProtocolStateMachine {
    connection: ConnectionState,
    #[cfg(feature = "std")]
    streams: HashMap<u32, StreamState>,
    #[cfg(not(feature = "std"))]
    streams: [(u32, StreamState); MAX_STREAMS],
    #[cfg(not(feature = "std"))]
    stream_count: usize,
}

impl ProtocolStateMachine {
    /// Create a new state machine
    pub fn new() -> Self {
        Self {
            connection: ConnectionState::Disconnected,
            #[cfg(feature = "std")]
            streams: HashMap::new(),
            #[cfg(not(feature = "std"))]
            streams: [(0, StreamState::Idle); MAX_STREAMS],
            #[cfg(not(feature = "std"))]
            stream_count: 0,
        }
    }

    /// Get current connection state
    pub fn connection_state(&self) -> ConnectionState {
        self.connection
    }

    /// Check if we're connected
    pub fn is_connected(&self) -> bool {
        matches!(self.connection, ConnectionState::Connected { .. })
    }

    /// Validate if a command can be sent in current state
    pub fn can_send(&self, cmd: Command, arg0: u32, _arg1: u32) -> Result<(), StateError> {
        match cmd {
            Command::CNXN => {
                match self.connection {
                    ConnectionState::Disconnected => Ok(()),
                    ConnectionState::Connecting { .. } => Ok(()), // Can resend CNXN
                    _ => Err(StateError::AlreadyConnected),
                }
            }
            Command::OPEN => {
                if !self.is_connected() {
                    return Err(StateError::NotConnected);
                }

                // Check if stream already exists
                if self.get_stream(arg0).is_some() {
                    return Err(StateError::StreamAlreadyActive);
                }

                #[cfg(not(feature = "std"))]
                {
                    if self.stream_count >= MAX_STREAMS {
                        return Err(StateError::TooManyStreams);
                    }
                }

                Ok(())
            }
            Command::WRTE | Command::CLSE => {
                if !self.is_connected() {
                    return Err(StateError::NotConnected);
                }

                match self.get_stream(arg0) {
                    Some(StreamState::Active { .. }) => Ok(()),
                    Some(StreamState::Opening { .. }) if cmd == Command::CLSE => Ok(()),
                    _ => Err(StateError::StreamNotFound),
                }
            }
            Command::OKAY => {
                // OKAY is typically a response, not initiated
                if !self.is_connected() {
                    return Err(StateError::NotConnected);
                }
                Ok(())
            }
            Command::PING => {
                if !self.is_connected() {
                    return Err(StateError::NotConnected);
                }
                Ok(())
            }
            Command::PONG => {
                // PONG is always a response to PING
                if !self.is_connected() {
                    return Err(StateError::NotConnected);
                }
                Ok(())
            }
        }
    }

    /// Update state after sending a command
    pub fn on_send(&mut self, cmd: Command, arg0: u32, arg1: u32) -> Result<(), StateError> {
        // First validate
        self.can_send(cmd, arg0, arg1)?;

        match cmd {
            Command::CNXN => {
                if matches!(self.connection, ConnectionState::Disconnected) {
                    self.connection = ConnectionState::Connecting {
                        version: arg0,
                        max_payload: arg1,
                    };
                }
            }
            Command::OPEN => {
                self.add_stream(arg0, StreamState::Opening { local_id: arg0 });
            }
            Command::CLSE => {
                if let Some(state) = self.get_stream(arg0) {
                    match state {
                        StreamState::Active { local_id, remote_id } => {
                            self.update_stream(*local_id, StreamState::Closing {
                                local_id: *local_id,
                                remote_id: *remote_id
                            });
                        }
                        StreamState::Opening { local_id } => {
                            self.update_stream(*local_id, StreamState::Closing {
                                local_id: *local_id,
                                remote_id: 0
                            });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Update state after receiving a command
    pub fn on_receive(&mut self, cmd: Command, arg0: u32, arg1: u32) -> Result<(), StateError> {
        match cmd {
            Command::CNXN => {
                match self.connection {
                    ConnectionState::Connecting { .. } => {
                        // First CNXN response - need to extract connect_id from banner
                        // For now, use arg0 as a placeholder
                        self.connection = ConnectionState::Connected {
                            remote_version: arg0,
                            remote_max_payload: arg1,
                            connect_id: 0, // Will be extracted from banner
                        };
                    }
                    ConnectionState::Disconnected => {
                        // Unsolicited CNXN - become connecting
                        self.connection = ConnectionState::Connecting {
                            version: arg0,
                            max_payload: arg1,
                        };
                    }
                    _ => {}
                }
            }
            Command::OKAY => {
                // OKAY response to our OPEN
                // arg0 is remote stream ID, arg1 is our local stream ID
                if let Some(state) = self.get_stream(arg1) {
                    if matches!(state, StreamState::Opening { .. }) {
                        self.update_stream(arg1, StreamState::Active {
                            local_id: arg1,
                            remote_id: arg0,
                        });
                    }
                }
            }
            Command::CLSE => {
                // Remote side closing stream
                // arg0 is remote stream ID, arg1 is our local stream ID
                if self.get_stream(arg1).is_some() {
                    self.remove_stream(arg1);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Get stream state by local ID
    pub fn get_stream(&self, local_id: u32) -> Option<&StreamState> {
        #[cfg(feature = "std")]
        {
            self.streams.get(&local_id)
        }

        #[cfg(not(feature = "std"))]
        {
            self.streams.iter()
                .find(|(id, _)| *id == local_id)
                .map(|(_, state)| state)
        }
    }

    /// Add a new stream
    fn add_stream(&mut self, local_id: u32, state: StreamState) {
        #[cfg(feature = "std")]
        {
            self.streams.insert(local_id, state);
        }

        #[cfg(not(feature = "std"))]
        {
            if self.stream_count < MAX_STREAMS {
                self.streams[self.stream_count] = (local_id, state);
                self.stream_count += 1;
            }
        }
    }

    /// Update existing stream
    fn update_stream(&mut self, local_id: u32, state: StreamState) {
        #[cfg(feature = "std")]
        {
            self.streams.insert(local_id, state);
        }

        #[cfg(not(feature = "std"))]
        {
            for i in 0..self.stream_count {
                if self.streams[i].0 == local_id {
                    self.streams[i].1 = state;
                    break;
                }
            }
        }
    }

    /// Remove a stream
    fn remove_stream(&mut self, local_id: u32) {
        #[cfg(feature = "std")]
        {
            self.streams.remove(&local_id);
        }

        #[cfg(not(feature = "std"))]
        {
            let mut found = false;
            for i in 0..self.stream_count {
                if found {
                    // Shift remaining elements
                    if i > 0 {
                        self.streams[i - 1] = self.streams[i];
                    }
                } else if self.streams[i].0 == local_id {
                    found = true;
                }
            }
            if found && self.stream_count > 0 {
                self.stream_count -= 1;
                self.streams[self.stream_count] = (0, StreamState::Idle);
            }
        }
    }

    /// Reset the state machine
    pub fn reset(&mut self) {
        self.connection = ConnectionState::Disconnected;

        #[cfg(feature = "std")]
        {
            self.streams.clear();
        }

        #[cfg(not(feature = "std"))]
        {
            self.streams = [(0, StreamState::Idle); MAX_STREAMS];
            self.stream_count = 0;
        }
    }

    /// Get the number of active streams
    pub fn stream_count(&self) -> usize {
        #[cfg(feature = "std")]
        {
            self.streams.len()
        }

        #[cfg(not(feature = "std"))]
        {
            self.stream_count
        }
    }
}

impl Default for ProtocolStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_transitions() {
        let mut machine = ProtocolStateMachine::new();

        // Initial state
        assert_eq!(machine.connection_state(), ConnectionState::Disconnected);

        // Can send CNXN when disconnected
        assert!(machine.can_send(Command::CNXN, 0x01000000, 256*1024).is_ok());

        // Cannot send other commands when disconnected
        assert!(machine.can_send(Command::OPEN, 1, 0).is_err());
        assert!(machine.can_send(Command::WRTE, 1, 0).is_err());

        // Send CNXN
        machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
        assert!(matches!(machine.connection_state(), ConnectionState::Connecting { .. }));

        // Receive CNXN response
        machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();
        assert!(matches!(machine.connection_state(), ConnectionState::Connected { .. }));

        // Now can send other commands
        assert!(machine.can_send(Command::OPEN, 1, 0).is_ok());
        assert!(machine.can_send(Command::PING, 0, 0).is_ok());
    }

    #[test]
    fn test_stream_state_transitions() {
        let mut machine = ProtocolStateMachine::new();

        // Connect first
        machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
        machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

        // Open stream
        assert!(machine.can_send(Command::OPEN, 1, 0).is_ok());
        machine.on_send(Command::OPEN, 1, 0).unwrap();

        // Stream is opening
        assert!(matches!(
            machine.get_stream(1),
            Some(StreamState::Opening { local_id: 1 })
        ));

        // Cannot write to opening stream
        assert!(machine.can_send(Command::WRTE, 1, 0).is_err());

        // Receive OKAY
        machine.on_receive(Command::OKAY, 100, 1).unwrap();

        // Stream is active
        assert!(matches!(
            machine.get_stream(1),
            Some(StreamState::Active { local_id: 1, remote_id: 100 })
        ));

        // Now can write
        assert!(machine.can_send(Command::WRTE, 1, 100).is_ok());

        // Close stream
        assert!(machine.can_send(Command::CLSE, 1, 100).is_ok());
        machine.on_send(Command::CLSE, 1, 100).unwrap();

        // Stream is closing
        assert!(matches!(
            machine.get_stream(1),
            Some(StreamState::Closing { .. })
        ));

        // Receive CLSE
        machine.on_receive(Command::CLSE, 100, 1).unwrap();

        // Stream removed
        assert!(machine.get_stream(1).is_none());
    }

    #[test]
    fn test_invalid_transitions() {
        let mut machine = ProtocolStateMachine::new();

        // Cannot send OPEN when not connected
        assert_eq!(
            machine.can_send(Command::OPEN, 1, 0),
            Err(StateError::NotConnected)
        );

        // Connect
        machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
        machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

        // Cannot send CNXN when already connected
        assert_eq!(
            machine.can_send(Command::CNXN, 0x01000000, 256*1024),
            Err(StateError::AlreadyConnected)
        );

        // Cannot write to non-existent stream
        assert_eq!(
            machine.can_send(Command::WRTE, 999, 0),
            Err(StateError::StreamNotFound)
        );

        // Open stream
        machine.on_send(Command::OPEN, 1, 0).unwrap();

        // Cannot open same stream again
        assert_eq!(
            machine.can_send(Command::OPEN, 1, 0),
            Err(StateError::StreamAlreadyActive)
        );
    }

    #[test]
    fn test_multiple_streams() {
        let mut machine = ProtocolStateMachine::new();

        // Connect
        machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
        machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

        // Open multiple streams
        for i in 1..=3 {
            machine.on_send(Command::OPEN, i, 0).unwrap();
            machine.on_receive(Command::OKAY, 100 + i, i).unwrap();
        }

        assert_eq!(machine.stream_count(), 3);

        // Close one stream
        machine.on_send(Command::CLSE, 2, 102).unwrap();
        machine.on_receive(Command::CLSE, 102, 2).unwrap();

        assert_eq!(machine.stream_count(), 2);
        assert!(machine.get_stream(1).is_some());
        assert!(machine.get_stream(2).is_none());
        assert!(machine.get_stream(3).is_some());
    }
}