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

const WARMUP_ORDERS: usize = 200;
const MEASURE_ORDERS: usize = 5000;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let passive = run_passive_insert_workload().await;
    let taker = run_taker_match_workload().await;

    println!(
        "workload=passive_limit_insert samples={} p50_us={} p75_us={} p99_us={} avg_us={:.2} max_us={} throughput_ops_per_sec={:.2}",
        passive.samples,
        passive.p50_us,
        passive.p75_us,
        passive.p99_us,
        passive.avg_us,
        passive.max_us,
        passive.throughput_ops_per_sec,
    );
    println!(
        "workload=taker_limit_match samples={} p50_us={} p75_us={} p99_us={} avg_us={:.2} max_us={} throughput_ops_per_sec={:.2}",
        taker.samples,
        taker.p50_us,
        taker.p75_us,
        taker.p99_us,
        taker.avg_us,
        taker.max_us,
        taker.throughput_ops_per_sec,
    );
}

#[derive(Debug)]
struct LatencySummary {
    samples: usize,
    p50_us: u128,
    p75_us: u128,
    p99_us: u128,
    avg_us: f64,
    max_us: u128,
    throughput_ops_per_sec: f64,
}

async fn run_passive_insert_workload() -> LatencySummary {
    let risk = seeded_risk();
    let engine = PartitionedMatchingEngine::new_with_registry(
        bench_config(),
        EventBus::new(),
        risk,
        benchmark_registry(),
    );

    for index in 0..WARMUP_ORDERS {
        engine
            .submit_new_order(limit_order(
                &format!("warmup-passive-{index}"),
                &format!("warmup-passive-{index}"),
                "passive-maker",
                Side::Buy,
                90,
                1,
            ))
            .await
            .unwrap();
    }

    let mut samples = Vec::with_capacity(MEASURE_ORDERS);
    let workload_start = Instant::now();
    for index in 0..MEASURE_ORDERS {
        let start = Instant::now();
        engine
            .submit_new_order(limit_order(
                &format!("passive-{index}"),
                &format!("passive-{index}"),
                "passive-maker",
                Side::Buy,
                90,
                1,
            ))
            .await
            .unwrap();
        samples.push(start.elapsed().as_micros());
    }

    summarize(samples, workload_start.elapsed().as_secs_f64())
}

async fn run_taker_match_workload() -> LatencySummary {
    let risk = seeded_risk();
    let engine = PartitionedMatchingEngine::new_with_registry(
        bench_config(),
        EventBus::new(),
        risk,
        benchmark_registry(),
    );

    let total_orders = WARMUP_ORDERS + MEASURE_ORDERS;
    for index in 0..total_orders {
        engine
            .submit_new_order(limit_order(
                &format!("maker-ask-{index}"),
                &format!("maker-ask-{index}"),
                "maker-ask-user",
                Side::Sell,
                100,
                1,
            ))
            .await
            .unwrap();
    }

    for index in 0..WARMUP_ORDERS {
        engine
            .submit_new_order(limit_order(
                &format!("warmup-taker-{index}"),
                &format!("warmup-taker-{index}"),
                "taker-user",
                Side::Buy,
                100,
                1,
            ))
            .await
            .unwrap();
    }

    let mut samples = Vec::with_capacity(MEASURE_ORDERS);
    let workload_start = Instant::now();
    for index in 0..MEASURE_ORDERS {
        let start = Instant::now();
        engine
            .submit_new_order(limit_order(
                &format!("taker-{index}"),
                &format!("taker-{index}"),
                "taker-user",
                Side::Buy,
                100,
                1,
            ))
            .await
            .unwrap();
        samples.push(start.elapsed().as_micros());
    }

    summarize(samples, workload_start.elapsed().as_secs_f64())
}

fn summarize(mut samples: Vec<u128>, elapsed_secs: f64) -> LatencySummary {
    samples.sort_unstable();
    let sample_count = samples.len();
    let total: u128 = samples.iter().sum();
    let max_us = *samples.last().unwrap_or(&0);

    LatencySummary {
        samples: sample_count,
        p50_us: percentile(&samples, 0.50),
        p75_us: percentile(&samples, 0.75),
        p99_us: percentile(&samples, 0.99),
        avg_us: if sample_count == 0 {
            0.0
        } else {
            total as f64 / sample_count as f64
        },
        max_us,
        throughput_ops_per_sec: if elapsed_secs > 0.0 {
            sample_count as f64 / elapsed_secs
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

fn bench_config() -> PartitionedEngineConfig {
    PartitionedEngineConfig {
        partitions: 1,
        queue_capacity: 4096,
        snapshot_interval_commands: usize::MAX,
        max_open_orders_per_user: WARMUP_ORDERS + MEASURE_ORDERS + 1024,
        ..PartitionedEngineConfig::default()
    }
}

fn seeded_risk() -> Arc<RiskEngine> {
    let ledger = Arc::new(LedgerService::new(EventBus::new()));
    for user in ["passive-maker", "maker-ask-user", "taker-user"] {
        ledger
            .process_deposit(user, 100_000_000, format!("deposit-{user}"))
            .unwrap();
        ledger
            .process_position_deposit(user, "btc-usdt", 0, 100_000_000, format!("position-{user}"))
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
