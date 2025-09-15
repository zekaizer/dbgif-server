use anyhow::{Result, Context};
use crate::test_client::{ConnectionManager, TestSession, TestType};
use std::time::{Duration, Instant};
use futures::future::join_all;
use tracing::{info, warn, debug, error, trace};
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct TestClientCli {
    verbose: bool,
    json_output: bool,
}

impl Default for TestClientCli {
    fn default() -> Self {
        Self::new()
    }
}

impl TestClientCli {
    pub fn new() -> Self {
        Self {
            verbose: false,
            json_output: false,
        }
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn json_output(mut self, json_output: bool) -> Self {
        self.json_output = json_output;
        self
    }

    pub async fn ping(&self, host: &str, port: u16, timeout: u64) -> Result<()> {
        info!(target: "test_client::ping",
              host = %host, port = %port, timeout = %timeout,
              "Starting ping test");

        let mut session = TestSession::new_with_type(TestType::Ping);
        session.start_test(&format!("ping-{}:{}", host, port));

        if self.verbose {
            println!("🔄 Starting ping test to {}:{} (timeout: {}s)", host, port, timeout);
        }

        let mut connection_manager = ConnectionManager::new();
        let start_time = Instant::now();

        session.increment_connection_count();
        session.start_connection_timing();

        // Connect with timeout
        let connect_result = tokio::time::timeout(
            Duration::from_secs(timeout),
            connection_manager.connect(host, port)
        ).await;

        match connect_result {
            Ok(Ok(())) => {
                session.end_connection_timing();
                session.increment_successful_connections();
                session.record_event("connection_established");
                if self.verbose {
                    println!("✅ Connected to {}:{} in {:.2}ms", host, port, start_time.elapsed().as_millis());
                }

                session.start_handshake_timing();
                // Attempt handshake
                let handshake_result = tokio::time::timeout(
                    Duration::from_secs(timeout / 2),
                    connection_manager.perform_handshake()
                ).await;

                match handshake_result {
                    Ok(Ok(())) => {
                        session.end_handshake_timing();
                        session.record_event("handshake_completed");
                        if self.verbose {
                            println!("✅ Handshake completed in {:.2}ms", start_time.elapsed().as_millis());
                        }

                        // Close connection
                        let _ = connection_manager.close().await;
                        session.record_event("connection_closed");
                    }
                    Ok(Err(e)) => {
                        session.record_event(&format!("handshake_failed: {}", e));
                        if self.verbose {
                            println!("❌ Handshake failed: {}", e);
                        }
                        session.fail_test(&format!("Handshake failed: {}", e));
                    }
                    Err(_) => {
                        session.record_event("handshake_timeout");
                        if self.verbose {
                            println!("⏰ Handshake timed out");
                        }
                        session.timeout_test();
                    }
                }
            }
            Ok(Err(e)) => {
                session.increment_failed_connections();
                session.record_event(&format!("connection_failed: {}", e));
                if self.verbose {
                    println!("❌ Connection failed: {}", e);
                }
                session.fail_test(&format!("Connection failed: {}", e));
            }
            Err(_) => {
                session.increment_failed_connections();
                session.record_event("connection_timeout");
                if self.verbose {
                    println!("⏰ Connection timed out after {}s", timeout);
                }
                session.timeout_test();
            }
        }

        session.end_test();
        let result = session.get_result();

        if self.json_output {
            // JSON output for automation scripts
            match result.to_json() {
                Ok(json) => println!("{}", json),
                Err(e) => eprintln!("Failed to serialize result to JSON: {}", e),
            }
        } else if self.verbose {
            println!("📊 Test result: {}", result.summary());
        }

        if result.is_success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Ping test failed: {:?}", result.error()))
        }
    }

    pub async fn host_commands(&self, host: &str, port: u16) -> Result<()> {
        let mut session = TestSession::new_with_type(TestType::HostCommands);
        session.start_test(&format!("host-commands-{}:{}", host, port));

        if self.verbose {
            println!("🔄 Testing host commands on {}:{}", host, port);
        }

        let mut connection_manager = ConnectionManager::new();

        // Connect and handshake
        connection_manager.connect(host, port).await
            .context("Failed to connect for host commands")?;
        session.record_event("connection_established");

        connection_manager.perform_handshake().await
            .context("Failed to perform handshake for host commands")?;
        session.record_event("handshake_completed");

        // For now, just test basic host commands without stream opening
        // This is a simplified test that verifies the connection works
        if self.verbose {
            println!("✅ Host commands test: connection and handshake successful");
        }
        session.record_event("host_commands_basic_test_completed");

        // Close connection
        let _ = connection_manager.close().await;
        session.record_event("connection_closed");

        session.end_test();
        let result = session.get_result();

        if self.json_output {
            // JSON output for automation scripts
            match result.to_json() {
                Ok(json) => println!("{}", json),
                Err(e) => eprintln!("Failed to serialize result to JSON: {}", e),
            }
        } else if self.verbose {
            println!("📊 Test result: {}", result.summary());
        }

        if result.is_success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Host commands test failed: {:?}", result.error()))
        }
    }

    pub async fn multi_connect(&self, host: &str, port: u16, count: u8) -> Result<()> {
        let count = count.min(10); // Enforce maximum
        let mut session = TestSession::new_with_type(TestType::MultiConnect);
        session.start_test(&format!("multi-connect-{}:{}-{}", host, port, count));

        if self.verbose {
            println!("🔄 Testing {} concurrent connections to {}:{}", count, host, port);
        }

        let start_time = Instant::now();

        // Create concurrent connection tasks
        let handles: Vec<_> = (0..count)
            .map(|i| {
                let host = host.to_string();
                let verbose = self.verbose;

                tokio::spawn(async move {
                    let mut connection_manager = ConnectionManager::new();
                    let conn_start = Instant::now();

                    // Connect
                    let connect_result = connection_manager.connect(&host, port).await;
                    if connect_result.is_err() {
                        return (i, false, conn_start.elapsed(), format!("Connection failed: {:?}", connect_result));
                    }

                    // Handshake
                    let handshake_result = connection_manager.perform_handshake().await;
                    if handshake_result.is_err() {
                        return (i, false, conn_start.elapsed(), format!("Handshake failed: {:?}", handshake_result));
                    }

                    // Brief operation
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    // Close
                    let _ = connection_manager.close().await;
                    let duration = conn_start.elapsed();

                    if verbose {
                        println!("✅ Connection {} completed in {:.2}ms", i, duration.as_millis());
                    }

                    (i, true, duration, "Success".to_string())
                })
            })
            .collect();

        // Wait for all connections to complete
        let results = join_all(handles).await;
        let total_duration = start_time.elapsed();

        let mut successful_connections = 0;
        let mut failed_connections = 0;
        let mut total_connection_time = Duration::ZERO;

        for result in results {
            match result {
                Ok((id, success, duration, message)) => {
                    total_connection_time += duration;
                    if success {
                        successful_connections += 1;
                        session.record_event(&format!("connection_{}_success", id));
                    } else {
                        failed_connections += 1;
                        session.record_event(&format!("connection_{}_failed: {}", id, message));
                        if self.verbose {
                            println!("❌ Connection {} failed: {}", id, message);
                        }
                    }
                }
                Err(e) => {
                    failed_connections += 1;
                    session.record_event(&format!("connection_task_panic: {}", e));
                    if self.verbose {
                        println!("💥 Connection task panicked: {}", e);
                    }
                }
            }
        }

        session.record_event(&format!("total_connections: {}, successful: {}, failed: {}",
                                     count, successful_connections, failed_connections));

        if self.verbose {
            println!("📊 Multi-connect results:");
            println!("  Total connections: {}", count);
            println!("  Successful: {} ({:.1}%)", successful_connections,
                    (successful_connections as f32 / count as f32) * 100.0);
            println!("  Failed: {}", failed_connections);
            println!("  Total time: {:.2}ms", total_duration.as_millis());
            println!("  Average per connection: {:.2}ms",
                    total_connection_time.as_millis() as f32 / count as f32);
        }

        session.end_test();
        let result = session.get_result();

        if self.json_output {
            // JSON output for automation scripts
            match result.to_json() {
                Ok(json) => println!("{}", json),
                Err(e) => eprintln!("Failed to serialize result to JSON: {}", e),
            }
        } else if self.verbose {
            println!("📊 Test result: {}", result.summary());
        }

        // Consider test successful if at least 50% of connections succeeded
        if successful_connections >= (count + 1) / 2 {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Multi-connect test failed: only {}/{} connections succeeded",
                              successful_connections, count))
        }
    }

    pub async fn aging(&self, host: &str, port: u16, duration: u64, packet_size: usize, interval: u64, connections: u8) -> Result<()> {
        let duration = Duration::from_secs(duration.min(3600)); // Max 1 hour
        let interval = Duration::from_millis(interval.max(10)); // Min 10ms interval
        let connections = connections.min(10).max(1); // 1-10 connections
        let packet_size = packet_size.min(65536).max(1); // 1B-64KB packets

        let mut session = TestSession::new_with_type(TestType::Aging);
        session.start_test(&format!("aging-{}:{}-{}s-{}b-{}ms-{}conn",
                                   host, port, duration.as_secs(), packet_size, interval.as_millis(), connections));

        if self.verbose {
            println!("🔄 Starting aging test:");
            println!("  Target: {}:{}", host, port);
            println!("  Duration: {}s", duration.as_secs());
            println!("  Packet size: {} bytes", packet_size);
            println!("  Interval: {}ms", interval.as_millis());
            println!("  Connections: {}", connections);
        }

        let start_time = Instant::now();
        let end_time = start_time + duration;

        // Create test data pattern
        let test_data = create_test_pattern(packet_size);

        // Start concurrent aging connections
        let handles: Vec<_> = (0..connections)
            .map(|conn_id| {
                let host = host.to_string();
                let test_data = test_data.clone();
                let verbose = self.verbose;

                tokio::spawn(async move {
                    aging_connection_worker(conn_id, &host, port, end_time, test_data, interval, verbose).await
                })
            })
            .collect();

        // Monitor progress
        let mut last_update = Instant::now();
        while Instant::now() < end_time {
            tokio::time::sleep(Duration::from_secs(1)).await;

            let elapsed = start_time.elapsed();
            let remaining = duration.saturating_sub(elapsed);

            if self.verbose && last_update.elapsed() >= Duration::from_secs(10) {
                println!("🕐 Aging test progress: {:.1}% ({}/{}s remaining)",
                        (elapsed.as_secs_f64() / duration.as_secs_f64()) * 100.0,
                        remaining.as_secs(),
                        duration.as_secs());
                last_update = Instant::now();
            }
        }

        // Wait for all connections to finish
        let results = join_all(handles).await;
        let total_duration = start_time.elapsed();

        // Analyze results
        let mut total_packets_sent = 0u64;
        let mut total_packets_received = 0u64;
        let mut total_errors = 0u64;
        let mut connection_failures = 0u64;

        for (conn_id, result) in results.iter().enumerate() {
            match result {
                Ok(stats) => {
                    total_packets_sent += stats.packets_sent;
                    total_packets_received += stats.packets_received;
                    total_errors += stats.errors;
                    if stats.connection_failed {
                        connection_failures += 1;
                    }
                    session.record_event(&format!("connection_{}_stats: sent={}, received={}, errors={}",
                                                 conn_id, stats.packets_sent, stats.packets_received, stats.errors));
                }
                Err(e) => {
                    connection_failures += 1;
                    session.record_event(&format!("connection_{}_panic: {}", conn_id, e));
                }
            }
        }

        let success_rate = if total_packets_sent > 0 {
            (total_packets_received as f64 / total_packets_sent as f64) * 100.0
        } else {
            0.0
        };

        session.record_event(&format!("aging_complete: duration={}s, packets_sent={}, packets_received={}, success_rate={:.2}%, errors={}, conn_failures={}",
                                     total_duration.as_secs(), total_packets_sent, total_packets_received, success_rate, total_errors, connection_failures));

        if self.verbose {
            println!("📊 Aging test results:");
            println!("  Total duration: {:.1}s", total_duration.as_secs_f64());
            println!("  Packets sent: {}", total_packets_sent);
            println!("  Packets received: {}", total_packets_received);
            println!("  Success rate: {:.2}%", success_rate);
            println!("  Total errors: {}", total_errors);
            println!("  Connection failures: {}", connection_failures);
            println!("  Throughput: {:.1} packets/sec", total_packets_sent as f64 / total_duration.as_secs_f64());
        }

        session.end_test();
        let result = session.get_result();

        if self.json_output {
            match result.to_json() {
                Ok(json) => println!("{}", json),
                Err(e) => eprintln!("Failed to serialize result to JSON: {}", e),
            }
        } else if self.verbose {
            println!("📊 Test result: {}", result.summary());
        }

        // Consider test successful if success rate > 95% and no connection failures
        if success_rate > 95.0 && connection_failures == 0 {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Aging test failed: success rate {:.2}%, {} connection failures",
                              success_rate, connection_failures))
        }
    }
}

#[derive(Debug, Clone)]
struct AgingStats {
    packets_sent: u64,
    packets_received: u64,
    errors: u64,
    connection_failed: bool,
}

async fn aging_connection_worker(
    conn_id: u8,
    host: &str,
    port: u16,
    end_time: Instant,
    test_data: Vec<u8>,
    interval: Duration,
    verbose: bool,
) -> AgingStats {
    let mut stats = AgingStats {
        packets_sent: 0,
        packets_received: 0,
        errors: 0,
        connection_failed: false,
    };

    // Connect to echo service
    let mut stream = match TcpStream::connect(format!("{}:{}", host, port)).await {
        Ok(stream) => {
            if verbose {
                println!("✅ Connection {} established to {}:{}", conn_id, host, port);
            }
            stream
        }
        Err(e) => {
            if verbose {
                println!("❌ Connection {} failed: {}", conn_id, e);
            }
            stats.connection_failed = true;
            return stats;
        }
    };

    // Send packets until end time
    while Instant::now() < end_time {
        // Send test data
        match stream.write_all(&test_data).await {
            Ok(_) => {
                stats.packets_sent += 1;

                // Read echo response
                let mut buffer = vec![0u8; test_data.len()];
                match tokio::time::timeout(Duration::from_secs(1), stream.read_exact(&mut buffer)).await {
                    Ok(Ok(_)) => {
                        if buffer == test_data {
                            stats.packets_received += 1;
                        } else {
                            stats.errors += 1;
                            if verbose {
                                println!("⚠️  Connection {} data corruption detected", conn_id);
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        stats.errors += 1;
                        if verbose {
                            println!("⚠️  Connection {} read error: {}", conn_id, e);
                        }
                    }
                    Err(_) => {
                        stats.errors += 1;
                        if verbose {
                            println!("⚠️  Connection {} read timeout", conn_id);
                        }
                    }
                }
            }
            Err(e) => {
                stats.errors += 1;
                if verbose {
                    println!("⚠️  Connection {} write error: {}", conn_id, e);
                }
            }
        }

        // Wait for next iteration
        tokio::time::sleep(interval).await;
    }

    if verbose {
        println!("📋 Connection {} finished: sent={}, received={}, errors={}",
                conn_id, stats.packets_sent, stats.packets_received, stats.errors);
    }

    stats
}

fn create_test_pattern(size: usize) -> Vec<u8> {
    // Create a repeating pattern for easy verification
    (0..size).map(|i| (i % 256) as u8).collect()
}