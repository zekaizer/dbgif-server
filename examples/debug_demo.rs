/// Debug functionality demonstration
/// 
/// This example shows how to use the DebugTransport wrapper to debug
/// raw protocol messages in different transport implementations.

use dbgif_server::{
    transport::{TcpTransport, DebugTransport, TransportManager},
    protocol::{Message, Command},
    utils::hex_dump,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .init();

    println!("=== DBGIF Transport Debug Demo ===\n");

    // Demo 1: Basic hex dump functionality
    println!("1. Hex Dump Utility Demo:");
    let sample_data = b"Hello, DBGIF!\x00\x01\x02\x03\xff";
    hex_dump::hex_dump("Sample Data", sample_data, None);
    println!();

    // Demo 2: Message debug methods
    println!("2. Message Debug Methods:");
    let test_message = Message::new(
        Command::Connect,
        0x01000000,
        0x00100000,
        "host::dbgif_server\0"
    );
    
    println!("Debug format: {}", test_message.debug_format());
    println!("Compact format: {}", test_message.debug_compact());
    println!();

    // Demo 3: Raw message analysis
    println!("3. Raw Message Analysis:");
    println!("{}", test_message.debug_raw());
    println!();

    // Demo 4: Environment-based debug detection
    println!("4. Environment Debug Detection:");
    println!("DBGIF_DEBUG environment variable: {}", 
        std::env::var("DBGIF_DEBUG").unwrap_or("(not set)".to_string()));
    println!("Debug auto-enabled: {}", 
        dbgif_server::transport::is_debug_env_enabled());
    println!();

    // Demo 5: TransportManager with debug
    println!("5. TransportManager Debug Integration:");
    let manager = TransportManager::new();
    
    // This would normally create a real transport
    println!("- Manager created with debug integration support");
    println!("- Use add_transport_with_debug() or add_transport_auto_debug()");
    println!("- Use DBGIF_DEBUG=1 or DBGIF_DEBUG_TRANSPORT=1 environment variable");
    println!();

    println!("=== Demo Complete ===");
    println!();
    println!("To see debug output in a real server:");
    println!("RUST_LOG=dbgif_server::transport=debug DBGIF_DEBUG=1 cargo run");
    println!("or");
    println!("RUST_LOG=dbgif_server::transport=trace DBGIF_DEBUG=1 cargo run");

    Ok(())
}