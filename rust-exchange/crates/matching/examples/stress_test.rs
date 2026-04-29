/// 队列极限压力测试工具
///
/// 用法:
///   STRESS_TEST_SATURATION=1 cargo run --example stress_test --release
///   STRESS_TEST_BACKPRESSURE=1 cargo run --example stress_test --release
///   STRESS_TEST_CANCEL_MIX=1 cargo run --example stress_test --release
///
use eventbus::EventBus;
use instruments::InMemoryInstrumentRegistry;
use ledger::LedgerService;
use matching::{PartitionedEngineConfig, PartitionedMatchingEngine};
use risk::RiskEngine;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use types::{
    CancelOrderCommand, CommandMetadata, InstrumentKind, InstrumentSpec, InstrumentStatus,
    OrderType, Side, StpMode, TimeInForce,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 16)]
async fn main() {
    let run_saturation = std::env::var("STRESS_TEST_SATURATION").is_ok();
    let run_backpressure = std::env::var("STRESS_TEST_BACKPRESSURE").is_ok();
    let run_cancel_mix = std::env::var("STRESS_TEST_CANCEL_MIX").is_ok();

    if run_saturation {
        test_queue_saturation().await;
    } else if run_backpressure {
        test_backpressure().await;
    } else if run_cancel_mix {
        test_cancel_mix().await;
    } else {
        println!("Usage:");
        println!("  STRESS_TEST_SATURATION=1 cargo run --example stress_test --release");
        println!("  STRESS_TEST_BACKPRESSURE=1 cargo run --example stress_test --release");
        println!("  STRESS_TEST_CANCEL_MIX=1 cargo run --example stress_test --release");
    }
}

async fn test_queue_saturation() {
    println!("=== 队列饱和测试 (Queue Saturation) ===");
    let concurrency: usize = 8;
    let duration_secs: u64 = 10;

    let risk = Arc::new(seeded_risk(concurrency + 1));
    let engine = Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(65536),
        EventBus::new(),
        risk,
        benchmark_registry(),
    ));

    let latencies = Arc::new(std::sync::Mutex::new(Vec::new()));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let active_cnt = Arc::new(AtomicU64::new(0));

    let start = Instant::now();
    let mut handles = vec![];

    // Reporter task
    {
        let start_time = start;
        let stop = stop_flag.clone();
        let active = active_cnt.clone();
        let lats = latencies.clone();

        handles.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            while !stop.load(Ordering::Relaxed) {
                interval.tick().await;
                let elapsed = start_time.elapsed().as_secs();
                let depth = active.load(Ordering::Relaxed);
                let l = lats.lock().unwrap();
                if !l.is_empty() {
                    let mut s = l.clone();
                    s.sort();
                    let p99 = s[s.len() * 99 / 100];
                    println!("T+{}s depth={} p99={}µs", elapsed, depth, p99);
                }
            }
        }));
    }

    // Concurrent clients
    for i in 0..(concurrency) {
        let eng = engine.clone();
        let stop = stop_flag.clone();
        let lats = latencies.clone();
        let active = active_cnt.clone();

        handles.push(tokio::spawn(async move {
            let mut seq = 0;
            while !stop.load(Ordering::Relaxed) {
                let order = limit_order(
                    &format!("order-{}-{}", i, seq),
                    &format!("order-{}-{}", i, seq),
                    &format!("user-{}", i),
                    if i % 2 == 0 { Side::Buy } else { Side::Sell },
                    50000,
                    100,
                );

                active.fetch_add(1, Ordering::Relaxed);
                let t0 = Instant::now();
                if let Ok(_) = eng.submit_new_order(order).await {
                    let elapsed = t0.elapsed().as_micros();
                    lats.lock().unwrap().push(elapsed);
                }
                active.fetch_sub(1, Ordering::Relaxed);
                seq += 1;
            }
        }));
    }

    tokio::time::sleep(Duration::from_secs(duration_secs)).await;
    stop_flag.store(true, Ordering::Relaxed);

    for h in handles {
        let _ = h.await;
    }

    // Final stats
    let lats = latencies.lock().unwrap();
    if !lats.is_empty() {
        let mut s = lats.clone();
        s.sort();
        let p99 = s[s.len() * 99 / 100];
        let p999 = s.get(s.len() * 999 / 1000).copied().unwrap_or(p99);
        println!("\nFinal: samples={} p99={}µs p999={}µs", s.len(), p99, p999);
    }
}

async fn test_backpressure() {
    println!("=== 背压触发测试 (Backpressure) ===");
    let concurrency: usize = 16;
    let duration_secs: u64 = 3;

    let risk = Arc::new(seeded_risk(concurrency + 1));
    let engine = Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(2048), // Small queue to trigger backpressure
        EventBus::new(),
        risk,
        benchmark_registry(),
    ));

    let success = Arc::new(AtomicU64::new(0));
    let rejected = Arc::new(AtomicU64::new(0));
    let timeout = Arc::new(AtomicU64::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    let start = Instant::now();
    let mut handles = vec![];

    // Reporter
    {
        let s = success.clone();
        let r = rejected.clone();
        let t = timeout.clone();
        let stop = stop_flag.clone();

        handles.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            while !stop.load(Ordering::Relaxed) {
                interval.tick().await;
                println!(
                    "success={} reject={} timeout={}",
                    s.load(Ordering::Relaxed),
                    r.load(Ordering::Relaxed),
                    t.load(Ordering::Relaxed)
                );
            }
        }));
    }

    // Aggressive clients
    for i in 0..concurrency {
        let eng = engine.clone();
        let stop = stop_flag.clone();
        let s = success.clone();
        let r = rejected.clone();
        let tm = timeout.clone();

        handles.push(tokio::spawn(async move {
            let mut seq = 0;
            while !stop.load(Ordering::Relaxed) {
                let order = limit_order(
                    &format!("order-{}-{}", i, seq),
                    &format!("order-{}-{}", i, seq),
                    &format!("user-{}", i),
                    if seq % 2 == 0 { Side::Buy } else { Side::Sell },
                    50000,
                    100,
                );

                match tokio::time::timeout(Duration::from_millis(50), eng.submit_new_order(order))
                    .await
                {
                    Ok(Ok(_)) => s.fetch_add(1, Ordering::Relaxed),
                    Ok(Err(_)) => r.fetch_add(1, Ordering::Relaxed),
                    Err(_) => tm.fetch_add(1, Ordering::Relaxed),
                };

                seq += 1;
            }
        }));
    }

    tokio::time::sleep(Duration::from_secs(duration_secs)).await;
    stop_flag.store(true, Ordering::Relaxed);

    for h in handles {
        let _ = h.await;
    }

    println!(
        "\nFinal: success={} reject={} timeout={}",
        success.load(Ordering::Relaxed),
        rejected.load(Ordering::Relaxed),
        timeout.load(Ordering::Relaxed)
    );
}

async fn test_cancel_mix() {
    println!("=== Cancel优先级测试 (Cancel Mix) ===");
    let concurrency: usize = 4;

    let risk = Arc::new(seeded_risk(concurrency + 1));
    let engine = Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(16384),
        EventBus::new(),
        risk,
        benchmark_registry(),
    ));

    let cancel_lats = Arc::new(std::sync::Mutex::new(Vec::new()));
    let new_lats = Arc::new(std::sync::Mutex::new(Vec::new()));
    let stop_flag = Arc::new(AtomicBool::new(false));

    let mut handles = vec![];

    // Workers
    for i in 0..concurrency {
        let eng = engine.clone();
        let stop = stop_flag.clone();
        let c_lats = cancel_lats.clone();
        let n_lats = new_lats.clone();

        handles.push(tokio::spawn(async move {
            let mut order_ids = vec![];
            let mut seq = 0 as usize;

            while !stop.load(Ordering::Relaxed) {
                let rand = (seq * 17 + i) % 100;

                if rand < 70 && !order_ids.is_empty() {
                    // 70% cancel
                    let idx = seq % order_ids.len();
                    let oid = order_ids.remove(idx);
                    let t0 = Instant::now();

                    if let Ok(_) = eng
                        .cancel_order(CancelOrderCommand {
                            metadata: CommandMetadata::new(format!("cancel-{}-{}", i, seq)),
                            user_id: format!("user-{}", i),
                            market_id: "btc-usdt".to_string(),
                            outcome: Some(0),
                            order_id: oid,
                            client_order_id: None,
                        })
                        .await
                    {
                        c_lats.lock().unwrap().push(t0.elapsed().as_micros());
                    }
                } else {
                    // 30% new
                    let order = limit_order(
                        &format!("new-{}-{}", i, seq),
                        &format!("new-{}-{}", i, seq),
                        &format!("user-{}", i),
                        if seq % 2 == 0 { Side::Buy } else { Side::Sell },
                        50000,
                        100,
                    );
                    let t0 = Instant::now();

                    if let Ok(res) = eng.submit_new_order(order).await {
                        n_lats.lock().unwrap().push(t0.elapsed().as_micros());
                        order_ids.push(res.order_id);
                    }
                }

                seq += 1;
            }
        }));
    }

    tokio::time::sleep(Duration::from_secs(30)).await;
    stop_flag.store(true, Ordering::Relaxed);

    for h in handles {
        let _ = h.await;
    }

    // Analysis
    let cancels = cancel_lats.lock().unwrap();
    let news = new_lats.lock().unwrap();

    if !cancels.is_empty() && !news.is_empty() {
        let mut cs = cancels.clone();
        cs.sort();
        let mut ns = news.clone();
        ns.sort();

        let c_p99 = cs[cs.len() * 99 / 100];
        let n_p99 = ns[ns.len() * 99 / 100];

        println!(
            "Cancel P99: {}µs\nNew P99:    {}µs\nRatio: {:.2}x",
            c_p99,
            n_p99,
            c_p99 as f64 / n_p99 as f64
        );
    } else {
        println!("Not enough samples");
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn limit_order(
    req_id: &str,
    client_id: &str,
    user_id: &str,
    side: Side,
    price: i64,
    amount: i64,
) -> types::NewOrderCommand {
    types::NewOrderCommand {
        metadata: CommandMetadata::new(req_id),
        client_order_id: client_id.to_string(),
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
        stp_mode: StpMode::default(),
        trigger_price: None,
        trigger_type: None,
        display_qty: None,
        min_fill_qty: None,
        stp_group_id: None,
        is_market_maker: false,
    }
}

fn bench_config(cap: usize) -> PartitionedEngineConfig {
    PartitionedEngineConfig {
        partitions: 8,
        queue_capacity: cap,
        snapshot_interval_commands: usize::MAX,
        max_open_orders_per_user: 200,
        ..PartitionedEngineConfig::default()
    }
}

fn seeded_risk(extra_users: usize) -> RiskEngine {
    let ledger = Arc::new(LedgerService::new(EventBus::new()));
    let mut users = vec!["maker-ask-user".to_string()];
    for i in 0..extra_users.max(1) {
        users.push(format!("user-{i}"));
    }

    for user in users {
        ledger
            .process_deposit(&user, 1_000_000_000, format!("dep-{user}"))
            .unwrap();
        ledger
            .process_position_deposit(&user, "btc-usdt", 0, 1_000_000_000, format!("pos-{user}"))
            .unwrap();
    }
    RiskEngine::new(ledger)
}

fn benchmark_registry() -> Arc<dyn instruments::InstrumentRegistry> {
    let r = InMemoryInstrumentRegistry::new();
    r.register(InstrumentSpec {
        instrument_id: "btc-usdt".to_string(),
        kind: InstrumentKind::Spot,
        base_asset: String::new(),
        quote_asset: "USDC".to_string(),
        margin_mode: None,
        max_leverage: None,
        tick_size: 1,
        lot_size: 1,
        price_band_bps: 1_000,
        risk_policy_id: "spot-v1".to_string(),
        min_order_amount: 0,
        max_notional: 0,
        maker_fee_bps: 0,
        taker_fee_bps: 0,
        max_position_notional: 0,
        maintenance_margin_bps: 0,
        contract_multiplier: 1,
        funding_interval_secs: 0,
        status: InstrumentStatus::Active,
        circuit_breaker: None,
        mm_protection: None,
        max_order_amount: 0,
        order_type_rule: None,
        margin_rule: None,
        liquidation_rule: None,
        fee_schedule: None,
        margin_tiers: None,
        expiry: None,
        option_spec: None,
        settlement_currency: None,
    });
    Arc::new(r)
}
