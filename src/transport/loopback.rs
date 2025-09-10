use anyhow::Result;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, trace};

use super::{ConnectionStatus, Transport, TransportType};

/// Shared data queues for loopback pair communication
#[derive(Clone)]
pub struct LoopbackQueues {
    /// Data sent from A to B
    a_to_b: Arc<Mutex<VecDeque<Vec<u8>>>>,
    /// Data sent from B to A
    b_to_a: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

impl LoopbackQueues {
    pub fn new() -> Self {
        Self {
            a_to_b: Arc::new(Mutex::new(VecDeque::new())),
            b_to_a: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

/// Virtual loopback transport for testing without hardware
/// 
/// This transport can work in two modes:
/// 1. Single device mode: echoes back messages (original behavior)
/// 2. Pair mode: routes messages between two paired devices for true bidirectional testing
pub struct LoopbackTransport {
    device_id: String,
    is_connected: bool,
    /// For single device mode (echo)
    echo_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    /// For pair mode (bidirectional)
    pair_queues: Option<LoopbackQueues>,
    is_device_a: bool, // true for device A, false for device B in pair mode
    latency: Duration,
    packet_loss_rate: f32,
}

impl LoopbackTransport {
    /// Create a new single loopback transport (echo mode)
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            is_connected: false,
            echo_queue: Arc::new(Mutex::new(VecDeque::new())),
            pair_queues: None,
            is_device_a: false,
            latency: Duration::from_millis(1),
            packet_loss_rate: 0.0,
        }
    }

    /// Create a new loopback transport with custom latency
    pub fn with_latency(device_id: String, latency: Duration) -> Self {
        Self {
            device_id,
            is_connected: false,
            echo_queue: Arc::new(Mutex::new(VecDeque::new())),
            pair_queues: None,
            is_device_a: false,
            latency,
            packet_loss_rate: 0.0,
        }
    }

    /// Create a new loopback transport with latency and packet loss simulation
    pub fn with_simulation(device_id: String, latency: Duration, packet_loss_rate: f32) -> Self {
        Self {
            device_id,
            is_connected: false,
            echo_queue: Arc::new(Mutex::new(VecDeque::new())),
            pair_queues: None,
            is_device_a: false,
            latency,
            packet_loss_rate: packet_loss_rate.clamp(0.0, 1.0),
        }
    }

    /// Create a pair of loopback transports for bidirectional communication
    pub fn create_pair(device_a_id: String, device_b_id: String) -> (Self, Self) {
        let shared_queues = LoopbackQueues::new();
        
        let device_a = Self {
            device_id: device_a_id,
            is_connected: false,
            echo_queue: Arc::new(Mutex::new(VecDeque::new())), // Not used in pair mode
            pair_queues: Some(shared_queues.clone()),
            is_device_a: true,
            latency: Duration::from_millis(1),
            packet_loss_rate: 0.0,
        };

        let device_b = Self {
            device_id: device_b_id,
            is_connected: false,
            echo_queue: Arc::new(Mutex::new(VecDeque::new())), // Not used in pair mode
            pair_queues: Some(shared_queues),
            is_device_a: false,
            latency: Duration::from_millis(1),
            packet_loss_rate: 0.0,
        };

        (device_a, device_b)
    }

    /// Create a pair of loopback transports with custom latency
    pub fn create_pair_with_latency(
        device_a_id: String,
        device_b_id: String,
        latency: Duration,
    ) -> (Self, Self) {
        let (mut device_a, mut device_b) = Self::create_pair(device_a_id, device_b_id);
        device_a.latency = latency;
        device_b.latency = latency;
        (device_a, device_b)
    }

    /// Simulate packet loss (returns true if packet should be dropped)
    fn should_drop_packet(&self) -> bool {
        if self.packet_loss_rate <= 0.0 {
            return false;
        }
        
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;
        
        let mut hasher = DefaultHasher::new();
        SystemTime::now().hash(&mut hasher);
        let random = (hasher.finish() % 1000) as f32 / 1000.0;
        
        random < self.packet_loss_rate
    }
}

#[async_trait]
impl Transport for LoopbackTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        if !self.is_connected {
            return Err(anyhow::anyhow!("Loopback transport not connected"));
        }

        trace!(
            "[{}] Sending {} bytes",
            self.device_id,
            data.len()
        );

        // Simulate network latency
        if !self.latency.is_zero() {
            sleep(self.latency).await;
        }

        // Simulate packet loss
        if self.should_drop_packet() {
            debug!("[{}] Simulating packet loss - data dropped", self.device_id);
            return Err(anyhow::anyhow!("Simulated packet loss"));
        }

        match &self.pair_queues {
            Some(queues) => {
                // Pair mode: send to the other device
                let target_queue = if self.is_device_a {
                    &queues.a_to_b
                } else {
                    &queues.b_to_a
                };
                
                let mut queue = target_queue.lock().await;
                queue.push_back(data.to_vec());
                
                debug!("[{}] Data sent to paired device (queue length: {})", 
                       self.device_id, queue.len());
            }
            None => {
                // Single device mode: echo behavior
                let mut queue = self.echo_queue.lock().await;
                queue.push_back(data.to_vec());
                
                debug!("[{}] Data queued for echo (queue length: {})", 
                       self.device_id, queue.len());
            }
        }
        
        Ok(())
    }

    async fn receive(&mut self) -> Result<Vec<u8>> {
        if !self.is_connected {
            return Err(anyhow::anyhow!("Loopback transport not connected"));
        }

        // Simulate network latency
        if !self.latency.is_zero() {
            sleep(self.latency).await;
        }

        let data = match &self.pair_queues {
            Some(queues) => {
                // Pair mode: receive from the other device
                let source_queue = if self.is_device_a {
                    &queues.b_to_a
                } else {
                    &queues.a_to_b
                };
                
                let mut queue = source_queue.lock().await;
                match queue.pop_front() {
                    Some(data) => {
                        debug!("[{}] Data received from paired device (remaining: {})", 
                               self.device_id, queue.len());
                        Ok(data)
                    }
                    None => {
                        Err(anyhow::anyhow!("No data available from paired device"))
                    }
                }
            }
            None => {
                // Single device mode: echo behavior
                let mut queue = self.echo_queue.lock().await;
                match queue.pop_front() {
                    Some(data) => {
                        debug!("[{}] Data retrieved from echo queue (remaining: {})", 
                               self.device_id, queue.len());
                        Ok(data)
                    }
                    None => {
                        Err(anyhow::anyhow!("No data available in echo queue"))
                    }
                }
            }
        };

        match &data {
            Ok(bytes) => {
                trace!(
                    "[{}] Receiving {} bytes",
                    self.device_id,
                    bytes.len()
                );
            }
            Err(_) => {}
        }

        data
    }

    async fn connect(&mut self) -> Result<ConnectionStatus> {
        if self.is_connected {
            return Ok(ConnectionStatus::Ready);
        }

        debug!("[{}] Connecting loopback transport...", self.device_id);
        
        // Simulate connection establishment
        sleep(Duration::from_millis(10)).await;
        
        self.is_connected = true;
        debug!("[{}] Loopback transport connected successfully", self.device_id);
        
        // Loopback transport is always immediately ready
        Ok(ConnectionStatus::Ready)
    }

    async fn disconnect(&mut self) -> Result<()> {
        if !self.is_connected {
            return Ok(());
        }

        debug!("[{}] Disconnecting loopback transport...", self.device_id);
        
        // Clear queues on disconnect
        let data_cleared = match &self.pair_queues {
            Some(_queues) => {
                // In pair mode, we don't clear shared queues as the other device might still need them
                // Only clear if both devices disconnect
                debug!("[{}] Pair mode: shared queues retained for paired device", self.device_id);
                0
            }
            None => {
                // Single device mode: clear echo queue
                let mut queue = self.echo_queue.lock().await;
                let count = queue.len();
                queue.clear();
                count
            }
        };
        
        self.is_connected = false;
        
        debug!("[{}] Loopback transport disconnected (cleared {} queued items)", 
               self.device_id, data_cleared);
        
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        self.is_connected
    }

    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Loopback
    }

    async fn health_check(&self) -> Result<()> {
        if !self.is_connected {
            return Err(anyhow::anyhow!("Loopback transport not connected"));
        }

        // Check queue sizes
        let (_queue_size, mode_desc) = match &self.pair_queues {
            Some(queues) => {
                let a_to_b_size = queues.a_to_b.lock().await.len();
                let b_to_a_size = queues.b_to_a.lock().await.len();
                let total_size = a_to_b_size + b_to_a_size;
                
                if total_size > 1000 {
                    return Err(anyhow::anyhow!(
                        "Loopback pair queues too large: {} total messages (A→B: {}, B→A: {})", 
                        total_size, a_to_b_size, b_to_a_size
                    ));
                }
                
                (total_size, format!("pair mode (A→B: {}, B→A: {})", a_to_b_size, b_to_a_size))
            }
            None => {
                let queue = self.echo_queue.lock().await;
                let size = queue.len();
                
                if size > 1000 {
                    return Err(anyhow::anyhow!(
                        "Loopback echo queue too large: {} messages", 
                        size
                    ));
                }
                
                (size, format!("echo mode: {} messages", size))
            }
        };

        debug!("[{}] Loopback transport health check passed ({})", 
               self.device_id, mode_desc);
        
        Ok(())
    }

    async fn get_connection_status(&self) -> ConnectionStatus {
        if self.is_connected {
            ConnectionStatus::Ready
        } else {
            ConnectionStatus::Disconnected
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_loopback_basic() {
        let mut transport = LoopbackTransport::new("test_loopback".to_string());
        
        // Test connection
        let status = transport.connect().await.unwrap();
        assert_eq!(status, ConnectionStatus::Ready);
        assert!(transport.is_connected().await);
        
        // Test data echo
        let original_data = b"Hello, World!".to_vec();
        
        transport.send(&original_data).await.unwrap();
        let received_data = transport.receive().await.unwrap();
        
        assert_eq!(original_data, received_data);
        
        // Test health check
        transport.health_check().await.unwrap();
        
        // Test disconnect
        transport.disconnect().await.unwrap();
        assert!(!transport.is_connected().await);
    }
    
    #[tokio::test]
    async fn test_loopback_queue_behavior() {
        let mut transport = LoopbackTransport::new("test_queue".to_string());
        transport.connect().await.unwrap();
        
        // Send multiple data packets
        for i in 0..5 {
            let data = vec![i as u8; 10];
            transport.send(&data).await.unwrap();
        }
        
        // Receive them in order
        for i in 0..5 {
            let received = transport.receive().await.unwrap();
            assert_eq!(received, vec![i as u8; 10]);
        }
        
        // Queue should be empty now
        assert!(transport.receive().await.is_err());
    }

    #[tokio::test]
    async fn test_loopback_with_latency() {
        let latency = Duration::from_millis(50);
        let mut transport = LoopbackTransport::with_latency("test_latency".to_string(), latency);
        transport.connect().await.unwrap();
        
        let data = b"test".to_vec();
        
        let start = std::time::Instant::now();
        transport.send(&data).await.unwrap();
        let send_duration = start.elapsed();
        
        let start = std::time::Instant::now();
        transport.receive().await.unwrap();
        let recv_duration = start.elapsed();
        
        // Each operation should take at least the configured latency
        assert!(send_duration >= latency);
        assert!(recv_duration >= latency);
    }

    #[tokio::test]
    async fn test_loopback_pair_bidirectional() {
        let (mut device_a, mut device_b) = LoopbackTransport::create_pair(
            "device_a".to_string(),
            "device_b".to_string(),
        );
        
        // Connect both devices
        device_a.connect().await.unwrap();
        device_b.connect().await.unwrap();
        
        // Test A → B communication
        let data_a_to_b = b"Message from A to B".to_vec();
        
        device_a.send(&data_a_to_b).await.unwrap();
        let received_by_b = device_b.receive().await.unwrap();
        
        assert_eq!(data_a_to_b, received_by_b);
        
        // Test B → A communication
        let data_b_to_a = b"Reply from B to A".to_vec();
        
        device_b.send(&data_b_to_a).await.unwrap();
        let received_by_a = device_a.receive().await.unwrap();
        
        assert_eq!(data_b_to_a, received_by_a);
        
        // Test health checks
        device_a.health_check().await.unwrap();
        device_b.health_check().await.unwrap();
        
        // Test disconnection
        device_a.disconnect().await.unwrap();
        device_b.disconnect().await.unwrap();
        
        assert!(!device_a.is_connected().await);
        assert!(!device_b.is_connected().await);
    }

    #[tokio::test]
    async fn test_loopback_pair_multiple_messages() {
        let (mut device_a, mut device_b) = LoopbackTransport::create_pair(
            "multi_a".to_string(),
            "multi_b".to_string(),
        );
        
        device_a.connect().await.unwrap();
        device_b.connect().await.unwrap();
        
        // Send multiple data A → B
        for i in 0..3 {
            let data = format!("Message {} from A", i).as_bytes().to_vec();
            device_a.send(&data).await.unwrap();
        }
        
        // Send multiple data B → A
        for i in 10..13 {
            let data = format!("Message {} from B", i).as_bytes().to_vec();
            device_b.send(&data).await.unwrap();
        }
        
        // Receive data in B (from A)
        for i in 0..3 {
            let received = device_b.receive().await.unwrap();
            assert_eq!(received, format!("Message {} from A", i).as_bytes());
        }
        
        // Receive data in A (from B)
        for i in 10..13 {
            let received = device_a.receive().await.unwrap();
            assert_eq!(received, format!("Message {} from B", i).as_bytes());
        }
        
        // Both queues should be empty now
        assert!(device_a.receive().await.is_err());
        assert!(device_b.receive().await.is_err());
    }

    #[tokio::test]
    async fn test_loopback_pair_with_latency() {
        let latency = Duration::from_millis(25);
        let (mut device_a, mut device_b) = LoopbackTransport::create_pair_with_latency(
            "latency_a".to_string(),
            "latency_b".to_string(),
            latency,
        );
        
        device_a.connect().await.unwrap();
        device_b.connect().await.unwrap();
        
        let data = b"Latency test".to_vec();
        
        let start = std::time::Instant::now();
        device_a.send(&data).await.unwrap();
        let send_duration = start.elapsed();
        
        let start = std::time::Instant::now();
        device_b.receive().await.unwrap();
        let recv_duration = start.elapsed();
        
        // Each operation should take at least the configured latency
        assert!(send_duration >= latency);
        assert!(recv_duration >= latency);
    }
}