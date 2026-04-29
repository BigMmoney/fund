mod support;

use matching::partitioned::TradeSettlementStatus;
use proptest::prelude::*;
use support::{config_with_partitions, test_config, TestHarness};
use types::{Command, MarketState, Side, TriggerType};

#[tokio::test]
async fn replay_and_restart_recover_from_stale_snapshot_without_double_settlement() {
    let mut config = test_config();
    config.snapshot_interval_commands = 10_000;

    let mut harness = TestHarness::file_backed(config).unwrap();

    let ask_seq = harness.next_command_seq();
    let ask = TestHarness::with_command_seq(
        harness.limit_order(
            format!("seq-{ask_seq}"),
            "crash-ask-1",
            "maker-a",
            "btc-usdt",
            0,
            Side::Sell,
            100,
            5,
        ),
        ask_seq,
    );
    harness.engine.submit_new_order(ask).await.unwrap();
    harness.engine.flush_all_snapshots().await.unwrap();

    let stale_snapshot = harness.market_snapshot("btc-usdt", 0).await.unwrap();
    assert_eq!(stale_snapshot.open_orders, 1);

    let buy_seq = harness.next_command_seq();
    let buy = TestHarness::with_command_seq(
        harness.limit_order(
            format!("seq-{buy_seq}"),
            "crash-bid-1",
            "taker",
            "btc-usdt",
            0,
            Side::Buy,
            100,
            5,
        ),
        buy_seq,
    );
    harness.engine.submit_new_order(buy.clone()).await.unwrap();

    let trade_records = harness.trade_records();
    assert_eq!(trade_records.len(), 1);
    let trade_id = trade_records[0].trade_id.clone();

    let harness = harness.restart().await.unwrap();
    harness.assert_core_invariants().await;

    let recovered_snapshot = harness.market_snapshot("btc-usdt", 0).await.unwrap();
    assert_eq!(
        recovered_snapshot.open_orders, 1,
        "restart without a fresh snapshot should reload the stale book image"
    );

    harness
        .engine
        .replay_command(Command::NewOrder(buy))
        .await
        .unwrap();
    harness.assert_core_invariants().await;

    let final_snapshot = harness.market_snapshot("btc-usdt", 0).await.unwrap();
    assert_eq!(final_snapshot.open_orders, 0);
    assert_eq!(final_snapshot.last_trade_price, Some(100));
    assert_eq!(harness.trade_records().len(), 1);

    let settlement_records = harness.settlement_records();
    let prepared = settlement_records
        .iter()
        .filter(|record| {
            record.trade_id == trade_id && record.status == TradeSettlementStatus::Prepared
        })
        .count();
    let applied = settlement_records
        .iter()
        .filter(|record| {
            record.trade_id == trade_id && record.status == TradeSettlementStatus::Applied
        })
        .count();
    assert_eq!(prepared, 1);
    assert_eq!(applied, 1);

    assert_eq!(harness.ledger.cash_available_balance("maker-a"), 1_000_500);
    assert_eq!(
        harness
            .ledger
            .position_available_balance("maker-a", "btc-usdt", 0),
        495
    );
    assert_eq!(
        harness
            .ledger
            .position_available_balance("taker", "btc-usdt", 0),
        505
    );
}

#[tokio::test]
async fn duplicate_retry_and_replay_paths_stay_idempotent() {
    let mut harness = TestHarness::file_backed(test_config()).unwrap();

    let ask_seq = harness.next_command_seq();
    let ask = TestHarness::with_command_seq(
        harness.limit_order(
            format!("seq-{ask_seq}"),
            "dup-ask-1",
            "maker-a",
            "btc-usdt",
            0,
            Side::Sell,
            101,
            3,
        ),
        ask_seq,
    );
    harness.engine.submit_new_order(ask).await.unwrap();

    let duplicate_error = harness
        .engine
        .submit_new_order(harness.limit_order(
            "dup-open-order",
            "dup-ask-1",
            "maker-a",
            "btc-usdt",
            0,
            Side::Sell,
            101,
            3,
        ))
        .await
        .unwrap_err();
    assert!(
        duplicate_error.to_string().contains("duplicate order id"),
        "expected duplicate order rejection, got {duplicate_error}"
    );

    let buy_seq = harness.next_command_seq();
    let buy = TestHarness::with_command_seq(
        harness.limit_order(
            format!("seq-{buy_seq}"),
            "dup-bid-1",
            "taker",
            "btc-usdt",
            0,
            Side::Buy,
            101,
            3,
        ),
        buy_seq,
    );
    harness.engine.submit_new_order(buy.clone()).await.unwrap();
    harness.engine.flush_all_snapshots().await.unwrap();

    let trade_count = harness.trade_records().len();
    let settlement_count = harness.settlement_records().len();
    let maker_cash = harness.ledger.cash_available_balance("maker-a");
    let taker_position = harness
        .ledger
        .position_available_balance("taker", "btc-usdt", 0);

    let harness = harness.graceful_restart().await.unwrap();
    harness
        .engine
        .replay_command(Command::NewOrder(buy.clone()))
        .await
        .unwrap();
    harness
        .engine
        .replay_command(Command::NewOrder(buy))
        .await
        .unwrap();

    harness.assert_core_invariants().await;
    assert_eq!(harness.trade_records().len(), trade_count);
    assert_eq!(harness.settlement_records().len(), settlement_count);
    assert_eq!(harness.ledger.cash_available_balance("maker-a"), maker_cash);
    assert_eq!(
        harness
            .ledger
            .position_available_balance("taker", "btc-usdt", 0),
        taker_position
    );
}

#[tokio::test]
async fn multi_market_multi_partition_isolation_and_market_scoped_controls_work() {
    let mut harness = TestHarness::in_memory(config_with_partitions(4)).unwrap();
    let distinct = harness.markets_on_distinct_partitions(2);
    assert_eq!(distinct.len(), 2);

    let (market_a, partition_a) = distinct[0].clone();
    let (market_b, partition_b) = distinct[1].clone();
    assert_ne!(partition_a, partition_b);

    harness.seed_spot_market(&market_a, &[0]).unwrap();
    harness.seed_spot_market(&market_b, &[0]).unwrap();

    let iso_a_req = harness.request_id("iso-a");
    let iso_a_cmd = harness.limit_order(
        iso_a_req,
        "iso-a-ask-1",
        "maker-a",
        market_a.clone(),
        0,
        Side::Sell,
        100,
        2,
    );
    let ask_a = harness.engine.submit_new_order(iso_a_cmd).await.unwrap();
    let iso_b_req = harness.request_id("iso-b");
    let iso_b_cmd = harness.limit_order(
        iso_b_req,
        "iso-b-ask-1",
        "maker-b",
        market_b.clone(),
        0,
        Side::Sell,
        110,
        2,
    );
    let ask_b = harness.engine.submit_new_order(iso_b_cmd).await.unwrap();

    assert_eq!(ask_a.partition, partition_a);
    assert_eq!(ask_b.partition, partition_b);

    let iso_cross_req = harness.request_id("iso-cross");
    let iso_cross_cmd = harness.limit_order(
        iso_cross_req,
        "iso-a-bid-1",
        "taker",
        market_a.clone(),
        0,
        Side::Buy,
        100,
        2,
    );
    harness
        .engine
        .submit_new_order(iso_cross_cmd)
        .await
        .unwrap();

    let snapshot_a = harness.market_snapshot(&market_a, 0).await.unwrap();
    let snapshot_b = harness.market_snapshot(&market_b, 0).await.unwrap();
    assert_eq!(snapshot_a.open_orders, 0);
    assert_eq!(snapshot_b.open_orders, 1);

    let kill_req = harness.request_id("kill-a");
    let kill_cmd = harness.market_kill_switch_command(kill_req, market_a.clone(), true);
    harness.engine.submit_admin(kill_cmd).await.unwrap();

    let snapshot_a = harness.market_snapshot(&market_a, 0).await.unwrap();
    let snapshot_b = harness.market_snapshot(&market_b, 0).await.unwrap();
    assert_eq!(snapshot_a.state, MarketState::Halted);
    assert_eq!(snapshot_b.state, MarketState::Normal);

    let mass_req = harness.request_id("mass-b");
    let mass_cmd = harness.mass_cancel_by_market_command(mass_req, market_b.clone());
    harness
        .engine
        .mass_cancel_by_market(mass_cmd)
        .await
        .unwrap();

    let snapshot_b = harness.market_snapshot(&market_b, 0).await.unwrap();
    assert_eq!(snapshot_b.open_orders, 0);
    harness.assert_core_invariants().await;
}

#[tokio::test]
async fn risk_ledger_and_matching_stay_aligned_for_spot_and_margin_flows() {
    let mut harness = TestHarness::in_memory(test_config()).unwrap();

    let spot_ask_req = harness.request_id("spot-ask");
    let spot_ask_cmd = harness.limit_order(
        spot_ask_req,
        "spot-ask-1",
        "maker-a",
        "btc-usdt",
        0,
        Side::Sell,
        100,
        4,
    );
    harness.engine.submit_new_order(spot_ask_cmd).await.unwrap();
    let spot_bid_req = harness.request_id("spot-bid");
    let spot_bid_cmd = harness.limit_order(
        spot_bid_req,
        "spot-bid-1",
        "taker",
        "btc-usdt",
        0,
        Side::Buy,
        100,
        4,
    );
    harness.engine.submit_new_order(spot_bid_cmd).await.unwrap();

    assert_eq!(harness.ledger.cash_available_balance("maker-a"), 1_000_400);
    assert_eq!(
        harness
            .ledger
            .position_available_balance("maker-a", "btc-usdt", 0),
        496
    );
    assert_eq!(
        harness
            .ledger
            .position_available_balance("taker", "btc-usdt", 0),
        504
    );

    let margin_ask_req = harness.request_id("margin-ask");
    let margin_ask_cmd = harness.leveraged_limit_order(
        margin_ask_req,
        "margin-ask-1",
        "maker-b",
        "margin:btc-usdt",
        0,
        Side::Sell,
        110,
        7,
        5,
    );
    harness
        .engine
        .submit_new_order(margin_ask_cmd)
        .await
        .unwrap();
    let margin_bid_req = harness.request_id("margin-bid");
    let margin_bid_cmd = harness.leveraged_limit_order(
        margin_bid_req,
        "margin-bid-1",
        "alice",
        "margin:btc-usdt",
        0,
        Side::Buy,
        110,
        7,
        5,
    );
    harness
        .engine
        .submit_new_order(margin_bid_cmd)
        .await
        .unwrap();

    assert_eq!(
        harness
            .ledger
            .derivative_position_balance("maker-b", "margin:btc-usdt", 0),
        -7
    );
    assert_eq!(
        harness
            .ledger
            .derivative_position_balance("alice", "margin:btc-usdt", 0),
        7
    );
    assert_eq!(harness.ledger.open_interest("margin:btc-usdt", 0), 7);

    let instrument = harness.engine.resolve_instrument("margin:btc-usdt");
    let snapshot = harness
        .risk
        .margin_snapshot(
            "alice",
            &instrument,
            0,
            110,
            Some(5),
            instrument.maintenance_margin_bps,
        )
        .unwrap();
    assert_eq!(snapshot.position_qty, 7);
    assert!(!snapshot.liquidation_required);

    harness.assert_core_invariants().await;
}

#[tokio::test]
async fn steady_state_restart_loop_preserves_books_balances_and_trade_history() {
    let mut config = test_config();
    config.snapshot_interval_commands = 4;
    let mut harness = TestHarness::file_backed(config).unwrap();

    let spot_markets = vec!["btc-usdt".to_string(), "eth-usdt".to_string()];
    let derivative_market = "margin:btc-usdt".to_string();

    for step in 0..72usize {
        match step % 6 {
            0 => {
                let market = spot_markets[step % spot_markets.len()].clone();
                let request_id = harness.request_id("loop-ask");
                let command = harness.limit_order(
                    request_id,
                    format!("loop-ask-{step}"),
                    "maker-a",
                    market,
                    0,
                    Side::Sell,
                    100 + (step % 5) as i64,
                    2 + (step % 3) as i64,
                );
                harness.engine.submit_new_order(command).await.unwrap();
            }
            1 => {
                let market = spot_markets[step % spot_markets.len()].clone();
                let request_id = harness.request_id("loop-take");
                let command = harness.market_order(
                    request_id,
                    format!("loop-take-{step}"),
                    "taker",
                    market,
                    0,
                    Side::Buy,
                    2,
                );
                harness.engine.submit_new_order(command).await.unwrap();
            }
            2 => {
                let request_id = harness.request_id("loop-margin-ask");
                let command = harness.leveraged_limit_order(
                    request_id,
                    format!("loop-margin-ask-{step}"),
                    "maker-b",
                    derivative_market.clone(),
                    0,
                    Side::Sell,
                    105 + (step % 4) as i64,
                    3,
                    4,
                );
                harness.engine.submit_new_order(command).await.unwrap();
            }
            3 => {
                let request_id = harness.request_id("loop-margin-bid");
                let command = harness.leveraged_limit_order(
                    request_id,
                    format!("loop-margin-bid-{step}"),
                    "alice",
                    derivative_market.clone(),
                    0,
                    Side::Buy,
                    106 + (step % 4) as i64,
                    3,
                    4,
                );
                harness.engine.submit_new_order(command).await.unwrap();
            }
            4 => {
                let open_orders = harness.all_open_orders().await;
                if let Some(order) = open_orders.first() {
                    let request_id = harness.request_id("loop-cancel");
                    let command = harness.cancel_command(
                        request_id,
                        order.user_id.clone(),
                        order.market_id.clone(),
                        Some(order.outcome),
                        order.order_id.clone(),
                    );
                    harness.engine.cancel_order(command).await.unwrap();
                }
            }
            _ => {
                let open_orders = harness.all_open_orders().await;
                if let Some(order) = open_orders.first() {
                    let request_id = harness.request_id("loop-replace");
                    let command = harness.replace_command(
                        request_id,
                        order.user_id.clone(),
                        order.market_id.clone(),
                        Some(order.outcome),
                        order.order_id.clone(),
                        format!("{}-r", order.order_id),
                        order.price + 1,
                        2,
                    );
                    harness.engine.replace_order(command).await.unwrap();
                }
            }
        }

        harness.assert_core_invariants().await;

        if step > 0 && step % 12 == 0 {
            harness = harness.graceful_restart().await.unwrap();
            harness.assert_core_invariants().await;
        }
    }

    let trade_count = harness.trade_records().len();
    let open_orders_before = harness.all_open_orders().await.len();
    let maker_cash_before = harness.ledger.cash_available_balance("maker-a");

    let harness = harness.graceful_restart().await.unwrap();
    harness.assert_core_invariants().await;

    assert!(trade_count > 0);
    assert_eq!(harness.trade_records().len(), trade_count);
    assert_eq!(harness.all_open_orders().await.len(), open_orders_before);
    assert_eq!(
        harness.ledger.cash_available_balance("maker-a"),
        maker_cash_before
    );
}

#[derive(Debug, Clone)]
enum GeneratedOp {
    PlaceBid {
        market: u8,
        user: u8,
        price: i64,
        amount: i64,
    },
    PlaceAsk {
        market: u8,
        user: u8,
        price: i64,
        amount: i64,
    },
    MarketTake {
        market: u8,
        user: u8,
        side: Side,
        amount: i64,
    },
    StopSell {
        market: u8,
        user: u8,
        trigger_price: i64,
        amount: i64,
    },
    CancelAny {
        slot: u8,
    },
    ReplaceAny {
        slot: u8,
        price: i64,
        amount: i64,
    },
    Restart,
}

fn generated_ops_strategy() -> impl Strategy<Value = Vec<GeneratedOp>> {
    prop::collection::vec(
        prop_oneof![
            (0u8..3, 0u8..4, 95i64..115, 1i64..8).prop_map(|(market, user, price, amount)| {
                GeneratedOp::PlaceBid {
                    market,
                    user,
                    price,
                    amount,
                }
            }),
            (0u8..3, 0u8..4, 95i64..115, 1i64..8).prop_map(|(market, user, price, amount)| {
                GeneratedOp::PlaceAsk {
                    market,
                    user,
                    price,
                    amount,
                }
            }),
            (
                0u8..3,
                0u8..4,
                prop_oneof![Just(Side::Buy), Just(Side::Sell)],
                1i64..6
            )
                .prop_map(|(market, user, side, amount)| GeneratedOp::MarketTake {
                    market,
                    user,
                    side,
                    amount,
                }),
            (0u8..2, 0u8..4, 90i64..120, 1i64..4).prop_map(
                |(market, user, trigger_price, amount)| GeneratedOp::StopSell {
                    market,
                    user,
                    trigger_price,
                    amount,
                }
            ),
            (0u8..24).prop_map(|slot| GeneratedOp::CancelAny { slot }),
            (0u8..24, 94i64..118, 1i64..8).prop_map(|(slot, price, amount)| {
                GeneratedOp::ReplaceAny {
                    slot,
                    price,
                    amount,
                }
            }),
            Just(GeneratedOp::Restart),
        ],
        24..56,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn generated_operation_stream_preserves_matching_invariants(ops in generated_ops_strategy()) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async move {
            let mut config = test_config();
            config.snapshot_interval_commands = 6;
            let mut harness = TestHarness::in_memory(config).unwrap();
            let markets = ["btc-usdt", "eth-usdt", "margin:btc-usdt"];
            let users = ["maker-a", "maker-b", "taker", "alice"];

            for (index, op) in ops.into_iter().enumerate() {
                match op {
                    GeneratedOp::PlaceBid { market, user, price, amount } => {
                        let market_id = markets[market as usize % markets.len()];
                        let user_id = users[user as usize % users.len()];
                        if market_id.starts_with("margin:") {
                            let request_id = harness.request_id("prop-bid");
                            let command = harness.leveraged_limit_order(
                                request_id,
                                format!("prop-bid-{index}"),
                                user_id,
                                market_id,
                                0,
                                Side::Buy,
                                price,
                                amount,
                                3,
                            );
                            let _ = harness.engine.submit_new_order(command).await;
                        } else {
                            let request_id = harness.request_id("prop-bid");
                            let command = harness.limit_order(
                                request_id,
                                format!("prop-bid-{index}"),
                                user_id,
                                market_id,
                                0,
                                Side::Buy,
                                price,
                                amount,
                            );
                            let _ = harness.engine.submit_new_order(command).await;
                        }
                    }
                    GeneratedOp::PlaceAsk { market, user, price, amount } => {
                        let market_id = markets[market as usize % markets.len()];
                        let user_id = users[user as usize % users.len()];
                        if market_id.starts_with("margin:") {
                            let request_id = harness.request_id("prop-ask");
                            let command = harness.leveraged_limit_order(
                                request_id,
                                format!("prop-ask-{index}"),
                                user_id,
                                market_id,
                                0,
                                Side::Sell,
                                price,
                                amount,
                                3,
                            );
                            let _ = harness.engine.submit_new_order(command).await;
                        } else {
                            let request_id = harness.request_id("prop-ask");
                            let command = harness.limit_order(
                                request_id,
                                format!("prop-ask-{index}"),
                                user_id,
                                market_id,
                                0,
                                Side::Sell,
                                price,
                                amount,
                            );
                            let _ = harness.engine.submit_new_order(command).await;
                        }
                    }
                    GeneratedOp::MarketTake { market, user, side, amount } => {
                        let market_id = markets[market as usize % markets.len()];
                        let user_id = users[user as usize % users.len()];
                        let request_id = harness.request_id("prop-market");
                        let command = harness.market_order(
                            request_id,
                            format!("prop-market-{index}"),
                            user_id,
                            market_id,
                            0,
                            side,
                            amount,
                        );
                        let _ = harness.engine.submit_new_order(command).await;
                    }
                    GeneratedOp::StopSell { market, user, trigger_price, amount } => {
                        let market_id = markets[market as usize % 2];
                        let user_id = users[user as usize % users.len()];
                        let request_id = harness.request_id("prop-stop");
                        let command = harness.stop_order(
                            request_id,
                            format!("prop-stop-{index}"),
                            user_id,
                            market_id,
                            0,
                            Side::Sell,
                            trigger_price,
                            amount,
                            TriggerType::LastPrice,
                        );
                        let _ = harness.engine.submit_new_order(command).await;
                    }
                    GeneratedOp::CancelAny { slot } => {
                        let open_orders = harness.all_open_orders().await;
                        if !open_orders.is_empty() {
                            let order = &open_orders[slot as usize % open_orders.len()];
                            let request_id = harness.request_id("prop-cancel");
                            let command = harness.cancel_command(
                                request_id,
                                order.user_id.clone(),
                                order.market_id.clone(),
                                Some(order.outcome),
                                order.order_id.clone(),
                            );
                            let _ = harness.engine.cancel_order(command).await;
                        }
                    }
                    GeneratedOp::ReplaceAny { slot, price, amount } => {
                        let open_orders = harness.all_open_orders().await;
                        if !open_orders.is_empty() {
                            let order = &open_orders[slot as usize % open_orders.len()];
                            let request_id = harness.request_id("prop-replace");
                            let command = harness.replace_command(
                                request_id,
                                order.user_id.clone(),
                                order.market_id.clone(),
                                Some(order.outcome),
                                order.order_id.clone(),
                                format!("{}-r-{index}", order.order_id),
                                price,
                                amount,
                            );
                            let _ = harness.engine.replace_order(command).await;
                        }
                    }
                    GeneratedOp::Restart => {
                        harness = harness.graceful_restart().await.unwrap();
                    }
                }

                harness.assert_core_invariants().await;
            }
        });
    }
}
