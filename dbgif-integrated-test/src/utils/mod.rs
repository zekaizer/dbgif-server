use anyhow::Result;
use std::net::{TcpListener, SocketAddr};
use std::time::Duration;
use tokio::time::{sleep, timeout};

/// Find an available port on the system
pub fn find_available_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

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