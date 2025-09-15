use dbgif_server::test_client::{ConnectionManager, TestSession, TestResult};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use futures::future::join_all;

#[tokio::test]
async fn test_concurrent_connections_basic() {
    let connection_count = 3;
    let mut handles = Vec::new();

    for i in 0..connection_count {
        let handle = tokio::spawn(async move {
            let mut connection_manager = ConnectionManager::new();
            let result = connection_manager.connect("localhost", 5037).await;
            (i, result)
        });
        handles.push(handle);
    }

    let results = join_all(handles).await;

    let mut success_count = 0;
    for result in results {
        if let Ok((id, connection_result)) = result {
            match connection_result {
                Ok(()) => {
                    success_count += 1;
                    println!("Connection {} succeeded", id);
                }
                Err(e) => println!("Connection {} failed: {}", id, e),
            }
        }
    }

    assert!(success_count > 0, "At least one concurrent connection should succeed");
}

#[tokio::test]
async fn test_concurrent_connections_performance() {
    let connection_count = 5;
    let start_time = Instant::now();

    let handles: Vec<_> = (0..connection_count)
        .map(|i| {
            tokio::spawn(async move {
                let mut connection_manager = ConnectionManager::new();
                let connect_start = Instant::now();

                let result = connection_manager.connect("localhost", 5037).await;
                let connect_duration = connect_start.elapsed();

                if let Ok(()) = result {
                    let _ = connection_manager.perform_handshake().await;
                    let _ = connection_manager.close().await;
                }

                (i, result, connect_duration)
            })
        })
        .collect();

    let results = join_all(handles).await;
    let total_duration = start_time.elapsed();

    println!("Total time for {} concurrent connections: {:?}", connection_count, total_duration);

    for result in results {
        if let Ok((id, connection_result, duration)) = result {
            println!("Connection {}: {:?} in {:?}", id, connection_result.is_ok(), duration);
        }
    }

    // Concurrent connections should be significantly faster than sequential
    assert!(total_duration < Duration::from_secs(connection_count as u64 * 2),
           "Concurrent connections should be faster than sequential");
}

#[tokio::test]
async fn test_concurrent_connections_max_limit() {
    let connection_count = 10; // Maximum allowed
    let mut session = TestSession::new();

    session.start_test("concurrent_max_connections");

    let handles: Vec<_> = (0..connection_count)
        .map(|i| {
            tokio::spawn(async move {
                let mut connection_manager = ConnectionManager::new();

                timeout(
                    Duration::from_secs(15),
                    connection_manager.connect("localhost", 5037)
                ).await
                .map_err(|_| format!("Connection {} timed out", i))
                .and_then(|result| result.map_err(|e| format!("Connection {} failed: {}", i, e)))
                .map(|_| i)
            })
        })
        .collect();

    let results = join_all(handles).await;
    let successful_connections: Vec<_> = results.into_iter()
        .filter_map(|r| r.ok().and_then(|inner| inner.ok()))
        .collect();

    session.record_event(&format!("{} connections established", successful_connections.len()));
    session.end_test();

    assert!(successful_connections.len() > 0, "Some connections should succeed");
    println!("Successfully established {} out of {} connections",
             successful_connections.len(), connection_count);
}

#[tokio::test]
async fn test_concurrent_connections_data_exchange() {
    let connection_count = 3;

    let handles: Vec<_> = (0..connection_count)
        .map(|i| {
            tokio::spawn(async move {
                let mut connection_manager = ConnectionManager::new();

                if let Ok(()) = connection_manager.connect("localhost", 5037).await {
                    if let Ok(()) = connection_manager.perform_handshake().await {
                        // Each connection sends different data
                        let test_data = format!("host:version-{}", i);
                        let send_result = connection_manager.send_data(test_data.as_bytes()).await;

                        if send_result.is_ok() {
                            let receive_result = connection_manager.receive_data().await;
                            let _ = connection_manager.close().await;
                            return (i, receive_result.is_ok());
                        }
                    }
                }
                (i, false)
            })
        })
        .collect();

    let results = join_all(handles).await;
    let successful_exchanges = results.iter()
        .filter_map(|r| r.as_ref().ok())
        .filter(|(_, success)| *success)
        .count();

    assert!(successful_exchanges > 0,
           "At least one concurrent connection should successfully exchange data");
    println!("Successful data exchanges: {}/{}", successful_exchanges, connection_count);
}

#[tokio::test]
async fn test_concurrent_connections_resource_cleanup() {
    let connection_count = 5;

    let handles: Vec<_> = (0..connection_count)
        .map(|i| {
            tokio::spawn(async move {
                let mut connection_manager = ConnectionManager::new();
                let mut cleanup_successful = false;

                if let Ok(()) = connection_manager.connect("localhost", 5037).await {
                    if let Ok(()) = connection_manager.perform_handshake().await {
                        // Simulate some work
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        // Test graceful cleanup
                        if connection_manager.close().await.is_ok() {
                            cleanup_successful = !connection_manager.is_connected();
                        }
                    }
                }

                (i, cleanup_successful)
            })
        })
        .collect();

    let results = join_all(handles).await;
    let successful_cleanups = results.iter()
        .filter_map(|r| r.as_ref().ok())
        .filter(|(_, cleanup_ok)| *cleanup_ok)
        .count();

    assert!(successful_cleanups > 0, "Resource cleanup should work for concurrent connections");
    println!("Successful cleanups: {}/{}", successful_cleanups, connection_count);
}

#[tokio::test]
async fn test_concurrent_connections_failure_isolation() {
    let connection_count = 4;

    let handles: Vec<_> = (0..connection_count)
        .map(|i| {
            tokio::spawn(async move {
                let mut connection_manager = ConnectionManager::new();

                // Introduce deliberate failure for connection 2
                let port = if i == 2 { 12345 } else { 5037 };
                let host = if i == 3 { "192.0.2.1" } else { "localhost" };

                let result = timeout(
                    Duration::from_secs(5),
                    connection_manager.connect(host, port)
                ).await;

                match result {
                    Ok(Ok(())) => (i, "success"),
                    Ok(Err(_)) => (i, "connection_failed"),
                    Err(_) => (i, "timeout"),
                }
            })
        })
        .collect();

    let results = join_all(handles).await;
    let mut success_count = 0;
    let mut failure_count = 0;

    for result in results {
        if let Ok((id, status)) = result {
            println!("Connection {}: {}", id, status);
            match status {
                "success" => success_count += 1,
                _ => failure_count += 1,
            }
        }
    }

    assert!(success_count > 0, "Some connections should succeed despite others failing");
    assert!(failure_count > 0, "Some connections should fail as designed");
    println!("Results: {} successes, {} failures", success_count, failure_count);
}