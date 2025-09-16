#[allow(unused_imports)]
use clap::Parser;
#[allow(unused_imports)]
use tracing::{info, error};
#[allow(unused_imports)]
use dbgif_protocol;

#[derive(Parser)]
#[command(name = "dbgif-test-client")]
#[command(about = "DBGIF Protocol Test Client")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:5555")]
    server: String,

    #[arg(long, default_value = "basic")]
    test: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    info!("Connecting to DBGIF server: {}", args.server);
    info!("Running test: {}", args.test);

    // TODO: Implement test client logic

    Ok(())
}