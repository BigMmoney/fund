use anyhow::{anyhow, Result as AnyhowResult};
use chrono::{DateTime, Utc};
use eventbus::EventBus;
use instruments::{shared_default_registry, InstrumentRegistry};
use ledger::LedgerService;
use persistence::WalStore;
use risk::{policy_for_instrument_kind, FillIntent, RiskEngine, RiskError};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use types::{
    AdminAction, AdminCommand, AuthenticatedPrincipal, CancelOrderCommand, Command,
    CommandLifecycle, CommandMetadata, Event, Fill, InstrumentKind, InstrumentSpec, LedgerDelta,
    LedgerEntry, MarketState, MassCancelByMarketCommand, MassCancelBySessionCommand,
    MassCancelByUserCommand, NewOrderCommand, OrderState, OrderType, PrincipalRole,
    ReplaceOrderCommand, ReplayCursor, Side, TimeInForce,
};

#[derive(Debug, Clone)]
pub struct PartitionedEngineConfig {
    pub partitions: usize,
    pub queue_capacity: usize,
    pub snapshot_interval_commands: usize,
    pub max_open_orders_per_user: usize,
    pub cancel_window: Duration,
    pub max_cancel_to_new_ratio: f64,
    pub min_cancel_events_before_guard: usize,
    pub cancel_only_price_band_bps: i64,
    pub halt_price_band_bps: i64,
}

impl Default for PartitionedEngineConfig {
    fn default() -> Self {
        Self {
            partitions: 8,
            queue_capacity: 4096,
            snapshot_interval_commands: 64,
            max_open_orders_per_user: 200,
            cancel_window: Duration::from_secs(2),
            max_cancel_to_new_ratio: 3.0,
            min_cancel_events_before_guard: 25,
            cancel_only_price_band_bps: 500,
            halt_price_band_bps: 1_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionError {
    QueueFull {
        partition: usize,
    },
    PartitionClosed {
        partition: usize,
    },
    QueueResponseDropped,
    KillSwitchActive,
    InvalidOrder(&'static str),
    DuplicateOrderId(String),
    OrderNotFound(String),
    MarketClosed {
        market_id: String,
        outcome: i32,
        state: MarketState,
    },
    Persistence(String),
    PriceBandBreached {
        market_id: String,
        outcome: i32,
        state: MarketState,
        reference_price: i64,
        attempted_price: i64,
        deviation_bps: i64,
    },
    InsufficientLiquidityForFok,
    SelfTradePrevented(String),
    Ledger(String),
}

impl fmt::Display for SubmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubmissionError::QueueFull { partition } => {
                write!(f, "partition queue {partition} is full")
            }
            SubmissionError::PartitionClosed { partition } => {
                write!(f, "partition queue {partition} is closed")
            }
            SubmissionError::QueueResponseDropped => write!(f, "partition response dropped"),
            SubmissionError::KillSwitchActive => write!(f, "kill switch is active"),
            SubmissionError::InvalidOrder(reason) => write!(f, "invalid order: {reason}"),
            SubmissionError::DuplicateOrderId(order_id) => {
                write!(f, "duplicate order id: {order_id}")
            }
            SubmissionError::OrderNotFound(order_id) => write!(f, "order not found: {order_id}"),
            SubmissionError::Persistence(error) => write!(f, "persistence error: {error}"),
            SubmissionError::MarketClosed {
                market_id,
                outcome,
                state,
            } => {
                write!(f, "market {market_id}:{outcome} is {state:?}")
            }
            SubmissionError::PriceBandBreached {
                market_id,
                outcome,
                state,
                reference_price,
                attempted_price,
                deviation_bps,
            } => write!(
                f,
                "market {market_id}:{outcome} moved to {state:?}; price {attempted_price} deviates {deviation_bps}bps from reference {reference_price}"
            ),
            SubmissionError::InsufficientLiquidityForFok => {
                write!(f, "insufficient liquidity for fill-or-kill")
            }
            SubmissionError::SelfTradePrevented(order_id) => {
                write!(f, "self-trade prevented for order: {order_id}")
            }
            SubmissionError::Ledger(error) => write!(f, "ledger error: {error}"),
        }
    }
}

impl std::error::Error for SubmissionError {}

#[derive(Debug, Clone)]
pub struct SubmitOrderResult {
    pub metadata: CommandMetadata,
    pub order_id: String,
    pub market_state: MarketState,
    pub fills: Vec<Fill>,
    pub state: OrderState,
    pub remaining_amount: i64,
}

#[derive(Debug, Clone)]
pub struct CancelResult {
    pub metadata: CommandMetadata,
    pub market_state: MarketState,
    pub cancelled_order_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MarketSnapshot {
    pub market_id: String,
    pub outcome: i32,
    pub state: MarketState,
    pub reference_price: Option<i64>,
    pub last_trade_price: Option<i64>,
    pub best_bid: Option<i64>,
    pub best_ask: Option<i64>,
    pub open_orders: usize,
    pub recent_new_orders: usize,
    pub recent_cancel_events: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionQueueDepth {
    pub partition_id: usize,
    pub inflight: usize,
    pub capacity: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartitionStateSnapshot {
    pub replay_cursor: ReplayCursor,
    pub markets: Vec<MarketRuntimeSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionSnapshotRecord {
    pub partition_id: usize,
    pub kill_switch_enabled: bool,
    pub persisted_at: DateTime<Utc>,
    pub snapshot_version: u32,
    pub snapshot_checksum: u64,
    pub last_applied_command_seq: Option<u64>,
    pub snapshot: PartitionStateSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeJournalRecord {
    pub partition_id: usize,
    pub trade_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub buy_order_id: String,
    pub buy_user_id: String,
    pub sell_order_id: String,
    pub sell_user_id: String,
    pub price: i64,
    pub amount: i64,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketRuntimeSnapshot {
    pub market_id: String,
    pub outcome: i32,
    pub state: MarketState,
    pub reference_price: Option<i64>,
    pub last_trade_price: Option<i64>,
    pub reference_sources: Vec<ReferencePriceSourceSnapshot>,
    pub orders: Vec<RestingOrderSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferencePriceSourceSnapshot {
    pub source: String,
    pub price: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestingOrderSnapshot {
    pub order_id: String,
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub command_seq: Option<u64>,
    pub user_id: String,
    pub session_id: Option<String>,
    pub market_id: String,
    pub outcome: i32,
    pub side: Side,
    pub price: i64,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub post_only: bool,
    pub reduce_only: bool,
    #[serde(default)]
    pub leverage: Option<u32>,
    pub original_amount: i64,
    pub remaining_amount: i64,
    pub expires_at: Option<DateTime<Utc>>,
}

const SNAPSHOT_VERSION: u32 = 1;

#[derive(Clone)]
pub struct PartitionedMatchingEngine {
    config: PartitionedEngineConfig,
    partitions: Arc<Vec<PartitionHandle>>,
    kill_switch: Arc<AtomicBool>,
    risk: Arc<RiskEngine>,
    instruments: Arc<dyn InstrumentRegistry>,
    snapshot_store: Option<Arc<dyn WalStore<PartitionSnapshotRecord>>>,
}

impl PartitionedMatchingEngine {
    pub fn new(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
    ) -> Self {
        Self::new_with_registry(config, event_bus, risk, shared_default_registry())
    }

    pub fn new_with_registry(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
        instruments: Arc<dyn InstrumentRegistry>,
    ) -> Self {
        Self::build(
            config,
            event_bus,
            risk,
            instruments,
            None,
            None,
            HashMap::new(),
            false,
            HashMap::new(),
        )
    }

    pub fn with_snapshot_store(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
        snapshot_store: Arc<dyn WalStore<PartitionSnapshotRecord>>,
    ) -> AnyhowResult<Self> {
        Self::with_stores_and_registry(
            config,
            event_bus,
            risk,
            shared_default_registry(),
            Some(snapshot_store),
            None,
        )
    }

    pub fn with_stores(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
        snapshot_store: Option<Arc<dyn WalStore<PartitionSnapshotRecord>>>,
        trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
    ) -> AnyhowResult<Self> {
        Self::with_stores_and_registry(
            config,
            event_bus,
            risk,
            shared_default_registry(),
            snapshot_store,
            trade_store,
        )
    }

    pub fn with_stores_and_registry(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
        instruments: Arc<dyn InstrumentRegistry>,
        snapshot_store: Option<Arc<dyn WalStore<PartitionSnapshotRecord>>>,
        trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
    ) -> AnyhowResult<Self> {
        let mut latest_snapshots = HashMap::new();
        let mut kill_switch_enabled = false;
        let mut seen_trade_ids_by_partition: HashMap<usize, HashSet<String>> = HashMap::new();

        if let Some(store) = &snapshot_store {
            for record in store.entries()? {
                validate_snapshot_record(&record)?;
                kill_switch_enabled = record.kill_switch_enabled;
                latest_snapshots.insert(record.partition_id, record.snapshot);
            }
        }
        if let Some(store) = &trade_store {
            for record in store.entries()? {
                seen_trade_ids_by_partition
                    .entry(record.partition_id)
                    .or_default()
                    .insert(record.trade_id.clone());
            }
        }

        Ok(Self::build(
            config,
            event_bus,
            risk,
            instruments,
            snapshot_store,
            trade_store,
            latest_snapshots,
            kill_switch_enabled,
            seen_trade_ids_by_partition,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
        instruments: Arc<dyn InstrumentRegistry>,
        snapshot_store: Option<Arc<dyn WalStore<PartitionSnapshotRecord>>>,
        trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
        mut initial_snapshots: HashMap<usize, PartitionStateSnapshot>,
        kill_switch_enabled: bool,
        mut seen_trade_ids_by_partition: HashMap<usize, HashSet<String>>,
    ) -> Self {
        assert!(config.partitions > 0);
        assert!(config.queue_capacity > 0);

        let kill_switch = Arc::new(AtomicBool::new(kill_switch_enabled));
        let mut partitions = Vec::with_capacity(config.partitions);
        for partition_id in 0..config.partitions {
            let (tx, rx) = mpsc::channel(config.queue_capacity);
            let inflight = Arc::new(AtomicUsize::new(0));
            let dirty_commands = Arc::new(AtomicUsize::new(0));
            let initial_snapshot = initial_snapshots.remove(&partition_id).unwrap_or_default();
            tokio::spawn(run_partition(
                rx,
                inflight.clone(),
                config.clone(),
                event_bus.clone(),
                risk.clone(),
                instruments.clone(),
                kill_switch.clone(),
                trade_store.clone(),
                partition_id,
                initial_snapshot,
                seen_trade_ids_by_partition
                    .remove(&partition_id)
                    .unwrap_or_default(),
            ));
            partitions.push(PartitionHandle {
                partition_id,
                queue_capacity: config.queue_capacity,
                inflight,
                dirty_commands,
                tx,
            });
        }

        Self {
            config,
            partitions: Arc::new(partitions),
            kill_switch,
            risk,
            instruments,
            snapshot_store,
        }
    }

    pub fn kill_switch_enabled(&self) -> bool {
        self.kill_switch.load(Ordering::Relaxed)
    }

    pub fn queue_depths(&self) -> Vec<PartitionQueueDepth> {
        self.partitions
            .iter()
            .map(|handle| PartitionQueueDepth {
                partition_id: handle.partition_id,
                inflight: handle.inflight.load(Ordering::Relaxed),
                capacity: handle.queue_capacity,
            })
            .collect()
    }

    pub async fn export_snapshots(&self) -> Result<Vec<PartitionSnapshotRecord>, SubmissionError> {
        let mut snapshots = Vec::with_capacity(self.config.partitions);
        for partition in 0..self.config.partitions {
            snapshots.push(self.export_partition_snapshot(partition).await?);
        }
        Ok(snapshots)
    }

    #[deprecated(
        note = "max snapshot seq is not a safe replay boundary; use global_replay_floor_command_seq() or per-partition cursors"
    )]
    pub async fn latest_applied_command_seq(&self) -> Result<Option<u64>, SubmissionError> {
        let snapshots = self.export_snapshots().await?;
        Ok(snapshots
            .into_iter()
            .filter_map(|record| record.last_applied_command_seq)
            .max())
    }

    pub async fn global_replay_floor_command_seq(&self) -> Result<Option<u64>, SubmissionError> {
        let snapshots = self.export_snapshots().await?;
        Ok(snapshots
            .into_iter()
            .filter_map(|record| record.last_applied_command_seq)
            .min())
    }

    pub fn partitions_for_command(&self, command: &Command) -> Vec<usize> {
        match command {
            Command::NewOrder(command) => {
                vec![self.partition_for_market(&command.market_id, command.outcome)]
            }
            Command::CancelOrder(command) => command
                .outcome
                .map(|outcome| vec![self.partition_for_market(&command.market_id, outcome)])
                .unwrap_or_else(|| (0..self.config.partitions).collect()),
            Command::ReplaceOrder(command) => command
                .outcome
                .map(|outcome| vec![self.partition_for_market(&command.market_id, outcome)])
                .unwrap_or_else(|| (0..self.config.partitions).collect()),
            Command::MassCancelByUser(_)
            | Command::MassCancelBySession(_)
            | Command::MassCancelByMarket(_)
            | Command::Admin(_) => (0..self.config.partitions).collect(),
        }
    }

    pub fn resolve_instrument(&self, market_id: &str) -> InstrumentSpec {
        self.instruments.resolve(market_id)
    }

    pub async fn replay_command(&self, command: Command) -> Result<(), SubmissionError> {
        match command {
            Command::NewOrder(command) => {
                self.submit_new_order(command).await?;
            }
            Command::CancelOrder(command) => {
                self.cancel_order(command).await?;
            }
            Command::ReplaceOrder(command) => {
                self.replace_order(command).await?;
            }
            Command::MassCancelByUser(command) => {
                self.mass_cancel_by_user(command).await?;
            }
            Command::MassCancelBySession(command) => {
                self.mass_cancel_by_session(command).await?;
            }
            Command::MassCancelByMarket(command) => {
                self.mass_cancel_by_market(command).await?;
            }
            Command::Admin(command) => {
                self.submit_admin(command).await?;
            }
        }
        Ok(())
    }

    pub async fn submit_new_order(
        &self,
        mut command: NewOrderCommand,
    ) -> Result<SubmitOrderResult, SubmissionError> {
        if self.kill_switch_enabled() {
            return Err(SubmissionError::KillSwitchActive);
        }
        command.metadata.advance(CommandLifecycle::Routed);
        let partition = self.partition_for_market(&command.market_id, command.outcome);
        let (response_tx, response_rx) = oneshot::channel();
        self.send_to_partition(
            partition,
            PartitionRequest::NewOrder {
                command,
                response: response_tx,
            },
        )?;
        let result = response_rx
            .await
            .map_err(|_| SubmissionError::QueueResponseDropped)??;
        self.partitions[partition]
            .dirty_commands
            .fetch_add(1, Ordering::Relaxed);
        self.persist_partitions(&[partition]).await?;
        Ok(result)
    }

    pub async fn replace_order(
        &self,
        mut command: ReplaceOrderCommand,
    ) -> Result<SubmitOrderResult, SubmissionError> {
        command.metadata.advance(CommandLifecycle::Routed);
        let partitions: Vec<usize> = if let Some(outcome) = command.outcome {
            vec![self.partition_for_market(&command.market_id, outcome)]
        } else {
            (0..self.config.partitions).collect()
        };

        let mut last_not_found = None;
        for partition in partitions {
            let (response_tx, response_rx) = oneshot::channel();
            self.send_to_partition(
                partition,
                PartitionRequest::ReplaceOrder {
                    command: command.clone(),
                    response: response_tx,
                },
            )?;

            match response_rx
                .await
                .map_err(|_| SubmissionError::QueueResponseDropped)?
            {
                Ok(result) => {
                    for handle in self.partitions.iter() {
                        handle.dirty_commands.fetch_add(1, Ordering::Relaxed);
                    }
                    self.persist_all_partitions().await?;
                    return Ok(result);
                }
                Err(SubmissionError::OrderNotFound(order_id)) => {
                    last_not_found = Some(order_id);
                }
                Err(error) => return Err(error),
            }
        }

        Err(SubmissionError::OrderNotFound(
            last_not_found.unwrap_or(command.order_id),
        ))
    }

    pub async fn cancel_order(
        &self,
        mut command: CancelOrderCommand,
    ) -> Result<CancelResult, SubmissionError> {
        command.metadata.advance(CommandLifecycle::Routed);

        let partitions: Vec<usize> = if let Some(outcome) = command.outcome {
            vec![self.partition_for_market(&command.market_id, outcome)]
        } else {
            (0..self.config.partitions).collect()
        };
        let mut cancelled_order_ids = Vec::new();
        let mut metadata = None;
        let mut market_state = MarketState::Normal;

        for partition in partitions {
            let (response_tx, response_rx) = oneshot::channel();
            self.send_to_partition(
                partition,
                PartitionRequest::CancelOrder {
                    command: command.clone(),
                    response: response_tx,
                },
            )?;

            match response_rx
                .await
                .map_err(|_| SubmissionError::QueueResponseDropped)?
            {
                Ok(result) => {
                    cancelled_order_ids.extend(result.cancelled_order_ids);
                    metadata = Some(result.metadata);
                    market_state = combine_market_state(market_state, result.market_state);
                }
                Err(SubmissionError::OrderNotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        if cancelled_order_ids.is_empty() {
            return Err(SubmissionError::OrderNotFound(command.order_id));
        }

        for handle in self.partitions.iter() {
            handle.dirty_commands.fetch_add(1, Ordering::Relaxed);
        }
        self.persist_all_partitions().await?;
        Ok(CancelResult {
            metadata: metadata.expect("cancel success must have metadata"),
            market_state,
            cancelled_order_ids,
        })
    }
    pub async fn mass_cancel_by_user(
        &self,
        mut command: MassCancelByUserCommand,
    ) -> Result<CancelResult, SubmissionError> {
        command.metadata.advance(CommandLifecycle::Routed);
        let result = self
            .broadcast_cancel(|response| PartitionRequest::MassCancelByUser {
                command: command.clone(),
                response,
            })
            .await?;
        for handle in self.partitions.iter() {
            handle.dirty_commands.fetch_add(1, Ordering::Relaxed);
        }
        self.persist_all_partitions().await?;
        Ok(result)
    }

    pub async fn mass_cancel_by_session(
        &self,
        mut command: MassCancelBySessionCommand,
    ) -> Result<CancelResult, SubmissionError> {
        command.metadata.advance(CommandLifecycle::Routed);
        let result = self
            .broadcast_cancel(|response| PartitionRequest::MassCancelBySession {
                command: command.clone(),
                response,
            })
            .await?;
        for handle in self.partitions.iter() {
            handle.dirty_commands.fetch_add(1, Ordering::Relaxed);
        }
        self.persist_all_partitions().await?;
        Ok(result)
    }

    pub async fn mass_cancel_by_market(
        &self,
        mut command: MassCancelByMarketCommand,
    ) -> Result<CancelResult, SubmissionError> {
        command.metadata.advance(CommandLifecycle::Routed);
        let result = self
            .broadcast_cancel(|response| PartitionRequest::MassCancelByMarket {
                command: command.clone(),
                response,
            })
            .await?;
        for handle in self.partitions.iter() {
            handle.dirty_commands.fetch_add(1, Ordering::Relaxed);
        }
        self.persist_all_partitions().await?;
        Ok(result)
    }

    pub async fn submit_admin(&self, mut command: AdminCommand) -> Result<(), SubmissionError> {
        command.metadata.advance(CommandLifecycle::Routed);
        for partition in 0..self.config.partitions {
            let (response_tx, response_rx) = oneshot::channel();
            self.send_to_partition(
                partition,
                PartitionRequest::Admin {
                    command: command.clone(),
                    response: response_tx,
                },
            )?;
            response_rx
                .await
                .map_err(|_| SubmissionError::QueueResponseDropped)??;
        }
        for handle in self.partitions.iter() {
            handle.dirty_commands.fetch_add(1, Ordering::Relaxed);
        }
        self.persist_all_partitions().await?;
        Ok(())
    }

    pub async fn update_reference_price(
        &self,
        market_id: impl Into<String>,
        outcome: i32,
        source: impl Into<String>,
        reference_price: i64,
    ) -> Result<MarketSnapshot, SubmissionError> {
        let market_id = market_id.into();
        let source = source.into();
        let partition = self.partition_for_market(&market_id, outcome);
        let (response_tx, response_rx) = oneshot::channel();
        self.send_to_partition(
            partition,
            PartitionRequest::UpdateReferencePrice {
                market_id,
                outcome,
                source,
                reference_price,
                response: response_tx,
            },
        )?;
        let result = response_rx
            .await
            .map_err(|_| SubmissionError::QueueResponseDropped)??;
        self.partitions[partition]
            .dirty_commands
            .fetch_add(1, Ordering::Relaxed);
        self.persist_partitions(&[partition]).await?;
        Ok(result)
    }

    pub async fn snapshot_market(
        &self,
        market_id: impl Into<String>,
        outcome: i32,
    ) -> Result<Option<MarketSnapshot>, SubmissionError> {
        let market_id = market_id.into();
        let partition = self.partition_for_market(&market_id, outcome);
        let (response_tx, response_rx) = oneshot::channel();
        self.send_to_partition(
            partition,
            PartitionRequest::Snapshot {
                market_id,
                outcome,
                response: response_tx,
            },
        )?;
        response_rx
            .await
            .map_err(|_| SubmissionError::QueueResponseDropped)
    }

    async fn broadcast_cancel<F>(&self, mut builder: F) -> Result<CancelResult, SubmissionError>
    where
        F: FnMut(oneshot::Sender<Result<CancelResult, SubmissionError>>) -> PartitionRequest,
    {
        let mut cancelled_order_ids = Vec::new();
        let mut metadata = None;
        let mut market_state = MarketState::Normal;

        for partition in 0..self.config.partitions {
            let (response_tx, response_rx) = oneshot::channel();
            self.send_to_partition(partition, builder(response_tx))?;
            let result = response_rx
                .await
                .map_err(|_| SubmissionError::QueueResponseDropped)??;
            cancelled_order_ids.extend(result.cancelled_order_ids);
            metadata = Some(result.metadata);
            market_state = combine_market_state(market_state, result.market_state);
        }

        Ok(CancelResult {
            metadata: metadata.expect("broadcast always has partitions"),
            market_state,
            cancelled_order_ids,
        })
    }

    async fn export_partition_snapshot(
        &self,
        partition: usize,
    ) -> Result<PartitionSnapshotRecord, SubmissionError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.send_to_partition(
            partition,
            PartitionRequest::ExportSnapshot {
                response: response_tx,
            },
        )?;
        let snapshot = response_rx
            .await
            .map_err(|_| SubmissionError::QueueResponseDropped)?;
        let kill_switch_enabled = self.kill_switch_enabled();
        let snapshot_checksum =
            calculate_snapshot_checksum(partition, kill_switch_enabled, &snapshot);
        Ok(PartitionSnapshotRecord {
            partition_id: partition,
            kill_switch_enabled,
            persisted_at: Utc::now(),
            snapshot_version: SNAPSHOT_VERSION,
            snapshot_checksum,
            last_applied_command_seq: snapshot.replay_cursor.snapshot_seq,
            snapshot,
        })
    }

    async fn persist_all_partitions(&self) -> Result<(), SubmissionError> {
        let partitions: Vec<usize> = (0..self.config.partitions).collect();
        self.persist_partitions(&partitions).await
    }

    async fn persist_partitions(&self, partition_ids: &[usize]) -> Result<(), SubmissionError> {
        let Some(store) = &self.snapshot_store else {
            return Ok(());
        };

        let mut partitions = partition_ids.to_vec();
        partitions.sort_unstable();
        partitions.dedup();

        let mut wrote_snapshot = false;
        for partition in partitions {
            let dirty_commands = self.partitions[partition]
                .dirty_commands
                .load(Ordering::Relaxed);
            if dirty_commands < self.config.snapshot_interval_commands {
                continue;
            }
            let record = self.export_partition_snapshot(partition).await?;
            store
                .append(&record)
                .map_err(|error| SubmissionError::Persistence(error.to_string()))?;
            self.partitions[partition]
                .dirty_commands
                .store(0, Ordering::Relaxed);
            wrote_snapshot = true;
        }

        if wrote_snapshot {
            if let Some(replay_floor) = self.global_replay_floor_command_seq().await? {
                let _ = self.risk.ledger().prune_seen_op_ids_up_to(replay_floor);
            }
        }

        Ok(())
    }

    fn partition_for_market(&self, market_id: &str, outcome: i32) -> usize {
        let mut hasher = DefaultHasher::new();
        market_id.hash(&mut hasher);
        outcome.hash(&mut hasher);
        (hasher.finish() as usize) % self.config.partitions
    }

    fn send_to_partition(
        &self,
        partition: usize,
        request: PartitionRequest,
    ) -> Result<(), SubmissionError> {
        let handle = &self.partitions[partition];
        handle.inflight.fetch_add(1, Ordering::Relaxed);
        match handle.tx.try_send(request) {
            Ok(()) => Ok(()),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                handle.inflight.fetch_sub(1, Ordering::Relaxed);
                Err(SubmissionError::QueueFull { partition })
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                handle.inflight.fetch_sub(1, Ordering::Relaxed);
                Err(SubmissionError::PartitionClosed { partition })
            }
        }
    }
}

#[derive(Clone)]
struct PartitionHandle {
    partition_id: usize,
    queue_capacity: usize,
    inflight: Arc<AtomicUsize>,
    dirty_commands: Arc<AtomicUsize>,
    tx: mpsc::Sender<PartitionRequest>,
}

enum PartitionRequest {
    NewOrder {
        command: NewOrderCommand,
        response: oneshot::Sender<Result<SubmitOrderResult, SubmissionError>>,
    },
    ReplaceOrder {
        command: ReplaceOrderCommand,
        response: oneshot::Sender<Result<SubmitOrderResult, SubmissionError>>,
    },
    CancelOrder {
        command: CancelOrderCommand,
        response: oneshot::Sender<Result<CancelResult, SubmissionError>>,
    },
    MassCancelByUser {
        command: MassCancelByUserCommand,
        response: oneshot::Sender<Result<CancelResult, SubmissionError>>,
    },
    MassCancelBySession {
        command: MassCancelBySessionCommand,
        response: oneshot::Sender<Result<CancelResult, SubmissionError>>,
    },
    MassCancelByMarket {
        command: MassCancelByMarketCommand,
        response: oneshot::Sender<Result<CancelResult, SubmissionError>>,
    },
    Admin {
        command: AdminCommand,
        response: oneshot::Sender<Result<(), SubmissionError>>,
    },
    UpdateReferencePrice {
        market_id: String,
        outcome: i32,
        source: String,
        reference_price: i64,
        response: oneshot::Sender<Result<MarketSnapshot, SubmissionError>>,
    },
    Snapshot {
        market_id: String,
        outcome: i32,
        response: oneshot::Sender<Option<MarketSnapshot>>,
    },
    ExportSnapshot {
        response: oneshot::Sender<PartitionStateSnapshot>,
    },
}

#[allow(clippy::too_many_arguments)]
async fn run_partition(
    mut rx: mpsc::Receiver<PartitionRequest>,
    inflight: Arc<AtomicUsize>,
    config: PartitionedEngineConfig,
    event_bus: EventBus,
    risk: Arc<RiskEngine>,
    instruments: Arc<dyn InstrumentRegistry>,
    kill_switch: Arc<AtomicBool>,
    trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
    partition_id: usize,
    initial_snapshot: PartitionStateSnapshot,
    seen_trade_ids: HashSet<String>,
) {
    let mut state = PartitionState::from_snapshot(
        config,
        event_bus,
        risk,
        instruments,
        kill_switch,
        trade_store,
        partition_id,
        initial_snapshot,
        seen_trade_ids,
    );
    while let Some(request) = rx.recv().await {
        state.process(request);
        inflight.fetch_sub(1, Ordering::Relaxed);
    }
}
struct PartitionState {
    config: PartitionedEngineConfig,
    event_bus: EventBus,
    risk: Arc<RiskEngine>,
    instruments: Arc<dyn InstrumentRegistry>,
    kill_switch: Arc<AtomicBool>,
    trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
    partition_id: usize,
    markets: HashMap<MarketKey, MarketRuntime>,
    replay_cursor: ReplayCursor,
    seen_trade_ids: HashSet<String>,
}

impl PartitionState {
    #[allow(clippy::too_many_arguments)]
    fn from_snapshot(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
        instruments: Arc<dyn InstrumentRegistry>,
        kill_switch: Arc<AtomicBool>,
        trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
        partition_id: usize,
        snapshot: PartitionStateSnapshot,
        seen_trade_ids: HashSet<String>,
    ) -> Self {
        let replay_cursor = snapshot.replay_cursor;
        let markets = snapshot
            .markets
            .into_iter()
            .map(|market| {
                let instrument = instruments.resolve(&market.market_id);
                let runtime = MarketRuntime::from_snapshot(market, &instrument);
                (
                    MarketKey::new(runtime.market_id.clone(), runtime.outcome),
                    runtime,
                )
            })
            .collect();

        Self {
            config,
            event_bus,
            risk,
            instruments,
            kill_switch,
            trade_store,
            partition_id,
            markets,
            replay_cursor,
            seen_trade_ids,
        }
    }

    fn export_snapshot(&mut self) -> PartitionStateSnapshot {
        if let Some(snapshot_seq) = self.replay_cursor.snapshot_seq {
            compact_seen_trade_ids(&mut self.seen_trade_ids, snapshot_seq);
        }
        PartitionStateSnapshot {
            replay_cursor: self.replay_cursor,
            markets: self
                .markets
                .values()
                .map(MarketRuntime::export_snapshot)
                .collect(),
        }
    }

    fn instrument_spec(&self, market_id: &str) -> InstrumentSpec {
        self.instruments.resolve(market_id)
    }

    fn process(&mut self, request: PartitionRequest) {
        match request {
            PartitionRequest::NewOrder { command, response } => {
                let _ = response.send(self.process_new_order(command));
            }
            PartitionRequest::ReplaceOrder { command, response } => {
                let _ = response.send(self.process_replace_order(command));
            }
            PartitionRequest::CancelOrder { command, response } => {
                let _ = response.send(self.process_cancel_order(command));
            }
            PartitionRequest::MassCancelByUser { command, response } => {
                let _ = response.send(self.process_mass_cancel_by_user(command));
            }
            PartitionRequest::MassCancelBySession { command, response } => {
                let _ = response.send(self.process_mass_cancel_by_session(command));
            }
            PartitionRequest::MassCancelByMarket { command, response } => {
                let _ = response.send(self.process_mass_cancel_by_market(command));
            }
            PartitionRequest::Admin { command, response } => {
                let _ = response.send(self.process_admin(command));
            }
            PartitionRequest::UpdateReferencePrice {
                market_id,
                outcome,
                source,
                reference_price,
                response,
            } => {
                let _ = response.send(self.process_update_reference_price(
                    market_id,
                    outcome,
                    source,
                    reference_price,
                ));
            }
            PartitionRequest::Snapshot {
                market_id,
                outcome,
                response,
            } => {
                let _ = response.send(self.snapshot_market(&market_id, outcome));
            }
            PartitionRequest::ExportSnapshot { response } => {
                let _ = response.send(self.export_snapshot());
            }
        }
    }

    fn process_new_order(
        &mut self,
        mut command: NewOrderCommand,
    ) -> Result<SubmitOrderResult, SubmissionError> {
        if self.should_skip_replayed_command(command.metadata.command_seq) {
            return Ok(skipped_new_order_result(
                &command,
                self.market_state_for(&command.market_id, command.outcome),
            ));
        }
        validate_new_order(&command)?;
        let instrument = self.instrument_spec(&command.market_id);
        command.leverage = normalized_command_leverage(&instrument, &command)?;
        if self.kill_switch.load(Ordering::Relaxed) {
            return Err(SubmissionError::KillSwitchActive);
        }

        let key = MarketKey::new(command.market_id.clone(), command.outcome);
        self.evict_expired_orders_for_market(&key, command.metadata.received_at)?;
        let (markets, seen_trade_ids) = (&mut self.markets, &mut self.seen_trade_ids);
        let market = markets
            .entry(key.clone())
            .or_insert_with(|| MarketRuntime::new(&key.market_id, key.outcome));
        validate_order_acceptance(
            market,
            &self.config,
            &self.risk,
            &instrument,
            &command,
            None,
        )?;

        command
            .metadata
            .advance(CommandLifecycle::PartitionAccepted);

        let mut incoming = RestingOrder::from_new_order(command.clone());
        if incoming.order_type == OrderType::Market
            && (incoming.side == Side::Buy || instrument.kind != InstrumentKind::Spot)
        {
            incoming.reserved_cash =
                market_buy_budget(market, &self.risk, &instrument, &command, None)?;
        }
        let reserve_ids =
            reserve_order_reservation(&self.risk, &instrument, &mut incoming, "new_order")?;
        command.metadata.advance(CommandLifecycle::RiskReserved);
        let _checked_command = self.risk.to_risk_checked_command(
            AuthenticatedPrincipal {
                subject: command.user_id.clone(),
                role: PrincipalRole::User,
                session_id: command.session_id.clone(),
            },
            Command::NewOrder(command.clone()),
            reserve_ids,
        );
        let match_outcome = match_incoming(
            market,
            &mut incoming,
            &instrument,
            &self.config,
            &self.event_bus,
            &self.risk,
            self.trade_store.as_deref(),
            seen_trade_ids,
            self.partition_id,
        )?;
        let fills = match_outcome.fills;
        if !fills.is_empty() {
            command.metadata.advance(CommandLifecycle::Executed);
        }

        if let Some(error) = match_outcome.aborted {
            command.metadata.advance(CommandLifecycle::Completed);
            release_order_reservation(&self.risk, &instrument, &incoming, "aborted")?;
            let market_state = market.state;
            record_recent_event(market, &self.config, RecentMarketEventKind::NewOrder, 1);
            self.advance_replay_cursor(command.metadata.command_seq);
            return if fills.is_empty() {
                Err(error)
            } else {
                Ok(SubmitOrderResult {
                    metadata: command.metadata,
                    order_id: incoming.order_id,
                    market_state,
                    fills,
                    state: OrderState::PartiallyFilled,
                    remaining_amount: incoming.remaining_amount,
                })
            };
        }

        let state = if incoming.remaining_amount == 0 {
            command.metadata.advance(CommandLifecycle::Completed);
            release_order_reservation(&self.risk, &instrument, &incoming, "completed")?;
            OrderState::Filled
        } else if incoming.order_type == OrderType::Market
            || matches!(incoming.time_in_force, TimeInForce::Ioc | TimeInForce::Fok)
        {
            command.metadata.advance(CommandLifecycle::Completed);
            release_order_reservation(&self.risk, &instrument, &incoming, "non_resting")?;
            if incoming.remaining_amount < incoming.original_amount {
                OrderState::PartiallyFilled
            } else {
                OrderState::Cancelled
            }
        } else {
            insert_resting_order(market, incoming.clone());
            OrderState::Active
        };

        let market_state = market.state;
        let command_seq = command.metadata.command_seq;

        record_recent_event(market, &self.config, RecentMarketEventKind::NewOrder, 1);
        self.advance_replay_cursor(command_seq);

        Ok(SubmitOrderResult {
            metadata: command.metadata,
            order_id: incoming.order_id,
            market_state,
            fills,
            state,
            remaining_amount: incoming.remaining_amount,
        })
    }

    fn process_replace_order(
        &mut self,
        command: ReplaceOrderCommand,
    ) -> Result<SubmitOrderResult, SubmissionError> {
        if self.should_skip_replayed_command(command.metadata.command_seq) {
            return Ok(skipped_replace_order_result(
                &command,
                aggregate_market_state(&self.markets),
            ));
        }

        let candidate_keys: Vec<_> = self
            .markets
            .keys()
            .filter(|key| {
                key.market_id == command.market_id
                    && command.outcome.is_none_or(|outcome| key.outcome == outcome)
            })
            .cloned()
            .collect();
        for key in &candidate_keys {
            self.evict_expired_orders_for_market(key, command.metadata.received_at)?;
        }
        let (market_key, existing) = self.find_existing_order_for_replace(&command)?;
        let mut replacement = build_replacement_order_command(&existing, command.clone());
        let instrument = self.instrument_spec(&replacement.market_id);
        replacement.leverage = normalized_command_leverage(&instrument, &replacement)?;
        {
            let market = self
                .markets
                .get_mut(&market_key)
                .ok_or_else(|| SubmissionError::OrderNotFound(command.order_id.clone()))?;
            validate_order_acceptance(
                market,
                &self.config,
                &self.risk,
                &instrument,
                &replacement,
                Some(&existing),
            )?;
        }

        release_order_reservation(&self.risk, &instrument, &existing, "replace_release")?;
        {
            let market = self
                .markets
                .get_mut(&market_key)
                .ok_or_else(|| SubmissionError::OrderNotFound(command.order_id.clone()))?;
            market.orders.remove(&existing.order_id);
            market.remove_from_book(&existing);
            market.remove_order_indexes(
                &existing.order_id,
                &existing.user_id,
                existing.session_id.as_deref(),
            );
        }

        match self.process_new_order(replacement) {
            Ok(result) => Ok(result),
            Err(error) => {
                let mut restored = existing.clone();
                if let Err(restore_error) = reserve_order_reservation(
                    &self.risk,
                    &instrument,
                    &mut restored,
                    "replace_restore",
                ) {
                    if let Some(market) = self.markets.get_mut(&market_key) {
                        market.state = MarketState::Halted;
                    }
                    return Err(restore_error);
                }
                let market = self.markets.entry(market_key.clone()).or_insert_with(|| {
                    MarketRuntime::new(&market_key.market_id, market_key.outcome)
                });
                insert_resting_order(market, restored);
                Err(error)
            }
        }
    }

    fn process_cancel_order(
        &mut self,
        mut command: CancelOrderCommand,
    ) -> Result<CancelResult, SubmissionError> {
        if self.should_skip_replayed_command(command.metadata.command_seq) {
            return Ok(skipped_cancel_result(
                command.metadata,
                aggregate_market_state(&self.markets),
            ));
        }
        let cancelled_order_ids = cancel_orders(
            &mut self.markets,
            &self.config,
            &self.risk,
            self.instruments.as_ref(),
            Some(&command.market_id),
            command.outcome,
            Some(command.order_id.as_str()),
            None,
            Some(command.user_id.as_str()),
        )?;
        command.metadata.advance(CommandLifecycle::Executed);
        command.metadata.advance(CommandLifecycle::Completed);
        self.advance_replay_cursor(command.metadata.command_seq);
        Ok(CancelResult {
            metadata: command.metadata,
            market_state: aggregate_market_state(&self.markets),
            cancelled_order_ids,
        })
    }

    fn process_mass_cancel_by_user(
        &mut self,
        mut command: MassCancelByUserCommand,
    ) -> Result<CancelResult, SubmissionError> {
        if self.should_skip_replayed_command(command.metadata.command_seq) {
            return Ok(skipped_cancel_result(
                command.metadata,
                aggregate_market_state(&self.markets),
            ));
        }
        let cancelled_order_ids = cancel_orders(
            &mut self.markets,
            &self.config,
            &self.risk,
            self.instruments.as_ref(),
            None,
            None,
            None,
            None,
            Some(command.user_id.as_str()),
        )?;
        command.metadata.advance(CommandLifecycle::Executed);
        command.metadata.advance(CommandLifecycle::Completed);
        self.advance_replay_cursor(command.metadata.command_seq);
        Ok(CancelResult {
            metadata: command.metadata,
            market_state: aggregate_market_state(&self.markets),
            cancelled_order_ids,
        })
    }

    fn process_mass_cancel_by_session(
        &mut self,
        mut command: MassCancelBySessionCommand,
    ) -> Result<CancelResult, SubmissionError> {
        if self.should_skip_replayed_command(command.metadata.command_seq) {
            return Ok(skipped_cancel_result(
                command.metadata,
                aggregate_market_state(&self.markets),
            ));
        }
        let cancelled_order_ids = cancel_orders(
            &mut self.markets,
            &self.config,
            &self.risk,
            self.instruments.as_ref(),
            None,
            None,
            None,
            Some(command.session_id.as_str()),
            Some(command.user_id.as_str()),
        )?;
        command.metadata.advance(CommandLifecycle::Executed);
        command.metadata.advance(CommandLifecycle::Completed);
        self.advance_replay_cursor(command.metadata.command_seq);
        Ok(CancelResult {
            metadata: command.metadata,
            market_state: aggregate_market_state(&self.markets),
            cancelled_order_ids,
        })
    }

    fn process_mass_cancel_by_market(
        &mut self,
        mut command: MassCancelByMarketCommand,
    ) -> Result<CancelResult, SubmissionError> {
        if self.should_skip_replayed_command(command.metadata.command_seq) {
            return Ok(skipped_cancel_result(
                command.metadata,
                aggregate_market_state(&self.markets),
            ));
        }
        let cancelled_order_ids = cancel_orders(
            &mut self.markets,
            &self.config,
            &self.risk,
            self.instruments.as_ref(),
            Some(command.market_id.as_str()),
            None,
            None,
            None,
            None,
        )?;
        command.metadata.advance(CommandLifecycle::Executed);
        command.metadata.advance(CommandLifecycle::Completed);
        self.advance_replay_cursor(command.metadata.command_seq);
        Ok(CancelResult {
            metadata: command.metadata,
            market_state: aggregate_market_state(&self.markets),
            cancelled_order_ids,
        })
    }

    fn process_admin(&mut self, mut command: AdminCommand) -> Result<(), SubmissionError> {
        if self.should_skip_replayed_command(command.metadata.command_seq) {
            return Ok(());
        }
        match command.action {
            AdminAction::KillSwitch { enabled } => {
                self.kill_switch.store(enabled, Ordering::Relaxed);
            }
            AdminAction::SetMarketState {
                market_id,
                outcome,
                state,
            } => {
                for market in self.markets.values_mut().filter(|market| {
                    market.market_id == market_id
                        && outcome.is_none_or(|value| market.outcome == value)
                }) {
                    market.state = state;
                }
            }
        }
        command.metadata.advance(CommandLifecycle::Executed);
        command.metadata.advance(CommandLifecycle::Completed);
        self.advance_replay_cursor(command.metadata.command_seq);
        Ok(())
    }

    fn should_skip_replayed_command(&self, command_seq: Option<u64>) -> bool {
        match (self.replay_cursor.snapshot_seq, command_seq) {
            (Some(snapshot_seq), Some(command_seq)) => command_seq <= snapshot_seq,
            _ => false,
        }
    }

    fn market_state_for(&self, market_id: &str, outcome: i32) -> MarketState {
        self.markets
            .get(&MarketKey::new(market_id.to_string(), outcome))
            .map(|market| market.state)
            .unwrap_or(MarketState::Normal)
    }

    fn find_existing_order_for_replace(
        &self,
        command: &ReplaceOrderCommand,
    ) -> Result<(MarketKey, RestingOrder), SubmissionError> {
        self.markets
            .iter()
            .filter(|(key, _)| {
                key.market_id == command.market_id
                    && command.outcome.is_none_or(|value| key.outcome == value)
            })
            .find_map(|(key, market)| {
                market
                    .orders
                    .get(&command.order_id)
                    .cloned()
                    .map(|order| (key.clone(), order))
            })
            .filter(|(_, order)| order.user_id == command.user_id)
            .ok_or_else(|| SubmissionError::OrderNotFound(command.order_id.clone()))
    }

    fn process_update_reference_price(
        &mut self,
        market_id: String,
        outcome: i32,
        source: String,
        reference_price: i64,
    ) -> Result<MarketSnapshot, SubmissionError> {
        if reference_price <= 0 {
            return Err(SubmissionError::InvalidOrder(
                "reference price must be positive",
            ));
        }
        let key = MarketKey::new(market_id, outcome);
        let market = self
            .markets
            .entry(key.clone())
            .or_insert_with(|| MarketRuntime::new(&key.market_id, key.outcome));
        market.reference_price = Some(reference_price);
        market.reference_sources.insert(
            source.clone(),
            ReferencePriceSourceSnapshot {
                source,
                price: reference_price,
                updated_at: Utc::now(),
            },
        );
        Ok(market.snapshot())
    }

    fn snapshot_market(&mut self, market_id: &str, outcome: i32) -> Option<MarketSnapshot> {
        let key = MarketKey::new(market_id.to_string(), outcome);
        let _ = self.evict_expired_orders_for_market(&key, Utc::now());
        self.markets.get_mut(&key).map(|market| {
            evict_stale_events(market, self.config.cancel_window);
            market.snapshot()
        })
    }

    fn advance_replay_cursor(&mut self, command_seq: Option<u64>) {
        let Some(command_seq) = command_seq else {
            return;
        };
        self.replay_cursor.snapshot_seq = Some(command_seq);
        self.replay_cursor.next_seq = command_seq.saturating_add(1);
    }

    fn evict_expired_orders_for_market(
        &mut self,
        key: &MarketKey,
        now: DateTime<Utc>,
    ) -> Result<(), SubmissionError> {
        let instruments = self.instruments.clone();
        let Some(market) = self.markets.get_mut(key) else {
            return Ok(());
        };
        let expired_ids: Vec<String> = market
            .orders
            .values()
            .filter(|order| order.expires_at.is_some_and(|expiry| expiry <= now))
            .map(|order| order.order_id.clone())
            .collect();
        for order_id in expired_ids {
            let Some(order) = market.orders.remove(&order_id) else {
                continue;
            };
            market.remove_from_book(&order);
            market.remove_order_indexes(
                &order.order_id,
                &order.user_id,
                order.session_id.as_deref(),
            );
            let instrument = instruments.resolve(&order.market_id);
            release_order_reservation(&self.risk, &instrument, &order, "expired")?;
            record_recent_event(market, &self.config, RecentMarketEventKind::Cancel, 1);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct MarketKey {
    market_id: String,
    outcome: i32,
}

impl MarketKey {
    fn new(market_id: String, outcome: i32) -> Self {
        Self { market_id, outcome }
    }
}

#[derive(Debug)]
struct MarketRuntime {
    market_id: String,
    outcome: i32,
    state: MarketState,
    reference_price: Option<i64>,
    last_trade_price: Option<i64>,
    reference_sources: HashMap<String, ReferencePriceSourceSnapshot>,
    bids: BTreeMap<i64, VecDeque<String>>,
    asks: BTreeMap<i64, VecDeque<String>>,
    orders: HashMap<String, RestingOrder>,
    user_orders: HashMap<String, HashSet<String>>,
    session_orders: HashMap<String, HashSet<String>>,
    recent_events: VecDeque<RecentMarketEvent>,
}

impl MarketRuntime {
    fn new(market_id: &str, outcome: i32) -> Self {
        Self {
            market_id: market_id.to_string(),
            outcome,
            state: MarketState::Normal,
            reference_price: None,
            last_trade_price: None,
            reference_sources: HashMap::new(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            orders: HashMap::new(),
            user_orders: HashMap::new(),
            session_orders: HashMap::new(),
            recent_events: VecDeque::new(),
        }
    }

    fn from_snapshot(snapshot: MarketRuntimeSnapshot, instrument: &InstrumentSpec) -> Self {
        let mut market = Self {
            market_id: snapshot.market_id,
            outcome: snapshot.outcome,
            state: snapshot.state,
            reference_price: snapshot.reference_price,
            last_trade_price: snapshot.last_trade_price,
            reference_sources: snapshot
                .reference_sources
                .into_iter()
                .map(|item| (item.source.clone(), item))
                .collect(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            orders: HashMap::new(),
            user_orders: HashMap::new(),
            session_orders: HashMap::new(),
            recent_events: VecDeque::new(),
        };

        for order in snapshot.orders {
            insert_resting_order(&mut market, RestingOrder::from_snapshot(order, instrument));
        }

        market
    }

    fn export_snapshot(&self) -> MarketRuntimeSnapshot {
        let mut orders = Vec::with_capacity(self.orders.len());
        for queue in self.bids.values() {
            for order_id in queue {
                if let Some(order) = self.orders.get(order_id) {
                    orders.push(order.export_snapshot());
                }
            }
        }
        for queue in self.asks.values() {
            for order_id in queue {
                if let Some(order) = self.orders.get(order_id) {
                    orders.push(order.export_snapshot());
                }
            }
        }

        MarketRuntimeSnapshot {
            market_id: self.market_id.clone(),
            outcome: self.outcome,
            state: self.state,
            reference_price: self.reference_price,
            last_trade_price: self.last_trade_price,
            reference_sources: self.reference_sources.values().cloned().collect(),
            orders,
        }
    }

    fn snapshot(&self) -> MarketSnapshot {
        let (recent_new_orders, recent_cancel_events) =
            summarize_recent_events(&self.recent_events);
        MarketSnapshot {
            market_id: self.market_id.clone(),
            outcome: self.outcome,
            state: self.state,
            reference_price: self.reference_price,
            last_trade_price: self.last_trade_price,
            best_bid: self.best_bid(),
            best_ask: self.best_ask(),
            open_orders: self.orders.len(),
            recent_new_orders,
            recent_cancel_events,
        }
    }

    fn best_bid(&self) -> Option<i64> {
        self.bids.keys().next_back().copied()
    }

    fn best_ask(&self) -> Option<i64> {
        self.asks.keys().next().copied()
    }

    fn index_order(&mut self, order: &RestingOrder) {
        self.user_orders
            .entry(order.user_id.clone())
            .or_default()
            .insert(order.order_id.clone());
        if let Some(session_id) = &order.session_id {
            self.session_orders
                .entry(session_id.clone())
                .or_default()
                .insert(order.order_id.clone());
        }
    }

    fn remove_order_indexes(&mut self, order_id: &str, user_id: &str, session_id: Option<&str>) {
        if let Some(order_ids) = self.user_orders.get_mut(user_id) {
            order_ids.remove(order_id);
            if order_ids.is_empty() {
                self.user_orders.remove(user_id);
            }
        }
        if let Some(session_id) = session_id {
            if let Some(order_ids) = self.session_orders.get_mut(session_id) {
                order_ids.remove(order_id);
                if order_ids.is_empty() {
                    self.session_orders.remove(session_id);
                }
            }
        }
    }

    fn remove_from_book(&mut self, order: &RestingOrder) {
        let levels = if order.side == Side::Buy {
            &mut self.bids
        } else {
            &mut self.asks
        };
        if let Some(queue) = levels.get_mut(&order.price) {
            queue.retain(|id| id != &order.order_id);
            if queue.is_empty() {
                levels.remove(&order.price);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct RestingOrder {
    order_id: String,
    request_id: String,
    command_seq: Option<u64>,
    user_id: String,
    session_id: Option<String>,
    market_id: String,
    outcome: i32,
    side: Side,
    price: i64,
    order_type: OrderType,
    time_in_force: TimeInForce,
    post_only: bool,
    reduce_only: bool,
    leverage: Option<u32>,
    original_amount: i64,
    remaining_amount: i64,
    expires_at: Option<DateTime<Utc>>,
    reserved_cash: i64,
    reserved_position: i64,
}

#[derive(Debug, Default)]
struct MatchOutcome {
    fills: Vec<Fill>,
    aborted: Option<SubmissionError>,
}

impl RestingOrder {
    fn from_new_order(command: NewOrderCommand) -> Self {
        let limit_price = command.price.unwrap_or(0);
        Self {
            order_id: command.client_order_id,
            request_id: command.metadata.request_id,
            command_seq: command.metadata.command_seq,
            user_id: command.user_id,
            session_id: command.session_id,
            market_id: command.market_id,
            outcome: command.outcome,
            side: command.side,
            price: limit_price,
            order_type: command.order_type,
            time_in_force: command.time_in_force,
            post_only: command.post_only,
            reduce_only: command.reduce_only,
            leverage: command.leverage,
            original_amount: command.amount,
            remaining_amount: command.amount,
            expires_at: command.expires_at,
            reserved_cash: 0,
            reserved_position: 0,
        }
    }

    fn from_snapshot(snapshot: RestingOrderSnapshot, instrument: &InstrumentSpec) -> Self {
        let instrument_kind = instrument.kind;
        let reserved_cash = match instrument_kind {
            InstrumentKind::Spot => {
                if snapshot.side == Side::Buy && snapshot.order_type == OrderType::Limit {
                    snapshot.price.saturating_mul(snapshot.remaining_amount)
                } else {
                    0
                }
            }
            InstrumentKind::Margin | InstrumentKind::Perpetual => {
                let notional = snapshot.price.saturating_mul(snapshot.remaining_amount);
                required_margin(notional, snapshot.leverage.unwrap_or(1)).unwrap_or(0)
            }
        };
        let reserved_position =
            if instrument_kind == InstrumentKind::Spot && snapshot.side == Side::Sell {
                snapshot.remaining_amount
            } else {
                0
            };
        Self {
            order_id: snapshot.order_id,
            request_id: snapshot.request_id,
            command_seq: snapshot.command_seq,
            user_id: snapshot.user_id,
            session_id: snapshot.session_id,
            market_id: snapshot.market_id,
            outcome: snapshot.outcome,
            side: snapshot.side,
            price: snapshot.price,
            order_type: snapshot.order_type,
            time_in_force: snapshot.time_in_force,
            post_only: snapshot.post_only,
            reduce_only: snapshot.reduce_only,
            leverage: snapshot.leverage,
            original_amount: snapshot.original_amount,
            remaining_amount: snapshot.remaining_amount,
            expires_at: snapshot.expires_at,
            reserved_cash,
            reserved_position,
        }
    }

    fn export_snapshot(&self) -> RestingOrderSnapshot {
        RestingOrderSnapshot {
            order_id: self.order_id.clone(),
            request_id: self.request_id.clone(),
            command_seq: self.command_seq,
            user_id: self.user_id.clone(),
            session_id: self.session_id.clone(),
            market_id: self.market_id.clone(),
            outcome: self.outcome,
            side: self.side,
            price: self.price,
            order_type: self.order_type,
            time_in_force: self.time_in_force,
            post_only: self.post_only,
            reduce_only: self.reduce_only,
            leverage: self.leverage,
            original_amount: self.original_amount,
            remaining_amount: self.remaining_amount,
            expires_at: self.expires_at,
        }
    }

    fn crosses_price(&self, resting_price: i64) -> bool {
        match self.side {
            Side::Buy => self.order_type == OrderType::Market || self.price >= resting_price,
            Side::Sell => self.order_type == OrderType::Market || self.price <= resting_price,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RecentMarketEventKind {
    NewOrder,
    Cancel,
}

#[derive(Debug, Clone, Copy)]
struct RecentMarketEvent {
    at: Instant,
    kind: RecentMarketEventKind,
    weight: usize,
}

fn insert_resting_order(market: &mut MarketRuntime, order: RestingOrder) {
    market.index_order(&order);
    market.orders.insert(order.order_id.clone(), order.clone());
    let queue = if order.side == Side::Buy {
        market.bids.entry(order.price).or_default()
    } else {
        market.asks.entry(order.price).or_default()
    };
    queue.push_back(order.order_id);
}

fn build_replacement_order_command(
    existing: &RestingOrder,
    command: ReplaceOrderCommand,
) -> NewOrderCommand {
    NewOrderCommand {
        metadata: command.metadata,
        client_order_id: command
            .new_client_order_id
            .unwrap_or_else(types::generate_id),
        user_id: command.user_id,
        session_id: existing.session_id.clone(),
        market_id: existing.market_id.clone(),
        side: existing.side,
        order_type: existing.order_type,
        time_in_force: command.new_time_in_force.unwrap_or(existing.time_in_force),
        price: command.new_price.or(Some(existing.price)),
        amount: command.new_amount.unwrap_or(existing.remaining_amount),
        outcome: existing.outcome,
        post_only: command.post_only.unwrap_or(existing.post_only),
        reduce_only: command.reduce_only.unwrap_or(existing.reduce_only),
        leverage: command.new_leverage.or(existing.leverage),
        expires_at: command.new_expires_at.or(existing.expires_at),
    }
}

fn skipped_new_order_result(
    command: &NewOrderCommand,
    market_state: MarketState,
) -> SubmitOrderResult {
    SubmitOrderResult {
        metadata: command.metadata.clone(),
        order_id: command.client_order_id.clone(),
        market_state,
        fills: Vec::new(),
        state: OrderState::Active,
        remaining_amount: command.amount,
    }
}

fn skipped_replace_order_result(
    command: &ReplaceOrderCommand,
    market_state: MarketState,
) -> SubmitOrderResult {
    SubmitOrderResult {
        metadata: command.metadata.clone(),
        order_id: command
            .new_client_order_id
            .clone()
            .unwrap_or_else(|| command.order_id.clone()),
        market_state,
        fills: Vec::new(),
        state: OrderState::Active,
        remaining_amount: command.new_amount.unwrap_or_default(),
    }
}

fn skipped_cancel_result(metadata: CommandMetadata, market_state: MarketState) -> CancelResult {
    CancelResult {
        metadata,
        market_state,
        cancelled_order_ids: Vec::new(),
    }
}

fn validate_order_acceptance(
    market: &mut MarketRuntime,
    config: &PartitionedEngineConfig,
    risk: &RiskEngine,
    instrument: &InstrumentSpec,
    command: &NewOrderCommand,
    replaced_order: Option<&RestingOrder>,
) -> Result<(), SubmissionError> {
    let instrument_kind = instrument.kind;
    let policy = policy_for_instrument_kind(instrument_kind);
    policy
        .validate_order(&risk.context_for_instrument(instrument.clone()), command)
        .map_err(risk_error_to_submission)?;
    if command.reduce_only && command.side == Side::Buy {
        return Err(SubmissionError::InvalidOrder(
            "reduce-only buy is not supported",
        ));
    }

    if matches!(market.state, MarketState::CancelOnly | MarketState::Halted) {
        return Err(SubmissionError::MarketClosed {
            market_id: market.market_id.clone(),
            outcome: market.outcome,
            state: market.state,
        });
    }

    let order_id = command.client_order_id.clone();
    if market.orders.get(&order_id).is_some_and(|existing| {
        replaced_order.is_none_or(|replaced| existing.order_id != replaced.order_id)
    }) {
        return Err(SubmissionError::DuplicateOrderId(order_id));
    }

    let mut user_open_orders = market
        .user_orders
        .get(&command.user_id)
        .map_or(0, |set| set.len());
    if replaced_order.is_some_and(|order| order.user_id == command.user_id && user_open_orders > 0)
    {
        user_open_orders -= 1;
    }
    if user_open_orders >= config.max_open_orders_per_user {
        return Err(SubmissionError::InvalidOrder(
            "user open order limit exceeded",
        ));
    }

    if let Some(price) = command.price {
        apply_price_band_guard(market, config, price)?;
    }
    if command.post_only && crosses_book(market, command) {
        return Err(SubmissionError::InvalidOrder(
            "post-only order would take liquidity",
        ));
    }
    let available_cash =
        available_cash_with_replace_credit(risk, instrument, command, replaced_order);
    let market_estimate = if command.order_type == OrderType::Market {
        Some(estimate_market_execution(
            market,
            instrument,
            command,
            (command.side == Side::Buy).then_some(available_cash),
        )?)
    } else {
        None
    };
    if let Some(estimate) = market_estimate {
        if let Some(terminal_price) = estimate.terminal_price {
            apply_price_band_guard(market, config, terminal_price)?;
        }
        if command.side == Side::Buy
            && estimate.executable_amount == 0
            && market.best_ask().is_some()
        {
            return Err(SubmissionError::Ledger(
                "insufficient available cash".to_string(),
            ));
        }
        if command.time_in_force == TimeInForce::Fok && estimate.executable_amount < command.amount
        {
            return Err(SubmissionError::InsufficientLiquidityForFok);
        }
        if instrument_kind != InstrumentKind::Spot {
            let leverage = normalized_command_leverage(instrument, command)?.unwrap_or(1);
            let required = estimate.required_reserve;
            if required > available_cash {
                return Err(SubmissionError::Ledger(
                    "insufficient available margin".to_string(),
                ));
            }
            if estimate.executable_amount > 0 && leverage == 0 {
                return Err(SubmissionError::InvalidOrder(
                    "invalid leverage for leveraged market",
                ));
            }
        }
    } else if command.time_in_force == TimeInForce::Fok && !can_fully_fill(market, command) {
        return Err(SubmissionError::InsufficientLiquidityForFok);
    }

    preflight_order_reservation_capacity(risk, instrument, command, replaced_order)?;

    if command.reduce_only && command.side == Side::Sell {
        let already_reserved = reserved_sell_amount_excluding(
            market,
            instrument_kind,
            &command.user_id,
            replaced_order.map(|order| order.order_id.as_str()),
        );
        risk.ensure_reduce_only_sell_capacity(
            instrument_kind,
            &command.user_id,
            &command.market_id,
            command.outcome,
            command.amount,
            already_reserved,
        )
        .map_err(|error| match error {
            RiskError::InsufficientReduceOnlyPosition => {
                SubmissionError::InvalidOrder("reduce-only sell exceeds net position")
            }
            RiskError::OperationFailed(reason) => SubmissionError::Ledger(reason),
        })?;
    }

    let incoming = RestingOrder::from_new_order(command.clone());
    if would_self_trade(market, &incoming) {
        return Err(SubmissionError::SelfTradePrevented(
            incoming.order_id.clone(),
        ));
    }

    Ok(())
}

fn preflight_order_reservation_capacity(
    risk: &RiskEngine,
    instrument: &InstrumentSpec,
    command: &NewOrderCommand,
    replaced_order: Option<&RestingOrder>,
) -> Result<(), SubmissionError> {
    let instrument_kind = instrument.kind;
    match (instrument_kind, command.side, command.order_type) {
        (InstrumentKind::Spot, Side::Buy, OrderType::Limit) => {
            let notional = command
                .price
                .unwrap_or_default()
                .checked_mul(command.amount)
                .ok_or(SubmissionError::InvalidOrder("price*amount overflow"))?;
            let released_cash = replaced_order
                .filter(|order| order.side == Side::Buy)
                .map(|order| order.reserved_cash)
                .unwrap_or(0);
            let available_cash = risk
                .available_cash(&command.user_id)
                .saturating_add(released_cash);
            if notional > available_cash {
                return Err(SubmissionError::Ledger(
                    "insufficient available cash".to_string(),
                ));
            }
        }
        (InstrumentKind::Spot, Side::Sell, _) => {
            let released_position = replaced_order
                .filter(|order| order.side == Side::Sell)
                .map(|order| order.reserved_position)
                .unwrap_or(0);
            let available_position = risk
                .available_position(&command.user_id, &command.market_id, command.outcome)
                .saturating_add(released_position);
            if command.amount > available_position {
                return Err(SubmissionError::Ledger(
                    "insufficient available position".to_string(),
                ));
            }
        }
        (InstrumentKind::Margin | InstrumentKind::Perpetual, _, OrderType::Limit) => {
            let leverage = normalized_command_leverage(instrument, command)?.unwrap_or(1);
            let policy = policy_for_instrument_kind(instrument_kind);
            let reserve_decision = policy
                .reserve_requirement(&risk.context_for_instrument(instrument.clone()), command)
                .map_err(risk_error_to_submission)?;
            let notional = command
                .price
                .unwrap_or_default()
                .checked_mul(command.amount)
                .ok_or(SubmissionError::InvalidOrder("price*amount overflow"))?;
            let required_cash = reserve_decision
                .reserve_cash
                .max(required_margin(notional, leverage)?);
            let released_cash = replaced_order.map(|order| order.reserved_cash).unwrap_or(0);
            let available_cash = risk
                .available_cash(&command.user_id)
                .saturating_add(released_cash);
            if required_cash > available_cash {
                return Err(SubmissionError::Ledger(
                    "insufficient available margin".to_string(),
                ));
            }
        }
        _ => {}
    }

    Ok(())
}

fn available_cash_with_replace_credit(
    risk: &RiskEngine,
    instrument: &InstrumentSpec,
    command: &NewOrderCommand,
    replaced_order: Option<&RestingOrder>,
) -> i64 {
    let instrument_kind = instrument.kind;
    let released_cash = replaced_order
        .filter(|order| instrument_kind != InstrumentKind::Spot || order.side == Side::Buy)
        .map(|order| order.reserved_cash)
        .unwrap_or(0);
    risk.available_cash(&command.user_id)
        .saturating_add(released_cash)
}

fn crosses_book(market: &MarketRuntime, command: &NewOrderCommand) -> bool {
    match command.side {
        Side::Buy => match (market.best_ask(), limit_price(command)) {
            (Some(best_ask), Some(price)) => price >= best_ask,
            (Some(_), None) => true,
            _ => false,
        },
        Side::Sell => match (market.best_bid(), limit_price(command)) {
            (Some(best_bid), Some(price)) => price <= best_bid,
            (Some(_), None) => true,
            _ => false,
        },
    }
}

fn can_fully_fill(market: &MarketRuntime, command: &NewOrderCommand) -> bool {
    let mut remaining = command.amount;
    match command.side {
        Side::Buy => {
            for (price, queue) in &market.asks {
                if let Some(limit) = limit_price(command) {
                    if *price > limit {
                        break;
                    }
                }
                for order_id in queue {
                    if let Some(order) = market.orders.get(order_id) {
                        remaining -= order.remaining_amount;
                        if remaining <= 0 {
                            return true;
                        }
                    }
                }
            }
        }
        Side::Sell => {
            for (price, queue) in market.bids.iter().rev() {
                if let Some(limit) = limit_price(command) {
                    if *price < limit {
                        break;
                    }
                }
                for order_id in queue {
                    if let Some(order) = market.orders.get(order_id) {
                        remaining -= order.remaining_amount;
                        if remaining <= 0 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    remaining <= 0
}

#[derive(Debug, Default, Clone, Copy)]
struct MarketExecutionEstimate {
    executable_amount: i64,
    executable_notional: i64,
    required_reserve: i64,
    terminal_price: Option<i64>,
}

fn estimate_market_execution(
    market: &MarketRuntime,
    instrument: &InstrumentSpec,
    command: &NewOrderCommand,
    cash_budget: Option<i64>,
) -> Result<MarketExecutionEstimate, SubmissionError> {
    let mut estimate = MarketExecutionEstimate::default();
    let mut remaining_amount = command.amount;
    let mut remaining_cash = cash_budget.unwrap_or(i64::MAX);
    let instrument_kind = instrument.kind;
    let leverage = normalized_command_leverage(instrument, command)?.unwrap_or(1);

    match command.side {
        Side::Buy => {
            'price_levels: for (price, queue) in &market.asks {
                if let Some(limit) = limit_price(command) {
                    if *price > limit {
                        break;
                    }
                }
                if cash_budget.is_some()
                    && instrument_kind == InstrumentKind::Spot
                    && remaining_cash < *price
                {
                    break;
                }
                for order_id in queue {
                    if remaining_amount <= 0 {
                        break 'price_levels;
                    }
                    let Some(order) = market.orders.get(order_id) else {
                        continue;
                    };
                    let mut executable_amount = order.remaining_amount.min(remaining_amount);
                    if cash_budget.is_some() {
                        executable_amount = executable_amount.min(match instrument_kind {
                            InstrumentKind::Spot => remaining_cash / *price,
                            InstrumentKind::Margin | InstrumentKind::Perpetual => {
                                (((remaining_cash as i128) * (leverage as i128)) / (*price as i128))
                                    .clamp(0, i64::MAX as i128)
                                    as i64
                            }
                        });
                    }
                    if executable_amount <= 0 {
                        break 'price_levels;
                    }
                    let notional = price
                        .checked_mul(executable_amount)
                        .ok_or(SubmissionError::InvalidOrder("price*amount overflow"))?;
                    estimate.executable_amount += executable_amount;
                    estimate.executable_notional += notional;
                    estimate.required_reserve += match instrument_kind {
                        InstrumentKind::Spot => notional,
                        InstrumentKind::Margin | InstrumentKind::Perpetual => {
                            required_margin(notional, leverage)?
                        }
                    };
                    estimate.terminal_price = Some(*price);
                    remaining_amount -= executable_amount;
                    remaining_cash = remaining_cash.saturating_sub(match instrument_kind {
                        InstrumentKind::Spot => notional,
                        InstrumentKind::Margin | InstrumentKind::Perpetual => {
                            required_margin(notional, leverage)?
                        }
                    });
                }
            }
        }
        Side::Sell => {
            'price_levels: for (price, queue) in market.bids.iter().rev() {
                if let Some(limit) = limit_price(command) {
                    if *price < limit {
                        break;
                    }
                }
                for order_id in queue {
                    if remaining_amount <= 0 {
                        break 'price_levels;
                    }
                    let Some(order) = market.orders.get(order_id) else {
                        continue;
                    };
                    let executable_amount = order.remaining_amount.min(remaining_amount);
                    if executable_amount <= 0 {
                        continue;
                    }
                    let notional = price
                        .checked_mul(executable_amount)
                        .ok_or(SubmissionError::InvalidOrder("price*amount overflow"))?;
                    estimate.executable_amount += executable_amount;
                    estimate.executable_notional += notional;
                    estimate.required_reserve += match instrument_kind {
                        InstrumentKind::Spot => 0,
                        InstrumentKind::Margin | InstrumentKind::Perpetual => {
                            required_margin(notional, leverage)?
                        }
                    };
                    estimate.terminal_price = Some(*price);
                    remaining_amount -= executable_amount;
                }
            }
        }
    }

    Ok(estimate)
}

fn market_buy_budget(
    market: &MarketRuntime,
    risk: &RiskEngine,
    instrument: &InstrumentSpec,
    command: &NewOrderCommand,
    replaced_order: Option<&RestingOrder>,
) -> Result<i64, SubmissionError> {
    let available_cash =
        available_cash_with_replace_credit(risk, instrument, command, replaced_order);
    let estimate = estimate_market_execution(market, instrument, command, Some(available_cash))?;
    Ok(estimate.required_reserve)
}

fn would_self_trade(market: &MarketRuntime, incoming: &RestingOrder) -> bool {
    match incoming.side {
        Side::Buy => market
            .asks
            .iter()
            .take_while(|(price, _)| incoming.crosses_price(**price))
            .flat_map(|(_, queue)| queue.iter())
            .filter_map(|order_id| market.orders.get(order_id))
            .any(|resting| resting.user_id == incoming.user_id),
        Side::Sell => market
            .bids
            .iter()
            .rev()
            .take_while(|(price, _)| incoming.crosses_price(**price))
            .flat_map(|(_, queue)| queue.iter())
            .filter_map(|order_id| market.orders.get(order_id))
            .any(|resting| resting.user_id == incoming.user_id),
    }
}

fn reserved_sell_amount_excluding(
    market: &MarketRuntime,
    instrument_kind: InstrumentKind,
    user_id: &str,
    excluded_order_id: Option<&str>,
) -> i64 {
    market
        .user_orders
        .get(user_id)
        .map(|order_ids| {
            order_ids
                .iter()
                .filter(|order_id| {
                    excluded_order_id.is_none_or(|excluded| excluded != order_id.as_str())
                })
                .filter_map(|order_id| market.orders.get(order_id))
                .filter(|order| {
                    order.side == Side::Sell
                        && (instrument_kind == InstrumentKind::Spot || order.reduce_only)
                })
                .map(|order| order.remaining_amount)
                .sum()
        })
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn match_incoming(
    market: &mut MarketRuntime,
    incoming: &mut RestingOrder,
    instrument: &InstrumentSpec,
    config: &PartitionedEngineConfig,
    event_bus: &EventBus,
    risk: &RiskEngine,
    trade_store: Option<&dyn WalStore<TradeJournalRecord>>,
    seen_trade_ids: &mut HashSet<String>,
    partition_id: usize,
) -> Result<MatchOutcome, SubmissionError> {
    let mut outcome = MatchOutcome::default();
    let mut fill_index = 0usize;
    let instrument_kind = instrument.kind;
    let policy = policy_for_instrument_kind(instrument_kind);
    loop {
        if incoming.remaining_amount == 0 {
            break;
        }
        let Some(best_price) = best_crossing_price(market, incoming) else {
            break;
        };
        let resting_side = opposite_side(incoming.side);
        let resting_order_id = {
            let levels = if resting_side == Side::Buy {
                &market.bids
            } else {
                &market.asks
            };
            let Some(queue) = levels.get(&best_price) else {
                break;
            };
            let Some(resting_order_id) = queue.front() else {
                break;
            };
            resting_order_id.clone()
        };
        let Some(mut resting) = market.orders.get(&resting_order_id).cloned() else {
            continue;
        };
        if resting.user_id == incoming.user_id {
            break;
        }

        let mut executed_amount = incoming.remaining_amount.min(resting.remaining_amount);
        if incoming.order_type == OrderType::Market {
            executed_amount = executed_amount.min(match instrument_kind {
                InstrumentKind::Spot if incoming.side == Side::Buy => {
                    incoming.reserved_cash / best_price
                }
                InstrumentKind::Spot => i64::MAX,
                InstrumentKind::Margin | InstrumentKind::Perpetual => {
                    (((incoming.reserved_cash as i128) * (order_leverage(incoming) as i128))
                        / (best_price as i128))
                        .clamp(0, i64::MAX as i128) as i64
                }
            });
            if executed_amount <= 0 {
                break;
            }
        }
        let trade_id = trade_id_for_fill(incoming, partition_id, fill_index);
        fill_index = fill_index.saturating_add(1);
        let buy_intent_id = if incoming.side == Side::Buy {
            incoming.order_id.clone()
        } else {
            resting.order_id.clone()
        };
        let buy_user_id = if incoming.side == Side::Buy {
            incoming.user_id.clone()
        } else {
            resting.user_id.clone()
        };
        let buy_market_id = if incoming.side == Side::Buy {
            incoming.market_id.clone()
        } else {
            resting.market_id.clone()
        };
        let buy_outcome = if incoming.side == Side::Buy {
            incoming.outcome
        } else {
            resting.outcome
        };
        let sell_intent_id = if incoming.side == Side::Sell {
            incoming.order_id.clone()
        } else {
            resting.order_id.clone()
        };
        let sell_user_id = if incoming.side == Side::Sell {
            incoming.user_id.clone()
        } else {
            resting.user_id.clone()
        };
        let sell_market_id = if incoming.side == Side::Sell {
            incoming.market_id.clone()
        } else {
            resting.market_id.clone()
        };
        let sell_outcome = if incoming.side == Side::Sell {
            incoming.outcome
        } else {
            resting.outcome
        };
        let Some(notional) = best_price.checked_mul(executed_amount) else {
            market.state = MarketState::Halted;
            outcome.aborted = Some(SubmissionError::Ledger(
                "trade notional overflow".to_string(),
            ));
            break;
        };
        let settlement_decision = policy
            .settlement_decision(
                &risk.context_for_instrument(instrument.clone()),
                &FillIntent {
                    buy_user_id: buy_user_id.clone(),
                    sell_user_id: sell_user_id.clone(),
                    market_id: market.market_id.clone(),
                    outcome: market.outcome,
                    price: best_price,
                    amount: executed_amount,
                },
                incoming
                    .side
                    .eq(&Side::Buy)
                    .then_some(order_leverage(incoming))
                    .or(resting
                        .side
                        .eq(&Side::Buy)
                        .then_some(order_leverage(&resting))),
                incoming
                    .side
                    .eq(&Side::Sell)
                    .then_some(order_leverage(incoming))
                    .or(resting
                        .side
                        .eq(&Side::Sell)
                        .then_some(order_leverage(&resting))),
            )
            .map_err(risk_error_to_submission)?;
        let settle_op = trade_settle_op_id(&trade_id);
        let rollback_settle_op = rollback_settle_op_id(&trade_id);
        let settlement_result = if settlement_decision.use_spot_settlement {
            risk.settle_trade(
                &buy_user_id,
                &sell_user_id,
                &market.market_id,
                market.outcome,
                best_price,
                executed_amount,
                &settle_op,
            )
        } else {
            risk.settle_derivative_trade(
                &buy_user_id,
                &sell_user_id,
                &market.market_id,
                market.outcome,
                executed_amount,
                &settle_op,
            )
        };
        if let Err(error) = settlement_result {
            tracing::error!("trade settlement failed: {}", error);
            market.state = MarketState::Halted;
            outcome.aborted = Some(SubmissionError::Ledger(error.to_string()));
            break;
        }
        if let Some(store) = trade_store {
            let record = TradeJournalRecord {
                partition_id,
                trade_id: trade_id.clone(),
                market_id: market.market_id.clone(),
                outcome: market.outcome,
                buy_order_id: buy_intent_id.clone(),
                buy_user_id: buy_user_id.clone(),
                sell_order_id: sell_intent_id.clone(),
                sell_user_id: sell_user_id.clone(),
                price: best_price,
                amount: executed_amount,
                recorded_at: Utc::now(),
            };
            if !seen_trade_ids.contains(&trade_id) {
                if let Err(error) = store.append(&record) {
                    tracing::error!("trade journal append failed: {}", error);
                    if let Err(rollback_error) = rollback_trade_settlement(
                        instrument_kind,
                        risk,
                        &buy_user_id,
                        &sell_user_id,
                        &market.market_id,
                        market.outcome,
                        best_price,
                        executed_amount,
                        &rollback_settle_op,
                    ) {
                        tracing::error!("trade settlement rollback failed: {}", rollback_error);
                        market.state = MarketState::Halted;
                        outcome.aborted = Some(rollback_error);
                        break;
                    }
                    market.state = MarketState::Halted;
                    outcome.aborted = Some(SubmissionError::Persistence(error.to_string()));
                    break;
                }
                seen_trade_ids.insert(trade_id.clone());
            }
        }
        incoming.remaining_amount -= executed_amount;
        resting.remaining_amount -= executed_amount;
        match instrument_kind {
            InstrumentKind::Spot => {
                if incoming.side == Side::Buy {
                    incoming.reserved_cash = incoming.reserved_cash.saturating_sub(notional);
                } else {
                    incoming.reserved_position =
                        incoming.reserved_position.saturating_sub(executed_amount);
                }
                if resting.side == Side::Buy {
                    resting.reserved_cash = resting.reserved_cash.saturating_sub(notional);
                } else {
                    resting.reserved_position =
                        resting.reserved_position.saturating_sub(executed_amount);
                }
            }
            InstrumentKind::Margin | InstrumentKind::Perpetual => {
                let incoming_consumed = if incoming.side == Side::Buy {
                    settlement_decision.reserve_consumed_buy
                } else {
                    settlement_decision.reserve_consumed_sell
                };
                let resting_consumed = if resting.side == Side::Buy {
                    settlement_decision.reserve_consumed_buy
                } else {
                    settlement_decision.reserve_consumed_sell
                };
                incoming.reserved_cash = incoming.reserved_cash.saturating_sub(incoming_consumed);
                resting.reserved_cash = resting.reserved_cash.saturating_sub(resting_consumed);
            }
        }
        market.last_trade_price = Some(best_price);
        market.orders.remove(&resting_order_id);
        market.remove_from_book(&resting);
        let buy_fill = Fill {
            id: trade_id.clone(),
            intent_id: buy_intent_id,
            user_id: buy_user_id,
            market_id: buy_market_id,
            side: Side::Buy,
            price: best_price,
            amount: executed_amount,
            outcome: buy_outcome,
            timestamp: chrono::Utc::now(),
            op_id: format!("trade_buy_{trade_id}"),
        };
        let sell_fill = Fill {
            id: trade_id.clone(),
            intent_id: sell_intent_id,
            user_id: sell_user_id,
            market_id: sell_market_id,
            side: Side::Sell,
            price: best_price,
            amount: executed_amount,
            outcome: sell_outcome,
            timestamp: chrono::Utc::now(),
            op_id: format!("trade_sell_{trade_id}"),
        };
        event_bus.publish(Event::FillCreated(buy_fill.clone()));
        event_bus.publish(Event::FillCreated(sell_fill.clone()));
        outcome.fills.push(buy_fill);
        outcome.fills.push(sell_fill);

        if resting.remaining_amount > 0 {
            insert_resting_front(market, resting);
        } else {
            let _ = release_order_reservation(risk, instrument, &resting, "filled");
            market.remove_order_indexes(
                &resting.order_id,
                &resting.user_id,
                resting.session_id.as_deref(),
            );
        }

        apply_trade_price_guard(market, config, best_price);
        if market.state == MarketState::Halted {
            break;
        }
    }
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
fn rollback_trade_settlement(
    instrument_kind: InstrumentKind,
    risk: &RiskEngine,
    buy_user_id: &str,
    sell_user_id: &str,
    market_id: &str,
    outcome: i32,
    price: i64,
    amount: i64,
    op_id: &str,
) -> Result<(), SubmissionError> {
    let entries = match instrument_kind {
        InstrumentKind::Spot => {
            let notional = price
                .checked_mul(amount)
                .ok_or(SubmissionError::InvalidOrder("price*amount overflow"))?;
            vec![
                LedgerEntry {
                    debit_account: LedgerService::cash_account(sell_user_id),
                    credit_account: LedgerService::cash_hold_account(buy_user_id),
                    amount: notional,
                    op_id: format!("{op_id}:cash"),
                    timestamp: Utc::now(),
                },
                LedgerEntry {
                    debit_account: LedgerService::position_account(buy_user_id, market_id, outcome),
                    credit_account: LedgerService::position_hold_account(
                        sell_user_id,
                        market_id,
                        outcome,
                    ),
                    amount,
                    op_id: format!("{op_id}:position"),
                    timestamp: Utc::now(),
                },
            ]
        }
        InstrumentKind::Margin | InstrumentKind::Perpetual => vec![LedgerEntry {
            debit_account: LedgerService::derivative_position_account(
                buy_user_id,
                market_id,
                outcome,
            ),
            credit_account: LedgerService::derivative_position_account(
                sell_user_id,
                market_id,
                outcome,
            ),
            amount,
            op_id: format!("{op_id}:deriv"),
            timestamp: Utc::now(),
        }],
    };
    risk.ledger()
        .commit_delta(LedgerDelta {
            op_id: op_id.to_string(),
            entries,
            timestamp: Utc::now(),
        })
        .map_err(|error| SubmissionError::Ledger(error.to_string()))
}

fn insert_resting_front(market: &mut MarketRuntime, order: RestingOrder) {
    let order_id = order.order_id.clone();
    let side = order.side;
    let price = order.price;
    market.orders.insert(order_id.clone(), order);
    let queue = if side == Side::Buy {
        market.bids.entry(price).or_default()
    } else {
        market.asks.entry(price).or_default()
    };
    queue.push_front(order_id);
}

fn order_idempotency_key(order: &RestingOrder) -> String {
    order
        .command_seq
        .map(|seq| format!("seq-{seq}"))
        .or_else(|| {
            (!order.request_id.trim().is_empty()).then(|| format!("req-{}", order.request_id))
        })
        .unwrap_or_else(|| format!("order-{}", order.order_id))
}

fn trade_id_for_fill(incoming: &RestingOrder, partition_id: usize, fill_index: usize) -> String {
    format!(
        "trade:{}:{}:{}",
        order_idempotency_key(incoming),
        partition_id,
        fill_index
    )
}

fn trade_settle_op_id(trade_id: &str) -> String {
    format!("settle_{trade_id}")
}

fn rollback_settle_op_id(trade_id: &str) -> String {
    format!("rollback_settle_{trade_id}")
}

fn compact_seen_trade_ids(seen_trade_ids: &mut HashSet<String>, snapshot_seq: u64) {
    seen_trade_ids
        .retain(|trade_id| parse_command_seq_token(trade_id).is_none_or(|seq| seq > snapshot_seq));
}

fn parse_command_seq_token(value: &str) -> Option<u64> {
    let marker = "seq-";
    let start = value.find(marker)? + marker.len();
    let digits = value[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn reserve_cash_op_id(order: &RestingOrder, reason: &str) -> String {
    format!("reserve_cash_{reason}_{}", order_idempotency_key(order))
}

fn reserve_position_op_id(order: &RestingOrder, reason: &str) -> String {
    format!("reserve_pos_{reason}_{}", order_idempotency_key(order))
}

fn release_cash_op_id(order: &RestingOrder, reason: &str) -> String {
    format!("release_cash_{reason}_{}", order_idempotency_key(order))
}

fn release_position_op_id(order: &RestingOrder, reason: &str) -> String {
    format!("release_pos_{reason}_{}", order_idempotency_key(order))
}

fn best_crossing_price(market: &MarketRuntime, incoming: &RestingOrder) -> Option<i64> {
    match incoming.side {
        Side::Buy => market
            .best_ask()
            .filter(|price| incoming.crosses_price(*price)),
        Side::Sell => market
            .best_bid()
            .filter(|price| incoming.crosses_price(*price)),
    }
}

fn reserve_order_reservation(
    risk: &RiskEngine,
    instrument: &InstrumentSpec,
    order: &mut RestingOrder,
    reason: &str,
) -> Result<types::RiskReserveIds, SubmissionError> {
    match instrument.kind {
        InstrumentKind::Spot => match order.side {
            Side::Buy => {
                let reserve_amount = if order.order_type == OrderType::Limit {
                    order
                        .price
                        .checked_mul(order.remaining_amount)
                        .ok_or(SubmissionError::InvalidOrder("price*amount overflow"))?
                } else {
                    order.reserved_cash
                };
                if reserve_amount > 0 {
                    let reserve_ids = risk
                        .reserve_buy(
                            &order.user_id,
                            reserve_amount,
                            &reserve_cash_op_id(order, reason),
                        )
                        .map_err(|error| SubmissionError::Ledger(error.to_string()))?;
                    order.reserved_cash = reserve_amount;
                    return Ok(reserve_ids);
                }
            }
            Side::Sell => {
                if order.remaining_amount > 0 {
                    let reserve_ids = risk
                        .reserve_sell(
                            &order.user_id,
                            &order.market_id,
                            order.outcome,
                            order.remaining_amount,
                            &reserve_position_op_id(order, reason),
                        )
                        .map_err(|error| SubmissionError::Ledger(error.to_string()))?;
                    order.reserved_position = order
                        .reserved_position
                        .saturating_add(order.remaining_amount);
                    return Ok(reserve_ids);
                }
            }
        },
        InstrumentKind::Margin | InstrumentKind::Perpetual => {
            let reserve_amount = if order.order_type == OrderType::Limit {
                let notional = order
                    .price
                    .checked_mul(order.remaining_amount)
                    .ok_or(SubmissionError::InvalidOrder("price*amount overflow"))?;
                required_margin(notional, order_leverage(order))?
            } else {
                order.reserved_cash
            };
            if reserve_amount > 0 {
                let reserve_ids = risk
                    .reserve_margin(
                        &order.user_id,
                        reserve_amount,
                        &reserve_cash_op_id(order, reason),
                    )
                    .map_err(|error| SubmissionError::Ledger(error.to_string()))?;
                order.reserved_cash = reserve_amount;
                return Ok(reserve_ids);
            }
        }
    }
    Ok(types::RiskReserveIds::default())
}

fn release_order_reservation(
    risk: &RiskEngine,
    instrument: &InstrumentSpec,
    order: &RestingOrder,
    reason: &str,
) -> Result<(), SubmissionError> {
    if order.reserved_cash > 0 {
        match instrument.kind {
            InstrumentKind::Spot => risk
                .release_buy(
                    &order.user_id,
                    order.reserved_cash,
                    &release_cash_op_id(order, reason),
                )
                .map_err(|error| SubmissionError::Ledger(error.to_string()))?,
            InstrumentKind::Margin | InstrumentKind::Perpetual => risk
                .release_margin(
                    &order.user_id,
                    order.reserved_cash,
                    &release_cash_op_id(order, reason),
                )
                .map_err(|error| SubmissionError::Ledger(error.to_string()))?,
        }
    }
    if order.reserved_position > 0 {
        risk.release_sell(
            &order.user_id,
            &order.market_id,
            order.outcome,
            order.reserved_position,
            &release_position_op_id(order, reason),
        )
        .map_err(|error| SubmissionError::Ledger(error.to_string()))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cancel_orders(
    markets: &mut HashMap<MarketKey, MarketRuntime>,
    config: &PartitionedEngineConfig,
    risk: &RiskEngine,
    instruments: &dyn InstrumentRegistry,
    market_id: Option<&str>,
    outcome: Option<i32>,
    specific_order_id: Option<&str>,
    session_id: Option<&str>,
    user_id: Option<&str>,
) -> Result<Vec<String>, SubmissionError> {
    let keys: Vec<_> = markets
        .keys()
        .filter(|key| market_id.is_none_or(|value| value == key.market_id))
        .filter(|key| outcome.is_none_or(|value| value == key.outcome))
        .cloned()
        .collect();

    let mut cancelled = Vec::new();
    for key in keys {
        let Some(market) = markets.get_mut(&key) else {
            continue;
        };
        evict_stale_events(market, config.cancel_window);
        let target_ids: Vec<String> = if let Some(order_id) = specific_order_id {
            vec![order_id.to_string()]
        } else if let Some(session_id) = session_id {
            market
                .session_orders
                .get(session_id)
                .map(|ids| ids.iter().cloned().collect())
                .unwrap_or_default()
        } else if let Some(user_id) = user_id {
            market
                .user_orders
                .get(user_id)
                .map(|ids| ids.iter().cloned().collect())
                .unwrap_or_default()
        } else {
            market.orders.keys().cloned().collect()
        };

        let mut cancelled_in_market = 0usize;
        for order_id in target_ids {
            let Some(order) = market.orders.remove(&order_id) else {
                continue;
            };
            if user_id.is_some_and(|expected| expected != order.user_id) {
                market.orders.insert(order.order_id.clone(), order);
                continue;
            }
            if session_id.is_some_and(|expected| order.session_id.as_deref() != Some(expected)) {
                market.orders.insert(order.order_id.clone(), order);
                continue;
            }
            market.remove_from_book(&order);
            market.remove_order_indexes(
                &order.order_id,
                &order.user_id,
                order.session_id.as_deref(),
            );
            let instrument = instruments.resolve(&order.market_id);
            release_order_reservation(risk, &instrument, &order, "cancel")?;
            cancelled.push(order.order_id.clone());
            cancelled_in_market += 1;
        }

        if cancelled_in_market > 0 {
            record_recent_event(
                market,
                config,
                RecentMarketEventKind::Cancel,
                cancelled_in_market,
            );
            apply_cancel_guard(market, config);
        }
    }

    if let Some(order_id) = specific_order_id {
        if cancelled.is_empty() {
            return Err(SubmissionError::OrderNotFound(order_id.to_string()));
        }
    }

    Ok(cancelled)
}

fn record_recent_event(
    market: &mut MarketRuntime,
    config: &PartitionedEngineConfig,
    kind: RecentMarketEventKind,
    weight: usize,
) {
    evict_stale_events(market, config.cancel_window);
    market.recent_events.push_back(RecentMarketEvent {
        at: Instant::now(),
        kind,
        weight,
    });
}

fn evict_stale_events(market: &mut MarketRuntime, window: Duration) {
    let now = Instant::now();
    while market
        .recent_events
        .front()
        .is_some_and(|event| now.duration_since(event.at) > window)
    {
        market.recent_events.pop_front();
    }
}

fn summarize_recent_events(events: &VecDeque<RecentMarketEvent>) -> (usize, usize) {
    let mut new_orders = 0;
    let mut cancel_events = 0;
    for event in events {
        match event.kind {
            RecentMarketEventKind::NewOrder => new_orders += event.weight,
            RecentMarketEventKind::Cancel => cancel_events += event.weight,
        }
    }
    (new_orders, cancel_events)
}

fn apply_cancel_guard(market: &mut MarketRuntime, config: &PartitionedEngineConfig) {
    let (new_orders, cancel_events) = summarize_recent_events(&market.recent_events);
    if cancel_events < config.min_cancel_events_before_guard {
        return;
    }
    let ratio = if new_orders == 0 {
        cancel_events as f64
    } else {
        cancel_events as f64 / new_orders as f64
    };
    if ratio >= config.max_cancel_to_new_ratio {
        market.state = combine_market_state(market.state, MarketState::CancelOnly);
    }
}

fn apply_price_band_guard(
    market: &mut MarketRuntime,
    config: &PartitionedEngineConfig,
    attempted_price: i64,
) -> Result<(), SubmissionError> {
    let Some(reference_price) = market.reference_price else {
        return Ok(());
    };
    let deviation_bps = deviation_bps(reference_price, attempted_price);
    if deviation_bps >= config.halt_price_band_bps {
        market.state = MarketState::Halted;
        return Err(SubmissionError::PriceBandBreached {
            market_id: market.market_id.clone(),
            outcome: market.outcome,
            state: market.state,
            reference_price,
            attempted_price,
            deviation_bps,
        });
    }
    if deviation_bps >= config.cancel_only_price_band_bps {
        market.state = combine_market_state(market.state, MarketState::CancelOnly);
        return Err(SubmissionError::PriceBandBreached {
            market_id: market.market_id.clone(),
            outcome: market.outcome,
            state: market.state,
            reference_price,
            attempted_price,
            deviation_bps,
        });
    }
    Ok(())
}

fn apply_trade_price_guard(
    market: &mut MarketRuntime,
    config: &PartitionedEngineConfig,
    trade_price: i64,
) {
    let Some(reference_price) = market.reference_price else {
        return;
    };
    let deviation = deviation_bps(reference_price, trade_price);
    if deviation >= config.halt_price_band_bps {
        market.state = MarketState::Halted;
    } else if deviation >= config.cancel_only_price_band_bps {
        market.state = combine_market_state(market.state, MarketState::CancelOnly);
    }
}

fn calculate_snapshot_checksum(
    partition_id: usize,
    kill_switch_enabled: bool,
    snapshot: &PartitionStateSnapshot,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    partition_id.hash(&mut hasher);
    kill_switch_enabled.hash(&mut hasher);
    serde_json::to_string(snapshot)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

fn validate_snapshot_record(record: &PartitionSnapshotRecord) -> AnyhowResult<()> {
    if record.snapshot_version != SNAPSHOT_VERSION {
        return Err(anyhow!(
            "unsupported snapshot_version {} (expected {})",
            record.snapshot_version,
            SNAPSHOT_VERSION
        ));
    }
    let expected = calculate_snapshot_checksum(
        record.partition_id,
        record.kill_switch_enabled,
        &record.snapshot,
    );
    if expected != record.snapshot_checksum {
        return Err(anyhow!(
            "snapshot checksum mismatch for partition {}",
            record.partition_id
        ));
    }
    Ok(())
}

fn validate_new_order(command: &NewOrderCommand) -> Result<(), SubmissionError> {
    if command.amount <= 0 {
        return Err(SubmissionError::InvalidOrder("amount must be positive"));
    }
    if matches!(command.order_type, OrderType::Limit) && command.price.is_none() {
        return Err(SubmissionError::InvalidOrder(
            "limit order price is required",
        ));
    }
    if let Some(price) = command.price {
        if price <= 0 {
            return Err(SubmissionError::InvalidOrder("price must be positive"));
        }
    }
    if command.time_in_force == TimeInForce::Gtd && command.expires_at.is_none() {
        return Err(SubmissionError::InvalidOrder(
            "gtd order requires expires_at",
        ));
    }
    if let Some(expires_at) = command.expires_at {
        if expires_at <= command.metadata.received_at {
            return Err(SubmissionError::InvalidOrder(
                "expires_at must be after request receipt time",
            ));
        }
    }
    Ok(())
}

const MAX_LEVERAGE: u32 = 20;

fn normalize_leverage(
    instrument: &InstrumentSpec,
    leverage: Option<u32>,
) -> Result<Option<u32>, SubmissionError> {
    match instrument.kind {
        InstrumentKind::Spot => {
            if leverage.is_some() {
                Err(SubmissionError::InvalidOrder(
                    "spot market does not accept leverage",
                ))
            } else {
                Ok(None)
            }
        }
        InstrumentKind::Margin | InstrumentKind::Perpetual => {
            let leverage = leverage.unwrap_or(1);
            let max_leverage = instrument.max_leverage.unwrap_or(MAX_LEVERAGE);
            if !(1..=max_leverage).contains(&leverage) {
                return Err(SubmissionError::InvalidOrder(
                    "invalid leverage for leveraged market",
                ));
            }
            Ok(Some(leverage))
        }
    }
}

fn normalized_command_leverage(
    instrument: &InstrumentSpec,
    command: &NewOrderCommand,
) -> Result<Option<u32>, SubmissionError> {
    normalize_leverage(instrument, command.leverage)
}

fn order_leverage(order: &RestingOrder) -> u32 {
    order.leverage.unwrap_or(1)
}

fn risk_error_to_submission(error: RiskError) -> SubmissionError {
    match error {
        RiskError::InsufficientReduceOnlyPosition => {
            SubmissionError::InvalidOrder("reduce-only sell exceeds net position")
        }
        RiskError::OperationFailed(reason) => match reason.as_str() {
            "amount must be positive" => SubmissionError::InvalidOrder("amount must be positive"),
            "price must be positive" => SubmissionError::InvalidOrder("price must be positive"),
            "invalid leverage" => SubmissionError::InvalidOrder("invalid leverage"),
            "leverage exceeds instrument maximum" => {
                SubmissionError::InvalidOrder("leverage exceeds instrument maximum")
            }
            "spot orders do not support leverage" => {
                SubmissionError::InvalidOrder("spot orders do not support leverage")
            }
            _ => SubmissionError::Ledger(reason),
        },
    }
}

fn required_margin(notional: i64, leverage: u32) -> Result<i64, SubmissionError> {
    if notional < 0 {
        return Err(SubmissionError::InvalidOrder("negative notional"));
    }
    let leverage = leverage.max(1) as i64;
    Ok((notional.saturating_add(leverage - 1)) / leverage)
}

fn limit_price(command: &NewOrderCommand) -> Option<i64> {
    match command.order_type {
        OrderType::Limit => command.price,
        OrderType::Market => None,
    }
}

fn deviation_bps(reference_price: i64, attempted_price: i64) -> i64 {
    ((attempted_price - reference_price).abs() * 10_000) / reference_price.max(1)
}

fn opposite_side(side: Side) -> Side {
    match side {
        Side::Buy => Side::Sell,
        Side::Sell => Side::Buy,
    }
}

fn aggregate_market_state(markets: &HashMap<MarketKey, MarketRuntime>) -> MarketState {
    markets
        .values()
        .map(|market| market.state)
        .max_by_key(|state| market_state_rank(*state))
        .unwrap_or(MarketState::Normal)
}

fn combine_market_state(lhs: MarketState, rhs: MarketState) -> MarketState {
    if market_state_rank(lhs) >= market_state_rank(rhs) {
        lhs
    } else {
        rhs
    }
}

fn market_state_rank(state: MarketState) -> usize {
    match state {
        MarketState::Normal => 0,
        MarketState::Stress => 1,
        MarketState::AuctionCall => 2,
        MarketState::CancelOnly => 3,
        MarketState::Halted => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use ledger::LedgerService;
    use parking_lot::Mutex;
    use persistence::{InMemoryWal, WalStore};
    use risk::RiskEngine;
    use types::{
        CancelOrderCommand, Command, CommandLifecycle, CommandMetadata, LedgerDelta, OrderType,
        ReplaceOrderCommand, Side, TimeInForce,
    };

    fn config() -> PartitionedEngineConfig {
        PartitionedEngineConfig {
            partitions: 1,
            queue_capacity: 64,
            snapshot_interval_commands: 1,
            max_open_orders_per_user: 16,
            cancel_window: Duration::from_secs(30),
            max_cancel_to_new_ratio: 1.0,
            min_cancel_events_before_guard: 2,
            cancel_only_price_band_bps: 500,
            halt_price_band_bps: 1_000,
        }
    }

    fn config_with_partitions(partitions: usize) -> PartitionedEngineConfig {
        let mut config = config();
        config.partitions = partitions;
        config
    }

    #[allow(clippy::too_many_arguments)]
    fn new_order_with_outcome(
        request_id: &str,
        client_order_id: &str,
        user_id: &str,
        session_id: Option<&str>,
        side: Side,
        price: i64,
        amount: i64,
        outcome: i32,
    ) -> NewOrderCommand {
        let mut command = new_order(
            request_id,
            client_order_id,
            user_id,
            session_id,
            side,
            price,
            amount,
        );
        command.outcome = outcome;
        command
    }

    fn new_order(
        request_id: &str,
        client_order_id: &str,
        user_id: &str,
        session_id: Option<&str>,
        side: Side,
        price: i64,
        amount: i64,
    ) -> NewOrderCommand {
        NewOrderCommand {
            metadata: CommandMetadata::new(request_id),
            client_order_id: client_order_id.to_string(),
            user_id: user_id.to_string(),
            session_id: session_id.map(str::to_string),
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

    #[allow(clippy::too_many_arguments)]
    fn leveraged_order(
        request_id: &str,
        client_order_id: &str,
        user_id: &str,
        side: Side,
        market_id: &str,
        price: i64,
        amount: i64,
        leverage: u32,
    ) -> NewOrderCommand {
        let mut command = new_order(
            request_id,
            client_order_id,
            user_id,
            None,
            side,
            price,
            amount,
        );
        command.market_id = market_id.to_string();
        command.leverage = Some(leverage);
        command
    }

    fn seeded_ledger() -> Arc<LedgerService> {
        seeded_ledger_with_wal(Arc::new(InMemoryWal::new()))
    }

    fn seeded_ledger_with_wal(wal_store: Arc<dyn WalStore<LedgerDelta>>) -> Arc<LedgerService> {
        let ledger = Arc::new(LedgerService::with_wal_store(EventBus::new(), wal_store));
        for user in ["maker-1", "maker-2", "taker", "u-1", "u-2"] {
            ledger
                .process_deposit(user, 1_000_000, format!("deposit_{user}"))
                .unwrap();
            ledger
                .process_position_deposit(user, "btc-usdt", 0, 1_000, format!("position_{user}_0"))
                .unwrap();
            ledger
                .process_position_deposit(user, "btc-usdt", 7, 1_000, format!("position_{user}_7"))
                .unwrap();
        }
        ledger
    }

    fn seeded_risk() -> Arc<RiskEngine> {
        Arc::new(RiskEngine::new(seeded_ledger()))
    }

    fn seeded_risk_with_ledger(ledger: Arc<LedgerService>) -> Arc<RiskEngine> {
        Arc::new(RiskEngine::new(ledger))
    }

    #[tokio::test]
    async fn spot_market_rejects_leverage() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        let error = engine
            .submit_new_order(leveraged_order(
                "req-1",
                "spot-lev-1",
                "maker-1",
                Side::Buy,
                "btc-usdt",
                100,
                5,
                5,
            ))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            SubmissionError::InvalidOrder("spot market does not accept leverage")
        );
    }

    #[tokio::test]
    async fn margin_limit_order_reserves_initial_margin_only() {
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), risk);

        engine
            .submit_new_order(leveraged_order(
                "req-1",
                "margin-bid-1",
                "maker-1",
                Side::Buy,
                "margin:btc-usdt",
                100,
                10,
                10,
            ))
            .await
            .unwrap();

        assert_eq!(ledger.cash_hold_balance("maker-1"), 100);
        let snapshot = engine
            .export_snapshots()
            .await
            .unwrap()
            .into_iter()
            .flat_map(|record| record.snapshot.markets.into_iter())
            .find(|market| market.market_id == "margin:btc-usdt" && market.outcome == 0)
            .unwrap();
        assert_eq!(snapshot.orders.len(), 1);
        assert_eq!(snapshot.orders[0].leverage, Some(10));
    }

    #[tokio::test]
    async fn margin_short_fill_creates_negative_derivative_position() {
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), risk);

        engine
            .submit_new_order(leveraged_order(
                "req-1",
                "margin-ask-1",
                "maker-1",
                Side::Sell,
                "margin:btc-usdt",
                100,
                5,
                5,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(leveraged_order(
                "req-2",
                "margin-bid-1",
                "taker",
                Side::Buy,
                "margin:btc-usdt",
                100,
                5,
                5,
            ))
            .await
            .unwrap();

        assert_eq!(
            ledger.derivative_position_balance("maker-1", "margin:btc-usdt", 0),
            -5
        );
        assert_eq!(
            ledger.derivative_position_balance("taker", "margin:btc-usdt", 0),
            5
        );
    }

    #[tokio::test]
    async fn perpetual_fill_updates_derivative_positions() {
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), risk);

        engine
            .submit_new_order(leveraged_order(
                "req-1",
                "perp-ask-1",
                "maker-1",
                Side::Sell,
                "perp:btc-usdt",
                100,
                3,
                3,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(leveraged_order(
                "req-2",
                "perp-bid-1",
                "taker",
                Side::Buy,
                "perp:btc-usdt",
                100,
                3,
                3,
            ))
            .await
            .unwrap();

        assert_eq!(
            ledger.derivative_position_balance("maker-1", "perp:btc-usdt", 0),
            -3
        );
        assert_eq!(
            ledger.derivative_position_balance("taker", "perp:btc-usdt", 0),
            3
        );
    }

    #[tokio::test]
    async fn replace_preserves_existing_leverage_when_not_overridden() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        engine
            .submit_new_order(leveraged_order(
                "req-1",
                "margin-bid-1",
                "maker-1",
                Side::Buy,
                "margin:btc-usdt",
                100,
                5,
                4,
            ))
            .await
            .unwrap();

        engine
            .replace_order(ReplaceOrderCommand {
                metadata: CommandMetadata::new("req-2"),
                user_id: "maker-1".to_string(),
                market_id: "margin:btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "margin-bid-1".to_string(),
                new_client_order_id: Some("margin-bid-1r".to_string()),
                new_price: Some(101),
                new_amount: Some(5),
                new_time_in_force: Some(TimeInForce::Gtc),
                post_only: Some(false),
                reduce_only: Some(false),
                new_leverage: None,
                new_expires_at: None,
            })
            .await
            .unwrap();

        let snapshot = engine
            .export_snapshots()
            .await
            .unwrap()
            .into_iter()
            .flat_map(|record| record.snapshot.markets.into_iter())
            .find(|market| market.market_id == "margin:btc-usdt" && market.outcome == 0)
            .unwrap();
        assert_eq!(snapshot.orders.len(), 1);
        assert_eq!(snapshot.orders[0].leverage, Some(4));
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

    #[derive(Default)]
    struct FailingTradeWal {
        entries: Mutex<Vec<TradeJournalRecord>>,
        fail: bool,
    }

    impl FailingTradeWal {
        fn always_fail() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
                fail: true,
            }
        }
    }

    impl WalStore<TradeJournalRecord> for FailingTradeWal {
        fn append(&self, record: &TradeJournalRecord) -> anyhow::Result<()> {
            if self.fail {
                return Err(anyhow!(
                    "forced trade journal failure for {}",
                    record.trade_id
                ));
            }
            self.entries.lock().push(record.clone());
            Ok(())
        }

        fn entries(&self) -> anyhow::Result<Vec<TradeJournalRecord>> {
            Ok(self.entries.lock().clone())
        }
    }

    fn with_command_seq(mut command: NewOrderCommand, seq: u64) -> NewOrderCommand {
        command.metadata.command_seq = Some(seq);
        command.metadata.lifecycle = CommandLifecycle::WalAppended;
        command
    }

    fn partition_for(market_id: &str, outcome: i32, partitions: usize) -> usize {
        let mut hasher = DefaultHasher::new();
        market_id.hash(&mut hasher);
        outcome.hash(&mut hasher);
        (hasher.finish() as usize) % partitions
    }

    fn find_distinct_outcomes(partitions: usize) -> (i32, i32) {
        for lhs in 0..256 {
            for rhs in (lhs + 1)..256 {
                if partition_for("btc-usdt", lhs, partitions)
                    != partition_for("btc-usdt", rhs, partitions)
                {
                    return (lhs, rhs);
                }
            }
        }
        panic!("failed to find outcomes in different partitions");
    }

    #[tokio::test]
    async fn price_time_priority_prefers_oldest_resting_order() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-2",
                "ask-2",
                "maker-2",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();
        let result = engine
            .submit_new_order(new_order(
                "req-3",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                7,
            ))
            .await
            .unwrap();

        let sell_fills: Vec<_> = result
            .fills
            .iter()
            .filter(|fill| fill.side == Side::Sell)
            .map(|fill| fill.intent_id.clone())
            .collect();
        assert_eq!(sell_fills, vec!["ask-1".to_string(), "ask-2".to_string()]);

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
    }

    #[tokio::test]
    async fn mass_cancel_by_user_removes_all_resting_orders() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(new_order(
                "req-1",
                "bid-1",
                "u-1",
                Some("s-1"),
                Side::Buy,
                99,
                5,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-2",
                "bid-2",
                "u-1",
                Some("s-1"),
                Side::Buy,
                98,
                5,
            ))
            .await
            .unwrap();
        let result = engine
            .mass_cancel_by_user(MassCancelByUserCommand {
                metadata: CommandMetadata::new("req-3"),
                user_id: "u-1".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(result.cancelled_order_ids.len(), 2);
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 0);
    }

    #[tokio::test]
    async fn cancel_storm_switches_market_to_cancel_only() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(new_order(
                "req-1",
                "bid-1",
                "u-1",
                Some("s-1"),
                Side::Buy,
                99,
                5,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-2",
                "bid-2",
                "u-2",
                Some("s-2"),
                Side::Buy,
                98,
                5,
            ))
            .await
            .unwrap();
        engine
            .mass_cancel_by_user(MassCancelByUserCommand {
                metadata: CommandMetadata::new("req-3"),
                user_id: "u-1".to_string(),
            })
            .await
            .unwrap();
        engine
            .mass_cancel_by_user(MassCancelByUserCommand {
                metadata: CommandMetadata::new("req-4"),
                user_id: "u-2".to_string(),
            })
            .await
            .unwrap();
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.state, MarketState::CancelOnly);
    }

    #[tokio::test]
    async fn extreme_deviation_halts_market() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .update_reference_price("btc-usdt", 0, "manual", 100)
            .await
            .unwrap();
        let error = engine
            .submit_new_order(new_order("req-1", "bid-1", "u-1", None, Side::Buy, 120, 5))
            .await
            .unwrap_err();
        match error {
            SubmissionError::PriceBandBreached { state, .. } => {
                assert_eq!(state, MarketState::Halted)
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn admin_kill_switch_rejects_new_orders() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_admin(AdminCommand {
                metadata: CommandMetadata::new("req-admin"),
                actor_id: "ops-1".to_string(),
                action: AdminAction::KillSwitch { enabled: true },
            })
            .await
            .unwrap();
        let error = engine
            .submit_new_order(new_order("req-1", "bid-1", "u-1", None, Side::Buy, 100, 5))
            .await
            .unwrap_err();
        assert_eq!(error, SubmissionError::KillSwitchActive);
        assert!(engine.kill_switch_enabled());
    }

    #[tokio::test]
    async fn snapshot_store_recovers_fifo_ordering() {
        let snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        let risk = seeded_risk();
        let engine = PartitionedMatchingEngine::with_snapshot_store(
            config(),
            EventBus::new(),
            risk.clone(),
            snapshot_store.clone(),
        )
        .unwrap();

        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-2",
                "ask-2",
                "maker-2",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();

        let recovered = PartitionedMatchingEngine::with_snapshot_store(
            config(),
            EventBus::new(),
            risk,
            snapshot_store.clone(),
        )
        .unwrap();

        let result = recovered
            .submit_new_order(new_order(
                "req-3",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                7,
            ))
            .await
            .unwrap();

        let sell_fills: Vec<_> = result
            .fills
            .iter()
            .filter(|fill| fill.side == Side::Sell)
            .map(|fill| fill.intent_id.clone())
            .collect();

        assert_eq!(sell_fills, vec!["ask-1".to_string(), "ask-2".to_string()]);
    }

    #[tokio::test]
    async fn cancel_order_broadcast_finds_nonzero_outcome_partition() {
        let engine = PartitionedMatchingEngine::new(
            config_with_partitions(4),
            EventBus::new(),
            seeded_risk(),
        );

        engine
            .submit_new_order(new_order_with_outcome(
                "req-1",
                "bid-outcome-7",
                "u-1",
                Some("s-1"),
                Side::Buy,
                99,
                5,
                7,
            ))
            .await
            .unwrap();

        let result = engine
            .cancel_order(CancelOrderCommand {
                metadata: CommandMetadata::new("req-cancel"),
                user_id: "u-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(7),
                order_id: "bid-outcome-7".to_string(),
                client_order_id: None,
            })
            .await
            .unwrap();

        assert_eq!(
            result.cancelled_order_ids,
            vec!["bid-outcome-7".to_string()]
        );
        let snapshot = engine
            .snapshot_market("btc-usdt", 7)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 0);
    }

    #[tokio::test]
    async fn self_trade_prevention_rejects_taker() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-self",
                "u-1",
                Some("s-1"),
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();

        let error = engine
            .submit_new_order(new_order(
                "req-2",
                "bid-self",
                "u-1",
                Some("s-1"),
                Side::Buy,
                100,
                5,
            ))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            SubmissionError::SelfTradePrevented("bid-self".to_string())
        );
    }

    #[tokio::test]
    async fn replace_order_loses_priority() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-2",
                "ask-2",
                "maker-2",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();

        engine
            .replace_order(ReplaceOrderCommand {
                metadata: CommandMetadata::new("req-3"),
                user_id: "maker-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "ask-1".to_string(),
                new_client_order_id: Some("ask-1r".to_string()),
                new_price: Some(100),
                new_amount: Some(5),
                new_time_in_force: Some(TimeInForce::Gtc),
                post_only: Some(false),
                reduce_only: Some(false),
                new_leverage: None,
                new_expires_at: None,
            })
            .await
            .unwrap();

        let result = engine
            .submit_new_order(new_order(
                "req-4",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                10,
            ))
            .await
            .unwrap();
        let sell_fills: Vec<_> = result
            .fills
            .iter()
            .filter(|fill| fill.side == Side::Sell)
            .map(|fill| fill.intent_id.clone())
            .collect();

        assert_eq!(sell_fills, vec!["ask-2".to_string(), "ask-1r".to_string()]);
    }

    #[tokio::test]
    async fn replace_order_invalid_new_order_keeps_existing_order() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();

        let error = engine
            .replace_order(ReplaceOrderCommand {
                metadata: CommandMetadata::new("req-2"),
                user_id: "maker-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "ask-1".to_string(),
                new_client_order_id: Some("ask-1r".to_string()),
                new_price: Some(100),
                new_amount: Some(0),
                new_time_in_force: Some(TimeInForce::Gtc),
                post_only: Some(false),
                reduce_only: Some(false),
                new_leverage: None,
                new_expires_at: None,
            })
            .await
            .unwrap_err();

        assert_eq!(
            error,
            SubmissionError::InvalidOrder("amount must be positive")
        );
        let result = engine
            .submit_new_order(new_order(
                "req-3",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                5,
            ))
            .await
            .unwrap();
        let sell_fills: Vec<_> = result
            .fills
            .iter()
            .filter(|fill| fill.side == Side::Sell)
            .map(|fill| fill.intent_id.clone())
            .collect();
        assert_eq!(sell_fills, vec!["ask-1".to_string()]);
    }

    #[tokio::test]
    async fn replace_order_risk_failure_keeps_existing_hold_and_order() {
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), risk);
        engine
            .submit_new_order(new_order(
                "req-1",
                "bid-1",
                "maker-1",
                None,
                Side::Buy,
                100,
                5,
            ))
            .await
            .unwrap();

        let initial_hold = ledger.cash_hold_balance("maker-1");
        let error = engine
            .replace_order(ReplaceOrderCommand {
                metadata: CommandMetadata::new("req-2"),
                user_id: "maker-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "bid-1".to_string(),
                new_client_order_id: Some("bid-1r".to_string()),
                new_price: Some(10_000),
                new_amount: Some(1_000_000),
                new_time_in_force: Some(TimeInForce::Gtc),
                post_only: Some(false),
                reduce_only: Some(false),
                new_leverage: None,
                new_expires_at: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, SubmissionError::Ledger(_)));
        assert_eq!(ledger.cash_hold_balance("maker-1"), initial_hold);
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
    }

    #[tokio::test]
    async fn replace_order_price_band_failure_keeps_existing_order() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();
        engine
            .update_reference_price("btc-usdt", 0, "manual", 100)
            .await
            .unwrap();

        let error = engine
            .replace_order(ReplaceOrderCommand {
                metadata: CommandMetadata::new("req-2"),
                user_id: "maker-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "ask-1".to_string(),
                new_client_order_id: Some("ask-1r".to_string()),
                new_price: Some(200),
                new_amount: Some(5),
                new_time_in_force: Some(TimeInForce::Gtc),
                post_only: Some(false),
                reduce_only: Some(false),
                new_leverage: None,
                new_expires_at: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, SubmissionError::PriceBandBreached { .. }));
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
    }

    #[tokio::test]
    async fn replay_skips_partitions_with_newer_snapshot_cursor() {
        let snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        let risk = seeded_risk();
        let (outcome_a, outcome_b) = find_distinct_outcomes(2);
        let engine = PartitionedMatchingEngine::with_snapshot_store(
            config_with_partitions(2),
            EventBus::new(),
            risk.clone(),
            snapshot_store.clone(),
        )
        .unwrap();

        engine
            .submit_new_order(with_command_seq(
                new_order_with_outcome(
                    "req-a",
                    "bid-a",
                    "maker-1",
                    None,
                    Side::Buy,
                    100,
                    5,
                    outcome_a,
                ),
                100,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(with_command_seq(
                new_order_with_outcome(
                    "req-b",
                    "bid-b",
                    "maker-2",
                    None,
                    Side::Buy,
                    100,
                    5,
                    outcome_b,
                ),
                90,
            ))
            .await
            .unwrap();

        let recovered = PartitionedMatchingEngine::with_snapshot_store(
            config_with_partitions(2),
            EventBus::new(),
            risk,
            snapshot_store,
        )
        .unwrap();

        recovered
            .replay_command(Command::NewOrder(with_command_seq(
                new_order_with_outcome(
                    "req-old-a",
                    "bid-a-old",
                    "maker-1",
                    None,
                    Side::Buy,
                    101,
                    1,
                    outcome_a,
                ),
                95,
            )))
            .await
            .unwrap();
        recovered
            .replay_command(Command::NewOrder(with_command_seq(
                new_order_with_outcome(
                    "req-old-b",
                    "bid-b-old",
                    "maker-2",
                    None,
                    Side::Buy,
                    101,
                    1,
                    outcome_b,
                ),
                95,
            )))
            .await
            .unwrap();

        let snapshot_a = recovered
            .snapshot_market("btc-usdt", outcome_a)
            .await
            .unwrap()
            .unwrap();
        let snapshot_b = recovered
            .snapshot_market("btc-usdt", outcome_b)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot_a.open_orders, 1);
        assert_eq!(snapshot_b.open_orders, 2);
    }

    #[tokio::test]
    async fn settlement_failure_keeps_resting_order_and_halts_market() {
        let wal = Arc::new(FailingLedgerWal::new("settle_"));
        let ledger = seeded_ledger_with_wal(wal);
        let risk = seeded_risk_with_ledger(ledger.clone());
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), risk);

        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();
        let error = engine
            .submit_new_order(new_order(
                "req-2",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                5,
            ))
            .await
            .unwrap_err();

        assert!(matches!(error, SubmissionError::Ledger(_)));
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
        assert_eq!(snapshot.state, MarketState::Halted);
        assert_eq!(ledger.cash_available_balance("maker-1"), 1_000_000);
        assert_eq!(ledger.position_hold_balance("maker-1", "btc-usdt", 0), 5);
    }

    #[tokio::test]
    async fn trade_journal_failure_rolls_back_settlement_and_keeps_book_consistent() {
        let ledger = seeded_ledger();
        let risk = seeded_risk_with_ledger(ledger.clone());
        let trade_store: Arc<dyn WalStore<TradeJournalRecord>> =
            Arc::new(FailingTradeWal::always_fail());
        let engine = PartitionedMatchingEngine::with_stores(
            config(),
            EventBus::new(),
            risk,
            None,
            Some(trade_store),
        )
        .unwrap();

        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();
        let error = engine
            .submit_new_order(new_order(
                "req-2",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                5,
            ))
            .await
            .unwrap_err();

        assert!(matches!(error, SubmissionError::Persistence(_)));
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
        assert_eq!(snapshot.state, MarketState::Halted);
        assert_eq!(ledger.cash_available_balance("maker-1"), 1_000_000);
        assert_eq!(
            ledger.position_available_balance("taker", "btc-usdt", 0),
            1_000
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn inflight_queue_depth_stays_bounded_under_load() {
        let cfg = PartitionedEngineConfig {
            partitions: 1,
            queue_capacity: 8,
            snapshot_interval_commands: usize::MAX,
            ..config()
        };
        let engine = Arc::new(PartitionedMatchingEngine::new(
            cfg,
            EventBus::new(),
            seeded_risk(),
        ));
        let mut tasks = Vec::new();
        for index in 0..128usize {
            let engine = engine.clone();
            tasks.push(tokio::spawn(async move {
                let _ = engine
                    .submit_new_order(new_order(
                        &format!("req-{index}"),
                        &format!("bid-{index}"),
                        "u-1",
                        Some("s-1"),
                        Side::Buy,
                        99,
                        1,
                    ))
                    .await;
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }

        let depth = engine.queue_depths().into_iter().next().unwrap();
        assert!(depth.inflight <= depth.capacity);
    }

    #[tokio::test]
    async fn reused_client_order_id_does_not_dedupe_distinct_trades() {
        let trade_store = Arc::new(InMemoryWal::<TradeJournalRecord>::new());
        let engine = PartitionedMatchingEngine::with_stores(
            config(),
            EventBus::new(),
            seeded_risk(),
            None,
            Some(trade_store.clone()),
        )
        .unwrap();

        engine
            .submit_new_order(new_order(
                "req-ask-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                2,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-buy-1",
                "reused-order-id",
                "taker",
                None,
                Side::Buy,
                100,
                2,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-ask-2",
                "ask-2",
                "maker-1",
                None,
                Side::Sell,
                100,
                3,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-buy-2",
                "reused-order-id",
                "taker",
                None,
                Side::Buy,
                100,
                3,
            ))
            .await
            .unwrap();

        let trades = trade_store.entries().unwrap();
        assert_eq!(trades.len(), 2);
        assert_ne!(trades[0].trade_id, trades[1].trade_id);
    }

    #[tokio::test]
    async fn market_buy_rejects_when_best_offer_exceeds_available_cash() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        engine
            .submit_new_order(new_order(
                "req-ask-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                1_000_001,
                1,
            ))
            .await
            .unwrap();

        let mut market_buy = new_order("req-buy-1", "mkt-buy-1", "taker", None, Side::Buy, 1, 1);
        market_buy.order_type = OrderType::Market;
        market_buy.price = None;

        let error = engine.submit_new_order(market_buy).await.unwrap_err();
        assert!(matches!(error, SubmissionError::Ledger(_)));

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.state, MarketState::Normal);
        assert_eq!(snapshot.open_orders, 1);
    }

    #[tokio::test]
    async fn market_buy_price_band_breach_is_rejected_pre_trade() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(new_order(
                "req-ask-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                200,
                1,
            ))
            .await
            .unwrap();
        engine
            .update_reference_price("btc-usdt", 0, "manual", 100)
            .await
            .unwrap();

        let mut market_buy = new_order("req-buy-1", "mkt-buy-1", "taker", None, Side::Buy, 1, 1);
        market_buy.order_type = OrderType::Market;
        market_buy.price = None;

        let error = engine.submit_new_order(market_buy).await.unwrap_err();
        assert!(matches!(error, SubmissionError::PriceBandBreached { .. }));

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
        assert_eq!(snapshot.state, MarketState::Halted);
    }

    #[tokio::test]
    async fn replay_does_not_duplicate_trade_journal_entries() {
        let snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        let trade_store = Arc::new(InMemoryWal::<TradeJournalRecord>::new());
        let risk = seeded_risk();
        let engine = PartitionedMatchingEngine::with_stores(
            config(),
            EventBus::new(),
            risk.clone(),
            Some(snapshot_store.clone()),
            Some(trade_store.clone()),
        )
        .unwrap();

        engine
            .submit_new_order(with_command_seq(
                new_order("req-ask-1", "ask-1", "maker-1", None, Side::Sell, 100, 1),
                1,
            ))
            .await
            .unwrap();

        let stale_snapshot = snapshot_store.entries().unwrap().last().cloned().unwrap();

        engine
            .submit_new_order(with_command_seq(
                new_order("req-buy-1", "buy-1", "taker", None, Side::Buy, 100, 1),
                2,
            ))
            .await
            .unwrap();

        assert_eq!(trade_store.entries().unwrap().len(), 1);

        let stale_snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        stale_snapshot_store.append(&stale_snapshot).unwrap();

        let recovered = PartitionedMatchingEngine::with_stores(
            config(),
            EventBus::new(),
            risk,
            Some(stale_snapshot_store),
            Some(trade_store.clone()),
        )
        .unwrap();

        recovered
            .replay_command(Command::NewOrder(with_command_seq(
                new_order("req-buy-1", "buy-1", "taker", None, Side::Buy, 100, 1),
                2,
            )))
            .await
            .unwrap();

        let trades = trade_store.entries().unwrap();
        assert_eq!(trades.len(), 1);
        let snapshot = recovered
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 0);
    }
}
