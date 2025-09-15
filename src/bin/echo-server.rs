use dbgif_server::transport::EchoTransportServer;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .init();

    info!("Starting Echo Transport Server on port 5038");

    let mut server = EchoTransportServer::default();

    // Start the server
    if let Err(e) = server.start().await {
        error!("Failed to start echo server: {}", e);
        return Err(e.into());
    }

    info!("Echo server started successfully");

    // Setup graceful shutdown
    let shutdown_signal = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        info!("Shutdown signal received");
    };

    // Run server with graceful shutdown
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                error!("Echo server error: {}", e);
                return Err(e.into());
            }
        }
        _ = shutdown_signal => {
            info!("Shutting down echo server gracefully");
            server.stop().await;
        }
    }

    info!("Echo server stopped");
    Ok(())
}