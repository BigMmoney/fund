//! WAL replay scaling micro-benchmark.
//!
//! Measures `Sequencer::recover_from_wal` throughput at 1k / 10k / 100k pre-built
//! sequenced-command records on a real on-disk `*.jsonl` WAL. This is the
//! canonical "WAL replay" path used by `api::bootstrap::bootstrap_runtime`
//! immediately after `LedgerService::recover_from_wal`. Numbers from this
//! bench drive RTO budgets.
//!
//! For each size N, the WAL is built once outside the timed region (criterion's
//! `iter_custom` is used so we control timing precisely), then each iteration:
//!   1. Opens a fresh `JsonlFileWal` handle (counts lines once, cheap).
//!   2. Constructs a fresh `Sequencer` with that WAL handle.
//!   3. Calls `recover_from_wal_with_policy(AllowGaps)` and measures the
//!      JSON parse + dedup + frontier reconstruction.
//!
//! Run with `cargo bench -p sequencer --bench replay_scaling`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use persistence::{JsonlFileWal, WalStore};
use sequencer::{SequencedCommandRecord, Sequencer, SequencerRecoveryPolicy};
use tempfile::TempDir;
use types::{
    Command, CommandLifecycle, CommandMetadata, NewOrderCommand, OrderType, Side, StpMode,
    TimeInForce,
};

fn make_record(seq: u64) -> SequencedCommandRecord {
    let mut metadata = CommandMetadata::new(format!("req-{:016x}", seq));
    metadata.command_seq = Some(seq);
    metadata.lifecycle = CommandLifecycle::WalAppended;
    let command = Command::NewOrder(NewOrderCommand {
        metadata,
        client_order_id: format!("coid-{:016x}", seq),
        user_id: "test-trader-01".to_string(),
        session_id: Some("bench-session".to_string()),
        market_id: "btc-usdt".to_string(),
        side: if seq % 2 == 0 { Side::Buy } else { Side::Sell },
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: Some(50_000 + (seq as i64 % 100)),
        amount: 10,
        outcome: 0,
        post_only: false,
        reduce_only: false,
        leverage: None,
        expires_at: None,
        stp_mode: StpMode::default(),
        trigger_price: None,
        trigger_type: None,
        display_qty: None,
        min_fill_qty: None,
        stp_group_id: None,
        is_market_maker: false,
    });
    SequencedCommandRecord {
        request_id: format!("req-{:016x}", seq),
        command_seq: seq,
        command,
        recorded_at: Utc.timestamp_opt(1_777_531_277 + seq as i64, 0).unwrap(),
    }
}

fn build_wal(path: &std::path::Path, count: u64) {
    let wal = JsonlFileWal::<SequencedCommandRecord>::with_rotation(path, 0)
        .expect("wal init");
    for seq in 1..=count {
        wal.append(&make_record(seq)).expect("append");
    }
}

fn bench_replay_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("replay_scaling");
    // Larger WAL sizes need fewer samples to keep total runtime sane on dev hardware.
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(30));
    group.warm_up_time(Duration::from_secs(3));

    for &count in &[1_000u64, 10_000u64, 100_000u64] {
        let tmp = TempDir::new().expect("tempdir");
        let wal_path = tmp.path().join(format!("sequencer-{count}.wal.jsonl"));
        build_wal(&wal_path, count);

        group.throughput(Throughput::Elements(count));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &wal_path,
            |b, wal_path| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let wal: Arc<dyn WalStore<SequencedCommandRecord>> = Arc::new(
                            JsonlFileWal::<SequencedCommandRecord>::with_rotation(wal_path, 0)
                                .expect("wal open"),
                        );
                        let sequencer = Sequencer::with_wal(1, wal);
                        let start = Instant::now();
                        let recovered = sequencer
                            .recover_from_wal_with_policy(SequencerRecoveryPolicy::AllowGaps)
                            .expect("recover");
                        total += start.elapsed();
                        black_box(recovered);
                    }
                    total
                });
            },
        );

        // tmp drops here -> WAL file removed before the next size.
        drop(tmp);
    }
    group.finish();
}

criterion_group!(benches, bench_replay_scaling);
criterion_main!(benches);
