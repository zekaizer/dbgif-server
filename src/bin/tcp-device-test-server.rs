#[allow(unused_imports)]
use clap::Parser;
#[allow(unused_imports)]
use tracing::{info, error};
#[allow(unused_imports)]
use dbgif_protocol;

#[derive(Parser)]
#[command(name = "tcp-device-test-server")]
#[command(about = "TCP Device Test Server for DBGIF")]
struct Args {
    #[arg(long, default_value = "5557")]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    info!("Starting TCP device test server on port {}", args.port);

    // TODO: Implement device test server logic

    Ok(())
}