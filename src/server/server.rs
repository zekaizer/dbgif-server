use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{RwLock, watch};
use tracing::{debug, error, info};

use super::client_handler::ClientHandler;
use crate::protocol::constants::DEFAULT_PORT;
use crate::transport::TransportManager;
use crate::session::AdbSessionManager;

pub struct DbgifServer {
    listener: Option<TcpListener>,
    client_counter: Arc<RwLock<u32>>,
    transport_manager: Arc<TransportManager>,
    session_manager: Arc<AdbSessionManager>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl DbgifServer {
    pub fn new() -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let transport_manager = Arc::new(TransportManager::new());
        Self {
            listener: None,
            client_counter: Arc::new(RwLock::new(0)),
            transport_manager: transport_manager.clone(),
            session_manager: Arc::new(AdbSessionManager::new(transport_manager)),
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Get reference to transport manager for adding devices
    pub fn transport_manager(&self) -> Arc<TransportManager> {
        self.transport_manager.clone()
    }

    /// Signal the server to shutdown gracefully
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    pub async fn bind(&mut self, port: Option<u16>) -> Result<()> {
        let bind_port = port.unwrap_or(DEFAULT_PORT);
        let bind_address = format!("127.0.0.1:{}", bind_port);

        info!("Binding dbgif server to {}", bind_address);

        let listener = TcpListener::bind(&bind_address).await?;
        self.listener = Some(listener);

        info!("dbgif server listening on {}", bind_address);
        Ok(())
    }

    pub async fn run(&self) -> Result<()> {
        let listener = self
            .listener
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Server not bound. Call bind() first"))?;

        let mut shutdown_rx = self.shutdown_rx.clone();
        
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let client_id = {
                                let mut counter = self.client_counter.write().await;
                                *counter += 1;
                                *counter
                            };

                            info!("New client connected: {} (client_id: {})", addr, client_id);

                            let transport_manager = self.transport_manager.clone();
                            let session_manager = self.session_manager.clone();
                            tokio::spawn(async move {
                                let mut handler =
                                    ClientHandler::new(client_id.to_string(), stream, transport_manager, session_manager);

                                if let Err(e) = handler.handle().await {
                                    error!("Client {} disconnected with error: {}", client_id, e);
                                } else {
                                    debug!("Client {} disconnected gracefully", client_id);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Server received shutdown signal, stopping accept loop");
                        return Ok(());
                    }
                }
            }
        }
    }
}

impl Default for DbgifServer {
    fn default() -> Self {
        Self::new()
    }
}
