use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, error, info, warn};

use dbgif_server::protocol::{
    constants::{MAXDATA, VERSION},
    message::{Command, Message},
};

/// Command line arguments for loopback client
#[derive(Parser, Debug)]
#[command(name = "loopback_client")]
#[command(about = "Generic DBGIF loopback test client")]
struct Args {
    /// DBGIF server address
    #[arg(long, default_value = "127.0.0.1:5037")]
    server: String,

    /// List available devices and exit
    #[arg(long, short)]
    list: bool,

    /// Device A identifier (from device list)
    #[arg(long, short = 'a')]
    device_a: Option<String>,

    /// Device B identifier (from device list)
    #[arg(long, short = 'b')]
    device_b: Option<String>,

    /// Test duration in seconds (0 = infinite)
    #[arg(long, default_value = "60")]
    duration: u64,

    /// Data size per transfer in bytes
    #[arg(long, default_value = "65536")]
    size: usize,

    /// Delay between transfers in milliseconds
    #[arg(long, default_value = "0")]
    delay: u64,

    /// Enable CSV output for statistics
    #[arg(long)]
    csv: bool,

    /// Test pattern
    #[arg(long, default_value = "echo")]
    pattern: TestPattern,

    /// Enable bridge loopback test mode (requires both device A and B)
    #[arg(long)]
    bridge: bool,
}

/// Available test patterns
#[derive(Debug, Clone, ValueEnum)]
enum TestPattern {
    /// Simple echo test
    Echo,
    /// Bulk transfer test
    Bulk,
    /// Random size test
    Random,
}

/// Device information from server
#[derive(Debug, Clone)]
struct DeviceInfo {
    id: String,
    status: String,
}

/// DBGIF client for server communication
pub struct DbgifClient {
    server_addr: String,
}

impl DbgifClient {
    pub fn new(server_addr: String) -> Self {
        Self { server_addr }
    }

    /// Connect to DBGIF server and perform handshake with retry
    pub async fn connect(&self) -> Result<TcpStream> {
        self.connect_with_retry(3).await
    }

    /// Connect to DBGIF server with specified retry count
    async fn connect_with_retry(&self, max_retries: usize) -> Result<TcpStream> {
        let mut last_error = None;
        
        for attempt in 0..max_retries {
            if attempt > 0 {
                warn!("Connection attempt {} failed, retrying...", attempt);
                tokio::time::sleep(Duration::from_millis(1000 * attempt as u64)).await;
            }
            
            match self.try_connect().await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < max_retries {
                        warn!("Connection attempt {} failed: {}", attempt + 1, last_error.as_ref().unwrap());
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All connection attempts failed")))
    }

    /// Single connection attempt
    async fn try_connect(&self) -> Result<TcpStream> {
        info!("Connecting to DBGIF server at {}", self.server_addr);
        
        let mut stream = tokio::time::timeout(
            Duration::from_secs(10),
            TcpStream::connect(&self.server_addr)
        ).await
        .context("Connection timeout")?
        .context("Failed to connect to server")?;

        // Send CNXN message
        let system_identity = b"host::loopback_client".to_vec();
        let cnxn_msg = Message::new(
            Command::Connect,
            VERSION,
            MAXDATA as u32,
            system_identity,
        );

        self.send_message(&mut stream, &cnxn_msg).await?;
        info!("Sent CNXN message to server");

        // Wait for CNXN response (skip AUTH for now)
        let response = self.receive_message_timeout(&mut stream, Duration::from_secs(10)).await?;
        if response.command != Command::Connect {
            bail!("Expected CNXN response, got {:?}", response.command);
        }

        info!("✓ Connected to DBGIF server successfully");
        Ok(stream)
    }

    /// List all available devices from server
    pub async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut stream = self.connect().await?;

        // Open stream for host:devices command
        let local_id = 1;
        let open_msg = Message::new(Command::Open, local_id, 0, b"host:devices".to_vec());

        self.send_message(&mut stream, &open_msg).await?;
        debug!("Sent OPEN for host:devices");

        // Wait for OKAY
        let okay_response = self.receive_message(&mut stream).await?;
        if okay_response.command != Command::Okay {
            bail!("Expected OKAY for host:devices, got {:?}", okay_response.command);
        }

        let remote_id = okay_response.arg0;
        debug!("Stream opened for host:devices: local={}, remote={}", local_id, remote_id);

        // Wait for WRTE with device list data, or CLSE if no devices
        let next_response = self.receive_message(&mut stream).await?;
        let devices = match next_response.command {
            Command::Write => {
                // Parse device list
                let device_list_str = String::from_utf8_lossy(&next_response.data);
                let devices = self.parse_device_list(&device_list_str)?;
                
                // Wait for CLSE
                let _close_response = self.receive_message(&mut stream).await?;
                devices
            },
            Command::Close => {
                // No devices available - return empty list
                debug!("No devices available");
                Vec::new()
            },
            _ => {
                bail!("Expected WRTE or CLSE, got {:?}", next_response.command);
            }
        };

        Ok(devices)
    }

    /// Select specific device and return connection stream
    pub async fn select_device(&self, device_id: &str) -> Result<TcpStream> {
        // First verify device exists and is online
        let devices = self.list_devices().await?;
        let device = devices.iter().find(|d| d.id == device_id)
            .ok_or_else(|| anyhow::anyhow!("Device '{}' not found", device_id))?;
        
        if device.status != "device" {
            bail!("Device '{}' is not online (status: {})", device_id, device.status);
        }

        let mut stream = self.connect().await?;

        // Open stream for host:transport:<device_id>
        let local_id = 1;
        let service = format!("host:transport:{}", device_id);
        let service_bytes = service.as_bytes().to_vec();
        let open_msg = Message::new(Command::Open, local_id, 0, service_bytes);

        self.send_message(&mut stream, &open_msg).await?;
        debug!("Sent OPEN for {}", service);

        // Wait for OKAY with timeout
        let okay_response = self.receive_message_timeout(&mut stream, Duration::from_secs(15)).await?;
        if okay_response.command != Command::Okay {
            bail!("Failed to select device '{}': server responded with {:?}", 
                  device_id, okay_response.command);
        }

        info!("✓ Selected device: {}", device_id);
        Ok(stream)
    }

    /// Send message to stream
    async fn send_message(&self, stream: &mut TcpStream, message: &Message) -> Result<()> {
        let data = message.serialize();
        stream.write_all(&data).await.context("Failed to send message")?;
        debug!("Sent message: {:?}", message.command);
        Ok(())
    }

    /// Receive message from stream with timeout
    async fn receive_message(&self, stream: &mut TcpStream) -> Result<Message> {
        self.receive_message_timeout(stream, Duration::from_secs(30)).await
    }

    /// Receive message from stream with custom timeout
    async fn receive_message_timeout(&self, stream: &mut TcpStream, timeout: Duration) -> Result<Message> {
        let result = tokio::time::timeout(timeout, async {
            // Read header (24 bytes)
            let mut header = [0u8; 24];
            stream.read_exact(&mut header).await.context("Failed to read message header")?;

            // Parse header to get data length
            use bytes::Buf;
            let mut header_cursor = std::io::Cursor::new(&header);
            let _command = header_cursor.get_u32_le();
            let _arg0 = header_cursor.get_u32_le();
            let _arg1 = header_cursor.get_u32_le();
            let data_length = header_cursor.get_u32_le();

            // Read data if present
            let mut full_message = header.to_vec();
            if data_length > 0 {
                let mut data = vec![0u8; data_length as usize];
                stream.read_exact(&mut data).await.context("Failed to read message data")?;
                full_message.extend_from_slice(&data);
            }

            let message = Message::deserialize(full_message.as_slice()).context("Failed to deserialize message")?;
            debug!("Received message: {:?}", message.command);
            Ok(message)
        }).await;

        match result {
            Ok(message) => message,
            Err(_) => bail!("Message receive timeout after {:?}", timeout),
        }
    }

    /// Parse device list response from server
    fn parse_device_list(&self, device_list_str: &str) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();
        
        for line in device_list_str.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            
            // Format: "device_id\tdevice" or "device_id\toffline"
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                devices.push(DeviceInfo {
                    id: parts[0].to_string(),
                    status: parts[1].to_string(),
                });
            }
        }
        
        Ok(devices)
    }
}

/// Performance statistics tracking
pub struct TestStats {
    start_time: Instant,
    total_transfers: AtomicU64,
    total_bytes: AtomicU64,
    total_errors: AtomicU32,
    min_latency_ns: AtomicU64,
    max_latency_ns: AtomicU64,
    total_latency_ns: AtomicU64,
    csv_output: bool,
}

impl TestStats {
    pub fn new(csv_output: bool) -> Self {
        if csv_output {
            println!("timestamp_ms,bytes,latency_us,success");
        }

        Self {
            start_time: Instant::now(),
            total_transfers: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            total_errors: AtomicU32::new(0),
            min_latency_ns: AtomicU64::new(u64::MAX),
            max_latency_ns: AtomicU64::new(0),
            total_latency_ns: AtomicU64::new(0),
            csv_output,
        }
    }

    pub fn record_success(&self, bytes: usize, latency: Duration) {
        self.total_transfers.fetch_add(1, Ordering::Relaxed);
        self.total_bytes.fetch_add(bytes as u64, Ordering::Relaxed);

        let latency_ns = latency.as_nanos() as u64;
        self.total_latency_ns.fetch_add(latency_ns, Ordering::Relaxed);

        // Update min latency
        let mut current_min = self.min_latency_ns.load(Ordering::Relaxed);
        while current_min > latency_ns {
            match self.min_latency_ns.compare_exchange_weak(
                current_min,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        // Update max latency
        let mut current_max = self.max_latency_ns.load(Ordering::Relaxed);
        while current_max < latency_ns {
            match self.max_latency_ns.compare_exchange_weak(
                current_max,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }

        if self.csv_output {
            let timestamp_ms = self.start_time.elapsed().as_millis();
            println!("{},{},{},true", timestamp_ms, bytes, latency.as_micros());
        }
    }

    pub fn record_error(&self) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);

        if self.csv_output {
            let timestamp_ms = self.start_time.elapsed().as_millis();
            println!("{},0,0,false", timestamp_ms);
        }
    }

    pub fn print_stats(&self) {
        let elapsed = self.start_time.elapsed();
        let transfers = self.total_transfers.load(Ordering::Relaxed);
        let bytes = self.total_bytes.load(Ordering::Relaxed);
        let errors = self.total_errors.load(Ordering::Relaxed);

        let throughput_mbps = if elapsed.as_secs() > 0 {
            (bytes / 1_000_000) / elapsed.as_secs()
        } else {
            0
        };

        let avg_latency_us = if transfers > 0 {
            (self.total_latency_ns.load(Ordering::Relaxed) / transfers) / 1_000
        } else {
            0
        };

        info!(
            "Stats - Elapsed: {:?} | Transfers: {} | Bytes: {} MB | Throughput: {} MB/s | Errors: {} | Avg Latency: {}μs",
            elapsed,
            transfers,
            bytes / 1_000_000,
            throughput_mbps,
            errors,
            avg_latency_us
        );
    }

    pub fn print_summary(&self) {
        let elapsed = self.start_time.elapsed();
        let transfers = self.total_transfers.load(Ordering::Relaxed);
        let bytes = self.total_bytes.load(Ordering::Relaxed);
        let errors = self.total_errors.load(Ordering::Relaxed);

        let throughput_mbps = if elapsed.as_secs() > 0 {
            (bytes / 1_000_000) / elapsed.as_secs()
        } else {
            0
        };

        let success_rate = if transfers > 0 {
            ((transfers - errors as u64) as f64 / transfers as f64) * 100.0
        } else {
            0.0
        };

        let avg_latency_us = if transfers > 0 {
            (self.total_latency_ns.load(Ordering::Relaxed) / transfers) / 1_000
        } else {
            0
        };

        info!("=== FINAL SUMMARY ===");
        info!("Total Duration: {:?}", elapsed);
        info!("Total Transfers: {}", transfers);
        info!("Total Bytes: {} MB", bytes / 1_000_000);
        info!("Average Throughput: {} MB/s", throughput_mbps);
        info!("Success Rate: {:.2}%", success_rate);
        info!("Latency - Min: {}μs | Avg: {}μs | Max: {}μs",
              self.min_latency_ns.load(Ordering::Relaxed) / 1_000,
              avg_latency_us,
              self.max_latency_ns.load(Ordering::Relaxed) / 1_000);
    }
}

/// Main loopback test implementation
pub struct LoopbackTest {
    client: DbgifClient,
    stats: Arc<TestStats>,
}

impl LoopbackTest {
    pub fn new(client: DbgifClient, csv_output: bool) -> Self {
        Self {
            client,
            stats: Arc::new(TestStats::new(csv_output)),
        }
    }

    /// Run single device echo test
    pub async fn run_single_device(&mut self, device_id: &str, config: &TestConfig) -> Result<()> {
        info!("Starting single device echo test with {}", device_id);
        
        let mut stream = self.client.select_device(device_id).await?;
        
        // Start statistics reporting
        let stats_clone = Arc::clone(&self.stats);
        let stats_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                stats_clone.print_stats();
            }
        });

        // Test loop
        let test_duration = if config.duration > 0 {
            Duration::from_secs(config.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };

        let start = Instant::now();
        while start.elapsed() < test_duration {
            match self.test_echo(&mut stream, config).await {
                Ok(latency) => {
                    self.stats.record_success(config.size, latency);
                }
                Err(e) => {
                    warn!("Echo test failed: {}", e);
                    self.stats.record_error();
                }
            }

            if config.delay > 0 {
                tokio::time::sleep(Duration::from_millis(config.delay)).await;
            }
        }

        stats_task.abort();
        self.stats.print_summary();
        Ok(())
    }

    /// Run bridge loopback test between two transports
    pub async fn run_bridge_loopback(&mut self, device_a: &str, device_b: &str, config: &TestConfig) -> Result<()> {
        info!("Starting bridge loopback test: {} <-> {}", device_a, device_b);
        
        // Create separate connections for each transport
        let client_a = DbgifClient::new(self.client.server_addr.clone());
        let client_b = DbgifClient::new(self.client.server_addr.clone());
        
        // Create separate statistics for each direction
        let stats_a_to_b = Arc::new(TestStats::new(config.csv_output));
        let stats_b_to_a = Arc::new(TestStats::new(config.csv_output));
        
        if config.csv_output {
            println!("direction,timestamp_ms,bytes,latency_us,success");
        }

        // Start statistics reporting
        let stats_a_clone = Arc::clone(&stats_a_to_b);
        let stats_b_clone = Arc::clone(&stats_b_to_a);
        let stats_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                info!("=== A->B Bridge Stats ===");
                stats_a_clone.print_stats();
                info!("=== B->A Bridge Stats ===");
                stats_b_clone.print_stats();
            }
        });

        let test_duration = if config.duration > 0 {
            Duration::from_secs(config.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };

        // Create bridge test tasks
        let config_a = TestConfig {
            duration: config.duration,
            size: config.size,
            delay: config.delay,
            pattern: config.pattern.clone(),
            csv_output: config.csv_output,
        };
        let config_b = TestConfig {
            duration: config.duration,
            size: config.size,
            delay: config.delay,
            pattern: config.pattern.clone(),
            csv_output: config.csv_output,
        };

        let device_a_str = device_a.to_string();
        let device_b_str = device_b.to_string();
        let stats_a = Arc::clone(&stats_a_to_b);
        let stats_b = Arc::clone(&stats_b_to_a);

        // Task A: Send from device A, receive on device B
        let task_a_to_b = tokio::spawn(async move {
            if let Err(e) = Self::run_bridge_sender(&client_a, &device_a_str, &config_a, stats_a, "A->B").await {
                error!("A->B bridge test failed: {}", e);
            }
        });

        // Task B: Send from device B, receive on device A
        let task_b_to_a = tokio::spawn(async move {
            // Wait a bit to let A->B task start first
            tokio::time::sleep(Duration::from_millis(1000)).await;
            if let Err(e) = Self::run_bridge_sender(&client_b, &device_b_str, &config_b, stats_b, "B->A").await {
                error!("B->A bridge test failed: {}", e);
            }
        });

        // Wait for test duration
        tokio::time::sleep(test_duration).await;
        info!("Bridge test duration reached, stopping...");

        // Cancel all tasks
        task_a_to_b.abort();
        task_b_to_a.abort();
        stats_task.abort();

        // Print final summary
        info!("=== BRIDGE LOOPBACK TEST SUMMARY ===");
        info!("=== A->B Direction ===");
        stats_a_to_b.print_summary();
        info!("=== B->A Direction ===");
        stats_b_to_a.print_summary();

        Ok(())
    }

    /// Run bidirectional test between device A and device B
    pub async fn run_bidirectional(&mut self, device_a: &str, device_b: &str, config: &TestConfig) -> Result<()> {
        info!("Starting bidirectional test: {} (client) <-> {} (server)", device_a, device_b);
        
        // Create separate statistics for each direction
        let stats_a_to_b = Arc::new(TestStats::new(config.csv_output));
        let stats_b_to_a = Arc::new(TestStats::new(config.csv_output));
        
        if config.csv_output {
            println!("direction,timestamp_ms,bytes,latency_us,success");
        }

        // Start statistics reporting
        let stats_a_clone = Arc::clone(&stats_a_to_b);
        let stats_b_clone = Arc::clone(&stats_b_to_a);
        let stats_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                info!("=== A->B Stats ===");
                stats_a_clone.print_stats();
                info!("=== B->A Stats ===");
                stats_b_clone.print_stats();
            }
        });

        let test_duration = if config.duration > 0 {
            Duration::from_secs(config.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };

        // Task 1: A->B->A communication (A as client, B as server)
        let client_clone = DbgifClient::new(self.client.server_addr.clone());
        let config_clone = TestConfig {
            duration: config.duration,
            size: config.size,
            delay: config.delay,
            pattern: config.pattern.clone(),
            csv_output: config.csv_output,
        };
        let stats_a_clone = Arc::clone(&stats_a_to_b);
        let device_a_str = device_a.to_string();
        let device_b_str = device_b.to_string();

        let task_a_to_b = tokio::spawn(async move {
            if let Err(e) = Self::run_client_to_server(&client_clone, &device_a_str, &device_b_str, &config_clone, stats_a_clone, "A->B").await {
                error!("A->B test failed: {}", e);
            }
        });

        // Task 2: B->A->B communication (B as client, A as server)  
        let client_clone2 = DbgifClient::new(self.client.server_addr.clone());
        let config_clone2 = TestConfig {
            duration: config.duration,
            size: config.size,
            delay: config.delay,
            pattern: config.pattern.clone(),
            csv_output: config.csv_output,
        };
        let stats_b_clone = Arc::clone(&stats_b_to_a);
        let device_a_str2 = device_a.to_string();
        let device_b_str2 = device_b.to_string();

        let task_b_to_a = tokio::spawn(async move {
            // Wait a bit to let A->B task start first
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Err(e) = Self::run_client_to_server(&client_clone2, &device_b_str2, &device_a_str2, &config_clone2, stats_b_clone, "B->A").await {
                error!("B->A test failed: {}", e);
            }
        });

        // Wait for test duration
        tokio::time::sleep(test_duration).await;
        info!("Test duration reached, stopping...");

        // Cancel all tasks
        task_a_to_b.abort();
        task_b_to_a.abort();
        stats_task.abort();

        // Print final summary
        info!("=== BIDIRECTIONAL TEST SUMMARY ===");
        info!("=== A->B Direction ===");
        stats_a_to_b.print_summary();
        info!("=== B->A Direction ===");
        stats_b_to_a.print_summary();

        Ok(())
    }

    /// Run bridge sender (sends data and receives from other transport with separated TX/RX tasks)
    async fn run_bridge_sender(
        client: &DbgifClient,
        device: &str,
        config: &TestConfig,
        stats: Arc<TestStats>,
        direction: &str,
    ) -> Result<()> {
        info!("Starting {} bridge sender for device: {}", direction, device);
        
        let mut stream = client.select_device(device).await?;
        let start = Instant::now();
        let test_duration = if config.duration > 0 {
            Duration::from_secs(config.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };

        // Open bridge stream
        let local_id = 2;
        let bridge_service = b"bridge:loopback".to_vec();
        let open_msg = Message::new(Command::Open, local_id, 0, bridge_service);
        
        Self::send_message_static(client, &mut stream, &open_msg).await?;
        
        // Wait for OKAY response
        let okay_response = Self::receive_message_static(client, &mut stream).await?;
        if okay_response.command != Command::Okay {
            bail!("{} - Expected OKAY for bridge open, got {:?}", direction, okay_response.command);
        }
        
        let remote_id = okay_response.arg0;
        info!("{} - Bridge stream opened: local={}, remote={}", direction, local_id, remote_id);

        // Split stream for concurrent TX and RX
        let (mut read_half, mut write_half) = stream.into_split();
        
        // Create channels for coordination
        let (tx_done, mut rx_done) = tokio::sync::mpsc::channel::<()>(1);
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(2);
        
        let send_stats = Arc::clone(&stats);
        let receive_stats = Arc::clone(&stats);
        let start_clone = start;
        let direction_clone = direction.to_string();
        let config_clone = TestConfig {
            duration: config.duration,
            size: config.size,
            delay: config.delay,
            pattern: config.pattern.clone(),
            csv_output: config.csv_output,
        };
        
        // TX Task - sends data continuously
        let tx_task = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx.subscribe();
            while start_clone.elapsed() < test_duration {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("{} - TX task shutting down", direction_clone);
                        break;
                    }
                    _ = async {
                        let test_data = Self::generate_test_data_static(config_clone.size, &config_clone.pattern);
                        let request_start = Instant::now();
                        
                        let write_msg = Message::new(Command::Write, local_id, remote_id, test_data.clone());
                        let serialized = write_msg.serialize();
                        
                        // Send with flush (already implemented in transport layer)
                        if let Err(e) = write_half.write_all(&serialized).await {
                            warn!("{} - Failed to send bridge data: {}", direction_clone, e);
                            send_stats.record_error();
                            return;
                        }
                        
                        let send_latency = request_start.elapsed();
                        send_stats.record_success(config_clone.size, send_latency);
                        
                        if config_clone.csv_output {
                            let timestamp_ms = start_clone.elapsed().as_millis();
                            println!("{}_tx,{},{},{},true", direction_clone, timestamp_ms, config_clone.size, send_latency.as_micros());
                        }
                        
                        if config_clone.delay > 0 {
                            tokio::time::sleep(Duration::from_millis(config_clone.delay)).await;
                        }
                    } => {}
                }
            }
            let _ = tx_done.send(()).await;
        });
        
        let direction_clone2 = direction.to_string();
        
        // RX Task - receives data continuously  
        let rx_task = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx.subscribe();
            while start.elapsed() < test_duration {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("{} - RX task shutting down", direction_clone2);
                        break;
                    }
                    result = tokio::time::timeout(Duration::from_millis(500), async {
                        // Read message header
                        let mut header = [0u8; 24];
                        read_half.read_exact(&mut header).await?;
                        
                        // Parse header to get data length
                        use bytes::Buf;
                        let mut header_cursor = std::io::Cursor::new(&header);
                        let _command = header_cursor.get_u32_le();
                        let _arg0 = header_cursor.get_u32_le();
                        let _arg1 = header_cursor.get_u32_le();
                        let data_length = header_cursor.get_u32_le();
                        
                        // Read data if present
                        let mut full_message = header.to_vec();
                        if data_length > 0 {
                            let mut data = vec![0u8; data_length as usize];
                            read_half.read_exact(&mut data).await?;
                            full_message.extend_from_slice(&data);
                        }
                        
                        let message = Message::deserialize(full_message.as_slice())?;
                        Ok::<Message, anyhow::Error>(message)
                    }) => {
                        match result {
                            Ok(Ok(recv_msg)) => {
                                if recv_msg.command == Command::Write {
                                    info!("{} - Received bridge data: {} bytes", direction_clone2, recv_msg.data.len());
                                    
                                    if config.csv_output {
                                        let timestamp_ms = start.elapsed().as_millis();
                                        println!("{}_rx,{},{},0,true", direction_clone2, timestamp_ms, recv_msg.data.len());
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                debug!("{} - RX error: {}", direction_clone2, e);
                            }
                            Err(_) => {
                                // Timeout is expected when no data available
                            }
                        }
                    }
                }
            }
        });

        // Wait for completion or timeout
        tokio::select! {
            _ = tokio::time::sleep(test_duration) => {
                info!("{} - Test duration reached, stopping tasks", direction);
            }
            _ = rx_done.recv() => {
                info!("{} - TX task completed", direction);
            }
        }
        
        // Shutdown both tasks
        let _ = shutdown_tx.send(());
        tx_task.abort();
        rx_task.abort();
        
        info!("{} bridge sender completed", direction);
        Ok(())
    }

    /// Run client to server test with improved TX/RX handling (one direction of bidirectional test)
    async fn run_client_to_server(
        client: &DbgifClient,
        client_device: &str,
        server_device: &str,
        config: &TestConfig,
        stats: Arc<TestStats>,
        direction: &str,
    ) -> Result<()> {
        info!("Starting {} test: {} -> {}", direction, client_device, server_device);
        
        let mut stream = client.select_device(client_device).await?;
        let start = Instant::now();
        let test_duration = if config.duration > 0 {
            Duration::from_secs(config.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };

        // Open persistent shell:echo service for better performance
        let local_id = 2;
        let echo_service = b"shell:echo".to_vec();
        let open_msg = Message::new(Command::Open, local_id, 0, echo_service);
        
        Self::send_message_static(client, &mut stream, &open_msg).await?;
        
        // Wait for OKAY response
        let okay_response = Self::receive_message_static(client, &mut stream).await?;
        if okay_response.command != Command::Okay {
            bail!("{} - Expected OKAY for echo service, got {:?}", direction, okay_response.command);
        }
        
        let remote_id = okay_response.arg0;
        info!("{} - Echo service opened: local={}, remote={}", direction, local_id, remote_id);

        // Split stream for better performance
        let (mut read_half, mut write_half) = stream.into_split();
        
        // Create coordination channels
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(2);
        
        let send_stats = Arc::clone(&stats);
        let receive_stats = Arc::clone(&stats);
        let start_clone = start;
        let direction_clone = direction.to_string();
        let config_clone = TestConfig {
            duration: config.duration,
            size: config.size,
            delay: config.delay,
            pattern: config.pattern.clone(),
            csv_output: config.csv_output,
        };
        
        // TX Task - sends echo requests
        let tx_task = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx.subscribe();
            let mut request_counter = 0u32;
            
            while start_clone.elapsed() < test_duration {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("{} - TX task shutting down", direction_clone);
                        break;
                    }
                    _ = async {
                        let test_data = Self::generate_test_data_static(config_clone.size, &config_clone.pattern);
                        let request_start = Instant::now();
                        request_counter += 1;
                        
                        // Send WRITE message
                        let write_msg = Message::new(Command::Write, local_id, remote_id, test_data.clone());
                        let serialized = write_msg.serialize();
                        
                        if let Err(e) = write_half.write_all(&serialized).await {
                            warn!("{} - Failed to send WRITE: {}", direction_clone, e);
                            send_stats.record_error();
                            return;
                        }
                        
                        // Record send timing (we'll measure full round-trip in RX task)
                        debug!("{} - Sent request #{}: {} bytes", direction_clone, request_counter, config_clone.size);
                        
                        if config_clone.delay > 0 {
                            tokio::time::sleep(Duration::from_millis(config_clone.delay)).await;
                        }
                    } => {}
                }
            }
        });
        
        let direction_clone2 = direction.to_string();
        
        // RX Task - receives echo responses and OKAY acknowledgments
        let rx_task = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx.subscribe();
            let mut response_counter = 0u32;
            
            while start.elapsed() < test_duration {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("{} - RX task shutting down", direction_clone2);
                        break;
                    }
                    result = tokio::time::timeout(Duration::from_secs(5), async {
                        // Read message header
                        let mut header = [0u8; 24];
                        read_half.read_exact(&mut header).await?;
                        
                        // Parse header to get data length
                        use bytes::Buf;
                        let mut header_cursor = std::io::Cursor::new(&header);
                        let _command = header_cursor.get_u32_le();
                        let _arg0 = header_cursor.get_u32_le();
                        let _arg1 = header_cursor.get_u32_le();
                        let data_length = header_cursor.get_u32_le();
                        
                        // Read data if present
                        let mut full_message = header.to_vec();
                        if data_length > 0 {
                            let mut data = vec![0u8; data_length as usize];
                            read_half.read_exact(&mut data).await?;
                            full_message.extend_from_slice(&data);
                        }
                        
                        let message = Message::deserialize(full_message.as_slice())?;
                        Ok::<Message, anyhow::Error>(message)
                    }) => {
                        match result {
                            Ok(Ok(recv_msg)) => {
                                match recv_msg.command {
                                    Command::Okay => {
                                        // WRITE acknowledgment - expected but no action needed
                                        debug!("{} - Received OKAY acknowledgment", direction_clone2);
                                    }
                                    Command::Write => {
                                        // Echo response
                                        response_counter += 1;
                                        
                                        // Send OKAY acknowledgment for echo response
                                        let ack_msg = Message::new(Command::Okay, recv_msg.arg1, recv_msg.arg0, Vec::new());
                                        let ack_serialized = ack_msg.serialize();
                                        let _ = write_half.write_all(&ack_serialized).await;
                                        
                                        // Record successful round-trip
                                        let latency = Duration::from_millis(100); // Approximate since we can't track individual requests easily in split mode
                                        receive_stats.record_success(config.size, latency);
                                        
                                        if config.csv_output {
                                            let timestamp_ms = start.elapsed().as_millis();
                                            println!("{},{},{},{},true", direction_clone2, timestamp_ms, config.size, latency.as_micros());
                                        }
                                        
                                        debug!("{} - Received echo response #{}: {} bytes", direction_clone2, response_counter, recv_msg.data.len());
                                    }
                                    _ => {
                                        warn!("{} - Unexpected message type: {:?}", direction_clone2, recv_msg.command);
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                warn!("{} - RX error: {}", direction_clone2, e);
                                receive_stats.record_error();
                            }
                            Err(_) => {
                                debug!("{} - RX timeout (no data available)", direction_clone2);
                            }
                        }
                    }
                }
            }
        });

        // Wait for completion or timeout
        tokio::select! {
            _ = tokio::time::sleep(test_duration) => {
                info!("{} - Test duration reached, stopping tasks", direction);
            }
        }
        
        // Shutdown both tasks
        let _ = shutdown_tx.send(());
        tx_task.abort();
        rx_task.abort();
        
        // Close the echo service stream
        let close_msg = Message::new(Command::Close, local_id, remote_id, Vec::new());
        let close_serialized = close_msg.serialize();
        let _ = write_half.write_all(&close_serialized).await;

        info!("{} test completed", direction);
        Ok(())
    }

    /// Static helper for sending messages
    async fn send_message_static(client: &DbgifClient, stream: &mut TcpStream, message: &Message) -> Result<()> {
        client.send_message(stream, message).await
    }

    /// Static helper for receiving messages
    async fn receive_message_static(client: &DbgifClient, stream: &mut TcpStream) -> Result<Message> {
        client.receive_message(stream).await
    }

    /// Static helper for generating test data
    fn generate_test_data_static(size: usize, pattern: &TestPattern) -> Vec<u8> {
        match pattern {
            TestPattern::Echo => vec![0xAA; size],
            TestPattern::Bulk => vec![0xBB; size],
            TestPattern::Random => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                (0..size).map(|_| rng.gen::<u8>()).collect()
            }
        }
    }

    /// Perform single echo test
    async fn test_echo(&self, stream: &mut TcpStream, config: &TestConfig) -> Result<Duration> {
        let test_data = self.generate_test_data(config.size, &config.pattern);
        let start_time = Instant::now();

        // Open shell:echo service
        let local_id = 2; // Use different local ID for echo stream
        let echo_service = b"shell:echo".to_vec();
        let open_msg = Message::new(Command::Open, local_id, 0, echo_service);
        
        self.client.send_message(stream, &open_msg).await?;
        
        // Wait for OKAY response
        let okay_response = self.client.receive_message(stream).await?;
        if okay_response.command != Command::Okay {
            bail!("Failed to open echo service: got {:?}", okay_response.command);
        }
        
        let remote_id = okay_response.arg0;
        debug!("Echo service opened: local={}, remote={}", local_id, remote_id);
        
        // Send test data
        self.write_stream_data(stream, local_id, remote_id, &test_data).await?;
        
        // Read echo response
        let (_recv_local, _recv_remote, recv_data) = self.read_stream_data(stream).await?;
        
        // Verify data integrity
        if recv_data != test_data {
            bail!("Echo data mismatch: sent {} bytes, received {} bytes", 
                  test_data.len(), recv_data.len());
        }
        
        // Close the echo stream
        let close_msg = Message::new(Command::Close, local_id, remote_id, Vec::new());
        self.client.send_message(stream, &close_msg).await?;
        
        let latency = start_time.elapsed();
        Ok(latency)
    }

    /// Send data through stream
    async fn write_stream_data(&self, stream: &mut TcpStream, local_id: u32, remote_id: u32, data: &[u8]) -> Result<()> {
        let write_msg = Message::new(Command::Write, local_id, remote_id, data.to_vec());
        self.client.send_message(stream, &write_msg).await?;
        
        // Wait for OKAY acknowledgment with shorter timeout for echo tests
        let ack = self.client.receive_message_timeout(stream, Duration::from_secs(5)).await
            .context("Timeout waiting for WRTE acknowledgment")?;
        if ack.command != Command::Okay {
            bail!("Expected OKAY for WRTE, got {:?}", ack.command);
        }
        
        Ok(())
    }

    /// Read data from stream
    async fn read_stream_data(&self, stream: &mut TcpStream) -> Result<(u32, u32, Vec<u8>)> {
        let response = self.client.receive_message_timeout(stream, Duration::from_secs(5)).await
            .context("Timeout waiting for echo response")?;
        if response.command != Command::Write {
            bail!("Expected WRTE response, got {:?}", response.command);
        }
        
        // Send OKAY acknowledgment
        let okay_msg = Message::new(Command::Okay, response.arg1, response.arg0, Vec::new());
        self.client.send_message(stream, &okay_msg).await?;
        
        Ok((response.arg0, response.arg1, response.data.to_vec()))
    }

    /// Generate test data based on pattern
    fn generate_test_data(&self, size: usize, pattern: &TestPattern) -> Vec<u8> {
        match pattern {
            TestPattern::Echo => vec![0xAA; size],
            TestPattern::Bulk => vec![0xBB; size],
            TestPattern::Random => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                (0..size).map(|_| rng.gen::<u8>()).collect()
            }
        }
    }
}

/// Test configuration
pub struct TestConfig {
    pub duration: u64,
    pub size: usize,
    pub delay: u64,
    pub pattern: TestPattern,
    pub csv_output: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("loopback_client=info".parse().unwrap())
                .add_directive("dbgif_server=info".parse().unwrap())
        )
        .init();

    info!("DBGIF Loopback Test Client");
    info!("========================");

    let client = DbgifClient::new(args.server.clone());

    // List devices if requested
    if args.list {
        info!("Listing available devices...");
        let devices = client.list_devices().await?;
        
        if devices.is_empty() {
            info!("No devices found");
            return Ok(());
        }

        info!("Available devices:");
        for device in devices {
            info!("  {} ({})", device.id, device.status);
        }
        return Ok(());
    }

    // Validate bridge mode requirements
    if args.bridge && (args.device_a.is_none() || args.device_b.is_none()) {
        bail!("Bridge loopback test requires both --device-a and --device-b to be specified");
    }

    // Determine test devices
    let devices = client.list_devices().await?;
    if devices.is_empty() {
        bail!("No devices available for testing");
    }

    // Create test configuration
    let config = TestConfig {
        duration: args.duration,
        size: args.size,
        delay: args.delay,
        pattern: args.pattern,
        csv_output: args.csv,
    };

    // Run test
    let mut loopback_test = LoopbackTest::new(client, args.csv);
    
    // Set up Ctrl+C handler
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        info!("Received Ctrl+C, shutting down gracefully...");
        let _ = tx.send(()).await;
    });

    // Check if both device A and B are specified
    if let (Some(device_a), Some(device_b)) = (&args.device_a, &args.device_b) {
        if args.bridge {
            info!("Running bridge loopback test: {} <-> {}", device_a, device_b);
            
            // Run bridge loopback test with cancellation support
            tokio::select! {
                result = loopback_test.run_bridge_loopback(device_a, device_b, &config) => {
                    match result {
                        Ok(_) => info!("✓ Bridge loopback test completed successfully"),
                        Err(e) => error!("✗ Bridge loopback test failed: {}", e),
                    }
                }
                _ = rx.recv() => {
                    info!("Test interrupted by user");
                }
            }
        } else {
            info!("Running bidirectional test: {} <-> {}", device_a, device_b);
            
            // Run bidirectional test with cancellation support
            tokio::select! {
                result = loopback_test.run_bidirectional(device_a, device_b, &config) => {
                    match result {
                        Ok(_) => info!("✓ Bidirectional loopback test completed successfully"),
                        Err(e) => error!("✗ Bidirectional loopback test failed: {}", e),
                    }
                }
                _ = rx.recv() => {
                    info!("Test interrupted by user");
                }
            }
        }
    } else {
        // Single device test
        let device_a = args.device_a.as_ref()
            .or_else(|| devices.first().map(|d| &d.id))
            .ok_or_else(|| anyhow::anyhow!("No device A specified or available"))?;

        info!("Using device for single loopback test: {}", device_a);

        // Run single device test with cancellation support
        tokio::select! {
            result = loopback_test.run_single_device(device_a, &config) => {
                match result {
                    Ok(_) => info!("✓ Single loopback test completed successfully"),
                    Err(e) => error!("✗ Single loopback test failed: {}", e),
                }
            }
            _ = rx.recv() => {
                info!("Test interrupted by user");
                loopback_test.stats.print_summary();
            }
        }
    }

    Ok(())
}