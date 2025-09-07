use anyhow::Result;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tokio::signal;

use dbgif_server::server::DbgifServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dbgif_server=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting dbgif server...");

    let mut server = DbgifServer::new();
    
    // Bind to default port (5037)
    if let Err(e) = server.bind(None).await {
        error!("Failed to bind server: {}", e);
        return Err(e);
    }

    // Set up graceful shutdown
    let shutdown_signal = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        info!("Received CTRL+C, shutting down gracefully...");
    };

    // Run server with graceful shutdown
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                error!("Server error: {}", e);
                return Err(e);
            }
        }
        _ = shutdown_signal => {
            info!("Server shutdown complete");
        }
    }

    Ok(())
}