/// 多市场负载测试
///
/// 目的：模拟不同市场同时活跃的场景，测试分区逻辑和热点市场处理能力
///
/// 用法:
///   cargo run --example multi_market_load --release
///   MARKET_HOT_RATIO=0.7 cargo run --example multi_market_load --release  (70%流量集中在一个市场)
///
use eventbus::EventBus;
use instruments::{InMemoryInstrumentRegistry, InstrumentRegistry};
use ledger::LedgerService;
use matching::{PartitionedEngineConfig, PartitionedMatchingEngine, SubmitOrderResult};
use risk::RiskEngine;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use types::{
    CommandMetadata, InstrumentKind, InstrumentSpec, InstrumentStatus, NewOrderCommand, OrderType,
    Side, StpMode, TimeInForce,
};

const MARKETS: &[&str] = &["btc-usdt", "eth-usdt", "sol-usdt", "btc-perp", "eth-perp"];

#[tokio::main(flavor = "multi_thread", worker_threads = 16)]
async fn main() {
    let hot_ratio: f64 = std::env::var("MARKET_HOT_RATIO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.5);

    test_multi_market_load(hot_ratio).await;
}

async fn test_multi_market_load(hot_ratio: f64) {
    println!("=== 多市场负载测试 (Multi-Market Load) ===");
    println!(
        "Hot market ratio: {:.0}% traffic to BTC-USDT",
        hot_ratio * 100.0
    );
    println!("Markets: {}", MARKETS.join(", "));

    let concurrency: usize = 16;
    let orders_per_client: usize = 200;
    let duration_secs: u64 = 15;

    let risk = Arc::new(seeded_risk(concurrency + 1));
    let engine = Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(65536),
        EventBus::new(),
        risk,
        benchmark_registry(),
    ));

    let per_market_stats: Vec<Arc<MarketStats>> = MARKETS
        .iter()
        .map(|_| Arc::new(MarketStats::default()))
        .collect();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let total_success = Arc::new(AtomicU64::new(0));
    let total_fail = Arc::new(AtomicU64::new(0));

    let start = Instant::now();
    let mut handles = vec![];

    // Reporter task
    {
        let stop = stop_flag.clone();
        let stats_ref: Vec<_> = per_market_stats.iter().cloned().collect();
        let ts = total_success.clone();
        let tf = total_fail.clone();

        handles.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            while !stop.load(Ordering::Relaxed) {
                interval.tick().await;
                let elapsed = start.elapsed().as_secs();
                println!("\n--- T+{}s ---", elapsed);
                for (i, mkt) in MARKETS.iter().enumerate() {
                    let s = &stats_ref[i];
                    let succ = s.success.load(Ordering::Relaxed);
                    let rej = s.rejected.load(Ordering::Relaxed);
                    let avg = if succ > 0 {
                        s.total_latency.load(Ordering::Relaxed) / succ
                    } else {
                        0
                    };
                    println!(
                        "  {:>12}  success={}  rejected={}  avg={}µs",
                        mkt, succ, rej, avg
                    );
                }
                println!(
                    "  TOTAL: success={}  fail={}",
                    ts.load(Ordering::Relaxed),
                    tf.load(Ordering::Relaxed)
                );
            }
        }));
    }

    // Concurrent clients
    for i in 0..concurrency {
        let eng = engine.clone();
        let stop = stop_flag.clone();
        let stats: Vec<Arc<MarketStats>> = per_market_stats.iter().cloned().collect();
        let ts = total_success.clone();
        let tf = total_fail.clone();

        handles.push(tokio::spawn(async move {
            let mut seq = 0;
            while seq < orders_per_client && !stop.load(Ordering::Relaxed) {
                // Select market: hot_ratio goes to btc-usdt, rest distributed evenly
                let market = select_market(i, seq, hot_ratio);
                let market_idx = MARKETS.iter().position(|&m| m == market).unwrap();

                let side = if seq % 2 == 0 { Side::Buy } else { Side::Sell };
                let price = base_price(market) + ((seq as i64 % 10) - 5) * 10;
                let order = make_order(
                    &format!("mkt-{}-{}-{}", market, i, seq),
                    &format!("co-{}-{}-{}", market, i, seq),
                    &format!("user-{}", i),
                    market,
                    side,
                    price,
                    100,
                );

                let t0 = Instant::now();
                match eng.submit_new_order(order).await {
                    Ok(_) => {
                        let elapsed = t0.elapsed().as_micros() as u64;
                        stats[market_idx].success.fetch_add(1, Ordering::Relaxed);
                        stats[market_idx]
                            .total_latency
                            .fetch_add(elapsed, Ordering::Relaxed);
                        ts.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        stats[market_idx].rejected.fetch_add(1, Ordering::Relaxed);
                        tf.fetch_add(1, Ordering::Relaxed);
                        if seq < 3 {
                            println!("  [WARN] {} error: {:?}", market, e);
                        }
                    }
                }
                seq += 1;
            }
        }));
    }

    // Timeout guard
    let timeout_stop = stop_flag.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(duration_secs)).await;
        timeout_stop.store(true, Ordering::Relaxed);
    });

    for h in handles {
        let _ = h.await;
    }
    stop_flag.store(true, Ordering::Relaxed);

    // Final report
    println!("\n=== Final Report ===");
    let mut total_all = 0u64;
    for (i, mkt) in MARKETS.iter().enumerate() {
        let s = &per_market_stats[i];
        let succ = s.success.load(Ordering::Relaxed);
        let rej = s.rejected.load(Ordering::Relaxed);
        total_all += succ + rej;
        let avg = if succ > 0 {
            s.total_latency.load(Ordering::Relaxed) / succ
        } else {
            0
        };
        let pct = if total_all > 0 {
            (succ + rej) as f64 / total_all as f64 * 100.0
        } else {
            0.0
        };
        println!(
            "  {:>12}  submitted={}  success={}  rejected={}  avg={}µs  share={:.1}%",
            mkt,
            succ + rej,
            succ,
            rej,
            avg,
            pct
        );
    }

    // Partition balance check
    let snapshots = engine.export_snapshots().await.unwrap();
    println!("\n=== Partition Distribution ===");
    for snap in &snapshots {
        let market_ids: Vec<_> = snap
            .snapshot
            .markets
            .iter()
            .map(|m| m.market_id.clone())
            .collect();
        println!(
            "  Partition {:>2}: markets={:?}",
            snap.partition_id, market_ids
        );
    }
}

fn select_market(client_id: usize, seq: usize, hot_ratio: f64) -> &'static str {
    let combined = (client_id.wrapping_mul(31).wrapping_add(seq)).wrapping_mul(17);
    let threshold = (hot_ratio * 1000.0) as u64;
    if (combined as u64 % 1000) < threshold {
        "btc-usdt"
    } else {
        let idx = (combined as usize) % (MARKETS.len() - 1);
        MARKETS[1 + idx]
    }
}

fn base_price(market: &str) -> i64 {
    match market {
        "btc-usdt" => 50000,
        "eth-usdt" => 3000,
        "sol-usdt" => 100,
        "btc-perp" => 50000,
        "eth-perp" => 3000,
        _ => 1000,
    }
}

fn make_order(
    req_id: &str,
    client_id: &str,
    user_id: &str,
    market_id: &str,
    side: Side,
    price: i64,
    amount: i64,
) -> NewOrderCommand {
    NewOrderCommand {
        metadata: CommandMetadata::new(req_id),
        client_order_id: client_id.to_string(),
        user_id: user_id.to_string(),
        session_id: None,
        market_id: market_id.to_string(),
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

#[derive(Default)]
struct MarketStats {
    success: AtomicU64,
    rejected: AtomicU64,
    total_latency: AtomicU64,
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

fn benchmark_registry() -> Arc<dyn InstrumentRegistry> {
    let r = InMemoryInstrumentRegistry::new();
    for market in MARKETS {
        r.register(InstrumentSpec {
            instrument_id: market.to_string(),
            kind: InstrumentKind::Spot,
            base_asset: String::new(),
            quote_asset: "USDC".to_string(),
            margin_mode: None,
            max_leverage: None,
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 10_000,
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
    }
    Arc::new(r)
}
