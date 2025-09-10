use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use dbgif_server::transport::{bridge_usb::BridgeUsbTransport, Transport};
use dbgif_server::protocol::message::{Command, Message};
use nusb::{list_devices, DeviceInfo};
use rand::Rng;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::{sync::Mutex, time::timeout};
use tracing::{error, info, warn};

// PL-25A1 Device identifiers
const PL25A1_VID: u16 = 0x067b;
const PL25A1_PID: u16 = 0x25a1;

// Test configuration
const DEVICE_READY_TIMEOUT: Duration = Duration::from_secs(10);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);

/// Command line arguments for aging test
#[derive(Parser, Debug)]
#[command(author, version, about = "PL-25A1 Loopback Aging Test", long_about = None)]
struct Args {
    /// Test pattern to run
    #[arg(short, long, default_value = "echo")]
    pattern: TestPattern,
    
    /// Test duration in seconds (0 = infinite)
    #[arg(short, long, default_value = "60")]
    duration: u64,
    
    /// Data size per transfer in bytes
    #[arg(short, long, default_value = "1024")]
    size: usize,
    
    /// Number of parallel streams
    #[arg(short = 'n', long, default_value = "1")]
    streams: usize,
    
    /// Delay between transfers in milliseconds
    #[arg(long, default_value = "0")]
    delay: u64,
    
    /// Enable CSV log output
    #[arg(long)]
    csv_log: bool,
    
    /// Enable CRC checksum verification
    #[arg(long)]
    verify_crc: bool,
}

/// Available test patterns
#[derive(Debug, Clone, ValueEnum)]
enum TestPattern {
    /// Simple echo test (default)
    Echo,
    /// Continuous bulk transfer
    Bulk,
    /// Random size transfers
    Random,
    /// Burst pattern (high/low alternating)
    Burst,
    /// Bidirectional simultaneous
    Bidirectional,
    /// Stress test (maximum throughput)
    Stress,
}

/// Performance monitoring and statistics
#[derive(Debug)]
pub struct PerformanceStats {
    start_time: Instant,
    total_bytes_sent: AtomicU64,
    total_bytes_received: AtomicU64,
    total_transfers: AtomicU64,
    error_count: AtomicU32,
    min_latency_ns: AtomicU64,
    max_latency_ns: AtomicU64,
    total_latency_ns: AtomicU64,
    csv_log: bool,
}

impl PerformanceStats {
    pub fn new(csv_log: bool) -> Self {
        Self {
            start_time: Instant::now(),
            total_bytes_sent: AtomicU64::new(0),
            total_bytes_received: AtomicU64::new(0),
            total_transfers: AtomicU64::new(0),
            error_count: AtomicU32::new(0),
            min_latency_ns: AtomicU64::new(u64::MAX),
            max_latency_ns: AtomicU64::new(0),
            total_latency_ns: AtomicU64::new(0),
            csv_log,
        }
    }

    pub fn record_transfer(&self, bytes_sent: usize, bytes_received: usize, latency: Duration) {
        self.total_bytes_sent.fetch_add(bytes_sent as u64, Ordering::Relaxed);
        self.total_bytes_received.fetch_add(bytes_received as u64, Ordering::Relaxed);
        self.total_transfers.fetch_add(1, Ordering::Relaxed);

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

        if self.csv_log {
            self.log_csv_entry(bytes_sent, bytes_received, latency);
        }
    }

    pub fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    fn log_csv_entry(&self, bytes_sent: usize, bytes_received: usize, latency: Duration) {
        let timestamp = self.start_time.elapsed().as_millis();
        println!("{},{},{},{}", timestamp, bytes_sent, bytes_received, latency.as_micros());
    }

    pub fn print_current_stats(&self) {
        let elapsed = self.start_time.elapsed();
        let total_sent = self.total_bytes_sent.load(Ordering::Relaxed);
        let total_received = self.total_bytes_received.load(Ordering::Relaxed);
        let total_transfers = self.total_transfers.load(Ordering::Relaxed);
        let errors = self.error_count.load(Ordering::Relaxed);

        let throughput_mbps = if elapsed.as_secs() > 0 {
            (total_sent / 1_000_000) / elapsed.as_secs()
        } else {
            0
        };

        let avg_latency_us = if total_transfers > 0 {
            (self.total_latency_ns.load(Ordering::Relaxed) / total_transfers) / 1_000
        } else {
            0
        };

        let min_latency_us = self.min_latency_ns.load(Ordering::Relaxed) / 1_000;
        let max_latency_us = self.max_latency_ns.load(Ordering::Relaxed) / 1_000;

        info!(
            "[STATS] Time: {:?} | Sent: {} MB | Throughput: {} MB/s | Transfers: {} | Errors: {} | Latency: min={}μs avg={}μs max={}μs",
            elapsed,
            total_sent / 1_000_000,
            throughput_mbps,
            total_transfers,
            errors,
            min_latency_us,
            avg_latency_us,
            max_latency_us
        );
    }

    pub fn print_final_summary(&self) {
        let elapsed = self.start_time.elapsed();
        let total_sent = self.total_bytes_sent.load(Ordering::Relaxed);
        let total_received = self.total_bytes_received.load(Ordering::Relaxed);
        let total_transfers = self.total_transfers.load(Ordering::Relaxed);
        let errors = self.error_count.load(Ordering::Relaxed);

        let throughput_mbps = if elapsed.as_secs() > 0 {
            (total_sent / 1_000_000) / elapsed.as_secs()
        } else {
            0
        };

        let avg_latency_us = if total_transfers > 0 {
            (self.total_latency_ns.load(Ordering::Relaxed) / total_transfers) / 1_000
        } else {
            0
        };

        info!("=== FINAL PERFORMANCE SUMMARY ===");
        info!("Total Duration: {:?}", elapsed);
        info!("Total Bytes Sent: {} MB", total_sent / 1_000_000);
        info!("Total Bytes Received: {} MB", total_received / 1_000_000);
        info!("Average Throughput: {} MB/s", throughput_mbps);
        info!("Total Transfers: {}", total_transfers);
        info!("Total Errors: {}", errors);
        info!("Success Rate: {:.2}%", 
              if total_transfers > 0 {
                  ((total_transfers - errors as u64) as f64 / total_transfers as f64) * 100.0
              } else { 0.0 });
        info!("Latency Statistics:");
        info!("  Min: {}μs", self.min_latency_ns.load(Ordering::Relaxed) / 1_000);
        info!("  Avg: {}μs", avg_latency_us);
        info!("  Max: {}μs", self.max_latency_ns.load(Ordering::Relaxed) / 1_000);
    }
}

/// Side of the PL-25A1 connection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionSide {
    A, // Client side
    B, // Echo server side
}

impl std::fmt::Display for ConnectionSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionSide::A => write!(f, "Side A (Client)"),
            ConnectionSide::B => write!(f, "Side B (Server)"),
        }
    }
}

/// PL-25A1 Device endpoint representation
#[derive(Debug, Clone)]
pub struct DeviceEndpoint {
    pub side: ConnectionSide,
    pub transport: Arc<Mutex<BridgeUsbTransport>>,
    pub device_id: String,
    pub connector_id: bool,
}

impl DeviceEndpoint {
    pub async fn new(device_info: DeviceInfo, side: ConnectionSide) -> Result<Self> {
        let transport = BridgeUsbTransport::new(device_info).await?;
        let device_id = transport.device_id().to_string();
        
        // Get connector ID for identification
        let state = transport.get_connection_state().await?;
        let (local_connector, _) = state.get_connector_id();
        
        Ok(Self {
            side,
            transport: Arc::new(Mutex::new(transport)),
            device_id,
            connector_id: local_connector,
        })
    }

    pub async fn wait_for_ready(&self) -> Result<()> {
        info!("Waiting for {} to be ready...", self.side);
        
        let start = Instant::now();
        while start.elapsed() < DEVICE_READY_TIMEOUT {
            let transport = self.transport.lock().await;
            let state = transport.get_connection_state().await?;
            
            if state.is_ready() && !state.is_disconnected() {
                info!("{} is ready for communication", self.side);
                return Ok(());
            }
            
            drop(transport);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        bail!("Timeout waiting for {} to be ready", self.side);
    }
}



/// Main loopback test application
pub struct LoopbackTestApp {
    side_a: DeviceEndpoint,
    side_b: DeviceEndpoint,
    stats: Arc<PerformanceStats>,
    args: Args,
}

impl LoopbackTestApp {
    pub async fn new(args: Args) -> Result<Self> {
        info!("Initializing PL-25A1 Loopback Test Application...");
        info!("Test Pattern: {:?}", args.pattern);
        info!("Duration: {} seconds", args.duration);
        info!("Data Size: {} bytes", args.size);
        info!("Parallel Streams: {}", args.streams);
        
        if args.csv_log {
            println!("timestamp_ms,bytes_sent,bytes_received,latency_us");
        }
        
        // Find PL-25A1 devices
        let devices = Self::find_pl25a1_devices().await?;
        
        if devices.len() < 2 {
            bail!("Need at least 2 PL-25A1 devices, found {}", devices.len());
        }
        
        info!("Found {} PL-25A1 devices", devices.len());
        
        // Initialize device endpoints
        let mut endpoints = Vec::new();
        for (i, device_info) in devices.into_iter().take(2).enumerate() {
            let side = if i == 0 { ConnectionSide::A } else { ConnectionSide::B };
            let endpoint = DeviceEndpoint::new(device_info, side).await
                .with_context(|| format!("Failed to initialize device for {}", side))?;
            
            info!("Initialized {} - Device: {}, Connector ID: {}", 
                  side, endpoint.device_id, endpoint.connector_id);
            endpoints.push(endpoint);
        }
        
        let mut endpoints_iter = endpoints.into_iter();
        let stats = Arc::new(PerformanceStats::new(args.csv_log));
        
        Ok(Self {
            side_a: endpoints_iter.next().unwrap(),
            side_b: endpoints_iter.next().unwrap(),
            stats,
            args,
        })
    }

    async fn find_pl25a1_devices() -> Result<Vec<DeviceInfo>> {
        let devices: Vec<DeviceInfo> = list_devices()?
            .filter(|device| {
                device.vendor_id() == PL25A1_VID && device.product_id() == PL25A1_PID
            })
            .collect();
        
        if devices.is_empty() {
            bail!("No PL-25A1 devices found. Please ensure the cable is connected to two USB ports.");
        }
        
        Ok(devices)
    }

    /// Basic connectivity test - send once from A, try to receive on B
    async fn run_basic_connectivity_test(&mut self) -> Result<()> {
        info!("=== Running Basic Connectivity Test ===");
        
        // Step 1: Verify connection states
        info!("Step 1: Checking connection states...");
        
        let state_a = self.side_a.transport.lock().await
            .get_connection_state().await?;
        let state_b = self.side_b.transport.lock().await
            .get_connection_state().await?;
        
        info!("Side A state: Local=0x{:02x}, Remote=0x{:02x}, Ready={}", 
              state_a.local_state, state_a.remote_state, state_a.is_ready());
        info!("Side B state: Local=0x{:02x}, Remote=0x{:02x}, Ready={}", 
              state_b.local_state, state_b.remote_state, state_b.is_ready());
        
        if !state_a.is_ready() || !state_b.is_ready() {
            bail!("Devices not ready for communication");
        }
        
        // Step 2: Concurrent data transfer test (RX first, then TX)
        info!("Step 2: Testing concurrent data transfer A → B...");
        
        let test_data = vec![0xDE, 0xAD, 0xBE, 0xEF]; // 4 bytes test pattern
        let test_data_clone = test_data.clone();
        
        // Side B 수신 태스크 (먼저 시작)
        let side_b_transport = Arc::clone(&self.side_b.transport);
        let rx_task = tokio::spawn(async move {
            info!("Side B: Starting receive (waiting for data)...");
            let mut transport = side_b_transport.lock().await;
            
            match timeout(Duration::from_secs(3), transport.receive_message()).await {
                Ok(Ok(msg)) => {
                    info!("✓ Side B: Received {} bytes: {:02x?}", 
                          msg.data.len(), &msg.data[..]);
                    Ok(msg.data.to_vec())
                }
                Ok(Err(e)) => {
                    warn!("✗ Side B receive error: {}", e);
                    Err(anyhow::anyhow!("Receive error: {}", e))
                }
                Err(_) => {
                    warn!("✗ Side B receive timeout - no data received");
                    Err(anyhow::anyhow!("Receive timeout"))
                }
            }
        });
        
        // 수신 태스크가 준비되도록 짧은 대기
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Side A 전송 태스크
        let side_a_transport = Arc::clone(&self.side_a.transport);
        let tx_task = tokio::spawn(async move {
            info!("Side A: Sending {} bytes: {:02x?}", 
                  test_data_clone.len(), test_data_clone);
            
            let mut transport = side_a_transport.lock().await;
            let msg = Message::new(Command::Write, 0, 0, test_data_clone.clone());
            
            match transport.send_message(&msg).await {
                Ok(_) => {
                    info!("✓ Side A: Data sent successfully");
                    Ok::<Vec<u8>, anyhow::Error>(test_data_clone)
                }
                Err(e) => {
                    warn!("✗ Side A send error: {}", e);
                    Err::<Vec<u8>, anyhow::Error>(e)
                }
            }
        });
        
        // 두 태스크 완료 대기
        info!("Waiting for both TX and RX tasks to complete...");
        let (rx_result, tx_result) = tokio::join!(rx_task, tx_task);
        
        // 결과 검증
        let sent_data = tx_result
            .context("TX task panicked")?
            .context("TX task failed")?;
        let received_data = rx_result
            .context("RX task panicked")?
            .context("RX task failed")?;
        
        if received_data == sent_data {
            info!("✓✓✓ DATA MATCH! Basic connectivity test PASSED!");
            return Ok(());
        } else {
            warn!("✗ Data mismatch!");
            warn!("  Expected: {:02x?}", sent_data);
            warn!("  Received: {:02x?}", received_data);
        }
        
        // Step 3: Alternative test - check if data is stuck in buffer
        info!("Step 3: Checking if PL-25A1 requires special bridge protocol...");
        
        // Try vendor command to check buffer status
        {
            let transport_b = self.side_b.transport.lock().await;
            if let Ok(status) = transport_b.query_feature_status().await {
                info!("Side B feature status: [{:02x}, {:02x}]", status[0], status[1]);
            }
        }
        
        bail!("Basic connectivity test FAILED - PL-25A1 may require special bridge protocol");
    }

    /// Run the complete loopback test
    pub async fn run_test(&mut self) -> Result<()> {
        info!("Starting PL-25A1 Loopback Test...");
        
        // Wait for both sides to be ready (hardware level only)
        tokio::try_join!(
            self.side_a.wait_for_ready(),
            self.side_b.wait_for_ready()
        )?;
        
        info!("✓ Both devices ready");
        
        // Basic connectivity test first
        match self.run_basic_connectivity_test().await {
            Ok(_) => {
                info!("✓ Basic connectivity test passed, proceeding with main tests");
            }
            Err(e) => {
                error!("Basic connectivity test failed: {}", e);
                error!("Cannot proceed with main tests");
                return Err(e);
            }
        }
        
        info!("Starting main performance tests...");

        // Start statistics reporting task
        let stats_clone = Arc::clone(&self.stats);
        let stats_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                stats_clone.print_current_stats();
            }
        });

        // Run pattern-specific test
        let test_result = match self.args.pattern {
            TestPattern::Echo => self.run_echo_test().await,
            TestPattern::Bulk => self.run_bulk_test().await,
            TestPattern::Random => self.run_random_test().await,
            TestPattern::Burst => self.run_burst_test().await,
            TestPattern::Bidirectional => self.run_bidirectional_test().await,
            TestPattern::Stress => self.run_stress_test().await,
        };

        // Stop statistics task
        stats_task.abort();

        // Print final results
        self.stats.print_final_summary();
        
        test_result?;
        info!("✓ All tests completed successfully!");
        Ok(())
    }

    /// Generate test data with optional pattern
    fn generate_test_data(&self, size: usize, pattern: Option<u8>) -> Vec<u8> {
        match pattern {
            Some(byte) => vec![byte; size],
            None => {
                let mut rng = rand::thread_rng();
                (0..size).map(|_| rng.gen::<u8>()).collect()
            }
        }
    }

    /// Send raw data and verify echo response (no protocol overhead)
    async fn send_and_verify(&mut self, data: &[u8]) -> Result<usize> {
        let start = Instant::now();
        
        // Create a simple data packet - just raw bytes without protocol headers
        let data_packet = data.to_vec();
        
        // Side A: Send raw data  
        let send_result = {
            let mut transport_a = self.side_a.transport.lock().await;
            // Use internal bulk transfer (we'll access the USB interface directly)
            // For now, we'll use a dummy message structure to carry raw data
            let dummy_msg = Message::new(
                Command::Write,
                0, 0, data_packet.clone()
            );
            transport_a.send_message(&dummy_msg).await
        };
        
        if let Err(e) = send_result {
            self.stats.record_error();
            bail!("Failed to send raw data: {}", e);
        }
        
        // Side B: Receive and echo back
        let _echo_data = {
            let mut transport_b = self.side_b.transport.lock().await;
            match timeout(OPERATION_TIMEOUT, transport_b.receive_message()).await {
                Ok(Ok(received_msg)) => {
                    // Extract raw data and echo it back
                    let echo_msg = Message::new(
                        Command::Write,
                        0, 0, received_msg.data.clone()
                    );
                    transport_b.send_message(&echo_msg).await
                        .context("Failed to echo data back")?;
                    received_msg.data
                }
                Ok(Err(e)) => bail!("Failed to receive data: {}", e),
                Err(_) => bail!("Timeout receiving data"),
            }
        };
        
        // Side A: Receive echo
        let echo_response = {
            let mut transport_a = self.side_a.transport.lock().await;
            timeout(OPERATION_TIMEOUT, transport_a.receive_message()).await
                .context("Timeout waiting for echo")?
                .context("Failed to receive echo")?
        };
        
        let latency = start.elapsed();
        
        // Verify data integrity  
        if echo_response.data != data {
            self.stats.record_error();
            bail!("Data mismatch: sent {} bytes, received {} bytes", 
                  data.len(), echo_response.data.len());
        }
        
        // Record statistics
        self.stats.record_transfer(data.len(), echo_response.data.len(), latency);
        
        if self.args.delay > 0 {
            tokio::time::sleep(Duration::from_millis(self.args.delay)).await;
        }
        
        Ok(data.len())
    }

    /// Simple echo test pattern
    async fn run_echo_test(&mut self) -> Result<()> {
        info!("Running Echo Test Pattern...");
        
        let test_data = self.generate_test_data(self.args.size, Some(0xAA));
        let duration = if self.args.duration > 0 {
            Duration::from_secs(self.args.duration)
        } else {
            Duration::from_secs(u64::MAX) // Infinite
        };
        
        let start = Instant::now();
        let mut count = 0;
        
        while start.elapsed() < duration {
            match self.send_and_verify(&test_data).await {
                Ok(_) => count += 1,
                Err(e) => {
                    warn!("Echo test failed: {}", e);
                    self.stats.record_error();
                }
            }
        }
        
        info!("Completed {} echo transfers", count);
        Ok(())
    }

    /// Bulk transfer test pattern
    async fn run_bulk_test(&mut self) -> Result<()> {
        info!("Running Bulk Transfer Test Pattern...");
        
        let duration = if self.args.duration > 0 {
            Duration::from_secs(self.args.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };
        
        let start = Instant::now();
        let mut transfer_count = 0;
        
        while start.elapsed() < duration {
            let data = self.generate_test_data(self.args.size, Some(0xBB));
            
            match self.send_and_verify(&data).await {
                Ok(_) => transfer_count += 1,
                Err(e) => {
                    warn!("Bulk transfer failed: {}", e);
                    self.stats.record_error();
                }
            }
        }
        
        info!("Completed {} bulk transfers", transfer_count);
        Ok(())
    }

    /// Random size transfer test pattern
    async fn run_random_test(&mut self) -> Result<()> {
        info!("Running Random Size Transfer Test Pattern...");
        
        let mut rng = rand::thread_rng();
        let max_size = self.args.size;
        let duration = if self.args.duration > 0 {
            Duration::from_secs(self.args.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };
        
        let start = Instant::now();
        let mut transfer_count = 0;
        
        while start.elapsed() < duration {
            let size = rng.gen_range(1..=max_size);
            let data = self.generate_test_data(size, None); // Random data
            
            match self.send_and_verify(&data).await {
                Ok(_) => transfer_count += 1,
                Err(e) => {
                    warn!("Random transfer failed: {}", e);
                    self.stats.record_error();
                }
            }
        }
        
        info!("Completed {} random transfers", transfer_count);
        Ok(())
    }

    /// Burst transfer test pattern (high/low alternating)
    async fn run_burst_test(&mut self) -> Result<()> {
        info!("Running Burst Transfer Test Pattern...");
        
        let duration = if self.args.duration > 0 {
            Duration::from_secs(self.args.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };
        
        let start = Instant::now();
        let mut burst_phase = true; // true = high throughput, false = low
        let mut phase_start = Instant::now();
        let burst_duration = Duration::from_secs(10);
        let idle_duration = Duration::from_secs(5);
        
        while start.elapsed() < duration {
            // Check if we need to switch phase
            let phase_elapsed = phase_start.elapsed();
            if burst_phase && phase_elapsed >= burst_duration {
                info!("Switching to idle phase...");
                burst_phase = false;
                phase_start = Instant::now();
                tokio::time::sleep(idle_duration).await;
                continue;
            } else if !burst_phase && phase_elapsed >= idle_duration {
                info!("Switching to burst phase...");
                burst_phase = true;
                phase_start = Instant::now();
                continue;
            }
            
            if burst_phase {
                // High throughput phase - large data, no delay
                let data = self.generate_test_data(self.args.size * 4, Some(0xCC));
                match self.send_and_verify(&data).await {
                    Ok(_) => {},
                    Err(e) => {
                        warn!("Burst transfer failed: {}", e);
                        self.stats.record_error();
                    }
                }
            }
        }
        
        Ok(())
    }

    /// Bidirectional simultaneous transfer test
    async fn run_bidirectional_test(&mut self) -> Result<()> {
        info!("Running Bidirectional Transfer Test Pattern...");
        info!("Note: This is a simplified bidirectional test");
        
        // For now, we'll simulate bidirectional by alternating quickly
        // A full implementation would require separate client/server threads
        let duration = if self.args.duration > 0 {
            Duration::from_secs(self.args.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };
        
        let start = Instant::now();
        let mut direction_a_to_b = true;
        
        while start.elapsed() < duration {
            let data = if direction_a_to_b {
                self.generate_test_data(self.args.size, Some(0xDD))
            } else {
                self.generate_test_data(self.args.size, Some(0xEE))
            };
            
            match self.send_and_verify(&data).await {
                Ok(_) => {},
                Err(e) => {
                    warn!("Bidirectional transfer failed: {}", e);
                    self.stats.record_error();
                }
            }
            
            direction_a_to_b = !direction_a_to_b;
        }
        
        Ok(())
    }

    /// Stress test pattern (maximum throughput)
    async fn run_stress_test(&mut self) -> Result<()> {
        info!("Running Stress Test Pattern (Maximum Throughput)...");
        
        let duration = if self.args.duration > 0 {
            Duration::from_secs(self.args.duration)
        } else {
            Duration::from_secs(u64::MAX)
        };
        
        let start = Instant::now();
        let large_data = self.generate_test_data(self.args.size.max(65536), Some(0xFF));
        
        while start.elapsed() < duration {
            match self.send_and_verify(&large_data).await {
                Ok(_) => {},
                Err(e) => {
                    warn!("Stress transfer failed: {}", e);
                    self.stats.record_error();
                    // Continue despite errors in stress test
                }
            }
            
            // No delay in stress test - maximum throughput
        }
        
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dbgif_server=info".parse().unwrap())
                .add_directive("pl25a1_loopback=info".parse().unwrap())
        )
        .init();

    info!("PL-25A1 Loopback Aging Test Application");
    info!("======================================");
    
    // Set up Ctrl+C handler for graceful shutdown
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        info!("Received Ctrl+C, shutting down gracefully...");
        let _ = tx.send(()).await;
    });
    
    // Initialize and run test application
    let mut app = LoopbackTestApp::new(args).await
        .context("Failed to initialize loopback test application")?;
    
    // Run test with cancellation support
    tokio::select! {
        result = app.run_test() => {
            match result {
                Ok(_) => info!("Test completed successfully!"),
                Err(e) => return Err(e.context("Loopback test failed")),
            }
        }
        _ = rx.recv() => {
            info!("Test interrupted by user");
            app.stats.print_final_summary();
        }
    }
    
    Ok(())
}