/// Basic usage example for dbgif-integrated-test
///
/// Run with: cargo run --example basic_usage

use anyhow::Result;
use dbgif_integrated_test::{
    client::TestClient,
    device::{DeviceConfig, EmbeddedDeviceServer},
    scenarios::{BasicConnectionScenario, Scenario},
};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    // Server address
    let server_addr: SocketAddr = "127.0.0.1:5555".parse()?;

    // Example 1: Simple client connection
    println!("=== Example 1: Simple Client Connection ===");
    simple_client_test(server_addr).await?;

    // Example 2: Device server spawn
    println!("\n=== Example 2: Device Server ===");
    device_server_test(server_addr).await?;

    // Example 3: Run a scenario
    println!("\n=== Example 3: Scenario Execution ===");
    scenario_test(server_addr).await?;

    Ok(())
}

async fn simple_client_test(server_addr: SocketAddr) -> Result<()> {
    // Create and connect client
    let mut client = TestClient::new(server_addr);

    println!("Connecting to server at {}...", server_addr);
    client.connect().await?;
    println!("Connected!");

    // Test host:version
    let version = client.ascii()?.test_host_version().await?;
    println!("Server version: {}", version);

    // Test host:list
    let devices = client.ascii()?.test_host_list().await?;
    println!("Connected devices: {} found", devices.len());
    for device in devices {
        println!("  - {}", device);
    }

    // Disconnect
    client.disconnect().await?;
    println!("Disconnected");

    Ok(())
}

async fn device_server_test(server_addr: SocketAddr) -> Result<()> {
    // Configure device
    let device_config = DeviceConfig {
        device_id: "example-device-001".to_string(),
        port: None, // Auto-allocate
        model: Some("ExampleDevice".to_string()),
        capabilities: Some(vec!["shell".to_string(), "file".to_string()]),
    };

    // Spawn device server
    println!("Starting device server...");
    let mut device = EmbeddedDeviceServer::spawn(device_config).await?;
    println!("Device server started on port {}", device.port());

    // Wait a bit
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Connect client to server
    let mut client = TestClient::new(server_addr);
    client.connect().await?;

    // Connect to device via host:connect
    println!("Connecting to device...");
    client.connect_device("127.0.0.1", device.port()).await?;
    println!("Device connected!");

    // Verify device appears in list
    let devices = client.ascii()?.test_host_list().await?;
    println!("Devices after connection: {}", devices.len());

    // Cleanup
    client.disconnect().await?;
    device.stop().await?;
    println!("Device server stopped");

    Ok(())
}

async fn scenario_test(server_addr: SocketAddr) -> Result<()> {
    // Create a scenario
    let scenario = BasicConnectionScenario::new(server_addr);

    // Execute scenario
    println!("Running {} scenario...", scenario.name());
    match scenario.execute().await {
        Ok(_) => println!("✅ Scenario completed successfully"),
        Err(e) => println!("❌ Scenario failed: {}", e),
    }

    Ok(())
}