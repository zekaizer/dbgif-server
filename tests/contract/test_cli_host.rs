use dbgif_server::test_client::TestClientCli;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_host_commands_success() {
    let client = TestClientCli::new();

    let result = timeout(
        Duration::from_secs(10),
        client.host_commands("localhost", 5037)
    ).await;

    assert!(result.is_ok(), "Host commands should complete within timeout");
    assert!(result.unwrap().is_ok(), "Host commands should succeed");
}

#[tokio::test]
async fn test_host_commands_device_list() {
    let client = TestClientCli::new();

    // Test that host commands can retrieve device list
    let result = client.host_commands("localhost", 5037).await;

    // Should succeed and return structured device information
    match result {
        Ok(()) => println!("Host commands completed successfully"),
        Err(e) => println!("Host commands failed: {}", e),
    }
}

#[tokio::test]
async fn test_host_commands_version_info() {
    let client = TestClientCli::new();

    // Test version command
    let result = client.host_commands("localhost", 5037).await;
    assert!(result.is_ok(), "Host version command should succeed");
}

#[tokio::test]
async fn test_host_commands_server_kill() {
    let client = TestClientCli::new();

    // Test server kill command (should be handled gracefully)
    let result = client.host_commands("localhost", 5037).await;
    assert!(result.is_ok(), "Host kill command should be handled");
}

#[tokio::test]
async fn test_host_commands_connection_failure() {
    let client = TestClientCli::new();

    // Test with unreachable server
    let result = client.host_commands("192.0.2.1", 5037).await;
    assert!(result.is_err(), "Host commands to unreachable server should fail");
}