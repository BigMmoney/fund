use anyhow::anyhow;
use eventbus::EventBus;
use instruments::{InMemoryInstrumentRegistry, InstrumentRegistry};
use ledger::LedgerService;
use matching::{PartitionSnapshotRecord, PartitionedEngineConfig, PartitionedMatchingEngine};
use parking_lot::Mutex;
use persistence::{InMemoryWal, WalStore};
use risk::RiskEngine;
use std::sync::Arc;
use types::{
    Command, CommandLifecycle, CommandMetadata, InstrumentKind, InstrumentSpec, LedgerDelta,
    MarginMode, NewOrderCommand, OrderType, Side, TimeInForce,
};

fn main() {
    scenario_order_replay_after_crash_before_partition_apply();
    scenario_settlement_crash_during_match();
    scenario_journal_pre_append_failure();
    scenario_journal_post_append_recovery();
}

fn scenario_order_replay_after_crash_before_partition_apply() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let risk = seeded_risk();
        let snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        let trade_store = Arc::new(InMemoryTradeWal::default());

        let recovered = PartitionedMatchingEngine::with_stores_and_registry(
            config(),
            EventBus::new(),
            risk,
            benchmark_registry(),
            Some(snapshot_store),
            Some(trade_store),
        )
        .unwrap();

        recovered
            .replay_command(Command::NewOrder(with_command_seq(
                new_order(
                    "req-order-crash",
                    "bid-order-crash",
                    "maker-1",
                    Side::Buy,
                    100,
                    5,
                ),
                1,
            )))
            .await
            .unwrap();

        let snapshot = recovered
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();

        println!(
            "scenario=order_crash_before_partition_apply result=ok open_orders={} state={:?}",
            snapshot.open_orders, snapshot.state
        );
    });
}

fn scenario_settlement_crash_during_match() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let ledger = seeded_ledger_with_wal(Arc::new(FailingLedgerWal::new("settle_")));
        let risk = Arc::new(RiskEngine::new(ledger));
        let engine = PartitionedMatchingEngine::new_with_registry(
            config(),
            EventBus::new(),
            risk,
            benchmark_registry(),
        );

        engine
            .submit_new_order(new_order(
                "req-settle-1",
                "ask-settle-1",
                "maker-1",
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();

        let error = engine
            .submit_new_order(new_order(
                "req-settle-2",
                "bid-settle-1",
                "taker",
                Side::Buy,
                100,
                5,
            ))
            .await
            .unwrap_err();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();

        println!(
            "scenario=settlement_crash_during_match result=ok error={:?} open_orders={} state={:?}",
            error, snapshot.open_orders, snapshot.state
        );
    });
}

fn scenario_journal_pre_append_failure() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let risk = seeded_risk();
        let trade_store = Arc::new(FailingTradeWal::always_fail());
        let engine = PartitionedMatchingEngine::with_stores_and_registry(
            config(),
            EventBus::new(),
            risk,
            benchmark_registry(),
            None,
            Some(trade_store.clone()),
        )
        .unwrap();

        engine
            .submit_new_order(new_order("req-journal-pre-1", "ask-journal-pre", "maker-1", Side::Sell, 100, 5))
            .await
            .unwrap();

        let error = engine
            .submit_new_order(new_order("req-journal-pre-2", "bid-journal-pre", "taker", Side::Buy, 100, 5))
            .await
            .unwrap_err();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();

        println!(
            "scenario=journal_pre_append_failure result=ok error={:?} open_orders={} state={:?} trade_entries={}",
            error,
            snapshot.open_orders,
            snapshot.state,
            trade_store.entries().unwrap().len()
        );
    });
}

fn scenario_journal_post_append_recovery() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        let trade_store = Arc::new(InMemoryTradeWal::default());
        let risk = seeded_risk();

        let engine = PartitionedMatchingEngine::with_stores_and_registry(
            config(),
            EventBus::new(),
            risk.clone(),
            benchmark_registry(),
            Some(snapshot_store.clone()),
            Some(trade_store.clone()),
        )
        .unwrap();

        engine
            .submit_new_order(with_command_seq(
                new_order("req-journal-post-1", "ask-journal-post", "maker-1", Side::Sell, 100, 1),
                1,
            ))
            .await
            .unwrap();

        let stale_snapshot = snapshot_store.entries().unwrap().last().cloned().unwrap();

        engine
            .submit_new_order(with_command_seq(
                new_order("req-journal-post-2", "bid-journal-post", "taker", Side::Buy, 100, 1),
                2,
            ))
            .await
            .unwrap();

        let stale_snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        stale_snapshot_store.append(&stale_snapshot).unwrap();

        let recovered = PartitionedMatchingEngine::with_stores_and_registry(
            config(),
            EventBus::new(),
            risk,
            benchmark_registry(),
            Some(stale_snapshot_store),
            Some(trade_store.clone()),
        )
        .unwrap();

        recovered
            .replay_command(Command::NewOrder(with_command_seq(
                new_order("req-journal-post-2", "bid-journal-post", "taker", Side::Buy, 100, 1),
                2,
            )))
            .await
            .unwrap();

        let snapshot = recovered
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();

        println!(
            "scenario=journal_post_append_recovery result=ok open_orders={} state={:?} trade_entries={}",
            snapshot.open_orders,
            snapshot.state,
            trade_store.entries().unwrap().len()
        );
    });
}

#[derive(Default)]
struct FailingLedgerWal {
    entries: Mutex<Vec<LedgerDelta>>,
    fail_prefix: &'static str,
}

impl FailingLedgerWal {
    fn new(fail_prefix: &'static str) -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            fail_prefix,
        }
    }
}

impl WalStore<LedgerDelta> for FailingLedgerWal {
    fn append(&self, record: &LedgerDelta) -> anyhow::Result<()> {
        if record.op_id.starts_with(self.fail_prefix) {
            return Err(anyhow!("forced ledger wal failure for {}", record.op_id));
        }
        self.entries.lock().push(record.clone());
        Ok(())
    }

    fn entries(&self) -> anyhow::Result<Vec<LedgerDelta>> {
        Ok(self.entries.lock().clone())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TradeRecord {
    inner: matching::partitioned::TradeJournalRecord,
}

#[derive(Default)]
struct InMemoryTradeWal {
    entries: Mutex<Vec<matching::partitioned::TradeJournalRecord>>,
}

impl WalStore<matching::partitioned::TradeJournalRecord> for InMemoryTradeWal {
    fn append(&self, record: &matching::partitioned::TradeJournalRecord) -> anyhow::Result<()> {
        self.entries.lock().push(record.clone());
        Ok(())
    }

    fn entries(&self) -> anyhow::Result<Vec<matching::partitioned::TradeJournalRecord>> {
        Ok(self.entries.lock().clone())
    }
}

#[derive(Default)]
struct FailingTradeWal {
    entries: Mutex<Vec<matching::partitioned::TradeJournalRecord>>,
}

impl FailingTradeWal {
    fn always_fail() -> Self {
        Self::default()
    }
}

impl WalStore<matching::partitioned::TradeJournalRecord> for FailingTradeWal {
    fn append(&self, record: &matching::partitioned::TradeJournalRecord) -> anyhow::Result<()> {
        let _ = record;
        Err(anyhow!("forced trade journal failure"))
    }

    fn entries(&self) -> anyhow::Result<Vec<matching::partitioned::TradeJournalRecord>> {
        Ok(self.entries.lock().clone())
    }
}

fn config() -> PartitionedEngineConfig {
    PartitionedEngineConfig {
        partitions: 1,
        queue_capacity: 64,
        snapshot_interval_commands: 1,
        max_open_orders_per_user: 32,
        cancel_window: std::time::Duration::from_secs(30),
        max_cancel_to_new_ratio: 1.0,
        min_cancel_events_before_guard: 2,
        cancel_only_price_band_bps: 500,
        halt_price_band_bps: 1_000,
    }
}

fn new_order(
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

fn with_command_seq(mut command: NewOrderCommand, seq: u64) -> NewOrderCommand {
    command.metadata.command_seq = Some(seq);
    command.metadata.lifecycle = CommandLifecycle::WalAppended;
    command
}

fn seeded_ledger_with_wal(wal_store: Arc<dyn WalStore<LedgerDelta>>) -> Arc<LedgerService> {
    let ledger = Arc::new(LedgerService::with_wal_store(EventBus::new(), wal_store));
    for user in ["maker-1", "taker"] {
        ledger
            .process_deposit(user, 1_000_000, format!("deposit_{user}"))
            .unwrap();
        ledger
            .process_position_deposit(user, "btc-usdt", 0, 1_000, format!("position_{user}"))
            .unwrap();
    }
    ledger
}

fn seeded_risk() -> Arc<RiskEngine> {
    Arc::new(RiskEngine::new(seeded_ledger_with_wal(Arc::new(
        InMemoryWal::<LedgerDelta>::new(),
    ))))
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
