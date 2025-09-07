use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{info, error, debug};
use anyhow::Result;

use crate::protocol::constants::DEFAULT_PORT;
use super::client_handler::ClientHandler;

pub struct DbgifServer {
    listener: Option<TcpListener>,
    client_counter: Arc<RwLock<u32>>,
}

impl DbgifServer {
    pub fn new() -> Self {
        Self {
            listener: None,
            client_counter: Arc::new(RwLock::new(0)),
        }
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
        let listener = self.listener.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Server not bound. Call bind() first"))?;

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let client_id = {
                        let mut counter = self.client_counter.write().await;
                        *counter += 1;
                        *counter
                    };

                    info!("New client connected: {} (client_id: {})", addr, client_id);
                    
                    tokio::spawn(async move {
                        let mut handler = ClientHandler::new(client_id, stream);
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
    }
}

impl Default for DbgifServer {
    fn default() -> Self {
        Self::new()
    }
}