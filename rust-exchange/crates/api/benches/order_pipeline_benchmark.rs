//! End-to-end order pipeline benchmarks.
//!
//! Measures 3 critical stages of the order submission chain:
//! 1. **Sequencer benchmarks** — WAL append latency, dedup overhead, sequence numbering
//! 2. **Ledger + conservation benchmarks** — deposit latency, balance lookups, invariant checks
//! 3. **OrderBook matching benchmarks** — insert, match, cancel latency on the core data structure
//!
//! Run with: `cargo bench --package api`

use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use eventbus::EventBus;
use ledger::LedgerService;
use matching::high_performance::OrderBook;
use persistence::{InMemoryWal, WalStore};
use sequencer::{SequencedCommandRecord, Sequencer};
use types::{
    Command, CommandMetadata, NewOrderCommand, Order, OrderState, OrderType, Side, TimeInForce,
};

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────

fn build_ledger() -> Arc<LedgerService> {
    let event_bus = EventBus::new();
    let wal = Arc::new(InMemoryWal::new());
    Arc::new(LedgerService::with_wal_store(event_bus, wal))
}

fn build_sequencer() -> Arc<Sequencer> {
    let wal: Arc<dyn WalStore<SequencedCommandRecord>> = Arc::new(InMemoryWal::new());
    Arc::new(Sequencer::with_wal(1, wal))
}

fn make_order_command(
    request_id: &str,
    user_id: &str,
    market_id: &str,
    side: Side,
    price: i64,
    amount: i64,
) -> NewOrderCommand {
    NewOrderCommand {
        metadata: CommandMetadata::new(request_id),
        client_order_id: format!("bench-{request_id}"),
        user_id: user_id.into(),
        session_id: None,
        market_id: market_id.into(),
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
        stp_mode: types::StpMode::default(),
        trigger_price: None,
        trigger_type: None,
        display_qty: None,
        min_fill_qty: None,
        stp_group_id: None,
        is_market_maker: false,
    }
}

fn make_order_book_order(id: u64, side: Side, price: i64, amount: i64) -> Order {
    Order {
        id: format!("bench-{id}"),
        user_id: if matches!(side, Side::Buy) {
            "buyer".into()
        } else {
            "seller".into()
        },
        market_id: "BTC-USDT".into(),
        side,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price,
        amount,
        filled_amount: 0,
        outcome: 0,
        status: OrderState::Active,
        created_at: chrono::Utc::now(),
        updated_at: None,
        client_order_id: None,
        trigger_price: None,
        trigger_type: None,
        cumulative_fee: 0,
        avg_fill_price: None,
    }
}

// ─────────────────────────────────────────────────────────────
// Group 1: Sequencer Benchmarks — WAL append + dedup latency
// ─────────────────────────────────────────────────────────────

fn bench_sequencer_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequencer");

    // 1a: Single order sequencing latency (WAL append path)
    group.bench_function("sequence_single_order", |b| {
        let sequencer = build_sequencer();
        let mut seq = 0u64;
        b.iter(|| {
            seq += 1;
            let cmd = Command::NewOrder(make_order_command(
                &format!("req-{seq}"),
                "user",
                "btc-usdt",
                Side::Buy,
                50_000,
                10,
            ));
            black_box(sequencer.sequence_and_append(cmd).unwrap());
        });
    });

    // 1b: Batch sequencing throughput (100 orders)
    group.throughput(Throughput::Elements(100));
    group.bench_function("sequence_batch_100", |b| {
        b.iter_batched(
            || build_sequencer(),
            |sequencer| {
                for i in 0..100 {
                    let cmd = Command::NewOrder(make_order_command(
                        &format!("seq-batch-{i}"),
                        "user",
                        "btc-usdt",
                        Side::Buy,
                        50_000 - (i as i64 % 100),
                        10,
                    ));
                    black_box(sequencer.sequence_and_append(cmd).unwrap());
                }
            },
            BatchSize::SmallInput,
        );
    });

    // 1c: Deduplication overhead — repeated request_id should fail fast
    group.bench_function("deduplicate_repeated_request_id", |b| {
        let sequencer = build_sequencer();
        // First one succeeds, rest should be rejected
        let cmd = Command::NewOrder(make_order_command(
            "dedup-test",
            "user",
            "btc-usdt",
            Side::Buy,
            50_000,
            10,
        ));
        let _ = sequencer.sequence_and_append(cmd);
        b.iter(|| {
            let cmd = Command::NewOrder(make_order_command(
                "dedup-test",
                "user",
                "btc-usdt",
                Side::Buy,
                50_000,
                10,
            ));
            black_box(sequencer.sequence_and_append(cmd))
        });
    });

    group.finish();
}

// ─────────────────────────────────────────────────────────────
// Group 2: Ledger + Conservation Benchmarks
// ─────────────────────────────────────────────────────────────

fn bench_ledger_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("ledger");

    // 2a: Cash deposit latency
    group.bench_function("deposit_single", |b| {
        let ledger = build_ledger();
        b.iter(|| {
            ledger
                .process_deposit("bench-user", black_box(1_000_000), "bench-dep".into())
                .unwrap()
        });
    });

    // 2b: Position deposit latency
    group.bench_function("position_deposit_single", |b| {
        let ledger = build_ledger();
        b.iter(|| {
            ledger
                .process_position_deposit(
                    "bench-user",
                    "btc-usdt",
                    0,
                    black_box(100_000),
                    "bench-pos".into(),
                )
                .unwrap()
        });
    });

    // 2c: Balance lookup latency after 100 deposits
    group.throughput(Throughput::Elements(100));
    group.bench_function("balance_lookup_after_100_deposits", |b| {
        b.iter_batched(
            || {
                let ledger = build_ledger();
                for i in 0..100 {
                    ledger
                        .process_deposit(&format!("user-{i}"), 1_000_000, format!("dep-{i}"))
                        .unwrap();
                }
                ledger
            },
            |ledger| {
                for i in 0..100 {
                    black_box(ledger.cash_available_balance(&format!("user-{i}")));
                }
            },
            BatchSize::SmallInput,
        );
    });

    // 2d: Global invariant check cost (conservation verification)
    group.bench_function("verify_global_invariant_100_users", |b| {
        b.iter_batched(
            || {
                let ledger = build_ledger();
                for i in 0..100 {
                    ledger
                        .process_deposit(&format!("user-{i}"), 1_000_000, format!("dep-{i}"))
                        .unwrap();
                }
                ledger
            },
            |ledger| black_box(ledger.verify_global_invariant()),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ─────────────────────────────────────────────────────────────
// Group 3: OrderBook Matching Benchmarks — core execution latency
// ─────────────────────────────────────────────────────────────

fn bench_orderbook_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook");

    // 3a: Insert resting orders (no match) — measures add_order latency
    group.throughput(Throughput::Elements(1000));
    group.bench_function("insert_1000_resting_orders", |b| {
        b.iter_batched(
            || {
                let book = OrderBook::new();
                let orders: Vec<Order> = (0..1000)
                    .map(|i| {
                        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                        let price = if matches!(side, Side::Buy) {
                            50_000 - (i as i64 % 100)
                        } else {
                            50_100 + (i as i64 % 100)
                        };
                        make_order_book_order(i, side, price, 100)
                    })
                    .collect();
                (book, orders)
            },
            |(mut book, orders)| {
                for order in orders {
                    book.add_order(order);
                }
                black_box(book);
            },
            BatchSize::SmallInput,
        );
    });

    // 3b: Crossing order — measures add_order latency for aggressive orders
    group.bench_function("add_aggressive_buy_sweeping_100_levels", |b| {
        b.iter_batched(
            || {
                let mut book = OrderBook::new();
                // Seed 100 resting sell levels
                for i in 0..100 {
                    let sell = make_order_book_order(i, Side::Sell, 50_000 + (i as i64), 10);
                    book.add_order(sell);
                }
                book
            },
            |mut book| {
                // Send a large crossing buy (adds to book, no matching in this OrderBook impl)
                let buy = make_order_book_order(9999, Side::Buy, 50_200, 500);
                book.add_order(buy);
                black_box(book.best_bid());
                black_box(book.best_ask());
            },
            BatchSize::SmallInput,
        );
    });

    // 3c: Mixed insert — alternating buy/sell on a growing book
    group.throughput(Throughput::Elements(500));
    group.bench_function("insert_500_mixed_orders", |b| {
        b.iter_batched(
            || OrderBook::new(),
            |mut book| {
                for i in 0..500 {
                    let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                    let price = if matches!(side, Side::Buy) {
                        50_000 - (i as i64 % 200)
                    } else {
                        50_100 + (i as i64 % 200)
                    };
                    book.add_order(make_order_book_order(i, side, price, 10));
                }
                black_box(book);
            },
            BatchSize::SmallInput,
        );
    });

    // 3d: Best bid/ask lookup on a 10k-order book
    group.bench_function("best_bid_ask_lookup_10k", |b| {
        b.iter(|| {
            let mut book = OrderBook::new();
            for i in 0..10_000 {
                let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                let price = if matches!(side, Side::Buy) {
                    50_000 - (i as i64 % 500)
                } else {
                    50_100 + (i as i64 % 500)
                };
                book.add_order(make_order_book_order(i, side, price, 10));
            }
            black_box(book.best_bid());
            black_box(book.best_ask());
        });
    });

    group.finish();
}

// ─────────────────────────────────────────────────────────────
// Group 4: E2E Submit-Order Benchmarks — full pipeline
//
// Uses the real PartitionedMatchingEngine + Sequencer + Ledger
// to measure actual HTTP-to-settlement latency.
// ─────────────────────────────────────────────────────────────

fn bench_e2e_submit_order(c: &mut Criterion) {
    use matching::partitioned::{CancelResult, PartitionedEngineConfig, PartitionedMatchingEngine};
    use risk::RiskEngine;
    use types::{CancelOrderCommand, InstrumentKind, InstrumentSpec, InstrumentStatus};

    let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");

    let mut group = c.benchmark_group("e2e_submit_order");
    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(5));

    fn make_btc_spec() -> InstrumentSpec {
        InstrumentSpec {
            instrument_id: "BTC-USDT".into(),
            kind: InstrumentKind::Spot,
            base_asset: String::new(),
            quote_asset: "USDT".into(),
            margin_mode: None,
            max_leverage: None,
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "spot-v1".into(),
            min_order_amount: 1,
            max_notional: 0,
            maker_fee_bps: 10,
            taker_fee_bps: 20,
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
        }
    }

    fn make_eth_spec() -> InstrumentSpec {
        InstrumentSpec {
            instrument_id: "ETH-USDT".into(),
            kind: InstrumentKind::Spot,
            base_asset: String::new(),
            quote_asset: "USDT".into(),
            margin_mode: None,
            max_leverage: None,
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "spot-v1".into(),
            min_order_amount: 1,
            max_notional: 0,
            maker_fee_bps: 10,
            taker_fee_bps: 20,
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
        }
    }

    // Shared setup: build engine with 1 market, seed liquidity
    // MUST be called inside an active tokio runtime context (rt.enter())
    // because PartitionedMatchingEngine spawns partition tasks during construction.
    let setup_engine = || {
        let event_bus = EventBus::new();
        let ledger = Arc::new(LedgerService::new(event_bus.clone()));
        let risk = Arc::new(RiskEngine::new(ledger.clone()));
        let instruments = Arc::new(instruments::InMemoryInstrumentRegistry::new());
        instruments.register(make_btc_spec());
        let config = PartitionedEngineConfig {
            partitions: 1,
            max_open_orders_per_user: 10_000,
            auto_recover_after_commands: 0,
            ..Default::default()
        };
        let engine = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config,
            event_bus,
            risk,
            instruments,
            None,
            None,
            None,
            None,
        )
        .expect("engine");
        (engine, ledger)
    };

    // Seed resting orders on a market
    let seed_liquidity = |engine: &PartitionedMatchingEngine, n: usize| {
        for i in 0..n {
            let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
            let price = if matches!(side, Side::Buy) {
                50_000 - (i as i64 % 100)
            } else {
                50_100 + (i as i64 % 100)
            };
            let cmd = NewOrderCommand {
                metadata: CommandMetadata::new(&format!("seed-{i}")),
                client_order_id: format!("seed-{i}"),
                user_id: "seed-user".into(),
                session_id: None,
                market_id: "BTC-USDT".into(),
                side,
                order_type: OrderType::Limit,
                time_in_force: TimeInForce::Gtc,
                price: Some(price),
                amount: 100,
                outcome: 0,
                post_only: false,
                reduce_only: false,
                leverage: None,
                expires_at: None,
                stp_mode: types::StpMode::default(),
                trigger_price: None,
                trigger_type: None,
                display_qty: None,
                min_fill_qty: None,
                stp_group_id: None,
                is_market_maker: false,
            };
            let _ = rt.block_on(engine.submit_new_order(cmd));
        }
    };

    // 4a: Single market, single order — baseline e2e latency
    group.bench_function("single_market_baseline", |b| {
        let _guard = rt.enter();
        let (engine, ledger) = setup_engine();
        seed_liquidity(&engine, 50);
        // Seed user balance
        ledger
            .process_deposit("e2e-user", 100_000_000, "e2e-seed".into())
            .expect("deposit");
        ledger
            .process_position_deposit("e2e-user", "BTC-USDT", 0, 100_000, "e2e-pos".into())
            .expect("position deposit");

        b.iter_custom(|iters| {
            let mut total_us = 0u128;
            for i in 0..iters {
                let cmd = NewOrderCommand {
                    metadata: CommandMetadata::new(&format!("e2e-single-{i}")),
                    client_order_id: format!("e2e-single-{i}"),
                    user_id: "e2e-user".into(),
                    session_id: None,
                    market_id: "BTC-USDT".into(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price: Some(50_000 + (i as i64 % 50)),
                    amount: 10,
                    outcome: 0,
                    post_only: false,
                    reduce_only: false,
                    leverage: None,
                    expires_at: None,
                    stp_mode: types::StpMode::default(),
                    trigger_price: None,
                    trigger_type: None,
                    display_qty: None,
                    min_fill_qty: None,
                    stp_group_id: None,
                    is_market_maker: false,
                };
                let start = std::time::Instant::now();
                let _ = rt.block_on(engine.submit_new_order(cmd));
                total_us += start.elapsed().as_micros();
            }
            std::time::Duration::from_micros(total_us as u64)
        });
    });

    // 4b: Two markets — cross-market interference measurement
    group.bench_function("two_markets_interference", |b| {
        let _guard = rt.enter();
        let event_bus = EventBus::new();
        let ledger = Arc::new(LedgerService::new(event_bus.clone()));
        let risk = Arc::new(RiskEngine::new(ledger.clone()));
        let instruments = Arc::new(instruments::InMemoryInstrumentRegistry::new());
        instruments.register(make_btc_spec());
        instruments.register(make_eth_spec());
        let config = PartitionedEngineConfig {
            partitions: 2,
            max_open_orders_per_user: 10_000,
            auto_recover_after_commands: 0,
            ..Default::default()
        };
        let engine = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config,
            event_bus,
            risk,
            instruments,
            None,
            None,
            None,
            None,
        )
        .expect("engine");

        // Alternate between BTC and ETH orders
        b.iter_custom(|iters| {
            let mut total_us = 0u128;
            for i in 0..iters {
                let market = if i % 2 == 0 { "BTC-USDT" } else { "ETH-USDT" };
                let cmd = NewOrderCommand {
                    metadata: CommandMetadata::new(&format!("e2e-2mkt-{i}")),
                    client_order_id: format!("e2e-2mkt-{i}"),
                    user_id: "e2e-user".into(),
                    session_id: None,
                    market_id: market.into(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price: Some(50_000),
                    amount: 10,
                    outcome: 0,
                    post_only: false,
                    reduce_only: false,
                    leverage: None,
                    expires_at: None,
                    stp_mode: types::StpMode::default(),
                    trigger_price: None,
                    trigger_type: None,
                    display_qty: None,
                    min_fill_qty: None,
                    stp_group_id: None,
                    is_market_maker: false,
                };
                let start = std::time::Instant::now();
                let _ = rt.block_on(engine.submit_new_order(cmd));
                total_us += start.elapsed().as_micros();
            }
            std::time::Duration::from_micros(total_us as u64)
        });
    });

    // 4c: Batch ACK — submit N orders rapidly, measure aggregate throughput
    group.throughput(Throughput::Elements(50));
    group.bench_function("batch_50_orders", |b| {
        let _guard = rt.enter();
        let (engine, ledger) = setup_engine();
        seed_liquidity(&engine, 50);
        ledger
            .process_deposit("e2e-user", 100_000_000, "e2e-seed".into())
            .expect("deposit");
        ledger
            .process_position_deposit("e2e-user", "BTC-USDT", 0, 100_000, "e2e-pos".into())
            .expect("position deposit");

        b.iter_custom(|iters| {
            let mut total_us = 0u128;
            for _ in 0..iters {
                let batch_start = std::time::Instant::now();
                for i in 0..50 {
                    let cmd = NewOrderCommand {
                        metadata: CommandMetadata::new(&format!("e2e-batch-{i}")),
                        client_order_id: format!("e2e-batch-{i}"),
                        user_id: "e2e-user".into(),
                        session_id: None,
                        market_id: "BTC-USDT".into(),
                        side: Side::Buy,
                        order_type: OrderType::Limit,
                        time_in_force: TimeInForce::Gtc,
                        price: Some(50_000 + i),
                        amount: 10,
                        outcome: 0,
                        post_only: false,
                        reduce_only: false,
                        leverage: None,
                        expires_at: None,
                        stp_mode: types::StpMode::default(),
                        trigger_price: None,
                        trigger_type: None,
                        display_qty: None,
                        min_fill_qty: None,
                        stp_group_id: None,
                        is_market_maker: false,
                    };
                    let _ = rt.block_on(engine.submit_new_order(cmd));
                }
                total_us += batch_start.elapsed().as_micros();
            }
            std::time::Duration::from_micros(total_us as u64)
        });
    });

    // 4d: Cancel + Replace cycle — submit, cancel, resubmit
    group.bench_function("cancel_replace_cycle", |b| {
        let _guard = rt.enter();
        let (engine, ledger) = setup_engine();
        seed_liquidity(&engine, 50);
        ledger
            .process_deposit("e2e-user", 100_000_000, "e2e-seed".into())
            .expect("deposit");
        ledger
            .process_position_deposit("e2e-user", "BTC-USDT", 0, 100_000, "e2e-pos".into())
            .expect("position deposit");

        b.iter_custom(|iters| {
            let mut total_us = 0u128;
            for i in 0..iters {
                let cycle_start = std::time::Instant::now();
                // Step 1: Submit order
                let submit_cmd = NewOrderCommand {
                    metadata: CommandMetadata::new(&format!("e2e-cr-{i}")),
                    client_order_id: format!("e2e-cr-{i}"),
                    user_id: "e2e-user".into(),
                    session_id: None,
                    market_id: "BTC-USDT".into(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price: Some(50_000),
                    amount: 10,
                    outcome: 0,
                    post_only: false,
                    reduce_only: false,
                    leverage: None,
                    expires_at: None,
                    stp_mode: types::StpMode::default(),
                    trigger_price: None,
                    trigger_type: None,
                    display_qty: None,
                    min_fill_qty: None,
                    stp_group_id: None,
                    is_market_maker: false,
                };
                let result = rt.block_on(engine.submit_new_order(submit_cmd));
                let order_id = result.as_ref().map(|r| r.order_id.clone()).ok();

                // Step 2: Cancel order
                if let Some(oid) = order_id {
                    let cancel_cmd = CancelOrderCommand {
                        metadata: CommandMetadata::new(&format!("e2e-cr-cancel-{i}")),
                        user_id: "e2e-user".into(),
                        market_id: "BTC-USDT".into(),
                        outcome: Some(0),
                        order_id: oid,
                        client_order_id: None,
                    };
                    let _result: Result<CancelResult, _> =
                        rt.block_on(engine.cancel_order(cancel_cmd));
                }

                // Step 3: Resubmit with different price
                let replace_cmd = NewOrderCommand {
                    metadata: CommandMetadata::new(&format!("e2e-cr-replace-{i}")),
                    client_order_id: format!("e2e-cr-replace-{i}"),
                    user_id: "e2e-user".into(),
                    session_id: None,
                    market_id: "BTC-USDT".into(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price: Some(50_001),
                    amount: 10,
                    outcome: 0,
                    post_only: false,
                    reduce_only: false,
                    leverage: None,
                    expires_at: None,
                    stp_mode: types::StpMode::default(),
                    trigger_price: None,
                    trigger_type: None,
                    display_qty: None,
                    min_fill_qty: None,
                    stp_group_id: None,
                    is_market_maker: false,
                };
                let _ = rt.block_on(engine.submit_new_order(replace_cmd));
                total_us += cycle_start.elapsed().as_micros();
            }
            std::time::Duration::from_micros(total_us as u64)
        });
    });

    group.finish();
}

// ─────────────────────────────────────────────────────────────
// Criterion harness
// ─────────────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_sequencer_throughput,
    bench_ledger_operations,
    bench_orderbook_matching,
    bench_e2e_submit_order,
);
criterion_main!(benches);
