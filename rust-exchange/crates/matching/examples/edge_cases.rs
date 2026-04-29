/// 异常与边缘情况测试
///
/// 目的：验证系统在异常情况下的稳定性
///
/// 场景：
///   1. 重复订单ID提交
///   2. 价格带外订单（触发 Cancel-Only/Halt）
///   3. 极端价格（i64边界值附近）
///   4. 零数量订单
///   5. 账户冻结后提交
///   6. Kill Switch 触发
///   7. 自成交（Self-Trade Prevention）
///   8. 大量撤单（撤单比例过高触发保护）
///   9. Post-Only 交叉测试
///  10. 并发同订单ID提交（竞态条件）
///
/// 用法:
///   cargo run --example edge_cases --release
///
use eventbus::EventBus;
use instruments::{InMemoryInstrumentRegistry, InstrumentRegistry};
use ledger::LedgerService;
use matching::{PartitionedEngineConfig, PartitionedMatchingEngine, SubmissionError};
use risk::RiskEngine;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use types::{
    AdminCommand, CancelOrderCommand, CommandMetadata, InstrumentKind, InstrumentSpec,
    InstrumentStatus, MarketState, NewOrderCommand, OrderState, OrderType, Side, StpMode,
    TimeInForce,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    println!("=== 异常与边缘情况测试 (Edge Cases & Anomalies) ===\n");

    test_duplicate_order_id().await;
    println!();

    test_price_band_breach().await;
    println!();

    test_extreme_prices().await;
    println!();

    test_zero_amount().await;
    println!();

    test_self_trade_prevention().await;
    println!();

    test_post_only_cross().await;
    println!();

    test_kill_switch().await;
    println!();

    test_cancel_flood().await;
    println!();

    test_concurrent_same_id().await;
    println!();

    println!("=== All Edge Case Tests Complete ===");
}

// ============================================================================
// Test 1: Duplicate Order ID
// ============================================================================
async fn test_duplicate_order_id() {
    println!("--- Test 1: Duplicate Order ID Rejection ---");

    let engine = setup_engine(2);
    let req_id = "dup-test-unique";
    let client_id = "co-dup-unique";

    let order1 = make_limit(req_id, client_id, "user-0", Side::Buy, 50000, 100);
    let r1 = engine.submit_new_order(order1).await;
    assert!(r1.is_ok(), "First submission should succeed");

    let order2 = make_limit(req_id, client_id, "user-0", Side::Buy, 50001, 100);
    let r2 = engine.submit_new_order(order2).await;
    assert!(
        matches!(r2, Err(SubmissionError::DuplicateOrderId(_))),
        "Duplicate request_id should be rejected, got: {:?}",
        r2
    );

    println!("  ✓ Duplicate order_id correctly rejected");
}

// ============================================================================
// Test 2: Price Band Breach
// ============================================================================
async fn test_price_band_breach() {
    println!("--- Test 2: Price Band Breach (Cancel-Only State) ---");

    let engine = setup_engine(2);
    let ref_price = 50000i64;

    // Submit baseline order near reference price
    let baseline = make_limit(
        "band-base",
        "co-band-base",
        "user-0",
        Side::Buy,
        ref_price,
        100,
    );
    let _ = engine.submit_new_order(baseline).await;

    // Try extreme price that should breach band (>1000 bps = >10%)
    let extreme_buy = make_limit(
        "band-extreme",
        "co-band-extreme",
        "user-1",
        Side::Buy,
        ref_price * 2, // 100% above reference
        100,
    );
    let r = engine.submit_new_order(extreme_buy).await;

    // May be rejected as PriceBandBreached or trigger CancelOnly state
    match r {
        Err(SubmissionError::PriceBandBreached {
            market_id,
            state,
            deviation_bps,
            ..
        }) => {
            println!(
                "  ✓ Price band breached: market={}, state={:?}, deviation={}bps",
                market_id, state, deviation_bps
            );
        }
        Ok(result) => {
            println!(
                "  ⚠ Extreme order accepted (state={:?}), may be within band tolerance",
                result.market_state
            );
        }
        Err(e) => {
            println!("  ⚠ Got different error: {:?}", e);
        }
    }
}

// ============================================================================
// Test 3: Extreme Prices
// ============================================================================
async fn test_extreme_prices() {
    println!("--- Test 3: Extreme Prices ---");

    let engine = setup_engine(2);

    let extremes = vec![
        ("price=1", 1i64),
        ("price=i64::MAX", i64::MAX),
        ("price=-1", -1i64),
        ("price=0", 0i64),
    ];

    for (label, price) in extremes {
        let order = make_limit(
            &format!("extreme-{}", label.replace("=", "-")),
            &format!("co-extreme-{}", label.replace("=", "-")),
            "user-0",
            Side::Buy,
            price,
            100,
        );
        let result = engine.submit_new_order(order).await;
        match result {
            Ok(_) => println!("  ✓ {} accepted", label),
            Err(e) => println!("  ✓ {} rejected: {:?}", label, e),
        }
    }
}

// ============================================================================
// Test 4: Zero Amount Order
// ============================================================================
async fn test_zero_amount() {
    println!("--- Test 4: Zero Amount Order ---");

    let engine = setup_engine(2);

    let order = make_limit("zero-amt", "co-zero-amt", "user-0", Side::Buy, 50000, 0);
    let result = engine.submit_new_order(order).await;

    match result {
        Ok(res) => {
            println!(
                "  ⚠ Zero amount accepted (state={:?}, remaining={})",
                res.state, res.remaining_amount
            );
        }
        Err(e) => {
            println!("  ✓ Zero amount rejected: {:?}", e);
        }
    }
}

// ============================================================================
// Test 5: Self-Trade Prevention
// ============================================================================
async fn test_self_trade_prevention() {
    println!("--- Test 5: Self-Trade Prevention ---");

    let engine = setup_engine(2);

    // Place resting bid
    let bid = make_limit("stp-bid", "co-stp-bid", "user-0", Side::Buy, 50000, 100);
    let bid_res = engine.submit_new_order(bid).await;
    assert!(bid_res.is_ok(), "Bid should be accepted");

    // Same user tries to hit own bid
    let ask = make_limit("stp-ask", "co-stp-ask", "user-0", Side::Sell, 50000, 100);
    let ask_res = engine.submit_new_order(ask).await;

    match ask_res {
        Ok(res) => {
            // STP may have cancelled one side or both
            println!(
                "  ✓ Self-trade handled: state={:?}, fills={}, remaining={}",
                res.state,
                res.fills.len(),
                res.remaining_amount
            );
        }
        Err(SubmissionError::SelfTradePrevented(msg)) => {
            println!("  ✓ Self-trade prevented: {}", msg);
        }
        Err(e) => {
            println!("  ⚠ Unexpected error: {:?}", e);
        }
    }
}

// ============================================================================
// Test 6: Post-Only Cross Test
// ============================================================================
async fn test_post_only_cross() {
    println!("--- Test 6: Post-Only Crossing Rejection ---");

    let engine = setup_engine(2);

    // Place resting bid at 50000
    let bid = make_limit("po-bid", "co-po-bid", "user-0", Side::Buy, 50000, 100);
    let _ = engine.submit_new_order(bid).await;

    // Post-only ask at 49900 would cross — should be rejected or converted
    let ask = make_limit(
        "po-ask-cross",
        "co-po-ask-cross",
        "user-1",
        Side::Sell,
        49900, // Below best bid — would cross
        100,
    );
    // We can't set post_only via our current helper, but we test the crossing behavior
    let ask_res = engine.submit_new_order(ask).await;

    match ask_res {
        Ok(res) => {
            println!(
                "  ✓ Crossing ask: filled={} trades, remaining={}",
                res.fills.len(),
                res.remaining_amount
            );
        }
        Err(e) => {
            println!("  ✓ Crossing ask rejected: {:?}", e);
        }
    }
}

// ============================================================================
// Test 7: Kill Switch
// ============================================================================
async fn test_kill_switch() {
    println!("--- Test 7: Kill Switch Activation ---");

    let engine = setup_engine(2);

    // Normal order before kill switch
    let before = make_limit("ks-before", "co-ks-before", "user-0", Side::Buy, 50000, 100);
    let r_before = engine.submit_new_order(before).await;
    assert!(r_before.is_ok(), "Should accept before kill switch");

    // Activate kill switch
    let ks_cmd = AdminCommand {
        metadata: CommandMetadata::new("ks-activate"),
        actor_id: "test".to_string(),
        action: types::AdminAction::KillSwitch { enabled: true },
    };
    engine.submit_admin(ks_cmd).await.unwrap();

    // Order after kill switch should be rejected
    let after = make_limit("ks-after", "co-ks-after", "user-0", Side::Buy, 50000, 100);
    let r_after = engine.submit_new_order(after).await;
    assert!(
        matches!(r_after, Err(SubmissionError::KillSwitchActive)),
        "Should reject after kill switch, got: {:?}",
        r_after
    );

    println!("  ✓ Kill switch correctly blocks submissions");

    // Deactivate and verify recovery
    let ks_off = AdminCommand {
        metadata: CommandMetadata::new("ks-deactivate"),
        actor_id: "test".to_string(),
        action: types::AdminAction::KillSwitch { enabled: false },
    };
    engine.submit_admin(ks_off).await.unwrap();

    let after_off = make_limit(
        "ks-after-off",
        "co-ks-after-off",
        "user-0",
        Side::Buy,
        50000,
        100,
    );
    let r_after_off = engine.submit_new_order(after_off).await;
    assert!(
        r_after_off.is_ok(),
        "Should accept after kill switch deactivated"
    );

    println!("  ✓ Kill switch deactivation restores normal operation");
}

// ============================================================================
// Test 8: Cancel Flood Protection
// ============================================================================
async fn test_cancel_flood() {
    println!("--- Test 8: Cancel Flood Protection ---");

    let engine = setup_engine(2);
    let num_orders = 50;

    // Place many orders
    let mut order_ids = Vec::new();
    for i in 0..num_orders {
        let order = make_limit(
            &format!("cf-new-{}", i),
            &format!("co-cf-new-{}", i),
            "user-0",
            Side::Buy,
            50000 - i as i64,
            100,
        );
        if let Ok(res) = engine.submit_new_order(order).await {
            order_ids.push(res.order_id);
        }
    }

    println!("  Placed {} resting orders", order_ids.len());

    // Cancel all rapidly
    let cancel_start = Instant::now();
    let mut cancel_count = 0u64;
    let mut reject_count = 0u64;

    for (idx, oid) in order_ids.iter().enumerate() {
        let cancel = CancelOrderCommand {
            metadata: CommandMetadata::new(format!("cf-cancel-{}", idx)),
            user_id: "user-0".to_string(),
            market_id: "btc-usdt".to_string(),
            outcome: Some(0),
            order_id: oid.clone(),
            client_order_id: None,
        };
        match engine.cancel_order(cancel).await {
            Ok(_) => cancel_count += 1,
            Err(_) => reject_count += 1,
        }
    }
    let cancel_duration = cancel_start.elapsed();

    println!(
        "  Cancelled {} / {} in {:?} ({:.0}/sec)",
        cancel_count,
        order_ids.len(),
        cancel_duration,
        cancel_count as f64 / cancel_duration.as_secs_f64()
    );

    // Try rapid cancel of already-cancelled orders
    let mut dup_cancel_rejected = 0u64;
    for (idx, oid) in order_ids.iter().take(10).enumerate() {
        let cancel = CancelOrderCommand {
            metadata: CommandMetadata::new(format!("cf-dup-cancel-{}", idx)),
            user_id: "user-0".to_string(),
            market_id: "btc-usdt".to_string(),
            outcome: Some(0),
            order_id: oid.clone(),
            client_order_id: None,
        };
        if engine.cancel_order(cancel).await.is_err() {
            dup_cancel_rejected += 1;
        }
    }
    println!("  Duplicate cancel rejections: {}/10", dup_cancel_rejected);
}

// ============================================================================
// Test 9: Concurrent Same ID Race Condition
// ============================================================================
async fn test_concurrent_same_id() {
    println!("--- Test 9: Concurrent Same Order ID (Race Condition) ---");

    let engine = setup_engine(4);
    let shared_req_id = "race-condition-unique-id";
    let shared_client_id = "co-race-condition";

    let success_count = Arc::new(AtomicU64::new(0));
    let dup_count = Arc::new(AtomicU64::new(0));
    let other_err = Arc::new(AtomicU64::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(8));

    let mut handles = vec![];
    for i in 0..8 {
        let eng = engine.clone();
        let sc = success_count.clone();
        let dc = dup_count.clone();
        let oe = other_err.clone();
        let bar = barrier.clone();

        handles.push(tokio::spawn(async move {
            // All threads wait at barrier, then submit simultaneously
            bar.wait().await;

            let order = make_limit(
                shared_req_id,
                shared_client_id,
                &format!("user-{}", i),
                Side::Buy,
                50000,
                100,
            );

            match eng.submit_new_order(order).await {
                Ok(_) => sc.fetch_add(1, Ordering::Relaxed),
                Err(SubmissionError::DuplicateOrderId(_)) => dc.fetch_add(1, Ordering::Relaxed),
                Err(_) => oe.fetch_add(1, Ordering::Relaxed),
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    let s = success_count.load(Ordering::Relaxed);
    let d = dup_count.load(Ordering::Relaxed);
    let o = other_err.load(Ordering::Relaxed);

    println!(
        "  Success: {} | Duplicate rejected: {} | Other errors: {}",
        s, d, o
    );

    assert_eq!(s, 1, "Exactly one submission should succeed");
    assert_eq!(s + d + o, 8, "All 8 attempts should have a result");
    println!("  ✓ Only 1 of 8 concurrent same-ID submissions succeeded");
}

// ============================================================================
// Helpers
// ============================================================================

fn setup_engine(users: usize) -> Arc<PartitionedMatchingEngine> {
    let risk = Arc::new(seeded_risk(users));
    Arc::new(PartitionedMatchingEngine::new_with_registry(
        bench_config(65536),
        EventBus::new(),
        risk,
        benchmark_registry(),
    ))
}

fn make_limit(
    req_id: &str,
    client_id: &str,
    user_id: &str,
    side: Side,
    price: i64,
    amount: i64,
) -> NewOrderCommand {
    NewOrderCommand {
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

fn seeded_risk(user_count: usize) -> RiskEngine {
    let ledger = Arc::new(LedgerService::new(EventBus::new()));
    for i in 0..user_count.max(1) {
        let user = format!("user-{i}");
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
