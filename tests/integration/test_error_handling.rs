use dbgif_server::test_client::{ConnectionManager, TestSession, TestResult};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_connection_refused_error() {
    let mut connection_manager = ConnectionManager::new();

    // Test connection to a port that should be refused
    let result = connection_manager.connect("localhost", 12345).await;
    assert!(result.is_err(), "Connection to unused port should fail");

    if let Err(e) = result {
        let error_msg = e.to_string().to_lowercase();
        assert!(
            error_msg.contains("refused") || error_msg.contains("connect"),
            "Error should indicate connection refusal: {}", e
        );
    }
}

#[tokio::test]
async fn test_connection_timeout_error() {
    let mut connection_manager = ConnectionManager::new();

    // Test connection timeout with unreachable host
    let result = timeout(
        Duration::from_secs(3),
        connection_manager.connect("192.0.2.1", 5037) // RFC5737 test address
    ).await;

    match result {
        Ok(Ok(())) => println!("Connection succeeded unexpectedly"),
        Ok(Err(e)) => {
            let error_msg = e.to_string().to_lowercase();
            assert!(
                error_msg.contains("timeout") || error_msg.contains("unreachable"),
                "Error should indicate timeout or unreachable: {}", e
            );
        }
        Err(_) => println!("Connection timed out as expected"),
    }
}

#[tokio::test]
async fn test_invalid_host_error() {
    let mut connection_manager = ConnectionManager::new();

    // Test connection to invalid hostname
    let result = connection_manager.connect("invalid.hostname.nonexistent", 5037).await;
    assert!(result.is_err(), "Connection to invalid hostname should fail");

    if let Err(e) = result {
        let error_msg = e.to_string().to_lowercase();
        assert!(
            error_msg.contains("resolve") || error_msg.contains("host") || error_msg.contains("dns"),
            "Error should indicate DNS/hostname resolution failure: {}", e
        );
    }
}

#[tokio::test]
async fn test_handshake_failure_handling() {
    let mut connection_manager = ConnectionManager::new();
    let mut session = TestSession::new();

    session.start_test("handshake_failure_test");

    // Try to connect first
    if let Ok(()) = connection_manager.connect("localhost", 5037).await {
        session.record_event("connection_established");

        // Test handshake failure scenarios
        let handshake_result = connection_manager.perform_handshake().await;

        match handshake_result {
            Ok(()) => {
                session.record_event("handshake_succeeded");
                println!("Handshake succeeded");
            }
            Err(e) => {
                session.record_event("handshake_failed");
                println!("Handshake failed as expected: {}", e);

                let error_msg = e.to_string().to_lowercase();
                assert!(
                    error_msg.contains("handshake") ||
                    error_msg.contains("protocol") ||
                    error_msg.contains("auth"),
                    "Error should be handshake related: {}", e
                );
            }
        }

        let _ = connection_manager.close().await;
    }

    session.end_test();
}

#[tokio::test]
async fn test_data_transmission_error() {
    let mut connection_manager = ConnectionManager::new();

    if let Ok(()) = connection_manager.connect("localhost", 5037).await {
        if let Ok(()) = connection_manager.perform_handshake().await {
            // Test sending invalid/oversized data
            let oversized_data = vec![0u8; 1024 * 1024]; // 1MB
            let result = connection_manager.send_data(&oversized_data).await;

            match result {
                Ok(()) => println!("Large data sent successfully"),
                Err(e) => {
                    println!("Large data transmission failed as expected: {}", e);
                    let error_msg = e.to_string().to_lowercase();
                    assert!(
                        error_msg.contains("size") ||
                        error_msg.contains("limit") ||
                        error_msg.contains("data"),
                        "Error should be data size related: {}", e
                    );
                }
            }

            let _ = connection_manager.close().await;
        }
    }
}

#[tokio::test]
async fn test_connection_drop_recovery() {
    let mut connection_manager = ConnectionManager::new();

    // Establish connection
    if connection_manager.connect("localhost", 5037).await.is_ok() {
        if connection_manager.perform_handshake().await.is_ok() {
            // Simulate connection drop by force closing
            let _ = connection_manager.close().await;

            // Try to send data after connection drop
            let result = connection_manager.send_data(b"test").await;
            assert!(result.is_err(), "Data sending should fail on dropped connection");

            // Test recovery
            let recovery_result = connection_manager.connect("localhost", 5037).await;
            match recovery_result {
                Ok(()) => println!("Successfully recovered from connection drop"),
                Err(e) => println!("Recovery failed: {}", e),
            }
        }
    }
}

#[tokio::test]
async fn test_concurrent_error_isolation() {
    let handles = vec![
        // Valid connection
        tokio::spawn(async {
            let mut cm = ConnectionManager::new();
            cm.connect("localhost", 5037).await.map(|_| "success")
        }),

        // Invalid port
        tokio::spawn(async {
            let mut cm = ConnectionManager::new();
            cm.connect("localhost", 12345).await.map(|_| "success")
        }),

        // Invalid host
        tokio::spawn(async {
            let mut cm = ConnectionManager::new();
            cm.connect("invalid.host", 5037).await.map(|_| "success")
        }),
    ];

    let results = futures::future::join_all(handles).await;
    let mut success_count = 0;
    let mut error_count = 0;

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(Ok(_)) => {
                success_count += 1;
                println!("Connection {} succeeded", i);
            }
            Ok(Err(e)) => {
                error_count += 1;
                println!("Connection {} failed as expected: {}", i, e);
            }
            Err(e) => {
                error_count += 1;
                println!("Connection {} panicked: {}", i, e);
            }
        }
    }

    assert!(success_count > 0, "At least one connection should succeed");
    assert!(error_count > 0, "Some connections should fail as designed");
    println!("Error isolation test: {} successes, {} errors", success_count, error_count);
}

#[tokio::test]
async fn test_resource_exhaustion_handling() {
    let mut connection_manager = ConnectionManager::new();
    let mut session = TestSession::new();

    session.start_test("resource_exhaustion_test");

    // Attempt to create many connections rapidly
    let connection_attempts = 20;
    let mut results = Vec::new();

    for i in 0..connection_attempts {
        let result = timeout(
            Duration::from_secs(2),
            connection_manager.connect("localhost", 5037)
        ).await;

        results.push((i, result));

        // Small delay to avoid overwhelming the system
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    session.record_event(&format!("attempted {} connections", connection_attempts));

    let successful_connections = results.iter()
        .filter(|(_, result)| matches!(result, Ok(Ok(()))))
        .count();

    session.record_event(&format!("{} connections succeeded", successful_connections));
    session.end_test();

    // Should handle resource limits gracefully
    println!("Resource exhaustion test: {}/{} connections succeeded",
             successful_connections, connection_attempts);
}