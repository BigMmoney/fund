/// 混合订单类型测试
///
/// 目的：验证系统处理不同订单类型（Limit、Market、IOC、FOK、止盈止损等）的正确性和性能
///
/// 用法:
///   cargo run --example mixed_order_types --release
///
use eventbus::EventBus;
use instruments::{InMemoryInstrumentRegistry, InstrumentRegistry};
use ledger::LedgerService;
use matching::{PartitionedEngineConfig, PartitionedMatchingEngine};
use risk::RiskEngine;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use types::{
    CancelOrderCommand, CommandMetadata, InstrumentKind, InstrumentSpec, InstrumentStatus,
    NewOrderCommand, OrderState, OrderType, Side, StpMode, TimeInForce,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 16)]
async fn main() {
    test_mixed_order_types().await;
}

async fn test_mixed_order_types() {
    println!("=== 混合订单类型测试 (Mixed Order Types) ===");

    let concurrency: usize = 8;
    let orders_per_client: usize = 100;
    let duration_secs: u64 = 20;

    let risk = Arc::new(seeded_risk(concurrency + 1));
    let engine = Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(65536),
        EventBus::new(),
        risk,
        benchmark_registry(),
    ));

    let stop_flag = Arc::new(AtomicBool::new(false));
    let total_success = Arc::new(AtomicU64::new(0));
    let total_fail = Arc::new(AtomicU64::new(0));

    // Per-type stats
    let order_types = vec![
        OrderTypeSpec::LimitGtc,
        OrderTypeSpec::LimitIoc,
        OrderTypeSpec::LimitFok,
        OrderTypeSpec::Market,
        OrderTypeSpec::StopMarket,
        OrderTypeSpec::StopLimit,
        OrderTypeSpec::TakeProfitMarket,
        OrderTypeSpec::TakeProfitLimit,
    ];

    let type_stats: Vec<Arc<TypeStats>> = order_types
        .iter()
        .map(|_| Arc::new(TypeStats::default()))
        .collect();

    let start = Instant::now();
    let mut handles = vec![];

    let ot_labels: Vec<_> = order_types.iter().map(|s| s.label()).collect();

    // Reporter task
    {
        let stop = stop_flag.clone();
        let stats_ref: Vec<_> = type_stats.iter().cloned().collect();
        let ts = total_success.clone();
        let tf = total_fail.clone();

        handles.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            while !stop.load(Ordering::Relaxed) {
                interval.tick().await;
                let elapsed = start.elapsed().as_secs();
                println!("\n--- T+{}s ---", elapsed);
                for (i, label) in ot_labels.iter().enumerate() {
                    let s = &stats_ref[i];
                    let succ = s.success.load(Ordering::Relaxed);
                    let rej = s.rejected.load(Ordering::Relaxed);
                    let filled = s.filled.load(Ordering::Relaxed);
                    let resting = s.resting.load(Ordering::Relaxed);
                    let avg = if succ > 0 {
                        s.total_latency.load(Ordering::Relaxed) / succ
                    } else {
                        0
                    };
                    println!(
                        "  {:>20}  ok={}  rej={}  filled={}  resting={}  avg={}µs",
                        label, succ, rej, filled, resting, avg
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

    // Concurrent clients with mixed order types
    let ot_count = order_types.len();
    for i in 0..concurrency {
        let eng = engine.clone();
        let stop = stop_flag.clone();
        let stats: Vec<Arc<TypeStats>> = type_stats.iter().cloned().collect();
        let ts = total_success.clone();
        let tf = total_fail.clone();
        let specs: Vec<_> = order_types.iter().cloned().collect();

        handles.push(tokio::spawn(async move {
            let mut seq = 0;
            let mut resting_order_ids: Vec<(String, String)> = Vec::new(); // (order_id, market_id)

            while seq < orders_per_client && !stop.load(Ordering::Relaxed) {
                let type_idx = seq % ot_count;
                let spec = &specs[type_idx];

                let side = if seq % 2 == 0 { Side::Buy } else { Side::Sell };
                let order = spec.make_order(i, seq, side);

                let t0 = Instant::now();
                match eng.submit_new_order(order.clone()).await {
                    Ok(result) => {
                        let elapsed = t0.elapsed().as_micros() as u64;
                        stats[type_idx].success.fetch_add(1, Ordering::Relaxed);
                        stats[type_idx]
                            .total_latency
                            .fetch_add(elapsed, Ordering::Relaxed);
                        ts.fetch_add(1, Ordering::Relaxed);

                        // Track fills vs resting
                        if !result.fills.is_empty() {
                            stats[type_idx]
                                .filled
                                .fetch_add(result.fills.len() as u64, Ordering::Relaxed);
                        }
                        if result.state == OrderState::Active
                            || result.state == OrderState::PartiallyFilled
                        {
                            stats[type_idx].resting.fetch_add(1, Ordering::Relaxed);
                            resting_order_ids
                                .push((result.order_id.clone(), "btc-usdt".to_string()));
                        }
                    }
                    Err(e) => {
                        stats[type_idx].rejected.fetch_add(1, Ordering::Relaxed);
                        tf.fetch_add(1, Ordering::Relaxed);
                        if seq < 5 {
                            println!("  [WARN] {:>20} error: {:?}", spec.label(), e);
                        }
                    }
                }
                seq += 1;

                // Periodically cancel some resting orders to test cancel path
                if seq % 20 == 0 && !resting_order_ids.is_empty() {
                    let idx = (seq / 20 - 1) % resting_order_ids.len();
                    let (oid, mid) = resting_order_ids.remove(idx);
                    let cancel = CancelOrderCommand {
                        metadata: CommandMetadata::new(format!("cancel-{}-{}", i, seq)),
                        user_id: format!("user-{}", i),
                        market_id: mid.clone(),
                        outcome: Some(0),
                        order_id: oid.clone(),
                        client_order_id: None,
                    };
                    let _ = eng.cancel_order(cancel).await;
                }
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
    for (i, spec) in order_types.iter().enumerate() {
        let s = &type_stats[i];
        let succ = s.success.load(Ordering::Relaxed);
        let rej = s.rejected.load(Ordering::Relaxed);
        total_all += succ + rej;
        let avg = if succ > 0 {
            s.total_latency.load(Ordering::Relaxed) / succ
        } else {
            0
        };
        let filled = s.filled.load(Ordering::Relaxed);
        let resting = s.resting.load(Ordering::Relaxed);
        println!(
            "  {:>20}  submitted={}  success={}  rejected={}  filled={}  resting={}  avg={}µs",
            spec.label(),
            succ + rej,
            succ,
            rej,
            filled,
            resting,
            avg
        );
    }
    println!(
        "\n  TOTAL: submitted={}  success={}  fail={}",
        total_all,
        total_success.load(Ordering::Relaxed),
        total_fail.load(Ordering::Relaxed)
    );
}

// ============================================================================
// Order Type Specifications
// ============================================================================

#[derive(Clone)]
enum OrderTypeSpec {
    LimitGtc,
    LimitIoc,
    LimitFok,
    Market,
    StopMarket,
    StopLimit,
    TakeProfitMarket,
    TakeProfitLimit,
}

impl OrderTypeSpec {
    fn label(&self) -> &'static str {
        match self {
            Self::LimitGtc => "Limit/GTC",
            Self::LimitIoc => "Limit/IOC",
            Self::LimitFok => "Limit/FOK",
            Self::Market => "Market",
            Self::StopMarket => "StopMarket",
            Self::StopLimit => "StopLimit",
            Self::TakeProfitMarket => "TakeProfitMarket",
            Self::TakeProfitLimit => "TakeProfitLimit",
        }
    }

    fn make_order(&self, client_id: usize, seq: usize, side: Side) -> NewOrderCommand {
        let req_id = format!("{}-{}-{}", self.label(), client_id, seq);
        let client_id_str = format!("co-{}-{}-{}", self.label(), client_id, seq);
        let user_id = format!("user-{}", client_id);

        match self {
            Self::LimitGtc => make_limit_order(
                &req_id,
                &client_id_str,
                &user_id,
                side,
                50000,
                100,
                TimeInForce::Gtc,
            ),
            Self::LimitIoc => make_limit_order(
                &req_id,
                &client_id_str,
                &user_id,
                side,
                50000,
                100,
                TimeInForce::Ioc,
            ),
            Self::LimitFok => make_limit_order(
                &req_id,
                &client_id_str,
                &user_id,
                side,
                50000,
                100,
                TimeInForce::Fok,
            ),
            Self::Market => make_market_order(&req_id, &client_id_str, &user_id, side, 100),
            Self::StopMarket => {
                make_stop_order(&req_id, &client_id_str, &user_id, side, 50000, 100, false)
            }
            Self::StopLimit => {
                make_stop_order(&req_id, &client_id_str, &user_id, side, 50000, 100, true)
            }
            Self::TakeProfitMarket => {
                make_stop_order(&req_id, &client_id_str, &user_id, side, 51000, 100, false)
            }
            Self::TakeProfitLimit => {
                make_stop_order(&req_id, &client_id_str, &user_id, side, 51000, 100, true)
            }
        }
    }
}

fn make_limit_order(
    req_id: &str,
    client_id: &str,
    user_id: &str,
    side: Side,
    price: i64,
    amount: i64,
    tif: TimeInForce,
) -> NewOrderCommand {
    NewOrderCommand {
        metadata: CommandMetadata::new(req_id),
        client_order_id: client_id.to_string(),
        user_id: user_id.to_string(),
        session_id: None,
        market_id: "btc-usdt".to_string(),
        side,
        order_type: OrderType::Limit,
        time_in_force: tif,
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

fn make_market_order(
    req_id: &str,
    client_id: &str,
    user_id: &str,
    side: Side,
    amount: i64,
) -> NewOrderCommand {
    NewOrderCommand {
        metadata: CommandMetadata::new(req_id),
        client_order_id: client_id.to_string(),
        user_id: user_id.to_string(),
        session_id: None,
        market_id: "btc-usdt".to_string(),
        side,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        price: None,
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

fn make_stop_order(
    req_id: &str,
    client_id: &str,
    user_id: &str,
    side: Side,
    trigger_price: i64,
    amount: i64,
    is_limit: bool,
) -> NewOrderCommand {
    let (order_type, price) = if is_limit {
        (OrderType::StopLimit, Some(trigger_price))
    } else {
        (OrderType::StopMarket, None)
    };

    NewOrderCommand {
        metadata: CommandMetadata::new(req_id),
        client_order_id: client_id.to_string(),
        user_id: user_id.to_string(),
        session_id: None,
        market_id: "btc-usdt".to_string(),
        side,
        order_type,
        time_in_force: TimeInForce::Gtc,
        price,
        amount,
        outcome: 0,
        post_only: false,
        reduce_only: false,
        leverage: None,
        expires_at: None,
        stp_mode: StpMode::default(),
        trigger_price: Some(trigger_price),
        trigger_type: Some(types::TriggerType::LastPrice),
        display_qty: None,
        min_fill_qty: None,
        stp_group_id: None,
        is_market_maker: false,
    }
}

#[derive(Default)]
struct TypeStats {
    success: AtomicU64,
    rejected: AtomicU64,
    filled: AtomicU64,
    resting: AtomicU64,
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
    r.register(InstrumentSpec {
        instrument_id: "btc-usdt".to_string(),
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
    Arc::new(r)
}
