mod support;

use anyhow::Result as AnyhowResult;
use eventbus::EventBus;
use instruments::{InMemoryInstrumentRegistry, InstrumentRegistry};
use ledger::LedgerService;
use matching::partitioned::{
    PartitionSnapshotRecord, TradeJournalRecord, TradeSettlementRecord, TradeSettlementStatus,
};
use matching::{PartitionedEngineConfig, PartitionedMatchingEngine, SubmissionError};
use persistence::WalStore;
use risk::RiskEngine;
use sequencer::{SequencedCommandRecord, Sequencer};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use support::{
    default_test_markets, default_test_outcomes, default_test_users, file_wal, seed_test_ledger,
    test_config, FailOnAppendWal, TestHarness,
};
use tempfile::TempDir;
use types::{
    AdminAction, AdminCommand, Command, CommandLifecycle, CommandMetadata, MarketState,
    NewOrderCommand, OrderType, Side, StpMode, TimeInForce,
};

struct TestPaths {
    ledger: PathBuf,
    snapshot: PathBuf,
    trade: PathBuf,
    settlement: PathBuf,
    sequencer: PathBuf,
}

fn make_paths(root: &TempDir) -> TestPaths {
    TestPaths {
        ledger: root.path().join("ledger.wal.jsonl"),
        snapshot: root.path().join("matching.snapshot.jsonl"),
        trade: root.path().join("trade_journal.wal.jsonl"),
        settlement: root.path().join("trade_settlement.wal.jsonl"),
        sequencer: root.path().join("sequencer.wal.jsonl"),
    }
}

struct FileStack {
    ledger: Arc<LedgerService>,
    engine: PartitionedMatchingEngine,
}

fn base_limit_order(
    request_id: impl Into<String>,
    client_order_id: impl Into<String>,
    user_id: impl Into<String>,
    market_id: impl Into<String>,
    outcome: i32,
    side: Side,
    price: i64,
    amount: i64,
) -> NewOrderCommand {
    NewOrderCommand {
        metadata: CommandMetadata::new(request_id),
        client_order_id: client_order_id.into(),
        user_id: user_id.into(),
        session_id: None,
        market_id: market_id.into(),
        side,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: Some(price),
        amount,
        outcome,
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

fn build_file_stack(
    config: PartitionedEngineConfig,
    paths: &TestPaths,
    snapshot_store: Arc<dyn WalStore<PartitionSnapshotRecord>>,
    trade_store: Arc<dyn WalStore<TradeJournalRecord>>,
    settlement_store: Arc<dyn WalStore<TradeSettlementRecord>>,
) -> AnyhowResult<FileStack> {
    let ledger_store = file_wal(paths.ledger.clone())?;
    let had_ledger_entries = !ledger_store.entries()?.is_empty();
    let ledger = Arc::new(LedgerService::with_wal_store(EventBus::new(), ledger_store));
    if had_ledger_entries {
        ledger.recover_from_wal()?;
    } else {
        seed_test_ledger(
            &ledger,
            default_test_users(),
            default_test_markets(),
            default_test_outcomes(),
        )?;
    }

    let risk = Arc::new(RiskEngine::new(ledger.clone()));
    let registry = Arc::new(InMemoryInstrumentRegistry::new());
    let registry_trait: Arc<dyn InstrumentRegistry> = registry;
    let engine = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
        config,
        EventBus::new(),
        risk,
        registry_trait,
        Some(snapshot_store),
        Some(trade_store),
        None,
        Some(settlement_store),
    )?;

    Ok(FileStack { ledger, engine })
}

fn file_trade_entries(path: &Path) -> Vec<TradeJournalRecord> {
    file_wal::<TradeJournalRecord>(path.to_path_buf())
        .unwrap()
        .entries()
        .unwrap()
}

fn file_settlement_entries(path: &Path) -> Vec<TradeSettlementRecord> {
    file_wal::<TradeSettlementRecord>(path.to_path_buf())
        .unwrap()
        .entries()
        .unwrap()
}

#[tokio::test]
async fn file_backed_trade_journal_failure_rolls_back_cleanly() {
    let root = tempfile::tempdir().unwrap();
    let paths = make_paths(&root);
    let config = test_config();

    let snapshot_store = file_wal::<PartitionSnapshotRecord>(paths.snapshot.clone()).unwrap();
    let trade_store_real = file_wal::<TradeJournalRecord>(paths.trade.clone()).unwrap();
    let trade_store: Arc<dyn WalStore<TradeJournalRecord>> = Arc::new(FailOnAppendWal::new(
        trade_store_real.clone(),
        "trade_journal",
        [1],
    ));
    let settlement_store = file_wal::<TradeSettlementRecord>(paths.settlement.clone()).unwrap();

    let stack = build_file_stack(
        config.clone(),
        &paths,
        snapshot_store,
        trade_store,
        settlement_store,
    )
    .unwrap();

    stack
        .engine
        .submit_new_order(base_limit_order(
            "fj-ask-1",
            "fj-ask-1",
            "maker-a",
            "btc-usdt",
            0,
            Side::Sell,
            100,
            5,
        ))
        .await
        .unwrap();
    stack.engine.flush_all_snapshots().await.unwrap();

    let error = stack
        .engine
        .submit_new_order(base_limit_order(
            "fj-bid-1",
            "fj-bid-1",
            "taker",
            "btc-usdt",
            0,
            Side::Buy,
            100,
            5,
        ))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        SubmissionError::Persistence {
            component: "trade_journal",
            ..
        }
    ));
    let snapshot = stack
        .engine
        .snapshot_market("btc-usdt", 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.open_orders, 1);
    assert_eq!(stack.ledger.cash_available_balance("maker-a"), 1_000_000);
    assert_eq!(
        stack
            .ledger
            .position_available_balance("taker", "btc-usdt", 0),
        500
    );
    assert!(file_trade_entries(&paths.trade).is_empty());

    let restart_stack = build_file_stack(
        config,
        &paths,
        file_wal(paths.snapshot.clone()).unwrap(),
        file_wal(paths.trade.clone()).unwrap(),
        file_wal(paths.settlement.clone()).unwrap(),
    )
    .unwrap();
    let snapshot = restart_stack
        .engine
        .snapshot_market("btc-usdt", 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.open_orders, 1);
    restart_stack.ledger.verify_global_invariant().unwrap();
}

#[tokio::test]
async fn file_backed_settlement_wal_failure_aborts_before_ledger_commit() {
    let root = tempfile::tempdir().unwrap();
    let paths = make_paths(&root);
    let config = test_config();

    let snapshot_store = file_wal::<PartitionSnapshotRecord>(paths.snapshot.clone()).unwrap();
    let trade_store = file_wal::<TradeJournalRecord>(paths.trade.clone()).unwrap();
    let settlement_store_real =
        file_wal::<TradeSettlementRecord>(paths.settlement.clone()).unwrap();
    let settlement_store: Arc<dyn WalStore<TradeSettlementRecord>> = Arc::new(
        FailOnAppendWal::new(settlement_store_real.clone(), "settlement", [1]),
    );

    let stack = build_file_stack(
        config.clone(),
        &paths,
        snapshot_store,
        trade_store,
        settlement_store,
    )
    .unwrap();

    stack
        .engine
        .submit_new_order(base_limit_order(
            "fs-ask-1",
            "fs-ask-1",
            "maker-a",
            "btc-usdt",
            0,
            Side::Sell,
            100,
            5,
        ))
        .await
        .unwrap();
    stack.engine.flush_all_snapshots().await.unwrap();

    let error = stack
        .engine
        .submit_new_order(base_limit_order(
            "fs-bid-1",
            "fs-bid-1",
            "taker",
            "btc-usdt",
            0,
            Side::Buy,
            100,
            5,
        ))
        .await
        .unwrap_err();

    assert!(matches!(error, SubmissionError::Persistence { .. }));
    let snapshot = stack
        .engine
        .snapshot_market("btc-usdt", 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.open_orders, 1);
    assert_eq!(stack.ledger.cash_available_balance("maker-a"), 1_000_000);
    assert!(file_settlement_entries(&paths.settlement).is_empty());

    let restart_stack = build_file_stack(
        config,
        &paths,
        file_wal(paths.snapshot.clone()).unwrap(),
        file_wal(paths.trade.clone()).unwrap(),
        file_wal(paths.settlement.clone()).unwrap(),
    )
    .unwrap();
    let snapshot = restart_stack
        .engine
        .snapshot_market("btc-usdt", 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.open_orders, 1);
}

#[tokio::test]
async fn file_backed_prepared_without_applied_recovers_exactly_once() {
    let root = tempfile::tempdir().unwrap();
    let paths = make_paths(&root);
    let mut config = test_config();
    config.snapshot_interval_commands = 10_000;

    let stack = build_file_stack(
        config.clone(),
        &paths,
        file_wal(paths.snapshot.clone()).unwrap(),
        file_wal(paths.trade.clone()).unwrap(),
        file_wal(paths.settlement.clone()).unwrap(),
    )
    .unwrap();

    let mut ask = base_limit_order(
        "seq-1",
        "prep-ask-1",
        "maker-a",
        "btc-usdt",
        0,
        Side::Sell,
        100,
        1,
    );
    ask.metadata.command_seq = Some(1);
    stack.engine.submit_new_order(ask).await.unwrap();
    stack.engine.flush_all_snapshots().await.unwrap();

    let partition = stack
        .engine
        .partitions_for_command(&Command::NewOrder(base_limit_order(
            "probe",
            "probe",
            "maker-a",
            "btc-usdt",
            0,
            Side::Buy,
            100,
            1,
        )))[0];
    let trade_id = format!("trade:seq-2:{partition}:0");
    let settlement_file = file_wal::<TradeSettlementRecord>(paths.settlement.clone()).unwrap();
    settlement_file
        .append(&TradeSettlementRecord {
            partition_id: partition,
            trade_id: trade_id.clone(),
            market_id: "btc-usdt".to_string(),
            outcome: 0,
            instrument_kind: types::InstrumentKind::Spot,
            buy_order_id: "prep-bid-1".to_string(),
            buy_user_id: "taker".to_string(),
            sell_order_id: "prep-ask-1".to_string(),
            sell_user_id: "maker-a".to_string(),
            price: 100,
            amount: 1,
            settle_op_id: format!("trade-settle:{trade_id}"),
            rollback_op_id: format!("trade-rollback:{trade_id}"),
            status: TradeSettlementStatus::Prepared,
            recorded_at: chrono::Utc::now(),
        })
        .unwrap();
    let restart_stack = build_file_stack(
        config,
        &paths,
        file_wal(paths.snapshot.clone()).unwrap(),
        file_wal(paths.trade.clone()).unwrap(),
        file_wal(paths.settlement.clone()).unwrap(),
    )
    .unwrap();
    let stale = restart_stack
        .engine
        .snapshot_market("btc-usdt", 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stale.open_orders, 1);

    let mut buy = base_limit_order(
        "seq-2",
        "prep-bid-1",
        "taker",
        "btc-usdt",
        0,
        Side::Buy,
        100,
        1,
    );
    buy.metadata.command_seq = Some(2);
    restart_stack
        .engine
        .replay_command(Command::NewOrder(buy))
        .await
        .unwrap();

    let trade_entries = file_trade_entries(&paths.trade);
    assert_eq!(trade_entries.len(), 1);
    let settlement_entries = file_settlement_entries(&paths.settlement);
    assert!(settlement_entries.iter().any(|entry| {
        entry.trade_id == trade_id && entry.status == TradeSettlementStatus::Prepared
    }));
    assert!(settlement_entries.iter().any(|entry| {
        entry.trade_id == trade_id && entry.status == TradeSettlementStatus::Applied
    }));
    let snapshot = restart_stack
        .engine
        .snapshot_market("btc-usdt", 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.open_orders, 0);
    restart_stack.ledger.verify_global_invariant().unwrap();
}

#[tokio::test]
async fn sequencer_recovery_replays_commands_above_snapshot_floor_once() {
    let root = tempfile::tempdir().unwrap();
    let paths = make_paths(&root);
    let seq_wal = file_wal::<SequencedCommandRecord>(paths.sequencer.clone()).unwrap();
    let sequencer = Sequencer::with_wal(1, seq_wal.clone());

    let mut config = test_config();
    config.snapshot_interval_commands = 10_000;
    let harness = TestHarness::file_backed(config).unwrap();

    let ask1 = sequencer
        .sequence_and_append(Command::NewOrder(harness.limit_order(
            "pipe-ask-1",
            "pipe-ask-1",
            "maker-a",
            "btc-usdt",
            0,
            Side::Sell,
            100,
            2,
        )))
        .unwrap();
    let req1 = ask1.request_id().to_string();
    sequencer.mark_risk_reserved(&req1).unwrap();
    sequencer.mark_routed(&req1).unwrap();
    harness.engine.replay_command(ask1).await.unwrap();
    sequencer.mark_partition_accepted(&req1).unwrap();
    sequencer.mark_executed(&req1).unwrap();
    sequencer.mark_completed(&req1).unwrap();

    let ask2 = sequencer
        .sequence_and_append(Command::NewOrder(harness.limit_order(
            "pipe-ask-2",
            "pipe-ask-2",
            "maker-b",
            "eth-usdt",
            0,
            Side::Sell,
            210,
            3,
        )))
        .unwrap();
    let req2 = ask2.request_id().to_string();
    sequencer.mark_risk_reserved(&req2).unwrap();
    sequencer.mark_routed(&req2).unwrap();
    harness.engine.replay_command(ask2).await.unwrap();
    sequencer.mark_partition_accepted(&req2).unwrap();
    sequencer.mark_executed(&req2).unwrap();
    sequencer.mark_completed(&req2).unwrap();

    harness.engine.flush_all_snapshots().await.unwrap();

    let buy3 = sequencer
        .sequence_and_append(Command::NewOrder(harness.limit_order(
            "pipe-bid-3",
            "pipe-bid-3",
            "taker",
            "btc-usdt",
            0,
            Side::Buy,
            100,
            2,
        )))
        .unwrap();
    let req3 = buy3.request_id().to_string();
    sequencer.mark_risk_reserved(&req3).unwrap();
    sequencer.mark_routed(&req3).unwrap();
    harness.engine.replay_command(buy3.clone()).await.unwrap();
    sequencer.mark_partition_accepted(&req3).unwrap();
    sequencer.mark_executed(&req3).unwrap();
    sequencer.mark_settled(&req3).unwrap();
    sequencer.mark_completed(&req3).unwrap();

    let admin4 = sequencer
        .sequence_and_append(Command::Admin(AdminCommand {
            metadata: CommandMetadata::new("pipe-admin-4"),
            actor_id: "admin".to_string(),
            action: AdminAction::MarketKillSwitch {
                market_id: "eth-usdt".to_string(),
                enabled: true,
            },
        }))
        .unwrap();
    let req4 = admin4.request_id().to_string();
    sequencer.mark_routed(&req4).unwrap();
    harness.engine.replay_command(admin4.clone()).await.unwrap();
    sequencer.mark_partition_accepted(&req4).unwrap();
    sequencer.mark_completed(&req4).unwrap();

    let pre_restart_trades = harness.trade_records().len();
    let restarted = harness.restart().await.unwrap();
    let floor = restarted
        .engine
        .global_replay_floor_command_seq()
        .await
        .unwrap();
    assert_eq!(floor, Some(1));

    let recovered = Sequencer::with_wal(1, seq_wal);
    recovered.recover_from_wal().unwrap();
    let replayable: Vec<_> = recovered
        .latest_records()
        .into_iter()
        .filter(|record| record.command_seq > floor.unwrap())
        .map(|record| record.command)
        .collect();
    assert_eq!(replayable.len(), 3);

    for command in replayable {
        restarted.engine.replay_command(command).await.unwrap();
    }

    restarted.assert_core_invariants().await;
    let btc = restarted.market_snapshot("btc-usdt", 0).await.unwrap();
    let eth = restarted.market_snapshot("eth-usdt", 0).await.unwrap();
    assert_eq!(btc.open_orders, 0);
    assert_eq!(btc.last_trade_price, Some(100));
    assert_eq!(eth.state, MarketState::Halted);
    assert_eq!(restarted.trade_records().len(), pre_restart_trades);
    assert_eq!(
        recovered.metadata("pipe-admin-4").unwrap().lifecycle,
        CommandLifecycle::Completed
    );
}
