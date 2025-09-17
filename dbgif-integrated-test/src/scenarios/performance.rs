use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{info, debug, warn};

use crate::{
    client::TestClient,
    device::{DeviceConfig, EmbeddedDeviceServer},
    scenarios::Scenario,
};

/// Test throughput performance
pub struct ThroughputScenario {
    server_addr: SocketAddr,
    data_size: usize,
    duration: Duration,
}

impl ThroughputScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self {
            server_addr,
            data_size: 4096,
            duration: Duration::from_secs(10),
        }
    }

    pub fn with_data_size(mut self, size: usize) -> Self {
        self.data_size = size;
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

#[async_trait]
impl Scenario for ThroughputScenario {
    fn name(&self) -> &str {
        "throughput_test"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Throughput Performance Test ===");
        info!("Data size: {} bytes", self.data_size);
        info!("Test duration: {:?}", self.duration);

        // Start device
        let device_config = DeviceConfig {
            device_id: "throughput-device".to_string(),
            port: None,
            model: Some("ThroughputDevice".to_string()),
            capabilities: Some(vec!["high_speed".to_string()]),
        };
        let device = EmbeddedDeviceServer::spawn(device_config).await?;

        // Connect client
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;
        client.connect_device("127.0.0.1", device.port()).await?;

        // Prepare test data
        let test_data = vec![0x42u8; self.data_size];
        let mut bytes_sent = 0u64;
        let mut messages_sent = 0u64;

        let start_time = Instant::now();
        let end_time = start_time + self.duration;

        info!("Starting throughput measurement...");

        while Instant::now() < end_time {
            // Send data
            match client.ascii()?.send_strm(1, &test_data).await {
                Ok(_) => {
                    bytes_sent += test_data.len() as u64;
                    messages_sent += 1;
                }
                Err(e) => {
                    warn!("Send error: {}", e);
                }
            }
        }

        let elapsed = start_time.elapsed();
        let throughput_mbps = (bytes_sent as f64 * 8.0) / (elapsed.as_secs_f64() * 1_000_000.0);
        let messages_per_sec = messages_sent as f64 / elapsed.as_secs_f64();

        info!("=== Throughput Test Results ===");
        info!("Duration: {:.2} seconds", elapsed.as_secs_f64());
        info!("Total bytes sent: {} bytes", bytes_sent);
        info!("Total messages: {}", messages_sent);
        info!("Throughput: {:.2} Mbps", throughput_mbps);
        info!("Messages/sec: {:.2}", messages_per_sec);

        info!("✅ Throughput test completed");
        Ok(())
    }
}

/// Test latency performance
pub struct LatencyScenario {
    server_addr: SocketAddr,
    iterations: usize,
}

impl LatencyScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self {
            server_addr,
            iterations: 100,
        }
    }

    pub fn with_iterations(mut self, iterations: usize) -> Self {
        self.iterations = iterations;
        self
    }
}

#[async_trait]
impl Scenario for LatencyScenario {
    fn name(&self) -> &str {
        "latency_test"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Latency Performance Test ===");
        info!("Iterations: {}", self.iterations);

        // Start device
        let device_config = DeviceConfig {
            device_id: "latency-device".to_string(),
            port: None,
            model: Some("LatencyDevice".to_string()),
            capabilities: None,
        };
        let device = EmbeddedDeviceServer::spawn(device_config).await?;

        // Connect client
        let mut client = TestClient::new(self.server_addr);
        client.connect().await?;
        client.connect_device("127.0.0.1", device.port()).await?;

        let mut latencies = Vec::new();
        let test_data = b"ping";

        info!("Measuring latency...");

        for i in 0..self.iterations {
            let start = Instant::now();

            // Send ping
            client.ascii()?.send_strm((i % 3 + 1) as u8, test_data).await?;

            // Wait for response (simulated echo)
            match tokio::time::timeout(
                Duration::from_millis(100),
                client.ascii()?.receive_strm()
            ).await {
                Ok(Ok((_, _))) => {
                    let latency = start.elapsed();
                    latencies.push(latency);
                    debug!("Iteration {}: {:?}", i, latency);
                }
                Ok(Err(e)) => {
                    warn!("Receive error: {}", e);
                }
                Err(_) => {
                    warn!("Timeout on iteration {}", i);
                }
            }
        }

        // Calculate statistics
        if !latencies.is_empty() {
            latencies.sort();
            let total: Duration = latencies.iter().sum();
            let avg = total / latencies.len() as u32;
            let min = latencies.first().unwrap();
            let max = latencies.last().unwrap();
            let p50 = &latencies[latencies.len() / 2];
            let p95 = &latencies[latencies.len() * 95 / 100];
            let p99 = &latencies[latencies.len() * 99 / 100];

            info!("=== Latency Test Results ===");
            info!("Samples: {}", latencies.len());
            info!("Min: {:?}", min);
            info!("Max: {:?}", max);
            info!("Average: {:?}", avg);
            info!("P50 (median): {:?}", p50);
            info!("P95: {:?}", p95);
            info!("P99: {:?}", p99);
        }

        info!("✅ Latency test completed");
        Ok(())
    }
}

/// Test connection limit
pub struct ConnectionLimitScenario {
    server_addr: SocketAddr,
    max_connections: usize,
}

impl ConnectionLimitScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self {
            server_addr,
            max_connections: 100,
        }
    }

    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }
}

#[async_trait]
impl Scenario for ConnectionLimitScenario {
    fn name(&self) -> &str {
        "connection_limit"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Connection Limit Test ===");
        info!("Testing up to {} connections", self.max_connections);

        // Start a device
        let device_config = DeviceConfig {
            device_id: "limit-device".to_string(),
            port: None,
            model: Some("LimitDevice".to_string()),
            capabilities: None,
        };
        let device = EmbeddedDeviceServer::spawn(device_config).await?;

        let successful_connections = Arc::new(AtomicU64::new(0));
        let failed_connections = Arc::new(AtomicU64::new(0));
        let mut handles = Vec::new();

        info!("Creating {} concurrent connections...", self.max_connections);

        for i in 0..self.max_connections {
            let server_addr = self.server_addr;
            let device_port = device.port();
            let success_count = successful_connections.clone();
            let fail_count = failed_connections.clone();

            let handle = tokio::spawn(async move {
                let mut client = TestClient::new(server_addr);

                match client.connect().await {
                    Ok(_) => {
                        // Try to connect to device
                        match client.connect_device("127.0.0.1", device_port).await {
                            Ok(_) => {
                                success_count.fetch_add(1, Ordering::Relaxed);
                                debug!("Connection {} successful", i);

                                // Keep connection alive
                                tokio::time::sleep(Duration::from_secs(2)).await;
                            }
                            Err(e) => {
                                fail_count.fetch_add(1, Ordering::Relaxed);
                                debug!("Connection {} failed: {}", i, e);
                            }
                        }
                    }
                    Err(e) => {
                        fail_count.fetch_add(1, Ordering::Relaxed);
                        debug!("Connection {} failed at server: {}", i, e);
                    }
                }
            });

            handles.push(handle);

            // Small delay to avoid overwhelming
            if i % 10 == 9 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        // Wait for all connections
        for handle in handles {
            let _ = handle.await;
        }

        let successful = successful_connections.load(Ordering::Relaxed);
        let failed = failed_connections.load(Ordering::Relaxed);

        info!("=== Connection Limit Results ===");
        info!("Successful connections: {}", successful);
        info!("Failed connections: {}", failed);
        info!("Success rate: {:.1}%", (successful as f64 / self.max_connections as f64) * 100.0);

        if successful == 0 {
            return Err(anyhow::anyhow!("No connections succeeded"));
        }

        info!("✅ Connection limit test completed");
        Ok(())
    }
}

/// Test memory leak detection
pub struct MemoryLeakScenario {
    server_addr: SocketAddr,
    iterations: usize,
}

impl MemoryLeakScenario {
    pub fn new(server_addr: SocketAddr) -> Self {
        Self {
            server_addr,
            iterations: 1000,
        }
    }
}

#[async_trait]
impl Scenario for MemoryLeakScenario {
    fn name(&self) -> &str {
        "memory_leak"
    }

    async fn execute(&self) -> Result<()> {
        info!("=== Memory Leak Detection Test ===");
        info!("Iterations: {}", self.iterations);

        // This test repeatedly creates and destroys connections/devices
        // to detect potential memory leaks

        for i in 0..self.iterations {
            if i % 100 == 0 {
                info!("Progress: {}/{}", i, self.iterations);
            }

            // Create and destroy a device
            let config = DeviceConfig {
                device_id: format!("leak-test-{}", i),
                port: None,
                model: Some("LeakTest".to_string()),
                capabilities: None,
            };
            let mut device = EmbeddedDeviceServer::spawn(config).await?;

            // Create and destroy a client connection
            let mut client = TestClient::new(self.server_addr);
            client.connect().await?;
            client.connect_device("127.0.0.1", device.port()).await?;

            // Send some data
            client.ascii()?.send_strm(1, b"test").await?;

            // Clean up
            client.disconnect().await?;
            device.stop().await?;

            // Small delay
            if i % 10 == 9 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }

        info!("Completed {} iterations", self.iterations);
        info!("Monitor system memory usage to detect leaks");
        info!("✅ Memory leak test completed");
        Ok(())
    }
}