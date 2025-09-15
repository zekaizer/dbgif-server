use clap::{Parser, Subcommand};
use dbgif_server::test_client::TestClientCli;
use tracing::{info, Level};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(name = "dbgif-test-client")]
#[command(about = "DBGIF Protocol Test Client")]
#[command(version = "1.0")]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Output results in JSON format for automation
    #[arg(short, long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Test ping command
    Ping {
        /// Target host (default: localhost)
        #[arg(long, default_value = "localhost")]
        host: String,
        /// Target port (default: 5037)
        #[arg(short, long, default_value_t = 5037)]
        port: u16,
        /// Timeout in seconds
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
    /// Test host commands
    HostCommands {
        /// Target host (default: localhost)
        #[arg(long, default_value = "localhost")]
        host: String,
        /// Target port (default: 5037)
        #[arg(short, long, default_value_t = 5037)]
        port: u16,
    },
    /// Test multiple connections
    MultiConnect {
        /// Target host (default: localhost)
        #[arg(long, default_value = "localhost")]
        host: String,
        /// Target port (default: 5037)
        #[arg(short, long, default_value_t = 5037)]
        port: u16,
        /// Number of connections (max: 10)
        #[arg(short, long, default_value_t = 3)]
        count: u8,
    },
    /// Run aging test for long-duration stability testing
    Aging {
        /// Target host (default: localhost for echo service)
        #[arg(long, default_value = "localhost")]
        host: String,
        /// Target port (default: 5038 for echo service)
        #[arg(short, long, default_value_t = 5038)]
        port: u16,
        /// Test duration in seconds (max: 3600)
        #[arg(short, long, default_value_t = 60)]
        duration: u64,
        /// Packet size in bytes (1-65536)
        #[arg(long, default_value_t = 1024)]
        packet_size: usize,
        /// Interval between packets in milliseconds (min: 10)
        #[arg(short, long, default_value_t = 100)]
        interval: u64,
        /// Number of concurrent connections (1-10)
        #[arg(short, long, default_value_t = 1)]
        connections: u8,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize structured logging
    let log_level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(log_level.into())
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .init();

    info!("DBGIF Test Client starting with verbose={}, json={}", cli.verbose, cli.json);

    let test_client = TestClientCli::new()
        .verbose(cli.verbose)
        .json_output(cli.json);

    let result = match cli.command {
        Commands::Ping { host, port, timeout } => {
            test_client.ping(&host, port, timeout).await
        }
        Commands::HostCommands { host, port } => {
            test_client.host_commands(&host, port).await
        }
        Commands::MultiConnect { host, port, count } => {
            let count = count.min(10); // Enforce max 10 connections
            test_client.multi_connect(&host, port, count).await
        }
        Commands::Aging { host, port, duration, packet_size, interval, connections } => {
            test_client.aging(&host, port, duration, packet_size, interval, connections).await
        }
    };

    match result {
        Ok(()) => Ok(()),
        Err(e) => Err(e.into()),
    }
}