//! `JsonlFileWal::append` micro-benchmark.
//!
//! Measures per-record append latency and throughput against a real on-disk
//! `*.jsonl` WAL. Each benchmark iteration uses a fresh tempdir-backed WAL so
//! the cost reflects real filesystem cost, not in-memory caching.
//!
//! Three groups:
//!   * `wal_single_append` — one record per iteration, with and without
//!     informational `with_group_commit(64)` set on the WAL handle.
//!   * `wal_batch_append`  — N records per iteration (N ∈ {64, 256, 1024}),
//!     measuring sequential-batch throughput.
//!
//! Run with `cargo bench -p persistence --bench wal_append`.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use persistence::{JsonlFileWal, WalStore};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

/// Synthetic WAL record sized to roughly match a real sequencer entry
/// (~250 bytes serialised including JSON overhead).
#[derive(Clone, Serialize, Deserialize)]
struct WalRecord {
    seq: u64,
    request_id: String,
    market_id: String,
    user_id: String,
    side: String,
    order_type: String,
    price: i64,
    amount: i64,
    outcome: i32,
    timestamp_ns: u64,
    payload: String,
}

fn make_record(seq: u64) -> WalRecord {
    WalRecord {
        seq,
        request_id: format!("req-{:016x}", seq),
        market_id: "btc-usdt".to_string(),
        user_id: "test-trader-01".to_string(),
        side: "buy".to_string(),
        order_type: "limit".to_string(),
        price: 50_000,
        amount: 10,
        outcome: 0,
        timestamp_ns: 1_777_531_277_000_000_000 + seq,
        payload: "x".repeat(64),
    }
}

fn bench_single_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("wal_single_append");
    group.throughput(Throughput::Elements(1));

    group.bench_function("group_commit_off", |b| {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("wal.jsonl");
        let wal = JsonlFileWal::<WalRecord>::with_rotation(&path, 0).expect("wal init");
        let mut seq: u64 = 0;
        b.iter(|| {
            seq += 1;
            wal.append(black_box(&make_record(seq))).expect("append");
        });
        // tmp drops here -> directory and file removed.
    });

    group.bench_function("group_commit_64", |b| {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("wal.jsonl");
        let wal = JsonlFileWal::<WalRecord>::with_rotation(&path, 0)
            .expect("wal init")
            .with_group_commit(64);
        let mut seq: u64 = 0;
        b.iter(|| {
            seq += 1;
            wal.append(black_box(&make_record(seq))).expect("append");
        });
    });

    group.finish();
}

fn bench_batch_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("wal_batch_append");
    for &batch in &[64usize, 256, 1024] {
        group.throughput(Throughput::Elements(batch as u64));
        group.bench_with_input(BenchmarkId::from_parameter(batch), &batch, |b, &n| {
            let tmp = TempDir::new().expect("tempdir");
            let path = tmp.path().join("wal.jsonl");
            let wal = JsonlFileWal::<WalRecord>::with_rotation(&path, 0).expect("wal init");
            let mut seq: u64 = 0;
            b.iter(|| {
                for _ in 0..n {
                    seq += 1;
                    wal.append(black_box(&make_record(seq))).expect("append");
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_single_append, bench_batch_append);
criterion_main!(benches);
