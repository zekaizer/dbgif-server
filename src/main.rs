use anyhow::Result;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use dbgif_server::server::DbgifServer;
use dbgif_server::transport::{AndroidUsbFactory, BridgeUsbFactory, UsbMonitor};

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

    // Setup USB monitoring
    let transport_manager = server.transport_manager();
    let mut usb_monitor = match UsbMonitor::new(transport_manager) {
        Ok(monitor) => Some(monitor),
        Err(e) => {
            warn!("Failed to create USB monitor: {} (USB hotplug disabled)", e);
            warn!("Server will continue without USB device auto-detection");
            None
        }
    };

    // Register USB factories if monitor was created successfully
    if let Some(ref mut monitor) = usb_monitor {
        info!("Registering USB transport factories...");
        monitor.register_factory(Arc::new(AndroidUsbFactory::new()));
        monitor.register_factory(Arc::new(BridgeUsbFactory::new()));

        // Start USB monitoring
        if let Err(e) = monitor.start_monitoring().await {
            warn!(
                "Failed to start USB monitoring: {} (continuing without hotplug)",
                e
            );
        } else {
            info!("USB hotplug monitoring enabled");

            // Scan for existing devices
            match monitor.scan_existing_devices().await {
                Ok(count) => {
                    if count > 0 {
                        info!("Found {} existing USB device(s)", count);
                    } else {
                        info!("No USB devices found during initial scan");
                    }
                }
                Err(e) => warn!("Initial USB device scan failed: {}", e),
            }
        }
    }

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
            // Signal server to shutdown
            server.shutdown();
        }
    }

    // Stop USB monitoring
    if let Some(ref mut monitor) = usb_monitor {
        if let Err(e) = monitor.stop_monitoring().await {
            warn!("Error stopping USB monitor: {}", e);
        } else {
            info!("USB monitoring stopped");
        }
    }

    info!("Server shutdown complete");

    Ok(())
}
