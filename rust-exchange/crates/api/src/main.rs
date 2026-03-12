use chrono::{DateTime, Utc};
use dashmap::DashMap;
use eventbus::EventBus;
use hmac::{Hmac, Mac};
use instruments::{InstrumentRegistry, PersistentInstrumentRegistry};
use ledger::LedgerService;
use matching::partitioned::TradeJournalRecord;
use matching::{
    MarketRuntimeSnapshot, PartitionSnapshotRecord, PartitionedEngineConfig,
    PartitionedMatchingEngine, RestingOrderSnapshot,
};
use parking_lot::Mutex;
use persistence::JsonlFileWal;
use projections::{project_margin, project_pnl, project_positions};
use risk::{AdlCandidate, AdlGovernance, RiskEngine};
use sequencer::{SequencedCommandRecord, Sequencer};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::Infallible;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use std::time::Instant;
use types::{
    AdminAction, AdminCommand, AuthenticatedPrincipal, CancelOrderCommand, Command,
    CommandMetadata, InstrumentKind, InstrumentSpec, LedgerDelta, MarginMode, MarketState,
    MassCancelByMarketCommand, MassCancelBySessionCommand, MassCancelByUserCommand,
    NewOrderCommand, OrderType, PrincipalRole, ReplaceOrderCommand, Side, TimeInForce,
};
use warp::{
    http::{Method, StatusCode},
    reject::Reject,
    Filter, Rejection, Reply,
};

mod accounts;
mod admin;
mod bootstrap;
mod control;
mod dto;
mod governance;
mod helpers;
mod liquidation;
mod markets;
mod pricing;
mod security;
mod stores;
mod trading;

use accounts::*;
use admin::*;
use bootstrap::*;
use control::*;
use dto::*;
use governance::*;
use helpers::*;
use liquidation::*;
use markets::*;
use pricing::*;
use security::*;
use stores::*;
use trading::*;

type JsonRoute = warp::filters::BoxedFilter<(warp::reply::Json,)>;

type HmacSha256 = Hmac<sha2::Sha256>;

const INTERNAL_AUTH_MAX_SKEW_SECONDS: i64 = 30;

static INTERNAL_AUTH_SHARED_SECRET: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FundingRateRecord {
    market_id: String,
    outcome: i32,
    funding_rate_ppm: i64,
    updated_by: String,
    recorded_at: DateTime<Utc>,
}

struct PersistentFundingRateStore {
    rates: DashMap<String, FundingRateRecord>,
    store: Arc<dyn persistence::WalStore<FundingRateRecord>>,
}

impl PersistentFundingRateStore {
    fn new(store: Arc<dyn persistence::WalStore<FundingRateRecord>>) -> anyhow::Result<Self> {
        let result = Self {
            rates: DashMap::new(),
            store,
        };
        for record in result.store.entries()? {
            result
                .rates
                .insert(rate_key(&record.market_id, record.outcome), record);
        }
        Ok(result)
    }

    fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<FundingRateRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    fn upsert(&self, record: FundingRateRecord) -> anyhow::Result<()> {
        self.store.append(&record)?;
        self.rates
            .insert(rate_key(&record.market_id, record.outcome), record);
        Ok(())
    }

    fn list(&self) -> Vec<FundingRateRecord> {
        let mut items: Vec<_> = self
            .rates
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|lhs, rhs| {
            lhs.market_id
                .cmp(&rhs.market_id)
                .then_with(|| lhs.outcome.cmp(&rhs.outcome))
        });
        items
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RiskAutomationAuditRecord {
    event_id: String,
    event_type: String,
    status: String,
    market_id: String,
    outcome: i32,
    user_id: Option<String>,
    counterparty_user_id: Option<String>,
    request_id: String,
    detail: serde_json::Value,
    recorded_at: DateTime<Utc>,
}

struct RiskAutomationAuditStore {
    store: Arc<dyn persistence::WalStore<RiskAutomationAuditRecord>>,
}

impl RiskAutomationAuditStore {
    fn new(store: Arc<dyn persistence::WalStore<RiskAutomationAuditRecord>>) -> Self {
        Self { store }
    }

    fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<RiskAutomationAuditRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Ok(Self::new(store))
    }

    fn append(&self, record: RiskAutomationAuditRecord) -> anyhow::Result<()> {
        self.store.append(&record)
    }

    fn list_recent(&self, limit: usize) -> anyhow::Result<Vec<RiskAutomationAuditRecord>> {
        let mut items = self.store.entries()?;
        items.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));
        items.truncate(limit);
        Ok(items)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LiquidationQueueRecord {
    queue_id: String,
    source: String,
    status: String,
    market_id: String,
    outcome: i32,
    user_id: String,
    liquidator_user_id: String,
    mark_price: i64,
    #[serde(default)]
    position_qty: i64,
    #[serde(default)]
    remaining_position_qty: i64,
    #[serde(default)]
    filled_position_qty: i64,
    #[serde(default)]
    auction_round: u32,
    margin_ratio_bps: Option<i64>,
    adl_candidates: Vec<AdlCandidate>,
    #[serde(default)]
    retry_tier: u32,
    #[serde(default)]
    retry_count: u32,
    #[serde(default)]
    strategy: String,
    #[serde(default)]
    next_attempt_at: Option<DateTime<Utc>>,
    #[serde(default)]
    last_attempt_at: Option<DateTime<Utc>>,
    error: Option<String>,
    recorded_at: DateTime<Utc>,
}

fn liquidation_queue_status_is_active(status: &str) -> bool {
    matches!(status, "queued" | "auction_open" | "running")
}

fn liquidation_strategy_for_tier(retry_tier: u32) -> &'static str {
    match retry_tier {
        0 => "auction",
        1 => "system_backstop",
        _ => "adl_backstop",
    }
}

struct LiquidationQueueStore {
    entries: DashMap<String, LiquidationQueueRecord>,
    store: Arc<dyn persistence::WalStore<LiquidationQueueRecord>>,
    write_lock: Mutex<()>,
}

impl LiquidationQueueStore {
    fn new(store: Arc<dyn persistence::WalStore<LiquidationQueueRecord>>) -> anyhow::Result<Self> {
        let result = Self {
            entries: DashMap::new(),
            store,
            write_lock: Mutex::new(()),
        };
        for mut record in result.store.entries()? {
            if record.strategy.is_empty() {
                record.strategy = liquidation_strategy_for_tier(record.retry_tier).to_string();
            }
            if record.remaining_position_qty == 0 && record.position_qty != 0 {
                record.remaining_position_qty = record.position_qty.abs();
            }
            result.entries.insert(record.queue_id.clone(), record);
        }
        Ok(result)
    }

    fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<LiquidationQueueRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    fn append(&self, record: LiquidationQueueRecord) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store.append(&record)?;
        self.entries.insert(record.queue_id.clone(), record);
        Ok(())
    }

    fn append_if_no_active_position(&self, record: LiquidationQueueRecord) -> anyhow::Result<bool> {
        let _guard = self.write_lock.lock();
        let has_active = self.entries.iter().any(|entry| {
            let item = entry.value();
            item.market_id == record.market_id
                && item.outcome == record.outcome
                && item.user_id == record.user_id
                && liquidation_queue_status_is_active(&item.status)
        });
        if has_active {
            return Ok(false);
        }
        self.store.append(&record)?;
        self.entries.insert(record.queue_id.clone(), record);
        Ok(true)
    }

    fn get(&self, queue_id: &str) -> Option<LiquidationQueueRecord> {
        self.entries
            .get(queue_id)
            .map(|entry| entry.value().clone())
    }

    fn list_recent(
        &self,
        limit: usize,
        status_filter: Option<&str>,
    ) -> Vec<LiquidationQueueRecord> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.value().clone())
            .filter(|item| status_filter.is_none_or(|status| item.status == status))
            .collect();
        items.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));
        items.truncate(limit);
        items
    }

    fn list_by_statuses_oldest(
        &self,
        limit: usize,
        statuses: &[&str],
    ) -> Vec<LiquidationQueueRecord> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.value().clone())
            .filter(|item| statuses.iter().any(|status| item.status == *status))
            .collect();
        items.sort_by(|lhs, rhs| lhs.recorded_at.cmp(&rhs.recorded_at));
        items.truncate(limit);
        items
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LiquidationAuctionBid {
    bidder_user_id: String,
    bid_price: i64,
    #[serde(default)]
    bid_quantity: i64,
    submitted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LiquidationAuctionRecord {
    auction_id: String,
    queue_id: String,
    status: String,
    market_id: String,
    outcome: i32,
    liquidated_user_id: String,
    reserve_price: i64,
    mark_price: i64,
    #[serde(default)]
    round: u32,
    #[serde(default)]
    target_position_qty: i64,
    #[serde(default)]
    filled_position_qty: i64,
    opened_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    best_bid_price: Option<i64>,
    best_bidder_user_id: Option<String>,
    bids: Vec<LiquidationAuctionBid>,
    winner_user_id: Option<String>,
    error: Option<String>,
    recorded_at: DateTime<Utc>,
}

struct LiquidationAuctionStore {
    entries: DashMap<String, LiquidationAuctionRecord>,
    store: Arc<dyn persistence::WalStore<LiquidationAuctionRecord>>,
    write_lock: Mutex<()>,
}

impl LiquidationAuctionStore {
    fn new(
        store: Arc<dyn persistence::WalStore<LiquidationAuctionRecord>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            entries: DashMap::new(),
            store,
            write_lock: Mutex::new(()),
        };
        for record in result.store.entries()? {
            result.entries.insert(record.queue_id.clone(), record);
        }
        Ok(result)
    }

    fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<LiquidationAuctionRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    fn append(&self, record: LiquidationAuctionRecord) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store.append(&record)?;
        self.entries.insert(record.queue_id.clone(), record);
        Ok(())
    }

    fn submit_bid(
        &self,
        queue_id: &str,
        bidder_user_id: &str,
        bid_price: i64,
        bid_quantity: i64,
        now: DateTime<Utc>,
    ) -> anyhow::Result<LiquidationAuctionRecord> {
        let _guard = self.write_lock.lock();
        let mut next = self
            .entries
            .get(queue_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| anyhow::anyhow!("liquidation auction not found"))?;
        if next.status != "open" {
            anyhow::bail!("auction is not open");
        }
        if next.expires_at <= now {
            anyhow::bail!("auction already expired");
        }
        next.bids.push(LiquidationAuctionBid {
            bidder_user_id: bidder_user_id.to_string(),
            bid_price,
            bid_quantity,
            submitted_at: now,
        });
        next.bids.sort_by(|lhs, rhs| {
            rhs.bid_price
                .cmp(&lhs.bid_price)
                .then_with(|| lhs.submitted_at.cmp(&rhs.submitted_at))
        });
        next.best_bid_price = next.bids.first().map(|bid| bid.bid_price);
        next.best_bidder_user_id = next.bids.first().map(|bid| bid.bidder_user_id.clone());
        next.recorded_at = now;
        self.store.append(&next)?;
        self.entries.insert(next.queue_id.clone(), next.clone());
        Ok(next)
    }

    fn get(&self, queue_id: &str) -> Option<LiquidationAuctionRecord> {
        self.entries
            .get(queue_id)
            .map(|entry| entry.value().clone())
    }

    fn list_recent(
        &self,
        limit: usize,
        status_filter: Option<&str>,
    ) -> Vec<LiquidationAuctionRecord> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.value().clone())
            .filter(|item| status_filter.is_none_or(|status| item.status == status))
            .collect();
        items.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));
        items.truncate(limit);
        items
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AdlGovernanceRecord {
    governance: AdlGovernance,
    updated_by: String,
    recorded_at: DateTime<Utc>,
}

struct PersistentAdlGovernanceStore {
    current: Mutex<AdlGovernanceRecord>,
    store: Arc<dyn persistence::WalStore<AdlGovernanceRecord>>,
}

impl PersistentAdlGovernanceStore {
    fn default_record() -> AdlGovernanceRecord {
        AdlGovernanceRecord {
            governance: AdlGovernance::default(),
            updated_by: "system-default".to_string(),
            recorded_at: Utc::now(),
        }
    }

    fn new(store: Arc<dyn persistence::WalStore<AdlGovernanceRecord>>) -> anyhow::Result<Self> {
        let current = store
            .entries()?
            .into_iter()
            .last()
            .unwrap_or_else(Self::default_record);
        Ok(Self {
            current: Mutex::new(current),
            store,
        })
    }

    fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<AdlGovernanceRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    fn current(&self) -> AdlGovernanceRecord {
        self.current.lock().clone()
    }

    fn upsert(&self, record: AdlGovernanceRecord) -> anyhow::Result<()> {
        let mut guard = self.current.lock();
        self.store.append(&record)?;
        *guard = record;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LiquidationPolicyRecord {
    auction_window_secs: i64,
    retry_backoff_secs: Vec<i64>,
    max_retry_tiers: u32,
    #[serde(default = "default_max_auction_rounds")]
    max_auction_rounds: u32,
    updated_by: String,
    recorded_at: DateTime<Utc>,
}

struct PersistentLiquidationPolicyStore {
    current: Mutex<LiquidationPolicyRecord>,
    store: Arc<dyn persistence::WalStore<LiquidationPolicyRecord>>,
}

impl PersistentLiquidationPolicyStore {
    fn default_record() -> LiquidationPolicyRecord {
        LiquidationPolicyRecord {
            auction_window_secs: liquidation_auction_window_secs(),
            retry_backoff_secs: vec![0, 5, 15],
            max_retry_tiers: 3,
            max_auction_rounds: 3,
            updated_by: "system-default".to_string(),
            recorded_at: Utc::now(),
        }
    }

    fn new(store: Arc<dyn persistence::WalStore<LiquidationPolicyRecord>>) -> anyhow::Result<Self> {
        let current = store
            .entries()?
            .into_iter()
            .last()
            .unwrap_or_else(Self::default_record);
        Ok(Self {
            current: Mutex::new(current),
            store,
        })
    }

    fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<LiquidationPolicyRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    fn current(&self) -> LiquidationPolicyRecord {
        self.current.lock().clone()
    }

    fn upsert(&self, record: LiquidationPolicyRecord) -> anyhow::Result<()> {
        let mut guard = self.current.lock();
        self.store.append(&record)?;
        *guard = record;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct IndexPriceRecord {
    market_id: String,
    outcome: i32,
    index_price: i64,
    source: String,
    recorded_at: DateTime<Utc>,
}

#[derive(Clone)]
struct FixedWindowRateLimiter {
    window: Duration,
    states: Arc<DashMap<String, VecDeque<Instant>>>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl Reject for ApiError {}

impl FixedWindowRateLimiter {
    fn new(window: Duration) -> Self {
        Self {
            window,
            states: Arc::new(DashMap::new()),
        }
    }

    fn check(&self, key: &str, limit: usize) -> Result<(), Rejection> {
        let now = Instant::now();
        let mut bucket = self.states.entry(key.to_string()).or_default();
        while bucket
            .front()
            .is_some_and(|timestamp| now.duration_since(*timestamp) > self.window)
        {
            bucket.pop_front();
        }
        if bucket.len() >= limit {
            return Err(warp::reject::custom(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: "rate limit exceeded".to_string(),
            }));
        }
        bucket.push_back(now);
        Ok(())
    }
}

fn reject_api(status: StatusCode, message: impl Into<String>) -> Rejection {
    warp::reject::custom(ApiError {
        status,
        message: message.into(),
    })
}

fn optional_query<T>() -> impl Filter<Extract = (T,), Error = Infallible> + Clone
where
    T: DeserializeOwned + Default + Send + 'static,
{
    warp::query::<T>().or(warp::any().map(T::default)).unify()
}

fn body_limit() -> impl Filter<Extract = (), Error = Rejection> + Clone {
    warp::body::content_length_limit(16 * 1024)
}

fn remote_ip() -> impl Filter<Extract = (Option<SocketAddr>,), Error = Infallible> + Clone {
    warp::addr::remote()
}

async fn handle_rejection(rejection: Rejection) -> Result<impl Reply, Infallible> {
    if let Some(error) = rejection.find::<ApiError>() {
        let body = serde_json::json!({"status":"error","error":error.message});
        return Ok(warp::reply::with_status(
            warp::reply::json(&body),
            error.status,
        ));
    }
    if rejection.is_not_found() {
        let body = serde_json::json!({"status":"error","error":"not found"});
        return Ok(warp::reply::with_status(
            warp::reply::json(&body),
            StatusCode::NOT_FOUND,
        ));
    }
    let body = serde_json::json!({"status":"error","error":"internal server error"});
    Ok(warp::reply::with_status(
        warp::reply::json(&body),
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

fn bind_address() -> SocketAddr {
    let host = env::var("API_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("API_BIND_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3030);
    let ip = host
        .parse::<IpAddr>()
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    SocketAddr::new(ip, port)
}

fn flatten_market_snapshots(records: &[PartitionSnapshotRecord]) -> Vec<MarketRuntimeSnapshot> {
    records
        .iter()
        .flat_map(|record| record.snapshot.markets.iter().cloned())
        .collect()
}

fn market_best_levels(snapshot: &MarketRuntimeSnapshot) -> (Option<i64>, Option<i64>) {
    let mut best_bid = None;
    let mut best_ask = None;
    for order in &snapshot.orders {
        match order.side {
            Side::Buy => {
                best_bid = Some(best_bid.map_or(order.price, |value: i64| value.max(order.price)))
            }
            Side::Sell => {
                best_ask = Some(best_ask.map_or(order.price, |value: i64| value.min(order.price)))
            }
        }
    }
    (best_bid, best_ask)
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

fn aggregate_market_state(states: impl Iterator<Item = MarketState>) -> MarketState {
    states
        .max_by_key(|state| market_state_rank(*state))
        .unwrap_or(MarketState::Normal)
}

fn snapshot_to_market_view(snapshot: &MarketRuntimeSnapshot) -> serde_json::Value {
    let (best_bid, best_ask) = market_best_levels(snapshot);
    serde_json::json!({
        "market_id": snapshot.market_id,
        "outcome": snapshot.outcome,
        "state": snapshot.state,
        "reference_price": snapshot.reference_price,
        "last_trade_price": snapshot.last_trade_price,
        "best_bid": best_bid,
        "best_ask": best_ask,
        "open_orders": snapshot.orders.len(),
    })
}

fn snapshots_to_market_list(snapshots: &[MarketRuntimeSnapshot]) -> Vec<serde_json::Value> {
    let mut grouped: BTreeMap<String, Vec<&MarketRuntimeSnapshot>> = BTreeMap::new();
    for snapshot in snapshots {
        grouped
            .entry(snapshot.market_id.clone())
            .or_default()
            .push(snapshot);
    }

    grouped
        .into_iter()
        .map(|(market_id, group)| {
            let state = aggregate_market_state(group.iter().map(|snapshot| snapshot.state));
            let outcomes: Vec<i32> = group.iter().map(|snapshot| snapshot.outcome).collect();
            let total_open_orders: usize = group.iter().map(|snapshot| snapshot.orders.len()).sum();
            serde_json::json!({
                "id": market_id,
                "market_id": market_id,
                "name": market_id,
                "state": state,
                "outcomes": outcomes,
                "open_orders": total_open_orders,
                "markets": group.into_iter().map(snapshot_to_market_view).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn validate_instrument_spec(spec: &InstrumentSpec) -> Result<(), Rejection> {
    if spec.instrument_id.trim().is_empty() {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "instrument_id is required",
        ));
    }
    if spec.quote_asset.trim().is_empty() {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "quote_asset is required",
        ));
    }
    if spec.risk_policy_id.trim().is_empty() {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "risk_policy_id is required",
        ));
    }
    if spec.tick_size <= 0 {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "tick_size must be positive",
        ));
    }
    if spec.lot_size <= 0 {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "lot_size must be positive",
        ));
    }
    if spec.price_band_bps < 0 {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "price_band_bps must be non-negative",
        ));
    }
    match spec.kind {
        InstrumentKind::Spot => {
            if spec.margin_mode.is_some() || spec.max_leverage.is_some() {
                return Err(reject_api(
                    StatusCode::BAD_REQUEST,
                    "spot instruments cannot define margin settings",
                ));
            }
        }
        InstrumentKind::Margin | InstrumentKind::Perpetual => {
            if spec.max_leverage.unwrap_or(0) == 0 {
                return Err(reject_api(
                    StatusCode::BAD_REQUEST,
                    "derivative instruments require positive max_leverage",
                ));
            }
        }
    }
    Ok(())
}

fn orders_to_levels(
    orders: &[RestingOrderSnapshot],
    side: Side,
    depth: usize,
) -> Vec<serde_json::Value> {
    let mut levels: BTreeMap<i64, (i64, usize)> = BTreeMap::new();
    for order in orders.iter().filter(|order| order.side == side) {
        let entry = levels.entry(order.price).or_insert((0, 0));
        entry.0 += order.remaining_amount;
        entry.1 += 1;
    }

    let mut items: Vec<_> = levels.into_iter().collect();
    if side == Side::Buy {
        items.reverse();
    }
    items
        .into_iter()
        .take(depth)
        .map(|(price, (amount, count))| {
            serde_json::json!({
                "price": price,
                "amount": amount,
                "count": count,
            })
        })
        .collect()
}

fn snapshot_to_order_book(snapshot: &MarketRuntimeSnapshot, depth: usize) -> serde_json::Value {
    serde_json::json!({
        "market_id": snapshot.market_id,
        "outcome": snapshot.outcome,
        "bids": orders_to_levels(&snapshot.orders, Side::Buy, depth),
        "asks": orders_to_levels(&snapshot.orders, Side::Sell, depth),
        "timestamp": Utc::now(),
    })
}

fn snapshots_to_orders(
    snapshots: &[MarketRuntimeSnapshot],
    user_id: &str,
    market_filter: Option<&str>,
    outcome_filter: Option<i32>,
) -> Vec<serde_json::Value> {
    let mut orders: Vec<_> = snapshots
        .iter()
        .filter(|snapshot| market_filter.is_none_or(|market_id| market_id == snapshot.market_id))
        .filter(|snapshot| outcome_filter.is_none_or(|outcome| outcome == snapshot.outcome))
        .flat_map(|snapshot| snapshot.orders.iter())
        .filter(|order| order.user_id == user_id)
        .map(|order| {
            serde_json::json!({
                "id": order.order_id,
                "market_id": order.market_id,
                "outcome": order.outcome,
                "side": order.side,
                "price": order.price,
                "amount": order.original_amount,
                "filled": order.original_amount - order.remaining_amount,
                "remaining": order.remaining_amount,
                "leverage": order.leverage,
                "reduce_only": order.reduce_only,
                "status": if order.remaining_amount < order.original_amount { "partial" } else { "open" },
                "created_at": Utc::now(),
            })
        })
        .collect();
    orders.sort_by(|lhs, rhs| {
        rhs["created_at"]
            .to_string()
            .cmp(&lhs["created_at"].to_string())
    });
    orders
}

fn trade_record_to_json(record: &TradeJournalRecord) -> serde_json::Value {
    serde_json::json!({
        "id": record.trade_id,
        "market_id": record.market_id,
        "outcome": record.outcome,
        "price": record.price,
        "amount": record.amount,
        "buyer": record.buy_user_id,
        "seller": record.sell_user_id,
        "buy_order_id": record.buy_order_id,
        "sell_order_id": record.sell_order_id,
        "timestamp": record.recorded_at,
    })
}

fn trades_to_history(
    market_id: &str,
    outcome: Option<i32>,
    trades: &[TradeJournalRecord],
    limit: usize,
) -> serde_json::Value {
    let mut grouped: BTreeMap<String, Vec<&TradeJournalRecord>> = BTreeMap::new();
    for trade in trades
        .iter()
        .filter(|trade| trade.market_id == market_id)
        .filter(|trade| outcome.is_none_or(|value| value == trade.outcome))
    {
        let key = trade.recorded_at.format("%Y-%m-%dT%H:00:00Z").to_string();
        grouped.entry(key).or_default().push(trade);
    }

    let mut data: Vec<_> = grouped
        .into_iter()
        .map(|(timestamp, bucket)| {
            let open = bucket.first().map(|trade| trade.price).unwrap_or(0);
            let close = bucket.last().map(|trade| trade.price).unwrap_or(open);
            let high = bucket.iter().map(|trade| trade.price).max().unwrap_or(open);
            let low = bucket.iter().map(|trade| trade.price).min().unwrap_or(open);
            let volume: i64 = bucket.iter().map(|trade| trade.amount).sum();
            serde_json::json!({
                "timestamp": timestamp,
                "price": close,
                "volume": volume,
                "high": high,
                "low": low,
                "open": open,
                "close": close,
            })
        })
        .collect();
    data.sort_by(|lhs, rhs| {
        lhs["timestamp"]
            .to_string()
            .cmp(&rhs["timestamp"].to_string())
    });
    if data.len() > limit {
        data = data.split_off(data.len() - limit);
    }
    serde_json::json!({
        "market_id": market_id,
        "interval": "1h",
        "data": data,
    })
}

fn deposits_from_ledger(user_id: &str, ledger_entries: &[LedgerDelta]) -> Vec<serde_json::Value> {
    let account = format!("U:{user_id}:USDC");
    ledger_entries
        .iter()
        .filter(|delta| {
            delta.entries.iter().any(|entry| {
                entry.credit_account == account && entry.debit_account == "SYS:ONCHAIN_VAULT:USDC"
            })
        })
        .map(|delta| {
            let amount: i64 = delta
                .entries
                .iter()
                .filter(|entry| entry.credit_account == account)
                .map(|entry| entry.amount)
                .sum();
            serde_json::json!({
                "id": delta.op_id,
                "amount": amount,
                "asset": "USDC",
                "tx_hash": delta.op_id,
                "status": "confirmed",
                "timestamp": delta.timestamp,
            })
        })
        .collect()
}

fn stats_from_snapshots_and_trades(
    snapshots: &[MarketRuntimeSnapshot],
    trades: &[TradeJournalRecord],
    ledger_entries: &[LedgerDelta],
) -> serde_json::Value {
    let total_volume_24h: i64 = trades
        .iter()
        .map(|trade| trade.price.saturating_mul(trade.amount))
        .sum();
    let total_liquidity: i64 = snapshots
        .iter()
        .flat_map(|snapshot| snapshot.orders.iter())
        .map(|order| order.remaining_amount)
        .sum();
    let total_users = ledger_entries
        .iter()
        .flat_map(|delta| delta.entries.iter())
        .filter_map(|entry| entry.credit_account.strip_prefix("U:"))
        .filter_map(|account| account.split(':').next())
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    serde_json::json!({
        "TotalVolume24h": total_volume_24h,
        "TotalTrades24h": trades.len(),
        "ActiveMarkets": snapshots.len(),
        "TotalUsers": total_users,
        "TotalLiquidity": total_liquidity,
        "LastUpdated": Utc::now(),
    })
}

fn sequencer_wal_path() -> String {
    env::var("SEQUENCER_WAL_PATH").unwrap_or_else(|_| "data/sequencer.wal.jsonl".to_string())
}

fn ledger_wal_path() -> String {
    env::var("LEDGER_WAL_PATH").unwrap_or_else(|_| "data/ledger.wal.jsonl".to_string())
}

fn matching_snapshot_wal_path() -> String {
    env::var("MATCHING_SNAPSHOT_WAL_PATH")
        .unwrap_or_else(|_| "data/matching.snapshot.jsonl".to_string())
}

fn trade_journal_wal_path() -> String {
    env::var("TRADE_JOURNAL_WAL_PATH")
        .unwrap_or_else(|_| "data/trade_journal.wal.jsonl".to_string())
}

fn instruments_registry_wal_path() -> String {
    env::var("INSTRUMENTS_REGISTRY_WAL_PATH")
        .unwrap_or_else(|_| "data/instruments.registry.jsonl".to_string())
}

fn funding_rates_wal_path() -> String {
    env::var("FUNDING_RATES_WAL_PATH").unwrap_or_else(|_| "data/funding_rates.jsonl".to_string())
}

fn risk_automation_audit_wal_path() -> String {
    env::var("RISK_AUTOMATION_AUDIT_WAL_PATH")
        .unwrap_or_else(|_| "data/risk_automation.audit.jsonl".to_string())
}

fn liquidation_queue_wal_path() -> String {
    env::var("LIQUIDATION_QUEUE_WAL_PATH")
        .unwrap_or_else(|_| "data/liquidation.queue.jsonl".to_string())
}

fn liquidation_auction_wal_path() -> String {
    env::var("LIQUIDATION_AUCTION_WAL_PATH")
        .unwrap_or_else(|_| "data/liquidation.auction.jsonl".to_string())
}

fn adl_governance_wal_path() -> String {
    env::var("ADL_GOVERNANCE_WAL_PATH").unwrap_or_else(|_| "data/adl.governance.jsonl".to_string())
}

fn liquidation_policy_wal_path() -> String {
    env::var("LIQUIDATION_POLICY_WAL_PATH")
        .unwrap_or_else(|_| "data/liquidation.policy.jsonl".to_string())
}

fn index_price_wal_path() -> String {
    env::var("INDEX_PRICE_WAL_PATH").unwrap_or_else(|_| "data/index.price.jsonl".to_string())
}

fn governance_action_wal_path() -> String {
    env::var("GOVERNANCE_ACTION_WAL_PATH")
        .unwrap_or_else(|_| "data/governance.actions.jsonl".to_string())
}

fn env_bool(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

fn automation_enabled() -> bool {
    env_bool("RISK_AUTOMATION_ENABLED", false)
}

fn liquidation_interval_secs() -> u64 {
    env::var("RISK_LIQUIDATION_INTERVAL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30)
}

fn funding_interval_secs() -> u64 {
    env::var("RISK_FUNDING_INTERVAL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60)
}

fn liquidation_worker_interval_secs() -> u64 {
    env::var("RISK_LIQUIDATION_WORKER_INTERVAL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5)
}

fn liquidation_auction_window_secs() -> i64 {
    env::var("RISK_LIQUIDATION_AUCTION_WINDOW_SECS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(15)
}

fn automation_liquidator_user_id() -> String {
    env::var("RISK_AUTOMATION_LIQUIDATOR_USER_ID")
        .unwrap_or_else(|_| "system-liquidator".to_string())
}

fn automation_maintenance_margin_bps() -> i64 {
    env::var("RISK_AUTOMATION_MAINTENANCE_MARGIN_BPS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(1_000)
}

fn automation_liquidation_penalty_bps() -> i64 {
    env::var("RISK_AUTOMATION_LIQUIDATION_PENALTY_BPS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(500)
}

fn default_max_auction_rounds() -> u32 {
    3
}

fn liquidation_retry_delay_secs(policy: &LiquidationPolicyRecord, retry_tier: u32) -> i64 {
    policy
        .retry_backoff_secs
        .get(retry_tier as usize)
        .copied()
        .or_else(|| policy.retry_backoff_secs.last().copied())
        .unwrap_or(0)
        .max(0)
}

#[allow(clippy::too_many_arguments)]
fn append_risk_audit_event(
    audit_store: &RiskAutomationAuditStore,
    event_type: &str,
    status: &str,
    market_id: &str,
    outcome: i32,
    user_id: Option<String>,
    counterparty_user_id: Option<String>,
    request_id: &str,
    detail: serde_json::Value,
) {
    let _ = audit_store.append(RiskAutomationAuditRecord {
        event_id: types::generate_op_id("risk-event"),
        event_type: event_type.to_string(),
        status: status.to_string(),
        market_id: market_id.to_string(),
        outcome,
        user_id,
        counterparty_user_id,
        request_id: request_id.to_string(),
        detail,
        recorded_at: Utc::now(),
    });
}

#[allow(clippy::too_many_arguments)]
fn sequence_new_order(
    sequencer: &Sequencer,
    request_id: String,
    client_order_id: String,
    user_id: String,
    session_id: Option<String>,
    market_id: String,
    side: Side,
    order_type: OrderType,
    time_in_force: TimeInForce,
    price: Option<i64>,
    amount: i64,
    outcome: i32,
    post_only: bool,
    reduce_only: bool,
    leverage: Option<u32>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<NewOrderCommand, String> {
    let command = Command::NewOrder(NewOrderCommand {
        metadata: CommandMetadata::new(request_id),
        client_order_id,
        user_id,
        session_id,
        market_id,
        side,
        order_type,
        time_in_force,
        price,
        amount,
        outcome,
        post_only,
        reduce_only,
        leverage,
        expires_at,
    });

    match sequencer
        .sequence_and_append(command)
        .map_err(|error| error.to_string())?
    {
        Command::NewOrder(command) => Ok(command),
        _ => Err("sequencer returned non-new-order command unexpectedly".to_string()),
    }
}

fn sequence_command(sequencer: &Sequencer, command: Command) -> Result<Command, String> {
    sequencer
        .sequence_and_append(command)
        .map_err(|error| error.to_string())
}

fn sequence_cancel_order(
    sequencer: &Sequencer,
    request_id: String,
    user_id: String,
    market_id: String,
    outcome: Option<i32>,
    order_id: String,
    client_order_id: Option<String>,
) -> Result<CancelOrderCommand, String> {
    match sequence_command(
        sequencer,
        Command::CancelOrder(CancelOrderCommand {
            metadata: CommandMetadata::new(request_id),
            user_id,
            market_id,
            outcome,
            order_id,
            client_order_id,
        }),
    )? {
        Command::CancelOrder(command) => Ok(command),
        _ => Err("sequencer returned non-cancel-order command unexpectedly".to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn sequence_replace_order(
    sequencer: &Sequencer,
    request_id: String,
    user_id: String,
    market_id: String,
    outcome: Option<i32>,
    order_id: String,
    new_client_order_id: Option<String>,
    new_price: Option<i64>,
    new_amount: Option<i64>,
    new_time_in_force: Option<TimeInForce>,
    post_only: Option<bool>,
    reduce_only: Option<bool>,
    new_leverage: Option<u32>,
    new_expires_at: Option<DateTime<Utc>>,
) -> Result<ReplaceOrderCommand, String> {
    match sequence_command(
        sequencer,
        Command::ReplaceOrder(ReplaceOrderCommand {
            metadata: CommandMetadata::new(request_id),
            user_id,
            market_id,
            outcome,
            order_id,
            new_client_order_id,
            new_price,
            new_amount,
            new_time_in_force,
            post_only,
            reduce_only,
            new_leverage,
            new_expires_at,
        }),
    )? {
        Command::ReplaceOrder(command) => Ok(command),
        _ => Err("sequencer returned non-replace-order command unexpectedly".to_string()),
    }
}

fn sequence_mass_cancel_by_user(
    sequencer: &Sequencer,
    request_id: String,
    user_id: String,
) -> Result<MassCancelByUserCommand, String> {
    match sequence_command(
        sequencer,
        Command::MassCancelByUser(MassCancelByUserCommand {
            metadata: CommandMetadata::new(request_id),
            user_id,
        }),
    )? {
        Command::MassCancelByUser(command) => Ok(command),
        _ => Err("sequencer returned non-mass-cancel-user command unexpectedly".to_string()),
    }
}

fn sequence_mass_cancel_by_session(
    sequencer: &Sequencer,
    request_id: String,
    user_id: String,
    session_id: String,
) -> Result<MassCancelBySessionCommand, String> {
    match sequence_command(
        sequencer,
        Command::MassCancelBySession(MassCancelBySessionCommand {
            metadata: CommandMetadata::new(request_id),
            user_id,
            session_id,
        }),
    )? {
        Command::MassCancelBySession(command) => Ok(command),
        _ => Err("sequencer returned non-mass-cancel-session command unexpectedly".to_string()),
    }
}

fn sequence_mass_cancel_by_market(
    sequencer: &Sequencer,
    request_id: String,
    market_id: String,
) -> Result<MassCancelByMarketCommand, String> {
    match sequence_command(
        sequencer,
        Command::MassCancelByMarket(MassCancelByMarketCommand {
            metadata: CommandMetadata::new(request_id),
            market_id,
        }),
    )? {
        Command::MassCancelByMarket(command) => Ok(command),
        _ => Err("sequencer returned non-mass-cancel-market command unexpectedly".to_string()),
    }
}

fn sequence_admin(
    sequencer: &Sequencer,
    request_id: String,
    actor_id: String,
    action: AdminAction,
) -> Result<AdminCommand, String> {
    match sequence_command(
        sequencer,
        Command::Admin(AdminCommand {
            metadata: CommandMetadata::new(request_id),
            actor_id,
            action,
        }),
    )? {
        Command::Admin(command) => Ok(command),
        _ => Err("sequencer returned non-admin command unexpectedly".to_string()),
    }
}

fn seed_default_instruments(registry: &PersistentInstrumentRegistry) {
    for spec in [
        InstrumentSpec {
            instrument_id: "btc-usdt".to_string(),
            kind: InstrumentKind::Spot,
            quote_asset: "USDC".to_string(),
            margin_mode: None,
            max_leverage: None,
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "spot-v1".to_string(),
        },
        InstrumentSpec {
            instrument_id: "margin:btc-usdt".to_string(),
            kind: InstrumentKind::Margin,
            quote_asset: "USDC".to_string(),
            margin_mode: Some(MarginMode::Isolated),
            max_leverage: Some(20),
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "margin-v1".to_string(),
        },
        InstrumentSpec {
            instrument_id: "perp:btc-usdt".to_string(),
            kind: InstrumentKind::Perpetual,
            quote_asset: "USDC".to_string(),
            margin_mode: Some(MarginMode::Isolated),
            max_leverage: Some(20),
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "perpetual-v1".to_string(),
        },
    ] {
        let _ = registry.upsert(spec);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_liquidation_cycle(
    engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    index_prices: Arc<PersistentIndexPriceStore>,
    audit_store: Arc<RiskAutomationAuditStore>,
    queue_store: Arc<LiquidationQueueStore>,
    adl_governance_store: Arc<PersistentAdlGovernanceStore>,
    liquidator_user_id: &str,
    maintenance_margin_bps: i64,
    _penalty_bps: i64,
) {
    let records = match engine.export_snapshots().await {
        Ok(records) => records,
        Err(error) => {
            append_risk_audit_event(
                audit_store.as_ref(),
                "liquidation_cycle",
                "error",
                "*",
                0,
                None,
                None,
                "automation-liquidation-cycle",
                serde_json::json!({"error": error.to_string()}),
            );
            return;
        }
    };
    let governance = adl_governance_store.current().governance;
    let snapshots = flatten_market_snapshots(&records);
    let user_ids = risk.ledger().user_ids();

    for snapshot in snapshots {
        let instrument = instruments.resolve(&snapshot.market_id);
        if instrument.kind == InstrumentKind::Spot {
            continue;
        }
        let Some(mark_price) = fair_price_quote_for_snapshot(&snapshot, index_prices.as_ref())
            .map(|quote| quote.fair_price)
        else {
            continue;
        };
        let candidates = match risk.liquidation_candidates(
            &user_ids,
            &instrument,
            snapshot.outcome,
            mark_price,
            instrument.max_leverage,
            maintenance_margin_bps,
        ) {
            Ok(candidates) => candidates,
            Err(error) => {
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "liquidation_scan",
                    "error",
                    &snapshot.market_id,
                    snapshot.outcome,
                    None,
                    None,
                    "automation-liquidation-scan",
                    serde_json::json!({"error": error.to_string()}),
                );
                continue;
            }
        };

        for candidate in candidates {
            if candidate.user_id == liquidator_user_id {
                continue;
            }
            let request_id = types::generate_op_id("auto-liq");
            let adl_candidates = risk.adl_ranking_with_governance(
                &instrument,
                snapshot.outcome,
                mark_price,
                candidate.position_qty,
                &governance,
            );
            let record = LiquidationQueueRecord {
                queue_id: request_id.clone(),
                source: "automation".to_string(),
                status: "queued".to_string(),
                market_id: snapshot.market_id.clone(),
                outcome: snapshot.outcome,
                user_id: candidate.user_id.clone(),
                liquidator_user_id: liquidator_user_id.to_string(),
                mark_price,
                position_qty: candidate.position_qty,
                remaining_position_qty: candidate.position_qty.abs(),
                filled_position_qty: 0,
                auction_round: 0,
                margin_ratio_bps: candidate.margin_ratio_bps,
                adl_candidates: adl_candidates.clone(),
                retry_tier: 0,
                retry_count: 0,
                strategy: liquidation_strategy_for_tier(0).to_string(),
                next_attempt_at: None,
                last_attempt_at: None,
                error: None,
                recorded_at: Utc::now(),
            };
            match queue_store.append_if_no_active_position(record) {
                Ok(true) => append_risk_audit_event(
                    audit_store.as_ref(),
                    "liquidation_queued",
                    "queued",
                    &snapshot.market_id,
                    snapshot.outcome,
                    Some(candidate.user_id.clone()),
                    Some(liquidator_user_id.to_string()),
                    &request_id,
                    serde_json::json!({
                        "mark_price": mark_price,
                        "position_qty": candidate.position_qty,
                        "margin_ratio_bps": candidate.margin_ratio_bps,
                        "maintenance_margin_required": candidate.maintenance_margin_required,
                        "retry_tier": 0,
                        "adl_candidates": adl_candidates,
                    }),
                ),
                Ok(false) => append_risk_audit_event(
                    audit_store.as_ref(),
                    "liquidation_queued",
                    "skipped",
                    &snapshot.market_id,
                    snapshot.outcome,
                    Some(candidate.user_id.clone()),
                    Some(liquidator_user_id.to_string()),
                    &request_id,
                    serde_json::json!({"reason": "active liquidation already exists"}),
                ),
                Err(error) => append_risk_audit_event(
                    audit_store.as_ref(),
                    "liquidation_queued",
                    "error",
                    &snapshot.market_id,
                    snapshot.outcome,
                    Some(candidate.user_id.clone()),
                    Some(liquidator_user_id.to_string()),
                    &request_id,
                    serde_json::json!({"error": error.to_string()}),
                ),
            };
        }
    }
}

async fn run_funding_cycle(
    engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    funding_rates: Arc<PersistentFundingRateStore>,
    index_prices: Arc<PersistentIndexPriceStore>,
    audit_store: Arc<RiskAutomationAuditStore>,
) {
    let records = match engine.export_snapshots().await {
        Ok(records) => records,
        Err(error) => {
            append_risk_audit_event(
                audit_store.as_ref(),
                "funding_cycle",
                "error",
                "*",
                0,
                None,
                None,
                "automation-funding-cycle",
                serde_json::json!({"error": error.to_string()}),
            );
            return;
        }
    };
    let snapshots = flatten_market_snapshots(&records);
    let user_ids = risk.ledger().user_ids();
    for rate in funding_rates.list() {
        let Some(mark_price) = resolve_mark_price_for_market(
            &snapshots,
            index_prices.as_ref(),
            &rate.market_id,
            rate.outcome,
        ) else {
            append_risk_audit_event(
                audit_store.as_ref(),
                "funding_batch",
                "skipped",
                &rate.market_id,
                rate.outcome,
                None,
                None,
                "automation-funding-skip",
                serde_json::json!({"reason": "mark price unavailable"}),
            );
            continue;
        };
        let request_id = types::generate_op_id("auto-funding");
        match risk.settle_funding_batch(
            &rate.market_id,
            rate.outcome,
            mark_price,
            rate.funding_rate_ppm,
            &user_ids,
            &request_id,
        ) {
            Ok(settlements) if settlements.is_empty() => append_risk_audit_event(
                audit_store.as_ref(),
                "funding_batch",
                "skipped",
                &rate.market_id,
                rate.outcome,
                None,
                None,
                &request_id,
                serde_json::json!({"reason": "no eligible counterparties"}),
            ),
            Ok(settlements) => {
                for settlement in settlements {
                    append_risk_audit_event(
                        audit_store.as_ref(),
                        "funding_settled",
                        "ok",
                        &rate.market_id,
                        rate.outcome,
                        Some(settlement.payer_user_id.clone()),
                        Some(settlement.receiver_user_id.clone()),
                        &request_id,
                        serde_json::json!(settlement),
                    );
                }
            }
            Err(error) => append_risk_audit_event(
                audit_store.as_ref(),
                "funding_batch",
                "error",
                &rate.market_id,
                rate.outcome,
                None,
                None,
                &request_id,
                serde_json::json!({"error": error.to_string()}),
            ),
        }
    }
}

async fn run_liquidation_scheduler(
    engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    index_prices: Arc<PersistentIndexPriceStore>,
    audit_store: Arc<RiskAutomationAuditStore>,
    queue_store: Arc<LiquidationQueueStore>,
    adl_governance_store: Arc<PersistentAdlGovernanceStore>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(liquidation_interval_secs()));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let liquidator_user_id = automation_liquidator_user_id();
    let maintenance_margin_bps = automation_maintenance_margin_bps();
    let penalty_bps = automation_liquidation_penalty_bps();
    loop {
        interval.tick().await;
        run_liquidation_cycle(
            engine.clone(),
            risk.clone(),
            instruments.clone(),
            index_prices.clone(),
            audit_store.clone(),
            queue_store.clone(),
            adl_governance_store.clone(),
            &liquidator_user_id,
            maintenance_margin_bps,
            penalty_bps,
        )
        .await;
    }
}

async fn run_liquidation_worker_scheduler(
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    audit_store: Arc<RiskAutomationAuditStore>,
    queue_store: Arc<LiquidationQueueStore>,
    auction_store: Arc<LiquidationAuctionStore>,
    adl_governance_store: Arc<PersistentAdlGovernanceStore>,
    liquidation_policy_store: Arc<PersistentLiquidationPolicyStore>,
) {
    let mut interval =
        tokio::time::interval(Duration::from_secs(liquidation_worker_interval_secs()));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let liquidator_user_id = automation_liquidator_user_id();
    let maintenance_margin_bps = automation_maintenance_margin_bps();
    let penalty_bps = automation_liquidation_penalty_bps();
    loop {
        interval.tick().await;
        run_liquidation_queue_worker(
            risk.clone(),
            instruments.clone(),
            audit_store.clone(),
            queue_store.clone(),
            auction_store.clone(),
            adl_governance_store.clone(),
            liquidation_policy_store.clone(),
            &liquidator_user_id,
            maintenance_margin_bps,
            penalty_bps,
        )
        .await;
    }
}

async fn run_funding_scheduler(
    engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    funding_rates: Arc<PersistentFundingRateStore>,
    index_prices: Arc<PersistentIndexPriceStore>,
    audit_store: Arc<RiskAutomationAuditStore>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(funding_interval_secs()));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        run_funding_cycle(
            engine.clone(),
            risk.clone(),
            funding_rates.clone(),
            index_prices.clone(),
            audit_store.clone(),
        )
        .await;
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    initialize_internal_auth_secret().expect("failed to initialize internal auth secret");

    tracing::info!("Starting Rust Exchange...");
    let app = bootstrap_runtime(EventBus::new()).await;
    let AppBootstrap {
        ledger,
        sequencer,
        risk,
        instruments,
        funding_rates,
        risk_automation_audit,
        liquidation_queue,
        liquidation_auction,
        adl_governance,
        liquidation_policy,
        index_prices,
        governance_actions,
        partitioned_engine,
        trade_journal_wal,
    } = app;

    let ip_rate_limiter = Arc::new(FixedWindowRateLimiter::new(Duration::from_secs(1)));
    let user_rate_limiter = Arc::new(FixedWindowRateLimiter::new(Duration::from_secs(1)));
    let admin_rate_limiter = Arc::new(FixedWindowRateLimiter::new(Duration::from_secs(1)));

    let trading_routes = build_trading_routes(
        partitioned_engine.clone(),
        sequencer.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    let control_routes = build_control_routes(
        partitioned_engine.clone(),
        ledger.clone(),
        sequencer.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let account_routes = build_account_routes(
        partitioned_engine.clone(),
        risk.clone(),
        instruments.clone(),
        ledger.clone(),
        index_prices.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    let market_routes = build_market_routes(
        partitioned_engine.clone(),
        trade_journal_wal.clone(),
        ledger.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let admin_routes = build_admin_routes(
        risk.clone(),
        instruments.clone(),
        funding_rates.clone(),
        risk_automation_audit.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let pricing_admin_routes = build_pricing_routes(
        partitioned_engine.clone(),
        index_prices.clone(),
        governance_actions.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let governance_admin_routes = build_governance_routes(
        adl_governance.clone(),
        liquidation_policy.clone(),
        index_prices.clone(),
        liquidation_queue.clone(),
        governance_actions.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let liquidation_admin_routes = build_liquidation_routes(
        risk.clone(),
        instruments.clone(),
        adl_governance.clone(),
        liquidation_queue.clone(),
        liquidation_auction.clone(),
        ledger.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    let metrics_route = warp::path("metrics")
        .and(warp::get())
        .map(move || -> warp::reply::Json {
            warp::reply::json(&serde_json::json!({
                "status": "disabled",
                "prototype_only": false,
                "message": "prototype metrics removed from the primary runtime"
            }))
        });

    let static_files = warp::fs::dir("./frontend");
    let cors = warp::cors()
        .allow_origin("http://127.0.0.1:5173")
        .allow_origin("http://localhost:5173")
        .allow_methods(vec!["GET", "POST"])
        .allow_headers(vec![
            "authorization",
            "content-type",
            "x-request-id",
            "x-internal-auth-subject",
            "x-internal-auth-role",
            "x-internal-auth-session-id",
            "x-internal-auth-timestamp",
            "x-internal-auth-signature",
            "x-internal-auth-body-sha256",
        ]);
    let routes = trading_routes
        .or(control_routes)
        .or(admin_routes)
        .or(pricing_admin_routes)
        .or(governance_admin_routes)
        .or(liquidation_admin_routes)
        .or(account_routes)
        .or(market_routes)
        .or(metrics_route)
        .or(static_files)
        .with(cors)
        .recover(handle_rejection);

    spawn_automation_tasks(AutomationRuntime {
        partitioned_engine: partitioned_engine.clone(),
        risk: risk.clone(),
        instruments: instruments.clone(),
        funding_rates: funding_rates.clone(),
        risk_automation_audit: risk_automation_audit.clone(),
        liquidation_queue: liquidation_queue.clone(),
        liquidation_auction: liquidation_auction.clone(),
        adl_governance: adl_governance.clone(),
        liquidation_policy: liquidation_policy.clone(),
        index_prices: index_prices.clone(),
    });

    let bind_addr = bind_address();
    tracing::info!("Starting HTTP server on {}", bind_addr);
    tokio::spawn(async move {
        warp::serve(routes).run(bind_addr).await;
    });

    tracing::info!("Exchange running with HTTP. Press Ctrl+C to exit.");
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    tracing::info!("Shutting down...");
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::InMemoryWal;

    fn sample_queue_record(queue_id: &str) -> LiquidationQueueRecord {
        LiquidationQueueRecord {
            queue_id: queue_id.to_string(),
            source: "automation".to_string(),
            status: "queued".to_string(),
            market_id: "BTC-USD-PERP".to_string(),
            outcome: 0,
            user_id: "user-a".to_string(),
            liquidator_user_id: "liq-system".to_string(),
            mark_price: 100_000,
            position_qty: 10,
            remaining_position_qty: 10,
            filled_position_qty: 0,
            auction_round: 0,
            margin_ratio_bps: Some(500),
            adl_candidates: Vec::new(),
            retry_tier: 0,
            retry_count: 0,
            strategy: liquidation_strategy_for_tier(0).to_string(),
            next_attempt_at: None,
            last_attempt_at: None,
            error: None,
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn liquidation_queue_store_deduplicates_active_positions() {
        let store =
            LiquidationQueueStore::new(Arc::new(InMemoryWal::<LiquidationQueueRecord>::new()))
                .expect("queue store");
        assert!(store
            .append_if_no_active_position(sample_queue_record("q1"))
            .expect("append first"));
        assert!(!store
            .append_if_no_active_position(sample_queue_record("q2"))
            .expect("dedupe second"));
        assert_eq!(store.list_recent(10, None).len(), 1);
    }

    #[test]
    fn liquidation_auction_store_accumulates_bids_without_losing_best_bid() {
        let store =
            LiquidationAuctionStore::new(Arc::new(InMemoryWal::<LiquidationAuctionRecord>::new()))
                .expect("auction store");
        let now = Utc::now();
        store
            .append(LiquidationAuctionRecord {
                auction_id: "auction-1".to_string(),
                queue_id: "queue-1".to_string(),
                status: "open".to_string(),
                market_id: "BTC-USD-PERP".to_string(),
                outcome: 0,
                liquidated_user_id: "user-a".to_string(),
                reserve_price: 99_000,
                mark_price: 100_000,
                round: 0,
                target_position_qty: 10,
                filled_position_qty: 0,
                opened_at: now,
                expires_at: now + chrono::Duration::seconds(30),
                best_bid_price: None,
                best_bidder_user_id: None,
                bids: Vec::new(),
                winner_user_id: None,
                error: None,
                recorded_at: now,
            })
            .expect("seed auction");

        let first = store
            .submit_bid("queue-1", "mm-1", 100_100, 5, now)
            .expect("first bid");
        let second = store
            .submit_bid(
                "queue-1",
                "mm-2",
                100_250,
                7,
                now + chrono::Duration::milliseconds(1),
            )
            .expect("second bid");

        assert_eq!(first.bids.len(), 1);
        assert_eq!(second.bids.len(), 2);
        assert_eq!(second.best_bid_price, Some(100_250));
        assert_eq!(second.best_bidder_user_id.as_deref(), Some("mm-2"));
    }
}
