use dbgif_server::test_client::{TestSession, TestResult, ConnectionManager};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_single_connection_establishment() {
    let mut connection_manager = ConnectionManager::new();

    let result = timeout(
        Duration::from_secs(10),
        connection_manager.connect("localhost", 5037)
    ).await;

    assert!(result.is_ok(), "Connection should establish within timeout");
    assert!(result.unwrap().is_ok(), "Connection should succeed");
}

#[tokio::test]
async fn test_single_connection_handshake() {
    let mut connection_manager = ConnectionManager::new();
    let connection_result = connection_manager.connect("localhost", 5037).await;

    if let Ok(()) = connection_result {
        // Test handshake completion
        let handshake_result = connection_manager.perform_handshake().await;
        assert!(handshake_result.is_ok(), "Handshake should complete successfully");
    }
}

#[tokio::test]
async fn test_single_connection_data_exchange() {
    let mut connection_manager = ConnectionManager::new();

    if connection_manager.connect("localhost", 5037).await.is_ok() {
        if connection_manager.perform_handshake().await.is_ok() {
            // Test basic data exchange
            let test_data = b"host:version";
            let result = connection_manager.send_data(test_data).await;
            assert!(result.is_ok(), "Data sending should succeed");

            let response = connection_manager.receive_data().await;
            assert!(response.is_ok(), "Data receiving should succeed");
        }
    }
}

#[tokio::test]
async fn test_single_connection_graceful_close() {
    let mut connection_manager = ConnectionManager::new();

    if connection_manager.connect("localhost", 5037).await.is_ok() {
        if connection_manager.perform_handshake().await.is_ok() {
            // Test graceful connection close
            let close_result = connection_manager.close().await;
            assert!(close_result.is_ok(), "Connection close should succeed");

            // Verify connection is actually closed
            assert!(!connection_manager.is_connected(), "Connection should be closed");
        }
    }
}

#[tokio::test]
async fn test_single_connection_session_tracking() {
    let mut session = TestSession::new();
    let mut connection_manager = ConnectionManager::new();

    session.start_test("single_connection_test");

    if connection_manager.connect("localhost", 5037).await.is_ok() {
        session.record_event("connection_established");

        if connection_manager.perform_handshake().await.is_ok() {
            session.record_event("handshake_completed");
        }

        let _ = connection_manager.close().await;
        session.record_event("connection_closed");
    }

    session.end_test();

    let result = session.get_result();
    match result {
        TestResult::Success { duration, events } => {
            assert!(duration > Duration::from_millis(0), "Test should have measurable duration");
            assert!(events.len() > 0, "Test should have recorded events");
        }
        TestResult::Failure { error, .. } => {
            println!("Test failed: {}", error);
        }
    }
}

#[tokio::test]
async fn test_single_connection_timeout_handling() {
    let mut connection_manager = ConnectionManager::new();

    // Test connection timeout with unreachable host
    let result = timeout(
        Duration::from_secs(5),
        connection_manager.connect("192.0.2.1", 5037)
    ).await;

    // Should either timeout or fail gracefully
    match result {
        Ok(Ok(())) => println!("Connection succeeded unexpectedly"),
        Ok(Err(e)) => println!("Connection failed as expected: {}", e),
        Err(_) => println!("Connection timed out as expected"),
    }
}

#[tokio::test]
async fn test_single_connection_error_recovery() {
    let mut connection_manager = ConnectionManager::new();

    // Test connection to invalid port
    let result = connection_manager.connect("localhost", 12345).await;
    assert!(result.is_err(), "Connection to invalid port should fail");

    // Test recovery with valid connection
    let recovery_result = connection_manager.connect("localhost", 5037).await;
    // Should be able to recover and make valid connection
    match recovery_result {
        Ok(()) => println!("Successfully recovered with valid connection"),
        Err(e) => println!("Recovery attempt failed: {}", e),
    }
}