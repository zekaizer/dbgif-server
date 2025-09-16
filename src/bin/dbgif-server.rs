#[allow(unused_imports)]
use clap::Parser;
#[allow(unused_imports)]
use tracing::{info, error};
#[allow(unused_imports)]
use dbgif_protocol;

#[derive(Parser)]
#[command(name = "dbgif-server")]
#[command(about = "DBGIF Protocol Server")]
struct Args {
    #[arg(long, default_value = "5555")]
    port: u16,

    #[arg(long, default_value = "5557")]
    device_port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    info!("Starting DBGIF server on port {}", args.port);
    info!("Device port: {}", args.device_port);

    // TODO: Implement server logic

    Ok(())
}