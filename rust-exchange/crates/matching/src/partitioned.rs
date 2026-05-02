use anyhow::{anyhow, Result as AnyhowResult};
use chrono::{DateTime, Utc};
use eventbus::EventBus;
use instruments::{shared_default_registry, InstrumentRegistry};
use persistence::WalStore;
use risk::{policy_for_instrument_kind, FillIntent, RiskEngine, RiskError};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use types::{
    AdminAction, AdminCommand, AuthenticatedPrincipal, CancelOrderCommand, Command,
    CommandLifecycle, CommandMetadata, Event, Fill, InstrumentKind, InstrumentSpec, MarketState,
    MassCancelByMarketCommand, MassCancelBySessionCommand, MassCancelByUserCommand,
    NewOrderCommand, OrderState, OrderTraceEvent, OrderTraceStage, OrderType, PrincipalRole,
    ReplaceOrderCommand, ReplayCursor, Side, StpMode, TimeInForce,
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
    /// Maximum order submissions per user within `order_rate_window`.
    /// 0 disables the rate limiter.
    pub max_orders_per_window_per_user: usize,
    /// Sliding window duration for per-user order rate limiting.
    pub order_rate_window: Duration,
    /// After this many consecutive successful new-order commands in
    /// `CancelOnly` state the market automatically recovers to `Normal`.
    /// 0 disables auto-recovery.
    pub auto_recover_after_commands: usize,
}

impl Default for PartitionedEngineConfig {
    fn default() -> Self {
        Self {
            partitions: 8,
            queue_capacity: 4096,
            snapshot_interval_commands: 256, // Increased from 64 to reduce snapshot flush overhead on P99
            max_open_orders_per_user: 200,
            cancel_window: Duration::from_secs(2),
            max_cancel_to_new_ratio: 3.0,
            min_cancel_events_before_guard: 25,
            cancel_only_price_band_bps: 500,
            halt_price_band_bps: 1_000,
            max_orders_per_window_per_user: 0,
            order_rate_window: Duration::from_secs(1),
            auto_recover_after_commands: 0,
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
    QueueResponseDropped {
        partition: usize,
    },
    KillSwitchActive,
    AccountFrozen,
    InvalidOrder(&'static str),
    DuplicateOrderId(String),
    OrderNotFound(String),
    MarketClosed {
        market_id: String,
        outcome: i32,
        state: MarketState,
    },
    Persistence {
        component: &'static str,
        detail: String,
    },
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
    InsufficientFunds {
        detail: String,
    },
    Ledger(String),
    TickSizeViolation {
        price: i64,
        tick_size: i64,
    },
    LotSizeViolation {
        amount: i64,
        lot_size: i64,
    },
    BelowMinAmount {
        amount: i64,
        min_order_amount: i64,
    },
    ExceedsMaxNotional {
        notional: i64,
        max_notional: i64,
    },
    RateLimited {
        user_id: String,
        limit: usize,
    },
    PostOnlyWouldTake,
    ReduceOnlyViolation {
        side: Side,
    },
    ExceedsMaxLeverage {
        leverage: u32,
        max_leverage: u32,
    },
    InstrumentHalted {
        instrument_id: String,
    },
    InstrumentDelisted {
        instrument_id: String,
    },
    UnsupportedOrderType {
        order_type: OrderType,
    },
    UnsupportedTimeInForce {
        time_in_force: TimeInForce,
    },
    FatFingerRejected {
        amount: i64,
        max_order_amount: i64,
    },
    CircuitBreakerTriggered {
        market_id: String,
    },
    MarketKillSwitchActive {
        market_id: String,
    },
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
            SubmissionError::QueueResponseDropped { partition } => {
                write!(f, "partition {partition} response dropped")
            },
            SubmissionError::KillSwitchActive => write!(f, "kill switch is active"),
            SubmissionError::AccountFrozen => write!(f, "account is frozen"),
            SubmissionError::InvalidOrder(reason) => write!(f, "invalid order: {reason}"),
            SubmissionError::DuplicateOrderId(order_id) => {
                write!(f, "duplicate order id: {order_id}")
            }
            SubmissionError::OrderNotFound(order_id) => write!(f, "order not found: {order_id}"),
            SubmissionError::Persistence { component, detail } => {
                write!(f, "persistence error [{component}]: {detail}")
            },
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
            SubmissionError::InsufficientFunds { detail } => write!(f, "insufficient funds: {detail}"),
            SubmissionError::Ledger(error) => write!(f, "ledger error: {error}"),
            SubmissionError::TickSizeViolation { price, tick_size } => {
                write!(f, "price {price} not aligned to tick size {tick_size}")
            }
            SubmissionError::LotSizeViolation { amount, lot_size } => {
                write!(f, "amount {amount} not aligned to lot size {lot_size}")
            }
            SubmissionError::BelowMinAmount { amount, min_order_amount } => {
                write!(f, "amount {amount} below minimum {min_order_amount}")
            }
            SubmissionError::ExceedsMaxNotional { notional, max_notional } => {
                write!(f, "notional {notional} exceeds maximum {max_notional}")
            }
            SubmissionError::RateLimited { user_id, limit } => {
                write!(f, "user {user_id} exceeds rate limit of {limit} orders per window")
            }
            SubmissionError::PostOnlyWouldTake => {
                write!(f, "post-only order would take liquidity")
            }
            SubmissionError::ReduceOnlyViolation { side } => {
                write!(f, "reduce-only {side:?} exceeds position")
            }
            SubmissionError::ExceedsMaxLeverage { leverage, max_leverage } => {
                write!(f, "leverage {leverage}x exceeds maximum {max_leverage}x")
            }
            SubmissionError::InstrumentHalted { instrument_id } => {
                write!(f, "instrument {instrument_id} is halted")
            }
            SubmissionError::InstrumentDelisted { instrument_id } => {
                write!(f, "instrument {instrument_id} is delisted")
            }
            SubmissionError::UnsupportedOrderType { order_type } => {
                write!(f, "order type {order_type:?} not supported on this instrument")
            }
            SubmissionError::UnsupportedTimeInForce { time_in_force } => {
                write!(f, "time-in-force {time_in_force:?} not supported on this instrument")
            }
            SubmissionError::FatFingerRejected { amount, max_order_amount } => {
                write!(f, "order amount {amount} exceeds fat-finger limit {max_order_amount}")
            }
            SubmissionError::CircuitBreakerTriggered { market_id } => {
                write!(f, "circuit breaker triggered on {market_id}")
            }
            SubmissionError::MarketKillSwitchActive { market_id } => {
                write!(f, "market kill switch active on {market_id}")
            }
        }
    }
}

impl std::error::Error for SubmissionError {}

/// Fine-grained timing breakdown of the critical path inside process_new_order.
/// All values are in microseconds.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TimingBreakdown {
    /// Validation: validate_new_order, instrument lookup, fat-finger guard
    pub validation_us: u64,
    /// Risk: order reservation, risk-checked command construction
    pub risk_us: u64,
    /// Core matching: order book traversal, fill execution (excludes settlement/persist)
    pub matching_us: u64,
    /// Settlement + WAL persistence: trade journal, settlement records, fee collection
    pub wal_us: u64,
    /// Post-processing: state transitions, replay cursor, trigger evaluation
    pub post_match_us: u64,
}

#[derive(Debug, Clone)]
pub struct SubmitOrderResult {
    pub metadata: CommandMetadata,
    pub order_id: String,
    pub market_state: MarketState,
    pub fills: Vec<Fill>,
    pub state: OrderState,
    pub remaining_amount: i64,
    pub partition: usize,
    /// Microseconds spent waiting in the partition channel queue.
    pub queue_wait_us: u64,
    /// Microseconds spent inside the matching engine (process_new_order).
    pub match_execution_us: u64,
    /// Microseconds spent persisting the snapshot (WAL I/O). Filled by caller.
    pub persist_us: u64,
    /// Fine-grained timing breakdown of the critical path.
    pub timing: TimingBreakdown,
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
    /// Midpoint between best bid and best ask (integer average).
    pub mid_price: Option<i64>,
    /// Spread = best_ask - best_bid.
    pub spread: Option<i64>,
    /// Total resting bid quantity across all price levels.
    pub total_bid_depth: i64,
    /// Total resting ask quantity across all price levels.
    pub total_ask_depth: i64,
    /// Number of distinct bid price levels.
    pub bid_levels: usize,
    /// Number of distinct ask price levels.
    pub ask_levels: usize,
    /// Cumulative trade statistics for this market.
    pub trade_stats: TradeStatistics,
    /// Order book imbalance ratio: (bid_depth - ask_depth) / (bid_depth + ask_depth).
    /// Range [-1.0, 1.0]. Positive = bid-heavy. `None` when book is empty.
    pub imbalance_ratio: Option<f64>,
    /// VWAP (volume-weighted average price) derived from trade statistics.
    pub vwap: Option<i64>,
    /// Number of conditional (stop/take-profit) orders pending trigger activation.
    pub pending_triggers: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionQueueDepth {
    pub partition_id: usize,
    pub inflight: usize,
    pub capacity: usize,
}

/// Backpressure signal from the matching engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackpressureSignal {
    /// System is operating normally.
    Normal,
    /// Queue is filling — slow down submissions.
    Degraded { queue_usage_pct: u32 },
    /// Queue is near full — reject non-critical commands.
    Critical { queue_usage_pct: u32 },
    /// System is shedding load — only cancels accepted.
    Shedding,
}

/// Priority invariant assertion result for matching engine auditing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityInvariantCheck {
    pub market_id: String,
    pub outcome: i32,
    /// Whether price-time priority holds across the entire book.
    pub price_time_priority_holds: bool,
    /// Number of orders checked.
    pub orders_checked: u64,
    /// Any violations found (empty = invariant holds).
    pub violations: Vec<PriorityViolation>,
}

/// A single priority violation in the order book.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityViolation {
    pub earlier_order_id: String,
    pub later_order_id: String,
    pub violation_type: String,
    pub detail: String,
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
    #[serde(default = "default_instrument_kind")]
    pub instrument_kind: InstrumentKind,
    pub buy_order_id: String,
    pub buy_user_id: String,
    pub sell_order_id: String,
    pub sell_user_id: String,
    pub price: i64,
    pub amount: i64,
    /// Maker fee charged (quote units).
    #[serde(default)]
    pub maker_fee: i64,
    /// Taker fee charged (quote units).
    #[serde(default)]
    pub taker_fee: i64,
    /// The side that aggressed (initiated) the trade.
    #[serde(default)]
    pub aggressor_side: Option<Side>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeSettlementStatus {
    Prepared,
    Applied,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeSettlementRecord {
    pub partition_id: usize,
    pub trade_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub instrument_kind: InstrumentKind,
    pub buy_order_id: String,
    pub buy_user_id: String,
    pub sell_order_id: String,
    pub sell_user_id: String,
    pub price: i64,
    pub amount: i64,
    pub settle_op_id: String,
    pub rollback_op_id: String,
    pub status: TradeSettlementStatus,
    pub recorded_at: DateTime<Utc>,
}

pub trait PositionCostStore: Send + Sync {
    fn record_trade(&self, record: &TradeJournalRecord) -> AnyhowResult<()>;
}

fn default_instrument_kind() -> InstrumentKind {
    InstrumentKind::Spot
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
    #[serde(default)]
    pub trade_stats: TradeStatistics,
    /// Conditional (stop/take-profit) orders waiting for trigger activation.
    #[serde(default)]
    pub trigger_orders: Vec<TriggerOrderSnapshot>,
}

/// Cumulative trade statistics for a market.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TradeStatistics {
    pub total_trades: u64,
    pub total_volume: i64,
    /// Sum of (price * amount) across all trades. Stored as i128 to prevent
    /// saturation in high-volume markets. Serialized as decimal string for JSON compatibility.
    #[serde(default)]
    pub total_turnover: i128,
    pub high_price: Option<i64>,
    pub low_price: Option<i64>,
    pub open_price: Option<i64>,
    pub last_trade_price: Option<i64>,
    pub last_trade_timestamp: Option<DateTime<Utc>>,
}

impl TradeStatistics {
    fn record(&mut self, price: i64, amount: i64) {
        self.total_trades += 1;
        self.total_volume = self.total_volume.saturating_add(amount);
        // Use i128 arithmetic without clamping — saturating_add on i128 won't
        // overflow for any realistic trading volume (i128::MAX ≈ 1.7e38).
        self.total_turnover = self
            .total_turnover
            .saturating_add(price as i128 * amount as i128);
        if self.open_price.is_none() {
            self.open_price = Some(price);
        }
        self.high_price = Some(self.high_price.map_or(price, |h| h.max(price)));
        self.low_price = Some(self.low_price.map_or(price, |l| l.min(price)));
        self.last_trade_price = Some(price);
        self.last_trade_timestamp = Some(Utc::now());
    }

    /// Volume-weighted average price. Returns `None` if no trades.
    pub fn vwap(&self) -> Option<i64> {
        if self.total_volume > 0 {
            Some((self.total_turnover / self.total_volume as i128) as i64)
        } else {
            None
        }
    }
}

/// One aggregated price level in the order book.
#[derive(Debug, Clone)]
pub struct OrderBookLevel {
    pub price: i64,
    pub total_amount: i64,
    pub order_count: usize,
}

/// Aggregated L2 order book depth for a single market/outcome.
#[derive(Debug, Clone)]
pub struct OrderBookDepth {
    pub market_id: String,
    pub outcome: i32,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    pub timestamp: DateTime<Utc>,
}

/// Estimated price impact for a hypothetical order.
#[derive(Debug, Clone)]
pub struct PriceImpactEstimate {
    pub side: Side,
    pub requested_amount: i64,
    /// How much of the order can actually be filled given current book depth.
    pub fillable_amount: i64,
    /// Weighted-average execution price across all levels consumed.
    pub avg_fill_price: Option<i64>,
    /// The worst (final) price level that would be touched.
    pub terminal_price: Option<i64>,
    /// Price impact in basis points relative to best available price.
    pub impact_bps: Option<i64>,
    /// Total notional that would be executed.
    pub total_notional: i64,
    /// Number of price levels consumed.
    pub levels_consumed: usize,
}

/// A conditional (stop / take-profit) order waiting to be triggered.
#[derive(Debug, Clone)]
struct TriggerOrder {
    command: NewOrderCommand,
    trigger_price: i64,
    trigger_type: types::TriggerType,
    /// The order type to use once activated (Market or Limit).
    activated_order_type: OrderType,
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
    #[serde(default)]
    pub stp_mode: StpMode,
    /// Iceberg display quantity. `None` or 0 = fully visible.
    #[serde(default)]
    pub display_qty: Option<i64>,
    /// Minimum individual fill size.
    #[serde(default)]
    pub min_fill_qty: Option<i64>,
    /// STP group identifier for self-trade prevention.
    #[serde(default)]
    pub stp_group_id: Option<String>,
    /// Whether this order belongs to a registered market maker.
    #[serde(default)]
    pub is_market_maker: bool,
}

/// Serialized representation of a conditional trigger order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerOrderSnapshot {
    pub client_order_id: String,
    pub user_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub market_id: String,
    pub outcome: i32,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub price: Option<i64>,
    pub amount: i64,
    pub post_only: bool,
    pub reduce_only: bool,
    #[serde(default)]
    pub leverage: Option<u32>,
    pub trigger_price: i64,
    #[serde(default)]
    pub trigger_type: types::TriggerType,
    #[serde(default)]
    pub stp_mode: StpMode,
    #[serde(default)]
    pub display_qty: Option<i64>,
    #[serde(default)]
    pub min_fill_qty: Option<i64>,
    /// Expiry timestamp for the trigger order.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// STP group identifier.
    #[serde(default)]
    pub stp_group_id: Option<String>,
    /// Whether this trigger order belongs to a market maker.
    #[serde(default)]
    pub is_market_maker: bool,
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
            None,
            None,
            HashMap::new(),
            false,
            HashMap::new(),
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
        Self::with_stores_registry_and_costs(
            config,
            event_bus,
            risk,
            instruments,
            snapshot_store,
            trade_store,
            None,
        )
    }

    pub fn with_stores_registry_and_costs(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
        instruments: Arc<dyn InstrumentRegistry>,
        snapshot_store: Option<Arc<dyn WalStore<PartitionSnapshotRecord>>>,
        trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
        cost_store: Option<Arc<dyn PositionCostStore>>,
    ) -> AnyhowResult<Self> {
        Self::with_stores_registry_costs_and_settlements(
            config,
            event_bus,
            risk,
            instruments,
            snapshot_store,
            trade_store,
            cost_store,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_stores_registry_costs_and_settlements(
        config: PartitionedEngineConfig,
        event_bus: EventBus,
        risk: Arc<RiskEngine>,
        instruments: Arc<dyn InstrumentRegistry>,
        snapshot_store: Option<Arc<dyn WalStore<PartitionSnapshotRecord>>>,
        trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
        cost_store: Option<Arc<dyn PositionCostStore>>,
        settlement_store: Option<Arc<dyn WalStore<TradeSettlementRecord>>>,
    ) -> AnyhowResult<Self> {
        let mut latest_snapshots = HashMap::new();
        let mut kill_switch_enabled = false;
        let mut seen_trade_ids_by_partition: HashMap<usize, HashSet<String>> = HashMap::new();
        let mut settlement_statuses_by_partition: HashMap<
            usize,
            HashMap<String, TradeSettlementStatus>,
        > = HashMap::new();

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
        if let Some(store) = &settlement_store {
            for record in store.entries()? {
                settlement_statuses_by_partition
                    .entry(record.partition_id)
                    .or_default()
                    .insert(record.trade_id.clone(), record.status);
            }
        }

        Ok(Self::build(
            config,
            event_bus,
            risk,
            instruments,
            snapshot_store,
            trade_store,
            cost_store,
            settlement_store,
            latest_snapshots,
            kill_switch_enabled,
            seen_trade_ids_by_partition,
            settlement_statuses_by_partition,
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
        cost_store: Option<Arc<dyn PositionCostStore>>,
        settlement_store: Option<Arc<dyn WalStore<TradeSettlementRecord>>>,
        mut initial_snapshots: HashMap<usize, PartitionStateSnapshot>,
        kill_switch_enabled: bool,
        mut seen_trade_ids_by_partition: HashMap<usize, HashSet<String>>,
        mut settlement_statuses_by_partition: HashMap<
            usize,
            HashMap<String, TradeSettlementStatus>,
        >,
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
                cost_store.clone(),
                settlement_store.clone(),
                partition_id,
                initial_snapshot,
                seen_trade_ids_by_partition
                    .remove(&partition_id)
                    .unwrap_or_default(),
                settlement_statuses_by_partition
                    .remove(&partition_id)
                    .unwrap_or_default(),
            ));
            partitions.push(PartitionHandle {
                partition_id,
                queue_capacity: config.queue_capacity,
                inflight,
                dirty_commands,
                last_snapshot_seq: Arc::new(AtomicU64::new(0)),
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

    /// Compute the aggregate backpressure signal across all partitions.
    /// The worst-case partition determines the signal level.
    pub fn backpressure_signal(&self) -> BackpressureSignal {
        let mut max_usage = 0u32;
        for handle in self.partitions.iter() {
            let inflight = handle.inflight.load(Ordering::Relaxed);
            let capacity = handle.queue_capacity.max(1);
            let usage_pct = (inflight as u64 * 100 / capacity as u64) as u32;
            max_usage = max_usage.max(usage_pct);
        }
        if self.kill_switch.load(Ordering::Relaxed) {
            return BackpressureSignal::Shedding;
        }
        match max_usage {
            0..=60 => BackpressureSignal::Normal,
            61..=85 => BackpressureSignal::Degraded {
                queue_usage_pct: max_usage,
            },
            86..=95 => BackpressureSignal::Critical {
                queue_usage_pct: max_usage,
            },
            _ => BackpressureSignal::Shedding,
        }
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
        let enqueued_at = Instant::now();
        self.send_to_partition(
            partition,
            PartitionRequest::NewOrder {
                command,
                response: response_tx,
                enqueued_at,
            },
        )?;
        let mut result = response_rx
            .await
            .map_err(|_| SubmissionError::QueueResponseDropped { partition })??;
        self.partitions[partition]
            .dirty_commands
            .fetch_add(1, Ordering::Relaxed);
        let persist_start = Instant::now();
        if let Err(error) = self.persist_partitions(&[partition]).await {
            tracing::error!(partition, error = %error, "post-commit snapshot persistence failed");
        }
        result.persist_us = persist_start.elapsed().as_micros() as u64;
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
                .map_err(|_| SubmissionError::QueueResponseDropped { partition })?
            {
                Ok(result) => {
                    for handle in self.partitions.iter() {
                        handle.dirty_commands.fetch_add(1, Ordering::Relaxed);
                    }
                    if let Err(error) = self.persist_all_partitions().await {
                        tracing::error!(error = %error, "post-commit snapshot persistence failed");
                    }
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
                .map_err(|_| SubmissionError::QueueResponseDropped { partition })?
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
        // Offload snapshot persistence to background to keep cancel latency low.
        // Snapshots are flushed when dirty_commands reaches snapshot_interval_commands.
        let all_partitions: Vec<usize> = (0..self.config.partitions).collect();
        self.persist_partitions_background(&all_partitions);
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
        if let Err(error) = self.persist_all_partitions().await {
            tracing::error!(error = %error, "post-commit snapshot persistence failed");
        }
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
        if let Err(error) = self.persist_all_partitions().await {
            tracing::error!(error = %error, "post-commit snapshot persistence failed");
        }
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
        if let Err(error) = self.persist_all_partitions().await {
            tracing::error!(error = %error, "post-commit snapshot persistence failed");
        }
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
                .map_err(|_| SubmissionError::QueueResponseDropped { partition })??;
        }
        for handle in self.partitions.iter() {
            handle.dirty_commands.fetch_add(1, Ordering::Relaxed);
        }
        if let Err(error) = self.persist_all_partitions().await {
            tracing::error!(error = %error, "post-commit snapshot persistence failed");
        }
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
            .map_err(|_| SubmissionError::QueueResponseDropped { partition })??;
        self.partitions[partition]
            .dirty_commands
            .fetch_add(1, Ordering::Relaxed);
        if let Err(error) = self.persist_partitions(&[partition]).await {
            tracing::error!(partition, error = %error, "post-commit snapshot persistence failed");
        }
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
            .map_err(|_| SubmissionError::QueueResponseDropped { partition })
    }

    /// Return aggregated L2 order book depth up to `max_levels` price levels per side.
    pub async fn order_book_depth(
        &self,
        market_id: impl Into<String>,
        outcome: i32,
        max_levels: usize,
    ) -> Result<Option<OrderBookDepth>, SubmissionError> {
        let market_id = market_id.into();
        let partition = self.partition_for_market(&market_id, outcome);
        let (response_tx, response_rx) = oneshot::channel();
        self.send_to_partition(
            partition,
            PartitionRequest::OrderBookDepth {
                market_id,
                outcome,
                max_levels,
                response: response_tx,
            },
        )?;
        response_rx
            .await
            .map_err(|_| SubmissionError::QueueResponseDropped { partition })
    }

    /// Estimate price impact of a hypothetical market-take of `amount` lots on `side`.
    pub async fn estimate_price_impact(
        &self,
        market_id: impl Into<String>,
        outcome: i32,
        side: Side,
        amount: i64,
    ) -> Result<Option<PriceImpactEstimate>, SubmissionError> {
        let market_id = market_id.into();
        let partition = self.partition_for_market(&market_id, outcome);
        let (response_tx, response_rx) = oneshot::channel();
        self.send_to_partition(
            partition,
            PartitionRequest::EstimatePriceImpact {
                market_id,
                outcome,
                side,
                amount,
                response: response_tx,
            },
        )?;
        response_rx
            .await
            .map_err(|_| SubmissionError::QueueResponseDropped { partition })
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
                .map_err(|_| SubmissionError::QueueResponseDropped { partition })??;
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
            .map_err(|_| SubmissionError::QueueResponseDropped { partition })?;
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

    /// Force-flush all partition snapshots to WAL, regardless of dirty count.
    /// Call this during graceful shutdown to ensure no state is lost.
    pub async fn flush_all_snapshots(&self) -> Result<(), SubmissionError> {
        let Some(store) = &self.snapshot_store else {
            return Ok(());
        };
        for partition in 0..self.config.partitions {
            let record = self.export_partition_snapshot(partition).await?;
            let persisted_seq = record.last_applied_command_seq.unwrap_or(0);
            store
                .append(&record)
                .map_err(|error| SubmissionError::Persistence {
                    component: "snapshot_store",
                    detail: error.to_string(),
                })?;
            self.partitions[partition]
                .last_snapshot_seq
                .store(persisted_seq, Ordering::Release);
            self.partitions[partition]
                .dirty_commands
                .store(0, Ordering::Relaxed);
        }

        // Prune op-ids after flushing all partitions.
        if let Some(floor) = self.replay_floor_from_atomics() {
            let _ = self.risk.ledger().prune_seen_op_ids_up_to(floor);
        }
        Ok(())
    }

    /// Check whether any partition is due for a snapshot and, if so, spawn a
    /// background task to export + persist it.  This keeps snapshot I/O off the
    /// order-submission hot-path.  Snapshot durability is best-effort �?trade
    /// journal WAL provides actual recoverability.
    #[allow(dead_code)]
    fn persist_partitions_background(&self, partition_ids: &[usize]) {
        let Some(store) = &self.snapshot_store else {
            return;
        };

        let mut due: Vec<usize> = Vec::new();
        for &partition in partition_ids {
            let dirty = self.partitions[partition]
                .dirty_commands
                .load(Ordering::Relaxed);
            if dirty >= self.config.snapshot_interval_commands {
                due.push(partition);
            }
        }
        if due.is_empty() {
            return;
        }
        due.sort_unstable();
        due.dedup();

        let partitions_arc = Arc::clone(&self.partitions);
        let store = Arc::clone(store);
        let risk = Arc::clone(&self.risk);
        let kill_switch = Arc::clone(&self.kill_switch);
        let config_partitions = self.config.partitions;

        tokio::spawn(async move {
            for partition in &due {
                let (response_tx, response_rx) = oneshot::channel();
                if partitions_arc[*partition]
                    .tx
                    .send(PartitionRequest::ExportSnapshot {
                        response: response_tx,
                    })
                    .await
                    .is_err()
                {
                    tracing::warn!(partition = partition, "snapshot export: partition closed");
                    continue;
                }
                let snapshot = match response_rx.await {
                    Ok(s) => s,
                    Err(_) => {
                        tracing::warn!(partition = partition, "snapshot export: response dropped");
                        continue;
                    }
                };
                let kill_switch_enabled = kill_switch.load(Ordering::Relaxed);
                let checksum =
                    calculate_snapshot_checksum(*partition, kill_switch_enabled, &snapshot);
                let persisted_seq = snapshot.replay_cursor.snapshot_seq.unwrap_or(0);
                let record = PartitionSnapshotRecord {
                    partition_id: *partition,
                    kill_switch_enabled,
                    persisted_at: Utc::now(),
                    snapshot_version: SNAPSHOT_VERSION,
                    snapshot_checksum: checksum,
                    last_applied_command_seq: snapshot.replay_cursor.snapshot_seq,
                    snapshot,
                };
                if let Err(error) = store.append(&record) {
                    tracing::error!(partition = partition, error = %error, "background snapshot write failed");
                    continue;
                }
                // Update atomic snapshot seq AFTER successful disk write to
                // avoid the replay-floor race condition.
                partitions_arc[*partition]
                    .last_snapshot_seq
                    .store(persisted_seq, Ordering::Release);
                partitions_arc[*partition]
                    .dirty_commands
                    .store(0, Ordering::Relaxed);
            }

            // Prune ledger op-ids up to the global replay floor using the
            // atomically-tracked per-partition sequences �?no channel
            // round-trips, no race window.
            let mut seqs = Vec::new();
            for p in 0..config_partitions {
                let seq = partitions_arc[p].last_snapshot_seq.load(Ordering::Acquire);
                if seq > 0 {
                    seqs.push(seq);
                }
            }
            // Floor only meaningful when ALL partitions have been snapshotted.
            if seqs.len() == config_partitions {
                if let Some(floor) = seqs.into_iter().min() {
                    let _ = risk.ledger().prune_seen_op_ids_up_to(floor);
                }
            }
        });
    }

    /// Synchronous version used by non-hot paths (replace, cancel, admin).
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
            let persisted_seq = record.last_applied_command_seq.unwrap_or(0);
            store
                .append(&record)
                .map_err(|error| SubmissionError::Persistence {
                    component: "snapshot_store",
                    detail: error.to_string(),
                })?;
            // Update the atomic snapshot seq AFTER successful disk write so
            // the replay-floor computation always reflects persisted state.
            self.partitions[partition]
                .last_snapshot_seq
                .store(persisted_seq, Ordering::Release);
            self.partitions[partition]
                .dirty_commands
                .store(0, Ordering::Relaxed);
            wrote_snapshot = true;
        }

        if wrote_snapshot {
            if let Some(floor) = self.replay_floor_from_atomics() {
                let _ = self.risk.ledger().prune_seen_op_ids_up_to(floor);
            }
        }

        Ok(())
    }

    /// Compute the global replay floor from atomically-tracked per-partition
    /// snapshot sequences �?no channel round-trips, no race window.
    fn replay_floor_from_atomics(&self) -> Option<u64> {
        let mut seqs = Vec::new();
        for handle in self.partitions.iter() {
            let seq = handle.last_snapshot_seq.load(Ordering::Acquire);
            if seq > 0 {
                seqs.push(seq);
            }
        }
        // Floor is only meaningful when ALL partitions have been snapshotted.
        if seqs.len() == self.config.partitions {
            seqs.into_iter().min()
        } else {
            None
        }
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
    /// Last command_seq persisted to a snapshot for this partition.
    /// Updated atomically after successful snapshot write �?used to compute
    /// the global replay floor without racy channel round-trips.
    last_snapshot_seq: Arc<AtomicU64>,
    tx: mpsc::Sender<PartitionRequest>,
}

enum PartitionRequest {
    NewOrder {
        command: NewOrderCommand,
        response: oneshot::Sender<Result<SubmitOrderResult, SubmissionError>>,
        enqueued_at: Instant,
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
    OrderBookDepth {
        market_id: String,
        outcome: i32,
        max_levels: usize,
        response: oneshot::Sender<Option<OrderBookDepth>>,
    },
    EstimatePriceImpact {
        market_id: String,
        outcome: i32,
        side: Side,
        amount: i64,
        response: oneshot::Sender<Option<PriceImpactEstimate>>,
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
    cost_store: Option<Arc<dyn PositionCostStore>>,
    settlement_store: Option<Arc<dyn WalStore<TradeSettlementRecord>>>,
    partition_id: usize,
    initial_snapshot: PartitionStateSnapshot,
    seen_trade_ids: HashSet<String>,
    settlement_statuses: HashMap<String, TradeSettlementStatus>,
) {
    let mut state = PartitionState::from_snapshot(
        config,
        event_bus,
        risk,
        instruments,
        kill_switch,
        trade_store,
        cost_store,
        settlement_store,
        partition_id,
        initial_snapshot,
        seen_trade_ids,
        settlement_statuses,
    );
    const BATCH_SIZE: usize = 32; // Conservative batch draining (was 128)
    let mut batch = Vec::with_capacity(BATCH_SIZE);

    while let Some(request) = rx.recv().await {
        batch.push(request);

        // Try to fill batch without blocking
        while batch.len() < BATCH_SIZE {
            match rx.try_recv() {
                Ok(req) => batch.push(req),
                Err(_) => break,
            }
        }

        // Process entire batch
        for request in batch.drain(..) {
            state.process(request);
            inflight.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
struct PartitionState {
    config: PartitionedEngineConfig,
    event_bus: EventBus,
    risk: Arc<RiskEngine>,
    instruments: Arc<dyn InstrumentRegistry>,
    kill_switch: Arc<AtomicBool>,
    trade_store: Option<Arc<dyn WalStore<TradeJournalRecord>>>,
    cost_store: Option<Arc<dyn PositionCostStore>>,
    settlement_store: Option<Arc<dyn WalStore<TradeSettlementRecord>>>,
    partition_id: usize,
    markets: HashMap<MarketKey, MarketRuntime>,
    replay_cursor: ReplayCursor,
    seen_trade_ids: HashSet<String>,
    settlement_statuses: HashMap<String, TradeSettlementStatus>,
    /// Accounts frozen by admin action — all new orders rejected.
    frozen_accounts: HashSet<String>,
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
        cost_store: Option<Arc<dyn PositionCostStore>>,
        settlement_store: Option<Arc<dyn WalStore<TradeSettlementRecord>>>,
        partition_id: usize,
        snapshot: PartitionStateSnapshot,
        seen_trade_ids: HashSet<String>,
        settlement_statuses: HashMap<String, TradeSettlementStatus>,
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
            cost_store,
            settlement_store,
            partition_id,
            markets,
            replay_cursor,
            seen_trade_ids,
            settlement_statuses,
            frozen_accounts: HashSet::new(),
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
            PartitionRequest::NewOrder {
                command,
                response,
                enqueued_at,
            } => {
                let queue_wait_us = enqueued_at.elapsed().as_micros() as u64;
                let match_start = Instant::now();
                let mut result = self.process_new_order(command);
                let match_execution_us = match_start.elapsed().as_micros() as u64;
                if let Ok(ref mut r) = result {
                    r.queue_wait_us = queue_wait_us;
                    r.match_execution_us = match_execution_us;
                }
                let _ = response.send(result);
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
            PartitionRequest::OrderBookDepth {
                market_id,
                outcome,
                max_levels,
                response,
            } => {
                let key = MarketKey::new(market_id, outcome);
                let depth = self.markets.get(&key).map(|m| m.book_depth(max_levels));
                let _ = response.send(depth);
            }
            PartitionRequest::EstimatePriceImpact {
                market_id,
                outcome,
                side,
                amount,
                response,
            } => {
                let key = MarketKey::new(market_id, outcome);
                let estimate = self
                    .markets
                    .get(&key)
                    .map(|m| m.estimate_impact(side, amount));
                let _ = response.send(estimate);
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
                self.partition_id,
            ));
        }
        validate_new_order(&command)?;
        let instrument = self.instrument_spec(&command.market_id);
        command.leverage = normalized_command_leverage(&instrument, &command)?;
        if self.kill_switch.load(Ordering::Relaxed) {
            return Err(SubmissionError::KillSwitchActive);
        }
        if self.frozen_accounts.contains(&command.user_id) {
            return Err(SubmissionError::AccountFrozen);
        }
        // Fat-finger guard
        validate_fat_finger(&instrument, &command)?;

        let key = MarketKey::new(command.market_id.clone(), command.outcome);
        self.evict_expired_orders_for_market(&key, command.metadata.received_at)?;

        // ── Conditional order (stop / take-profit): park in trigger book ──
        // Handle before the main markets destructuring to avoid borrow conflicts.
        if command.order_type.is_conditional() {
            let trigger_price = command.trigger_price.ok_or(SubmissionError::InvalidOrder(
                "conditional order requires trigger_price",
            ))?;
            let activated_order_type = command.order_type.triggered_type();
            let order_id = command.client_order_id.clone();
            let trigger_type = command.trigger_type.unwrap_or_default();
            let market = self
                .markets
                .entry(key.clone())
                .or_insert_with(|| MarketRuntime::new(&key.market_id, key.outcome));
            if market.check_rate_limit(&command.user_id, &self.config) {
                return Err(SubmissionError::RateLimited {
                    user_id: command.user_id.clone(),
                    limit: self.config.max_orders_per_window_per_user,
                });
            }
            if market.trigger_orders.contains_key(&order_id) {
                return Err(SubmissionError::DuplicateOrderId(order_id));
            }
            market.trigger_orders.insert(
                order_id.clone(),
                TriggerOrder {
                    command: command.clone(),
                    trigger_price,
                    trigger_type,
                    activated_order_type,
                },
            );
            command
                .metadata
                .advance(CommandLifecycle::PartitionAccepted);
            let market_state = market.state;
            record_recent_event(market, &self.config, RecentMarketEventKind::NewOrder, 1);
            self.advance_replay_cursor(command.metadata.command_seq);
            return Ok(SubmitOrderResult {
                metadata: command.metadata,
                order_id,
                market_state,
                fills: Vec::new(),
                state: OrderState::Active,
                remaining_amount: command.amount,
                partition: self.partition_id,
                queue_wait_us: 0,
                match_execution_us: 0,
                persist_us: 0,
                timing: TimingBreakdown::default(),
            });
        }

        let (markets, seen_trade_ids) = (&mut self.markets, &mut self.seen_trade_ids);
        let market = markets
            .entry(key.clone())
            .or_insert_with(|| MarketRuntime::new(&key.market_id, key.outcome));

        // ── Per-user rate limiter ──
        if market.check_rate_limit(&command.user_id, &self.config) {
            return Err(SubmissionError::RateLimited {
                user_id: command.user_id.clone(),
                limit: self.config.max_orders_per_window_per_user,
            });
        }

        // Phase 1: Validation timing
        let val_start = Instant::now();
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
        let validation_us = val_start.elapsed().as_micros() as u64;

        let mut incoming = RestingOrder::from_new_order(command.clone());
        if incoming.order_type == OrderType::Market
            && (incoming.side == Side::Buy || instrument.kind != InstrumentKind::Spot)
        {
            incoming.reserved_cash =
                market_buy_budget(market, &self.risk, &instrument, &command, None)?;
        }

        // Phase 2: Risk timing
        let risk_start = Instant::now();
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
        let risk_us = risk_start.elapsed().as_micros() as u64;
        // Phase 3: Matching timing (includes WAL writes inside match_incoming)
        let match_start = Instant::now();
        let match_outcome = match_incoming(
            market,
            &mut incoming,
            &instrument,
            &self.config,
            &self.event_bus,
            &self.risk,
            self.trade_store.as_deref(),
            self.cost_store.as_deref(),
            self.settlement_store.as_deref(),
            seen_trade_ids,
            &mut self.settlement_statuses,
            self.partition_id,
        )?;
        let matching_us = match_start.elapsed().as_micros() as u64;
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
            let settlement_persist_us = match_outcome.settlement_persist_us;
            return if fills.is_empty() {
                Err(error)
            } else {
                // Observer: aborted-with-partial-fill outcome.
                emit_matching_outcome_for_new_order(
                    &self.event_bus,
                    &command,
                    &incoming.order_id,
                    OrderState::PartiallyFilled,
                    incoming.remaining_amount,
                    fills.len(),
                );
                Ok(SubmitOrderResult {
                    metadata: command.metadata,
                    order_id: incoming.order_id,
                    market_state,
                    fills,
                    state: OrderState::PartiallyFilled,
                    remaining_amount: incoming.remaining_amount,
                    partition: self.partition_id,
                    queue_wait_us: 0,
                    match_execution_us: 0,
                    persist_us: settlement_persist_us,
                    timing: TimingBreakdown {
                        validation_us,
                        risk_us,
                        matching_us,
                        wal_us: settlement_persist_us,
                        post_match_us: 0,
                    },
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
            // Non-resting orders (Market/IOC/FOK) that are cancelled without filling
            // should still track client_order_id to prevent immediate reuse
            if incoming.remaining_amount < incoming.original_amount {
                // Partially filled — was already tracked during resting insert or match
            }
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

        // Phase 5: Post-match timing (state transitions, replay cursor, triggers)
        let post_match_start = Instant::now();
        record_recent_event(market, &self.config, RecentMarketEventKind::NewOrder, 1);

        // ── Auto-recovery from CancelOnly ──
        if market.state == MarketState::CancelOnly && self.config.auto_recover_after_commands > 0 {
            market.cancel_only_recovery_counter += 1;
            if market.cancel_only_recovery_counter >= self.config.auto_recover_after_commands {
                market.state = MarketState::Normal;
                market.cancel_only_recovery_counter = 0;
            }
        } else if market.state != MarketState::CancelOnly {
            market.cancel_only_recovery_counter = 0;
        }

        self.advance_replay_cursor(command_seq);

        // ── Trigger evaluation: activate stop/take-profit orders whose conditions are met ──
        // Only activate triggers if market is in a trading-eligible state.
        let triggered_key = if !fills.is_empty() { Some(key) } else { None };
        let triggered_commands = triggered_key
            .as_ref()
            .filter(|k| {
                self.markets
                    .get(*k)
                    .is_some_and(|m| matches!(m.state, MarketState::Normal | MarketState::Stress))
            })
            .and_then(|k| self.extract_triggered_commands(k))
            .unwrap_or_default();

        for cmd in triggered_commands {
            let client_order_id = cmd.client_order_id.clone();
            if let Err(err) = self.process_new_order(cmd) {
                tracing::warn!(
                    client_order_id = %client_order_id,
                    error = %err,
                    "triggered order activation failed"
                );
            }
        }
        let post_match_us = post_match_start.elapsed().as_micros() as u64;

        let settlement_persist_us = match_outcome.settlement_persist_us;

        // Observer: emit one matching trace event for the final outcome.
        // For new orders this is the binding moment that lets the projector
        // flush any pre-sequencer trace_key bucket per design §3.3.1.
        emit_matching_outcome_for_new_order(
            &self.event_bus,
            &command,
            &incoming.order_id,
            state,
            incoming.remaining_amount,
            fills.len(),
        );

        Ok(SubmitOrderResult {
            metadata: command.metadata,
            order_id: incoming.order_id,
            market_state,
            fills,
            state,
            remaining_amount: incoming.remaining_amount,
            partition: self.partition_id,
            queue_wait_us: 0,
            match_execution_us: 0,
            persist_us: settlement_persist_us,
            timing: TimingBreakdown {
                validation_us,
                risk_us,
                matching_us,
                wal_us: settlement_persist_us,
                post_match_us,
            },
        })
    }

    /// Extract conditional orders whose trigger conditions are now met.
    /// Returns activated commands ready for submission via `process_new_order`.
    fn extract_triggered_commands(&mut self, key: &MarketKey) -> Option<Vec<NewOrderCommand>> {
        let market = self.markets.get(key)?;
        let last_price = market.last_trade_price?;
        // Collect IDs of triggered orders.
        let triggered_ids: Vec<String> = market
            .trigger_orders
            .iter()
            .filter(|(_, trigger)| {
                let reference = match trigger.trigger_type {
                    types::TriggerType::LastPrice => last_price,
                    types::TriggerType::MarkPrice | types::TriggerType::IndexPrice => {
                        market.reference_price.unwrap_or(last_price)
                    }
                };
                is_trigger_met(&trigger.command, trigger.trigger_price, reference)
            })
            .map(|(id, _)| id.clone())
            .collect();

        if triggered_ids.is_empty() {
            return None;
        }

        let market = self.markets.get_mut(key)?;
        let mut activated = Vec::with_capacity(triggered_ids.len());
        for id in &triggered_ids {
            if let Some(trigger) = market.trigger_orders.remove(id) {
                let mut cmd = trigger.command;
                cmd.order_type = trigger.activated_order_type;
                cmd.trigger_price = None;
                cmd.trigger_type = None;
                cmd.metadata = CommandMetadata::new(&cmd.metadata.request_id);
                activated.push(cmd);
            }
        }
        Some(activated)
    }

    fn process_replace_order(
        &mut self,
        command: ReplaceOrderCommand,
    ) -> Result<SubmitOrderResult, SubmissionError> {
        if self.should_skip_replayed_command(command.metadata.command_seq) {
            return Ok(skipped_replace_order_result(
                &command,
                aggregate_market_state(&self.markets),
                self.partition_id,
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
            market.client_order_ids.remove(&existing.order_id);
            market.remove_from_book(&existing);
            market.remove_order_indexes(
                &existing.order_id,
                &existing.user_id,
                existing.session_id.as_deref(),
            );
        }
        // Observer: the old order is now removed from the book. The new
        // order's lifecycle (matching_resting / matching_filled / etc.)
        // will be emitted by the recursive process_new_order call below.
        emit_matching_cancelled(
            &self.event_bus,
            &existing.order_id,
            &command.metadata.request_id,
            command.metadata.command_seq,
            Some(&existing.user_id),
            Some(&market_key.market_id),
            Some(market_key.outcome),
        );

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
                    tracing::error!(
                        order_id = %restored.order_id,
                        error = %restore_error,
                        "replace-order reservation restore failed; re-inserting order and halting market"
                    );
                    // Always re-insert the order to prevent silent order loss.
                    let market = self.markets.entry(market_key.clone()).or_insert_with(|| {
                        MarketRuntime::new(&market_key.market_id, market_key.outcome)
                    });
                    insert_resting_order(market, restored);
                    market.state = MarketState::Halted;
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
        // ── Also check the trigger book for conditional orders ──
        let mut trigger_cancelled = false;
        for market in self.markets.values_mut().filter(|m| {
            m.market_id == command.market_id && command.outcome.is_none_or(|o| m.outcome == o)
        }) {
            if let Some(trigger) = market.trigger_orders.get(&command.order_id) {
                if trigger.command.user_id == command.user_id {
                    market.trigger_orders.remove(&command.order_id);
                    trigger_cancelled = true;
                }
            }
        }
        if trigger_cancelled {
            command.metadata.advance(CommandLifecycle::Executed);
            command.metadata.advance(CommandLifecycle::Completed);
            self.advance_replay_cursor(command.metadata.command_seq);
            // Observer: trigger order cancelled before activation.
            emit_matching_cancelled(
                &self.event_bus,
                &command.order_id,
                &command.metadata.request_id,
                command.metadata.command_seq,
                Some(&command.user_id),
                Some(&command.market_id),
                command.outcome,
            );
            return Ok(CancelResult {
                metadata: command.metadata,
                market_state: aggregate_market_state(&self.markets),
                cancelled_order_ids: vec![command.order_id],
            });
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
        // Observer: emit one matching_cancelled per cancelled order_id.
        for cancelled_id in &cancelled_order_ids {
            emit_matching_cancelled(
                &self.event_bus,
                cancelled_id,
                &command.metadata.request_id,
                command.metadata.command_seq,
                Some(&command.user_id),
                Some(&command.market_id),
                command.outcome,
            );
        }
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
        // Observer: emit one matching_cancelled per cancelled order_id.
        // Mass-cancel does not have a single market_id/outcome (it spans the
        // user's entire book), so those fields are populated from the
        // affected order's market lookup is skipped — `None` here.
        for cancelled_id in &cancelled_order_ids {
            emit_matching_cancelled(
                &self.event_bus,
                cancelled_id,
                &command.metadata.request_id,
                command.metadata.command_seq,
                Some(&command.user_id),
                None,
                None,
            );
        }
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
        for cancelled_id in &cancelled_order_ids {
            emit_matching_cancelled(
                &self.event_bus,
                cancelled_id,
                &command.metadata.request_id,
                command.metadata.command_seq,
                Some(&command.user_id),
                None,
                None,
            );
        }
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
        let market_id_for_trace = command.market_id.clone();
        for cancelled_id in &cancelled_order_ids {
            emit_matching_cancelled(
                &self.event_bus,
                cancelled_id,
                &command.metadata.request_id,
                command.metadata.command_seq,
                None,
                Some(&market_id_for_trace),
                None,
            );
        }
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
                    if !market.state.can_transition_to(state) {
                        tracing::warn!(
                            market_id = %market.market_id,
                            from = ?market.state,
                            to = ?state,
                            "invalid market state transition rejected"
                        );
                        continue;
                    }
                    market.state = state;
                }
            }
            AdminAction::UpdateInstrument { spec } => {
                tracing::info!(instrument = %spec.instrument_id, "instrument spec updated via admin action");
            }
            AdminAction::FreezeAccount { user_id, reason } => {
                tracing::warn!(user_id = %user_id, reason = %reason, "account frozen — cancelling all orders");
                self.frozen_accounts.insert(user_id.clone());
                // Mass-cancel all orders belonging to the frozen user.
                let _ = cancel_orders(
                    &mut self.markets,
                    &self.config,
                    &self.risk,
                    self.instruments.as_ref(),
                    None,
                    None,
                    None,
                    None,
                    Some(user_id.as_str()),
                );
            }
            AdminAction::UnfreezeAccount { user_id } => {
                tracing::info!(user_id = %user_id, "account unfrozen");
                self.frozen_accounts.remove(&user_id);
            }
            AdminAction::MarketKillSwitch { market_id, enabled } => {
                tracing::warn!(market_id = %market_id, enabled = enabled, "per-market kill switch toggled");
                for market in self
                    .markets
                    .values_mut()
                    .filter(|m| m.market_id == market_id)
                {
                    if enabled {
                        market.state = MarketState::Halted;
                    } else if market.state == MarketState::Halted {
                        market.state = MarketState::CancelOnly;
                    }
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
            market.client_order_ids.remove(&order_id);
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
    trade_stats: TradeStatistics,
    /// Per-user sliding window of order submission timestamps for rate limiting.
    user_order_timestamps: HashMap<String, VecDeque<Instant>>,
    /// Counter of consecutive successful new orders while in CancelOnly state.
    cancel_only_recovery_counter: usize,
    /// Conditional (stop/take-profit) orders keyed by client_order_id.
    trigger_orders: HashMap<String, TriggerOrder>,
    /// Rolling window of recent trade prices for realized volatility calculation.
    recent_trade_prices: VecDeque<i64>,
    /// Timestamp when the circuit breaker was last triggered (for cooldown).
    circuit_breaker_triggered_at: Option<Instant>,
    /// Per-market-maker rolling fill tracking: (user_id → MmFillTracker).
    mm_fill_trackers: HashMap<String, MmFillTracker>,
    /// Per-user trailing 30-day notional volume for fee tier resolution.
    user_volume_30d: HashMap<String, i64>,
    /// Track client_order_ids for regular resting orders to detect duplicates.
    client_order_ids: HashSet<String>,
}

/// Maximum number of user volume entries before triggering eviction.
const MAX_VOLUME_ENTRIES: usize = 100_000;
/// Maximum number of MM tracker entries before triggering eviction.
const MAX_MM_TRACKERS: usize = 10_000;
/// Maximum number of rate-limit timestamp entries before triggering eviction.
const MAX_RATE_LIMIT_ENTRIES: usize = 100_000;

/// Tracks rolling fills for a market maker within a window.
#[derive(Debug, Clone)]
struct MmFillTracker {
    /// (timestamp, signed_delta_qty, notional)
    fills: VecDeque<(Instant, i64, i64)>,
}

impl MmFillTracker {
    fn new() -> Self {
        Self {
            fills: VecDeque::new(),
        }
    }

    fn record_fill(&mut self, qty: i64, side: Side, price: i64) {
        let signed_delta = match side {
            Side::Buy => qty,
            Side::Sell => -qty,
        };
        let notional = price.saturating_mul(qty.abs());
        self.fills
            .push_back((Instant::now(), signed_delta, notional));
    }

    fn evict_old(&mut self, window: Duration) {
        let cutoff = Instant::now() - window;
        while self.fills.front().is_some_and(|(t, _, _)| *t < cutoff) {
            self.fills.pop_front();
        }
    }

    /// Cap the fills deque to prevent unbounded growth.
    fn cap_fills(&mut self, max_entries: usize) {
        while self.fills.len() > max_entries {
            self.fills.pop_front();
        }
    }

    fn net_delta(&self) -> i64 {
        self.fills.iter().map(|(_, d, _)| d).sum()
    }

    fn total_notional(&self) -> i64 {
        self.fills.iter().map(|(_, _, n)| *n).sum()
    }
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
            trade_stats: TradeStatistics::default(),
            user_order_timestamps: HashMap::new(),
            cancel_only_recovery_counter: 0,
            trigger_orders: HashMap::new(),
            recent_trade_prices: VecDeque::new(),
            circuit_breaker_triggered_at: None,
            mm_fill_trackers: HashMap::new(),
            user_volume_30d: HashMap::new(),
            client_order_ids: HashSet::new(),
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
            trade_stats: snapshot.trade_stats,
            user_order_timestamps: HashMap::new(),
            cancel_only_recovery_counter: 0,
            trigger_orders: snapshot
                .trigger_orders
                .into_iter()
                .map(|snap| {
                    let activated_order_type = snap.order_type.triggered_type();
                    let trigger = TriggerOrder {
                        command: NewOrderCommand {
                            metadata: CommandMetadata::new(&snap.client_order_id),
                            client_order_id: snap.client_order_id.clone(),
                            user_id: snap.user_id,
                            session_id: snap.session_id,
                            market_id: snap.market_id,
                            side: snap.side,
                            order_type: snap.order_type,
                            time_in_force: snap.time_in_force,
                            price: snap.price,
                            amount: snap.amount,
                            outcome: snap.outcome,
                            post_only: snap.post_only,
                            reduce_only: snap.reduce_only,
                            leverage: snap.leverage,
                            expires_at: snap.expires_at,
                            stp_mode: snap.stp_mode,
                            trigger_price: Some(snap.trigger_price),
                            trigger_type: Some(snap.trigger_type),
                            display_qty: snap.display_qty,
                            min_fill_qty: snap.min_fill_qty,
                            stp_group_id: snap.stp_group_id,
                            is_market_maker: snap.is_market_maker,
                        },
                        trigger_price: snap.trigger_price,
                        trigger_type: snap.trigger_type,
                        activated_order_type,
                    };
                    (snap.client_order_id, trigger)
                })
                .collect(),
            recent_trade_prices: VecDeque::new(),
            circuit_breaker_triggered_at: None,
            mm_fill_trackers: HashMap::new(),
            user_volume_30d: HashMap::new(),
            client_order_ids: HashSet::new(),
        };

        for order in snapshot.orders {
            market.client_order_ids.insert(order.order_id.clone());
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
            trade_stats: self.trade_stats.clone(),
            trigger_orders: self
                .trigger_orders
                .values()
                .map(|t| TriggerOrderSnapshot {
                    client_order_id: t.command.client_order_id.clone(),
                    user_id: t.command.user_id.clone(),
                    session_id: t.command.session_id.clone(),
                    market_id: t.command.market_id.clone(),
                    outcome: t.command.outcome,
                    side: t.command.side,
                    order_type: t.command.order_type,
                    time_in_force: t.command.time_in_force,
                    price: t.command.price,
                    amount: t.command.amount,
                    post_only: t.command.post_only,
                    reduce_only: t.command.reduce_only,
                    leverage: t.command.leverage,
                    trigger_price: t.trigger_price,
                    trigger_type: t.trigger_type,
                    stp_mode: t.command.stp_mode,
                    display_qty: t.command.display_qty,
                    min_fill_qty: t.command.min_fill_qty,
                    expires_at: t.command.expires_at,
                    stp_group_id: t.command.stp_group_id.clone(),
                    is_market_maker: t.command.is_market_maker,
                })
                .collect(),
        }
    }

    fn snapshot(&self) -> MarketSnapshot {
        let (recent_new_orders, recent_cancel_events) =
            summarize_recent_events(&self.recent_events);
        let best_bid = self.best_bid();
        let best_ask = self.best_ask();
        let mid_price = match (best_bid, best_ask) {
            (Some(b), Some(a)) => Some((b + a) / 2),
            _ => None,
        };
        let spread = match (best_bid, best_ask) {
            (Some(b), Some(a)) => Some(a - b),
            _ => None,
        };
        let total_bid_depth = self.total_side_depth(&self.bids);
        let total_ask_depth = self.total_side_depth(&self.asks);
        let total_depth = total_bid_depth + total_ask_depth;
        let imbalance_ratio = if total_depth > 0 {
            Some((total_bid_depth - total_ask_depth) as f64 / total_depth as f64)
        } else {
            None
        };
        let vwap = self.trade_stats.vwap();
        MarketSnapshot {
            market_id: self.market_id.clone(),
            outcome: self.outcome,
            state: self.state,
            reference_price: self.reference_price,
            last_trade_price: self.last_trade_price,
            best_bid,
            best_ask,
            open_orders: self.orders.len(),
            recent_new_orders,
            recent_cancel_events,
            mid_price,
            spread,
            total_bid_depth,
            total_ask_depth,
            bid_levels: self.bids.len(),
            ask_levels: self.asks.len(),
            trade_stats: self.trade_stats.clone(),
            imbalance_ratio,
            vwap,
            pending_triggers: self.trigger_orders.len(),
        }
    }

    fn best_bid(&self) -> Option<i64> {
        self.bids.keys().next_back().copied()
    }

    fn best_ask(&self) -> Option<i64> {
        self.asks.keys().next().copied()
    }

    /// Sum of remaining_amount for all resting orders on one side.
    fn total_side_depth(&self, side: &BTreeMap<i64, VecDeque<String>>) -> i64 {
        let mut total: i64 = 0;
        for queue in side.values() {
            for order_id in queue {
                if let Some(order) = self.orders.get(order_id) {
                    total = total.saturating_add(order.remaining_amount);
                }
            }
        }
        total
    }

    /// Aggregate L2 order book depth (up to `max_levels` price levels per side).
    fn book_depth(&self, max_levels: usize) -> OrderBookDepth {
        let bids: Vec<OrderBookLevel> = self
            .bids
            .iter()
            .rev()
            .take(max_levels)
            .map(|(&price, queue)| {
                let mut total_amount: i64 = 0;
                let mut count = 0usize;
                for order_id in queue {
                    if let Some(order) = self.orders.get(order_id) {
                        total_amount = total_amount.saturating_add(order.remaining_amount);
                        count += 1;
                    }
                }
                OrderBookLevel {
                    price,
                    total_amount,
                    order_count: count,
                }
            })
            .collect();

        let asks: Vec<OrderBookLevel> = self
            .asks
            .iter()
            .take(max_levels)
            .map(|(&price, queue)| {
                let mut total_amount: i64 = 0;
                let mut count = 0usize;
                for order_id in queue {
                    if let Some(order) = self.orders.get(order_id) {
                        total_amount = total_amount.saturating_add(order.remaining_amount);
                        count += 1;
                    }
                }
                OrderBookLevel {
                    price,
                    total_amount,
                    order_count: count,
                }
            })
            .collect();

        OrderBookDepth {
            market_id: self.market_id.clone(),
            outcome: self.outcome,
            bids,
            asks,
            timestamp: Utc::now(),
        }
    }

    /// Returns `true` if the user has exceeded the per-window rate limit.
    fn check_rate_limit(&mut self, user_id: &str, config: &PartitionedEngineConfig) -> bool {
        if config.max_orders_per_window_per_user == 0 {
            return false; // rate limiting disabled
        }
        // Evict stale user entries when the map grows too large.
        if self.user_order_timestamps.len() >= MAX_RATE_LIMIT_ENTRIES {
            self.user_order_timestamps.retain(|_, ts| {
                ts.back()
                    .is_some_and(|t| t.elapsed() <= config.order_rate_window)
            });
        }
        let now = Instant::now();
        let timestamps = self
            .user_order_timestamps
            .entry(user_id.to_string())
            .or_default();
        // Evict entries outside the window.
        while let Some(&front) = timestamps.front() {
            if now.duration_since(front) > config.order_rate_window {
                timestamps.pop_front();
            } else {
                break;
            }
        }
        if timestamps.len() >= config.max_orders_per_window_per_user {
            return true; // over limit
        }
        timestamps.push_back(now);
        false
    }

    /// Estimate price impact of a hypothetical market-take of `amount` on `side`.
    fn estimate_impact(&self, side: Side, amount: i64) -> PriceImpactEstimate {
        let levels_iter: Box<dyn Iterator<Item = (&i64, &VecDeque<String>)>> = match side {
            Side::Buy => Box::new(self.asks.iter()),
            Side::Sell => Box::new(self.bids.iter().rev()),
        };
        let mut remaining = amount;
        let mut total_notional: i64 = 0;
        let mut fillable: i64 = 0;
        let mut best_price: Option<i64> = None;
        let mut terminal: Option<i64> = None;
        let mut levels_consumed = 0usize;

        for (&price, queue) in levels_iter {
            if remaining <= 0 {
                break;
            }
            if best_price.is_none() {
                best_price = Some(price);
            }
            let mut level_filled: i64 = 0;
            for order_id in queue {
                if remaining <= 0 {
                    break;
                }
                if let Some(order) = self.orders.get(order_id) {
                    let exec = order.remaining_amount.min(remaining);
                    if exec > 0 {
                        level_filled += exec;
                        remaining -= exec;
                    }
                }
            }
            if level_filled > 0 {
                total_notional = total_notional.saturating_add(
                    (price as i128 * level_filled as i128).clamp(0, i64::MAX as i128) as i64,
                );
                fillable += level_filled;
                terminal = Some(price);
                levels_consumed += 1;
            }
        }

        let avg_fill_price = if fillable > 0 {
            Some(total_notional / fillable)
        } else {
            None
        };
        let impact_bps = match (best_price, terminal) {
            (Some(best), Some(term)) if best > 0 => {
                Some(((term as i128 - best as i128).abs() * 10_000 / best as i128) as i64)
            }
            _ => None,
        };

        PriceImpactEstimate {
            side,
            requested_amount: amount,
            fillable_amount: fillable,
            avg_fill_price,
            terminal_price: terminal,
            impact_bps,
            total_notional,
            levels_consumed,
        }
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
    stp_mode: StpMode,
    reserved_cash: i64,
    reserved_position: i64,
    /// Iceberg: display (visible) quantity. `None` or 0 = fully visible.
    display_qty: Option<i64>,
    /// Iceberg: how much of `remaining_amount` is currently visible on the book.
    visible_qty: i64,
    /// Minimum individual fill size. `None` or 0 = no minimum.
    min_fill_qty: Option<i64>,
    /// STP group identifier for firm/sub-account level self-trade prevention.
    stp_group_id: Option<String>,
    /// Whether this order was submitted by a designated market maker.
    is_market_maker: bool,
}

#[derive(Debug, Default)]
struct MatchOutcome {
    fills: Vec<Fill>,
    aborted: Option<SubmissionError>,
    /// Cumulative microseconds spent in settlement + WAL persistence within the match loop.
    settlement_persist_us: u64,
}

impl RestingOrder {
    fn from_new_order(command: NewOrderCommand) -> Self {
        let limit_price = command.price.unwrap_or(0);
        let display = command.display_qty.filter(|&q| q > 0);
        let visible = display.map_or(command.amount, |d| d.min(command.amount));
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
            stp_mode: command.stp_mode,
            reserved_cash: 0,
            reserved_position: 0,
            display_qty: display,
            visible_qty: visible,
            min_fill_qty: command.min_fill_qty.filter(|&q| q > 0),
            stp_group_id: command.stp_group_id,
            is_market_maker: command.is_market_maker,
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
            _ => {
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
            stp_mode: snapshot.stp_mode,
            reserved_cash,
            reserved_position,
            display_qty: snapshot.display_qty.filter(|&q| q > 0),
            visible_qty: snapshot
                .display_qty
                .filter(|&q| q > 0)
                .map_or(snapshot.remaining_amount, |d| {
                    d.min(snapshot.remaining_amount)
                }),
            min_fill_qty: snapshot.min_fill_qty.filter(|&q| q > 0),
            stp_group_id: snapshot.stp_group_id,
            is_market_maker: snapshot.is_market_maker,
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
            stp_mode: self.stp_mode,
            display_qty: self.display_qty,
            min_fill_qty: self.min_fill_qty,
            stp_group_id: self.stp_group_id.clone(),
            is_market_maker: self.is_market_maker,
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

/// Emit one matching trace event for the final outcome of a NewOrder
/// process. Observer-only (publishes to the eventbus `order.trace`
/// channel where lagged subscribers are silently dropped). The
/// `state` parameter selects the stage:
/// - `OrderState::Filled`         -> `matching_filled`
/// - `OrderState::Active`         -> `matching_resting` (may include partial fills,
///                                    captured in `filled_amount`)
/// - `OrderState::PartiallyFilled`-> `matching_partially_filled` (IOC/FOK that
///                                    filled some and was discarded)
/// - `OrderState::Cancelled`      -> `matching_cancelled` (IOC/FOK with no fills)
/// - other states are not emitted (defensive — they should not occur on
///   the normal-success return path).
///
/// `order_id` is the canonical id assigned by the engine — for new
/// orders this is the binding moment that lets the projector flush the
/// pre-sequencer trace_key bucket per design §3.3.1.
fn emit_matching_outcome_for_new_order(
    event_bus: &eventbus::EventBus,
    command: &NewOrderCommand,
    order_id: &str,
    state: OrderState,
    remaining_amount: i64,
    fills_count: usize,
) {
    let stage = match state {
        OrderState::Filled => OrderTraceStage::MatchingFilled,
        OrderState::Active => OrderTraceStage::MatchingResting,
        OrderState::PartiallyFilled => OrderTraceStage::MatchingPartiallyFilled,
        OrderState::Cancelled => OrderTraceStage::MatchingCancelled,
        // Rejected / Replaced should not arrive on a success-path emit.
        _ => return,
    };
    let mut ev = OrderTraceEvent::new(stage, order_id);
    ev.client_order_id = Some(command.client_order_id.clone());
    ev.user_id = Some(command.user_id.clone());
    ev.session_id = command.session_id.clone();
    ev.request_id = Some(command.metadata.request_id.clone());
    ev.command_seq = command.metadata.command_seq;
    ev.market_id = Some(command.market_id.clone());
    ev.outcome = Some(command.outcome);
    ev.side = Some(command.side);
    ev.price = command.price;
    ev.amount = Some(command.amount);
    ev.remaining_amount = Some(remaining_amount);
    ev.filled_amount = Some(command.amount.saturating_sub(remaining_amount));
    if fills_count > 0 {
        ev.detail = serde_json::json!({ "fills_count": fills_count });
    }
    event_bus.publish(Event::OrderTrace(ev));
}

/// Emit a `matching_cancelled` trace event for one specific order_id.
/// Used by direct cancels, mass-cancels, and the cancel-old-order leg
/// of replace.
fn emit_matching_cancelled(
    event_bus: &eventbus::EventBus,
    order_id: &str,
    request_id: &str,
    command_seq: Option<u64>,
    user_id: Option<&str>,
    market_id: Option<&str>,
    outcome: Option<i32>,
) {
    let mut ev = OrderTraceEvent::new(OrderTraceStage::MatchingCancelled, order_id);
    ev.request_id = Some(request_id.to_string());
    ev.command_seq = command_seq;
    ev.user_id = user_id.map(String::from);
    ev.market_id = market_id.map(String::from);
    ev.outcome = outcome;
    event_bus.publish(Event::OrderTrace(ev));
}

fn insert_resting_order(market: &mut MarketRuntime, order: RestingOrder) {
    // Track client_order_id only for resting orders to prevent duplicate submissions
    market.client_order_ids.insert(order.order_id.clone());
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
        stp_mode: existing.stp_mode,
        trigger_price: None,
        trigger_type: None,
        display_qty: command.new_display_qty.or(existing.display_qty),
        min_fill_qty: command.new_min_fill_qty.or(existing.min_fill_qty),
        stp_group_id: existing.stp_group_id.clone(),
        is_market_maker: existing.is_market_maker,
    }
}

fn skipped_new_order_result(
    command: &NewOrderCommand,
    market_state: MarketState,
    partition: usize,
) -> SubmitOrderResult {
    SubmitOrderResult {
        metadata: command.metadata.clone(),
        order_id: command.client_order_id.clone(),
        market_state,
        fills: Vec::new(),
        state: OrderState::Active,
        remaining_amount: command.amount,
        partition,
        queue_wait_us: 0,
        match_execution_us: 0,
        persist_us: 0,
        timing: TimingBreakdown::default(),
    }
}

fn skipped_replace_order_result(
    command: &ReplaceOrderCommand,
    market_state: MarketState,
    partition: usize,
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
        partition,
        queue_wait_us: 0,
        match_execution_us: 0,
        persist_us: 0,
        timing: TimingBreakdown::default(),
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

    // ── Gate: instrument lifecycle status ──
    match instrument.status {
        types::InstrumentStatus::Halted => {
            return Err(SubmissionError::InstrumentHalted {
                instrument_id: instrument.instrument_id.clone(),
            });
        }
        types::InstrumentStatus::Delisted => {
            return Err(SubmissionError::InstrumentDelisted {
                instrument_id: instrument.instrument_id.clone(),
            });
        }
        types::InstrumentStatus::Settling => {
            return Err(SubmissionError::InstrumentHalted {
                instrument_id: instrument.instrument_id.clone(),
            });
        }
        types::InstrumentStatus::Active => {}
    }

    // ── Gate: per-instrument order type rule ──
    if let Some(ref rule) = instrument.order_type_rule {
        if !rule.allowed_order_types.contains(&command.order_type) {
            return Err(SubmissionError::UnsupportedOrderType {
                order_type: command.order_type,
            });
        }
        if !rule.allowed_tif.contains(&command.time_in_force) {
            return Err(SubmissionError::UnsupportedTimeInForce {
                time_in_force: command.time_in_force,
            });
        }
        if command.post_only && !rule.post_only_allowed {
            return Err(SubmissionError::PostOnlyWouldTake);
        }
        if command.reduce_only && !rule.reduce_only_allowed {
            return Err(SubmissionError::ReduceOnlyViolation { side: command.side });
        }
        if command.display_qty.is_some() && !rule.iceberg_allowed {
            return Err(SubmissionError::UnsupportedOrderType {
                order_type: command.order_type,
            });
        }
        if command.order_type.is_conditional() && !rule.conditional_allowed {
            return Err(SubmissionError::UnsupportedOrderType {
                order_type: command.order_type,
            });
        }
    }

    // ── Gate: per-user aggregate risk limits ──
    if let Some(price) = command.price {
        if price > 0 {
            risk.check_user_risk_limits(
                &command.user_id,
                instrument,
                command.outcome,
                command.amount,
                price,
            )
            .map_err(risk_error_to_submission)?;
        }
    }

    // ── Gate: tick size alignment ──
    if instrument.tick_size > 1 {
        if let Some(price) = command.price {
            if price % instrument.tick_size != 0 {
                return Err(SubmissionError::TickSizeViolation {
                    price,
                    tick_size: instrument.tick_size,
                });
            }
        }
    }

    // ── Gate: lot size alignment ──
    if instrument.lot_size > 1 && command.amount % instrument.lot_size != 0 {
        return Err(SubmissionError::LotSizeViolation {
            amount: command.amount,
            lot_size: instrument.lot_size,
        });
    }

    // ── Gate: minimum order amount ──
    if instrument.min_order_amount > 0 && command.amount < instrument.min_order_amount {
        return Err(SubmissionError::BelowMinAmount {
            amount: command.amount,
            min_order_amount: instrument.min_order_amount,
        });
    }

    // ── Gate: maximum notional ──
    if instrument.max_notional > 0 {
        if let Some(price) = command.price {
            let notional = price.saturating_mul(command.amount);
            if notional > instrument.max_notional {
                return Err(SubmissionError::ExceedsMaxNotional {
                    notional,
                    max_notional: instrument.max_notional,
                });
            }
        }
    }

    if command.reduce_only && command.side == Side::Buy {
        // Reduce-only buy is only valid for derivatives with a short position.
        if instrument_kind == InstrumentKind::Spot {
            return Err(SubmissionError::InvalidOrder(
                "reduce-only buy is not supported for spot",
            ));
        }
        let already_reserved = reserved_buy_reduce_only_amount_excluding(
            market,
            &command.user_id,
            replaced_order.map(|order| order.order_id.as_str()),
        );
        risk.ensure_reduce_only_buy_capacity(
            &command.user_id,
            &command.market_id,
            command.outcome,
            command.amount,
            already_reserved,
        )
        .map_err(|error| match error {
            RiskError::InsufficientReduceOnlyPosition => {
                SubmissionError::ReduceOnlyViolation { side: Side::Buy }
            }
            RiskError::OperationFailed(reason) => SubmissionError::Ledger(reason),
        })?;
    }

    if matches!(
        market.state,
        MarketState::Halted | MarketState::Closed | MarketState::Maintenance
    ) {
        return Err(SubmissionError::MarketClosed {
            market_id: market.market_id.clone(),
            outcome: market.outcome,
            state: market.state,
        });
    }
    if market.state == MarketState::CancelOnly && config.auto_recover_after_commands == 0 {
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
        return Err(SubmissionError::PostOnlyWouldTake);
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
            return Err(SubmissionError::InsufficientFunds {
                detail: "insufficient available cash".to_string(),
            });
        }
        if command.time_in_force == TimeInForce::Fok && estimate.executable_amount < command.amount
        {
            return Err(SubmissionError::InsufficientLiquidityForFok);
        }
        if instrument_kind != InstrumentKind::Spot {
            let leverage = normalized_command_leverage(instrument, command)?.unwrap_or(1);
            let required = estimate.required_reserve;
            if required > available_cash {
                return Err(SubmissionError::InsufficientFunds {
                    detail: "insufficient available margin".to_string(),
                });
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
                SubmissionError::ReduceOnlyViolation { side: Side::Sell }
            }
            RiskError::OperationFailed(reason) => SubmissionError::Ledger(reason),
        })?;
    }

    let incoming = RestingOrder::from_new_order(command.clone());
    let self_trade_ids = self_trade_resting_ids(market, &incoming);
    if !self_trade_ids.is_empty() {
        match incoming.stp_mode {
            StpMode::CancelTaker => {
                return Err(SubmissionError::SelfTradePrevented(
                    incoming.order_id.clone(),
                ));
            }
            StpMode::CancelMaker | StpMode::CancelBoth => {
                // Remove conflicting resting orders from the book.
                for order_id in &self_trade_ids {
                    if let Some(resting) = market.orders.remove(order_id) {
                        market.client_order_ids.remove(order_id);
                        market.remove_from_book(&resting);
                        market.remove_order_indexes(
                            &resting.order_id,
                            &resting.user_id,
                            resting.session_id.as_deref(),
                        );
                        let _ = release_order_reservation(risk, instrument, &resting, "stp");
                    }
                }
                if incoming.stp_mode == StpMode::CancelBoth {
                    return Err(SubmissionError::SelfTradePrevented(
                        incoming.order_id.clone(),
                    ));
                }
            }
        }
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
                return Err(SubmissionError::InsufficientFunds {
                    detail: "insufficient available cash".to_string(),
                });
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
                return Err(SubmissionError::InsufficientFunds {
                    detail: "insufficient available position".to_string(),
                });
            }
        }
        (kind, _, OrderType::Limit) if kind.is_derivative() => {
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
                        remaining = remaining.saturating_sub(order.remaining_amount);
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
                        remaining = remaining.saturating_sub(order.remaining_amount);
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
                            _ => (((remaining_cash as i128) * (leverage as i128))
                                / (*price as i128))
                                .clamp(0, i64::MAX as i128) as i64,
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
                        _ => required_margin(notional, leverage)?,
                    };
                    estimate.terminal_price = Some(*price);
                    remaining_amount -= executable_amount;
                    remaining_cash = remaining_cash.saturating_sub(match instrument_kind {
                        InstrumentKind::Spot => notional,
                        _ => required_margin(notional, leverage)?,
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
                        _ => required_margin(notional, leverage)?,
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

fn self_trade_resting_ids(market: &MarketRuntime, incoming: &RestingOrder) -> Vec<String> {
    // Self-trade match: same user_id OR same stp_group_id (when set).
    let is_self_trade = |resting: &&RestingOrder| -> bool {
        if resting.user_id == incoming.user_id {
            return true;
        }
        matches!(
            (&incoming.stp_group_id, &resting.stp_group_id),
            (Some(a), Some(b)) if a == b
        )
    };

    match incoming.side {
        Side::Buy => market
            .asks
            .iter()
            .take_while(|(price, _)| incoming.crosses_price(**price))
            .flat_map(|(_, queue)| queue.iter())
            .filter_map(|order_id| market.orders.get(order_id))
            .filter(is_self_trade)
            .map(|resting| resting.order_id.clone())
            .collect(),
        Side::Sell => market
            .bids
            .iter()
            .rev()
            .take_while(|(price, _)| incoming.crosses_price(**price))
            .flat_map(|(_, queue)| queue.iter())
            .filter_map(|order_id| market.orders.get(order_id))
            .filter(is_self_trade)
            .map(|resting| resting.order_id.clone())
            .collect(),
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

fn reserved_buy_reduce_only_amount_excluding(
    market: &MarketRuntime,
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
                .filter(|order| order.side == Side::Buy && order.reduce_only)
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
    cost_store: Option<&dyn PositionCostStore>,
    settlement_store: Option<&dyn WalStore<TradeSettlementRecord>>,
    seen_trade_ids: &mut HashSet<String>,
    settlement_statuses: &mut HashMap<String, TradeSettlementStatus>,
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
        // Take ownership instead of cloning �?avoids ~200-byte deep copy per fill.
        let Some(resting_snapshot) = market.orders.get(&resting_order_id) else {
            continue;
        };
        let mut resting = resting_snapshot.clone();
        if resting.user_id == incoming.user_id {
            break;
        }

        let mut executed_amount = incoming.remaining_amount.min(resting.remaining_amount);
        // ── Iceberg: cap match at the resting order's visible portion ──
        if resting.display_qty.is_some() {
            executed_amount = executed_amount.min(resting.visible_qty);
        }
        // ── Minimum fill quantity check ──
        // If either side requires a minimum fill and this match is too small, skip.
        if let Some(min) = resting.min_fill_qty {
            if executed_amount < min && executed_amount < resting.remaining_amount {
                break; // cannot meet minimum, skip this level
            }
        }
        if let Some(min) = incoming.min_fill_qty {
            if executed_amount < min && executed_amount < incoming.remaining_amount {
                break;
            }
        }
        if incoming.order_type == OrderType::Market {
            executed_amount = executed_amount.min(match instrument_kind {
                InstrumentKind::Spot if incoming.side == Side::Buy => {
                    incoming.reserved_cash / best_price
                }
                InstrumentKind::Spot => i64::MAX,
                _ => (((incoming.reserved_cash as i128) * (order_leverage(incoming) as i128))
                    / (best_price as i128))
                    .clamp(0, i64::MAX as i128) as i64,
            });
            if executed_amount <= 0 {
                break;
            }
        }
        let trade_id = trade_id_for_fill(incoming, partition_id, fill_index);
        fill_index = fill_index.saturating_add(1);
        // Hoist buyer/seller references to avoid repeated branching and cloning.
        let (buyer, seller): (&RestingOrder, &RestingOrder) = if incoming.side == Side::Buy {
            (&*incoming, &resting)
        } else {
            (&resting, &*incoming)
        };
        let buy_intent_id = buyer.order_id.clone();
        let buy_user_id = buyer.user_id.clone();
        let buy_market_id = buyer.market_id.clone();
        let buy_outcome = buyer.outcome;
        let sell_intent_id = seller.order_id.clone();
        let sell_user_id = seller.user_id.clone();
        let sell_market_id = seller.market_id.clone();
        let sell_outcome = seller.outcome;
        let resting_user_id_for_mm = resting.user_id.clone();
        let Some(_notional) = best_price.checked_mul(executed_amount) else {
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
        // ── Settlement + persistence timing ──
        let settlement_start = Instant::now();
        // ── Fee calculation ──
        let notional = best_price.saturating_mul(executed_amount);
        let taker_is_incoming = true; // incoming order is always the taker
        let (mut incoming_fee_bps, mut resting_fee_bps) = if taker_is_incoming {
            (instrument.taker_fee_bps, instrument.maker_fee_bps)
        } else {
            (instrument.maker_fee_bps, instrument.taker_fee_bps)
        };
        // Override with volume-based fee schedule: resolve each side independently.
        if let Some(ref schedule) = instrument.fee_schedule {
            let taker_vol = market
                .user_volume_30d
                .get(&incoming.user_id)
                .copied()
                .unwrap_or(0);
            let maker_vol = market
                .user_volume_30d
                .get(&resting_user_id_for_mm)
                .copied()
                .unwrap_or(0);
            let (_, sched_taker) = schedule.resolve(
                taker_vol,
                instrument.maker_fee_bps,
                instrument.taker_fee_bps,
            );
            let (sched_maker, _) = schedule.resolve(
                maker_vol,
                instrument.maker_fee_bps,
                instrument.taker_fee_bps,
            );
            incoming_fee_bps = sched_taker;
            resting_fee_bps = sched_maker;
        }
        // Signed fee: positive = charge, negative = rebate
        let incoming_fee = (notional as i128 * incoming_fee_bps as i128 / 10_000) as i64;
        let resting_fee = (notional as i128 * resting_fee_bps as i128 / 10_000) as i64;
        let record = TradeJournalRecord {
            partition_id,
            trade_id: trade_id.clone(),
            market_id: market.market_id.clone(),
            outcome: market.outcome,
            instrument_kind,
            buy_order_id: buy_intent_id.clone(),
            buy_user_id: buy_user_id.clone(),
            sell_order_id: sell_intent_id.clone(),
            sell_user_id: sell_user_id.clone(),
            price: best_price,
            amount: executed_amount,
            maker_fee: resting_fee,
            taker_fee: incoming_fee,
            aggressor_side: Some(incoming.side),
            recorded_at: Utc::now(),
        };
        if !matches!(
            settlement_statuses.get(&trade_id),
            Some(TradeSettlementStatus::Prepared | TradeSettlementStatus::Applied)
        ) {
            append_trade_settlement_record(
                settlement_store,
                &record,
                instrument_kind,
                &settle_op,
                &rollback_settle_op,
                TradeSettlementStatus::Prepared,
            )?;
            settlement_statuses.insert(trade_id.clone(), TradeSettlementStatus::Prepared);
        }
        // ── Write trade journal BEFORE ledger commit (C-2 fix) ──
        // This ensures that if we crash after ledger commit, the trade journal
        // already has the record. The settlement WAL "Prepared" status enables
        // recovery to detect and reconcile.
        if !seen_trade_ids.contains(&trade_id) {
            if let Some(store) = trade_store {
                if let Err(error) = store.append(&record) {
                    tracing::error!("trade journal append failed before settlement: {}", error);
                    let _ = append_trade_settlement_record(
                        settlement_store,
                        &record,
                        instrument_kind,
                        &settle_op,
                        &rollback_settle_op,
                        TradeSettlementStatus::Failed,
                    );
                    settlement_statuses.insert(trade_id.clone(), TradeSettlementStatus::Failed);
                    market.state = MarketState::Halted;
                    outcome.aborted = Some(SubmissionError::Persistence {
                        component: "trade_journal",
                        detail: error.to_string(),
                    });
                    break;
                }
            }
        }
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
            let _ = append_trade_settlement_record(
                settlement_store,
                &record,
                instrument_kind,
                &settle_op,
                &rollback_settle_op,
                TradeSettlementStatus::Failed,
            );
            settlement_statuses.insert(trade_id.clone(), TradeSettlementStatus::Failed);
            market.state = MarketState::Halted;
            outcome.aborted = Some(SubmissionError::Ledger(error.to_string()));
            break;
        }
        if !seen_trade_ids.contains(&trade_id) {
            if let Some(store) = cost_store {
                if let Err(error) = store.record_trade(&record) {
                    tracing::error!(trade_id = %trade_id, error = %error, "position cost projection write failed; core settlement remains committed");
                }
            }
            seen_trade_ids.insert(trade_id.clone());
        }
        // ── Fee collection (failure halts market to prevent revenue leakage) ──
        // Positive fee = charge user → SYS:FEE_COLLECTOR
        // Negative fee = rebate SYS:FEE_COLLECTOR → user
        if incoming_fee > 0 {
            let fee_op = format!("fee_taker_{trade_id}");
            if let Err(e) = ignore_duplicate_ledger_result(risk.ledger().collect_fee(
                &incoming.user_id,
                incoming_fee,
                fee_op,
            )) {
                tracing::error!(trade_id = %trade_id, fee = incoming_fee, error = %e, "taker fee collection failed – halting market");
                market.state = MarketState::Halted;
                outcome.aborted = Some(SubmissionError::Ledger(format!(
                    "taker fee collection failed: {e}"
                )));
                break;
            }
        } else if incoming_fee < 0 {
            let fee_op = format!("rebate_taker_{trade_id}");
            if let Err(e) =
                ignore_duplicate_ledger_result(risk.ledger().transfer_cash_between_accounts(
                    "SYS:FEE_COLLECTOR:USDC",
                    &ledger::LedgerService::cash_account(&incoming.user_id),
                    -incoming_fee,
                    fee_op,
                ))
            {
                tracing::error!(trade_id = %trade_id, rebate = -incoming_fee, error = %e, "taker rebate failed – halting market");
                market.state = MarketState::Halted;
                outcome.aborted =
                    Some(SubmissionError::Ledger(format!("taker rebate failed: {e}")));
                break;
            }
        }
        if resting_fee > 0 {
            let fee_op = format!("fee_maker_{trade_id}");
            if let Err(e) = ignore_duplicate_ledger_result(risk.ledger().collect_fee(
                &resting.user_id,
                resting_fee,
                fee_op,
            )) {
                tracing::error!(trade_id = %trade_id, fee = resting_fee, error = %e, "maker fee collection failed – halting market");
                market.state = MarketState::Halted;
                outcome.aborted = Some(SubmissionError::Ledger(format!(
                    "maker fee collection failed: {e}"
                )));
                break;
            }
        } else if resting_fee < 0 {
            let fee_op = format!("rebate_maker_{trade_id}");
            if let Err(e) =
                ignore_duplicate_ledger_result(risk.ledger().transfer_cash_between_accounts(
                    "SYS:FEE_COLLECTOR:USDC",
                    &ledger::LedgerService::cash_account(&resting.user_id),
                    -resting_fee,
                    fee_op,
                ))
            {
                tracing::error!(trade_id = %trade_id, rebate = -resting_fee, error = %e, "maker rebate failed – halting market");
                market.state = MarketState::Halted;
                outcome.aborted =
                    Some(SubmissionError::Ledger(format!("maker rebate failed: {e}")));
                break;
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
            _ => {
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
        market.trade_stats.record(best_price, executed_amount);
        // Increment per-user trailing volume (used for fee tier resolution).
        // Bounded map size to prevent memory exhaustion — evict oldest when capacity reached.
        let fill_notional = best_price.saturating_mul(executed_amount);
        if market.user_volume_30d.len() >= MAX_VOLUME_ENTRIES {
            // Evict 10% of oldest entries when capacity is reached.
            let to_remove = MAX_VOLUME_ENTRIES / 10;
            let mut keys: Vec<String> = market.user_volume_30d.keys().cloned().collect();
            keys.sort();
            for key in keys.into_iter().take(to_remove) {
                market.user_volume_30d.remove(&key);
            }
        }
        // Credit trading volume to both counterparties, avoiding double-count
        // when self-trades are permitted (same user as maker and taker).
        let vol_in = market
            .user_volume_30d
            .entry(incoming.user_id.clone())
            .or_insert(0);
        *vol_in = vol_in.saturating_add(fill_notional);
        if resting_user_id_for_mm != incoming.user_id {
            let vol_resting = market
                .user_volume_30d
                .entry(resting_user_id_for_mm.clone())
                .or_insert(0);
            *vol_resting = vol_resting.saturating_add(fill_notional);
        }
        // Evict old fills from MM trackers to prevent unbounded growth.
        if let Some(mm_config) = &instrument.mm_protection {
            if let Some(tracker) = market.mm_fill_trackers.get_mut(&resting_user_id_for_mm) {
                tracker.evict_old(Duration::from_secs(mm_config.window_secs));
                tracker.cap_fills(10_000);
            }
        }
        // Cap total MM tracker count.
        if market.mm_fill_trackers.len() > MAX_MM_TRACKERS {
            let to_remove = MAX_MM_TRACKERS / 10;
            let mut keys: Vec<String> = market.mm_fill_trackers.keys().cloned().collect();
            keys.sort();
            for key in keys.into_iter().take(to_remove) {
                market.mm_fill_trackers.remove(&key);
            }
        }
        market.remove_from_book(&resting);
        let _ = market.orders.remove(&resting.order_id);
        market.client_order_ids.remove(&resting.order_id);
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
            fee: if incoming.side == Side::Buy {
                incoming_fee
            } else {
                resting_fee
            },
            fee_bps: if incoming.side == Side::Buy {
                incoming_fee_bps
            } else {
                resting_fee_bps
            },
            is_maker: incoming.side != Side::Buy,
            aggressor_side: Some(incoming.side),
            fill_index: (fill_index - 1) as u32,
            settlement_status: Default::default(),
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
            fee: if incoming.side == Side::Sell {
                incoming_fee
            } else {
                resting_fee
            },
            fee_bps: if incoming.side == Side::Sell {
                incoming_fee_bps
            } else {
                resting_fee_bps
            },
            is_maker: incoming.side != Side::Sell,
            aggressor_side: Some(incoming.side),
            fill_index: (fill_index - 1) as u32,
            settlement_status: Default::default(),
        };
        event_bus.publish(Event::FillCreated(buy_fill.clone()));
        event_bus.publish(Event::FillCreated(sell_fill.clone()));
        outcome.fills.push(buy_fill);
        outcome.fills.push(sell_fill);

        if resting.remaining_amount > 0 {
            // ── Iceberg replenishment ──
            // If this is an iceberg order, the visible portion was consumed.
            // Replenish visible_qty from hidden reserve and reinsert at the
            // BACK of the queue (standard iceberg semantics: loses priority).
            if let Some(display) = resting.display_qty {
                resting.visible_qty = display.min(resting.remaining_amount);
                // Reinsert at back (not front) �?iceberg loses time priority on replenish
                insert_resting_order(market, resting);
            } else {
                // Regular (non-iceberg) partially-filled order keeps FIFO priority.
                insert_resting_front(market, resting);
            }
        } else {
            let _ = release_order_reservation(risk, instrument, &resting, "filled");
            market.remove_order_indexes(
                &resting.order_id,
                &resting.user_id,
                resting.session_id.as_deref(),
            );
        }

        if !matches!(
            settlement_statuses.get(&trade_id),
            Some(TradeSettlementStatus::Applied)
        ) {
            if let Err(error) = append_trade_settlement_record(
                settlement_store,
                &record,
                instrument_kind,
                &settle_op,
                &rollback_settle_op,
                TradeSettlementStatus::Applied,
            ) {
                tracing::error!(trade_id = %trade_id, error = %error, "settlement applied marker append failed after core commit");
            } else {
                settlement_statuses.insert(trade_id.clone(), TradeSettlementStatus::Applied);
            }
        }

        apply_trade_price_guard(market, config, best_price);

        // Accumulate settlement + persistence timing for this fill iteration.
        outcome.settlement_persist_us += settlement_start.elapsed().as_micros() as u64;

        // Market-maker protection: track fills and break if limits exceeded.
        if let Some(ref mmp) = instrument.mm_protection {
            let resting_side = opposite_side(incoming.side);
            if check_mm_protection(
                market,
                mmp,
                &resting_user_id_for_mm,
                executed_amount,
                resting_side,
                best_price,
            )
            .is_err()
            {
                break; // current fill already committed; stop matching further
            }
        }

        // Auto circuit-breaker: if instrument has a circuit breaker config, evaluate it.
        if let Some(ref cb) = instrument.circuit_breaker {
            apply_circuit_breaker(market, cb, best_price);
        }
        if market.state == MarketState::Halted {
            break;
        }
    }
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn ignore_duplicate_ledger_result(result: anyhow::Result<()>) -> anyhow::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.to_string().contains("duplicate op_id") => Ok(()),
        Err(error) => Err(error),
    }
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

fn append_trade_settlement_record(
    settlement_store: Option<&dyn WalStore<TradeSettlementRecord>>,
    trade_record: &TradeJournalRecord,
    instrument_kind: InstrumentKind,
    settle_op_id: &str,
    rollback_op_id: &str,
    status: TradeSettlementStatus,
) -> Result<(), SubmissionError> {
    let Some(store) = settlement_store else {
        return Ok(());
    };
    store
        .append(&TradeSettlementRecord {
            partition_id: trade_record.partition_id,
            trade_id: trade_record.trade_id.clone(),
            market_id: trade_record.market_id.clone(),
            outcome: trade_record.outcome,
            instrument_kind,
            buy_order_id: trade_record.buy_order_id.clone(),
            buy_user_id: trade_record.buy_user_id.clone(),
            sell_order_id: trade_record.sell_order_id.clone(),
            sell_user_id: trade_record.sell_user_id.clone(),
            price: trade_record.price,
            amount: trade_record.amount,
            settle_op_id: settle_op_id.to_string(),
            rollback_op_id: rollback_op_id.to_string(),
            status,
            recorded_at: Utc::now(),
        })
        .map_err(|error| SubmissionError::Persistence {
            component: "trade_settlement",
            detail: error.to_string(),
        })
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
                    order.reserved_position = order.remaining_amount;
                    return Ok(reserve_ids);
                }
            }
        },
        _ => {
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
            _ => risk
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
            market.client_order_ids.remove(&order_id);
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

/// Apply circuit breaker logic after a trade: record price, compute rolling
/// volatility, and auto-transition market state if thresholds are exceeded.
fn apply_circuit_breaker(
    market: &mut MarketRuntime,
    cb: &types::CircuitBreakerConfig,
    trade_price: i64,
) {
    // Record trade price in rolling window
    market.recent_trade_prices.push_back(trade_price);
    while market.recent_trade_prices.len() > cb.vol_lookback_trades {
        market.recent_trade_prices.pop_front();
    }
    if market.recent_trade_prices.len() < 2 {
        return;
    }

    // Check cooldown
    if let Some(triggered_at) = market.circuit_breaker_triggered_at {
        if triggered_at.elapsed() < Duration::from_secs(cb.cooldown_secs) {
            return;
        }
    }

    // Compute realized volatility as max price range in bps
    let prices = &market.recent_trade_prices;
    let min_p = prices.iter().copied().min().unwrap_or(1).max(1);
    let max_p = prices.iter().copied().max().unwrap_or(1);
    let range_bps = ((max_p - min_p) as i128 * 10_000 / min_p as i128) as i64;

    let new_state = if range_bps >= cb.halt_threshold_bps {
        Some(MarketState::Halted)
    } else if range_bps >= cb.cancel_only_threshold_bps {
        Some(MarketState::CancelOnly)
    } else if range_bps >= cb.stress_threshold_bps {
        Some(MarketState::Stress)
    } else {
        None
    };

    if let Some(target) = new_state {
        if market.state.can_transition_to(target) {
            tracing::warn!(
                market_id = %market.market_id,
                from = ?market.state,
                to = ?target,
                volatility_bps = range_bps,
                "circuit breaker triggered — auto-transitioning market state"
            );
            market.state = target;
            market.circuit_breaker_triggered_at = Some(Instant::now());
        }
    }
}

/// Check market-maker protection limits. Returns Err if the MM has exceeded
/// delta or notional thresholds within the rolling window.
fn check_mm_protection(
    market: &mut MarketRuntime,
    mmp: &types::MarketMakerProtection,
    user_id: &str,
    fill_qty: i64,
    fill_side: Side,
    fill_price: i64,
) -> Result<(), SubmissionError> {
    let window = Duration::from_secs(mmp.window_secs);
    let tracker = market
        .mm_fill_trackers
        .entry(user_id.to_string())
        .or_insert_with(MmFillTracker::new);
    tracker.record_fill(fill_qty, fill_side, fill_price);
    tracker.evict_old(window);

    if mmp.max_delta_qty > 0 && tracker.net_delta().abs() > mmp.max_delta_qty {
        return Err(SubmissionError::InvalidOrder(
            "market maker delta limit exceeded",
        ));
    }
    if mmp.max_notional_window > 0 && tracker.total_notional() > mmp.max_notional_window {
        return Err(SubmissionError::InvalidOrder(
            "market maker notional limit exceeded",
        ));
    }
    Ok(())
}

/// Fat-finger guard: reject orders with amount exceeding instrument's max_order_amount.
fn validate_fat_finger(
    instrument: &InstrumentSpec,
    command: &NewOrderCommand,
) -> Result<(), SubmissionError> {
    if instrument.max_order_amount > 0 && command.amount > instrument.max_order_amount {
        return Err(SubmissionError::InvalidOrder(
            "order amount exceeds fat-finger limit",
        ));
    }
    Ok(())
}

fn calculate_snapshot_checksum(
    partition_id: usize,
    kill_switch_enabled: bool,
    snapshot: &PartitionStateSnapshot,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    partition_id.hash(&mut hasher);
    kill_switch_enabled.hash(&mut hasher);
    let json = serde_json::to_string(snapshot).expect("snapshot serialization must not fail");
    json.hash(&mut hasher);
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
    // Conditional orders must have a trigger_price.
    if command.order_type.is_conditional() {
        if command.trigger_price.is_none() {
            return Err(SubmissionError::InvalidOrder(
                "conditional order requires trigger_price",
            ));
        }
        if let Some(tp) = command.trigger_price {
            if tp <= 0 {
                return Err(SubmissionError::InvalidOrder(
                    "trigger_price must be positive",
                ));
            }
        }
        // StopLimit / TakeProfitLimit also require a limit price.
        if matches!(
            command.order_type,
            OrderType::StopLimit | OrderType::TakeProfitLimit
        ) && command.price.is_none()
        {
            return Err(SubmissionError::InvalidOrder(
                "stop-limit / take-profit-limit order requires price",
            ));
        }
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
    // GTD is contradictory with IOC/FOK �?they have immediate lifetime semantics.
    if matches!(command.time_in_force, TimeInForce::Ioc | TimeInForce::Fok)
        && command.expires_at.is_some()
    {
        return Err(SubmissionError::InvalidOrder(
            "IOC/FOK orders must not have expires_at",
        ));
    }
    // post_only + IOC is contradictory: post_only guarantees maker, IOC cancels unfilled.
    if command.post_only && command.time_in_force == TimeInForce::Ioc {
        return Err(SubmissionError::InvalidOrder(
            "post_only and IOC are contradictory",
        ));
    }
    if let Some(display) = command.display_qty {
        if display < 0 {
            return Err(SubmissionError::InvalidOrder(
                "display_qty must be non-negative",
            ));
        }
        if display > 0 && display > command.amount {
            return Err(SubmissionError::InvalidOrder(
                "display_qty must not exceed amount",
            ));
        }
        if display > 0 && command.order_type != OrderType::Limit {
            return Err(SubmissionError::InvalidOrder(
                "iceberg orders must be limit orders",
            ));
        }
    }
    if let Some(min_fill) = command.min_fill_qty {
        if min_fill < 0 {
            return Err(SubmissionError::InvalidOrder(
                "min_fill_qty must be non-negative",
            ));
        }
        if min_fill > command.amount {
            return Err(SubmissionError::InvalidOrder(
                "min_fill_qty must not exceed amount",
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
        kind if kind.is_derivative() => {
            let leverage = leverage.unwrap_or(1);
            let max_leverage = instrument.max_leverage.unwrap_or(MAX_LEVERAGE);
            if !(1..=max_leverage).contains(&leverage) {
                return Err(SubmissionError::InvalidOrder(
                    "invalid leverage for leveraged market",
                ));
            }
            Ok(Some(leverage))
        }
        // Defensive: treat any unknown future kind as derivative.
        _ => Ok(Some(leverage.unwrap_or(1))),
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
            SubmissionError::ReduceOnlyViolation { side: Side::Sell }
        }
        RiskError::OperationFailed(reason) => match reason.as_str() {
            "amount must be positive" => SubmissionError::InvalidOrder("amount must be positive"),
            "price must be positive" => SubmissionError::InvalidOrder("price must be positive"),
            "invalid leverage" => SubmissionError::InvalidOrder("invalid leverage"),
            "leverage exceeds instrument maximum" => SubmissionError::ExceedsMaxLeverage {
                leverage: 0,
                max_leverage: 0,
            },
            "spot orders do not support leverage" => {
                SubmissionError::InvalidOrder("spot orders do not support leverage")
            }
            _ if reason.starts_with("position notional") => {
                SubmissionError::InvalidOrder("position limit exceeded")
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
        OrderType::Limit | OrderType::StopLimit | OrderType::TakeProfitLimit => command.price,
        OrderType::Market | OrderType::StopMarket | OrderType::TakeProfitMarket => None,
    }
}

fn deviation_bps(reference_price: i64, attempted_price: i64) -> i64 {
    if reference_price <= 0 {
        return i64::MAX; // treat zero/negative reference as maximal deviation
    }
    let diff = (attempted_price as i128 - reference_price as i128).abs();
    (diff * 10_000 / (reference_price as i128)).min(i64::MAX as i128) as i64
}

/// Determine if a trigger condition is met.
/// Stop buy / take-profit sell: trigger when `reference >= trigger_price`.
/// Stop sell / take-profit buy: trigger when `reference <= trigger_price`.
fn is_trigger_met(command: &NewOrderCommand, trigger_price: i64, reference: i64) -> bool {
    match (command.order_type, command.side) {
        (OrderType::StopMarket | OrderType::StopLimit, Side::Buy) => reference >= trigger_price,
        (OrderType::StopMarket | OrderType::StopLimit, Side::Sell) => reference <= trigger_price,
        (OrderType::TakeProfitMarket | OrderType::TakeProfitLimit, Side::Buy) => {
            reference <= trigger_price
        }
        (OrderType::TakeProfitMarket | OrderType::TakeProfitLimit, Side::Sell) => {
            reference >= trigger_price
        }
        _ => false,
    }
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
        MarketState::Stress | MarketState::PreOpen => 1,
        MarketState::AuctionCall => 2,
        MarketState::CancelOnly | MarketState::Maintenance => 3,
        MarketState::Halted => 4,
        MarketState::Closed => 5,
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
    use types::InstrumentStatus;
    use types::{
        CancelOrderCommand, Command, CommandLifecycle, CommandMetadata, FeeSchedule, FeeTier,
        LedgerDelta, OrderType, ReplaceOrderCommand, Side, TimeInForce,
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
            ..Default::default()
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
            stp_mode: StpMode::default(),
            trigger_price: None,
            trigger_type: None,
            display_qty: None,
            min_fill_qty: None,
            stp_group_id: None,
            is_market_maker: false,
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
                new_display_qty: None,
                new_min_fill_qty: None,
                new_trigger_price: None,
                new_trigger_type: None,
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

    #[derive(Default)]
    struct FailingSettlementWal {
        entries: Mutex<Vec<TradeSettlementRecord>>,
        fail: bool,
    }

    impl FailingSettlementWal {
        fn always_fail() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
                fail: true,
            }
        }
    }

    struct FailAfterNSettlementWal {
        entries: Mutex<Vec<TradeSettlementRecord>>,
        append_count: AtomicUsize,
        fail_on_append: usize,
    }

    impl FailAfterNSettlementWal {
        fn new(fail_on_append: usize) -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
                append_count: AtomicUsize::new(0),
                fail_on_append,
            }
        }
    }

    impl WalStore<TradeSettlementRecord> for FailAfterNSettlementWal {
        fn append(&self, record: &TradeSettlementRecord) -> anyhow::Result<()> {
            let append_no = self.append_count.fetch_add(1, Ordering::SeqCst) + 1;
            if append_no == self.fail_on_append {
                return Err(anyhow!(
                    "forced trade settlement wal failure on append {} for {}",
                    append_no,
                    record.trade_id
                ));
            }
            self.entries.lock().push(record.clone());
            Ok(())
        }

        fn entries(&self) -> anyhow::Result<Vec<TradeSettlementRecord>> {
            Ok(self.entries.lock().clone())
        }
    }

    #[derive(Default)]
    struct FailingSnapshotWal;

    impl WalStore<PartitionSnapshotRecord> for FailingSnapshotWal {
        fn append(&self, _record: &PartitionSnapshotRecord) -> anyhow::Result<()> {
            Err(anyhow!("forced snapshot wal failure"))
        }

        fn entries(&self) -> anyhow::Result<Vec<PartitionSnapshotRecord>> {
            Ok(Vec::new())
        }
    }

    impl WalStore<TradeSettlementRecord> for FailingSettlementWal {
        fn append(&self, record: &TradeSettlementRecord) -> anyhow::Result<()> {
            if self.fail {
                return Err(anyhow!(
                    "forced trade settlement wal failure for {}",
                    record.trade_id
                ));
            }
            self.entries.lock().push(record.clone());
            Ok(())
        }

        fn entries(&self) -> anyhow::Result<Vec<TradeSettlementRecord>> {
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
                new_display_qty: None,
                new_min_fill_qty: None,
                new_trigger_price: None,
                new_trigger_type: None,
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
                new_display_qty: None,
                new_min_fill_qty: None,
                new_trigger_price: None,
                new_trigger_type: None,
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
                new_display_qty: None,
                new_min_fill_qty: None,
                new_trigger_price: None,
                new_trigger_type: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(error, SubmissionError::InsufficientFunds { .. }));
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
                new_display_qty: None,
                new_min_fill_qty: None,
                new_trigger_price: None,
                new_trigger_type: None,
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
    async fn settlement_wal_failure_aborts_before_ledger_commit() {
        let ledger = seeded_ledger();
        let risk = seeded_risk_with_ledger(ledger.clone());
        let settlement_store: Arc<dyn WalStore<TradeSettlementRecord>> =
            Arc::new(FailingSettlementWal::always_fail());
        let engine = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config(),
            EventBus::new(),
            risk,
            shared_default_registry(),
            None,
            None,
            None,
            Some(settlement_store),
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

        assert!(matches!(
            error,
            SubmissionError::Persistence {
                component: _,
                detail: _
            }
        ));
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
        assert_eq!(snapshot.state, MarketState::Normal);
        assert_eq!(ledger.cash_available_balance("maker-1"), 1_000_000);
        assert_eq!(
            ledger.position_available_balance("taker", "btc-usdt", 0),
            1_000
        );
    }

    #[tokio::test]
    async fn replay_uses_prepared_settlement_wal_to_finish_trade_journal() {
        let snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        let trade_store = Arc::new(InMemoryWal::<TradeJournalRecord>::new());
        let settlement_store = Arc::new(InMemoryWal::<TradeSettlementRecord>::new());
        let risk = seeded_risk();
        let engine = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config(),
            EventBus::new(),
            risk.clone(),
            shared_default_registry(),
            Some(snapshot_store.clone()),
            Some(trade_store.clone()),
            None,
            Some(settlement_store.clone()),
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
        let partition_id = partition_for("btc-usdt", 0, config().partitions);
        let trade_id = format!("trade:seq-2:{partition_id}:0");
        settlement_store
            .append(&TradeSettlementRecord {
                partition_id,
                trade_id: trade_id.clone(),
                market_id: "btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Spot,
                buy_order_id: "buy-1".to_string(),
                buy_user_id: "taker".to_string(),
                sell_order_id: "ask-1".to_string(),
                sell_user_id: "maker-1".to_string(),
                price: 100,
                amount: 1,
                settle_op_id: trade_settle_op_id(&trade_id),
                rollback_op_id: rollback_settle_op_id(&trade_id),
                status: TradeSettlementStatus::Prepared,
                recorded_at: Utc::now(),
            })
            .unwrap();

        let stale_snapshot_store = Arc::new(InMemoryWal::<PartitionSnapshotRecord>::new());
        stale_snapshot_store.append(&stale_snapshot).unwrap();
        let recovered = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config(),
            EventBus::new(),
            risk,
            shared_default_registry(),
            Some(stale_snapshot_store),
            Some(trade_store.clone()),
            None,
            Some(settlement_store.clone()),
        )
        .unwrap();

        recovered
            .replay_command(Command::NewOrder(with_command_seq(
                new_order("req-buy-1", "buy-1", "taker", None, Side::Buy, 100, 1),
                2,
            )))
            .await
            .unwrap();

        assert_eq!(trade_store.entries().unwrap().len(), 1);
        let settlement_entries = settlement_store.entries().unwrap();
        assert!(settlement_entries.iter().any(|entry| {
            entry.trade_id == trade_id && entry.status == TradeSettlementStatus::Prepared
        }));
        assert!(settlement_entries.iter().any(|entry| {
            entry.trade_id == trade_id && entry.status == TradeSettlementStatus::Applied
        }));
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

        assert!(matches!(
            error,
            SubmissionError::Persistence {
                component: _,
                detail: _
            }
        ));
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

    #[tokio::test]
    async fn applied_settlement_marker_failure_does_not_abort_core_trade_commit() {
        let ledger = seeded_ledger();
        let risk = seeded_risk_with_ledger(ledger.clone());
        let trade_store = Arc::new(InMemoryWal::<TradeJournalRecord>::new());
        let settlement_store: Arc<dyn WalStore<TradeSettlementRecord>> =
            Arc::new(FailAfterNSettlementWal::new(2));
        let engine = PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config(),
            EventBus::new(),
            risk,
            shared_default_registry(),
            None,
            Some(trade_store.clone()),
            None,
            Some(settlement_store.clone()),
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
        let result = engine
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
            .unwrap();

        assert_eq!(result.fills.len(), 2);
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 0);
        assert_eq!(trade_store.entries().unwrap().len(), 1);
        let settlement_entries = settlement_store.entries().unwrap();
        assert_eq!(settlement_entries.len(), 1);
        assert_eq!(
            settlement_entries[0].status,
            TradeSettlementStatus::Prepared
        );
    }

    #[tokio::test]
    async fn snapshot_persistence_failure_does_not_flip_submit_result_to_error() {
        let snapshot_store: Arc<dyn WalStore<PartitionSnapshotRecord>> =
            Arc::new(FailingSnapshotWal);
        let engine = PartitionedMatchingEngine::with_snapshot_store(
            config(),
            EventBus::new(),
            seeded_risk(),
            snapshot_store,
        )
        .unwrap();

        let result = engine
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

        assert_eq!(result.state, OrderState::Active);
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
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
        assert!(matches!(error, SubmissionError::InsufficientFunds { .. }));

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

    // ── Trading-rules & fee tests ──────────────────────────────────────

    fn strict_instrument(market_id: &str) -> InstrumentSpec {
        InstrumentSpec {
            instrument_id: market_id.to_string(),
            kind: InstrumentKind::Spot,
            base_asset: String::new(),
            quote_asset: "USDC".to_string(),
            margin_mode: None,
            max_leverage: None,
            tick_size: 10,
            lot_size: 5,
            price_band_bps: 1_000,
            risk_policy_id: "spot-v1".to_string(),
            min_order_amount: 10,
            max_notional: 50_000,
            maker_fee_bps: 5,
            taker_fee_bps: 10,
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

    fn engine_with_instrument(
        risk: Arc<RiskEngine>,
        spec: InstrumentSpec,
    ) -> PartitionedMatchingEngine {
        let registry = instruments::InMemoryInstrumentRegistry::new();
        registry.register(spec);
        PartitionedMatchingEngine::new_with_registry(
            config(),
            EventBus::new(),
            risk,
            Arc::new(registry),
        )
    }

    #[tokio::test]
    async fn tick_size_violation_rejected() {
        let engine = engine_with_instrument(seeded_risk(), strict_instrument("btc-usdt"));
        // price 105 is not aligned to tick_size 10
        let err = engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 105, 10))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            SubmissionError::TickSizeViolation {
                price: 105,
                tick_size: 10
            }
        ));
    }

    #[tokio::test]
    async fn tick_size_aligned_accepted() {
        let engine = engine_with_instrument(seeded_risk(), strict_instrument("btc-usdt"));
        // price 100 is aligned to tick_size 10
        engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 10))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn lot_size_violation_rejected() {
        let engine = engine_with_instrument(seeded_risk(), strict_instrument("btc-usdt"));
        // amount 7 is not aligned to lot_size 5
        let err = engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 7))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            SubmissionError::LotSizeViolation {
                amount: 7,
                lot_size: 5
            }
        ));
    }

    #[tokio::test]
    async fn lot_size_aligned_accepted() {
        let engine = engine_with_instrument(seeded_risk(), strict_instrument("btc-usdt"));
        // amount 15 is aligned to lot_size 5
        engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 15))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn below_min_amount_rejected() {
        let engine = engine_with_instrument(seeded_risk(), strict_instrument("btc-usdt"));
        // amount 5 is below min_order_amount 10
        let err = engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 5))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            SubmissionError::BelowMinAmount {
                amount: 5,
                min_order_amount: 10
            }
        ));
    }

    #[tokio::test]
    async fn exceeds_max_notional_rejected() {
        let engine = engine_with_instrument(seeded_risk(), strict_instrument("btc-usdt"));
        // price 100 * amount 510 = 51_000 > max_notional 50_000
        // amount must be lot_size aligned (510 = 5*102) and >= min_order_amount (10)
        let err = engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 510))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            SubmissionError::ExceedsMaxNotional {
                notional: 51_000,
                max_notional: 50_000
            }
        ));
    }

    #[tokio::test]
    async fn within_max_notional_accepted() {
        let engine = engine_with_instrument(seeded_risk(), strict_instrument("btc-usdt"));
        // price 100 * amount 500 = 50_000 == max_notional => ok
        engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 500))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn fee_applied_on_trade() {
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine = engine_with_instrument(risk, strict_instrument("btc-usdt"));

        // Place resting sell (maker)
        engine
            .submit_new_order(new_order(
                "r1",
                "ask1",
                "maker-1",
                None,
                Side::Sell,
                100,
                10,
            ))
            .await
            .unwrap();

        // Place incoming buy (taker) �?matches at price 100, amount 10
        let result = engine
            .submit_new_order(new_order("r2", "bid1", "taker", None, Side::Buy, 100, 10))
            .await
            .unwrap();

        // notional = 100 * 10 = 1_000
        // taker_fee = 1_000 * 10 / 10_000 = 1
        // maker_fee = 1_000 * 5 / 10_000 = 0 (integer truncation)
        assert_eq!(result.fills.len(), 2);
        let buy_fill = result.fills.iter().find(|f| f.side == Side::Buy).unwrap();
        let sell_fill = result.fills.iter().find(|f| f.side == Side::Sell).unwrap();

        // Buy fill is the taker (incoming)
        assert_eq!(buy_fill.fee, 1);
        assert_eq!(buy_fill.fee_bps, 10);
        assert!(!buy_fill.is_maker);

        // Sell fill is the maker (resting)
        assert_eq!(sell_fill.fee, 0); // 1_000 * 5 / 10_000 = 0.5 �?truncated to 0
        assert_eq!(sell_fill.fee_bps, 5);
        assert!(sell_fill.is_maker);

        // Fee pool should have collected the taker fee
        assert_eq!(ledger.fee_collector_balance(), 1);
    }

    #[tokio::test]
    async fn fee_with_larger_notional() {
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine = engine_with_instrument(risk, strict_instrument("btc-usdt"));

        // Resting sell at price 100, amount 500 (notional = 50_000)
        engine
            .submit_new_order(new_order(
                "r1",
                "ask1",
                "maker-1",
                None,
                Side::Sell,
                100,
                500,
            ))
            .await
            .unwrap();
        // Taker buy
        let result = engine
            .submit_new_order(new_order("r2", "bid1", "taker", None, Side::Buy, 100, 500))
            .await
            .unwrap();

        // notional = 100 * 500 = 50_000
        // taker_fee = 50_000 * 10 / 10_000 = 50
        // maker_fee = 50_000 * 5 / 10_000 = 25
        let buy_fill = result.fills.iter().find(|f| f.side == Side::Buy).unwrap();
        let sell_fill = result.fills.iter().find(|f| f.side == Side::Sell).unwrap();

        assert_eq!(buy_fill.fee, 50);
        assert_eq!(sell_fill.fee, 25);

        // Fee pool = 50 + 25 = 75
        assert_eq!(ledger.fee_collector_balance(), 75);
    }

    // ── Tests for audit fixes ──────────────────────────────────────────

    #[test]
    fn deviation_bps_no_overflow_on_extreme_values() {
        // C-3: previously would overflow with large i64 values
        let result = deviation_bps(1, i64::MAX);
        assert!(result > 0, "deviation should be positive");
        // result is guaranteed to fit in i64 by construction.

        let result2 = deviation_bps(i64::MAX, 1);
        assert!(result2 > 0);

        // Normal case still works
        assert_eq!(deviation_bps(100, 110), 1000); // 10% = 1000 bps
        assert_eq!(deviation_bps(100, 100), 0);
    }

    #[tokio::test]
    async fn negative_fee_bps_instrument_pays_rebate() {
        // H-2: negative fee_bps pays rebate from FEE_COLLECTOR to user
        let registry = Arc::new(instruments::InMemoryInstrumentRegistry::new());
        registry.register(InstrumentSpec {
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
            maker_fee_bps: -50,
            taker_fee_bps: -100,
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
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let fee_pool_before = ledger.fee_collector_balance();

        let engine =
            PartitionedMatchingEngine::new_with_registry(config(), EventBus::new(), risk, registry);

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
        let result = engine
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
            .unwrap();

        // notional = 100*5 = 500
        // incoming_fee_bps = taker_fee_bps = -100 → fee = 500 * -100 / 10000 = -5 (rebate)
        // resting_fee_bps = maker_fee_bps = -50 → fee = 500 * -50 / 10000 = -2 (rebate)
        let taker_fill = result.fills.iter().find(|f| !f.is_maker).unwrap();
        let maker_fill = result.fills.iter().find(|f| f.is_maker).unwrap();
        assert_eq!(taker_fill.fee, -5, "taker rebate = -5");
        assert_eq!(taker_fill.fee_bps, -100);
        assert_eq!(maker_fill.fee, -2, "maker rebate = -2");
        assert_eq!(maker_fill.fee_bps, -50);

        // Fee pool DECREASES (rebates paid out)
        let fee_pool_after = ledger.fee_collector_balance();
        assert_eq!(
            fee_pool_after - fee_pool_before,
            -7,
            "fee pool should decrease by 7 for rebates"
        );
    }

    #[tokio::test]
    async fn fee_schedule_overrides_flat_instrument_fees() {
        // When an instrument has a fee_schedule, the matching engine uses the
        // schedule's base tier (volume=0) instead of the flat fee_bps fields.
        let registry = Arc::new(instruments::InMemoryInstrumentRegistry::new());
        registry.register(InstrumentSpec {
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
            maker_fee_bps: 99, // flat fallback (should NOT be used)
            taker_fee_bps: 99,
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
            fee_schedule: Some(FeeSchedule {
                name: "Standard".into(),
                tiers: vec![
                    FeeTier {
                        min_volume: 0,
                        maker_fee_bps: 5,
                        taker_fee_bps: 10,
                    },
                    FeeTier {
                        min_volume: 1_000_000,
                        maker_fee_bps: 2,
                        taker_fee_bps: 8,
                    },
                ],
                withdrawal_fee: 0,
                mm_rebate_enabled: false,
            }),
            margin_tiers: None,
            expiry: None,
            option_spec: None,
            settlement_currency: None,
        });
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine =
            PartitionedMatchingEngine::new_with_registry(config(), EventBus::new(), risk, registry);

        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
                "maker-1",
                None,
                Side::Sell,
                100,
                10,
            ))
            .await
            .unwrap();
        let result = engine
            .submit_new_order(new_order(
                "req-2",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                10,
            ))
            .await
            .unwrap();

        // notional = 100*10 = 1000
        // Base tier (volume=0): taker=10bps, maker=5bps
        // taker_fee = 1000 * 10 / 10000 = 1
        // maker_fee = 1000 * 5 / 10000 = 0 (truncated)
        let taker_fill = result.fills.iter().find(|f| !f.is_maker).unwrap();
        let maker_fill = result.fills.iter().find(|f| f.is_maker).unwrap();
        assert_eq!(taker_fill.fee, 1, "taker fee from schedule base tier");
        assert_eq!(taker_fill.fee_bps, 10, "taker fee_bps from schedule");
        assert_eq!(maker_fill.fee, 0, "maker fee truncated to 0");
        assert_eq!(maker_fill.fee_bps, 5, "maker fee_bps from schedule");

        // Verify fee collected (only taker fee since maker rounds to 0)
        assert_eq!(ledger.fee_collector_balance(), 1);
    }

    #[tokio::test]
    async fn reduce_only_buy_allowed_for_short_derivative_position() {
        // H-8: reduce-only buy should work for derivatives with short positions
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), risk);

        // Create a short position: maker sells, taker buys
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

        // maker-1 now has a short position of -5
        assert_eq!(
            ledger.derivative_position_balance("maker-1", "margin:btc-usdt", 0),
            -5
        );

        // Reduce-only buy should be accepted to close the short
        let mut reduce_buy = leveraged_order(
            "req-3",
            "reduce-buy-1",
            "maker-1",
            Side::Buy,
            "margin:btc-usdt",
            100,
            3,
            5,
        );
        reduce_buy.reduce_only = true;
        engine.submit_new_order(reduce_buy).await.unwrap();
    }

    #[tokio::test]
    async fn reduce_only_buy_rejected_for_spot() {
        // H-8: reduce-only buy is still rejected for spot markets
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        let mut buy = new_order("req-1", "ro-buy-1", "maker-1", None, Side::Buy, 100, 5);
        buy.reduce_only = true;
        let result = engine.submit_new_order(buy).await;
        assert!(
            result.is_err(),
            "reduce-only buy on spot should be rejected"
        );
    }

    // ── Trade statistics tests ────────────────────────────────────────

    #[tokio::test]
    async fn trade_statistics_track_volume_and_prices() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        engine
            .submit_new_order(new_order(
                "r1",
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
                "r2",
                "ask-2",
                "maker-2",
                None,
                Side::Sell,
                110,
                3,
            ))
            .await
            .unwrap();
        // Buy 5 at 100
        engine
            .submit_new_order(new_order("r3", "bid-1", "taker", None, Side::Buy, 100, 5))
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(snapshot.trade_stats.total_trades, 1);
        assert_eq!(snapshot.trade_stats.total_volume, 5);
        assert_eq!(snapshot.trade_stats.open_price, Some(100));
        assert_eq!(snapshot.trade_stats.high_price, Some(100));
        assert_eq!(snapshot.trade_stats.low_price, Some(100));
        assert_eq!(snapshot.trade_stats.last_trade_price, Some(100));
        assert!(snapshot.trade_stats.last_trade_timestamp.is_some());
    }

    #[tokio::test]
    async fn trade_statistics_accumulate_across_multiple_trades() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        engine
            .submit_new_order(new_order(
                "r1",
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
                "r2",
                "ask-2",
                "maker-2",
                None,
                Side::Sell,
                120,
                3,
            ))
            .await
            .unwrap();

        // Trade at 100
        engine
            .submit_new_order(new_order("r3", "bid-1", "taker", None, Side::Buy, 100, 5))
            .await
            .unwrap();
        // Trade at 120
        engine
            .submit_new_order(new_order("r4", "bid-2", "taker", None, Side::Buy, 120, 3))
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(snapshot.trade_stats.total_trades, 2);
        assert_eq!(snapshot.trade_stats.total_volume, 8);
        assert_eq!(snapshot.trade_stats.open_price, Some(100));
        assert_eq!(snapshot.trade_stats.high_price, Some(120));
        assert_eq!(snapshot.trade_stats.low_price, Some(100));
        assert_eq!(snapshot.trade_stats.last_trade_price, Some(120));
    }

    // ── Order book depth tests ────────────────────────────────────────

    #[tokio::test]
    async fn order_book_depth_aggregates_levels() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        engine
            .submit_new_order(new_order("r1", "bid-1", "maker-1", None, Side::Buy, 99, 5))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r2", "bid-2", "maker-2", None, Side::Buy, 99, 3))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r3", "bid-3", "u-1", None, Side::Buy, 98, 10))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r4", "ask-1", "u-2", None, Side::Sell, 101, 7))
            .await
            .unwrap();

        let depth = engine
            .order_book_depth("btc-usdt", 0, 10)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(depth.bids.len(), 2);
        // Best bid at 99 has 2 orders totalling 8
        assert_eq!(depth.bids[0].price, 99);
        assert_eq!(depth.bids[0].total_amount, 8);
        assert_eq!(depth.bids[0].order_count, 2);
        // Next bid at 98
        assert_eq!(depth.bids[1].price, 98);
        assert_eq!(depth.bids[1].total_amount, 10);
        assert_eq!(depth.bids[1].order_count, 1);
        // Asks
        assert_eq!(depth.asks.len(), 1);
        assert_eq!(depth.asks[0].price, 101);
        assert_eq!(depth.asks[0].total_amount, 7);
    }

    #[tokio::test]
    async fn order_book_depth_respects_max_levels() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        for i in 0..5 {
            engine
                .submit_new_order(new_order(
                    &format!("r-{i}"),
                    &format!("bid-{i}"),
                    "maker-1",
                    None,
                    Side::Buy,
                    100 - (i + 1) as i64,
                    1,
                ))
                .await
                .unwrap();
        }

        let depth = engine
            .order_book_depth("btc-usdt", 0, 3)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(depth.bids.len(), 3);
        // Should return the 3 best (highest) bid levels
        assert_eq!(depth.bids[0].price, 99);
        assert_eq!(depth.bids[1].price, 98);
        assert_eq!(depth.bids[2].price, 97);
    }

    // ── Enhanced market snapshot tests ────────────────────────────────

    #[tokio::test]
    async fn market_snapshot_includes_depth_and_spread() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        engine
            .submit_new_order(new_order("r1", "bid-1", "maker-1", None, Side::Buy, 98, 5))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r2", "bid-2", "maker-2", None, Side::Buy, 99, 3))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r3", "ask-1", "u-1", None, Side::Sell, 101, 10))
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(snapshot.best_bid, Some(99));
        assert_eq!(snapshot.best_ask, Some(101));
        assert_eq!(snapshot.mid_price, Some(100));
        assert_eq!(snapshot.spread, Some(2));
        assert_eq!(snapshot.total_bid_depth, 8);
        assert_eq!(snapshot.total_ask_depth, 10);
        assert_eq!(snapshot.bid_levels, 2);
        assert_eq!(snapshot.ask_levels, 1);
    }

    // ── Rate limiter tests ───────────────────────────────────────────

    #[tokio::test]
    async fn rate_limiter_rejects_when_exceeded() {
        let mut cfg = config();
        cfg.max_orders_per_window_per_user = 2;
        cfg.order_rate_window = Duration::from_secs(60);
        let engine = PartitionedMatchingEngine::new(cfg, EventBus::new(), seeded_risk());

        engine
            .submit_new_order(new_order("r1", "bid-1", "u-1", None, Side::Buy, 99, 1))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r2", "bid-2", "u-1", None, Side::Buy, 98, 1))
            .await
            .unwrap();
        let error = engine
            .submit_new_order(new_order("r3", "bid-3", "u-1", None, Side::Buy, 97, 1))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            SubmissionError::RateLimited { limit: 2, .. }
        ));
    }

    #[tokio::test]
    async fn rate_limiter_allows_different_users() {
        let mut cfg = config();
        cfg.max_orders_per_window_per_user = 1;
        cfg.order_rate_window = Duration::from_secs(60);
        let engine = PartitionedMatchingEngine::new(cfg, EventBus::new(), seeded_risk());

        engine
            .submit_new_order(new_order("r1", "bid-1", "u-1", None, Side::Buy, 99, 1))
            .await
            .unwrap();
        // Different user should still be allowed
        engine
            .submit_new_order(new_order("r2", "bid-2", "u-2", None, Side::Buy, 98, 1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rate_limiter_disabled_when_zero() {
        // Default config has 0 => disabled
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        for i in 0..10 {
            engine
                .submit_new_order(new_order(
                    &format!("r-{i}"),
                    &format!("bid-{i}"),
                    "u-1",
                    None,
                    Side::Buy,
                    99 - i as i64,
                    1,
                ))
                .await
                .unwrap();
        }
    }

    // ── Auto-recovery tests ─────────────────────────────────────────

    #[tokio::test]
    async fn auto_recovery_from_cancel_only() {
        let mut cfg = config();
        cfg.auto_recover_after_commands = 3;
        let engine = PartitionedMatchingEngine::new(cfg, EventBus::new(), seeded_risk());

        // Place and cancel to trigger CancelOnly state
        engine
            .submit_new_order(new_order(
                "r1",
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
                "r2",
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
                metadata: CommandMetadata::new("r3"),
                user_id: "u-1".to_string(),
            })
            .await
            .unwrap();
        engine
            .mass_cancel_by_user(MassCancelByUserCommand {
                metadata: CommandMetadata::new("r4"),
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

        // Submit 3 successful orders to trigger recovery
        for i in 0..3 {
            engine
                .submit_new_order(new_order(
                    &format!("r-recov-{i}"),
                    &format!("recov-bid-{i}"),
                    "u-1",
                    None,
                    Side::Buy,
                    90 + i as i64,
                    1,
                ))
                .await
                .unwrap();
        }

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.state, MarketState::Normal);
    }

    // ── Trade stats persistence across snapshots ─────────────────────

    #[tokio::test]
    async fn trade_stats_survive_snapshot_recovery() {
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
                "r1",
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
            .submit_new_order(new_order("r2", "bid-1", "taker", None, Side::Buy, 100, 5))
            .await
            .unwrap();

        // Recover from snapshot
        let recovered = PartitionedMatchingEngine::with_snapshot_store(
            config(),
            EventBus::new(),
            risk,
            snapshot_store.clone(),
        )
        .unwrap();

        let snapshot = recovered
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(snapshot.trade_stats.total_trades, 1);
        assert_eq!(snapshot.trade_stats.total_volume, 5);
        assert_eq!(snapshot.trade_stats.high_price, Some(100));
    }

    // ══════════════════════════════════════════════════════════════�?    //  New tests: iceberg orders, min_fill_qty, aggressor_side,
    //  VWAP, imbalance_ratio, price impact estimation
    // ══════════════════════════════════════════════════════════════�?
    #[tokio::test]
    async fn iceberg_order_visible_qty_caps_single_match() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Post iceberg sell: total 10, display 3
        let mut iceberg = new_order("req-1", "ask-ice", "maker-1", None, Side::Sell, 100, 10);
        iceberg.display_qty = Some(3);
        engine.submit_new_order(iceberg).await.unwrap();

        // Buy 2 �?should fill 2 (within visible qty of 3)
        let result = engine
            .submit_new_order(new_order(
                "req-2",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                2,
            ))
            .await
            .unwrap();
        let buy_fill = result.fills.iter().find(|f| f.side == Side::Buy).unwrap();
        assert_eq!(buy_fill.amount, 2);

        // Remaining should be 8
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
        assert_eq!(snapshot.best_ask, Some(100));
    }

    #[tokio::test]
    async fn iceberg_order_replenishes_after_visible_qty_filled() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Post iceberg sell: total 10, display 3
        let mut iceberg = new_order("req-1", "ask-ice", "maker-1", None, Side::Sell, 100, 10);
        iceberg.display_qty = Some(3);
        engine.submit_new_order(iceberg).await.unwrap();

        // Buy exactly 3 �?fills visible portion, triggers replenishment
        let result = engine
            .submit_new_order(new_order(
                "req-2",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                3,
            ))
            .await
            .unwrap();
        let buy_fill = result.fills.iter().find(|f| f.side == Side::Buy).unwrap();
        assert_eq!(buy_fill.amount, 3);

        // Order should still be resting with replenished visible qty
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 1);
        assert_eq!(snapshot.best_ask, Some(100));
    }

    #[tokio::test]
    async fn iceberg_order_fully_consumed() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Post iceberg sell: total 6, display 3
        let mut iceberg = new_order("req-1", "ask-ice", "maker-1", None, Side::Sell, 100, 6);
        iceberg.display_qty = Some(3);
        engine.submit_new_order(iceberg).await.unwrap();

        // Buy 3 �?fills first visible tranche
        engine
            .submit_new_order(new_order(
                "req-2",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                3,
            ))
            .await
            .unwrap();

        // Buy 3 more �?fills the replenished tranche, order fully consumed
        engine
            .submit_new_order(new_order(
                "req-3",
                "bid-2",
                "taker",
                None,
                Side::Buy,
                100,
                3,
            ))
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.open_orders, 0);
    }

    #[tokio::test]
    async fn iceberg_loses_fifo_priority_on_replenishment() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Post iceberg sell: total 6, display 2
        let mut iceberg = new_order("req-1", "ask-ice", "maker-1", None, Side::Sell, 100, 6);
        iceberg.display_qty = Some(2);
        engine.submit_new_order(iceberg).await.unwrap();

        // Post regular sell at same price
        engine
            .submit_new_order(new_order(
                "req-2",
                "ask-reg",
                "maker-2",
                None,
                Side::Sell,
                100,
                5,
            ))
            .await
            .unwrap();

        // Buy 2 �?fills iceberg's visible qty (it was first)
        engine
            .submit_new_order(new_order(
                "req-3",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                2,
            ))
            .await
            .unwrap();

        // Now buy 1 more �?should match against the regular order (iceberg re-queued at back)
        let result = engine
            .submit_new_order(new_order(
                "req-4",
                "bid-2",
                "taker",
                None,
                Side::Buy,
                100,
                1,
            ))
            .await
            .unwrap();
        let sell_fill = result.fills.iter().find(|f| f.side == Side::Sell).unwrap();
        assert_eq!(sell_fill.intent_id, "ask-reg");
    }

    #[tokio::test]
    async fn min_fill_qty_skips_small_fills() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Post sell with min_fill_qty = 5
        let mut sell = new_order("req-1", "ask-mfq", "maker-1", None, Side::Sell, 100, 10);
        sell.min_fill_qty = Some(5);
        engine.submit_new_order(sell).await.unwrap();

        // Buy only 3 �?should NOT fill (below min_fill_qty=5)
        let result = engine
            .submit_new_order(new_order(
                "req-2",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                3,
            ))
            .await
            .unwrap();
        assert!(
            result.fills.is_empty(),
            "expected no fills due to min_fill_qty enforcement"
        );

        // Buy 5 �?should fill
        let result = engine
            .submit_new_order(new_order(
                "req-3",
                "bid-2",
                "taker",
                None,
                Side::Buy,
                100,
                5,
            ))
            .await
            .unwrap();
        assert_eq!(result.fills.len(), 2); // buy fill + sell fill
        let buy_fill = result.fills.iter().find(|f| f.side == Side::Buy).unwrap();
        assert_eq!(buy_fill.amount, 5);
    }

    #[tokio::test]
    async fn validate_rejects_display_qty_exceeding_amount() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        let mut order = new_order("req-1", "ask-bad", "maker-1", None, Side::Sell, 100, 5);
        order.display_qty = Some(10); // display_qty > amount
        let err = engine.submit_new_order(order).await.unwrap_err();
        assert!(matches!(err, SubmissionError::InvalidOrder(_)));
    }

    #[tokio::test]
    async fn validate_rejects_negative_display_qty() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        let mut order = new_order("req-1", "ask-bad", "maker-1", None, Side::Sell, 100, 5);
        order.display_qty = Some(-1);
        let err = engine.submit_new_order(order).await.unwrap_err();
        assert!(matches!(err, SubmissionError::InvalidOrder(_)));
    }

    #[tokio::test]
    async fn validate_rejects_negative_min_fill_qty() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        let mut order = new_order("req-1", "ask-bad", "maker-1", None, Side::Sell, 100, 5);
        order.min_fill_qty = Some(-1);
        let err = engine.submit_new_order(order).await.unwrap_err();
        assert!(matches!(err, SubmissionError::InvalidOrder(_)));
    }

    #[tokio::test]
    async fn validate_rejects_min_fill_qty_exceeding_amount() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        let mut order = new_order("req-1", "ask-bad", "maker-1", None, Side::Sell, 100, 5);
        order.min_fill_qty = Some(10);
        let err = engine.submit_new_order(order).await.unwrap_err();
        assert!(matches!(err, SubmissionError::InvalidOrder(_)));
    }

    #[tokio::test]
    async fn fills_carry_aggressor_side() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Post resting sell
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

        // Aggressive buy
        let result = engine
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
            .unwrap();

        assert_eq!(result.fills.len(), 2);
        for fill in &result.fills {
            assert_eq!(
                fill.aggressor_side,
                Some(Side::Buy),
                "aggressor_side should be Buy for all fills"
            );
        }
    }

    #[tokio::test]
    async fn fills_carry_fill_index() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Post two resting sells at same price
        engine
            .submit_new_order(new_order(
                "req-1",
                "ask-1",
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
                "req-2",
                "ask-2",
                "maker-2",
                None,
                Side::Sell,
                100,
                3,
            ))
            .await
            .unwrap();

        // Buy 6 �?matches both resting orders
        let result = engine
            .submit_new_order(new_order(
                "req-3",
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                6,
            ))
            .await
            .unwrap();

        let buy_fills: Vec<_> = result
            .fills
            .iter()
            .filter(|f| f.side == Side::Buy)
            .collect();
        assert_eq!(buy_fills.len(), 2);
        assert_eq!(buy_fills[0].fill_index, 0);
        assert_eq!(buy_fills[1].fill_index, 1);
    }

    #[tokio::test]
    async fn vwap_computed_from_trade_stats() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Trade 1: 5 @ 100
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
                "bid-1",
                "taker",
                None,
                Side::Buy,
                100,
                5,
            ))
            .await
            .unwrap();

        // Trade 2: 5 @ 200
        engine
            .submit_new_order(new_order(
                "req-3",
                "ask-2",
                "maker-2",
                None,
                Side::Sell,
                200,
                5,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order(
                "req-4",
                "bid-2",
                "taker",
                None,
                Side::Buy,
                200,
                5,
            ))
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        // VWAP = (5*100 + 5*200) / 10 = 150
        assert_eq!(snapshot.vwap, Some(150));
        assert_eq!(snapshot.trade_stats.vwap(), Some(150));
    }

    #[tokio::test]
    async fn imbalance_ratio_computed_correctly() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Post bid depth 10 @ 99
        engine
            .submit_new_order(new_order(
                "req-1",
                "bid-1",
                "maker-1",
                None,
                Side::Buy,
                99,
                10,
            ))
            .await
            .unwrap();

        // Post ask depth 5 @ 101
        engine
            .submit_new_order(new_order(
                "req-2",
                "ask-1",
                "maker-2",
                None,
                Side::Sell,
                101,
                5,
            ))
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        // imbalance = (bid - ask) / (bid + ask) = (10 - 5) / (10 + 5) = 0.333...
        let imb = snapshot
            .imbalance_ratio
            .expect("imbalance_ratio should be set");
        assert!((imb - 1.0 / 3.0).abs() < 0.01, "expected ~0.333, got {imb}");
    }

    #[tokio::test]
    async fn imbalance_ratio_none_when_book_empty() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Submit and cancel an order so the market exists
        engine
            .submit_new_order(new_order(
                "req-1",
                "bid-tmp",
                "maker-1",
                None,
                Side::Buy,
                99,
                1,
            ))
            .await
            .unwrap();
        engine
            .cancel_order(types::CancelOrderCommand {
                metadata: CommandMetadata::new("req-2"),
                user_id: "maker-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "bid-tmp".to_string(),
                client_order_id: None,
            })
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.imbalance_ratio, None);
    }

    #[tokio::test]
    async fn estimate_price_impact_buy_side() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Place asks: 5 @ 100, 5 @ 110
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
                110,
                5,
            ))
            .await
            .unwrap();

        let impact = engine
            .estimate_price_impact("btc-usdt", 0, Side::Buy, 8)
            .await
            .unwrap()
            .expect("market exists");

        assert_eq!(impact.side, Side::Buy);
        assert_eq!(impact.requested_amount, 8);
        assert_eq!(impact.fillable_amount, 8);
        assert_eq!(impact.levels_consumed, 2);
        // notional = 5*100 + 3*110 = 830
        assert_eq!(impact.total_notional, 830);
        // avg_fill_price = 830 / 8 = 103 (integer division)
        assert_eq!(impact.avg_fill_price, Some(103));
    }

    #[tokio::test]
    async fn estimate_price_impact_partial_fill() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Only 5 available
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

        let impact = engine
            .estimate_price_impact("btc-usdt", 0, Side::Buy, 10)
            .await
            .unwrap()
            .expect("market exists");

        assert_eq!(impact.requested_amount, 10);
        assert_eq!(impact.fillable_amount, 5);
        assert_eq!(impact.levels_consumed, 1);
    }

    #[tokio::test]
    async fn estimate_price_impact_empty_book() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Submit and cancel an order so the market exists
        engine
            .submit_new_order(new_order(
                "req-1",
                "bid-tmp",
                "maker-1",
                None,
                Side::Buy,
                99,
                1,
            ))
            .await
            .unwrap();
        engine
            .cancel_order(types::CancelOrderCommand {
                metadata: CommandMetadata::new("req-2"),
                user_id: "maker-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "bid-tmp".to_string(),
                client_order_id: None,
            })
            .await
            .unwrap();

        let impact = engine
            .estimate_price_impact("btc-usdt", 0, Side::Buy, 5)
            .await
            .unwrap()
            .expect("market exists");

        assert_eq!(impact.fillable_amount, 0);
        assert_eq!(impact.avg_fill_price, None);
        assert_eq!(impact.levels_consumed, 0);
    }

    #[tokio::test]
    async fn vwap_none_when_no_trades() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Submit and cancel an order so the market exists but no trades occur
        engine
            .submit_new_order(new_order(
                "req-1",
                "bid-tmp",
                "maker-1",
                None,
                Side::Buy,
                99,
                1,
            ))
            .await
            .unwrap();
        engine
            .cancel_order(types::CancelOrderCommand {
                metadata: CommandMetadata::new("req-2"),
                user_id: "maker-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "bid-tmp".to_string(),
                client_order_id: None,
            })
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.vwap, None);
        assert_eq!(snapshot.trade_stats.vwap(), None);
    }

    // ── Conditional (stop / take-profit) order tests ────────────────────

    fn stop_order(
        request_id: &str,
        client_order_id: &str,
        user_id: &str,
        side: Side,
        trigger_price: i64,
        amount: i64,
    ) -> NewOrderCommand {
        let mut cmd = new_order(request_id, client_order_id, user_id, None, side, 0, amount);
        cmd.order_type = OrderType::StopMarket;
        cmd.trigger_price = Some(trigger_price);
        cmd.trigger_type = Some(types::TriggerType::LastPrice);
        cmd.price = None; // market order �?no limit price
        cmd
    }

    fn stop_limit_order(
        request_id: &str,
        client_order_id: &str,
        user_id: &str,
        side: Side,
        trigger_price: i64,
        limit_price: i64,
        amount: i64,
    ) -> NewOrderCommand {
        let mut cmd = new_order(
            request_id,
            client_order_id,
            user_id,
            None,
            side,
            limit_price,
            amount,
        );
        cmd.order_type = OrderType::StopLimit;
        cmd.trigger_price = Some(trigger_price);
        cmd.trigger_type = Some(types::TriggerType::LastPrice);
        cmd
    }

    fn take_profit_order(
        request_id: &str,
        client_order_id: &str,
        user_id: &str,
        side: Side,
        trigger_price: i64,
        amount: i64,
    ) -> NewOrderCommand {
        let mut cmd = new_order(request_id, client_order_id, user_id, None, side, 0, amount);
        cmd.order_type = OrderType::TakeProfitMarket;
        cmd.trigger_price = Some(trigger_price);
        cmd.trigger_type = Some(types::TriggerType::LastPrice);
        cmd.price = None;
        cmd
    }

    #[tokio::test]
    async fn stop_order_parked_in_trigger_book() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        let result = engine
            .submit_new_order(stop_order("r1", "stop-1", "maker-1", Side::Buy, 110, 5))
            .await
            .unwrap();
        assert_eq!(result.state, OrderState::Active);
        assert!(result.fills.is_empty());

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.pending_triggers, 1);
        // Regular order book should be empty
        assert_eq!(snapshot.open_orders, 0);
    }

    #[tokio::test]
    async fn stop_buy_triggered_when_price_crosses_up() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Place a stop-buy: trigger when last_trade_price >= 100
        engine
            .submit_new_order(stop_order("r1", "stop-buy", "u-1", Side::Buy, 100, 3))
            .await
            .unwrap();
        assert_eq!(
            engine
                .snapshot_market("btc-usdt", 0)
                .await
                .unwrap()
                .unwrap()
                .pending_triggers,
            1
        );

        // Place a resting sell at 100 (provides liquidity for the triggered market order)
        engine
            .submit_new_order(new_order(
                "r2",
                "ask-liq",
                "maker-2",
                None,
                Side::Sell,
                100,
                10,
            ))
            .await
            .unwrap();

        // Place a resting buy at 100 �?this will match the sell and set last_trade_price = 100
        // which triggers the stop-buy.
        let result = engine
            .submit_new_order(new_order(
                "r3",
                "bid-trig",
                "maker-1",
                None,
                Side::Buy,
                100,
                3,
            ))
            .await
            .unwrap();
        assert!(!result.fills.is_empty());

        // After the trade at 100, the stop-buy should have triggered and consumed
        // some of the remaining sell liquidity at 100.
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.pending_triggers, 0);
    }

    #[tokio::test]
    async fn stop_sell_triggered_when_price_crosses_down() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Place a stop-sell: trigger when last_trade_price <= 90
        engine
            .submit_new_order(stop_order("r1", "stop-sell", "u-1", Side::Sell, 90, 3))
            .await
            .unwrap();

        // Place a resting buy at 90 (liquidity after trigger for the market sell)
        engine
            .submit_new_order(new_order(
                "r2",
                "bid-liq",
                "maker-2",
                None,
                Side::Buy,
                90,
                10,
            ))
            .await
            .unwrap();

        // Trade at 90 �?triggers the stop-sell
        let result = engine
            .submit_new_order(new_order(
                "r3",
                "ask-trig",
                "maker-1",
                None,
                Side::Sell,
                90,
                3,
            ))
            .await
            .unwrap();
        assert!(!result.fills.is_empty());

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.pending_triggers, 0);
    }

    #[tokio::test]
    async fn take_profit_sell_triggered_when_price_rises() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // take-profit sell triggers when last_trade_price >= trigger
        engine
            .submit_new_order(take_profit_order(
                "r1",
                "tp-sell",
                "u-1",
                Side::Sell,
                110,
                2,
            ))
            .await
            .unwrap();

        // Resting buy at 110 (liquidity for the triggered TP market sell)
        engine
            .submit_new_order(new_order(
                "r2",
                "bid-liq",
                "maker-2",
                None,
                Side::Buy,
                110,
                10,
            ))
            .await
            .unwrap();

        // Trade at 110 �?triggers the take-profit sell
        let result = engine
            .submit_new_order(new_order(
                "r3",
                "ask-trig",
                "maker-1",
                None,
                Side::Sell,
                110,
                2,
            ))
            .await
            .unwrap();
        assert!(!result.fills.is_empty());

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.pending_triggers, 0);
    }

    #[tokio::test]
    async fn stop_not_triggered_when_condition_not_met() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // stop-buy triggers at 110
        engine
            .submit_new_order(stop_order("r1", "stop-buy", "u-1", Side::Buy, 110, 3))
            .await
            .unwrap();

        // Trade at 100 �?below trigger, should NOT activate
        engine
            .submit_new_order(new_order(
                "r2",
                "ask-1",
                "maker-2",
                None,
                Side::Sell,
                100,
                3,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r3", "bid-1", "maker-1", None, Side::Buy, 100, 3))
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.pending_triggers, 1);
    }

    #[tokio::test]
    async fn cancel_trigger_order() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(stop_order("r1", "stop-1", "maker-1", Side::Buy, 110, 5))
            .await
            .unwrap();
        assert_eq!(
            engine
                .snapshot_market("btc-usdt", 0)
                .await
                .unwrap()
                .unwrap()
                .pending_triggers,
            1
        );

        let cancel_result = engine
            .cancel_order(types::CancelOrderCommand {
                metadata: CommandMetadata::new("r2"),
                user_id: "maker-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "stop-1".to_string(),
                client_order_id: None,
            })
            .await
            .unwrap();
        assert_eq!(cancel_result.cancelled_order_ids, vec!["stop-1"]);

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.pending_triggers, 0);
    }

    #[tokio::test]
    async fn cancel_trigger_order_wrong_user_rejected() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(stop_order("r1", "stop-1", "maker-1", Side::Buy, 110, 5))
            .await
            .unwrap();

        // Different user tries to cancel
        let cancel_result = engine
            .cancel_order(types::CancelOrderCommand {
                metadata: CommandMetadata::new("r2"),
                user_id: "maker-2".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: "stop-1".to_string(),
                client_order_id: None,
            })
            .await;
        // Should fail �?wrong user
        assert!(cancel_result.is_err());

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.pending_triggers, 1);
    }

    #[tokio::test]
    async fn conditional_order_without_trigger_price_rejected() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        let mut cmd = new_order("r1", "bad-stop", "maker-1", None, Side::Buy, 100, 5);
        cmd.order_type = OrderType::StopMarket;
        // trigger_price remains None
        let result = engine.submit_new_order(cmd).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn conditional_order_negative_trigger_price_rejected() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        let mut cmd = new_order("r1", "bad-stop", "maker-1", None, Side::Buy, 100, 5);
        cmd.order_type = OrderType::StopMarket;
        cmd.trigger_price = Some(-50);
        cmd.trigger_type = Some(types::TriggerType::LastPrice);
        let result = engine.submit_new_order(cmd).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stop_limit_without_price_rejected() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        let mut cmd = new_order("r1", "bad-sl", "maker-1", None, Side::Buy, 100, 5);
        cmd.order_type = OrderType::StopLimit;
        cmd.trigger_price = Some(110);
        cmd.trigger_type = Some(types::TriggerType::LastPrice);
        cmd.price = None; // missing limit price
        let result = engine.submit_new_order(cmd).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stop_limit_parked_and_activated_as_limit() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Place a stop-limit buy: trigger at 100, limit price 101
        engine
            .submit_new_order(stop_limit_order(
                "r1",
                "sl-buy",
                "u-1",
                Side::Buy,
                100,
                101,
                3,
            ))
            .await
            .unwrap();
        assert_eq!(
            engine
                .snapshot_market("btc-usdt", 0)
                .await
                .unwrap()
                .unwrap()
                .pending_triggers,
            1
        );

        // Provide sell liquidity at 101 for the activated limit order
        engine
            .submit_new_order(new_order(
                "r2",
                "ask-liq",
                "maker-2",
                None,
                Side::Sell,
                101,
                10,
            ))
            .await
            .unwrap();

        // Trade at 100 to trigger the stop
        engine
            .submit_new_order(new_order(
                "r3",
                "ask-trig",
                "maker-1",
                None,
                Side::Sell,
                100,
                2,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r4", "bid-trig", "u-2", None, Side::Buy, 100, 2))
            .await
            .unwrap();

        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.pending_triggers, 0);
    }

    #[tokio::test]
    async fn duplicate_trigger_order_id_rejected() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(stop_order("r1", "stop-dup", "maker-1", Side::Buy, 110, 5))
            .await
            .unwrap();
        let result = engine
            .submit_new_order(stop_order("r2", "stop-dup", "maker-1", Side::Buy, 120, 5))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn trigger_orders_survive_snapshot_roundtrip() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        engine
            .submit_new_order(stop_order("r1", "stop-snap", "maker-1", Side::Buy, 110, 5))
            .await
            .unwrap();

        // Export the runtime snapshot
        let records = engine.export_snapshots().await.unwrap();
        let market_snap = records
            .iter()
            .flat_map(|r| &r.snapshot.markets)
            .find(|s| s.market_id == "btc-usdt" && s.outcome == 0)
            .unwrap();
        assert_eq!(market_snap.trigger_orders.len(), 1);
        assert_eq!(market_snap.trigger_orders[0].client_order_id, "stop-snap");
        assert_eq!(market_snap.trigger_orders[0].trigger_price, 110);
    }

    // ── Fee collection to SYS:FEE_COLLECTOR:USDC ────────────────────────

    #[tokio::test]
    async fn fee_collected_to_sys_fee_collector() {
        let risk = seeded_risk();
        let ledger = risk.ledger();
        let engine = engine_with_instrument(risk, strict_instrument("btc-usdt"));

        // strict_instrument has taker_fee_bps=10, maker_fee_bps=5
        engine
            .submit_new_order(new_order(
                "r1",
                "bid-1",
                "maker-1",
                None,
                Side::Buy,
                100,
                10,
            ))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r2", "ask-1", "taker", None, Side::Sell, 100, 10))
            .await
            .unwrap();

        // Fees should go to SYS:FEE_COLLECTOR:USDC, not "sys:fee_pool"
        let fee_balance = ledger.fee_collector_balance();
        assert!(fee_balance > 0, "fee collector should have received fees");
        assert_eq!(ledger.cash_available_balance("sys:fee_pool"), 0);
    }

    // ── is_trigger_met unit tests ───────────────────────────────────────

    #[test]
    fn trigger_met_stop_buy_at_or_above() {
        let mut cmd = new_order("r", "o", "u", None, Side::Buy, 100, 1);
        cmd.order_type = OrderType::StopMarket;
        assert!(is_trigger_met(&cmd, 100, 100)); // exactly at trigger
        assert!(is_trigger_met(&cmd, 100, 110)); // above trigger
        assert!(!is_trigger_met(&cmd, 100, 99)); // below trigger
    }

    #[test]
    fn trigger_met_stop_sell_at_or_below() {
        let mut cmd = new_order("r", "o", "u", None, Side::Sell, 100, 1);
        cmd.order_type = OrderType::StopMarket;
        assert!(is_trigger_met(&cmd, 100, 100)); // exactly at trigger
        assert!(is_trigger_met(&cmd, 100, 90)); // below trigger
        assert!(!is_trigger_met(&cmd, 100, 101)); // above trigger
    }

    #[test]
    fn trigger_met_take_profit_buy_at_or_below() {
        let mut cmd = new_order("r", "o", "u", None, Side::Buy, 100, 1);
        cmd.order_type = OrderType::TakeProfitMarket;
        assert!(is_trigger_met(&cmd, 100, 100)); // exactly at trigger
        assert!(is_trigger_met(&cmd, 100, 90)); // below trigger
        assert!(!is_trigger_met(&cmd, 100, 101)); // above trigger
    }

    #[test]
    fn trigger_met_take_profit_sell_at_or_above() {
        let mut cmd = new_order("r", "o", "u", None, Side::Sell, 100, 1);
        cmd.order_type = OrderType::TakeProfitMarket;
        assert!(is_trigger_met(&cmd, 100, 100)); // exactly at trigger
        assert!(is_trigger_met(&cmd, 100, 110)); // above trigger
        assert!(!is_trigger_met(&cmd, 100, 99)); // below trigger
    }

    #[test]
    fn trigger_met_non_conditional_always_false() {
        let cmd = new_order("r", "o", "u", None, Side::Buy, 100, 1);
        assert!(!is_trigger_met(&cmd, 100, 100));
        assert!(!is_trigger_met(&cmd, 100, 200));
    }

    #[test]
    fn ioc_order_with_expires_at_is_rejected() {
        let mut cmd = new_order("r", "ioc-exp", "u", None, Side::Buy, 100, 1);
        cmd.time_in_force = TimeInForce::Ioc;
        cmd.expires_at = Some(Utc::now() + chrono::Duration::hours(1));
        let result = validate_new_order(&cmd);
        assert!(result.is_err());
    }

    #[test]
    fn fok_order_with_expires_at_is_rejected() {
        let mut cmd = new_order("r", "fok-exp", "u", None, Side::Buy, 100, 1);
        cmd.time_in_force = TimeInForce::Fok;
        cmd.expires_at = Some(Utc::now() + chrono::Duration::hours(1));
        let result = validate_new_order(&cmd);
        assert!(result.is_err());
    }

    #[test]
    fn post_only_ioc_is_rejected() {
        let mut cmd = new_order("r", "po-ioc", "u", None, Side::Buy, 100, 1);
        cmd.time_in_force = TimeInForce::Ioc;
        cmd.post_only = true;
        let result = validate_new_order(&cmd);
        assert!(result.is_err());
    }

    #[test]
    fn market_state_rank_handles_all_variants() {
        assert_eq!(market_state_rank(MarketState::Normal), 0);
        assert_eq!(market_state_rank(MarketState::PreOpen), 1);
        assert_eq!(market_state_rank(MarketState::Stress), 1);
        assert_eq!(market_state_rank(MarketState::AuctionCall), 2);
        assert_eq!(market_state_rank(MarketState::CancelOnly), 3);
        assert_eq!(market_state_rank(MarketState::Maintenance), 3);
        assert_eq!(market_state_rank(MarketState::Halted), 4);
        assert_eq!(market_state_rank(MarketState::Closed), 5);
    }

    #[test]
    fn combine_market_state_picks_higher_rank() {
        assert_eq!(
            combine_market_state(MarketState::Normal, MarketState::Halted),
            MarketState::Halted
        );
        assert_eq!(
            combine_market_state(MarketState::Closed, MarketState::Normal),
            MarketState::Closed
        );
    }

    #[test]
    fn frozen_account_rejects_new_orders() {
        let mut state = PartitionState {
            config: PartitionedEngineConfig::default(),
            event_bus: EventBus::new(),
            risk: Arc::new(RiskEngine::new(Arc::new(LedgerService::new(
                EventBus::new(),
            )))),
            instruments: instruments::shared_default_registry(),
            kill_switch: Arc::new(AtomicBool::new(false)),
            trade_store: None,
            cost_store: None,
            settlement_store: None,
            partition_id: 0,
            markets: HashMap::new(),
            replay_cursor: ReplayCursor::default(),
            seen_trade_ids: HashSet::new(),
            settlement_statuses: HashMap::new(),
            frozen_accounts: HashSet::new(),
        };
        // Deposit cash for the user
        state
            .risk
            .ledger()
            .process_deposit("user1", 1_000_000, "dep".to_string())
            .unwrap();

        // Before freeze: order should succeed
        let cmd = new_order("r1", "o1", "user1", None, Side::Buy, 100, 1);
        assert!(state.process_new_order(cmd).is_ok());

        // Freeze the account
        state.frozen_accounts.insert("user1".to_string());

        // After freeze: new order should be rejected
        let cmd2 = new_order("r2", "o2", "user1", None, Side::Buy, 100, 1);
        let result = state.process_new_order(cmd2);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("frozen"));
    }

    #[test]
    fn freeze_account_admin_action_cancels_orders() {
        let mut state = PartitionState {
            config: PartitionedEngineConfig::default(),
            event_bus: EventBus::new(),
            risk: Arc::new(RiskEngine::new(Arc::new(LedgerService::new(
                EventBus::new(),
            )))),
            instruments: instruments::shared_default_registry(),
            kill_switch: Arc::new(AtomicBool::new(false)),
            trade_store: None,
            cost_store: None,
            settlement_store: None,
            partition_id: 0,
            markets: HashMap::new(),
            replay_cursor: ReplayCursor::default(),
            seen_trade_ids: HashSet::new(),
            settlement_statuses: HashMap::new(),
            frozen_accounts: HashSet::new(),
        };
        state
            .risk
            .ledger()
            .process_deposit("user1", 1_000_000, "dep".to_string())
            .unwrap();

        // Place an order
        let cmd = new_order("r1", "o1", "user1", None, Side::Buy, 100, 5);
        state.process_new_order(cmd).unwrap();

        // Verify order exists
        let key = MarketKey::new("btc-usdt".to_string(), 0);
        assert!(!state.markets[&key].orders.is_empty());

        // Freeze via admin action
        let admin_cmd = AdminCommand {
            metadata: CommandMetadata::new("admin-1"),
            actor_id: "admin".to_string(),
            action: AdminAction::FreezeAccount {
                user_id: "user1".to_string(),
                reason: "suspicious activity".to_string(),
            },
        };
        state.process_admin(admin_cmd).unwrap();

        // Orders should be cancelled
        assert!(state.markets[&key].orders.is_empty());
        // Account should be frozen
        assert!(state.frozen_accounts.contains("user1"));
    }

    #[test]
    fn set_market_state_validates_transitions() {
        let mut state = PartitionState {
            config: PartitionedEngineConfig::default(),
            event_bus: EventBus::new(),
            risk: Arc::new(RiskEngine::new(Arc::new(LedgerService::new(
                EventBus::new(),
            )))),
            instruments: instruments::shared_default_registry(),
            kill_switch: Arc::new(AtomicBool::new(false)),
            trade_store: None,
            cost_store: None,
            settlement_store: None,
            partition_id: 0,
            markets: HashMap::new(),
            replay_cursor: ReplayCursor::default(),
            seen_trade_ids: HashSet::new(),
            settlement_statuses: HashMap::new(),
            frozen_accounts: HashSet::new(),
        };

        // Create a market with Normal state
        let key = MarketKey::new("btc-usdt".to_string(), 0);
        state
            .markets
            .insert(key.clone(), MarketRuntime::new("btc-usdt", 0));
        assert_eq!(state.markets[&key].state, MarketState::Normal);

        // Valid transition: Normal → Halted
        let cmd = AdminCommand {
            metadata: CommandMetadata::new("a1"),
            actor_id: "admin".to_string(),
            action: AdminAction::SetMarketState {
                market_id: "btc-usdt".to_string(),
                outcome: None,
                state: MarketState::Halted,
            },
        };
        state.process_admin(cmd).unwrap();
        assert_eq!(state.markets[&key].state, MarketState::Halted);

        // Invalid transition: Halted → Stress (not allowed)
        let cmd2 = AdminCommand {
            metadata: CommandMetadata::new("a2"),
            actor_id: "admin".to_string(),
            action: AdminAction::SetMarketState {
                market_id: "btc-usdt".to_string(),
                outcome: None,
                state: MarketState::Stress,
            },
        };
        state.process_admin(cmd2).unwrap();
        // State should NOT have changed
        assert_eq!(state.markets[&key].state, MarketState::Halted);
    }

    // ── Fat-finger guard tests ─────────────────────────────────────────

    #[tokio::test]
    async fn fat_finger_rejects_oversized_order() {
        let mut spec = strict_instrument("btc-usdt");
        spec.max_order_amount = 100;
        let engine = engine_with_instrument(seeded_risk(), spec);
        let err = engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 200))
            .await
            .unwrap_err();
        assert!(
            matches!(err, SubmissionError::InvalidOrder(msg) if msg.contains("fat-finger")),
            "expected fat-finger rejection, got: {err}"
        );
    }

    #[tokio::test]
    async fn fat_finger_allows_within_limit() {
        let mut spec = strict_instrument("btc-usdt");
        spec.max_order_amount = 100;
        spec.lot_size = 1;
        spec.tick_size = 1;
        spec.min_order_amount = 0;
        let engine = engine_with_instrument(seeded_risk(), spec);
        engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 50))
            .await
            .unwrap();
    }

    // ── Group STP tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn stp_group_prevents_cross_user_self_trade() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        // User maker-1 places a resting bid with stp_group_id = "firm1"
        let mut bid = new_order("r1", "bid-1", "maker-1", None, Side::Buy, 100, 5);
        bid.stp_group_id = Some("firm1".to_string());
        bid.stp_mode = StpMode::CancelTaker;
        engine.submit_new_order(bid).await.unwrap();

        // User maker-2 (different user, SAME group) places a crossing ask
        let mut ask = new_order("r2", "ask-1", "maker-2", None, Side::Sell, 100, 5);
        ask.stp_group_id = Some("firm1".to_string());
        ask.stp_mode = StpMode::CancelTaker;
        let err = engine.submit_new_order(ask).await.unwrap_err();
        assert!(
            matches!(err, SubmissionError::SelfTradePrevented(_)),
            "expected STP prevention across group, got: {err}"
        );
    }

    #[tokio::test]
    async fn stp_no_group_does_not_prevent_cross_user_trade() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        // maker-1 places bid without group
        let bid = new_order("r1", "bid-1", "maker-1", None, Side::Buy, 100, 5);
        engine.submit_new_order(bid).await.unwrap();

        // maker-2 places crossing ask without group — should trade
        let mut ask = new_order("r2", "ask-1", "maker-2", None, Side::Sell, 100, 5);
        ask.stp_mode = StpMode::CancelTaker;
        let result = engine.submit_new_order(ask).await.unwrap();
        assert!(!result.fills.is_empty(), "should have filled");
    }

    // ── Per-market kill switch tests ───────────────────────────────────

    #[tokio::test]
    async fn per_market_kill_switch_halts_single_market() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());
        // Place a resting order
        engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 5))
            .await
            .unwrap();

        // Activate per-market kill switch
        engine
            .submit_admin(AdminCommand {
                metadata: CommandMetadata::new("admin-1"),
                actor_id: "admin".to_string(),
                action: AdminAction::MarketKillSwitch {
                    market_id: "btc-usdt".to_string(),
                    enabled: true,
                },
            })
            .await
            .unwrap();

        // Market should be halted
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.state, MarketState::Halted);
    }

    // ── Unfreeze account test ──────────────────────────────────────────

    #[tokio::test]
    async fn unfreeze_account_allows_new_orders() {
        let engine = PartitionedMatchingEngine::new(config(), EventBus::new(), seeded_risk());

        // Freeze user
        engine
            .submit_admin(AdminCommand {
                metadata: CommandMetadata::new("a1"),
                actor_id: "admin".to_string(),
                action: AdminAction::FreezeAccount {
                    user_id: "maker-1".to_string(),
                    reason: "test".to_string(),
                },
            })
            .await
            .unwrap();

        // Order should be rejected
        let err = engine
            .submit_new_order(new_order("r1", "o1", "maker-1", None, Side::Buy, 100, 5))
            .await
            .unwrap_err();
        assert!(matches!(err, SubmissionError::AccountFrozen));

        // Unfreeze
        engine
            .submit_admin(AdminCommand {
                metadata: CommandMetadata::new("a2"),
                actor_id: "admin".to_string(),
                action: AdminAction::UnfreezeAccount {
                    user_id: "maker-1".to_string(),
                },
            })
            .await
            .unwrap();

        // Order should now succeed
        engine
            .submit_new_order(new_order("r2", "o2", "maker-1", None, Side::Buy, 100, 5))
            .await
            .unwrap();
    }

    // ── Circuit breaker auto-transition tests ──────────────────────────

    #[tokio::test]
    async fn circuit_breaker_transitions_market_on_volatility() {
        let mut spec = strict_instrument("btc-usdt");
        spec.tick_size = 1;
        spec.lot_size = 1;
        spec.min_order_amount = 1;
        spec.max_notional = 0;
        spec.circuit_breaker = Some(types::CircuitBreakerConfig {
            stress_threshold_bps: 200,      // 2% triggers stress
            cancel_only_threshold_bps: 500, // 5% triggers cancel-only
            halt_threshold_bps: 1000,       // 10% triggers halt
            cooldown_secs: 0,               // no cooldown for test
            vol_lookback_trades: 3,         // small window for test
        });
        let engine = engine_with_instrument(seeded_risk(), spec);

        // Trade at price 100
        engine
            .submit_new_order(new_order("r1", "b1", "maker-1", None, Side::Buy, 100, 1))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r2", "s1", "taker", None, Side::Sell, 100, 1))
            .await
            .unwrap();

        // Trade at price 103 (3% move from 100 → triggers stress at 200bps)
        engine
            .submit_new_order(new_order("r3", "b2", "maker-1", None, Side::Buy, 103, 1))
            .await
            .unwrap();
        engine
            .submit_new_order(new_order("r4", "s2", "taker", None, Side::Sell, 103, 1))
            .await
            .unwrap();

        // Trade at price 106 (range = 100-106 = 6%, cancel-only at 500bps)
        engine
            .submit_new_order(new_order("r5", "b3", "maker-1", None, Side::Buy, 106, 1))
            .await
            .unwrap();
        let _result = engine
            .submit_new_order(new_order("r6", "s3", "taker", None, Side::Sell, 106, 1))
            .await;

        // The market should have moved out of Normal
        let snapshot = engine
            .snapshot_market("btc-usdt", 0)
            .await
            .unwrap()
            .unwrap();
        assert!(
            snapshot.state != MarketState::Normal,
            "expected market state transition from Normal, got: {:?}",
            snapshot.state,
        );
    }
}
