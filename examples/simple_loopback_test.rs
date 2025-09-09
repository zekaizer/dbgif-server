use anyhow::Result;
use dbgif_server::protocol::message::{Command, Message};
use dbgif_server::transport::manager::TransportManager;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    println!("Creating transport manager and loopback pair...");
    let manager = TransportManager::new();
    
    // Create loopback pair
    let (device_a, device_b) = manager
        .add_loopback_pair(Some("test_a".to_string()), Some("test_b".to_string()))
        .await?;
    
    println!("Created loopback pair: {} <-> {}", device_a, device_b);
    
    // Create test message
    let test_data = b"Hello from A to B!".to_vec();
    let test_msg = Message::new(Command::Write, 1, 2, test_data.clone());
    
    println!("Sending message from {} to transport layer...", device_a);
    manager.send_message(&device_a, &test_msg).await?;
    
    // Small delay to ensure message is processed
    sleep(Duration::from_millis(10)).await;
    
    println!("Receiving message from {}...", device_b);
    match manager.receive_message(&device_b).await {
        Ok(received_msg) => {
            println!("✓ Received message successfully!");
            println!("  Command: {:?}", received_msg.command);
            println!("  Data length: {}", received_msg.data.len());
            println!("  Data: {:?}", String::from_utf8_lossy(&received_msg.data));
            
            if received_msg.data == test_data {
                println!("✓ Data integrity verified: A -> B communication works!");
            } else {
                println!("✗ Data mismatch: expected {:?}, got {:?}", 
                        String::from_utf8_lossy(&test_data),
                        String::from_utf8_lossy(&received_msg.data));
            }
        }
        Err(e) => {
            println!("✗ Failed to receive message: {}", e);
        }
    }
    
    // Test reverse direction (B -> A)
    println!("\nTesting reverse direction (B -> A)...");
    let test_data_reverse = b"Hello from B to A!".to_vec();
    let test_msg_reverse = Message::new(Command::Write, 2, 1, test_data_reverse.clone());
    
    manager.send_message(&device_b, &test_msg_reverse).await?;
    sleep(Duration::from_millis(10)).await;
    
    match manager.receive_message(&device_a).await {
        Ok(received_msg) => {
            println!("✓ Received reverse message successfully!");
            println!("  Data: {:?}", String::from_utf8_lossy(&received_msg.data));
            
            if received_msg.data == test_data_reverse {
                println!("✓ Reverse data integrity verified: B -> A communication works!");
            } else {
                println!("✗ Reverse data mismatch");
            }
        }
        Err(e) => {
            println!("✗ Failed to receive reverse message: {}", e);
        }
    }
    
    println!("\n=== Loopback pair test completed ===");
    Ok(())
}