use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use dbgif_server::protocol::{AdbMessage, Command, checksum};
use dbgif_server::transport::*;
use std::time::Duration;
use tokio::runtime::Runtime;

fn bench_protocol_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol");

    // Benchmark message creation
    group.bench_function("message_creation_empty", |b| {
        b.iter(|| {
            let message = AdbMessage::new(
                Command::CNXN,
                black_box(0x01000000),
                black_box(256 * 1024),
                bytes::Bytes::new(),
            );
            black_box(message);
        });
    });

    group.bench_function("message_creation_with_data", |b| {
        let test_data = bytes::Bytes::from(vec![0u8; 1024]); // 1KB test data
        b.iter(|| {
            let message = AdbMessage::new(
                Command::WRTE,
                black_box(1),
                black_box(2),
                black_box(test_data.clone()),
            );
            black_box(message);
        });
    });

    // Benchmark different data sizes
    for size in [64, 256, 1024, 4096, 16384, 65536].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("message_with_data_size", size),
            size,
            |b, &size| {
                let test_data = bytes::Bytes::from(vec![0u8; size]);
                b.iter(|| {
                    let message = AdbMessage::new(
                        Command::WRTE,
                        black_box(1),
                        black_box(2),
                        black_box(test_data.clone()),
                    );
                    black_box(message);
                });
            },
        );
    }

    group.finish();
}

fn bench_checksum_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("checksum");

    // Test different data sizes for checksum calculation
    for size in [64, 256, 1024, 4096, 16384, 65536].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("crc32_calculation", size),
            size,
            |b, &size| {
                let test_data = vec![0x42u8; size]; // Pattern data
                b.iter(|| {
                    let checksum = checksum::calculate_crc32(black_box(&test_data));
                    black_box(checksum);
                });
            },
        );
    }

    // Benchmark incremental checksum updates
    group.bench_function("crc32_incremental_1kb", |b| {
        let chunks: Vec<Vec<u8>> = (0..16).map(|_| vec![0x42u8; 64]).collect(); // 16 x 64 bytes = 1KB total
        b.iter(|| {
            let mut checksum = 0u32;
            for chunk in &chunks {
                checksum = checksum::update_crc32(black_box(checksum), black_box(chunk));
            }
            black_box(checksum);
        });
    });

    group.finish();
}

fn bench_message_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization");

    // Create test messages of different sizes
    let small_message = AdbMessage::new(
        Command::OKAY,
        1,
        2,
        bytes::Bytes::from(vec![0u8; 64]),
    );

    let medium_message = AdbMessage::new(
        Command::WRTE,
        1,
        2,
        bytes::Bytes::from(vec![0u8; 4096]),
    );

    let large_message = AdbMessage::new(
        Command::WRTE,
        1,
        2,
        bytes::Bytes::from(vec![0u8; 65536]),
    );

    group.bench_function("serialize_small_message", |b| {
        b.iter(|| {
            let serialized = small_message.serialize();
            black_box(serialized);
        });
    });

    group.bench_function("serialize_medium_message", |b| {
        b.iter(|| {
            let serialized = medium_message.serialize();
            black_box(serialized);
        });
    });

    group.bench_function("serialize_large_message", |b| {
        b.iter(|| {
            let serialized = large_message.serialize();
            black_box(serialized);
        });
    });

    // Benchmark deserialization
    let test_header = [
        0x4e, 0x58, 0x4e, 0x43, // CNXN command
        0xbc, 0xa7, 0xb1, 0xa3, // magic (NOT of CNXN)
        0x00, 0x00, 0x00, 0x01, // arg0
        0x00, 0x00, 0x04, 0x00, // arg1 (1024)
        0x10, 0x00, 0x00, 0x00, // data_length (16)
        0x00, 0x00, 0x00, 0x00, // data_checksum
    ];
    let test_payload = bytes::Bytes::from("test payload data");

    group.bench_function("deserialize_message", |b| {
        b.iter(|| {
            let result = AdbMessage::deserialize(
                black_box(&test_header),
                black_box(test_payload.clone()),
            );
            black_box(result);
        });
    });

    group.finish();
}

fn bench_transport_mock_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("transport_mock");
    let rt = Runtime::new().unwrap();

    // Create a mock transport for benchmarking
    let mock_transport = MockTransport::new("bench_device".to_string());

    group.bench_function("mock_transport_creation", |b| {
        b.iter(|| {
            let transport = MockTransport::new(black_box("bench_device".to_string()));
            black_box(transport);
        });
    });

    // Benchmark connection operations
    group.bench_function("mock_transport_connect", |b| {
        b.to_async(&rt).iter(|| async {
            let mut transport = MockTransport::new("bench_device".to_string());
            let result = transport.connect().await;
            black_box(result);
        });
    });

    // Benchmark send operations with different data sizes
    for size in [64, 256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("mock_transport_send", size),
            size,
            |b, &size| {
                let test_data = vec![0x42u8; size];
                b.to_async(&rt).iter(|| async {
                    let mut transport = MockTransport::new("bench_device".to_string());
                    transport.connect().await.unwrap();
                    let result = transport.send(black_box(&test_data)).await;
                    black_box(result);
                });
            },
        );
    }

    // Benchmark receive operations
    for size in [64, 256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("mock_transport_receive", size),
            size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    let mut transport = MockTransport::new("bench_device".to_string());
                    transport.connect().await.unwrap();
                    let mut buffer = vec![0u8; *size];
                    let result = transport.receive(black_box(&mut buffer)).await;
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

fn bench_discovery_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("discovery");
    let rt = Runtime::new().unwrap();

    // Benchmark transport factory operations
    group.bench_function("tcp_transport_factory_creation", |b| {
        b.iter(|| {
            let factory = TcpTransportFactory::new();
            black_box(factory);
        });
    });

    group.bench_function("usb_device_factory_creation", |b| {
        b.iter(|| {
            let factory = UsbDeviceTransportFactory::new();
            black_box(factory);
        });
    });

    group.bench_function("usb_bridge_factory_creation", |b| {
        b.iter(|| {
            let factory = UsbBridgeTransportFactory::new();
            black_box(factory);
        });
    });

    // Benchmark device discovery
    group.bench_function("mock_device_discovery", |b| {
        let factory = TcpTransportFactory::new();
        b.to_async(&rt).iter(|| async {
            let devices = factory.discover_devices().await;
            black_box(devices);
        });
    });

    group.finish();
}

fn bench_memory_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory");

    // Benchmark buffer allocations
    for size in [1024, 4096, 16384, 65536, 262144].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("buffer_allocation", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let buffer = vec![0u8; size];
                    black_box(buffer);
                });
            },
        );
    }

    // Benchmark zero-copy operations
    group.bench_function("bytes_from_vec", |b| {
        let test_data = vec![0x42u8; 1024];
        b.iter(|| {
            let bytes = bytes::Bytes::from(black_box(test_data.clone()));
            black_box(bytes);
        });
    });

    group.bench_function("bytes_slice_operations", |b| {
        let bytes = bytes::Bytes::from(vec![0x42u8; 1024]);
        b.iter(|| {
            let slice = bytes.slice(black_box(10)..black_box(100));
            black_box(slice);
        });
    });

    // Benchmark common string operations
    group.bench_function("device_id_formatting", |b| {
        b.iter(|| {
            let device_id = format!("device_{:06}", black_box(12345));
            black_box(device_id);
        });
    });

    group.bench_function("session_id_parsing", |b| {
        let session_id = "client_abc123:device_def456";
        b.iter(|| {
            let parts: Vec<&str> = black_box(session_id).split(':').collect();
            black_box(parts);
        });
    });

    group.finish();
}

fn bench_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent");
    let rt = Runtime::new().unwrap();

    // Benchmark concurrent message processing
    group.bench_function("concurrent_message_creation", |b| {
        b.to_async(&rt).iter(|| async {
            let tasks: Vec<_> = (0..10)
                .map(|i| {
                    let data = bytes::Bytes::from(vec![i as u8; 1024]);
                    tokio::spawn(async move {
                        AdbMessage::new(Command::WRTE, i as u32, i as u32 + 1, data)
                    })
                })
                .collect();

            let results: Vec<_> = futures::future::join_all(tasks).await;
            black_box(results);
        });
    });

    // Benchmark channel throughput
    group.bench_function("channel_throughput", |b| {
        b.to_async(&rt).iter(|| async {
            use tokio::sync::mpsc;

            let (tx, mut rx) = mpsc::channel::<u32>(100);

            // Producer task
            let producer = tokio::spawn(async move {
                for i in 0..100 {
                    if tx.send(i).await.is_err() {
                        break;
                    }
                }
            });

            // Consumer task
            let consumer = tokio::spawn(async move {
                let mut count = 0;
                while rx.recv().await.is_some() {
                    count += 1;
                }
                count
            });

            let (_, count) = tokio::join!(producer, consumer);
            black_box(count.unwrap());
        });
    });

    group.finish();
}

// Performance regression tests
fn bench_critical_path(c: &mut Criterion) {
    let mut group = c.benchmark_group("critical_path");
    group.significance_level(0.02); // More sensitive to regressions
    group.sample_size(200);

    // End-to-end message processing benchmark
    group.bench_function("e2e_message_processing", |b| {
        b.iter(|| {
            // Simulate a complete message processing cycle
            let input_data = vec![0x42u8; 4096];

            // 1. Create message
            let message = AdbMessage::new(
                Command::WRTE,
                black_box(1),
                black_box(2),
                bytes::Bytes::from(black_box(input_data)),
            );

            // 2. Serialize message
            let serialized = message.serialize();

            // 3. Calculate checksum
            let checksum = checksum::calculate_crc32(&serialized);

            // 4. Deserialize back
            let header = &serialized[..24];
            let payload = bytes::Bytes::from(&serialized[24..]);
            let deserialized = AdbMessage::deserialize(header, payload).unwrap();

            black_box((checksum, deserialized));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_protocol_operations,
    bench_checksum_operations,
    bench_message_serialization,
    bench_transport_mock_operations,
    bench_discovery_operations,
    bench_memory_operations,
    bench_concurrent_operations,
    bench_critical_path
);
criterion_main!(benches);