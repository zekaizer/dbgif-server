use anyhow::Result;
use dbgif_protocol::{
    commands::AdbCommand,
    message::AdbMessage,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Device server implementation that simulates a DBGIF device
pub struct DeviceServer {
    device_id: String,
    device_model: String,
    capabilities: Vec<String>,
    handle: Option<JoinHandle<Result<()>>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    port: u16,
}

impl DeviceServer {
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            device_model: "DBGIF-TestDevice".to_string(),
            capabilities: vec!["shell".to_string(), "file".to_string(), "debug".to_string()],
            handle: None,
            shutdown_tx: None,
            port: 0,
        }
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.device_model = model;
        self
    }

    pub fn with_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Start the device server
    pub async fn start(&mut self, port: Option<u16>) -> Result<()> {
        let bind_addr: SocketAddr = format!("127.0.0.1:{}", port.unwrap_or(0)).parse()?;
        let listener = TcpListener::bind(bind_addr).await?;
        self.port = listener.local_addr()?.port();

        info!("Device server {} listening on port {}", self.device_id, self.port);

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        let device_id = self.device_id.clone();
        let device_model = self.device_model.clone();

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, addr)) => {
                                info!("Device {} accepted connection from {}", device_id, addr);
                                let device_id_clone = device_id.clone();
                                let device_model_clone = device_model.clone();

                                tokio::spawn(async move {
                                    if let Err(e) = handle_connection(stream, device_id_clone, device_model_clone).await {
                                        error!("Connection error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Accept error: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Device server {} shutting down", device_id);
                        break;
                    }
                }
            }
            Ok(())
        });

        self.handle = Some(handle);
        Ok(())
    }

    /// Stop the device server
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        if let Some(handle) = self.handle.take() {
            handle.abort();
        }

        info!("Device server {} stopped", self.device_id);
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }
}

/// Handle a single connection to the device
async fn handle_connection(
    mut stream: TcpStream,
    device_id: String,
    device_model: String,
) -> Result<()> {
    debug!("Handling connection for device {}", device_id);

    // Simple CNXN handshake simulation
    let connect_string = format!("device:{}:model={}", device_id, device_model);
    let cnxn = AdbMessage::new_cnxn(0x01000000, 4096, connect_string.into_bytes());

    let buffer = cnxn.serialize();
    stream.write_all(&buffer).await?;

    // Connection handling loop
    let mut message_buffer = vec![0u8; 4096];
    let mut streams: HashMap<u32, String> = HashMap::new();
    let mut next_local_id = 1u32;

    loop {
        let n = stream.read(&mut message_buffer).await?;
        if n == 0 {
            debug!("Connection closed for device {}", device_id);
            break;
        }

        // Try to parse ADB message
        if n >= 24 {
            match AdbMessage::deserialize(&message_buffer[..n]) {
                Ok(msg) => {
                    debug!("Device {} received: {:?}", device_id, msg.command);

                    match msg.command {
                        x if x == AdbCommand::CNXN as u32 => {
                            // Already sent CNXN, this is server's CNXN
                            debug!("Received CNXN from server");
                        }
                        x if x == AdbCommand::OPEN as u32 => {
                            // Handle OPEN command
                            let service = String::from_utf8_lossy(&msg.data);
                            let local_id = next_local_id;
                            next_local_id += 1;

                            streams.insert(local_id, service.to_string());

                            // Send OKAY response
                            let okay = AdbMessage::new_okay(local_id, msg.arg0);
                            let response = okay.serialize();
                            stream.write_all(&response).await?;

                            debug!("Opened stream {} for service: {}", local_id, service);
                        }
                        x if x == AdbCommand::WRTE as u32 => {
                            // Echo back the data (simple echo service)
                            let echo_msg = AdbMessage::new_wrte(msg.arg1, msg.arg0, msg.data.clone());
                            let response = echo_msg.serialize();
                            stream.write_all(&response).await?;

                            // Send OKAY
                            let okay = AdbMessage::new_okay(msg.arg1, msg.arg0);
                            let okay_response = okay.serialize();
                            stream.write_all(&okay_response).await?;
                        }
                        x if x == AdbCommand::CLSE as u32 => {
                            // Handle close
                            streams.remove(&msg.arg0);

                            // Send CLSE response
                            let close = AdbMessage::new_clse(msg.arg1, msg.arg0);
                            let response = close.serialize();
                            stream.write_all(&response).await?;

                            debug!("Closed stream {}", msg.arg0);
                        }
                        _ => {
                            warn!("Unhandled command: {:?}", msg.command);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to parse message: {}", e);
                }
            }
        }
    }

    Ok(())
}