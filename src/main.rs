use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use dbgif_server::server::DbgifServer;
use dbgif_server::transport::{AndroidUsbFactory, BridgeUsbFactory, UsbMonitor};

#[derive(Parser, Debug)]
#[command(name = "dbgif-server")]
#[command(about = "DBGIF Protocol Server")]
struct Args {
    /// Enable virtual loopback transport for testing
    #[arg(long)]
    enable_loopback: bool,

    /// Server bind address
    #[arg(long, default_value = "0.0.0.0:5037")]
    bind: String,

    /// Enable verbose logging
    #[arg(long, short)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing with optional verbose mode
    let log_level = if args.verbose {
        "dbgif_server=trace,debug"
    } else {
        "dbgif_server=debug,info"
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting dbgif server with bind address: {}", args.bind);

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
        monitor.register_factory(Arc::new(AndroidUsbFactory));
        monitor.register_factory(Arc::new(BridgeUsbFactory));

        // Start USB monitoring
        if let Err(e) = monitor.start_monitoring().await {
            warn!("Failed to start USB monitoring: {} (continuing without hotplug)", e);
        } else {
            info!("USB hotplug monitoring enabled");
            
            // Scan for existing devices
            let devices = monitor.list_devices().await;
            let count = devices.len();
            if count > 0 {
                info!("Found {} existing USB device(s)", count);
            } else {
                info!("No USB devices found during initial scan");
            }
        }
    }

    // Add loopback transport if requested
    if args.enable_loopback {
        info!("Adding virtual loopback transport pair...");
        let transport_manager = server.transport_manager();
        match transport_manager.add_loopback_pair(None, None).await {
            Ok((device_a, device_b)) => {
                info!("Loopback transport pair added successfully: {} <-> {}", device_a, device_b);
            }
            Err(e) => {
                warn!("Failed to add loopback transport pair: {}", e);
            }
        }
    }

    // Parse port from bind address (for simplicity, assume format "host:port")
    let port = if let Some(port_str) = args.bind.split(':').last() {
        port_str.parse::<u16>()
            .unwrap_or_else(|_| {
                warn!("Invalid port in bind address '{}', using default port", args.bind);
                5037
            })
    } else {
        5037
    };

    // Bind to specified port
    if let Err(e) = server.bind(Some(port)).await {
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
