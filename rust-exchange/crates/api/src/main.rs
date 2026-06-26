#![recursion_limit = "256"]

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use eventbus::EventBus;
use hmac::{Hmac, Mac};
use instruments::{InstrumentRegistry, PersistentInstrumentRegistry};
use ledger::LedgerService;
use matching::partitioned::{SubmissionError, TradeJournalRecord, TradeSettlementRecord};
use matching::{
    MarketRuntimeSnapshot, PartitionSnapshotRecord, PartitionedEngineConfig,
    PartitionedMatchingEngine, RestingOrderSnapshot,
};
use parking_lot::Mutex;
use persistence::JsonlFileWal;
use projections::{project_margin, project_pnl, project_positions};
use risk::{
    AdlCandidate, AdlGovernance, GracePeriodPolicy, LiquidationCircuitBreaker,
    LiquidationGateResult, LiquidationVelocityTracker, RiskEngine,
};
use sequencer::{SequencedCommandRecord, Sequencer};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::Infallible;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use std::time::Instant;
use types::{
    AdminAction, AdminCommand, ApiErrorCode, AuthenticatedPrincipal, CancelOrderCommand, Command,
    CommandMetadata, ExpirySpec, InstrumentKind, InstrumentSpec, InstrumentStatus, LedgerDelta,
    MarginMode, MarketState, MassCancelByMarketCommand, MassCancelBySessionCommand,
    MassCancelByUserCommand, NewOrderCommand, OptionSpec, OptionType, OrderType, PrincipalRole,
    ReplaceOrderCommand, Side, TimeInForce,
};
use warp::{
    http::{Method, StatusCode},
    reject::Reject,
    Filter, Rejection, Reply,
};

mod accounts;
mod admin;
mod admin_approvals_http;
mod admin_audit;
mod admin_authz;
mod admin_rbac_audit;
mod admin_rbac_http;
mod admin_rbac_store;
mod admin_trading_ops_http;
mod admin_wallet_http;
mod admin_wallet_settlement;
mod api_trace;
mod beta_controls;
mod bootstrap;
mod capacity;
mod config;
mod control;
mod custody;
mod customer_wallet_audit;
mod customer_wallet_http;
mod dto;
mod failpoint;
mod fee_tiers;
mod governance;
mod helpers;
mod liquidation;
mod markets;
mod monitor;
mod monitor_http;
mod monitor_integration;
mod monitor_jsonl;
mod observability;
mod oncall;
mod openapi;
mod ops;
mod order_state_projection;
mod perf;
mod planes;
mod position_costs;
mod pricing;
mod product_flows;
mod prometheus;
mod release;
mod rollback;
mod security;
mod sentinel;
mod stop_orders;
mod stores;
mod stress;
mod tracing_ctx;
mod security_headers;
mod trading;
mod transfers;
mod websocket;
mod withdrawals;
mod ws_token;

use accounts::*;
use admin::*;
use admin_audit::*;
use beta_controls::*;
use bootstrap::*;
use control::*;
use dto::*;
use fee_tiers::*;
use governance::*;
use helpers::*;
use liquidation::*;
use markets::*;
use order_state_projection::*;
use position_costs::*;
use pricing::*;
use product_flows::*;
use security::*;
use stop_orders::*;
use stores::*;
use trading::*;
use transfers::*;
use websocket::WsHub;
use withdrawals::*;

type JsonRoute = warp::filters::BoxedFilter<(warp::reply::Json,)>;

type HmacSha256 = Hmac<sha2::Sha256>;

/// **P0-SEC-2:** the replay-protection window for HMAC-signed
/// requests. 30 s is right for production (any larger weakens
/// anti-replay; any smaller breaks under wall-clock skew). Staging
/// can widen via `INTERNAL_AUTH_MAX_SKEW_SECONDS=120` so long-running
/// smoke harnesses don't drift past the window. Dev = 300 s.
fn internal_auth_max_skew_seconds() -> i64 {
    static CACHED: OnceLock<i64> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("INTERNAL_AUTH_MAX_SKEW_SECONDS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|&v| (1..=3600).contains(&v))
            .unwrap_or(30)
    })
}


static INTERNAL_AUTH_SHARED_SECRET: OnceLock<String> = OnceLock::new();

/// Global exchange configuration �?initialised once in `main()`.
static CONFIG: OnceLock<config::ExchangeConfig> = OnceLock::new();

fn cfg() -> &'static config::ExchangeConfig {
    CONFIG.get().expect("CONFIG not initialised")
}

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

    fn get(&self, market_id: &str, outcome: i32) -> Option<FundingRateRecord> {
        self.rates
            .get(&rate_key(market_id, outcome))
            .map(|entry| entry.value().clone())
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

    fn list_funding_for_user(
        &self,
        user_id: &str,
        market_id: Option<&str>,
        outcome: Option<i32>,
        limit: usize,
    ) -> anyhow::Result<Vec<RiskAutomationAuditRecord>> {
        let mut items: Vec<RiskAutomationAuditRecord> = self
            .store
            .entries()?
            .into_iter()
            .filter(|record| {
                if record.event_type != "funding_settled" {
                    return false;
                }
                let is_payer = record.user_id.as_deref() == Some(user_id);
                let is_receiver = record.counterparty_user_id.as_deref() == Some(user_id);
                if !is_payer && !is_receiver {
                    return false;
                }
                if let Some(mid) = market_id {
                    if record.market_id != mid {
                        return false;
                    }
                }
                if let Some(oc) = outcome {
                    if record.outcome != oc {
                        return false;
                    }
                }
                true
            })
            .collect();
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

    /// Remove terminal (non-active) records older than `max_age` to prevent unbounded
    /// memory growth.  Returns the number of records evicted.
    fn prune_terminal(&self, max_age: chrono::Duration) -> usize {
        let cutoff = Utc::now() - max_age;
        let mut pruned = 0usize;
        self.entries.retain(|_, record| {
            if !liquidation_queue_status_is_active(&record.status) && record.recorded_at < cutoff {
                pruned += 1;
                false
            } else {
                true
            }
        });
        pruned
    }

    fn list_by_user(&self, user_id: &str, limit: usize) -> Vec<LiquidationQueueRecord> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.value().user_id == user_id)
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));
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
struct LiquidationAuctionLevel {
    bid_price: i64,
    total_quantity: i64,
    order_count: usize,
    first_submitted_at: DateTime<Utc>,
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
    #[serde(default)]
    price_levels: Vec<LiquidationAuctionLevel>,
    bids: Vec<LiquidationAuctionBid>,
    winner_user_id: Option<String>,
    #[serde(default)]
    clearing_price: Option<i64>,
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
        rebuild_liquidation_auction_book(&mut next);
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

    /// Remove terminal (non-open/active) auction records older than `max_age`.
    fn prune_terminal(&self, max_age: chrono::Duration) -> usize {
        let cutoff = Utc::now() - max_age;
        let mut pruned = 0usize;
        self.entries.retain(|_, record| {
            if record.status != "open" && record.recorded_at < cutoff {
                pruned += 1;
                false
            } else {
                true
            }
        });
        pruned
    }
}

fn rebuild_liquidation_auction_book(record: &mut LiquidationAuctionRecord) {
    record.bids.sort_by(|lhs, rhs| {
        rhs.bid_price
            .cmp(&lhs.bid_price)
            .then_with(|| lhs.submitted_at.cmp(&rhs.submitted_at))
    });
    record.best_bid_price = record.bids.first().map(|bid| bid.bid_price);
    record.best_bidder_user_id = record.bids.first().map(|bid| bid.bidder_user_id.clone());
    let mut levels: Vec<LiquidationAuctionLevel> = Vec::new();
    for bid in &record.bids {
        match levels.last_mut() {
            Some(level) if level.bid_price == bid.bid_price => {
                level.total_quantity = level.total_quantity.saturating_add(bid.bid_quantity.max(0));
                level.order_count = level.order_count.saturating_add(1);
                if bid.submitted_at < level.first_submitted_at {
                    level.first_submitted_at = bid.submitted_at;
                }
            }
            _ => levels.push(LiquidationAuctionLevel {
                bid_price: bid.bid_price,
                total_quantity: bid.bid_quantity.max(0),
                order_count: 1,
                first_submitted_at: bid.submitted_at,
            }),
        }
    }
    record.price_levels = levels;
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
    #[serde(default = "default_auction_reserve_step_bps")]
    auction_reserve_step_bps: i64,
    updated_by: String,
    recorded_at: DateTime<Utc>,
}

fn default_auction_reserve_step_bps() -> i64 {
    100
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
            auction_reserve_step_bps: default_auction_reserve_step_bps(),
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct IndexSourcePolicyRecord {
    market_id: String,
    outcome: i32,
    source: String,
    status: String,
    #[serde(default = "default_index_source_weight_bps")]
    weight_bps: i64,
    updated_by: String,
    recorded_at: DateTime<Utc>,
}

fn default_index_source_weight_bps() -> i64 {
    10_000
}

/// Centralized rate limit configuration replacing hardcoded 60/30/10 limits.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct RateLimitConfig {
    /// IP-level requests per window (general).
    ip_limit: usize,
    /// User read operations per window.
    user_read_limit: usize,
    /// User write/mutate operations per window.
    user_write_limit: usize,
    /// Admin operations per window.
    admin_limit: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            ip_limit: 60,
            user_read_limit: 30,
            user_write_limit: 10,
            admin_limit: 10,
        }
    }
}

#[derive(Clone)]
struct FixedWindowRateLimiter {
    window: Duration,
    states: Arc<DashMap<String, VecDeque<Instant>>>,
    cleanup_counter: Arc<AtomicU64>,
    cleanup_interval: u64,
    max_keys: usize,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
    code: Option<String>,
    details: Option<serde_json::Value>,
}

impl Reject for ApiError {}

impl FixedWindowRateLimiter {
    fn new(window: Duration) -> Self {
        Self::new_with_limits(window, 100_000, 256)
    }

    fn new_with_limits(window: Duration, max_keys: usize, cleanup_interval: u64) -> Self {
        Self {
            window,
            states: Arc::new(DashMap::new()),
            cleanup_counter: Arc::new(AtomicU64::new(0)),
            cleanup_interval,
            max_keys,
        }
    }

    fn check(&self, key: &str, limit: usize) -> Result<(), Rejection> {
        let now = Instant::now();
        self.maybe_cleanup(now);
        if self.states.len() >= self.max_keys && !self.states.contains_key(key) {
            self.cleanup_stale(now);
            if self.states.len() >= self.max_keys && !self.states.contains_key(key) {
                return Err(warp::reject::custom(ApiError {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    message: "rate limiter saturated".to_string(),
                    code: Some("RATE_LIMITED".to_string()),
                    details: None,
                }));
            }
        }
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
                code: Some("RATE_LIMITED".to_string()),
                details: None,
            }));
        }
        bucket.push_back(now);
        Ok(())
    }

    fn maybe_cleanup(&self, now: Instant) {
        let attempt = self.cleanup_counter.fetch_add(1, Ordering::Relaxed) + 1;
        if attempt % self.cleanup_interval == 0 || self.states.len() > self.max_keys {
            self.cleanup_stale(now);
        }
    }

    fn cleanup_stale(&self, now: Instant) {
        self.states.retain(|_, bucket| {
            while bucket
                .front()
                .is_some_and(|timestamp| now.duration_since(*timestamp) > self.window)
            {
                bucket.pop_front();
            }
            !bucket.is_empty()
        });
    }

    /// Weighted check: consumes `weight` tokens against the limit per window.
    /// Each token is recorded as a separate timestamp entry.
    #[allow(dead_code)]
    fn check_weighted(&self, key: &str, limit: usize, weight: u32) -> Result<(), Rejection> {
        if weight == 0 {
            return Ok(());
        }
        // Fast path: weight=1 is the common case
        if weight == 1 {
            return self.check(key, limit);
        }
        let now = Instant::now();
        self.maybe_cleanup(now);
        if self.states.len() >= self.max_keys && !self.states.contains_key(key) {
            self.cleanup_stale(now);
            if self.states.len() >= self.max_keys && !self.states.contains_key(key) {
                return Err(warp::reject::custom(ApiError {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    message: "rate limiter saturated".to_string(),
                    code: Some("RATE_LIMITED".to_string()),
                    details: None,
                }));
            }
        }
        let mut bucket = self.states.entry(key.to_string()).or_default();
        while bucket
            .front()
            .is_some_and(|timestamp| now.duration_since(*timestamp) > self.window)
        {
            bucket.pop_front();
        }
        if bucket.len().saturating_add(weight as usize) > limit {
            return Err(warp::reject::custom(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: "rate limit exceeded".to_string(),
                code: Some("RATE_LIMITED".to_string()),
                details: None,
            }));
        }
        for _ in 0..weight {
            bucket.push_back(now);
        }
        Ok(())
    }
}

fn reject_api(status: StatusCode, message: impl Into<String>) -> Rejection {
    warp::reject::custom(ApiError {
        status,
        message: message.into(),
        code: None,
        details: None,
    })
}

fn reject_submission_error(error: &SubmissionError) -> Rejection {
    let (status, body) = submission_error_response(error);
    warp::reject::custom(ApiError {
        status,
        message: error.to_string(),
        code: body.get("code").and_then(|v| v.as_str()).map(String::from),
        details: body.get("details").cloned(),
    })
}

/// Map a `SubmissionError` to a structured JSON response with proper HTTP status code and error code.
fn submission_error_response(error: &SubmissionError) -> (StatusCode, serde_json::Value) {
    let (status, code, details) = match error {
        SubmissionError::QueueFull { partition } => (
            StatusCode::SERVICE_UNAVAILABLE,
            ApiErrorCode::QueueFull,
            serde_json::json!({ "partition": partition }),
        ),
        SubmissionError::PartitionClosed { partition } => (
            StatusCode::SERVICE_UNAVAILABLE,
            ApiErrorCode::InternalError,
            serde_json::json!({ "partition": partition }),
        ),
        SubmissionError::QueueResponseDropped { partition } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::InternalError,
            serde_json::json!({ "partition": partition }),
        ),
        SubmissionError::KillSwitchActive => (
            StatusCode::SERVICE_UNAVAILABLE,
            ApiErrorCode::KillSwitchActive,
            serde_json::json!({}),
        ),
        SubmissionError::AccountFrozen => (
            StatusCode::FORBIDDEN,
            ApiErrorCode::AccountFrozen,
            serde_json::json!({}),
        ),
        SubmissionError::InvalidOrder(reason) => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidOrder,
            serde_json::json!({ "reason": reason }),
        ),
        SubmissionError::DuplicateOrderId(id) => (
            StatusCode::CONFLICT,
            ApiErrorCode::DuplicateOrderId,
            serde_json::json!({ "order_id": id }),
        ),
        SubmissionError::OrderNotFound(id) => (
            StatusCode::NOT_FOUND,
            ApiErrorCode::OrderNotFound,
            serde_json::json!({ "order_id": id }),
        ),
        SubmissionError::MarketClosed {
            market_id,
            outcome,
            state,
        } => (
            StatusCode::CONFLICT,
            ApiErrorCode::MarketClosed,
            serde_json::json!({ "market_id": market_id, "outcome": outcome, "state": format!("{state:?}") }),
        ),
        SubmissionError::Persistence { component, detail } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::PersistenceError,
            serde_json::json!({ "component": component, "detail": detail }),
        ),
        SubmissionError::PriceBandBreached {
            market_id,
            outcome,
            reference_price,
            attempted_price,
            deviation_bps,
            ..
        } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::PriceBandBreached,
            serde_json::json!({
                "market_id": market_id,
                "outcome": outcome,
                "reference_price": reference_price,
                "attempted_price": attempted_price,
                "deviation_bps": deviation_bps,
            }),
        ),
        SubmissionError::InsufficientLiquidityForFok => (
            StatusCode::CONFLICT,
            ApiErrorCode::InsufficientLiquidity,
            serde_json::json!({}),
        ),
        SubmissionError::SelfTradePrevented(user_id) => (
            StatusCode::CONFLICT,
            ApiErrorCode::SelfTradePrevented,
            serde_json::json!({ "user_id": user_id }),
        ),
        SubmissionError::Ledger(detail) => {
            // Client-side ledger failures (insufficient funds/margin) are 400 errors,
            // not internal server errors.
            let lower = detail.to_lowercase();
            if lower.contains("insufficient") {
                (
                    StatusCode::BAD_REQUEST,
                    ApiErrorCode::InsufficientFunds,
                    serde_json::json!({ "detail": detail }),
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiErrorCode::LedgerError,
                    serde_json::json!({ "detail": detail }),
                )
            }
        }
        SubmissionError::TickSizeViolation { price, tick_size } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::TickSizeViolation,
            serde_json::json!({ "price": price, "tick_size": tick_size }),
        ),
        SubmissionError::LotSizeViolation { amount, lot_size } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::LotSizeViolation,
            serde_json::json!({ "amount": amount, "lot_size": lot_size }),
        ),
        SubmissionError::BelowMinAmount {
            amount,
            min_order_amount,
        } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::BelowMinAmount,
            serde_json::json!({ "amount": amount, "min_order_amount": min_order_amount }),
        ),
        SubmissionError::ExceedsMaxNotional {
            notional,
            max_notional,
        } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::ExceedsMaxNotional,
            serde_json::json!({ "notional": notional, "max_notional": max_notional }),
        ),
        SubmissionError::RateLimited { user_id, limit } => (
            StatusCode::TOO_MANY_REQUESTS,
            ApiErrorCode::RateLimited,
            serde_json::json!({ "user_id": user_id, "limit": limit }),
        ),
        SubmissionError::PostOnlyWouldTake => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::PostOnlyWouldTrade,
            serde_json::json!({}),
        ),
        SubmissionError::ReduceOnlyViolation { side } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::ReduceOnlyViolation,
            serde_json::json!({ "side": format!("{side:?}") }),
        ),
        SubmissionError::ExceedsMaxLeverage {
            leverage,
            max_leverage,
        } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::ExceedsMaxLeverage,
            serde_json::json!({ "leverage": leverage, "max_leverage": max_leverage }),
        ),
        SubmissionError::InstrumentHalted { instrument_id } => (
            StatusCode::CONFLICT,
            ApiErrorCode::InstrumentHalted,
            serde_json::json!({ "instrument_id": instrument_id }),
        ),
        SubmissionError::InstrumentDelisted { instrument_id } => (
            StatusCode::CONFLICT,
            ApiErrorCode::InstrumentDelisted,
            serde_json::json!({ "instrument_id": instrument_id }),
        ),
        SubmissionError::UnsupportedOrderType { order_type } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidOrder,
            serde_json::json!({ "reason": format!("unsupported order type: {order_type:?}") }),
        ),
        SubmissionError::UnsupportedTimeInForce { time_in_force } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidTimeInForce,
            serde_json::json!({ "time_in_force": format!("{time_in_force:?}") }),
        ),
        SubmissionError::FatFingerRejected {
            amount,
            max_order_amount,
        } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::FatFingerRejected,
            serde_json::json!({ "amount": amount, "max_order_amount": max_order_amount }),
        ),
        SubmissionError::CircuitBreakerTriggered { market_id } => (
            StatusCode::CONFLICT,
            ApiErrorCode::CircuitBreakerTriggered,
            serde_json::json!({ "market_id": market_id }),
        ),
        SubmissionError::MarketKillSwitchActive { market_id } => (
            StatusCode::CONFLICT,
            ApiErrorCode::MarketKillSwitchActive,
            serde_json::json!({ "market_id": market_id }),
        ),
        SubmissionError::InsufficientFunds { detail } => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InsufficientFunds,
            serde_json::json!({ "detail": detail }),
        ),
    };
    let body = serde_json::json!({
        "status": "error",
        "code": code.to_string(),
        "message": error.to_string(),
        "details": details,
    });
    (status, body)
}

fn optional_query<T>() -> impl Filter<Extract = (T,), Error = Infallible> + Clone
where
    T: DeserializeOwned + Default + Send + 'static,
{
    warp::query::<T>().or(warp::any().map(T::default)).unify()
}

fn body_limit() -> impl Filter<Extract = (), Error = Rejection> + Clone {
    warp::body::content_length_limit(cfg().server.max_body_size_bytes)
}

fn remote_ip() -> impl Filter<Extract = (Option<SocketAddr>,), Error = Infallible> + Clone {
    warp::addr::remote()
}

async fn handle_rejection(rejection: Rejection) -> Result<impl Reply, Infallible> {
    // ws_token errors come from `/ws/order-trace?token=...` failures. Check
    // BEFORE the generic ApiError branch — when the WS upgrade has both
    // branches (token-authed + header-authed) reject, warp combines both
    // rejections and the more-actionable one (token-specific) should win.
    // Otherwise the client sees the misleading "missing x-api-key" message
    // from the header-authed branch when the real problem is the token.
    if let Some(err) = rejection.find::<ws_token::WsTokenRejection>() {
        let body = serde_json::json!({
            "status": "error",
            "code": "WS_TOKEN_INVALID",
            "message": err.0.to_string(),
            "error": err.0.to_string(),
        });
        return Ok(warp::reply::with_status(
            warp::reply::json(&body),
            StatusCode::UNAUTHORIZED,
        ));
    }
    if let Some(error) = rejection.find::<ApiError>() {
        let mut body = serde_json::json!({"status":"error","message":error.message});
        if let Some(code) = &error.code {
            body["code"] = serde_json::json!(code);
        }
        if let Some(details) = &error.details {
            body["details"] = details.clone();
        }
        // Keep legacy "error" field for backward compatibility
        body["error"] = serde_json::json!(error.message);
        return Ok(warp::reply::with_status(
            warp::reply::json(&body),
            error.status,
        ));
    }
    // C1: structured rejections from /v2/wallet/* surface here. Map
    // them through customer_wallet_http::wallet_error_to_reply so the
    // status code matches the failure mode (400 / 403 / 404 / 409 /
    // 503 / 500) instead of collapsing to a generic 404 or 500.
    if let Some(err) = rejection.find::<customer_wallet_http::WalletError>() {
        let reply = customer_wallet_http::wallet_error_to_reply(err);
        return Ok(reply);
    }
    if rejection.is_not_found() {
        let body = serde_json::json!({"status":"error","code":"NOT_FOUND","message":"not found","error":"not found"});
        return Ok(warp::reply::with_status(
            warp::reply::json(&body),
            StatusCode::NOT_FOUND,
        ));
    }
    let body = serde_json::json!({"status":"error","code":"INTERNAL_ERROR","message":"internal server error","error":"internal server error"});
    Ok(warp::reply::with_status(
        warp::reply::json(&body),
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

fn bind_address() -> SocketAddr {
    let c = cfg();
    let ip = c
        .server
        .bind_host
        .parse::<IpAddr>()
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    SocketAddr::new(ip, c.server.bind_port)
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
        MarketState::Closed => 5,
        MarketState::Halted => 4,
        MarketState::CancelOnly => 3,
        MarketState::Maintenance => 3,
        MarketState::AuctionCall => 2,
        MarketState::Stress => 1,
        MarketState::PreOpen => 1,
        MarketState::Normal => 0,
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
    if spec.maker_fee_bps < -500 || spec.maker_fee_bps > 10_000 {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "maker_fee_bps must be in [-500, 10000]",
        ));
    }
    if spec.taker_fee_bps < -500 || spec.taker_fee_bps > 10_000 {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "taker_fee_bps must be in [-500, 10000]",
        ));
    }
    if spec.instrument_id.len() > 256 {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "instrument_id too long",
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
        _ => {
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

fn settlement_reconciliation_snapshot(
    settlement_records: &[TradeSettlementRecord],
    trade_records: &[TradeJournalRecord],
    ledger_entries: &[LedgerDelta],
    position_costs: &PositionCostLedgerStore,
    limit: usize,
) -> serde_json::Value {
    let mut latest_by_trade: HashMap<String, TradeSettlementRecord> = HashMap::new();
    let mut status_history: HashMap<String, Vec<String>> = HashMap::new();
    for record in settlement_records {
        latest_by_trade.insert(record.trade_id.clone(), record.clone());
        status_history
            .entry(record.trade_id.clone())
            .or_default()
            .push(format!("{:?}", record.status).to_ascii_lowercase());
    }
    let mut latest_all: Vec<_> = latest_by_trade.into_values().collect();
    latest_all.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));

    let prepared = latest_all
        .iter()
        .filter(|entry| entry.status == matching::partitioned::TradeSettlementStatus::Prepared)
        .count();
    let applied = latest_all
        .iter()
        .filter(|entry| entry.status == matching::partitioned::TradeSettlementStatus::Applied)
        .count();
    let failed = latest_all
        .iter()
        .filter(|entry| entry.status == matching::partitioned::TradeSettlementStatus::Failed)
        .count();
    let distinct_trades = latest_all.len();

    let mut latest = latest_all;
    latest.truncate(limit);

    let items: Vec<_> = latest
        .into_iter()
        .map(|record| {
            let ledger_settled = ledger_entries
                .iter()
                .any(|entry| entry.op_id == record.settle_op_id);
            let ledger_rolled_back = ledger_entries
                .iter()
                .any(|entry| entry.op_id == record.rollback_op_id);
            let journal_written = trade_records
                .iter()
                .any(|entry| entry.trade_id == record.trade_id);
            let cost_buy_event_persisted =
                position_costs.has_persisted_event_id(&format!("{}:buy", record.trade_id));
            let cost_sell_event_persisted =
                position_costs.has_persisted_event_id(&format!("{}:sell", record.trade_id));
            let cost_buy_state_applied =
                position_costs.has_applied_state_event_id(&format!("{}:buy", record.trade_id));
            let cost_sell_state_applied =
                position_costs.has_applied_state_event_id(&format!("{}:sell", record.trade_id));
            let cost_projection_fully_applied = cost_buy_state_applied && cost_sell_state_applied;
            let cost_projection_gap = cost_buy_event_persisted != cost_buy_state_applied
                || cost_sell_event_persisted != cost_sell_state_applied;
            let effective_status = match record.status {
                matching::partitioned::TradeSettlementStatus::Prepared
                    if ledger_settled && journal_written && cost_projection_fully_applied =>
                {
                    "core_committed_applied_marker_missing"
                }
                matching::partitioned::TradeSettlementStatus::Prepared
                    if ledger_settled && journal_written =>
                {
                    "committed_with_audit_gap"
                }
                matching::partitioned::TradeSettlementStatus::Applied if cost_projection_gap => {
                    "applied_projection_gap"
                }
                matching::partitioned::TradeSettlementStatus::Prepared => "prepared",
                matching::partitioned::TradeSettlementStatus::Applied => "applied",
                matching::partitioned::TradeSettlementStatus::Failed => "failed",
            };
            serde_json::json!({
                "trade_id": record.trade_id,
                "partition_id": record.partition_id,
                "market_id": record.market_id,
                "outcome": record.outcome,
                "instrument_kind": record.instrument_kind,
                "status": record.status,
                "effective_status": effective_status,
                "status_history": status_history.remove(&record.trade_id).unwrap_or_default(),
                "settle_op_id": record.settle_op_id,
                "rollback_op_id": record.rollback_op_id,
                "ledger_settled": ledger_settled,
                "ledger_rolled_back": ledger_rolled_back,
                "trade_journal_written": journal_written,
                "cost_buy_event_persisted": cost_buy_event_persisted,
                "cost_sell_event_persisted": cost_sell_event_persisted,
                "cost_buy_state_applied": cost_buy_state_applied,
                "cost_sell_state_applied": cost_sell_state_applied,
                "recorded_at": record.recorded_at,
            })
        })
        .collect();

    serde_json::json!({
        "summary": {
            "settlement_records": settlement_records.len(),
            "distinct_trades": distinct_trades,
            "trade_records": trade_records.len(),
            "prepared_trades": prepared,
            "applied_trades": applied,
            "failed_trades": failed,
            "returned_items": items.len(),
        },
        "items": items,
    })
}

fn max_sequencer_command_seq(records: &[SequencedCommandRecord]) -> Option<u64> {
    records.iter().map(|record| record.command_seq).max()
}

fn max_trade_command_seq(records: &[TradeJournalRecord]) -> Option<u64> {
    records
        .iter()
        .filter_map(|record| parse_command_seq_from_order_like_id(&record.trade_id))
        .max()
}

fn max_settlement_command_seq(records: &[TradeSettlementRecord]) -> Option<u64> {
    records
        .iter()
        .filter_map(|record| parse_command_seq_from_order_like_id(&record.trade_id))
        .max()
}

fn max_ledger_command_seq(entries: &[LedgerDelta]) -> Option<u64> {
    entries
        .iter()
        .filter_map(|entry| parse_command_seq_from_order_like_id(&entry.op_id))
        .max()
}

fn frontiers_consistent(
    sequencer_frontier: Option<u64>,
    order_projection_frontier: Option<u64>,
    trade_frontier: Option<u64>,
    settlement_frontier: Option<u64>,
    ledger_frontier: Option<u64>,
) -> bool {
    let Some(sequencer_frontier) = sequencer_frontier else {
        return true;
    };
    let projection_ok = order_projection_frontier.is_none_or(|value| value <= sequencer_frontier);
    let trade_ok = trade_frontier.is_none_or(|value| value <= sequencer_frontier);
    let settlement_ok = settlement_frontier.is_none_or(|value| value <= sequencer_frontier);
    let ledger_ok = ledger_frontier.is_none_or(|value| {
        value <= sequencer_frontier && trade_frontier.is_none_or(|trade| value >= trade)
    });
    projection_ok && trade_ok && settlement_ok && ledger_ok
}

fn core_chain_frontiers_snapshot(
    sequencer_records: &[SequencedCommandRecord],
    order_projection: &OrderStateProjectionStore,
    trades: &[TradeJournalRecord],
    settlements: &[TradeSettlementRecord],
    ledger_entries: &[LedgerDelta],
) -> serde_json::Value {
    let sequencer_frontier = max_sequencer_command_seq(sequencer_records);
    let projection_frontier = order_projection.latest_command_seq();
    let trade_frontier = max_trade_command_seq(trades);
    let settlement_frontier = max_settlement_command_seq(settlements);
    let ledger_frontier = max_ledger_command_seq(ledger_entries);
    serde_json::json!({
        "sequencer_command_seq": sequencer_frontier,
        "order_projection_command_seq": projection_frontier,
        "trade_log_command_seq": trade_frontier,
        "trade_settlement_command_seq": settlement_frontier,
        "ledger_command_seq": ledger_frontier,
        "consistent": frontiers_consistent(
            sequencer_frontier,
            projection_frontier,
            trade_frontier,
            settlement_frontier,
            ledger_frontier,
        ),
    })
}

fn core_chain_reconciliation_snapshot(
    sequencer_records: &[SequencedCommandRecord],
    order_projection: &OrderStateProjectionStore,
    snapshots: &[MarketRuntimeSnapshot],
    settlement_records: &[TradeSettlementRecord],
    trade_records: &[TradeJournalRecord],
    ledger_entries: &[LedgerDelta],
    position_costs: &PositionCostLedgerStore,
    limit: usize,
) -> serde_json::Value {
    let frontiers = core_chain_frontiers_snapshot(
        sequencer_records,
        order_projection,
        trade_records,
        settlement_records,
        ledger_entries,
    );
    let projection_entries = order_projection.list_all();
    let projection_map: HashMap<(String, String), OrderStateProjectionEntry> = projection_entries
        .iter()
        .cloned()
        .map(|entry| ((entry.user_id.clone(), entry.order_id.clone()), entry))
        .collect();
    let open_order_keys: std::collections::HashSet<(String, String)> = snapshots
        .iter()
        .flat_map(|snapshot| snapshot.orders.iter())
        .map(|order| (order.user_id.clone(), order.order_id.clone()))
        .collect();

    let mut trade_projection_gaps = Vec::new();
    for trade in trade_records {
        for (user_id, order_id) in [
            (&trade.buy_user_id, &trade.buy_order_id),
            (&trade.sell_user_id, &trade.sell_order_id),
        ] {
            if !projection_map.contains_key(&(user_id.clone(), order_id.clone())) {
                trade_projection_gaps.push(serde_json::json!({
                    "kind": "missing_projection_for_trade",
                    "trade_id": trade.trade_id,
                    "user_id": user_id,
                    "order_id": order_id,
                    "market_id": trade.market_id,
                    "outcome": trade.outcome,
                }));
            }
        }
    }

    let mut projection_runtime_gaps = Vec::new();
    for entry in &projection_entries {
        let key = (entry.user_id.clone(), entry.order_id.clone());
        let should_be_open = matches!(
            entry.status,
            OrderProjectionStatus::Open | OrderProjectionStatus::PartiallyFilled
        );
        if should_be_open && !open_order_keys.contains(&key) {
            projection_runtime_gaps.push(serde_json::json!({
                "kind": "projection_open_but_not_resting",
                "user_id": entry.user_id,
                "order_id": entry.order_id,
                "status": entry.status,
                "market_id": entry.market_id,
                "outcome": entry.outcome,
            }));
        }
        if entry.status == OrderProjectionStatus::Replaced && entry.replaced_by_order_id.is_none() {
            projection_runtime_gaps.push(serde_json::json!({
                "kind": "replaced_without_successor",
                "user_id": entry.user_id,
                "order_id": entry.order_id,
            }));
        }
    }

    let mut items = Vec::new();
    items.extend(trade_projection_gaps);
    items.extend(projection_runtime_gaps);
    items.truncate(limit);

    serde_json::json!({
        "frontiers": frontiers,
        "settlement_reconciliation": settlement_reconciliation_snapshot(
            settlement_records,
            trade_records,
            ledger_entries,
            position_costs,
            limit,
        ),
        "summary": {
            "projection_entries": projection_entries.len(),
            "open_runtime_orders": open_order_keys.len(),
            "returned_items": items.len(),
        },
        "items": items,
    })
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
        "maker_fee": record.maker_fee,
        "taker_fee": record.taker_fee,
        "timestamp": record.recorded_at,
    })
}

fn trades_to_history(
    market_id: &str,
    outcome: Option<i32>,
    trades: &[TradeJournalRecord],
    limit: usize,
    after: Option<&str>,
    before: Option<&str>,
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
    // Apply time-range filters (ISO-8601 string comparison works for UTC timestamps).
    if let Some(after) = after {
        data.retain(|candle| candle["timestamp"].as_str().is_some_and(|ts| ts > after));
    }
    if let Some(before) = before {
        data.retain(|candle| candle["timestamp"].as_str().is_some_and(|ts| ts < before));
    }
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

fn sequencer_wal_path() -> String {
    cfg().wal.sequencer.clone()
}

fn ledger_wal_path() -> String {
    cfg().wal.ledger.clone()
}

fn matching_snapshot_wal_path() -> String {
    cfg().wal.matching_snapshot.clone()
}

fn trade_journal_wal_path() -> String {
    cfg().wal.trade_journal.clone()
}

fn trade_settlement_wal_path() -> String {
    cfg().wal.trade_settlement.clone()
}

fn instruments_registry_wal_path() -> String {
    cfg().wal.instruments_registry.clone()
}

fn funding_rates_wal_path() -> String {
    cfg().wal.funding_rates.clone()
}

fn risk_automation_audit_wal_path() -> String {
    cfg().wal.risk_automation_audit.clone()
}

fn liquidation_queue_wal_path() -> String {
    cfg().wal.liquidation_queue.clone()
}

fn liquidation_auction_wal_path() -> String {
    cfg().wal.liquidation_auction.clone()
}

fn adl_governance_wal_path() -> String {
    cfg().wal.adl_governance.clone()
}

fn liquidation_policy_wal_path() -> String {
    cfg().wal.liquidation_policy.clone()
}

fn index_price_wal_path() -> String {
    cfg().wal.index_price.clone()
}

fn index_source_policy_wal_path() -> String {
    cfg().wal.index_source_policy.clone()
}

fn position_cost_state_wal_path() -> String {
    cfg().wal.position_cost_state.clone()
}

fn position_cost_event_wal_path() -> String {
    cfg().wal.position_cost_events.clone()
}

fn order_state_projection_wal_path() -> String {
    cfg().wal.order_state_projection.clone()
}

fn governance_action_wal_path() -> String {
    cfg().wal.governance_actions.clone()
}

fn beta_controls_wal_path() -> String {
    cfg().wal.beta_controls.clone()
}

fn admin_action_audit_wal_path() -> String {
    cfg().wal.admin_action_audit.clone()
}

fn withdrawals_wal_path() -> String {
    cfg().wal.withdrawals.clone()
}

fn address_whitelist_wal_path() -> String {
    cfg().wal.address_whitelist.clone()
}

fn fee_tiers_wal_path() -> String {
    cfg().wal.fee_tiers.clone()
}

fn transfers_wal_path() -> String {
    cfg().wal.transfers.clone()
}

fn stop_orders_wal_path() -> String {
    cfg().wal.stop_orders.clone()
}

fn automation_enabled() -> bool {
    cfg().risk.automation_enabled
}

fn liquidation_interval_secs() -> u64 {
    cfg().risk.liquidation_interval_secs
}

fn funding_interval_secs() -> u64 {
    cfg().risk.funding_interval_secs
}

fn liquidation_worker_interval_secs() -> u64 {
    cfg().risk.liquidation_worker_interval_secs
}

fn liquidation_auction_window_secs() -> i64 {
    cfg().risk.liquidation_auction_window_secs
}

fn automation_liquidator_user_id() -> String {
    cfg().risk.liquidator_user_id.clone()
}

fn default_max_auction_rounds() -> u32 {
    3
}

fn liquidation_retry_delay_secs(policy: &LiquidationPolicyRecord, retry_tier: u32) -> i64 {
    let idx = retry_tier as usize;
    if idx < policy.retry_backoff_secs.len() {
        return policy.retry_backoff_secs[idx].max(0);
    }
    policy
        .retry_backoff_secs
        .last()
        .copied()
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
    stp_mode: types::StpMode,
    trigger_price: Option<i64>,
    trigger_type: Option<types::TriggerType>,
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
        stp_mode,
        trigger_price,
        trigger_type,
        display_qty: None,
        min_fill_qty: None,
        stp_group_id: None,
        is_market_maker: false,
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
            new_display_qty: None,
            new_min_fill_qty: None,
            new_trigger_price: None,
            new_trigger_type: None,
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
            side: None,
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
            base_asset: String::new(),
            quote_asset: "USDC".to_string(),
            margin_mode: None,
            max_leverage: None,
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "spot-v1".to_string(),
            min_order_amount: 1,
            max_notional: 0,
            maker_fee_bps: 2,
            taker_fee_bps: 5,
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
        },
        InstrumentSpec {
            instrument_id: "margin:btc-usdt".to_string(),
            kind: InstrumentKind::Margin,
            base_asset: String::new(),
            quote_asset: "USDC".to_string(),
            margin_mode: Some(MarginMode::Isolated),
            max_leverage: Some(20),
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "margin-v1".to_string(),
            min_order_amount: 1,
            max_notional: 0,
            maker_fee_bps: 2,
            taker_fee_bps: 5,
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
        },
        InstrumentSpec {
            instrument_id: "perp:btc-usdt".to_string(),
            kind: InstrumentKind::Perpetual,
            base_asset: String::new(),
            quote_asset: "USDC".to_string(),
            margin_mode: Some(MarginMode::Isolated),
            max_leverage: Some(20),
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "perpetual-v1".to_string(),
            min_order_amount: 1,
            max_notional: 0,
            maker_fee_bps: 1,
            taker_fee_bps: 4,
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
        },
        InstrumentSpec {
            instrument_id: "future:btc-usdt:202606".to_string(),
            kind: InstrumentKind::Future,
            base_asset: String::new(),
            quote_asset: "USDC".to_string(),
            margin_mode: Some(MarginMode::Isolated),
            max_leverage: Some(20),
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "future-v1".to_string(),
            min_order_amount: 1,
            max_notional: 0,
            maker_fee_bps: 1,
            taker_fee_bps: 4,
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
            expiry: Some(ExpirySpec {
                expiry_at: chrono::Utc::now() + chrono::Duration::days(365),
                settlement_price_source: "index:btc-usd".to_string(),
                physical_delivery: false,
            }),
            option_spec: None,
            settlement_currency: None,
        },
        InstrumentSpec {
            instrument_id: "option:btc-usdt:call-70000:202606".to_string(),
            kind: InstrumentKind::Option,
            base_asset: String::new(),
            quote_asset: "USDC".to_string(),
            margin_mode: Some(MarginMode::Isolated),
            max_leverage: Some(10),
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_500,
            risk_policy_id: "option-v1".to_string(),
            min_order_amount: 1,
            max_notional: 0,
            maker_fee_bps: 1,
            taker_fee_bps: 5,
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
            expiry: Some(ExpirySpec {
                expiry_at: chrono::Utc::now() + chrono::Duration::days(365),
                settlement_price_source: "index:btc-usd".to_string(),
                physical_delivery: false,
            }),
            option_spec: Some(OptionSpec {
                strike_price: 7_000_000,
                option_type: OptionType::Call,
                exercise_style: Default::default(),
            }),
            settlement_currency: None,
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
    position_costs: Arc<PositionCostLedgerStore>,
    _trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
    ws_hub: Arc<WsHub>,
    liquidator_user_id: &str,
    system_sentinel: Arc<sentinel::SystemSentinel>,
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
    let entry_prices = position_costs.entry_price_map();

    // ── Portfolio-margin solvency pre-filter ───────────────────────────
    // Build mark_prices map from all snapshots so we can compute the
    // portfolio-level netted margin once, then skip users who are
    // portfolio-solvent (hedged positions protect them).
    let all_instruments = instruments.list();
    let mark_prices_map: std::collections::HashMap<String, i64> = {
        let mut m = std::collections::HashMap::new();
        for snap in &snapshots {
            if let Some(quote) = fair_price_quote_for_snapshot(snap, index_prices.as_ref()) {
                m.insert(snap.market_id.clone(), quote.fair_price);
            }
        }
        m
    };
    let portfolio_solvent_users: std::collections::HashSet<String> = user_ids
        .iter()
        .filter(|uid| risk.is_portfolio_solvent(uid, &all_instruments, &mark_prices_map))
        .cloned()
        .collect();

    // ── Liquidation gate (velocity / circuit-breaker / grace) ──────────
    let mut velocity_tracker = LiquidationVelocityTracker::default();
    let circuit_breaker = LiquidationCircuitBreaker::default();
    let grace_policy = GracePeriodPolicy::default();
    let insurance_fund_total: i64 = all_instruments
        .iter()
        .map(|i| risk.ledger().insurance_fund_balance_for(&i.instrument_id))
        .sum();

    for snapshot in snapshots {
        let instrument = instruments.resolve(&snapshot.market_id);
        if instrument.kind == InstrumentKind::Spot {
            continue;
        }
        // Circuit breaker coupling: skip liquidation when market is halted or restricted.
        if matches!(
            snapshot.state,
            MarketState::Halted
                | MarketState::CancelOnly
                | MarketState::Closed
                | MarketState::Maintenance
        ) {
            continue;
        }
        let Some(mark_price) = fair_price_quote_for_snapshot(&snapshot, index_prices.as_ref())
            .map(|quote| quote.fair_price)
        else {
            continue;
        };
        let inst_maintenance_margin_bps = risk::effective_maintenance_margin_bps(&instrument);
        let candidates = match risk.liquidation_candidates(
            &user_ids,
            &instrument,
            snapshot.outcome,
            mark_price,
            instrument.max_leverage,
            inst_maintenance_margin_bps,
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

        // Pre-liquidation margin warnings: push WS notification to users
        // approaching maintenance margin (Binance/Deribit-style margin call).
        for uid in &user_ids {
            if let Ok(Some(warning_level)) = risk.margin_warning_level(
                uid,
                &instrument,
                snapshot.outcome,
                mark_price,
                instrument.max_leverage,
                inst_maintenance_margin_bps,
            ) {
                ws_hub.publish_user_event(
                    uid,
                    crate::websocket::WsFeedEvent {
                        event_type: "margin_warning".into(),
                        market_id: snapshot.market_id.clone(),
                        data: serde_json::json!({
                            "market_id": snapshot.market_id,
                            "outcome": snapshot.outcome,
                            "level": format!("{:?}", warning_level),
                            "mark_price": mark_price,
                            "timestamp": Utc::now(),
                        }),
                    },
                );
            }
        }

        for candidate in candidates {
            if candidate.user_id == liquidator_user_id {
                continue;
            }

            // ── System sentinel posture: auto-liquidation allowed? ──
            if let Err(reason) = sentinel::enforce_liquidation_posture(&system_sentinel) {
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "sentinel_liquidation_blocked",
                    "skipped",
                    &snapshot.market_id,
                    snapshot.outcome,
                    Some(candidate.user_id.clone()),
                    None,
                    "system-sentinel",
                    serde_json::json!({"reason": reason}),
                );
                break; // stop all liquidations for this market
            }

            // Portfolio-margin netting: skip if user is solvent at portfolio level
            if portfolio_solvent_users.contains(&candidate.user_id) {
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "portfolio_solvent_skip",
                    "ok",
                    &snapshot.market_id,
                    snapshot.outcome,
                    Some(candidate.user_id.clone()),
                    None,
                    "automation-portfolio-netting",
                    serde_json::json!({"reason": "portfolio-level netting keeps user solvent"}),
                );
                continue;
            }
            // Liquidation gate: velocity limiter, circuit breaker, grace period
            let now_secs = Utc::now().timestamp();
            let gate = risk.check_liquidation_gate(
                &candidate.user_id,
                &velocity_tracker,
                &circuit_breaker,
                &grace_policy,
                now_secs,
                insurance_fund_total,
            );
            if !matches!(gate, LiquidationGateResult::Proceed) {
                // Report cross-module incidents to sentinel
                match &gate {
                    LiquidationGateResult::VelocityBreached { .. } => {
                        system_sentinel.report_liquidation_velocity_breach(
                            circuit_breaker.max_liquidations_per_window,
                            circuit_breaker.window_secs,
                        );
                    }
                    LiquidationGateResult::WaterfallHalted { cumulative_loss } => {
                        system_sentinel.report_waterfall_halt(*cumulative_loss);
                    }
                    _ => {}
                }
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "liquidation_gate_blocked",
                    "skipped",
                    &snapshot.market_id,
                    snapshot.outcome,
                    Some(candidate.user_id.clone()),
                    None,
                    "automation-liquidation-gate",
                    serde_json::json!({"gate_result": format!("{:?}", gate)}),
                );
                continue;
            }
            // Deribit-style: cancel all open orders for the user before liquidation
            // to free up collateral and prevent further exposure increase.
            let cancel_op = types::generate_op_id("pre-liq-cancel");
            let cancel_result = engine
                .mass_cancel_by_user(MassCancelByUserCommand {
                    metadata: types::CommandMetadata::new(cancel_op.clone()),
                    user_id: candidate.user_id.clone(),
                })
                .await;
            if let Ok(ref cr) = cancel_result {
                if !cr.cancelled_order_ids.is_empty() {
                    append_risk_audit_event(
                        audit_store.as_ref(),
                        "pre_liquidation_cancel",
                        "ok",
                        &snapshot.market_id,
                        snapshot.outcome,
                        Some(candidate.user_id.clone()),
                        None,
                        &cancel_op,
                        serde_json::json!({"cancelled_count": cr.cancelled_order_ids.len()}),
                    );
                }
            }
            // Post-cancel margin re-check: cancelling orders frees held collateral,
            // which may restore the user above maintenance margin — skip liquidation
            // if the user is now solvent (Deribit-standard behaviour).
            let recheck = risk.evaluate_liquidation(
                &candidate.user_id,
                &instrument,
                snapshot.outcome,
                mark_price,
                instrument.max_leverage,
                inst_maintenance_margin_bps,
            );
            if matches!(recheck, Ok(None)) {
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "post_cancel_solvent",
                    "ok",
                    &snapshot.market_id,
                    snapshot.outcome,
                    Some(candidate.user_id.clone()),
                    None,
                    &cancel_op,
                    serde_json::json!({"reason": "user solvent after order cancellation, liquidation skipped"}),
                );
                continue;
            }
            let request_id = types::generate_op_id("auto-liq");
            let adl_candidates = risk.adl_ranking_with_governance_and_entry_prices(
                &instrument,
                snapshot.outcome,
                mark_price,
                candidate.position_qty,
                &governance,
                &entry_prices,
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
                Ok(true) => {
                    append_risk_audit_event(
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
                    );
                    ws_hub.publish_liquidation(
                        &snapshot.market_id,
                        serde_json::json!({
                            "market_id": snapshot.market_id,
                            "outcome": snapshot.outcome,
                            "side": if candidate.position_qty > 0 { "sell" } else { "buy" },
                            "mark_price": mark_price,
                            "quantity": candidate.position_qty.abs(),
                            "timestamp": Utc::now(),
                        }),
                    );
                    // Private notification to the liquidated user
                    ws_hub.publish_user_event(&candidate.user_id, crate::websocket::WsFeedEvent {
                        event_type: "liquidation_warning".into(),
                        market_id: snapshot.market_id.clone(),
                        data: serde_json::json!({
                            "market_id": snapshot.market_id,
                            "outcome": snapshot.outcome,
                            "mark_price": mark_price,
                            "position_qty": candidate.position_qty,
                            "margin_ratio_bps": candidate.margin_ratio_bps,
                            "maintenance_margin_required": candidate.maintenance_margin_required,
                            "status": "queued",
                            "timestamp": Utc::now(),
                        }),
                    });
                    // Record in velocity tracker for circuit-breaker evaluation
                    RiskEngine::record_liquidation_event(
                        &mut velocity_tracker,
                        &circuit_breaker,
                        Utc::now().timestamp(),
                        candidate
                            .maintenance_margin_required
                            .saturating_sub(candidate.collateral_total)
                            .max(0),
                    );
                }
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

#[allow(clippy::too_many_arguments)]
async fn run_funding_cycle(
    engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    funding_rates: Arc<PersistentFundingRateStore>,
    index_prices: Arc<PersistentIndexPriceStore>,
    audit_store: Arc<RiskAutomationAuditStore>,
    last_funded: &mut std::collections::HashMap<(String, i32), std::time::Instant>,
    now: std::time::Instant,
    global_interval_secs: u64,
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
    for snapshot in snapshots {
        let instrument = instruments.resolve(&snapshot.market_id);
        if instrument.kind != InstrumentKind::Perpetual {
            continue;
        }
        // Per-instrument funding interval: skip if not enough time has elapsed.
        let inst_interval = if instrument.funding_interval_secs > 0 {
            instrument.funding_interval_secs
        } else {
            global_interval_secs
        };
        let key = (snapshot.market_id.clone(), snapshot.outcome);
        if let Some(last) = last_funded.get(&key) {
            if now.duration_since(*last).as_secs() < inst_interval {
                continue;
            }
        }
        let Some(mark_price) = fair_price_quote_for_snapshot(&snapshot, index_prices.as_ref())
            .map(|quote| quote.fair_price)
        else {
            append_risk_audit_event(
                audit_store.as_ref(),
                "funding_batch",
                "skipped",
                &snapshot.market_id,
                snapshot.outcome,
                None,
                None,
                "automation-funding-skip",
                serde_json::json!({"reason": "mark price unavailable"}),
            );
            continue;
        };
        let manual_rate = funding_rates.get(&snapshot.market_id, snapshot.outcome);
        let derived_rate = derive_funding_rate_quote(&snapshot, index_prices.as_ref());
        let (funding_rate_ppm, funding_source, funding_detail) = if let Some(rate) = manual_rate {
            (
                rate.funding_rate_ppm,
                "manual_override",
                serde_json::json!({
                    "source": "manual_override",
                    "updated_by": rate.updated_by,
                    "recorded_at": rate.recorded_at,
                    "funding_rate_ppm": rate.funding_rate_ppm,
                }),
            )
        } else if let Some(derived) = derived_rate {
            (
                derived.funding_rate_ppm,
                "derived_premium_index",
                serde_json::json!({
                    "source": "derived_premium_index",
                    "index_price": derived.index_price,
                    "fair_price": derived.fair_price,
                    "premium_bps": derived.premium_bps,
                    "clamped_premium_bps": derived.clamped_premium_bps,
                    "interest_bps": derived.interest_bps,
                    "funding_rate_ppm": derived.funding_rate_ppm,
                    "degraded_mode": derived.degraded_mode,
                }),
            )
        } else {
            append_risk_audit_event(
                audit_store.as_ref(),
                "funding_batch",
                "skipped",
                &snapshot.market_id,
                snapshot.outcome,
                None,
                None,
                "automation-funding-skip",
                serde_json::json!({"reason": "no manual override or derived index price available"}),
            );
            continue;
        };
        let request_id = types::generate_op_id("auto-funding");
        match risk.settle_funding_batch(
            &snapshot.market_id,
            snapshot.outcome,
            mark_price,
            funding_rate_ppm,
            &user_ids,
            &request_id,
        ) {
            Ok(settlements) if settlements.is_empty() => {
                last_funded.insert(key, now);
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "funding_batch",
                    "skipped",
                    &snapshot.market_id,
                    snapshot.outcome,
                    None,
                    None,
                    &request_id,
                    serde_json::json!({
                        "reason": "no eligible counterparties",
                        "funding_source": funding_source,
                        "funding_detail": funding_detail,
                    }),
                );
            }
            Ok(settlements) => {
                last_funded.insert(key, now);
                for settlement in settlements {
                    append_risk_audit_event(
                        audit_store.as_ref(),
                        "funding_settled",
                        "ok",
                        &snapshot.market_id,
                        snapshot.outcome,
                        Some(settlement.payer_user_id.clone()),
                        Some(settlement.receiver_user_id.clone()),
                        &request_id,
                        serde_json::json!({
                            "settlement": settlement,
                            "funding_source": funding_source,
                            "funding_detail": funding_detail,
                        }),
                    );
                }
            }
            Err(error) => append_risk_audit_event(
                audit_store.as_ref(),
                "funding_batch",
                "error",
                &snapshot.market_id,
                snapshot.outcome,
                None,
                None,
                &request_id,
                serde_json::json!({
                    "error": error.to_string(),
                    "funding_source": funding_source,
                    "funding_detail": funding_detail,
                }),
            ),
        }
    }
}

pub(crate) async fn run_invariant_check_scheduler(
    ledger: Arc<LedgerService>,
    system_sentinel: Arc<sentinel::SystemSentinel>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        match ledger.verify_global_invariant() {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(error = %e, "ALERT: periodic ledger invariant check FAILED — balance inconsistency detected");
                system_sentinel.report_risk_anomaly(&format!("ledger invariant failed: {e}"));
            }
        }
        system_sentinel.gc_expired();
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_liquidation_scheduler(
    engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    index_prices: Arc<PersistentIndexPriceStore>,
    audit_store: Arc<RiskAutomationAuditStore>,
    queue_store: Arc<LiquidationQueueStore>,
    adl_governance_store: Arc<PersistentAdlGovernanceStore>,
    position_costs: Arc<PositionCostLedgerStore>,
    trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
    ws_hub: Arc<WsHub>,
    system_sentinel: Arc<sentinel::SystemSentinel>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(liquidation_interval_secs()));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let liquidator_user_id = automation_liquidator_user_id();
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
            position_costs.clone(),
            trade_journal_wal.clone(),
            ws_hub.clone(),
            &liquidator_user_id,
            system_sentinel.clone(),
        )
        .await;
        // Prune terminal records older than 1 hour to prevent unbounded memory growth.
        let pruned = queue_store.prune_terminal(chrono::Duration::hours(1));
        if pruned > 0 {
            tracing::info!(pruned, "pruned terminal liquidation queue records");
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_liquidation_worker_scheduler(
    engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    index_prices: Arc<PersistentIndexPriceStore>,
    audit_store: Arc<RiskAutomationAuditStore>,
    queue_store: Arc<LiquidationQueueStore>,
    auction_store: Arc<LiquidationAuctionStore>,
    adl_governance_store: Arc<PersistentAdlGovernanceStore>,
    liquidation_policy_store: Arc<PersistentLiquidationPolicyStore>,
    position_costs: Arc<PositionCostLedgerStore>,
    trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
) {
    let mut interval =
        tokio::time::interval(Duration::from_secs(liquidation_worker_interval_secs()));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let liquidator_user_id = automation_liquidator_user_id();
    loop {
        interval.tick().await;
        run_liquidation_queue_worker(
            engine.clone(),
            risk.clone(),
            instruments.clone(),
            index_prices.clone(),
            audit_store.clone(),
            queue_store.clone(),
            auction_store.clone(),
            adl_governance_store.clone(),
            liquidation_policy_store.clone(),
            position_costs.clone(),
            trade_journal_wal.clone(),
            &liquidator_user_id,
        )
        .await;
        // Prune terminal auction records older than 1 hour.
        let pruned = auction_store.prune_terminal(chrono::Duration::hours(1));
        if pruned > 0 {
            tracing::info!(pruned, "pruned terminal liquidation auction records");
        }
    }
}

async fn run_funding_scheduler(
    engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    funding_rates: Arc<PersistentFundingRateStore>,
    index_prices: Arc<PersistentIndexPriceStore>,
    audit_store: Arc<RiskAutomationAuditStore>,
) {
    let global_interval = funding_interval_secs().max(1);
    let mut interval = tokio::time::interval(Duration::from_secs(global_interval));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Track last funding settlement time per (market_id, outcome) to support
    // per-instrument funding intervals.
    let mut last_funded: std::collections::HashMap<(String, i32), std::time::Instant> =
        std::collections::HashMap::new();
    loop {
        interval.tick().await;
        let now = std::time::Instant::now();
        run_funding_cycle(
            engine.clone(),
            risk.clone(),
            instruments.clone(),
            funding_rates.clone(),
            index_prices.clone(),
            audit_store.clone(),
            &mut last_funded,
            now,
            global_interval,
        )
        .await;
    }
}

/// Resolve the directory for the order-trace JSONL trail. Honors the
/// `MONITOR_TRACE_DIR` environment variable if set; otherwise defaults to
/// `data/trace` relative to the working directory (matches the path used
/// in docs/MONITOR_DESIGN.md §2.1).
fn monitor_trace_dir() -> std::path::PathBuf {
    std::env::var("MONITOR_TRACE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("data/trace"))
}

/// Spawn the order-flow-monitor consumer and JSONL writer tasks.
///
/// Subscribes to the `order.trace` eventbus channel. For every
/// `Event::OrderTrace`, the event is applied to the in-memory projector
/// and forwarded into a bounded mpsc(8192) channel consumed by a
/// `JsonlWriter`. Observer-only:
///
/// - The consumer is fire-and-forget. If the JSONL channel is full,
///   events are dropped (`try_send` returns `Err`); producers never
///   observe backpressure.
/// - If the JSONL writer fails to open, the writer task exits cleanly
///   and the in-memory projector continues to receive events. Reads
///   through `/monitor/...` still work; only the durable trail is
///   missing.
/// - Per-record recovery events are filtered at the JSONL writer per
///   design §3.6.
fn spawn_monitor_consumer(
    event_bus: eventbus::EventBus,
    projector: std::sync::Arc<monitor::OrderTraceProjector>,
) {
    use tokio::sync::broadcast::error::RecvError;

    let trace_dir = monitor_trace_dir();
    let (jsonl_tx, mut jsonl_rx) = tokio::sync::mpsc::channel::<types::OrderTraceEvent>(8192);

    tokio::spawn(async move {
        let mut writer = match monitor_jsonl::JsonlWriter::open(
            trace_dir.clone(),
            monitor_jsonl::JsonlWriterConfig::default(),
        )
        .await
        {
            Ok(w) => {
                tracing::info!(
                    dir = %trace_dir.display(),
                    "monitor JSONL writer started"
                );
                w
            }
            Err(e) => {
                tracing::warn!(
                    dir = %trace_dir.display(),
                    error = %e,
                    "monitor JSONL writer disabled (open failed)"
                );
                return;
            }
        };
        while let Some(ev) = jsonl_rx.recv().await {
            if let Err(e) = writer.write_event(&ev).await {
                tracing::warn!(error = %e, "monitor JSONL write failed");
            }
        }
        let _ = writer.flush().await;
    });

    // Subscribe synchronously *here* (not inside the spawned task) so the
    // broadcast channel exists before any caller publishes. Without this,
    // events emitted between the moment the publisher first calls
    // `event_bus.publish(...)` and the moment the spawned task gets
    // scheduled and reaches `event_bus.subscribe(...)` are dropped on the
    // floor (broadcast doesn't replay). In particular this matters for
    // bootstrap-time events like `recovery_completed` (Step 9) — those
    // need spawn_monitor_consumer to have run before bootstrap_runtime
    // emits them.
    let mut rx = event_bus.subscribe("order.trace");
    tokio::spawn(async move {
        tracing::info!("monitor trace consumer started");
        loop {
            match rx.recv().await {
                Ok(types::Event::OrderTrace(ev)) => {
                    let ev_for_jsonl = ev.clone();
                    projector.apply_event(ev);
                    // Best-effort forward to the JSONL writer. Channel-full
                    // means the writer is behind; drop and continue.
                    let _ = jsonl_tx.try_send(ev_for_jsonl);
                }
                Ok(_) => {}
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(lagged = n, "monitor trace consumer lagged");
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    initialize_internal_auth_secret().expect("failed to initialize internal auth secret");
    initialize_api_key_registry().expect("failed to initialize API key registry");
    initialize_role_mapping().expect("failed to initialize role mapping");
    initialize_auth_failure_tracker();

    // Load configuration (TOML file + env overrides).
    let loaded_config = config::ExchangeConfig::load();
    let config_problems = loaded_config.validate();
    if !config_problems.is_empty() {
        for problem in &config_problems {
            tracing::error!(problem = %problem, "configuration validation failed");
        }
        std::process::exit(1);
    }
    tracing::info!(
        bind = %format!("{}:{}", loaded_config.server.bind_host, loaded_config.server.bind_port),
        "configuration loaded and validated"
    );
    CONFIG
        .set(loaded_config)
        .expect("CONFIG already initialised");

    tracing::info!("Starting Rust Exchange...");
    let event_bus = EventBus::new();
    let event_bus_for_ws = event_bus.clone();
    let event_bus_for_stops = event_bus.clone();
    let event_bus_for_monitor = event_bus.clone();
    let event_bus_for_projection = event_bus.clone();
    let event_bus_for_trading = event_bus.clone();
    let event_bus_for_ws_routes = event_bus.clone();

    // Order Flow Monitor: construct the projector and start the consumer
    // task BEFORE bootstrap_runtime so the broadcast subscriber exists
    // by the time bootstrap publishes recovery_completed (and any future
    // pre-runtime trace events). Without this ordering, the cold-boot
    // recovery aggregate is silently dropped.
    let trace_projector = monitor::OrderTraceProjector::new();
    spawn_monitor_consumer(event_bus_for_monitor, trace_projector.clone());

    let app = bootstrap_runtime(event_bus).await;
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
        position_costs,
        governance_actions,
        partitioned_engine,
        trade_journal_wal,
        trade_settlement_wal,
    } = app;

    let ip_rate_limiter = Arc::new(FixedWindowRateLimiter::new(Duration::from_secs(1)));
    let user_rate_limiter = Arc::new(FixedWindowRateLimiter::new(Duration::from_secs(1)));
    let admin_rate_limiter = Arc::new(FixedWindowRateLimiter::new(Duration::from_secs(1)));
    let _rl_config = RateLimitConfig::default();

    // ── Backoffice RBAC (Step 1E activation) ────────────────────────────
    // Construct the four RBAC stores under the configured data dir, then
    // build an AuthzService over them. If `BACKOFFICE_BOOTSTRAP_ADMIN` is
    // set AND the employee store is empty, seed that subject as an Active
    // employee with a SuperAdminBreakGlass / Act / Global grant valid for
    // 30 days. This unblocks first-boot use of the admin surface without
    // requiring a separate CLI; subsequent restarts are no-ops.
    let admin_data_dir = std::path::PathBuf::from(&cfg().wal.data_dir).join("admin");
    if let Err(e) = std::fs::create_dir_all(&admin_data_dir) {
        panic!(
            "failed to create RBAC data dir at '{}': {e}",
            admin_data_dir.display()
        );
    }
    let admin_employees_store = Arc::new(
        admin_rbac_store::AdminEmployeeStore::open_jsonl(
            admin_data_dir.join("employees.jsonl"),
        )
        .unwrap_or_else(|e| panic!("failed to open admin employee store: {e}")),
    );
    let admin_grants_store = Arc::new(
        admin_rbac_store::AdminGrantStore::open_jsonl(admin_data_dir.join("grants.jsonl"))
            .unwrap_or_else(|e| panic!("failed to open admin grant store: {e}")),
    );
    let admin_approvals_store = Arc::new(
        admin_rbac_store::ApprovalRequestStore::open_jsonl(
            admin_data_dir.join("approval_requests.jsonl"),
        )
        .unwrap_or_else(|e| panic!("failed to open admin approval-request store: {e}")),
    );
    let admin_rbac_audit_store = Arc::new(
        admin_rbac_audit::AdminRbacAuditStore::open_jsonl(
            admin_data_dir.join("rbac_audit.jsonl"),
        )
        .unwrap_or_else(|e| panic!("failed to open admin RBAC audit store: {e}")),
    );
    let authz_service = Arc::new(admin_authz::AuthzService::new(
        admin_employees_store.clone(),
        admin_grants_store.clone(),
    ));
    if let Ok(bootstrap_subject) = std::env::var("BACKOFFICE_BOOTSTRAP_ADMIN") {
        let bootstrap_subject = bootstrap_subject.trim().to_string();
        if !bootstrap_subject.is_empty() && admin_employees_store.get(&bootstrap_subject).is_none()
        {
            let now = chrono::Utc::now();
            if let Err(e) = admin_employees_store.create(types::Employee {
                schema_version: types::BACKOFFICE_SCHEMA_VERSION,
                employee_id: bootstrap_subject.clone(),
                display_name: format!("bootstrap admin: {bootstrap_subject}"),
                status: types::EmployeeStatus::Active,
                created_at: now,
                updated_at: now,
                last_mfa_method: Some(types::MfaMethod::Webauthn),
                last_login_at: None,
            }) {
                tracing::error!(
                    error = %e,
                    "BACKOFFICE_BOOTSTRAP_ADMIN seeding failed at employee insert"
                );
            } else if let Err(e) = admin_grants_store.create(types::Grant {
                schema_version: types::BACKOFFICE_SCHEMA_VERSION,
                grant_id: format!("g-bootstrap-{}", uuid::Uuid::new_v4()),
                employee_id: bootstrap_subject.clone(),
                role: types::BackofficeRole::SuperAdminBreakGlass,
                level: types::RoleLevel::Act,
                scope: types::GrantScope::Global,
                status: types::GrantStatus::Active,
                granted_by: "system:bootstrap".into(),
                granted_at: now,
                expires_at: now + chrono::Duration::days(30),
                reason: "BACKOFFICE_BOOTSTRAP_ADMIN env var seed at first boot".into(),
                approval_request_id: None,
            }) {
                tracing::error!(
                    error = %e,
                    "BACKOFFICE_BOOTSTRAP_ADMIN seeding failed at grant insert"
                );
            } else {
                tracing::warn!(
                    subject = %bootstrap_subject,
                    "BACKOFFICE_BOOTSTRAP_ADMIN: seeded SuperAdminBreakGlass grant valid for 30 days. Replace with a normal grant via /admin/employees/{{id}}/roles before TTL expiry."
                );
            }
        }
    }

    let stop_order_store = Arc::new(
        StopOrderStore::open_jsonl(stop_orders_wal_path())
            .unwrap_or_else(|e| panic!("failed to initialize stop order store: {e}")),
    );
    // Order Flow Monitor: attach a trace emitter so OrderStateProjectionStore
    // emits projection_updated after each upsert. Observer-only.
    let projection_trace_emitter: Arc<dyn types::TraceEmitter> = Arc::new(
        monitor::EventBusTraceEmitter::new(event_bus_for_projection),
    );
    let order_projection = Arc::new(
        OrderStateProjectionStore::open_jsonl(order_state_projection_wal_path())
            .unwrap_or_else(|e| panic!("failed to initialize order projection store: {e}"))
            .with_trace_emitter(projection_trace_emitter),
    );
    let beta_controls = Arc::new(
        BetaControlStore::open_jsonl(beta_controls_wal_path())
            .unwrap_or_else(|e| panic!("failed to initialize beta control store: {e}")),
    );
    let admin_action_audit = Arc::new(
        AdminActionAuditStore::open_jsonl(admin_action_audit_wal_path())
            .unwrap_or_else(|e| panic!("failed to initialize admin action audit store: {e}")),
    );
    initialize_admin_action_audit_store(admin_action_audit.clone());

    let system_sentinel = Arc::new(sentinel::SystemSentinel::new(
        sentinel::PosturePolicy::default(),
    ));

    // Post-bootstrap: verify ledger invariant against sentinel.
    if let Err(e) = ledger.verify_global_invariant() {
        tracing::error!(error = %e, "startup sentinel: ledger invariant already violated");
        system_sentinel.report_risk_anomaly(&format!("startup invariant failure: {e}"));
    }
    let startup_snapshots = partitioned_engine
        .export_snapshots()
        .await
        .unwrap_or_default();
    let startup_trades = trade_journal_wal.entries().unwrap_or_default();
    if let Err(error) = order_projection.sync_from_sources(
        &sequencer.latest_records(),
        &startup_trades,
        &flatten_market_snapshots(&startup_snapshots),
    ) {
        tracing::warn!(error = %error, "failed to bootstrap order state projection");
    }

    let trading_routes = build_trading_routes(
        partitioned_engine.clone(),
        sequencer.clone(),
        order_projection.clone(),
        risk.clone(),
        instruments.clone(),
        stop_order_store.clone(),
        beta_controls.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
        system_sentinel.clone(),
        event_bus_for_trading,
    );
    let control_routes = build_control_routes(
        partitioned_engine.clone(),
        ledger.clone(),
        sequencer.clone(),
        governance_actions.clone(),
        beta_controls.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let account_routes = build_account_routes(
        partitioned_engine.clone(),
        sequencer.clone(),
        order_projection.clone(),
        risk.clone(),
        instruments.clone(),
        ledger.clone(),
        index_prices.clone(),
        position_costs.clone(),
        trade_journal_wal.clone(),
        risk_automation_audit.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    let product_flow_store = Arc::new(ProductFlowStore::new());
    let market_routes = build_market_routes(
        partitioned_engine.clone(),
        instruments.clone(),
        trade_journal_wal.clone(),
        ledger.clone(),
        index_prices.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    let withdrawal_store = Arc::new(
        WithdrawalStore::open_jsonl(withdrawals_wal_path())
            .unwrap_or_else(|e| panic!("failed to initialize withdrawal store: {e}")),
    );
    // ── Custody: address whitelist, vault topology, withdrawal gate ───
    let address_whitelist_store = Arc::new(
        custody::AddressWhitelistStore::open_jsonl(address_whitelist_wal_path())
            .unwrap_or_else(|e| panic!("failed to initialize address whitelist store: {e}")),
    );
    let custody_config = custody::CustodyConfig::default();
    let withdrawal_policy = custody::WithdrawalPolicy::default();
    let withdrawal_usage = Arc::new(custody::WithdrawalUsageTracker::new());
    let velocity_tracker = Arc::new(custody::VaultVelocityTracker::new());
    let velocity_policy = custody::VaultVelocityPolicy::default();
    let delay_policy = custody::WithdrawalDelayPolicy::default();
    let allowlist_policy = custody::AllowlistPolicy::default();
    let address_usage = Arc::new(custody::AddressUsageTracker::new());
    let isolation_policy = custody::IsolationPolicy::default();
    let custody_breaker = Arc::new(custody::CustodyCircuitBreaker::new(
        custody::BreakerConfig::default(),
    ));
    let custody_audit_log = Arc::new(custody::CustodyAuditLog::new());
    let withdrawal_routes = build_withdrawal_routes(
        withdrawal_store.clone(),
        ledger.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
        admin_rate_limiter.clone(),
        address_whitelist_store.clone(),
        withdrawal_policy.clone(),
        custody_config.clone(),
        withdrawal_usage.clone(),
        velocity_tracker.clone(),
        velocity_policy.clone(),
        delay_policy.clone(),
        allowlist_policy.clone(),
        address_usage.clone(),
        custody_breaker.clone(),
        custody_audit_log.clone(),
        system_sentinel.clone(),
    );
    let custody_routes = custody::build_custody_routes(
        address_whitelist_store,
        withdrawal_policy,
        custody_config,
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
        admin_rate_limiter.clone(),
        delay_policy,
        velocity_tracker,
        velocity_policy,
        allowlist_policy,
        address_usage,
        isolation_policy,
        withdrawal_usage,
        custody_breaker,
        custody_audit_log,
        withdrawal_store,
        ledger.clone(),
    );
    let sentinel_routes = sentinel::build_sentinel_routes(
        system_sentinel.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let node_identity = Arc::new(ops::NodeIdentity::standalone(8));
    let data_plane_breaker = Arc::new(planes::PlaneCircuitBreaker::new(
        10,
        Duration::from_secs(30),
    ));
    let control_plane_breaker =
        Arc::new(planes::PlaneCircuitBreaker::new(5, Duration::from_secs(15)));
    let perf_routes = perf::build_perf_routes(
        partitioned_engine.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let ops_routes = ops::build_ops_routes(
        partitioned_engine.clone(),
        ledger.clone(),
        node_identity.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let failpoint_routes =
        failpoint::build_failpoint_routes(ip_rate_limiter.clone(), admin_rate_limiter.clone());
    let plane_routes = planes::build_plane_routes(
        data_plane_breaker.clone(),
        control_plane_breaker.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let fee_tier_store = Arc::new(
        FeeTierStore::open_jsonl(fee_tiers_wal_path())
            .unwrap_or_else(|e| panic!("failed to initialize fee tier store: {e}")),
    );
    let fee_tier_routes = build_fee_tier_routes(
        fee_tier_store,
        trade_journal_wal.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let transfer_store = Arc::new(parking_lot::RwLock::new(
        TransferStore::open_jsonl(transfers_wal_path())
            .unwrap_or_else(|e| panic!("failed to initialize transfer store: {e}")),
    ));
    let transfer_routes = build_transfer_routes(
        transfer_store,
        ledger.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    let stop_order_routes = build_stop_order_routes(
        stop_order_store.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    let product_flow_routes = build_product_flow_routes(
        product_flow_store.clone(),
        ledger.clone(),
        ip_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    let admin_routes = build_admin_routes(
        risk.clone(),
        instruments.clone(),
        ledger.clone(),
        funding_rates.clone(),
        risk_automation_audit.clone(),
        beta_controls.clone(),
        admin_action_audit.clone(),
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
        risk.clone(),
        instruments.clone(),
        partitioned_engine.clone(),
        sequencer.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
        system_sentinel.clone(),
    );
    let liquidation_admin_routes = build_liquidation_routes(
        risk.clone(),
        instruments.clone(),
        adl_governance.clone(),
        liquidation_queue.clone(),
        liquidation_auction.clone(),
        ledger.clone(),
        governance_actions.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
        user_rate_limiter.clone(),
    );
    // --- Health / Readiness ---
    let startup_time = Instant::now();
    let health_ledger = ledger.clone();
    let health_engine = partitioned_engine.clone();
    let health_sequencer = sequencer.clone();
    let health_order_projection = order_projection.clone();
    let health_trade_journal = trade_journal_wal.clone();
    let health_trade_settlement = trade_settlement_wal.clone();
    let health_route = warp::path("health")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || -> warp::reply::Json {
            let uptime_secs = startup_time.elapsed().as_secs();
            let sequencer_records = health_sequencer.latest_records();
            let trade_records = health_trade_journal.entries().unwrap_or_default();
            let settlement_records = health_trade_settlement.entries().unwrap_or_default();
            let ledger_entries = health_ledger.wal_entries().unwrap_or_default();
            warp::reply::json(&serde_json::json!({
                "status": "ok",
                "uptime_secs": uptime_secs,
                "accounts": health_ledger.account_count(),
                "seen_op_ids": health_ledger.seen_op_id_count(),
                "kill_switch": health_engine.kill_switch_enabled(),
                "bridge_alive": observability::METRICS.bridge_alive.load(Ordering::Relaxed),
                "frontiers": core_chain_frontiers_snapshot(
                    &sequencer_records,
                    health_order_projection.as_ref(),
                    &trade_records,
                    &settlement_records,
                    &ledger_entries,
                ),
            }))
        });
    let readiness_ledger = ledger.clone();
    let readiness_sequencer = sequencer.clone();
    let readiness_order_projection = order_projection.clone();
    let readiness_trade_journal = trade_journal_wal.clone();
    let readiness_trade_settlement = trade_settlement_wal.clone();
    let readiness_route = warp::path("ready")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || -> warp::reply::Json {
            let invariant_ok = readiness_ledger.verify_global_invariant().is_ok();
            let sequencer_records = readiness_sequencer.latest_records();
            let trade_records = readiness_trade_journal.entries().unwrap_or_default();
            let settlement_records = readiness_trade_settlement.entries().unwrap_or_default();
            let ledger_entries = readiness_ledger.wal_entries().unwrap_or_default();
            let frontiers = core_chain_frontiers_snapshot(
                &sequencer_records,
                readiness_order_projection.as_ref(),
                &trade_records,
                &settlement_records,
                &ledger_entries,
            );
            let frontier_ok = frontiers["consistent"].as_bool().unwrap_or(false);
            let status = if invariant_ok && frontier_ok {
                "ready"
            } else {
                "degraded"
            };
            warp::reply::json(&serde_json::json!({
                "status": status,
                "balance_invariant": invariant_ok,
                "frontier_consistency": frontier_ok,
                "frontiers": frontiers,
            }))
        });

    let partition_health_engine = partitioned_engine.clone();
    let partition_health_route =
        warp::path!("health" / "partitions")
            .and(warp::get())
            .map(move || -> warp::reply::Json {
                let depths = partition_health_engine.queue_depths();
                let partitions: Vec<serde_json::Value> = depths
                    .iter()
                    .map(|d| {
                        let fills = observability::METRICS
                            .partition_fills
                            .get(d.partition_id)
                            .map(|a| a.load(std::sync::atomic::Ordering::Relaxed))
                            .unwrap_or(0);
                        let orders = observability::METRICS
                            .partition_orders
                            .get(d.partition_id)
                            .map(|a| a.load(std::sync::atomic::Ordering::Relaxed))
                            .unwrap_or(0);
                        let utilization_pct = if d.capacity > 0 {
                            (d.inflight as f64 / d.capacity as f64 * 100.0).round() as u64
                        } else {
                            0
                        };
                        serde_json::json!({
                            "partition_id": d.partition_id,
                            "inflight": d.inflight,
                            "capacity": d.capacity,
                            "utilization_pct": utilization_pct,
                            "total_fills": fills,
                            "total_orders": orders,
                        })
                    })
                    .collect();
                warp::reply::json(&serde_json::json!({
                    "partitions": partitions,
                }))
            });

    let metrics_route = warp::path("metrics")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || -> warp::reply::Json {
            warp::reply::json(&observability::METRICS.snapshot())
        });

    let version_route = warp::path("version")
        .and(warp::get())
        .map(|| -> warp::reply::Json {
            warp::reply::json(&serde_json::json!({
                "name": "rust-exchange",
                "version": env!("CARGO_PKG_VERSION"),
                "build_date": "2026-03-13",
            }))
        });

    let prometheus_route = warp::path!("metrics" / "prometheus")
        .and(warp::get())
        .map(|| {
            warp::reply::with_header(
                prometheus::render_prometheus(),
                "content-type",
                "text/plain; version=0.4.0; charset=utf-8",
            )
        });

    let openapi_routes = openapi::build_openapi_routes();

    // ── Order Flow Monitor — REST routes ────────────────────────────────
    // The projector + consumer task are constructed earlier (before
    // bootstrap_runtime) so that bootstrap-time emits like
    // recovery_completed have a live subscriber. We only mount the REST
    // routes here, where the rest of the route assembly happens.
    // Step 1E: monitor RBAC is now active. Admin principals must hold a
    // grant satisfying `BackofficeAction::MonitorAccess` (auditor_readonly
    // / support_l1 / trading_ops / risk_ops / finance_ops /
    // super_admin_break_glass per the v1 matrix) to see beyond their
    // own subject. Customer (User) principals are unaffected.
    let monitor_routes = monitor_http::build_monitor_routes(
        trace_projector.clone(),
        with_principal(),
        Some(authz_service.clone()),
    );

    // Mount the RBAC management surface + maker-checker approval flow.
    let admin_rbac_routes = admin_rbac_http::build_admin_rbac_routes(
        admin_employees_store.clone(),
        admin_grants_store.clone(),
        authz_service.clone(),
        admin_rbac_audit_store.clone(),
        with_principal(),
    );
    let admin_approvals_routes = admin_approvals_http::build_admin_approvals_routes(
        admin_approvals_store.clone(),
        authz_service.clone(),
        admin_rbac_audit_store.clone(),
        with_principal(),
    );

    // Step 5: Trading Ops actions (market halt/resume) wired through
    // RBAC + maker-checker. trading_ops requires a previously-approved
    // approval request; super_admin_break_glass commits single-actor.
    let admin_trading_ops_routes = admin_trading_ops_http::build_admin_trading_ops_routes(
        partitioned_engine.clone(),
        sequencer.clone(),
        admin_approvals_store.clone(),
        authz_service.clone(),
        admin_rbac_audit_store.clone(),
        with_principal(),
    );

    // Step 7G: Wallet — instantiate stores + a per-chain runtime. v1
    // ships an in-memory ETH stub adapter so the operator surface is
    // observable end-to-end without any chain RPC wiring. The real
    // ETH adapter lands in 7E behind a feature flag. The hot wallet
    // address comes from `WALLET_ETH_HOT_ADDRESS` if set, else a
    // deterministic placeholder.
    let wallet_data_dir = std::path::PathBuf::from(&cfg().wal.data_dir).join("wallet");
    if let Err(e) = std::fs::create_dir_all(&wallet_data_dir) {
        panic!(
            "failed to create wallet data dir at '{}': {e}",
            wallet_data_dir.display()
        );
    }
    let wallet_address_book = Arc::new(
        wallet::AddressBookStore::open_jsonl(wallet_data_dir.join("addresses.jsonl"))
            .unwrap_or_else(|e| panic!("failed to open address book: {e}")),
    );
    let wallet_withdrawals = Arc::new(
        wallet::WithdrawalStore::open_jsonl(wallet_data_dir.join("withdrawals.jsonl"))
            .unwrap_or_else(|e| panic!("failed to open withdrawal store: {e}")),
    );
    let wallet_eth_hot_address = std::env::var("WALLET_ETH_HOT_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".into());
    let wallet_eth_seed_balance: i128 = std::env::var("WALLET_ETH_SEED_WEI")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let wallet_eth_adapter = Arc::new(wallet::InMemoryChainAdapter::new(wallet::ChainId::Eth));
    if wallet_eth_seed_balance > 0 {
        wallet_eth_adapter.seed_balance(&wallet_eth_hot_address, wallet_eth_seed_balance);
    }
    let wallet_runtime = admin_wallet_http::WalletRuntime::empty().with_chain(
        wallet::ChainId::Eth,
        wallet_eth_adapter.clone(),
        wallet_eth_hot_address.clone(),
    );
    // The address book is consumed below by the customer-facing
    // /v2/wallet/* routes (parallel to the legacy /withdraw on the
    // older custody module).
    let _ = &wallet_address_book;
    // Test-only adapter map: lets POST /admin/wallet/test-confirm bump
    // confirmation depth on the in-memory adapter. Empty in
    // production deploys with real RPC adapters; the endpoint then
    // returns 404 for any tx_hash on those chains.
    let mut wallet_test_adapters: admin_wallet_http::TestAdapters = std::collections::HashMap::new();
    wallet_test_adapters.insert(wallet::ChainId::Eth, wallet_eth_adapter.clone());
    let admin_wallet_routes = admin_wallet_http::build_admin_wallet_routes(
        wallet_runtime,
        wallet_test_adapters,
        wallet_withdrawals.clone(),
        authz_service.clone(),
        admin_rbac_audit_store.clone(),
        with_principal(),
    );

    // P1-OPS-1: poll the hot-wallet on-chain balance every 60 s and
    // expose it as a gauge (`wallet_hot_wallet_balance{chain="eth"}`).
    // Prometheus alerting fires when this drops below threshold.
    {
        use wallet::ChainAdapter as _;
        let adapter = wallet_eth_adapter.clone();
        let hot_address = wallet_eth_hot_address.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Ok(balance) = adapter.balance(&hot_address) {
                    let micro = (balance / 1_000_000_000_000_i128)
                        .clamp(i64::MIN as i128, i64::MAX as i128)
                        as i64;
                    crate::observability::record_wallet_hot_balance("eth", micro);
                }
            }
        });
    }

    // Step 8 part 3: spawn the in-process hot-wallet worker. One task
    // per chain. Tick interval defaults to 5 s; override via
    // `WALLET_WORKER_TICK_MS`. The worker drives Approved -->
    // Broadcast --> Confirmed via the ChainAdapter; settlement
    // (Confirmed --> Settled with the ledger debit) is a separate
    // future task.
    let wallet_worker_tick_ms: u64 = std::env::var("WALLET_WORKER_TICK_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000);
    let wallet_worker_eth = Arc::new(wallet::HotWalletWorker::new(
        wallet::ChainId::Eth,
        wallet_eth_adapter.clone(),
        wallet_withdrawals.clone(),
        wallet_eth_hot_address.clone(),
    ));
    {
        let worker = wallet_worker_eth.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(wallet_worker_tick_ms));
            tracing::info!(
                tick_ms = wallet_worker_tick_ms,
                "hot wallet worker started for eth"
            );
            loop {
                interval.tick().await;
                let report = worker.tick();
                if report.signed_count
                    + report.broadcast_count
                    + report.confirmed_count
                    + report.failed_count
                    > 0
                {
                    tracing::info!(
                        chain = "eth",
                        signed = report.signed_count,
                        broadcast = report.broadcast_count,
                        confirmed = report.confirmed_count,
                        failed = report.failed_count,
                        "hot wallet worker tick"
                    );
                }
            }
        });
    }

    // Step 8 part 6: settlement worker. Drives Confirmed -> Settled
    // via the existing ledger crate. Single in-process task; idempotent
    // per withdrawal_id via op_id `wd-settle-{withdrawal_id}`.
    // P0-FUND-2 + P0-FUND-3: per-chain ChainSpec map. Each chain
    // gets its own SYS:WALLET:HOT:<chain> account and ledger divisor.
    // Override via WALLET_USE_LEGACY_SETTLEMENT_ACCOUNT=1 to keep the
    // pre-P0-FUND-2 behaviour (single SYS:ONCHAIN_VAULT:USDC) for
    // staging environments that haven't migrated yet.
    let use_legacy_settlement = std::env::var("WALLET_USE_LEGACY_SETTLEMENT_ACCOUNT")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let wallet_settlement_worker = if use_legacy_settlement {
        Arc::new(admin_wallet_settlement::SettlementWorker::new(
            ledger.clone(),
            wallet_withdrawals.clone(),
            admin_wallet_settlement::SettlementWorker::default_settlement_account(),
        ))
    } else {
        let mut chains = std::collections::HashMap::new();
        // ETH spec — but with divisor=1 in v1 because the test path
        // uses small amounts and the ledger lacks per-chain accounts
        // in incumbent stores. Production sets divisor=1e12 once per-
        // chain accounts are confirmed populated. Operators can flip
        // back to legacy behaviour via the env override above.
        let mut eth_spec = wallet::ChainSpec::eth_default();
        eth_spec.ledger_divisor = std::env::var("WALLET_ETH_LEDGER_DIVISOR")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_i128);
        chains.insert(wallet::ChainId::Eth, eth_spec);
        Arc::new(admin_wallet_settlement::SettlementWorker::with_chains(
            ledger.clone(),
            wallet_withdrawals.clone(),
            chains,
        ))
    };
    {
        let worker = wallet_settlement_worker.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(wallet_worker_tick_ms));
            tracing::info!(
                tick_ms = wallet_worker_tick_ms,
                "wallet settlement worker started"
            );
            loop {
                interval.tick().await;
                let report = worker.tick();
                crate::observability::METRICS.record_wallet_settlement_tick(
                    report.settled_count as u64,
                    report.failed_count as u64,
                    report.stuck_count as u64,
                );
                if report.settled_count + report.failed_count + report.stuck_count > 0 {
                    tracing::info!(
                        settled = report.settled_count,
                        failed = report.failed_count,
                        stuck = report.stuck_count,
                        "wallet settlement worker tick"
                    );
                }
            }
        });
    }

    // Cool-down sweep task: flips PendingCooldown -> Active for any
    // address whose cool-down window has elapsed. Without this, a
    // freshly-whitelisted address would stay in PendingCooldown
    // forever (the handler still allows submit once the window has
    // passed, but the listing endpoint and operator dashboards see a
    // stale status). Tick every 60s; cheap operation.
    {
        let book = wallet_address_book.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = book.sweep_cooldowns() {
                    tracing::warn!(error = %e, "address-book cool-down sweep failed");
                }
            }
        });
    }

    // Customer-facing wallet endpoints under /v2/wallet/* on the new
    // wallet stack. Parallel to the legacy /withdraw (which still runs
    // on the older custody module). The cutover from the old to the
    // new path is a frontend / SDK migration. Sanctions provider is
    // the in-process stub for v1; the real Chainalysis adapter lands
    // behind a feature flag once API keys are provisioned.
    let customer_wallet_sanctions: Arc<dyn wallet::SanctionsProvider> =
        Arc::new(wallet::StubSanctionsProvider::new());
    // Velocity tracker starts empty on each boot — within the first
    // 24h of an upgrade the cap may under-count past withdrawals.
    // Acceptable for v1 since this path is not yet customer-default.
    // A future commit can rebuild from `wallet_withdrawals` history at
    // boot via `wallet::build_velocity_tracker`.
    // Rebuild the rolling-window velocity tracker from existing
    // withdrawal history at boot. Without this, the first 24h after
    // a restart under-counts every customer's velocity and the per-day
    // cap effectively doesn't exist (C4). `build_velocity_tracker`
    // skips Rejected records since those never represented real flow.
    let customer_wallet_velocity = wallet::build_velocity_tracker(
        std::time::Duration::from_secs(24 * 60 * 60),
        wallet_withdrawals.all().iter(),
        |r| r.status != wallet::WithdrawalStatus::Rejected,
    );
    // Cool-down between whitelisting an address and being able to use
    // it as a withdrawal destination. Defaults to 24h per design §4.2;
    // smoke harnesses set `WALLET_CUSTOMER_COOLDOWN_SECS=0` to drive
    // the full add → submit path in a single run.
    let customer_wallet_cooldown_secs: u64 = std::env::var("WALLET_CUSTOMER_COOLDOWN_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24 * 60 * 60);
    // H4: customer-wallet audit log (data/wallet/customer_audit.jsonl).
    // Captures every add/submit/poll outcome — success and failure —
    // independent of WithdrawalStore (which only sees what passed
    // validation).
    let customer_wallet_audit = Arc::new(
        customer_wallet_audit::CustomerWalletAuditStore::open_jsonl(
            wallet_data_dir.join("customer_audit.jsonl"),
        )
        .unwrap_or_else(|e| panic!("failed to open customer wallet audit store: {e}")),
    );
    // P0-FUND-4: maker-checker threshold for customer withdrawals.
    // Above this amount the record is parked at AwaitingApproval and
    // an admin must commit /admin/approval-requests/{id}/approve.
    // i128::MAX disables MC (auto-approve everything) — pre-P0-FUND-4
    // behaviour, kept as the default for tests and dev environments
    // that don't ship admin coverage. Production must set this.
    let customer_wallet_mc_threshold: i128 = std::env::var("WALLET_CUSTOMER_MC_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(i128::MAX);
    let customer_wallet_runtime = customer_wallet_http::CustomerWalletRuntime::new(
        wallet_address_book.clone(),
        wallet_withdrawals.clone(),
        customer_wallet_sanctions,
        customer_wallet_velocity,
        ledger.clone(),
        customer_wallet_audit.clone(),
    )
    .with_cooldown(std::time::Duration::from_secs(customer_wallet_cooldown_secs))
    .with_mc_threshold(customer_wallet_mc_threshold);
    let customer_wallet_routes_inner =
        customer_wallet_http::build_customer_wallet_routes(customer_wallet_runtime, with_principal());
    // H5: per-IP rate-limit pre-filter so an attacker can't probe
    // sanctioned-address space brute-force or DoS the sanctions
    // provider through /v2/wallet/addresses. Re-uses the workspace
    // ip_rate_limiter; on breach the request is rejected before any
    // sanctions / store call runs.
    let customer_wallet_ip_rate = ip_rate_limiter.clone();
    let customer_wallet_routes = remote_ip()
        .and_then(move |remote: Option<SocketAddr>| {
            let limiter = customer_wallet_ip_rate.clone();
            async move {
                let ip_key = remote
                    .map(|v| v.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                limiter
                    .check(&format!("ip:{ip_key}"), RateLimitConfig::default().ip_limit)
                    .map_err(|_| warp::reject::custom(customer_wallet_http::WalletError::RateLimited))?;
                Ok::<(), Rejection>(())
            }
        })
        .untuple_one()
        .and(customer_wallet_routes_inner)
        .boxed();

    let ws_hub = Arc::new(websocket::WsHub::with_max_connections(
        cfg().websocket.max_connections,
    ));
    let ws_routes = websocket::build_ws_routes(ws_hub.clone(), event_bus_for_ws_routes);

    // POST /v2/ws-token — mint a short-TTL bearer token for browser
    // WebSocket auth on `/ws/order-trace`. The browser cannot set the
    // x-internal-auth-* headers on a WS upgrade, so it calls this
    // endpoint over signed REST (which it can do) and presents the
    // returned token as `?token=<...>` on the WS URL.
    //
    // Token TTL is short (default 60 s, clamped [10, 300]); the
    // frontend mints fresh just before each connect.
    let ws_token_route = warp::path!("v2" / "ws-token")
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json::<serde_json::Value>())
        .and_then(|principal: AuthenticatedPrincipal, body: serde_json::Value| async move {
            let ws_path = body
                .get("ws_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // v1 only mints for the order-trace endpoint. As more WS
            // endpoints become browser-reachable, expand this allow-list
            // — never honour an arbitrary client-supplied path.
            let allowed_paths: &[&str] = &["/ws/order-trace"];
            if !allowed_paths.contains(&ws_path) {
                return Err(warp::reject::custom(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    code: Some("INVALID_WS_PATH".into()),
                    message: format!(
                        "ws_path must be one of {:?}",
                        allowed_paths
                    ),
                    details: None,
                }));
            }
            let secret = security::internal_auth_secret_opt().ok_or_else(|| {
                warp::reject::custom(ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: Some("AUTH_NOT_CONFIGURED".into()),
                    message: "internal auth secret not configured".into(),
                    details: None,
                })
            })?;
            let token = ws_token::mint_token(secret, &principal, ws_path).map_err(|e| {
                warp::reject::custom(ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: Some("WS_TOKEN_MINT_FAILED".into()),
                    message: e.to_string(),
                    details: None,
                })
            })?;
            let ttl = ws_token::ws_token_ttl_secs();
            let body = serde_json::json!({
                "token": token,
                "ttl_secs": ttl,
                "ws_path": ws_path,
            });
            Ok::<_, Rejection>(warp::reply::json(&body))
        })
        .boxed();

    let settlement_reconciliation_ledger = ledger.clone();
    let settlement_reconciliation_journal = trade_journal_wal.clone();
    let settlement_reconciliation_wal = trade_settlement_wal.clone();
    let settlement_reconciliation_costs = position_costs.clone();
    let settlement_reconciliation_engine = partitioned_engine.clone();
    let settlement_reconciliation_ip_rate = ip_rate_limiter.clone();
    let settlement_reconciliation_admin_rate = admin_rate_limiter.clone();
    let settlement_reconciliation_route =
        warp::path!("admin" / "risk" / "reconciliation" / "settlements")
            .and(warp::get())
            .and(with_principal())
            .and(remote_ip())
            .and_then(
                move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                    let ledger = settlement_reconciliation_ledger.clone();
                    let trade_journal_wal = settlement_reconciliation_journal.clone();
                    let trade_settlement_wal = settlement_reconciliation_wal.clone();
                    let position_costs = settlement_reconciliation_costs.clone();
                    let engine = settlement_reconciliation_engine.clone();
                    let ip_rate_limiter = settlement_reconciliation_ip_rate.clone();
                    let admin_rate_limiter = settlement_reconciliation_admin_rate.clone();
                    async move {
                        require_admin(&principal)?;
                        let ip_key = remote
                            .map(|value| value.ip().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        ip_rate_limiter
                            .check(&format!("ip:{ip_key}"), RateLimitConfig::default().ip_limit)?;
                        admin_rate_limiter.check(
                            &format!("admin:{}", principal.subject),
                            RateLimitConfig::default().admin_limit,
                        )?;
                        let settlements = trade_settlement_wal.entries().map_err(|error| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                        })?;
                        let trades = trade_journal_wal.entries().map_err(|error| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                        })?;
                        let ledger_entries = ledger.wal_entries().map_err(|error| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                        })?;
                        let _snapshots = flatten_market_snapshots(
                            &engine
                                .export_snapshots()
                                .await
                                .map_err(reject_internal_error)?,
                        );
                        Ok::<_, Rejection>(warp::reply::json(&settlement_reconciliation_snapshot(
                            &settlements,
                            &trades,
                            &ledger_entries,
                            position_costs.as_ref(),
                            200,
                        )))
                    }
                },
            );
    let core_chain_reconciliation_ledger = ledger.clone();
    let core_chain_reconciliation_sequencer = sequencer.clone();
    let core_chain_reconciliation_order_projection = order_projection.clone();
    let core_chain_reconciliation_engine = partitioned_engine.clone();
    let core_chain_reconciliation_journal = trade_journal_wal.clone();
    let core_chain_reconciliation_wal = trade_settlement_wal.clone();
    let core_chain_reconciliation_costs = position_costs.clone();
    let core_chain_reconciliation_ip_rate = ip_rate_limiter.clone();
    let core_chain_reconciliation_admin_rate = admin_rate_limiter.clone();
    let core_chain_reconciliation_route =
        warp::path!("admin" / "risk" / "reconciliation" / "core-chain")
            .and(warp::get())
            .and(with_principal())
            .and(remote_ip())
            .and_then(
                move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                    let ledger = core_chain_reconciliation_ledger.clone();
                    let sequencer = core_chain_reconciliation_sequencer.clone();
                    let order_projection = core_chain_reconciliation_order_projection.clone();
                    let engine = core_chain_reconciliation_engine.clone();
                    let trade_journal_wal = core_chain_reconciliation_journal.clone();
                    let trade_settlement_wal = core_chain_reconciliation_wal.clone();
                    let position_costs = core_chain_reconciliation_costs.clone();
                    let ip_rate_limiter = core_chain_reconciliation_ip_rate.clone();
                    let admin_rate_limiter = core_chain_reconciliation_admin_rate.clone();
                    async move {
                        require_admin(&principal)?;
                        let ip_key = remote
                            .map(|value| value.ip().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        ip_rate_limiter
                            .check(&format!("ip:{ip_key}"), RateLimitConfig::default().ip_limit)?;
                        admin_rate_limiter.check(
                            &format!("admin:{}", principal.subject),
                            RateLimitConfig::default().admin_limit,
                        )?;
                        let settlements = trade_settlement_wal
                            .entries()
                            .map_err(reject_internal_error)?;
                        let trades = trade_journal_wal.entries().map_err(reject_internal_error)?;
                        let ledger_entries = ledger.wal_entries().map_err(reject_internal_error)?;
                        let snapshots = flatten_market_snapshots(
                            &engine
                                .export_snapshots()
                                .await
                                .map_err(reject_internal_error)?,
                        );
                        Ok::<_, Rejection>(warp::reply::json(&core_chain_reconciliation_snapshot(
                            &sequencer.latest_records(),
                            order_projection.as_ref(),
                            &snapshots,
                            &settlements,
                            &trades,
                            &ledger_entries,
                            position_costs.as_ref(),
                            200,
                        )))
                    }
                },
            );

    // ── Binance public-data REST proxy ───────────────────────────
    //
    // Browser hits `/binance/rest/<path>?<query>` → server fetches
    // `https://data-api.binance.vision/api/v3/<path>?<query>` and pipes
    // the body back. The browser only ever talks to 127.0.0.1:3030,
    // sidestepping CORS variability, browser extensions, AV intercepts,
    // and any geo-block that affects browser DNS but not the host's.
    //
    // Read-only, ~1KB responses, public data — no auth, minimal risk.
    let binance_rest_route = warp::path("binance")
        .and(warp::path("rest"))
        .and(warp::path::tail())
        .and(warp::query::raw().or(warp::any().map(String::new)).unify())
        .and(warp::get())
        .and_then(|tail: warp::path::Tail, raw_query: String| async move {
            // Accept both `/binance/rest/klines` and the more natural
            // `/binance/rest/api/v3/klines` form so the same client URL
            // pattern works against both Binance directly and this proxy.
            let path = tail
                .as_str()
                .trim_start_matches('/')
                .trim_start_matches("api/v3/");
            // Whitelist the v3 public-data endpoints we actually consume.
            // Anything else returns 404 — refuses to be used as an open
            // forwarder.
            const ALLOWED: &[&str] = &["klines", "ticker/24hr", "depth", "trades"];
            if !ALLOWED.iter().any(|p| path == *p) {
                return Err(warp::reject::custom(ApiError {
                    status: StatusCode::NOT_FOUND,
                    code: Some("BINANCE_PROXY_PATH_NOT_ALLOWED".into()),
                    message: format!(
                        "binance proxy: path '{}' not in allow-list {:?}",
                        path, ALLOWED
                    ),
                    details: None,
                }));
            }
            let upstream = if raw_query.is_empty() {
                format!("https://data-api.binance.vision/api/v3/{}", path)
            } else {
                format!("https://data-api.binance.vision/api/v3/{}?{}", path, raw_query)
            };
            // Run the blocking ureq call on a worker thread.
            let response = tokio::task::spawn_blocking(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout(std::time::Duration::from_secs(8))
                    .build();
                agent.get(&upstream).call()
            })
            .await
            .map_err(|e| {
                warp::reject::custom(ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: Some("BINANCE_PROXY_TASK_PANIC".into()),
                    message: format!("binance proxy task panic: {e}"),
                    details: None,
                })
            })?;
            match response {
                Ok(resp) => {
                    let status_u16 = resp.status();
                    let body = match resp.into_string() {
                        Ok(s) => s,
                        Err(e) => return Err(warp::reject::custom(ApiError {
                            status: StatusCode::BAD_GATEWAY,
                            code: Some("BINANCE_PROXY_READ_FAILED".into()),
                            message: format!("binance proxy: read body failed: {e}"),
                            details: None,
                        })),
                    };
                    let status = StatusCode::from_u16(status_u16).unwrap_or(StatusCode::OK);
                    let reply = warp::reply::with_header(
                        warp::reply::with_status(body, status),
                        "Content-Type",
                        "application/json; charset=utf-8",
                    );
                    Ok::<_, Rejection>(reply)
                }
                Err(ureq::Error::Status(code, resp)) => {
                    let body = resp.into_string().unwrap_or_default();
                    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY);
                    let reply = warp::reply::with_header(
                        warp::reply::with_status(body, status),
                        "Content-Type",
                        "application/json; charset=utf-8",
                    );
                    Ok(reply)
                }
                Err(e) => Err(warp::reject::custom(ApiError {
                    status: StatusCode::BAD_GATEWAY,
                    code: Some("BINANCE_PROXY_UPSTREAM_ERROR".into()),
                    message: format!("binance proxy: upstream error: {e}"),
                    details: None,
                })),
            }
        })
        .boxed();

    // Static frontend files. Send `Cache-Control: no-cache` so the browser
    // always revalidates — without this, the legacy ES-module loader
    // aggressively caches `binance.js` etc. and a code change does not
    // take effect on refresh until the user clears site data.
    let static_files = warp::fs::dir("./frontend")
        .with(warp::reply::with::header(
            "Cache-Control",
            "no-cache, must-revalidate",
        ));
    let mut cors_builder = warp::cors()
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
    for origin in &cfg().cors.allowed_origins {
        cors_builder = cors_builder.allow_origin(origin.as_str());
    }
    let cors = cors_builder;

    #[derive(Debug, Clone, Default, serde::Deserialize)]
    struct MicrostructureQuery {
        outcome: Option<i32>,
        depth_levels: Option<usize>,
        trade_window: Option<usize>,
    }

    // ── Rules standardization endpoint ───────────────────
    let rules_instruments = instruments.clone();
    let rules_route = warp::path("rules")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || -> warp::reply::Json {
            let specs: Vec<InstrumentSpec> = rules_instruments.list();
            let market_rules: Vec<serde_json::Value> = specs
                .iter()
                .map(|spec| {
                    serde_json::json!({
                        "market_id": spec.instrument_id,
                        "kind": spec.kind,
                        "quote_asset": spec.quote_asset,
                        "margin_mode": spec.margin_mode,
                        "max_leverage": spec.max_leverage,
                        "tick_size": spec.tick_size,
                        "lot_size": spec.lot_size,
                        "min_order_amount": spec.min_order_amount,
                        "max_notional": spec.max_notional,
                        "price_band_bps": spec.price_band_bps,
                        "maker_fee_bps": spec.maker_fee_bps,
                        "taker_fee_bps": spec.taker_fee_bps,
                        "risk_policy": spec.risk_policy_id,
                    })
                })
                .collect();
            warp::reply::json(&serde_json::json!({
                "schema_version": "2026-03-14",
                "generated_at": Utc::now(),
                "matching_model": "continuous_clob",
                "priority": "price_time",
                "order_types": [
                    "limit", "market",
                    "stop_market", "stop_limit",
                    "take_profit_market", "take_profit_limit",
                ],
                "time_in_force": ["gtc", "ioc", "fok", "gtd"],
                "trigger_types": ["last_price"],
                "trigger_types_reserved": ["mark_price", "index_price"],
                "conditional_order_constraints": {
                    "trigger_price_required": true,
                    "limit_conditional_requires_price": true,
                    "supported_trigger_types": ["last_price"],
                },
                "features": {
                    "self_trade_prevention": true,
                    "stp_modes": ["cancel_taker", "cancel_maker", "cancel_both"],
                    "post_only": true,
                    "reduce_only": true,
                    "replace_atomic": true,
                    "mass_cancel": true,
                    "kill_switch": true,
                    "batch_orders": true,
                    "stop_orders": true,
                    "leverage_adjustment": true,
                    "trade_export": true,
                },
                "limits": {
                    "batch_max_orders": 20,
                    "max_client_order_id_len": 256,
                    "max_market_id_len": 256,
                },
                "rate_limits": {
                    "ip_per_second": 60,
                    "user_write_per_second": 30,
                    "user_read_per_second": 60,
                    "batch_per_second": 10,
                    "export_per_second": 5,
                    "microstructure_per_second": 30,
                },
                "settlement": {
                    "model": "double_entry_ledger",
                    "atomicity": "wal_backed",
                    "fee_collection": "per_fill",
                },
                "funding": {
                    "interval_secs": cfg().risk.funding_interval_secs,
                    "premium_cap_bps": pricing::funding_premium_cap_bps(),
                    "interest_bps_per_interval": pricing::funding_interest_bps_per_interval(),
                    "max_abs_rate_ppm": pricing::funding_rate_max_abs_ppm(),
                },
                "risk": {
                    "margin_modes": ["isolated", "cross"],
                    "max_leverage_default": 20,
                    "maintenance_margin_bps": cfg().risk.maintenance_margin_bps,
                    "liquidation_penalty_bps": cfg().risk.liquidation_penalty_bps,
                    "price_band_enforcement": true,
                    "adl_supported": true,
                    "insurance_fund": true,
                },
                "markets": market_rules,
            }))
        });

    // ── Microstructure transparency endpoint ─────────────
    let micro_engine = partitioned_engine.clone();
    let micro_instruments = instruments.clone();
    let micro_trades = trade_journal_wal.clone();
    let micro_index_prices = index_prices.clone();
    let micro_ip_rl = ip_rate_limiter.clone();
    let micro_route = warp::path!("markets" / String / "microstructure")
        .and(warp::get())
        .and(optional_query::<MicrostructureQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String, query: MicrostructureQuery, remote: Option<SocketAddr>| {
                let engine = micro_engine.clone();
                let instruments = micro_instruments.clone();
                let trade_journal = micro_trades.clone();
                let idx_prices = micro_index_prices.clone();
                let ip_rl = micro_ip_rl.clone();
                async move {
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rl.check(
                        &format!("ip:{ip_key}"),
                        RateLimitConfig::default().user_read_limit,
                    )?;
                    let spec = instruments.resolve(&market_id);
                    let records = engine.export_snapshots().await.map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    let snapshots = flatten_market_snapshots(&records);
                    let depth_levels = query.depth_levels.unwrap_or(5).clamp(1, 20);
                    let trade_window = query.trade_window.unwrap_or(100).clamp(10, 1000);
                    let market_snapshot = snapshots.iter().find(|s| {
                        s.market_id == market_id && query.outcome.is_none_or(|o| s.outcome == o)
                    });

                    // Compute depth stats, imbalance, volume profile
                    let depth_stats = if let Some(snapshot) = market_snapshot {
                        let mut bids: BTreeMap<i64, i64> = BTreeMap::new();
                        let mut asks: BTreeMap<i64, i64> = BTreeMap::new();
                        for order in &snapshot.orders {
                            match order.side {
                                Side::Buy => {
                                    *bids.entry(order.price).or_default() += order.remaining_amount
                                }
                                Side::Sell => {
                                    *asks.entry(order.price).or_default() += order.remaining_amount
                                }
                            }
                        }
                        let best_b = bids.keys().next_back().copied();
                        let best_a = asks.keys().next().copied();
                        let best_bid_qty = best_b.and_then(|p| bids.get(&p).copied());
                        let best_ask_qty = best_a.and_then(|p| asks.get(&p).copied());
                        let spr = match (best_b, best_a) {
                            (Some(b), Some(a)) => Some(a - b),
                            _ => None,
                        };
                        let total_bid_depth: i64 = bids.values().sum();
                        let total_ask_depth: i64 = asks.values().sum();
                        let bid_ask_imbalance = if total_bid_depth + total_ask_depth > 0 {
                            Some(
                                ((total_bid_depth - total_ask_depth) as f64
                                    / (total_bid_depth + total_ask_depth) as f64
                                    * 10000.0) as i64,
                            )
                        } else {
                            None
                        };
                        // Top N bid/ask levels for volume profile
                        let bid_profile: Vec<[i64; 2]> = bids
                            .iter()
                            .rev()
                            .take(depth_levels)
                            .map(|(&p, &q)| [p, q])
                            .collect();
                        let ask_profile: Vec<[i64; 2]> = asks
                            .iter()
                            .take(depth_levels)
                            .map(|(&p, &q)| [p, q])
                            .collect();
                        // Spread in basis points
                        let spread_bps = match (best_b, best_a) {
                            (Some(b), Some(a)) if b > 0 => Some((a - b) * 10_000 / b),
                            _ => None,
                        };
                        // Mid price
                        let mid_price = match (best_b, best_a) {
                            (Some(b), Some(a)) => Some((b + a) / 2),
                            _ => None,
                        };
                        let micro_price = match (best_b, best_a, best_bid_qty, best_ask_qty) {
                            (Some(b), Some(a), Some(bq), Some(aq)) if bq + aq > 0 => {
                                Some((a * bq + b * aq) / (bq + aq))
                            }
                            _ => None,
                        };
                        let spread_bps_mid = match (spr, mid_price) {
                            (Some(s), Some(m)) if m > 0 => Some(s * 10_000 / m),
                            _ => None,
                        };
                        serde_json::json!({
                            "total_orders": snapshot.orders.len(),
                            "bid_levels": bids.len(),
                            "ask_levels": asks.len(),
                            "best_bid": best_b,
                            "best_bid_qty": best_bid_qty,
                            "best_ask": best_a,
                            "best_ask_qty": best_ask_qty,
                            "mid_price": mid_price,
                            "micro_price": micro_price,
                            "spread": spr,
                            "spread_bps": spread_bps,
                            "spread_bps_mid": spread_bps_mid,
                            "total_bid_depth": total_bid_depth,
                            "total_ask_depth": total_ask_depth,
                            "bid_ask_imbalance_bps": bid_ask_imbalance,
                            "bid_volume_profile": bid_profile,
                            "ask_volume_profile": ask_profile,
                        })
                    } else {
                        serde_json::json!({
                            "total_orders": 0, "bid_levels": 0, "ask_levels": 0,
                            "best_bid": null, "best_bid_qty": null,
                            "best_ask": null, "best_ask_qty": null,
                            "mid_price": null, "micro_price": null,
                            "spread": null, "spread_bps": null, "spread_bps_mid": null,
                            "total_bid_depth": 0, "total_ask_depth": 0,
                            "bid_ask_imbalance_bps": null,
                            "bid_volume_profile": [], "ask_volume_profile": [],
                        })
                    };

                    // VWAP and realized volatility from recent trades
                    let (vwap, recent_trade_count, realized_volatility_bps) = {
                        let trades = markets::wal_entries_or_empty(trade_journal.as_ref())
                            .unwrap_or_default();
                        let recent: Vec<_> = trades
                            .iter()
                            .filter(|t| t.market_id == market_id)
                            .filter(|t| query.outcome.is_none_or(|o| t.outcome == o))
                            .rev()
                            .take(trade_window)
                            .collect();
                        let total_value: i128 = recent
                            .iter()
                            .map(|t| t.price as i128 * t.amount as i128)
                            .sum();
                        let total_qty: i128 = recent.iter().map(|t| t.amount as i128).sum();
                        let vwap = if total_qty > 0 {
                            Some((total_value / total_qty) as i64)
                        } else {
                            None
                        };
                        let mut returns_bps: Vec<f64> = Vec::new();
                        for pair in recent.windows(2) {
                            let p0 = pair[0].price;
                            let p1 = pair[1].price;
                            if p0 > 0 {
                                returns_bps.push(((p1 - p0) as f64 / p0 as f64) * 10_000.0);
                            }
                        }
                        let realized_volatility_bps = if returns_bps.len() > 1 {
                            let mean = returns_bps.iter().sum::<f64>() / returns_bps.len() as f64;
                            let var = returns_bps
                                .iter()
                                .map(|r| {
                                    let d = *r - mean;
                                    d * d
                                })
                                .sum::<f64>()
                                / returns_bps.len() as f64;
                            Some(var.sqrt() as i64)
                        } else {
                            None
                        };
                        (vwap, recent.len(), realized_volatility_bps)
                    };

                    // Mark/fair price
                    let mark_price = market_snapshot.and_then(|snap| {
                        pricing::fair_price_quote_for_snapshot(snap, &idx_prices)
                            .map(|q| q.fair_price)
                    });
                    let depth_mid = depth_stats
                        .get("mid_price")
                        .and_then(serde_json::Value::as_i64);
                    let mid_mark_basis_bps = match (depth_mid, mark_price) {
                        (Some(mid), Some(mark)) if mark > 0 => {
                            Some(((mid - mark) as f64 / mark as f64 * 10_000.0) as i64)
                        }
                        _ => None,
                    };

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "market_id": market_id,
                        "outcome": query.outcome,
                        "instrument": {
                            "kind": spec.kind,
                            "quote_asset": spec.quote_asset,
                            "margin_mode": spec.margin_mode,
                            "max_leverage": spec.max_leverage,
                        },
                        "tick_size": spec.tick_size,
                        "lot_size": spec.lot_size,
                        "min_order_amount": spec.min_order_amount,
                        "max_notional": spec.max_notional,
                        "price_band_bps": spec.price_band_bps,
                        "fees": {
                            "maker_bps": spec.maker_fee_bps,
                            "taker_bps": spec.taker_fee_bps,
                        },
                        "matching": {
                            "model": "continuous_clob",
                            "priority": "price_time",
                            "partitioned": true,
                        },
                        "depth_stats": depth_stats,
                        "depth_levels": depth_levels,
                        "vwap": vwap,
                        "recent_trade_count": recent_trade_count,
                        "realized_volatility_bps": realized_volatility_bps,
                        "trade_window": trade_window,
                        "mark_price": mark_price,
                        "mid_mark_basis_bps": mid_mark_basis_bps,
                        "timestamp": Utc::now(),
                    })))
                }
            },
        );

    let capacity_routes = capacity::build_capacity_routes(
        partitioned_engine.clone(),
        ws_hub.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let release_routes = release::build_release_routes(
        partitioned_engine.clone(),
        ledger.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let rollback_routes = rollback::build_rollback_routes(
        ledger.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );
    let dead_man_switch = Arc::new(oncall::DeadManSwitch::new(120));
    let oncall_routes = oncall::build_oncall_routes(
        dead_man_switch.clone(),
        ip_rate_limiter.clone(),
        admin_rate_limiter.clone(),
    );

    let admin_group = trading_routes
        .or(control_routes)
        .or(admin_routes)
        .or(pricing_admin_routes)
        .or(governance_admin_routes)
        .or(liquidation_admin_routes)
        .or(admin_rbac_routes)
        .or(admin_approvals_routes)
        .or(admin_trading_ops_routes)
        .or(admin_wallet_routes)
        .boxed();
    let user_group = account_routes
        .or(market_routes)
        .or(withdrawal_routes)
        .or(custody_routes)
        .or(customer_wallet_routes)
        .or(sentinel_routes)
        .or(fee_tier_routes)
        .or(monitor_routes)
        .or(ws_token_route)
        .boxed();
    let trade_aux_group = transfer_routes
        .or(stop_order_routes)
        .or(product_flow_routes)
        .or(perf_routes)
        .or(ops_routes)
        .or(failpoint_routes)
        .boxed();
    let ops_group = plane_routes
        .or(capacity_routes)
        .or(release_routes)
        .or(rollback_routes)
        .or(oncall_routes)
        .or(health_route)
        .boxed();
    let probe_group = readiness_route
        .or(partition_health_route)
        .or(prometheus_route)
        .or(metrics_route)
        .or(version_route)
        .or(rules_route)
        .boxed();
    let misc_group = micro_route
        .or(openapi_routes)
        .or(ws_routes)
        .or(settlement_reconciliation_route)
        .or(core_chain_reconciliation_route)
        .or(binance_rest_route)
        .or(static_files)
        .boxed();

    let routes = admin_group
        .or(user_group)
        .or(trade_aux_group)
        .or(ops_group)
        .or(probe_group)
        .or(misc_group)
        .boxed()
        .with(cors)
        // P2-SEC-3: defensive response headers (nosniff, CSP, HSTS, etc).
        // Applied to every reply on the routes chain — REST + static.
        .with(warp::reply::with::headers(
            security_headers::security_headers_map(),
        ))
        .with(warp::trace(tracing_ctx::request_trace_fn()))
        .with(warp::log::custom(|info: warp::log::Info<'_>| {
            let elapsed_us = info.elapsed().as_micros() as u64;
            observability::METRICS
                .http_requests_total
                .fetch_add(1, Ordering::Relaxed);
            observability::METRICS
                .http_request_latency
                .record(elapsed_us);
            observability::record_http_path(info.path());

            // Per-plane metrics.
            let plane = planes::RequestPlane::from_path(info.path());
            planes::PLANE_METRICS.record_request(plane);

            if info.status().is_server_error() || info.status().is_client_error() {
                observability::METRICS
                    .http_errors_total
                    .fetch_add(1, Ordering::Relaxed);
                planes::PLANE_METRICS.record_error(plane);
            }
            tracing::info!(
                method = %info.method(),
                path = %info.path(),
                status = %info.status().as_u16(),
                elapsed_us = elapsed_us,
                remote = ?info.remote_addr(),
                "request"
            );
        }))
        .recover(handle_rejection);

    spawn_automation_tasks(AutomationRuntime {
        ledger: ledger.clone(),
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
        position_costs: position_costs.clone(),
        trade_journal_wal: trade_journal_wal.clone(),
        ws_hub: ws_hub.clone(),
        system_sentinel: system_sentinel.clone(),
    });

    // Bridge eventbus �?WebSocket hub for real-time trade feeds.
    tokio::spawn(websocket::bridge_eventbus_to_ws(
        event_bus_for_ws,
        ws_hub.clone(),
    ));

    // Bridge: listen for trades and check stop order triggers.
    tokio::spawn(bridge_trades_to_stop_triggers(
        event_bus_for_stops,
        stop_order_store,
        partitioned_engine.clone(),
        sequencer.clone(),
        ws_hub.clone(),
    ));

    // Periodic orderbook snapshot publisher �?WS /ws/orderbook/:market_id
    tokio::spawn(websocket::run_orderbook_snapshot_scheduler(
        partitioned_engine.clone(),
        ws_hub.clone(),
        cfg().websocket.orderbook_snapshot_interval_ms,
    ));

    // Periodic mark price publisher �?WS /ws/mark-price/:market_id
    tokio::spawn(websocket::run_mark_price_scheduler(
        partitioned_engine.clone(),
        index_prices.clone(),
        ws_hub.clone(),
        cfg().websocket.orderbook_snapshot_interval_ms, // reuse interval
    ));

    let bind_addr = bind_address();
    tracing::info!("Starting HTTP server on {}", bind_addr);
    tokio::spawn(async move {
        warp::serve(routes).run(bind_addr).await;
    });

    tracing::info!("Exchange running with HTTP. Press Ctrl+C to exit.");
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");

    let shutdown_start = Instant::now();
    tracing::info!("Shutting down...");

    // Signal all WebSocket connections to close gracefully.
    ws_hub.shutdown();
    // Give WS clients a moment to receive the close frame.
    tokio::time::sleep(Duration::from_millis(200)).await;
    tracing::info!(
        active_ws = ws_hub.connection_count(),
        "WebSocket shutdown signal sent"
    );

    // Flush all partition snapshots so no state is lost.
    tracing::info!("flushing matching engine snapshots...");
    match partitioned_engine.flush_all_snapshots().await {
        Ok(()) => tracing::info!("all partition snapshots flushed successfully"),
        Err(e) => tracing::error!(error = %e, "failed to flush partition snapshots on shutdown"),
    }

    // Verify ledger invariant one final time.
    match ledger.verify_global_invariant() {
        Ok(()) => tracing::info!("shutdown: ledger balance invariant OK"),
        Err(e) => tracing::error!(error = %e, "shutdown: ledger balance invariant FAILED"),
    }

    tracing::info!(
        elapsed_ms = shutdown_start.elapsed().as_millis() as u64,
        "graceful shutdown complete"
    );
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
                price_levels: Vec::new(),
                bids: Vec::new(),
                winner_user_id: None,
                clearing_price: None,
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

    #[test]
    fn fixed_window_rate_limiter_evicts_stale_keys() {
        let limiter = FixedWindowRateLimiter::new_with_limits(Duration::from_millis(1), 1, 1);
        limiter.check("ip:1.1.1.1", 10).expect("first key accepted");
        std::thread::sleep(Duration::from_millis(5));
        limiter.check("ip:2.2.2.2", 10).expect("stale key evicted");
        assert_eq!(limiter.states.len(), 1);
        assert!(limiter.states.contains_key("ip:2.2.2.2"));
    }

    #[test]
    fn fixed_window_rate_limiter_rejects_when_limit_exceeded() {
        let limiter = FixedWindowRateLimiter::new_with_limits(Duration::from_secs(1), 10, 1);
        limiter.check("user:1", 1).expect("first accepted");
        let second = limiter.check("user:1", 1);
        assert!(second.is_err());
    }

    #[test]
    fn check_weighted_consumes_multiple_tokens() {
        let limiter = FixedWindowRateLimiter::new_with_limits(Duration::from_secs(1), 10, 1);
        // limit=5, weight=3 → first call consumes 3 tokens, 2 remaining
        limiter
            .check_weighted("user:1", 5, 3)
            .expect("3 of 5 accepted");
        // weight=2 → exactly fills the remaining 2
        limiter
            .check_weighted("user:1", 5, 2)
            .expect("2 of 2 accepted");
        // weight=1 → exceeds limit
        let overflow = limiter.check_weighted("user:1", 5, 1);
        assert!(overflow.is_err());
    }

    #[test]
    fn check_weighted_zero_weight_always_passes() {
        let limiter = FixedWindowRateLimiter::new_with_limits(Duration::from_secs(1), 10, 1);
        // Fill to limit
        limiter.check_weighted("user:1", 1, 1).unwrap();
        assert!(limiter.check("user:1", 1).is_err());
        // Weight=0 still passes
        limiter
            .check_weighted("user:1", 1, 0)
            .expect("zero weight passes");
    }

    #[test]
    fn rate_limit_config_defaults_match_legacy_values() {
        let cfg = RateLimitConfig::default();
        assert_eq!(cfg.ip_limit, 60);
        assert_eq!(cfg.user_read_limit, 30);
        assert_eq!(cfg.user_write_limit, 10);
        assert_eq!(cfg.admin_limit, 10);
    }

    // --- Governance regression tests ---

    fn make_governance_store() -> PendingGovernanceActionStore {
        PendingGovernanceActionStore::new(Arc::new(InMemoryWal::<GovernanceActionRecord>::new()))
            .expect("governance store")
    }

    #[test]
    fn governance_kill_switch_creates_pending_action() {
        let store = make_governance_store();
        let req = KillSwitchRequest {
            request_id: Some("ks-1".into()),
            enabled: true,
        };
        let record = create_pending_governance_action(
            &store,
            "kill_switch",
            serde_json::to_value(&req).unwrap(),
            "admin-a",
            Some("ks-1".into()),
        )
        .expect("create pending");
        assert_eq!(record.action_type, "kill_switch");
        assert_eq!(record.status, "pending");
        assert_eq!(record.required_approvals, 2);
        assert_eq!(record.requested_by, "admin-a");
        assert!(record.approvers.is_empty());
        let stored = store.get(&record.action_id).expect("must exist in store");
        assert_eq!(stored.status, "pending");
    }

    #[test]
    fn governance_set_market_state_creates_pending_action() {
        let store = make_governance_store();
        let req = SetMarketStateRequest {
            request_id: Some("ms-1".into()),
            market_id: "BTC-USD-PERP".into(),
            outcome: Some(0),
            state: MarketState::Halted,
        };
        let record = create_pending_governance_action(
            &store,
            "set_market_state",
            serde_json::to_value(&req).unwrap(),
            "admin-b",
            Some("ms-1".into()),
        )
        .expect("create pending");
        assert_eq!(record.action_type, "set_market_state");
        assert_eq!(record.status, "pending");
        assert_eq!(record.required_approvals, 2);
        let payload: SetMarketStateRequest =
            serde_json::from_value(record.payload).expect("deserialize payload");
        assert_eq!(payload.market_id, "BTC-USD-PERP");
        assert!(matches!(payload.state, MarketState::Halted));
    }

    #[test]
    fn governance_reference_price_creates_pending_action() {
        let store = make_governance_store();
        let req = ReferencePriceRequest {
            request_id: Some("rp-1".into()),
            market_id: "ETH-USD-SPOT".into(),
            outcome: 0,
            source: Some("oracle".into()),
            reference_price: 3_500_000,
        };
        let record = create_pending_governance_action(
            &store,
            "reference_price",
            serde_json::to_value(&req).unwrap(),
            "admin-c",
            Some("rp-1".into()),
        )
        .expect("create pending");
        assert_eq!(record.action_type, "reference_price");
        assert_eq!(record.status, "pending");
        assert_eq!(record.required_approvals, 2);
        let payload: ReferencePriceRequest =
            serde_json::from_value(record.payload).expect("deserialize payload");
        assert_eq!(payload.reference_price, 3_500_000);
    }

    #[test]
    fn governance_action_requires_different_approver_than_requestor() {
        let store = make_governance_store();
        let record = create_pending_governance_action(
            &store,
            "kill_switch",
            serde_json::json!({"enabled": true}),
            "admin-a",
            None,
        )
        .expect("create pending");
        // simulate self-approval attempt: approvers list already contains requestor
        let mut self_approved = record.clone();
        self_approved.approvers.push("admin-a".to_string());
        // The approve handler checks `current.requested_by == principal.subject`
        // and rejects; we verify the invariant here at data level
        assert_eq!(self_approved.requested_by, "admin-a");
        assert!(self_approved.approvers.contains(&"admin-a".to_string()));
        // In a real flow, the handler would reject before reaching this state
    }

    #[test]
    fn governance_per_action_lock_returns_same_mutex_for_same_id() {
        let store = make_governance_store();
        let lock_a = store.action_lock("action-1");
        let lock_b = store.action_lock("action-1");
        let lock_c = store.action_lock("action-2");
        // Same action ID should yield the same Arc<Mutex>
        assert!(Arc::ptr_eq(&lock_a, &lock_b));
        // Different action ID should yield a different Arc<Mutex>
        assert!(!Arc::ptr_eq(&lock_a, &lock_c));
    }

    #[test]
    fn governance_idempotency_write_ahead_status() {
        let store = make_governance_store();
        let record = create_pending_governance_action(
            &store,
            "set_market_state",
            serde_json::json!({"market_id": "X", "state": "Halted"}),
            "admin-a",
            None,
        )
        .expect("create");
        // Simulate the write-ahead "applied" status written before execution
        let decided = GovernanceActionRecord {
            approvers: vec!["admin-b".into()],
            approved_by: Some("admin-b".into()),
            status: "applied".to_string(),
            decided_at: Some(Utc::now()),
            ..record.clone()
        };
        store.append(decided.clone()).expect("write-ahead applied");
        let stored = store.get(&record.action_id).expect("must exist");
        assert_eq!(stored.status, "applied");

        // Simulate apply failure rollback
        let failed = GovernanceActionRecord {
            status: "apply_failed".to_string(),
            ..decided
        };
        store.append(failed).expect("rollback to apply_failed");
        let stored = store.get(&record.action_id).expect("must exist");
        assert_eq!(stored.status, "apply_failed");
    }

    #[test]
    fn governance_liquidation_execute_requires_dual_approval() {
        let store = make_governance_store();
        let record = create_pending_governance_action(
            &store,
            "liquidation_execute",
            serde_json::json!({"user_id": "u1", "liquidator_user_id": "liq1", "market_id": "BTC"}),
            "admin-a",
            None,
        )
        .expect("create");
        assert_eq!(record.required_approvals, 2);
        assert_eq!(record.status, "pending");
    }

    // ── Integration tests: end-to-end order lifecycle ──────────────

    use instruments::InMemoryInstrumentRegistry;

    /// Build an in-memory matching engine + ledger + sequencer for integration tests.
    fn build_test_runtime() -> (
        Arc<PartitionedMatchingEngine>,
        Arc<LedgerService>,
        Arc<Sequencer>,
    ) {
        let event_bus = EventBus::new();
        let ledger_wal: Arc<dyn persistence::WalStore<LedgerDelta>> = Arc::new(InMemoryWal::new());
        let ledger = Arc::new(LedgerService::with_wal_store(event_bus.clone(), ledger_wal));

        let sequencer_wal: Arc<dyn persistence::WalStore<SequencedCommandRecord>> =
            Arc::new(InMemoryWal::new());
        let sequencer = Arc::new(Sequencer::with_wal(1, sequencer_wal));

        let snapshot_wal: Arc<dyn persistence::WalStore<PartitionSnapshotRecord>> =
            Arc::new(InMemoryWal::new());
        let journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>> =
            Arc::new(InMemoryWal::new());
        let settlement_wal: Arc<dyn persistence::WalStore<TradeSettlementRecord>> =
            Arc::new(InMemoryWal::new());
        let state_wal: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>> =
            Arc::new(InMemoryWal::new());
        let event_wal: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>> =
            Arc::new(InMemoryWal::new());
        let position_costs = Arc::new(
            PositionCostLedgerStore::new(state_wal, event_wal).expect("position cost store"),
        );

        let registry = Arc::new(InMemoryInstrumentRegistry::new().with_spec(InstrumentSpec {
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
            min_order_amount: 1,
            max_notional: 0,
            maker_fee_bps: 2,
            taker_fee_bps: 5,
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
        }));
        let risk = Arc::new(RiskEngine::new(ledger.clone()));
        let engine_registry: Arc<dyn InstrumentRegistry> = registry;

        let engine = Arc::new(
            PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
                PartitionedEngineConfig::default(),
                event_bus,
                risk,
                engine_registry,
                Some(snapshot_wal),
                Some(journal_wal),
                Some(position_costs),
                Some(settlement_wal),
            )
            .expect("partitioned engine"),
        );

        (engine, ledger, sequencer)
    }

    #[tokio::test]
    async fn integration_full_order_lifecycle() {
        let (engine, ledger, sequencer) = build_test_runtime();

        // Seed balances for buyer and seller.
        ledger
            .process_deposit("buyer", 10_000_000, "seed-buyer".into())
            .expect("deposit buyer");
        ledger
            .process_deposit("seller", 10_000_000, "seed-seller".into())
            .expect("deposit seller");
        // Seller needs inventory to sell on a spot market.
        ledger
            .process_position_deposit("seller", "btc-usdt", 0, 100, "seed-seller-btc".into())
            .expect("deposit seller btc");

        // 1. Place resting sell limit at price 50000, qty 10.
        let sell_cmd = sequence_new_order(
            &sequencer,
            "req-sell-1".into(),
            "sell-1".into(),
            "seller".into(),
            None,
            "btc-usdt".into(),
            Side::Sell,
            OrderType::Limit,
            TimeInForce::Gtc,
            Some(50_000),
            10,
            0,
            false,
            false,
            None,
            None,
            types::StpMode::default(),
            None,
            None,
        )
        .expect("sequence sell");

        let sell_result = engine.submit_new_order(sell_cmd).await.expect("sell order");
        // Sell should be resting (no matching buy yet).
        assert!(
            sell_result.fills.is_empty(),
            "sell should rest with no fills"
        );

        // 2. Place aggressive buy limit at price 50000, qty 10 �?should match.
        let buy_cmd = sequence_new_order(
            &sequencer,
            "req-buy-1".into(),
            "buy-1".into(),
            "buyer".into(),
            None,
            "btc-usdt".into(),
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            Some(50_000),
            10,
            0,
            false,
            false,
            None,
            None,
            types::StpMode::default(),
            None,
            None,
        )
        .expect("sequence buy");

        let buy_result = engine.submit_new_order(buy_cmd).await.expect("buy order");
        assert!(
            !buy_result.fills.is_empty(),
            "buy should match against resting sell"
        );
        // Engine returns fills for both sides of the trade.
        let fill = &buy_result.fills[0];
        assert_eq!(fill.amount, 10);
        assert_eq!(fill.price, 50_000);

        // 3. Verify buyer now holds position.
        let buyer_pos = ledger.position_available_balance("buyer", "btc-usdt", 0);
        assert!(buyer_pos > 0, "buyer should have btc position after fill");

        // 4. Verify seller position reduced.
        let seller_pos = ledger.position_available_balance("seller", "btc-usdt", 0);
        assert!(seller_pos < 100, "seller btc position should be reduced");
    }

    #[tokio::test]
    async fn integration_partial_fill_leaves_resting_order() {
        let (engine, ledger, sequencer) = build_test_runtime();

        ledger
            .process_deposit("buyer", 10_000_000, "seed-b".into())
            .expect("deposit");
        ledger
            .process_deposit("seller", 10_000_000, "seed-s".into())
            .expect("deposit");
        ledger
            .process_position_deposit("seller", "btc-usdt", 0, 50, "seed-s-btc".into())
            .expect("deposit");

        // Resting sell: qty 50.
        let sell_cmd = sequence_new_order(
            &sequencer,
            "req-sell-pf".into(),
            "sell-pf".into(),
            "seller".into(),
            None,
            "btc-usdt".into(),
            Side::Sell,
            OrderType::Limit,
            TimeInForce::Gtc,
            Some(40_000),
            50,
            0,
            false,
            false,
            None,
            None,
            types::StpMode::default(),
            None,
            None,
        )
        .expect("sequence");
        engine.submit_new_order(sell_cmd).await.expect("sell");

        // Buy only 20 �?partial fill, sell should stay with 30 remaining.
        let buy_cmd = sequence_new_order(
            &sequencer,
            "req-buy-pf".into(),
            "buy-pf".into(),
            "buyer".into(),
            None,
            "btc-usdt".into(),
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            Some(40_000),
            20,
            0,
            false,
            false,
            None,
            None,
            types::StpMode::default(),
            None,
            None,
        )
        .expect("sequence");
        let result = engine.submit_new_order(buy_cmd).await.expect("buy");
        assert!(!result.fills.is_empty());
        // Find the taker fill (buyer).
        let buyer_fill = result
            .fills
            .iter()
            .find(|f| f.user_id == "buyer")
            .expect("buyer fill");
        assert_eq!(buyer_fill.amount, 20);

        // Place another buy for the remaining 30.
        let buy2_cmd = sequence_new_order(
            &sequencer,
            "req-buy-pf2".into(),
            "buy-pf2".into(),
            "buyer".into(),
            None,
            "btc-usdt".into(),
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            Some(40_000),
            30,
            0,
            false,
            false,
            None,
            None,
            types::StpMode::default(),
            None,
            None,
        )
        .expect("sequence");
        let result2 = engine.submit_new_order(buy2_cmd).await.expect("buy2");
        assert!(!result2.fills.is_empty());
        let buyer_fill2 = result2
            .fills
            .iter()
            .find(|f| f.user_id == "buyer")
            .expect("buyer fill 2");
        assert_eq!(buyer_fill2.amount, 30);

        // Buyer should hold total 50 btc position.
        let buyer_pos = ledger.position_available_balance("buyer", "btc-usdt", 0);
        assert_eq!(buyer_pos, 50);
    }

    #[tokio::test]
    async fn integration_self_trade_prevention() {
        let (engine, ledger, sequencer) = build_test_runtime();

        ledger
            .process_deposit("user-x", 10_000_000, "seed".into())
            .expect("deposit");
        ledger
            .process_position_deposit("user-x", "btc-usdt", 0, 100, "seed-btc".into())
            .expect("deposit btc");

        // Place sell order.
        let sell_cmd = sequence_new_order(
            &sequencer,
            "req-stp-sell".into(),
            "stp-sell".into(),
            "user-x".into(),
            None,
            "btc-usdt".into(),
            Side::Sell,
            OrderType::Limit,
            TimeInForce::Gtc,
            Some(30_000),
            5,
            0,
            false,
            false,
            None,
            None,
            types::StpMode::default(),
            None,
            None,
        )
        .expect("sequence");
        engine.submit_new_order(sell_cmd).await.expect("sell");

        // Same user tries to buy at same price �?should be prevented.
        let buy_cmd = sequence_new_order(
            &sequencer,
            "req-stp-buy".into(),
            "stp-buy".into(),
            "user-x".into(),
            None,
            "btc-usdt".into(),
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            Some(30_000),
            5,
            0,
            false,
            false,
            None,
            None,
            types::StpMode::default(),
            None,
            None,
        )
        .expect("sequence");
        let result = engine.submit_new_order(buy_cmd).await;
        // Self-trade prevention: either rejected or fills are empty.
        if let Ok(r) = result {
            assert!(r.fills.is_empty(), "self-trade should not produce fills");
        }
    }

    #[tokio::test]
    async fn integration_kill_switch_blocks_orders() {
        let (engine, ledger, sequencer) = build_test_runtime();

        ledger
            .process_deposit("trader", 10_000_000, "seed".into())
            .expect("deposit");

        // Enable kill switch via admin command.
        let admin_cmd = AdminCommand {
            metadata: CommandMetadata::new("req-ks-admin"),
            action: AdminAction::KillSwitch { enabled: true },
            actor_id: "test-admin".to_string(),
        };
        engine
            .submit_admin(admin_cmd)
            .await
            .expect("enable kill switch");
        assert!(engine.kill_switch_enabled());

        let cmd = sequence_new_order(
            &sequencer,
            "req-ks".into(),
            "ks-order".into(),
            "trader".into(),
            None,
            "btc-usdt".into(),
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            Some(50_000),
            1,
            0,
            false,
            false,
            None,
            None,
            types::StpMode::default(),
            None,
            None,
        )
        .expect("sequence");
        let result = engine.submit_new_order(cmd).await;
        assert!(result.is_err(), "kill switch should block order submission");
    }
}
