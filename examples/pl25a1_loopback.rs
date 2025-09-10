use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use dbgif_server::transport::{bridge_usb::BridgeUsbTransport, Transport};
use nusb::{list_devices, DeviceInfo, MaybeFuture};
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
    #[arg(short, long, default_value = "65536")]
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
                  side, endpoint.device_id, if endpoint.connector_id { "B" } else { "A" });
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
        let devices: Vec<DeviceInfo> = list_devices().wait()?
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
            
            match timeout(Duration::from_secs(3), transport.receive(4096)).await {
                Ok(Ok(raw_data)) => {
                    info!("✓ Side B: Received {} bytes: {:02x?}", 
                          raw_data.len(), &raw_data[..raw_data.len().min(8)]);
                    Ok(raw_data)
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
            
            match transport.send(&test_data_clone).await {
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

    /// Simple one-way transfer test (A sends, B receives continuously)
    async fn run_simple_oneway_test(&mut self) -> Result<()> {
        info!("=== Running Simple One-Way Transfer Test ===");
        info!("Side A will send continuously, Side B will receive continuously");
        
        let test_duration = Duration::from_secs(10); // Fixed 10 second test
        let test_data = vec![0x55; self.args.size]; // Fixed pattern
        let start_time = Instant::now();
        
        // Statistics tracking
        let tx_count = Arc::new(AtomicU64::new(0));
        let rx_count = Arc::new(AtomicU64::new(0));
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));
        
        // Task 1: Side A continuous TX loop
        let side_a_transport = Arc::clone(&self.side_a.transport);
        let tx_data = test_data.clone();
        let tx_count_clone = Arc::clone(&tx_count);
        let tx_bytes_clone = Arc::clone(&tx_bytes);
        let tx_task = tokio::spawn(async move {
            info!("Side A: Starting continuous TX loop...");
            let mut local_count = 0u64;
            let mut local_bytes = 0u64;
            
            while start_time.elapsed() < test_duration {
                let mut transport = side_a_transport.lock().await;
                match transport.send(&tx_data).await {
                    Ok(_) => {
                        local_count += 1;
                        local_bytes += tx_data.len() as u64;
                        
                        // Update global counters every 100 transfers
                        if local_count % 100 == 0 {
                            tx_count_clone.fetch_add(100, Ordering::Relaxed);
                            tx_bytes_clone.fetch_add(tx_data.len() as u64 * 100, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        warn!("TX failed: {}", e);
                        break;
                    }
                }
                drop(transport); // Release lock between sends
                tokio::task::yield_now().await; // Allow other tasks to run
            }
            
            // Final update
            tx_count_clone.store(local_count, Ordering::Relaxed);
            tx_bytes_clone.store(local_bytes, Ordering::Relaxed);
            info!("Side A: TX loop completed - {} transfers, {} bytes", local_count, local_bytes);
        });
        
        // Task 2: Side B continuous RX loop
        let side_b_transport = Arc::clone(&self.side_b.transport);
        let rx_count_clone = Arc::clone(&rx_count);
        let rx_bytes_clone = Arc::clone(&rx_bytes);
        let expected_size = test_data.len();
        let rx_task = tokio::spawn(async move {
            info!("Side B: Starting continuous RX loop...");
            let mut local_count = 0u64;
            let mut local_bytes = 0u64;
            
            while start_time.elapsed() < test_duration {
                let mut transport = side_b_transport.lock().await;
                match transport.receive(expected_size + 1024).await {
                    Ok(received_data) => {
                        local_count += 1;
                        local_bytes += received_data.len() as u64;
                        
                        // Update global counters every 100 transfers
                        if local_count % 100 == 0 {
                            rx_count_clone.fetch_add(100, Ordering::Relaxed);
                            rx_bytes_clone.fetch_add(received_data.len() as u64 * 100, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        warn!("RX failed: {}", e);
                        tokio::time::sleep(Duration::from_millis(1)).await; // Brief pause on error
                    }
                }
                drop(transport); // Release lock between receives
                tokio::task::yield_now().await; // Allow other tasks to run
            }
            
            // Final update
            rx_count_clone.store(local_count, Ordering::Relaxed);
            rx_bytes_clone.store(local_bytes, Ordering::Relaxed);
            info!("Side B: RX loop completed - {} transfers, {} bytes", local_count, local_bytes);
        });
        
        // Wait for both tasks to complete
        let (tx_result, rx_result) = tokio::join!(tx_task, rx_task);
        
        // Check results
        if let Err(e) = tx_result {
            warn!("TX task failed: {}", e);
        }
        if let Err(e) = rx_result {
            warn!("RX task failed: {}", e);
        }
        
        // Print final statistics
        let final_tx_count = tx_count.load(Ordering::Relaxed);
        let final_rx_count = rx_count.load(Ordering::Relaxed);
        let final_tx_bytes = tx_bytes.load(Ordering::Relaxed);
        let final_rx_bytes = rx_bytes.load(Ordering::Relaxed);
        let elapsed = start_time.elapsed();
        
        info!("=== One-Way Transfer Test Results ===");
        info!("Duration: {:?}", elapsed);
        info!("TX: {} transfers, {} MB ({} MB/s)", 
              final_tx_count, 
              final_tx_bytes / 1_000_000,
              if elapsed.as_secs() > 0 { (final_tx_bytes / 1_000_000) / elapsed.as_secs() } else { 0 });
        info!("RX: {} transfers, {} MB ({} MB/s)", 
              final_rx_count, 
              final_rx_bytes / 1_000_000,
              if elapsed.as_secs() > 0 { (final_rx_bytes / 1_000_000) / elapsed.as_secs() } else { 0 });
        info!("Transfer efficiency: {:.1}% ({}/{} packets)", 
              if final_tx_count > 0 { (final_rx_count as f64 / final_tx_count as f64) * 100.0 } else { 0.0 },
              final_rx_count, final_tx_count);
        
        Ok(())
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
                info!("✓ Basic connectivity test passed, proceeding with one-way test");
            }
            Err(e) => {
                error!("Basic connectivity test failed: {}", e);
                error!("Cannot proceed with main tests");
                return Err(e);
            }
        }
        
        // Simple one-way transfer test
        match self.run_simple_oneway_test().await {
            Ok(_) => {
                info!("✓ One-way transfer test completed, proceeding with main tests");
            }
            Err(e) => {
                warn!("One-way transfer test failed: {}", e);
                warn!("Continuing with main tests anyway...");
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

    /// Send raw data and verify echo response (maximum speed, concurrent tasks)
    async fn send_and_verify(&mut self, data: &[u8]) -> Result<usize> {
        let start = Instant::now();
        let data_len = data.len();
        let data_clone = data.to_vec();
        
        // Task 1: Side B RX (start first to be ready for incoming data)
        let side_b_transport_rx = Arc::clone(&self.side_b.transport);
        let rx_task = tokio::spawn(async move {
            let mut transport = side_b_transport_rx.lock().await;
            transport.receive(data_len + 1024).await
        });
        
        // Small delay to ensure RX is ready
        tokio::time::sleep(Duration::from_millis(1)).await;
        
        // Task 2: Side A TX (send data)
        let side_a_transport_tx = Arc::clone(&self.side_a.transport);
        let data_for_tx = data_clone.clone();
        let tx_task = tokio::spawn(async move {
            let mut transport = side_a_transport_tx.lock().await;
            transport.send(&data_for_tx).await
        });
        
        // Wait for RX to complete, then start echo
        let received_data = rx_task.await
            .context("RX task failed")?
            .context("Failed to receive data")?;
        
        // Task 3: Side B TX (echo back)
        let side_b_transport_tx = Arc::clone(&self.side_b.transport);
        let echo_task = tokio::spawn(async move {
            let mut transport = side_b_transport_tx.lock().await;
            transport.send(&received_data).await
        });
        
        // Task 4: Side A RX (receive echo)
        let side_a_transport_rx = Arc::clone(&self.side_a.transport);
        let final_rx_task = tokio::spawn(async move {
            let mut transport = side_a_transport_rx.lock().await;
            transport.receive(data_len + 1024).await
        });
        
        // Wait for all tasks to complete
        let (tx_result, echo_result, echo_response_result) = tokio::join!(
            tx_task,
            echo_task,
            final_rx_task
        );
        
        // Check results
        tx_result.context("TX task failed")?.context("Failed to send data")?;
        echo_result.context("Echo task failed")?.context("Failed to echo data")?;
        let echo_response = echo_response_result
            .context("Final RX task failed")?
            .context("Failed to receive echo")?;
        
        let latency = start.elapsed();
        
        // Quick data integrity check
        if echo_response.len() != data.len() || echo_response != data {
            self.stats.record_error();
            bail!("Data mismatch: sent {} bytes, received {} bytes", 
                  data.len(), echo_response.len());
        }
        
        // Record statistics
        self.stats.record_transfer(data.len(), echo_response.len(), latency);
        
        // Only add delay if explicitly requested
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