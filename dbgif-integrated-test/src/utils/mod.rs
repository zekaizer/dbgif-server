pub mod ports;

use anyhow::Result;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::{sleep, timeout};

// Re-export commonly used functions from ports module
pub use ports::{
    find_available_port,
    find_available_ports,
    find_random_port,
    is_port_in_use,
    global_port_manager,
    PortManager,
};

/// Wait for a TCP port to become available
pub async fn wait_for_port(addr: SocketAddr, max_wait: Duration) -> Result<()> {
    let start = std::time::Instant::now();

    while start.elapsed() < max_wait {
        if let Ok(_) = timeout(Duration::from_millis(100), tokio::net::TcpStream::connect(addr)).await {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }

    Err(anyhow::anyhow!("Port {} did not become available within {:?}", addr, max_wait))
}

/// Setup logging for the test environment
pub fn setup_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into())
        )
        .init();
}