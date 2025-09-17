use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::time::Duration;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use dbgif_integrated_test::scenarios::{
    Scenario,
    ThroughputScenario,
    LatencyScenario,
    ConnectionLimitScenario,
    MemoryLeakScenario,
};

#[derive(Parser)]
#[command(name = "performance-test")]
#[command(version = "1.0.0")]
#[command(about = "DBGIF Performance Testing Suite")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// DBGIF server address
    #[arg(long, default_value = "127.0.0.1:5555")]
    server: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run throughput test
    Throughput {
        /// Data size per message (bytes)
        #[arg(long, default_value = "4096")]
        data_size: usize,

        /// Test duration (seconds)
        #[arg(long, default_value = "10")]
        duration: u64,
    },

    /// Run latency test
    Latency {
        /// Number of ping-pong iterations
        #[arg(long, default_value = "100")]
        iterations: usize,
    },

    /// Run connection limit test
    Connections {
        /// Maximum number of concurrent connections
        #[arg(long, default_value = "100")]
        max: usize,
    },

    /// Run memory leak detection test
    MemoryLeak {
        /// Number of connect/disconnect cycles
        #[arg(long, default_value = "1000")]
        iterations: usize,
    },

    /// Run all performance tests
    All {
        /// Skip memory leak test (slow)
        #[arg(long)]
        skip_memory: bool,
    },

    /// Run quick benchmark suite
    Benchmark,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    setup_logging(cli.verbose);

    // Parse server address
    let server_addr: SocketAddr = cli.server.parse()
        .map_err(|e| anyhow::anyhow!("Invalid server address '{}': {}", cli.server, e))?;

    info!("🚀 DBGIF Performance Testing Suite");
    info!("Server: {}", server_addr);

    match cli.command {
        Commands::Throughput { data_size, duration } => {
            run_throughput_test(server_addr, data_size, duration).await?;
        }
        Commands::Latency { iterations } => {
            run_latency_test(server_addr, iterations).await?;
        }
        Commands::Connections { max } => {
            run_connection_test(server_addr, max).await?;
        }
        Commands::MemoryLeak { iterations } => {
            run_memory_leak_test(server_addr, iterations).await?;
        }
        Commands::All { skip_memory } => {
            run_all_tests(server_addr, skip_memory).await?;
        }
        Commands::Benchmark => {
            run_benchmark_suite(server_addr).await?;
        }
    }

    Ok(())
}

fn setup_logging(verbose: bool) {
    let filter = if verbose {
        EnvFilter::new("info")
    } else {
        EnvFilter::new("warn")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .compact()
        )
        .init();
}

async fn run_throughput_test(
    server_addr: SocketAddr,
    data_size: usize,
    duration_secs: u64,
) -> Result<()> {
    info!("\n📊 Running Throughput Test");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let scenario = ThroughputScenario::new(server_addr)
        .with_data_size(data_size)
        .with_duration(Duration::from_secs(duration_secs));

    scenario.execute().await?;
    Ok(())
}

async fn run_latency_test(
    server_addr: SocketAddr,
    iterations: usize,
) -> Result<()> {
    info!("\n⏱️  Running Latency Test");
    info!("━━━━━━━━━━━━━━━━━━━━━━━");

    let scenario = LatencyScenario::new(server_addr)
        .with_iterations(iterations);

    scenario.execute().await?;
    Ok(())
}

async fn run_connection_test(
    server_addr: SocketAddr,
    max_connections: usize,
) -> Result<()> {
    info!("\n🔌 Running Connection Limit Test");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let scenario = ConnectionLimitScenario::new(server_addr)
        .with_max_connections(max_connections);

    scenario.execute().await?;
    Ok(())
}

async fn run_memory_leak_test(
    server_addr: SocketAddr,
    _iterations: usize,
) -> Result<()> {
    info!("\n🔍 Running Memory Leak Detection Test");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let scenario = MemoryLeakScenario::new(server_addr);

    // Note: MemoryLeakScenario doesn't have with_iterations method in current implementation
    // Using the scenario with default iterations
    scenario.execute().await?;

    info!("Note: Monitor system memory usage during test");
    Ok(())
}

async fn run_all_tests(
    server_addr: SocketAddr,
    skip_memory: bool,
) -> Result<()> {
    info!("\n🎯 Running All Performance Tests");
    info!("═══════════════════════════════════");

    // Throughput test
    if let Err(e) = run_throughput_test(server_addr, 4096, 10).await {
        error!("❌ Throughput test failed: {}", e);
    }

    // Latency test
    if let Err(e) = run_latency_test(server_addr, 100).await {
        error!("❌ Latency test failed: {}", e);
    }

    // Connection limit test
    if let Err(e) = run_connection_test(server_addr, 50).await {
        error!("❌ Connection test failed: {}", e);
    }

    // Memory leak test (optional)
    if !skip_memory {
        if let Err(e) = run_memory_leak_test(server_addr, 100).await {
            error!("❌ Memory leak test failed: {}", e);
        }
    }

    info!("\n✅ All tests completed");
    Ok(())
}

async fn run_benchmark_suite(server_addr: SocketAddr) -> Result<()> {
    info!("\n🏁 Running Benchmark Suite");
    info!("═══════════════════════════");
    info!("This will run a standardized set of performance tests");

    // Small message throughput
    info!("\n1️⃣ Small Message Throughput (256 bytes)");
    let scenario = ThroughputScenario::new(server_addr)
        .with_data_size(256)
        .with_duration(Duration::from_secs(5));
    if let Err(e) = scenario.execute().await {
        error!("Failed: {}", e);
    }

    // Medium message throughput
    info!("\n2️⃣ Medium Message Throughput (4KB)");
    let scenario = ThroughputScenario::new(server_addr)
        .with_data_size(4096)
        .with_duration(Duration::from_secs(5));
    if let Err(e) = scenario.execute().await {
        error!("Failed: {}", e);
    }

    // Large message throughput
    info!("\n3️⃣ Large Message Throughput (64KB)");
    let scenario = ThroughputScenario::new(server_addr)
        .with_data_size(65536)
        .with_duration(Duration::from_secs(5));
    if let Err(e) = scenario.execute().await {
        error!("Failed: {}", e);
    }

    // Latency test
    info!("\n4️⃣ Latency Measurement (200 samples)");
    let scenario = LatencyScenario::new(server_addr)
        .with_iterations(200);
    if let Err(e) = scenario.execute().await {
        error!("Failed: {}", e);
    }

    // Connection stress test
    info!("\n5️⃣ Connection Stress Test (25 connections)");
    let scenario = ConnectionLimitScenario::new(server_addr)
        .with_max_connections(25);
    if let Err(e) = scenario.execute().await {
        error!("Failed: {}", e);
    }

    info!("\n📈 Benchmark Suite Complete");
    info!("═══════════════════════════");
    info!("Review the results above for performance metrics");

    Ok(())
}