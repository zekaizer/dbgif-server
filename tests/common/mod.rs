// Common test helpers for integration tests

use dbgif_protocol::protocol::message::AdbMessage;
use dbgif_protocol::protocol::commands::AdbCommand;
use dbgif_protocol::protocol::crc::calculate_crc32;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::net::SocketAddr;

/// Start a mock DBGIF server for testing
#[allow(dead_code)]
pub async fn start_test_dbgif_server() -> SocketAddr {
    // For now, create a mock server that just returns a valid address
    // TODO: Implement real server startup for integration testing

    // Bind to ephemeral port to get a valid address
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    // Start a simple echo server for testing
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0; 1024];
                while let Ok(n) = stream.read(&mut buffer).await {
                    if n == 0 { break; }
                    let _ = stream.write_all(&buffer[..n]).await;
                }
            });
        }
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    server_addr
}

/// Start a mock device daemon for testing
#[allow(dead_code)]
pub async fn start_test_device_daemon() -> SocketAddr {
    // Similar to server, create a mock device daemon
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let device_addr = listener.local_addr().unwrap();

    // Start a simple echo device daemon for testing
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0; 1024];
                while let Ok(n) = stream.read(&mut buffer).await {
                    if n == 0 { break; }
                    let _ = stream.write_all(&buffer[..n]).await;
                }
            });
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    device_addr
}

/// Stop a test device daemon (placeholder)
#[allow(dead_code)]
pub async fn stop_test_device_daemon(_device_addr: SocketAddr) {
    // TODO: Implement proper cleanup if needed
}

/// Establish CNXN handshake with server
#[allow(dead_code)]
pub async fn establish_cnxn_handshake(server_addr: SocketAddr) -> TcpStream {
    // Connect to server
    let mut stream = TcpStream::connect(server_addr).await.unwrap();

    // Send CNXN handshake message
    let data = b"host::integration-test";
    let cnxn_msg = AdbMessage {
        command: AdbCommand::CNXN as u32,
        arg0: 0x01000000, // version
        arg1: 1024 * 1024, // max data size
        data_length: data.len() as u32,
        data_crc32: calculate_crc32(data),
        magic: !(AdbCommand::CNXN as u32),
        data: data.to_vec(),
    };

    send_adb_message(&mut stream, &cnxn_msg).await.unwrap();

    // Receive CNXN response
    let _response = receive_adb_message(&mut stream).await.unwrap();

    stream
}

/// Establish a stream with the server
#[allow(dead_code)]
pub async fn establish_stream(client_stream: &mut TcpStream, service_name: &[u8]) -> (u32, u32) {
    let local_id = 1;
    let remote_id = 100;

    // Send OPEN message
    let open_msg = AdbMessage {
        command: AdbCommand::OPEN as u32,
        arg0: local_id,
        arg1: 0, // remote_id (assigned by server)
        data_length: service_name.len() as u32,
        data_crc32: calculate_crc32(service_name),
        magic: !(AdbCommand::OPEN as u32),
        data: service_name.to_vec(),
    };

    send_adb_message(client_stream, &open_msg).await.unwrap();

    // Receive OKAY response
    let _response = receive_adb_message(client_stream).await.unwrap();

    (local_id, remote_id)
}

/// Send an ADB message over TCP stream
#[allow(dead_code)]
pub async fn send_adb_message(stream: &mut TcpStream, message: &AdbMessage) -> Result<(), Box<dyn std::error::Error>> {
    // Serialize and send the ADB message
    let mut buffer = Vec::with_capacity(24 + message.data.len());

    // Serialize header (24 bytes)
    buffer.extend_from_slice(&message.command.to_le_bytes());
    buffer.extend_from_slice(&message.arg0.to_le_bytes());
    buffer.extend_from_slice(&message.arg1.to_le_bytes());
    buffer.extend_from_slice(&message.data_length.to_le_bytes());
    buffer.extend_from_slice(&message.data_crc32.to_le_bytes());
    buffer.extend_from_slice(&message.magic.to_le_bytes());

    // Add data payload
    buffer.extend_from_slice(&message.data);

    stream.write_all(&buffer).await?;
    stream.flush().await?;

    Ok(())
}

/// Receive an ADB message from TCP stream
#[allow(dead_code)]
pub async fn receive_adb_message(stream: &mut TcpStream) -> Result<AdbMessage, Box<dyn std::error::Error>> {
    // Read header (24 bytes)
    let mut header = [0u8; 24];
    stream.read_exact(&mut header).await?;

    // Parse header
    let command = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let arg0 = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let arg1 = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    let data_length = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
    let data_crc32 = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
    let magic = u32::from_le_bytes([header[20], header[21], header[22], header[23]]);

    // Read data payload
    let mut data = vec![0u8; data_length as usize];
    if data_length > 0 {
        stream.read_exact(&mut data).await?;
    }

    Ok(AdbMessage {
        command,
        arg0,
        arg1,
        data_length,
        data_crc32,
        magic,
        data,
    })
}

/// Serialize message header to bytes
#[allow(dead_code)]
pub fn serialize_message_header(message: &AdbMessage) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(24);

    buffer.extend_from_slice(&message.command.to_le_bytes());
    buffer.extend_from_slice(&message.arg0.to_le_bytes());
    buffer.extend_from_slice(&message.arg1.to_le_bytes());
    buffer.extend_from_slice(&message.data_length.to_le_bytes());
    buffer.extend_from_slice(&message.data_crc32.to_le_bytes());
    buffer.extend_from_slice(&message.magic.to_le_bytes());

    buffer
}

/// Calculate CRC32 of data
#[allow(dead_code)]
pub fn calculate_data_crc32(data: &[u8]) -> u32 {
    calculate_crc32(data)
}