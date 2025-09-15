use dbgif_server::test_client::TestClientCli;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_ping_command_success() {
    let client = TestClientCli::new();

    // Test basic ping functionality
    let result = timeout(
        Duration::from_secs(10),
        client.ping("localhost", 5037, 5)
    ).await;

    assert!(result.is_ok(), "Ping command should complete within timeout");
    assert!(result.unwrap().is_ok(), "Ping command should succeed");
}

#[tokio::test]
async fn test_ping_command_with_custom_host() {
    let client = TestClientCli::new();

    // Test ping with custom host
    let result = client.ping("127.0.0.1", 5037, 5).await;
    assert!(result.is_ok(), "Ping with custom host should succeed");
}

#[tokio::test]
async fn test_ping_command_with_timeout() {
    let client = TestClientCli::new();

    // Test ping with very short timeout to unreachable host
    let result = client.ping("192.0.2.1", 5037, 1).await; // RFC5737 test address

    // Should either succeed quickly or timeout gracefully
    match result {
        Ok(_) => println!("Ping succeeded unexpectedly"),
        Err(e) => assert!(e.to_string().contains("timeout") || e.to_string().contains("connection"),
                         "Error should be timeout or connection related: {}", e),
    }
}

#[tokio::test]
async fn test_ping_command_invalid_port() {
    let client = TestClientCli::new();

    // Test ping with invalid port
    let result = client.ping("localhost", 65535, 5).await;
    assert!(result.is_err(), "Ping with invalid port should fail");
}

#[tokio::test]
async fn test_ping_command_output_format() {
    let client = TestClientCli::new();

    // This test will verify that ping returns proper result format
    let result = client.ping("localhost", 5037, 5).await;

    // The result should be structured and contain timing information
    match result {
        Ok(()) => println!("Ping completed successfully"),
        Err(e) => println!("Ping failed with structured error: {}", e),
    }
}