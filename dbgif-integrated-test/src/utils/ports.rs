use anyhow::Result;
use std::collections::HashSet;
use std::net::TcpListener;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Port manager for automatic port allocation and tracking
pub struct PortManager {
    reserved_ports: Arc<RwLock<HashSet<u16>>>,
    min_port: u16,
    max_port: u16,
}

impl PortManager {
    pub fn new() -> Self {
        Self::with_range(50000, 60000)
    }

    pub fn with_range(min_port: u16, max_port: u16) -> Self {
        Self {
            reserved_ports: Arc::new(RwLock::new(HashSet::new())),
            min_port,
            max_port,
        }
    }

    /// Find an available port in the configured range
    pub async fn find_available_port(&self) -> Result<u16> {
        let reserved = self.reserved_ports.read().await;

        for port in self.min_port..=self.max_port {
            if reserved.contains(&port) {
                continue;
            }

            // Try to bind to check if port is available
            if let Ok(listener) = TcpListener::bind(format!("127.0.0.1:{}", port)) {
                drop(listener);
                debug!("Found available port: {}", port);
                return Ok(port);
            }
        }

        Err(anyhow::anyhow!(
            "No available ports in range {}-{}",
            self.min_port,
            self.max_port
        ))
    }

    /// Find multiple available ports
    pub async fn find_available_ports(&self, count: usize) -> Result<Vec<u16>> {
        let mut ports = Vec::new();
        let mut reserved = self.reserved_ports.write().await;

        for _ in 0..count {
            let port = self.find_single_port(&reserved).await?;
            reserved.insert(port);
            ports.push(port);
        }

        info!("Allocated {} ports: {:?}", count, ports);
        Ok(ports)
    }

    async fn find_single_port(&self, reserved: &HashSet<u16>) -> Result<u16> {
        for port in self.min_port..=self.max_port {
            if reserved.contains(&port) {
                continue;
            }

            if let Ok(listener) = TcpListener::bind(format!("127.0.0.1:{}", port)) {
                drop(listener);
                return Ok(port);
            }
        }

        Err(anyhow::anyhow!("No available ports"))
    }

    /// Reserve a specific port
    pub async fn reserve_port(&self, port: u16) -> Result<()> {
        let mut reserved = self.reserved_ports.write().await;

        if reserved.contains(&port) {
            return Err(anyhow::anyhow!("Port {} is already reserved", port));
        }

        // Check if port is actually available
        match TcpListener::bind(format!("127.0.0.1:{}", port)) {
            Ok(listener) => {
                drop(listener);
                reserved.insert(port);
                info!("Reserved port: {}", port);
                Ok(())
            }
            Err(e) => {
                Err(anyhow::anyhow!("Port {} is not available: {}", port, e))
            }
        }
    }

    /// Release a reserved port
    pub async fn release_port(&self, port: u16) {
        let mut reserved = self.reserved_ports.write().await;
        if reserved.remove(&port) {
            debug!("Released port: {}", port);
        }
    }

    /// Release all reserved ports
    pub async fn release_all(&self) {
        let mut reserved = self.reserved_ports.write().await;
        let count = reserved.len();
        reserved.clear();
        info!("Released {} reserved ports", count);
    }

    /// Get list of reserved ports
    pub async fn get_reserved_ports(&self) -> Vec<u16> {
        let reserved = self.reserved_ports.read().await;
        let mut ports: Vec<u16> = reserved.iter().copied().collect();
        ports.sort();
        ports
    }

    /// Check if a port is reserved
    pub async fn is_reserved(&self, port: u16) -> bool {
        self.reserved_ports.read().await.contains(&port)
    }
}

impl Default for PortManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global port manager instance
static GLOBAL_PORT_MANAGER: once_cell::sync::Lazy<PortManager> =
    once_cell::sync::Lazy::new(|| PortManager::new());

/// Get a reference to the global port manager
pub fn global_port_manager() -> &'static PortManager {
    &GLOBAL_PORT_MANAGER
}

/// Helper function to find an available port using the global manager
pub async fn find_available_port() -> Result<u16> {
    global_port_manager().find_available_port().await
}

/// Helper function to find multiple available ports
pub async fn find_available_ports(count: usize) -> Result<Vec<u16>> {
    global_port_manager().find_available_ports(count).await
}

/// Wait for a port to be available for connection
pub async fn wait_for_port_available(port: u16, max_retries: u32) -> Result<()> {
    for i in 0..max_retries {
        match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(_) => {
                debug!("Port {} is available after {} retries", port, i);
                return Ok(());
            }
            Err(_) => {
                if i < max_retries - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    Err(anyhow::anyhow!("Port {} not available after {} retries", port, max_retries))
}

/// Check if a port is in use
pub fn is_port_in_use(port: u16) -> bool {
    TcpListener::bind(format!("127.0.0.1:{}", port)).is_err()
}

/// Find a random available port (system allocated)
pub fn find_random_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}