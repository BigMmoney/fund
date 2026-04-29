#![allow(dead_code)]

use anyhow::Result as AnyhowResult;
use eventbus::EventBus;
use instruments::{InMemoryInstrumentRegistry, InstrumentRegistry};
use ledger::LedgerService;
use matching::{
    partitioned::{TradeJournalRecord, TradeSettlementRecord, TradeSettlementStatus},
    MarketSnapshot, PartitionSnapshotRecord, PartitionedEngineConfig, PartitionedMatchingEngine,
};
use persistence::{InMemoryWal, JsonlFileWal, WalStore};
use risk::RiskEngine;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tempfile::TempDir;
use types::{
    AdminAction, AdminCommand, CancelOrderCommand, Command, CommandMetadata, LedgerDelta,
    MarketState, MassCancelByMarketCommand, NewOrderCommand, OrderType, ReplaceOrderCommand, Side,
    StpMode, TimeInForce, TriggerType,
};

const DEFAULT_USERS: &[&str] = &["maker-a", "maker-b", "taker", "alice", "bob", "carol"];
const DEFAULT_SPOT_MARKETS: &[&str] = &["btc-usdt", "eth-usdt", "sol-usdt"];
const DEFAULT_OUTCOMES: &[i32] = &[0, 1];
const SEEDED_CASH: i64 = 1_000_000;
const SEEDED_POSITION: i64 = 500;

pub fn test_config() -> PartitionedEngineConfig {
    PartitionedEngineConfig {
        partitions: 4,
        queue_capacity: 256,
        snapshot_interval_commands: 8,
        max_open_orders_per_user: 128,
        cancel_window: std::time::Duration::from_secs(30),
        max_cancel_to_new_ratio: 4.0,
        min_cancel_events_before_guard: 3,
        cancel_only_price_band_bps: 500,
        halt_price_band_bps: 1_000,
        ..Default::default()
    }
}

pub fn config_with_partitions(partitions: usize) -> PartitionedEngineConfig {
    let mut config = test_config();
    config.partitions = partitions;
    config
}

enum StoreHandle<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    InMemory(Arc<dyn WalStore<T>>),
    File(PathBuf),
}

impl<T> StoreHandle<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn in_memory() -> Self {
        Self::InMemory(Arc::new(InMemoryWal::<T>::new()))
    }

    fn open(&self) -> AnyhowResult<Arc<dyn WalStore<T>>> {
        match self {
            Self::InMemory(store) => Ok(store.clone()),
            Self::File(path) => {
                let store: Arc<dyn WalStore<T>> = Arc::new(JsonlFileWal::<T>::new(path.clone())?);
                Ok(store)
            }
        }
    }
}

pub fn file_wal<T>(path: impl Into<PathBuf>) -> AnyhowResult<Arc<dyn WalStore<T>>>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    let store: Arc<dyn WalStore<T>> = Arc::new(JsonlFileWal::<T>::new(path.into())?);
    Ok(store)
}

pub struct FailOnAppendWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    inner: Arc<dyn WalStore<T>>,
    fail_on_appends: HashSet<u64>,
    append_count: AtomicU64,
    label: &'static str,
}

impl<T> FailOnAppendWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new(
        inner: Arc<dyn WalStore<T>>,
        label: &'static str,
        fail_on_appends: impl IntoIterator<Item = u64>,
    ) -> Self {
        Self {
            inner,
            fail_on_appends: fail_on_appends.into_iter().collect(),
            append_count: AtomicU64::new(0),
            label,
        }
    }
}

impl<T> WalStore<T> for FailOnAppendWal<T>
where
    T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn append(&self, record: &T) -> AnyhowResult<()> {
        let call_no = self.append_count.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_on_appends.contains(&call_no) {
            anyhow::bail!("forced {} append failure on call {}", self.label, call_no);
        }
        self.inner.append(record)
    }

    fn entries(&self) -> AnyhowResult<Vec<T>> {
        self.inner.entries()
    }

    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn sync(&self) -> AnyhowResult<()> {
        self.inner.sync()
    }
}

struct StoreBundle {
    _tempdir: Option<TempDir>,
    ledger: StoreHandle<LedgerDelta>,
    snapshot: StoreHandle<PartitionSnapshotRecord>,
    trade: StoreHandle<TradeJournalRecord>,
    settlement: StoreHandle<TradeSettlementRecord>,
}

impl StoreBundle {
    fn in_memory() -> Self {
        Self {
            _tempdir: None,
            ledger: StoreHandle::in_memory(),
            snapshot: StoreHandle::in_memory(),
            trade: StoreHandle::in_memory(),
            settlement: StoreHandle::in_memory(),
        }
    }

    fn file_backed() -> AnyhowResult<Self> {
        let root = tempfile::tempdir()?;
        let root_path = root.path().to_path_buf();
        Ok(Self {
            _tempdir: Some(root),
            ledger: StoreHandle::File(root_path.join("ledger.wal.jsonl")),
            snapshot: StoreHandle::File(root_path.join("matching.snapshot.jsonl")),
            trade: StoreHandle::File(root_path.join("trade_journal.wal.jsonl")),
            settlement: StoreHandle::File(root_path.join("trade_settlement.wal.jsonl")),
        })
    }
}

pub struct TestHarness {
    pub config: PartitionedEngineConfig,
    pub registry: Arc<InMemoryInstrumentRegistry>,
    pub ledger: Arc<LedgerService>,
    pub risk: Arc<RiskEngine>,
    pub engine: PartitionedMatchingEngine,
    stores: StoreBundle,
    next_sequence: u64,
}

impl TestHarness {
    pub fn in_memory(config: PartitionedEngineConfig) -> AnyhowResult<Self> {
        Self::build(config, StoreBundle::in_memory(), true)
    }

    pub fn file_backed(config: PartitionedEngineConfig) -> AnyhowResult<Self> {
        Self::build(config, StoreBundle::file_backed()?, true)
    }

    fn build(
        config: PartitionedEngineConfig,
        stores: StoreBundle,
        seed_defaults: bool,
    ) -> AnyhowResult<Self> {
        let registry = Arc::new(InMemoryInstrumentRegistry::new());
        let ledger_store = stores.ledger.open()?;
        let snapshot_store = stores.snapshot.open()?;
        let trade_store = stores.trade.open()?;
        let settlement_store = stores.settlement.open()?;
        let ledger = Arc::new(LedgerService::with_wal_store(EventBus::new(), ledger_store));
        let risk = Arc::new(RiskEngine::new(ledger.clone()));
        let registry_trait: Arc<dyn InstrumentRegistry> = registry.clone();
        let engine = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config.clone(),
            EventBus::new(),
            risk.clone(),
            registry_trait,
            Some(snapshot_store),
            Some(trade_store),
            None,
            Some(settlement_store),
        )?;

        let mut harness = Self {
            config,
            registry,
            ledger,
            risk,
            engine,
            stores,
            next_sequence: 0,
        };
        if seed_defaults {
            harness.seed_defaults()?;
        }
        Ok(harness)
    }

    fn seed_defaults(&mut self) -> AnyhowResult<()> {
        seed_test_ledger(
            &self.ledger,
            DEFAULT_USERS,
            DEFAULT_SPOT_MARKETS,
            DEFAULT_OUTCOMES,
        )?;
        self.ledger.verify_global_invariant()?;
        Ok(())
    }

    pub fn seed_spot_market(&self, market_id: &str, outcomes: &[i32]) -> AnyhowResult<()> {
        for user in DEFAULT_USERS {
            for outcome in outcomes {
                let op_id = format!("seed-pos-{user}-{market_id}-{outcome}");
                if self
                    .ledger
                    .position_available_balance(user, market_id, *outcome)
                    == 0
                {
                    self.ledger.process_position_deposit(
                        user,
                        market_id,
                        *outcome,
                        SEEDED_POSITION,
                        op_id,
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn next_command_seq(&mut self) -> u64 {
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.next_sequence
    }

    pub fn request_id(&mut self, prefix: &str) -> String {
        let seq = self.next_command_seq();
        format!("{prefix}-req-{seq}")
    }

    pub fn limit_order(
        &self,
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

    pub fn market_order(
        &self,
        request_id: impl Into<String>,
        client_order_id: impl Into<String>,
        user_id: impl Into<String>,
        market_id: impl Into<String>,
        outcome: i32,
        side: Side,
        amount: i64,
    ) -> NewOrderCommand {
        NewOrderCommand {
            metadata: CommandMetadata::new(request_id),
            client_order_id: client_order_id.into(),
            user_id: user_id.into(),
            session_id: None,
            market_id: market_id.into(),
            side,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Ioc,
            price: None,
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

    pub fn leveraged_limit_order(
        &self,
        request_id: impl Into<String>,
        client_order_id: impl Into<String>,
        user_id: impl Into<String>,
        market_id: impl Into<String>,
        outcome: i32,
        side: Side,
        price: i64,
        amount: i64,
        leverage: u32,
    ) -> NewOrderCommand {
        let mut command = self.limit_order(
            request_id,
            client_order_id,
            user_id,
            market_id,
            outcome,
            side,
            price,
            amount,
        );
        command.leverage = Some(leverage);
        command
    }

    pub fn stop_order(
        &self,
        request_id: impl Into<String>,
        client_order_id: impl Into<String>,
        user_id: impl Into<String>,
        market_id: impl Into<String>,
        outcome: i32,
        side: Side,
        trigger_price: i64,
        amount: i64,
        trigger_type: TriggerType,
    ) -> NewOrderCommand {
        let mut command = self.market_order(
            request_id,
            client_order_id,
            user_id,
            market_id,
            outcome,
            side,
            amount,
        );
        command.trigger_price = Some(trigger_price);
        command.trigger_type = Some(trigger_type);
        command
    }

    pub fn with_command_seq(mut command: NewOrderCommand, command_seq: u64) -> NewOrderCommand {
        command.metadata.command_seq = Some(command_seq);
        command
    }

    pub fn cancel_command(
        &self,
        request_id: impl Into<String>,
        user_id: impl Into<String>,
        market_id: impl Into<String>,
        outcome: Option<i32>,
        order_id: impl Into<String>,
    ) -> CancelOrderCommand {
        CancelOrderCommand {
            metadata: CommandMetadata::new(request_id),
            user_id: user_id.into(),
            market_id: market_id.into(),
            outcome,
            order_id: order_id.into(),
            client_order_id: None,
        }
    }

    pub fn replace_command(
        &self,
        request_id: impl Into<String>,
        user_id: impl Into<String>,
        market_id: impl Into<String>,
        outcome: Option<i32>,
        order_id: impl Into<String>,
        new_client_order_id: impl Into<String>,
        new_price: i64,
        new_amount: i64,
    ) -> ReplaceOrderCommand {
        ReplaceOrderCommand {
            metadata: CommandMetadata::new(request_id),
            user_id: user_id.into(),
            market_id: market_id.into(),
            outcome,
            order_id: order_id.into(),
            new_client_order_id: Some(new_client_order_id.into()),
            new_price: Some(new_price),
            new_amount: Some(new_amount),
            new_time_in_force: Some(TimeInForce::Gtc),
            post_only: Some(false),
            reduce_only: Some(false),
            new_leverage: None,
            new_expires_at: None,
            new_display_qty: None,
            new_min_fill_qty: None,
            new_trigger_price: None,
            new_trigger_type: None,
        }
    }

    pub fn market_kill_switch_command(
        &self,
        request_id: impl Into<String>,
        market_id: impl Into<String>,
        enabled: bool,
    ) -> AdminCommand {
        AdminCommand {
            metadata: CommandMetadata::new(request_id),
            actor_id: "admin".to_string(),
            action: AdminAction::MarketKillSwitch {
                market_id: market_id.into(),
                enabled,
            },
        }
    }

    pub fn mass_cancel_by_market_command(
        &self,
        request_id: impl Into<String>,
        market_id: impl Into<String>,
    ) -> MassCancelByMarketCommand {
        MassCancelByMarketCommand {
            metadata: CommandMetadata::new(request_id),
            market_id: market_id.into(),
            side: None,
        }
    }

    pub async fn graceful_restart(self) -> AnyhowResult<Self> {
        self.engine.flush_all_snapshots().await?;
        self.restart().await
    }

    pub async fn restart(self) -> AnyhowResult<Self> {
        let TestHarness {
            config,
            registry,
            stores,
            next_sequence,
            engine,
            ..
        } = self;
        drop(engine);
        tokio::task::yield_now().await;

        let ledger_store = stores.ledger.open()?;
        let snapshot_store = stores.snapshot.open()?;
        let trade_store = stores.trade.open()?;
        let settlement_store = stores.settlement.open()?;
        let ledger = Arc::new(LedgerService::with_wal_store(EventBus::new(), ledger_store));
        ledger.recover_from_wal()?;
        let risk = Arc::new(RiskEngine::new(ledger.clone()));
        let registry_trait: Arc<dyn InstrumentRegistry> = registry.clone();
        let engine = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config.clone(),
            EventBus::new(),
            risk.clone(),
            registry_trait,
            Some(snapshot_store),
            Some(trade_store),
            None,
            Some(settlement_store),
        )?;

        Ok(Self {
            config,
            registry,
            ledger,
            risk,
            engine,
            stores,
            next_sequence,
        })
    }

    pub fn trade_records(&self) -> Vec<TradeJournalRecord> {
        self.stores.trade.open().unwrap().entries().unwrap()
    }

    pub fn settlement_records(&self) -> Vec<TradeSettlementRecord> {
        self.stores.settlement.open().unwrap().entries().unwrap()
    }

    pub async fn market_snapshot(&self, market_id: &str, outcome: i32) -> Option<MarketSnapshot> {
        self.engine
            .snapshot_market(market_id, outcome)
            .await
            .unwrap()
    }

    pub async fn all_open_orders(&self) -> Vec<RestingOrderRef> {
        let mut orders = Vec::new();
        for market in self
            .engine
            .export_snapshots()
            .await
            .unwrap()
            .into_iter()
            .flat_map(|record| record.snapshot.markets.into_iter())
        {
            for order in market.orders {
                orders.push(RestingOrderRef {
                    user_id: order.user_id,
                    market_id: market.market_id.clone(),
                    outcome: market.outcome,
                    order_id: order.order_id,
                    price: order.price,
                });
            }
        }
        orders.sort_by(|lhs, rhs| lhs.order_id.cmp(&rhs.order_id));
        orders
    }

    pub fn markets_on_distinct_partitions(&self, count: usize) -> Vec<(String, usize)> {
        let mut found = HashMap::new();
        for idx in 0..10_000 {
            let market_id = format!("partition-market-{idx}");
            let command = Command::NewOrder(self.limit_order(
                "partition-probe",
                format!("partition-probe-{idx}"),
                "maker-a",
                market_id.clone(),
                0,
                Side::Buy,
                100,
                1,
            ));
            let partition = self.engine.partitions_for_command(&command)[0];
            found.entry(partition).or_insert(market_id);
            if found.len() >= count.min(self.config.partitions) {
                break;
            }
        }
        let mut markets: Vec<_> = found
            .into_iter()
            .map(|(partition, id)| (id, partition))
            .collect();
        markets.sort_by_key(|(_, partition)| *partition);
        markets.truncate(count.min(markets.len()));
        markets
    }

    pub async fn assert_core_invariants(&self) {
        self.ledger.verify_global_invariant().unwrap();

        let snapshots = self.engine.export_snapshots().await.unwrap();
        for market in snapshots
            .iter()
            .flat_map(|record| record.snapshot.markets.iter())
        {
            let mut seen_order_ids = HashSet::new();
            let mut best_bid = None;
            let mut best_ask = None;
            for order in &market.orders {
                assert!(
                    seen_order_ids.insert(order.order_id.clone()),
                    "duplicate resting order id in snapshot: {}",
                    order.order_id
                );
                assert!(
                    order.remaining_amount > 0,
                    "remaining amount must stay positive"
                );
                match order.side {
                    Side::Buy => {
                        best_bid = Some(
                            best_bid.map_or(order.price, |current: i64| current.max(order.price)),
                        );
                    }
                    Side::Sell => {
                        best_ask = Some(
                            best_ask.map_or(order.price, |current: i64| current.min(order.price)),
                        );
                    }
                }
            }
            if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
                assert!(
                    bid < ask,
                    "crossed book detected on {}:{} with bid {} and ask {}",
                    market.market_id,
                    market.outcome,
                    bid,
                    ask
                );
            }

            let materialized = self
                .engine
                .snapshot_market(&market.market_id, market.outcome)
                .await
                .unwrap()
                .expect("market snapshot should exist");
            assert_eq!(
                materialized.open_orders,
                market.orders.len(),
                "snapshot open order count should match runtime snapshot"
            );
            assert!(
                !matches!(materialized.state, MarketState::Closed) || materialized.open_orders == 0,
                "closed markets must not retain open orders"
            );
        }

        let trade_records = self.trade_records();
        let mut seen_trade_ids = HashSet::new();
        for record in &trade_records {
            assert!(
                seen_trade_ids.insert(record.trade_id.clone()),
                "duplicate trade journal entry for {}",
                record.trade_id
            );
        }

        let mut settlement_statuses: HashMap<String, Vec<TradeSettlementStatus>> = HashMap::new();
        for record in self.settlement_records() {
            settlement_statuses
                .entry(record.trade_id.clone())
                .or_default()
                .push(record.status);
        }
        for (trade_id, statuses) in settlement_statuses {
            let applied = statuses
                .iter()
                .filter(|status| **status == TradeSettlementStatus::Applied)
                .count();
            assert!(
                applied <= 1,
                "trade {} should not have more than one applied settlement record",
                trade_id
            );
        }
    }
}

pub fn seed_test_ledger(
    ledger: &Arc<LedgerService>,
    users: &[&str],
    spot_markets: &[&str],
    outcomes: &[i32],
) -> AnyhowResult<()> {
    for user in users {
        ledger.process_deposit(user, SEEDED_CASH, format!("seed-cash-{user}"))?;
    }
    for market in spot_markets {
        for user in users {
            for outcome in outcomes {
                if ledger.position_available_balance(user, market, *outcome) == 0 {
                    ledger.process_position_deposit(
                        user,
                        market,
                        *outcome,
                        SEEDED_POSITION,
                        format!("seed-pos-{user}-{market}-{outcome}"),
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub fn default_test_users() -> &'static [&'static str] {
    DEFAULT_USERS
}

pub fn default_test_markets() -> &'static [&'static str] {
    DEFAULT_SPOT_MARKETS
}

pub fn default_test_outcomes() -> &'static [i32] {
    DEFAULT_OUTCOMES
}

#[derive(Debug, Clone)]
pub struct RestingOrderRef {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub order_id: String,
    pub price: i64,
}
