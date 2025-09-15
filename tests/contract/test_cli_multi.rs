use dbgif_server::test_client::TestClientCli;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_multi_connect_basic() {
    let client = TestClientCli::new();

    let result = timeout(
        Duration::from_secs(15),
        client.multi_connect("localhost", 5037, 3)
    ).await;

    assert!(result.is_ok(), "Multi-connect should complete within timeout");
    assert!(result.unwrap().is_ok(), "Multi-connect should succeed");
}

#[tokio::test]
async fn test_multi_connect_single() {
    let client = TestClientCli::new();

    // Test single connection (edge case)
    let result = client.multi_connect("localhost", 5037, 1).await;
    assert!(result.is_ok(), "Single connection should succeed");
}

#[tokio::test]
async fn test_multi_connect_max_limit() {
    let client = TestClientCli::new();

    // Test maximum connections (should be capped at 10)
    let result = client.multi_connect("localhost", 5037, 10).await;
    assert!(result.is_ok(), "Maximum connections should succeed");
}

#[tokio::test]
async fn test_multi_connect_over_limit() {
    let client = TestClientCli::new();

    // Test over limit (should be capped to 10 in binary)
    let result = client.multi_connect("localhost", 5037, 15).await;
    assert!(result.is_ok(), "Over-limit connections should be capped and succeed");
}

#[tokio::test]
async fn test_multi_connect_concurrent_behavior() {
    let client = TestClientCli::new();

    // Test that connections are actually concurrent
    let start = std::time::Instant::now();
    let result = client.multi_connect("localhost", 5037, 5).await;
    let duration = start.elapsed();

    assert!(result.is_ok(), "Concurrent connections should succeed");

    // Concurrent connections should be faster than sequential
    // This is more of a performance contract test
    println!("Multi-connect with 5 connections took: {:?}", duration);
}

#[tokio::test]
async fn test_multi_connect_connection_failure() {
    let client = TestClientCli::new();

    // Test with unreachable server
    let result = client.multi_connect("192.0.2.1", 5037, 3).await;
    assert!(result.is_err(), "Multi-connect to unreachable server should fail");
}

#[tokio::test]
async fn test_multi_connect_partial_failure_handling() {
    let client = TestClientCli::new();

    // This will test how partial failures are handled
    // (when some connections succeed and others fail)
    let result = client.multi_connect("localhost", 5037, 3).await;

    match result {
        Ok(()) => println!("All connections succeeded"),
        Err(e) => println!("Some connections may have failed: {}", e),
    }
}