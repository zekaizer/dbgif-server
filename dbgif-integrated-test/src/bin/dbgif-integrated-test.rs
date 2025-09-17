use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{info, error};

use dbgif_integrated_test::{
    device::{DeviceConfig, EmbeddedDeviceServer},
    scenarios::{
        Scenario,
        ScenarioManager,
        BasicConnectionScenario,
        HostServicesScenario,
        MultiDeviceScenario,
        ThroughputScenario,
        LatencyScenario,
        ConnectionLimitScenario,
    },
};

#[derive(Parser)]
#[command(name = "dbgif-integrated-test")]
#[command(version = "0.1.0")]
#[command(about = "DBGIF Integrated Test Tool", long_about = None)]
#[command(author = "DBGIF Team")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// DBGIF server address
    #[arg(long, default_value = "127.0.0.1:5555")]
    server: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Connection timeout in seconds
    #[arg(long, default_value = "10")]
    timeout: u64,
}

#[derive(Subcommand)]
enum Commands {
    /// Run everything automatically
    AllInOne {
        /// Number of devices to spawn
        #[arg(long, default_value = "1")]
        devices: usize,

        /// Specific scenario to run
        #[arg(long)]
        scenario: Option<String>,

        /// Keep servers running after test
        #[arg(long)]
        keep_alive: bool,
    },

    /// Start only device server(s)
    DeviceServer {
        /// Number of device servers to start
        #[arg(long, default_value = "1")]
        count: usize,

        /// Specific ports to use (auto-allocate if not specified)
        #[arg(long)]
        ports: Vec<u16>,

        /// Device ID prefix
        #[arg(long, default_value = "test-device")]
        device_prefix: String,

        /// Keep running until interrupted
        #[arg(long)]
        daemon: bool,
    },

    /// Run specific test scenario
    Scenario {
        /// Scenario name (basic_connection, host_services, multi_device)
        name: String,

        /// Configuration file
        #[arg(long)]
        config: Option<PathBuf>,

        /// Number of devices for multi_device scenario
        #[arg(long, default_value = "3")]
        device_count: usize,
    },

    /// List available scenarios
    List,

    /// Interactive test mode
    Interactive,

    /// Run quick test suite
    Quick {
        /// Skip slow tests
        #[arg(long)]
        skip_slow: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    setup_logging(&cli);

    // Parse server address
    let server_addr: SocketAddr = cli.server.parse()
        .map_err(|e| anyhow::anyhow!("Invalid server address '{}': {}", cli.server, e))?;

    // Execute command
    match cli.command {
        Commands::AllInOne { devices, scenario, keep_alive } => {
            run_all_in_one(server_addr, devices, scenario, keep_alive, cli.timeout).await?;
        }
        Commands::DeviceServer { count, ports, device_prefix, daemon } => {
            run_device_servers(count, ports, device_prefix, daemon).await?;
        }
        Commands::Scenario { name, config, device_count } => {
            run_scenario(server_addr, &name, config, device_count, cli.timeout).await?;
        }
        Commands::List => {
            list_scenarios();
        }
        Commands::Interactive => {
            run_interactive(server_addr, cli.timeout).await?;
        }
        Commands::Quick { skip_slow } => {
            run_quick_tests(server_addr, skip_slow, cli.timeout).await?;
        }
    }

    Ok(())
}

fn setup_logging(cli: &Cli) {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let filter = if cli.debug {
        EnvFilter::new("debug")
    } else if cli.verbose {
        EnvFilter::new("info")
    } else {
        EnvFilter::new("warn")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(cli.debug)
                .with_line_number(cli.debug)
        )
        .init();
}

async fn run_all_in_one(
    server_addr: SocketAddr,
    device_count: usize,
    scenario: Option<String>,
    keep_alive: bool,
    _timeout: u64,
) -> Result<()> {
    info!("=== All-in-One Test Mode ===");
    info!("Server: {}", server_addr);
    info!("Devices: {}", device_count);

    // Start device servers
    let mut devices = Vec::new();
    for i in 0..device_count {
        let config = DeviceConfig {
            device_id: format!("auto-device-{:03}", i + 1),
            port: None,
            model: Some(format!("AutoDevice-{}", i + 1)),
            capabilities: None,
        };
        let device = EmbeddedDeviceServer::spawn(config).await?;
        info!("Started device {} on port {}", i + 1, device.port());
        devices.push(device);
    }

    // Run scenario or default test
    if let Some(scenario_name) = scenario {
        run_scenario(server_addr, &scenario_name, None, device_count, _timeout).await?
    } else {
        // Run default basic test
        let scenario = BasicConnectionScenario::new(server_addr);
        scenario.execute().await?;
    }

    if keep_alive {
        info!("Keeping servers alive. Press Ctrl+C to stop...");
        tokio::signal::ctrl_c().await?;
    }

    // Cleanup
    for mut device in devices {
        device.stop().await?;
    }

    info!("All-in-one test completed");
    Ok(())
}

async fn run_device_servers(
    count: usize,
    ports: Vec<u16>,
    device_prefix: String,
    daemon: bool,
) -> Result<()> {
    info!("Starting {} device server(s)", count);

    let mut devices = Vec::new();

    for i in 0..count {
        let port = if i < ports.len() {
            Some(ports[i])
        } else {
            None
        };

        let config = DeviceConfig {
            device_id: format!("{}-{:03}", device_prefix, i + 1),
            port,
            model: Some("StandaloneDevice".to_string()),
            capabilities: None,
        };

        let device = EmbeddedDeviceServer::spawn(config).await?;
        info!("Device server {} started on port {}", i + 1, device.port());
        devices.push(device);
    }

    if daemon {
        info!("Running in daemon mode. Press Ctrl+C to stop...");
        tokio::signal::ctrl_c().await?;
    } else {
        info!("Device servers started. Press Enter to stop...");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
    }

    // Cleanup
    for mut device in devices {
        device.stop().await?;
    }

    Ok(())
}

async fn run_scenario(
    server_addr: SocketAddr,
    name: &str,
    _config: Option<PathBuf>,
    device_count: usize,
    _timeout: u64,
) -> Result<()> {
    info!("Running scenario: {}", name);

    let mut manager = ScenarioManager::new();

    // Add available scenarios
    match name {
        "basic_connection" | "basic" => {
            manager.add_scenario(Box::new(BasicConnectionScenario::new(server_addr)));
        }
        "host_services" | "host" => {
            manager.add_scenario(Box::new(HostServicesScenario::new(server_addr)));
        }
        "multi_device" | "multi" => {
            manager.add_scenario(Box::new(MultiDeviceScenario::new(server_addr, device_count)));
        }
        "throughput" => {
            manager.add_scenario(Box::new(ThroughputScenario::new(server_addr)));
        }
        "latency" => {
            manager.add_scenario(Box::new(LatencyScenario::new(server_addr)));
        }
        "connection_limit" | "connections" => {
            manager.add_scenario(Box::new(ConnectionLimitScenario::new(server_addr)));
        }
        _ => {
            error!("Unknown scenario: {}", name);
            error!("Available scenarios: basic_connection, host_services, multi_device, throughput, latency, connection_limit");
            return Err(anyhow::anyhow!("Unknown scenario: {}", name));
        }
    }

    // Run the scenario
    manager.run_scenario(name).await
}

fn list_scenarios() {
    println!("Available scenarios:");
    println!("  - basic_connection (basic): Test basic device connection");
    println!("  - host_services (host): Test host service commands");
    println!("  - multi_device (multi): Test multiple device connections");
    println!("  - throughput: Test data throughput performance");
    println!("  - latency: Test round-trip latency");
    println!("  - connection_limit (connections): Test maximum concurrent connections");
}

async fn run_interactive(server_addr: SocketAddr, _timeout: u64) -> Result<()> {
    info!("=== Interactive Test Mode ===");
    info!("Server: {}", server_addr);

    // TODO: Implement interactive command loop
    println!("Interactive mode not yet implemented");

    Ok(())
}

async fn run_quick_tests(server_addr: SocketAddr, skip_slow: bool, _timeout: u64) -> Result<()> {
    info!("=== Quick Test Suite ===");

    let mut manager = ScenarioManager::new();

    // Add quick scenarios
    manager.add_scenario(Box::new(BasicConnectionScenario::new(server_addr)));
    manager.add_scenario(Box::new(HostServicesScenario::new(server_addr)));

    if !skip_slow {
        manager.add_scenario(Box::new(MultiDeviceScenario::new(server_addr, 2)));
    }

    // Run all scenarios
    for scenario_name in manager.list_scenarios() {
        info!("Running: {}", scenario_name);
        match manager.run_scenario(&scenario_name).await {
            Ok(_) => info!("✅ {} passed", scenario_name),
            Err(e) => {
                error!("❌ {} failed: {}", scenario_name, e);
                return Err(e);
            }
        }
    }

    info!("Quick test suite completed successfully");
    Ok(())
}