use eventbus::EventBus;
use instruments::{InMemoryInstrumentRegistry, InstrumentRegistry};
use ledger::LedgerService;
use matching::{PartitionedEngineConfig, PartitionedMatchingEngine};
use risk::RiskEngine;
use std::sync::Arc;
use std::time::Instant;
use types::{
    CommandMetadata, InstrumentKind, InstrumentSpec, MarginMode, NewOrderCommand, OrderType, Side,
    TimeInForce,
};

const DEFAULT_TOTAL_SAMPLES: usize = 25_000;
const DEFAULT_CONCURRENCY_LEVELS: [usize; 4] = [1, 4, 8, 16];

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    let total_samples = std::env::var("SCALE_BENCH_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_TOTAL_SAMPLES);
    let concurrency_levels = std::env::var("SCALE_BENCH_LEVELS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|item| item.trim().parse::<usize>().ok())
                .collect::<Vec<_>>()
        })
        .filter(|levels| !levels.is_empty())
        .unwrap_or_else(|| DEFAULT_CONCURRENCY_LEVELS.to_vec());

    for concurrency in concurrency_levels {
        let passive = run_parallel_passive(concurrency, total_samples).await;
        println!(
            "mode=passive concurrency={} total_samples={} p50_us={} p75_us={} p99_us={} p999_us={} avg_us={:.2} max_us={} throughput_ops_per_sec={:.2}",
            concurrency,
            passive.samples,
            passive.p50_us,
            passive.p75_us,
            passive.p99_us,
            passive.p999_us,
            passive.avg_us,
            passive.max_us,
            passive.throughput_ops_per_sec
        );

        let taker = run_parallel_taker(concurrency, total_samples).await;
        println!(
            "mode=taker concurrency={} total_samples={} p50_us={} p75_us={} p99_us={} p999_us={} avg_us={:.2} max_us={} throughput_ops_per_sec={:.2}",
            concurrency,
            taker.samples,
            taker.p50_us,
            taker.p75_us,
            taker.p99_us,
            taker.p999_us,
            taker.avg_us,
            taker.max_us,
            taker.throughput_ops_per_sec
        );

        let cancel = run_parallel_cancel(concurrency, total_samples).await;
        println!(
            "mode=cancel concurrency={} total_samples={} p50_us={} p75_us={} p99_us={} p999_us={} avg_us={:.2} max_us={} throughput_ops_per_sec={:.2}",
            concurrency,
            cancel.samples,
            cancel.p50_us,
            cancel.p75_us,
            cancel.p99_us,
            cancel.p999_us,
            cancel.avg_us,
            cancel.max_us,
            cancel.throughput_ops_per_sec
        );
    }
}

#[derive(Debug)]
struct Summary {
    samples: usize,
    p50_us: u128,
    p75_us: u128,
    p99_us: u128,
    p999_us: u128,
    avg_us: f64,
    max_us: u128,
    throughput_ops_per_sec: f64,
}

async fn run_parallel_passive(concurrency: usize, total_samples: usize) -> Summary {
    let risk = seeded_risk(concurrency);
    let engine = Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(total_samples + 4096),
        EventBus::new(),
        risk,
        benchmark_registry(),
    ));

    run_parallel(concurrency, total_samples, move |worker_id, index| {
        let engine = engine.clone();
        async move {
            let start = Instant::now();
            engine
                .submit_new_order(limit_order(
                    &format!("passive-{worker_id}-{index}"),
                    &format!("passive-{worker_id}-{index}"),
                    &format!("passive-user-{worker_id}"),
                    Side::Buy,
                    90,
                    1,
                ))
                .await
                .unwrap();
            start.elapsed().as_micros()
        }
    })
    .await
}

async fn run_parallel_taker(concurrency: usize, total_samples: usize) -> Summary {
    let risk = seeded_risk(concurrency + 1);
    let engine = Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(total_samples + 4096),
        EventBus::new(),
        risk,
        benchmark_registry(),
    ));

    for index in 0..total_samples {
        engine
            .submit_new_order(limit_order(
                &format!("ask-{index}"),
                &format!("ask-{index}"),
                "maker-ask-user",
                Side::Sell,
                100,
                1,
            ))
            .await
            .unwrap();
    }

    run_parallel(concurrency, total_samples, move |worker_id, index| {
        let engine = engine.clone();
        async move {
            let start = Instant::now();
            engine
                .submit_new_order(limit_order(
                    &format!("taker-{worker_id}-{index}"),
                    &format!("taker-{worker_id}-{index}"),
                    &format!("taker-user-{worker_id}"),
                    Side::Buy,
                    100,
                    1,
                ))
                .await
                .unwrap();
            start.elapsed().as_micros()
        }
    })
    .await
}

async fn run_parallel_cancel(concurrency: usize, total_samples: usize) -> Summary {
    let risk = seeded_risk(concurrency + 1);
    let engine = Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(total_samples + 4096),
        EventBus::new(),
        risk,
        benchmark_registry(),
    ));

    let samples_per_worker = total_samples / concurrency;
    for worker_id in 0..concurrency {
        for index in 0..samples_per_worker {
            engine
                .submit_new_order(limit_order(
                    &format!("cancel-seed-{worker_id}-{index}"),
                    &format!("cancel-seed-{worker_id}-{index}"),
                    &format!("cancel-user-{worker_id}"),
                    Side::Buy,
                    95,
                    1,
                ))
                .await
                .unwrap();
        }
    }

    let workload_start = Instant::now();
    let mut handles = Vec::with_capacity(concurrency);
    for worker_id in 0..concurrency {
        let engine = engine.clone();
        handles.push(tokio::spawn(async move {
            let mut samples = Vec::with_capacity(samples_per_worker);
            for index in 0..samples_per_worker {
                let start = Instant::now();
                engine
                    .cancel_order(types::CancelOrderCommand {
                        metadata: CommandMetadata::new(format!("cancel-{worker_id}-{index}")),
                        user_id: format!("cancel-user-{worker_id}"),
                        market_id: "btc-usdt".to_string(),
                        outcome: Some(0),
                        order_id: format!("cancel-seed-{worker_id}-{index}"),
                        client_order_id: None,
                    })
                    .await
                    .unwrap();
                samples.push(start.elapsed().as_micros());
            }
            samples
        }));
    }

    let mut all_samples = Vec::with_capacity(total_samples);
    for handle in handles {
        all_samples.extend(handle.await.unwrap());
    }
    summarize(all_samples, workload_start.elapsed().as_secs_f64())
}

async fn run_parallel<F, Fut>(concurrency: usize, total_samples: usize, task_builder: F) -> Summary
where
    F: Fn(usize, usize) -> Fut + Send + Sync + 'static + Clone,
    Fut: std::future::Future<Output = u128> + Send + 'static,
{
    let samples_per_worker = total_samples / concurrency;
    let workload_start = Instant::now();
    let mut handles = Vec::with_capacity(concurrency);

    for worker_id in 0..concurrency {
        let task_builder = task_builder.clone();
        handles.push(tokio::spawn(async move {
            let mut samples = Vec::with_capacity(samples_per_worker);
            for index in 0..samples_per_worker {
                samples.push(task_builder(worker_id, index).await);
            }
            samples
        }));
    }

    let mut all_samples = Vec::with_capacity(samples_per_worker * concurrency);
    for handle in handles {
        all_samples.extend(handle.await.unwrap());
    }

    summarize(all_samples, workload_start.elapsed().as_secs_f64())
}

fn summarize(mut samples: Vec<u128>, elapsed_secs: f64) -> Summary {
    samples.sort_unstable();
    let count = samples.len();
    let total: u128 = samples.iter().sum();
    Summary {
        samples: count,
        p50_us: percentile(&samples, 0.50),
        p75_us: percentile(&samples, 0.75),
        p99_us: percentile(&samples, 0.99),
        p999_us: percentile(&samples, 0.999),
        avg_us: if count == 0 {
            0.0
        } else {
            total as f64 / count as f64
        },
        max_us: *samples.last().unwrap_or(&0),
        throughput_ops_per_sec: if elapsed_secs > 0.0 {
            count as f64 / elapsed_secs
        } else {
            0.0
        },
    }
}

fn percentile(samples: &[u128], ratio: f64) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    let index = ((samples.len() - 1) as f64 * ratio).round() as usize;
    samples[index.min(samples.len() - 1)]
}

fn bench_config(max_open_orders_per_user: usize) -> PartitionedEngineConfig {
    PartitionedEngineConfig {
        partitions: 1,
        queue_capacity: 16_384,
        snapshot_interval_commands: usize::MAX,
        max_open_orders_per_user,
        ..PartitionedEngineConfig::default()
    }
}

fn seeded_risk(extra_users: usize) -> Arc<RiskEngine> {
    let ledger = Arc::new(LedgerService::new(EventBus::new()));
    let mut users = vec!["maker-ask-user".to_string()];
    for worker_id in 0..extra_users.max(1) {
        users.push(format!("passive-user-{worker_id}"));
        users.push(format!("taker-user-{worker_id}"));
        users.push(format!("cancel-user-{worker_id}"));
    }

    for user in users {
        ledger
            .process_deposit(&user, 100_000_000, format!("deposit-{user}"))
            .unwrap();
        ledger
            .process_position_deposit(
                &user,
                "btc-usdt",
                0,
                100_000_000,
                format!("position-{user}"),
            )
            .unwrap();
    }
    Arc::new(RiskEngine::new(ledger))
}

fn benchmark_registry() -> Arc<dyn InstrumentRegistry> {
    let registry = InMemoryInstrumentRegistry::new();
    registry.register(InstrumentSpec {
        instrument_id: "btc-usdt".to_string(),
        kind: InstrumentKind::Spot,
        quote_asset: "USDC".to_string(),
        margin_mode: None,
        max_leverage: None,
        tick_size: 1,
        lot_size: 1,
        price_band_bps: 1_000,
        risk_policy_id: "spot-v1".to_string(),
    });
    registry.register(InstrumentSpec {
        instrument_id: "margin:btc-usdt".to_string(),
        kind: InstrumentKind::Margin,
        quote_asset: "USDC".to_string(),
        margin_mode: Some(MarginMode::Isolated),
        max_leverage: Some(20),
        tick_size: 1,
        lot_size: 1,
        price_band_bps: 1_000,
        risk_policy_id: "margin-v1".to_string(),
    });
    registry.register(InstrumentSpec {
        instrument_id: "perp:btc-usdt".to_string(),
        kind: InstrumentKind::Perpetual,
        quote_asset: "USDC".to_string(),
        margin_mode: Some(MarginMode::Isolated),
        max_leverage: Some(20),
        tick_size: 1,
        lot_size: 1,
        price_band_bps: 1_000,
        risk_policy_id: "perpetual-v1".to_string(),
    });
    Arc::new(registry)
}

fn limit_order(
    request_id: &str,
    client_order_id: &str,
    user_id: &str,
    side: Side,
    price: i64,
    amount: i64,
) -> NewOrderCommand {
    NewOrderCommand {
        metadata: CommandMetadata::new(request_id),
        client_order_id: client_order_id.to_string(),
        user_id: user_id.to_string(),
        session_id: None,
        market_id: "btc-usdt".to_string(),
        side,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: Some(price),
        amount,
        outcome: 0,
        post_only: false,
        reduce_only: false,
        leverage: None,
        expires_at: None,
    }
}
