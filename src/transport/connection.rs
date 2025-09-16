use std::net::SocketAddr;
#[allow(unused_imports)]
use tokio::time::Duration;

// Transport connection trait
#[async_trait::async_trait]
pub trait Connection: Send + Sync {
    async fn send(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>>;
    async fn receive(&mut self) -> Result<Vec<u8>, Box<dyn std::error::Error>>;
    async fn close(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn is_connected(&self) -> bool;
    fn local_addr(&self) -> Result<SocketAddr, Box<dyn std::error::Error>>;
    fn remote_addr(&self) -> Result<SocketAddr, Box<dyn std::error::Error>>;
}