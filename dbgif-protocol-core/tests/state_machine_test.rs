#![cfg(feature = "std")]

use dbgif_protocol_core::*;

#[test]
fn test_cnxn_handshake() {
    let mut machine = ProtocolStateMachine::new();

    // Initial state should be disconnected
    assert_eq!(machine.connection_state(), ConnectionState::Disconnected);
    assert!(!machine.is_connected());

    // Start connection - send CNXN
    assert!(machine.can_send(Command::CNXN, 0x01000000, 256*1024).is_ok());
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Should be in connecting state
    assert!(matches!(
        machine.connection_state(),
        ConnectionState::Connecting { version: 0x01000000, max_payload: 262144 }
    ));
    assert!(!machine.is_connected());

    // Receive CNXN response
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Should be connected
    assert!(matches!(
        machine.connection_state(),
        ConnectionState::Connected { .. }
    ));
    assert!(machine.is_connected());
}

#[test]
fn test_stream_lifecycle() {
    let mut machine = ProtocolStateMachine::new();

    // Connect first
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Open a stream
    let local_id = 1;
    assert!(machine.can_send(Command::OPEN, local_id, 0).is_ok());
    machine.on_send(Command::OPEN, local_id, 0).unwrap();

    // Stream should be opening
    let stream = machine.get_stream(local_id);
    assert!(matches!(stream, Some(StreamState::Opening { local_id: 1 })));

    // Cannot write to opening stream
    assert_eq!(
        machine.can_send(Command::WRTE, local_id, 0),
        Err(StateError::StreamNotFound)
    );

    // Receive OKAY with remote stream ID
    let remote_id = 100;
    machine.on_receive(Command::OKAY, remote_id, local_id).unwrap();

    // Stream should be active
    let stream = machine.get_stream(local_id);
    assert!(matches!(
        stream,
        Some(StreamState::Active { local_id: 1, remote_id: 100 })
    ));

    // Now can write
    assert!(machine.can_send(Command::WRTE, local_id, remote_id).is_ok());
    machine.on_send(Command::WRTE, local_id, remote_id).unwrap();

    // Close the stream
    assert!(machine.can_send(Command::CLSE, local_id, remote_id).is_ok());
    machine.on_send(Command::CLSE, local_id, remote_id).unwrap();

    // Stream should be closing
    let stream = machine.get_stream(local_id);
    assert!(matches!(
        stream,
        Some(StreamState::Closing { local_id: 1, remote_id: 100 })
    ));

    // Receive CLSE acknowledgment
    machine.on_receive(Command::CLSE, remote_id, local_id).unwrap();

    // Stream should be removed
    assert!(machine.get_stream(local_id).is_none());
}

#[test]
fn test_invalid_commands_when_disconnected() {
    let machine = ProtocolStateMachine::new();

    // Cannot send these commands when disconnected
    assert_eq!(machine.can_send(Command::OPEN, 1, 0), Err(StateError::NotConnected));
    assert_eq!(machine.can_send(Command::WRTE, 1, 0), Err(StateError::NotConnected));
    assert_eq!(machine.can_send(Command::CLSE, 1, 0), Err(StateError::NotConnected));
    assert_eq!(machine.can_send(Command::PING, 0, 0), Err(StateError::NotConnected));
    assert_eq!(machine.can_send(Command::PONG, 0, 0), Err(StateError::NotConnected));

    // Can send CNXN
    assert!(machine.can_send(Command::CNXN, 0x01000000, 256*1024).is_ok());
}

#[test]
fn test_cannot_double_connect() {
    let mut machine = ProtocolStateMachine::new();

    // First connection
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Cannot send CNXN when already connected
    assert_eq!(
        machine.can_send(Command::CNXN, 0x01000000, 256*1024),
        Err(StateError::AlreadyConnected)
    );
}

#[test]
fn test_multiple_concurrent_streams() {
    let mut machine = ProtocolStateMachine::new();

    // Connect
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Open multiple streams
    let streams = vec![
        (1, 100),  // (local_id, remote_id)
        (2, 200),
        (3, 300),
    ];

    for (local_id, remote_id) in &streams {
        // Open stream
        machine.on_send(Command::OPEN, *local_id, 0).unwrap();
        machine.on_receive(Command::OKAY, *remote_id, *local_id).unwrap();

        // Verify it's active
        assert!(matches!(
            machine.get_stream(*local_id),
            Some(StreamState::Active { .. })
        ));
    }

    // Should have 3 active streams
    assert_eq!(machine.stream_count(), 3);

    // Close middle stream
    machine.on_send(Command::CLSE, 2, 200).unwrap();
    machine.on_receive(Command::CLSE, 200, 2).unwrap();

    // Should have 2 streams now
    assert_eq!(machine.stream_count(), 2);
    assert!(machine.get_stream(1).is_some());
    assert!(machine.get_stream(2).is_none());
    assert!(machine.get_stream(3).is_some());
}

#[test]
fn test_ping_pong_requires_connection() {
    let mut machine = ProtocolStateMachine::new();

    // Cannot ping when disconnected
    assert_eq!(machine.can_send(Command::PING, 0, 0), Err(StateError::NotConnected));
    assert_eq!(machine.can_send(Command::PONG, 0, 0), Err(StateError::NotConnected));

    // Connect
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Now can ping/pong
    assert!(machine.can_send(Command::PING, 0x12345678, 0xABCD).is_ok());
    assert!(machine.can_send(Command::PONG, 0x12345678, 0xABCD).is_ok());
}

#[test]
fn test_stream_cannot_be_opened_twice() {
    let mut machine = ProtocolStateMachine::new();

    // Connect
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Open stream 1
    machine.on_send(Command::OPEN, 1, 0).unwrap();

    // Cannot open same stream again
    assert_eq!(
        machine.can_send(Command::OPEN, 1, 0),
        Err(StateError::StreamAlreadyActive)
    );

    // Even after it's active
    machine.on_receive(Command::OKAY, 100, 1).unwrap();
    assert_eq!(
        machine.can_send(Command::OPEN, 1, 0),
        Err(StateError::StreamAlreadyActive)
    );
}

#[test]
fn test_write_requires_active_stream() {
    let mut machine = ProtocolStateMachine::new();

    // Connect
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Cannot write to non-existent stream
    assert_eq!(
        machine.can_send(Command::WRTE, 1, 0),
        Err(StateError::StreamNotFound)
    );

    // Open stream but don't wait for OKAY
    machine.on_send(Command::OPEN, 1, 0).unwrap();

    // Still cannot write (stream is Opening, not Active)
    assert_eq!(
        machine.can_send(Command::WRTE, 1, 0),
        Err(StateError::StreamNotFound)
    );

    // Receive OKAY
    machine.on_receive(Command::OKAY, 100, 1).unwrap();

    // Now can write
    assert!(machine.can_send(Command::WRTE, 1, 100).is_ok());
}

#[test]
fn test_close_can_happen_during_opening() {
    let mut machine = ProtocolStateMachine::new();

    // Connect
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Open stream
    machine.on_send(Command::OPEN, 1, 0).unwrap();

    // Can close stream even while opening
    assert!(machine.can_send(Command::CLSE, 1, 0).is_ok());
    machine.on_send(Command::CLSE, 1, 0).unwrap();

    // Stream should be in closing state
    assert!(matches!(
        machine.get_stream(1),
        Some(StreamState::Closing { local_id: 1, remote_id: 0 })
    ));
}

#[test]
fn test_state_machine_reset() {
    let mut machine = ProtocolStateMachine::new();

    // Set up connection and streams
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    machine.on_send(Command::OPEN, 1, 0).unwrap();
    machine.on_receive(Command::OKAY, 100, 1).unwrap();

    machine.on_send(Command::OPEN, 2, 0).unwrap();
    machine.on_receive(Command::OKAY, 200, 2).unwrap();

    assert!(machine.is_connected());
    assert_eq!(machine.stream_count(), 2);

    // Reset
    machine.reset();

    // Should be back to initial state
    assert_eq!(machine.connection_state(), ConnectionState::Disconnected);
    assert!(!machine.is_connected());
    assert_eq!(machine.stream_count(), 0);
    assert!(machine.get_stream(1).is_none());
    assert!(machine.get_stream(2).is_none());
}

#[test]
fn test_receive_unsolicited_cnxn() {
    let mut machine = ProtocolStateMachine::new();

    // Receive CNXN without sending (server side behavior)
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Should be in connecting state
    assert!(matches!(
        machine.connection_state(),
        ConnectionState::Connecting { .. }
    ));

    // Send CNXN response
    machine.on_send(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Should still be connecting until we receive another CNXN
    machine.on_receive(Command::CNXN, 0x01000000, 256*1024).unwrap();

    // Now should be connected
    assert!(machine.is_connected());
}