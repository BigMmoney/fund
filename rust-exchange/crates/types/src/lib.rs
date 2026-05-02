//! Core domain types for the exchange platform.
//!
//! All monetary values (`price`, `amount`, `balance`, `fee`) are represented as
//! `i64` in the smallest indivisible unit of the asset (e.g. satoshis, cents).
//! Timestamps are always `DateTime<Utc>`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub mod order_trace;
pub use order_trace::{
    OrderTraceEvent, OrderTraceStage, TraceEmitter, ORDER_TRACE_SCHEMA_VERSION,
};

/// A user or system account with a balance tracked in the ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub balance: i64,
    pub version: i64,
    pub account_type: String,
    /// Account-level margin mode governing collateral and risk calculation.
    #[serde(default)]
    pub account_mode: AccountMode,
}

/// A single debit/credit movement between two accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub debit_account: String,
    pub credit_account: String,
    /// Amount in smallest indivisible units (always positive).
    pub amount: i64,
    pub op_id: String,
    pub timestamp: DateTime<Utc>,
}

/// Atomic batch of ledger entries committed together under a single op_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerDelta {
    pub op_id: String,
    pub entries: Vec<LedgerEntry>,
    pub timestamp: DateTime<Utc>,
}

/// A user's trading intent (simplified limit-order submission).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: String,
    pub user_id: String,
    pub market_id: String,
    pub side: Side,
    /// Price in smallest units.
    pub price: i64,
    /// Quantity in lot units.
    pub amount: i64,
    pub outcome: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub status: IntentStatus,
}

/// Order side: buy (bid) or sell (ask).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

/// Order type: limit (price-specified) or market (immediate fill at best available).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Limit,
    Market,
    StopMarket,
    StopLimit,
    TakeProfitMarket,
    TakeProfitLimit,
}

impl OrderType {
    /// Whether this order type is conditional (stop/take-profit).
    pub fn is_conditional(self) -> bool {
        matches!(
            self,
            Self::StopMarket | Self::StopLimit | Self::TakeProfitMarket | Self::TakeProfitLimit
        )
    }

    /// The underlying execution type once triggered.
    pub fn triggered_type(self) -> Self {
        match self {
            Self::StopMarket | Self::TakeProfitMarket => Self::Market,
            Self::StopLimit | Self::TakeProfitLimit => Self::Limit,
            other => other,
        }
    }
}

/// Trigger source for conditional orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    /// Trigger when last trade price crosses threshold (default).
    LastPrice,
    /// Trigger when mark price crosses threshold.
    MarkPrice,
    /// Trigger when index price crosses threshold.
    IndexPrice,
}

impl Default for TriggerType {
    fn default() -> Self {
        Self::LastPrice
    }
}

/// Self-trade prevention mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StpMode {
    /// Reject the incoming order entirely if it would self-trade (default).
    CancelTaker,
    /// Cancel the resting (maker) order(s) instead.
    CancelMaker,
    /// Cancel both the incoming and the resting order(s).
    CancelBoth,
}

impl Default for StpMode {
    fn default() -> Self {
        Self::CancelTaker
    }
}

/// Time-in-force policy for order lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeInForce {
    /// Good-til-cancelled: rests on the book until explicitly cancelled.
    Gtc,
    /// Immediate-or-cancel: fill what's possible, cancel the rest.
    Ioc,
    /// Fill-or-kill: must fill completely or reject entirely.
    Fok,
    /// Good-til-date: rests until `expires_at` timestamp.
    Gtd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntentStatus {
    Pending,
    Filled,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    PendingNew,
    Active,
    PartiallyFilled,
    PendingCancel,
    Cancelled,
    Filled,
    Rejected,
    Expired,
    PendingReplace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketState {
    /// Pre-open session — no matching, order entry may be allowed.
    PreOpen,
    Normal,
    Stress,
    AuctionCall,
    CancelOnly,
    Halted,
    /// Maintenance window — no order entry, positions held.
    Maintenance,
    /// Market permanently closed / delisted.
    Closed,
}

impl MarketState {
    /// Returns `true` if transitioning from `self` to `target` is a valid state change.
    /// Enforces a state-machine: e.g. Closed → Normal is not allowed without PreOpen.
    pub fn can_transition_to(self, target: MarketState) -> bool {
        use MarketState::*;
        matches!(
            (self, target),
            // From PreOpen
            (PreOpen, Normal) | (PreOpen, AuctionCall) | (PreOpen, Halted) | (PreOpen, Closed)
            // From Normal
            | (Normal, Stress) | (Normal, AuctionCall) | (Normal, CancelOnly) | (Normal, Halted) | (Normal, Maintenance) | (Normal, Closed)
            // From Stress
            | (Stress, Normal) | (Stress, AuctionCall) | (Stress, CancelOnly) | (Stress, Halted) | (Stress, Closed)
            // From AuctionCall
            | (AuctionCall, Normal) | (AuctionCall, Halted) | (AuctionCall, CancelOnly)
            // From CancelOnly
            | (CancelOnly, Normal) | (CancelOnly, Halted) | (CancelOnly, Closed)
            // From Halted
            | (Halted, PreOpen) | (Halted, Normal) | (Halted, CancelOnly) | (Halted, Closed)
            // From Maintenance
            | (Maintenance, PreOpen) | (Maintenance, Normal) | (Maintenance, Closed)
            // From Closed: terminal — can only reopen via PreOpen
            | (Closed, PreOpen)
            // Identity transitions are always allowed
            | (PreOpen, PreOpen) | (Normal, Normal) | (Stress, Stress) | (AuctionCall, AuctionCall)
            | (CancelOnly, CancelOnly) | (Halted, Halted) | (Maintenance, Maintenance) | (Closed, Closed)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchingMode {
    ContinuousClob,
    FrequentBatchAuction { window_ms: u64 },
}

/// Classification of a tradable instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    Spot,
    Margin,
    Perpetual,
    Future,
    Option,
}

impl InstrumentKind {
    pub fn is_derivative(self) -> bool {
        !matches!(self, Self::Spot)
    }

    pub fn supports_funding(self) -> bool {
        matches!(self, Self::Perpetual)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarginMode {
    Isolated,
    Cross,
}

/// Account-level margin mode (OKX-style tiered model).
/// Governs how collateral and risk are calculated across positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountMode {
    /// Simple spot-only account — no margin, no leverage.
    #[default]
    Simple,
    /// Single-currency margin: one settlement asset (e.g. USDC).
    SingleCurrencyMargin,
    /// Multi-currency margin: collateral in multiple assets with haircuts.
    MultiCurrencyMargin,
    /// Portfolio margin: cross-instrument netting with VaR-based requirements.
    PortfolioMargin,
}

/// A supported collateral asset with its risk parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollateralAsset {
    /// Asset identifier (e.g. "BTC", "ETH", "USDC").
    pub asset_id: String,
    /// Haircut in basis points (e.g. 500 = 5% haircut → 95% collateral value).
    /// USDC/stablecoins would be 0 (100% value).
    pub haircut_bps: i64,
    /// Whether this asset can be used as collateral.
    pub eligible: bool,
    /// Maximum amount of this asset counted as collateral (0 = unlimited).
    #[serde(default)]
    pub concentration_cap: i64,
}

/// Circuit breaker configuration for automatic volatility-based state transitions.
/// When realized volatility exceeds the threshold, the engine transitions the
/// market through degradation states (Normal → Stress → CancelOnly → Halted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Volatility threshold in basis points to trigger Stress mode.
    /// E.g. 800 = 8% realized volatility triggers stress.
    pub stress_threshold_bps: i64,
    /// Volatility threshold in basis points to trigger CancelOnly mode.
    pub cancel_only_threshold_bps: i64,
    /// Volatility threshold in basis points to trigger Halt.
    pub halt_threshold_bps: i64,
    /// Cooldown in seconds before auto-recovering from a breaker state.
    /// 0 = manual recovery only.
    pub cooldown_secs: u64,
    /// Number of trades in the lookback window for volatility calculation.
    #[serde(default = "default_vol_lookback")]
    pub vol_lookback_trades: usize,
}

fn default_vol_lookback() -> usize {
    50
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            stress_threshold_bps: 500,
            cancel_only_threshold_bps: 800,
            halt_threshold_bps: 1200,
            cooldown_secs: 300,
            vol_lookback_trades: default_vol_lookback(),
        }
    }
}

/// Market-maker protection configuration per instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketMakerProtection {
    /// Maximum delta (net position change) within a rolling window before auto-cancel.
    /// 0 = disabled.
    pub max_delta_qty: i64,
    /// Maximum notional filled within a rolling window before auto-cancel.
    /// 0 = disabled.
    pub max_notional_window: i64,
    /// Rolling window duration in seconds.
    pub window_secs: u64,
}

impl Default for MarketMakerProtection {
    fn default() -> Self {
        Self {
            max_delta_qty: 0,
            max_notional_window: 0,
            window_secs: 1,
        }
    }
}

// ── Formalized Rule Types (Coinbase / Kraken maturity) ──────────────────

/// Volume-based fee tier (e.g. VIP-0 … VIP-9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeTier {
    /// Minimum 30-day trailing volume to qualify (quote units).
    pub min_volume: i64,
    /// Maker fee in basis points (negative = rebate).
    pub maker_fee_bps: i64,
    /// Taker fee in basis points.
    pub taker_fee_bps: i64,
}

/// Exchange-wide fee schedule with tiered maker/taker rates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeSchedule {
    /// Human-readable name (e.g. "Standard", "VIP").
    pub name: String,
    /// Tiers sorted ascending by `min_volume`.
    pub tiers: Vec<FeeTier>,
    /// Withdrawal fee in absolute units per withdrawal (0 = free).
    #[serde(default)]
    pub withdrawal_fee: i64,
    /// Whether market-maker rebates are enabled.
    #[serde(default)]
    pub mm_rebate_enabled: bool,
}

impl FeeSchedule {
    /// Resolve maker/taker fee basis points for a given 30-day trading volume.
    /// Picks the highest tier whose `min_volume` ≤ `volume_30d`.
    /// Returns the instrument-level defaults when no tier qualifies.
    pub fn resolve(&self, volume_30d: i64, default_maker: i64, default_taker: i64) -> (i64, i64) {
        let mut maker = default_maker;
        let mut taker = default_taker;
        for tier in &self.tiers {
            if volume_30d >= tier.min_volume {
                maker = tier.maker_fee_bps;
                taker = tier.taker_fee_bps;
            } else {
                break;
            }
        }
        (maker, taker)
    }
}

/// Formalized margin requirement rule for an instrument class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarginRule {
    /// Initial margin in basis points (e.g. 1000 = 10%).
    pub initial_margin_bps: i64,
    /// Maintenance margin in basis points (e.g. 500 = 5%).
    pub maintenance_margin_bps: i64,
    /// Liquidation penalty deducted from remaining collateral (bps).
    pub liquidation_penalty_bps: i64,
    /// Maximum allowed leverage (1 = no leverage).
    pub max_leverage: u32,
    /// Whether auto-deleverage is enabled when insurance fund is exhausted.
    #[serde(default)]
    pub auto_deleverage_enabled: bool,
}

/// Graduated margin tier: higher margin requirements for larger positions.
/// Deribit/Binance style — positions exceeding `notional_up_to` step up to this tier's rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarginTier {
    /// Maximum notional value for this tier (exclusive upper bound).
    /// Use i64::MAX or 0 for the final unbounded tier.
    pub notional_up_to: i64,
    /// Initial margin rate in basis points for this tier.
    pub initial_margin_bps: i64,
    /// Maintenance margin rate in basis points for this tier.
    pub maintenance_margin_bps: i64,
    /// Maximum leverage allowed at this tier.
    pub max_leverage: u32,
}

/// Formalized liquidation process parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiquidationRule {
    /// Penalty basis points charged to the liquidated user.
    pub penalty_bps: i64,
    /// Share of penalty directed to the insurance fund (bps out of 10_000).
    pub insurance_fund_share_bps: i64,
    /// Whether auto-deleverage is triggered on insurance fund exhaustion.
    #[serde(default)]
    pub adl_enabled: bool,
    /// Maximum duration in seconds for a liquidation auction (0 = immediate).
    #[serde(default)]
    pub auction_duration_secs: u64,
    /// Whether partial liquidation is attempted before full liquidation.
    #[serde(default = "default_true")]
    pub partial_liquidation_enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Rate limiting rule per session / API key / IP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitRule {
    /// Maximum new orders per second per session.
    pub max_orders_per_second: u32,
    /// Maximum cancel requests per second per session.
    pub max_cancels_per_second: u32,
    /// Maximum total WebSocket/REST messages per second.
    pub max_messages_per_second: u32,
    /// Weight cost of a single NewOrder request.
    #[serde(default = "default_order_weight")]
    pub order_weight: u32,
    /// Weight cost of a single CancelOrder request.
    #[serde(default = "default_cancel_weight")]
    pub cancel_weight: u32,
}

fn default_order_weight() -> u32 {
    1
}

fn default_cancel_weight() -> u32 {
    1
}

/// Formalized specification of allowed order behaviour per instrument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderTypeRule {
    /// Supported order types on this instrument.
    pub allowed_order_types: Vec<OrderType>,
    /// Supported time-in-force policies.
    pub allowed_tif: Vec<TimeInForce>,
    /// Whether post-only (maker-only) execution is permitted.
    #[serde(default)]
    pub post_only_allowed: bool,
    /// Whether reduce-only orders are permitted.
    #[serde(default)]
    pub reduce_only_allowed: bool,
    /// Whether iceberg (display quantity) orders are supported.
    #[serde(default)]
    pub iceberg_allowed: bool,
    /// Whether conditional (stop / take-profit) orders are supported.
    #[serde(default)]
    pub conditional_allowed: bool,
}

/// Option type for options contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionType {
    Call,
    Put,
}

/// Exercise style for options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExerciseStyle {
    /// Can only be exercised at expiry.
    European,
    /// Can be exercised any time before expiry.
    American,
}

impl Default for ExerciseStyle {
    fn default() -> Self {
        Self::European
    }
}

/// Option-specific contract parameters. Only present for `InstrumentKind::Option`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionSpec {
    /// Strike price in quote-asset smallest units.
    pub strike_price: i64,
    /// Call or Put.
    pub option_type: OptionType,
    /// European (default) or American.
    #[serde(default)]
    pub exercise_style: ExerciseStyle,
}

/// Expiry/settlement parameters for dated contracts (Futures, Options).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpirySpec {
    /// Expiration datetime (UTC). After this, no new orders are accepted.
    pub expiry_at: DateTime<Utc>,
    /// Settlement price source identifier (e.g. "index:btc-usd", "twap:30m").
    #[serde(default)]
    pub settlement_price_source: String,
    /// Whether physical delivery is required (vs cash-settled).
    #[serde(default)]
    pub physical_delivery: bool,
}

/// Full specification of a tradable instrument including tick/lot sizes, fees, and risk params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentSpec {
    pub instrument_id: String,
    pub kind: InstrumentKind,
    /// Base asset of the trading pair (e.g. "BTC" in BTC/USDC).
    #[serde(default)]
    pub base_asset: String,
    pub quote_asset: String,
    pub margin_mode: Option<MarginMode>,
    pub max_leverage: Option<u32>,
    pub tick_size: i64,
    pub lot_size: i64,
    pub price_band_bps: i64,
    pub risk_policy_id: String,
    /// Minimum order quantity (lot units). Orders below this are rejected.
    #[serde(default)]
    pub min_order_amount: i64,
    /// Maximum single-order notional (price × amount). 0 = no limit.
    #[serde(default)]
    pub max_notional: i64,
    /// Maker fee in basis points (e.g. 5 = 0.05%).
    #[serde(default)]
    pub maker_fee_bps: i64,
    /// Taker fee in basis points (e.g. 10 = 0.10%).
    #[serde(default)]
    pub taker_fee_bps: i64,
    /// Maximum position notional per user (price × qty). 0 = no limit.
    #[serde(default)]
    pub max_position_notional: i64,
    /// Maintenance margin requirement in basis points for derivatives.
    /// E.g. 500 = 5% maintenance margin. 0 = use default.
    #[serde(default)]
    pub maintenance_margin_bps: i64,
    /// Contract multiplier for futures/options (e.g. 100 for 100x contract).
    /// 0 or 1 = standard 1:1 contract.
    #[serde(default = "default_contract_multiplier")]
    pub contract_multiplier: i64,
    /// Funding rate settlement interval in seconds (e.g. 28800 = 8h).
    /// 0 = no periodic funding (spot/margin). Only meaningful for perpetuals.
    #[serde(default)]
    pub funding_interval_secs: u64,
    /// Instrument trading status.
    #[serde(default)]
    pub status: InstrumentStatus,
    /// Circuit breaker configuration for automated volatility-driven state transitions.
    #[serde(default)]
    pub circuit_breaker: Option<CircuitBreakerConfig>,
    /// Market-maker protection parameters. Only applies to MM-tagged orders.
    #[serde(default)]
    pub mm_protection: Option<MarketMakerProtection>,
    /// Maximum single-order amount (lots). 0 = no limit.
    /// Fat-finger guard: orders exceeding this are rejected outright.
    #[serde(default)]
    pub max_order_amount: i64,
    /// Per-instrument order type/TIF validation rules. `None` = all types allowed.
    #[serde(default)]
    pub order_type_rule: Option<OrderTypeRule>,
    /// Formalized margin requirement rule. `None` = use legacy field-based params.
    #[serde(default)]
    pub margin_rule: Option<MarginRule>,
    /// Formalized liquidation rule. `None` = use defaults.
    #[serde(default)]
    pub liquidation_rule: Option<LiquidationRule>,
    /// Volume-based fee schedule. When present, overrides flat `maker_fee_bps`/`taker_fee_bps`
    /// based on 30-day trailing volume.
    #[serde(default)]
    pub fee_schedule: Option<FeeSchedule>,
    /// Graduated margin tiers (Deribit/Binance style). When present, IM/MM scales with position size.
    /// Must be sorted by `notional_up_to` ascending. Empty or `None` = flat margin from MarginRule.
    #[serde(default)]
    pub margin_tiers: Option<Vec<MarginTier>>,
    /// Expiry specification for dated instruments (Future, Option). `None` = perpetual/spot.
    #[serde(default)]
    pub expiry: Option<ExpirySpec>,
    /// Option contract specification (strike, call/put, exercise). `None` = not an option.
    #[serde(default)]
    pub option_spec: Option<OptionSpec>,
    /// Settlement/quote currency override. `None` = default (USDC).
    #[serde(default)]
    pub settlement_currency: Option<String>,
}

fn default_contract_multiplier() -> i64 {
    1
}

impl InstrumentSpec {
    /// Validate instrument spec after deserialization.
    /// Catches cases where `#[serde(default)]` masked missing critical fields
    /// or where malicious input sets unsafe values (e.g. tick_size=0).
    pub fn validate(&self) -> Result<(), String> {
        if self.instrument_id.is_empty() {
            return Err("instrument_id must not be empty".into());
        }
        if self.quote_asset.is_empty() {
            return Err("quote_asset must not be empty".into());
        }
        if self.tick_size <= 0 {
            return Err(format!(
                "tick_size must be positive, got {}",
                self.tick_size
            ));
        }
        if self.lot_size <= 0 {
            return Err(format!("lot_size must be positive, got {}", self.lot_size));
        }
        if self.price_band_bps <= 0 {
            return Err(format!(
                "price_band_bps must be positive, got {}",
                self.price_band_bps
            ));
        }
        if self.risk_policy_id.is_empty() {
            return Err("risk_policy_id must not be empty".into());
        }
        // Optional limits: 0 means disabled, which is safe
        if self.min_order_amount < 0 {
            return Err("min_order_amount must not be negative".into());
        }
        if self.max_notional < 0 {
            return Err("max_notional must not be negative".into());
        }
        if self.max_order_amount < 0 {
            return Err("max_order_amount must not be negative".into());
        }
        // Fee sanity: negative fees = rebates, but cap at -1000 bps (-10%) to
        // prevent catastrophic misconfiguration.
        if self.maker_fee_bps < -1000 {
            return Err("maker_fee_bps must not exceed -1000 bps (-10% rebate cap)".into());
        }
        if self.taker_fee_bps < -1000 {
            return Err("taker_fee_bps must not exceed -1000 bps (-10% rebate cap)".into());
        }
        // Margin tiers must be sorted ascending
        if let Some(ref tiers) = self.margin_tiers {
            for w in tiers.windows(2) {
                if w[0].notional_up_to >= w[1].notional_up_to && w[1].notional_up_to != 0 {
                    return Err("margin_tiers must be sorted by notional_up_to ascending".into());
                }
            }
        }
        Ok(())
    }
}

/// Trading status of an instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentStatus {
    /// Active and available for trading (default).
    #[default]
    Active,
    /// Temporarily halted — no new orders accepted.
    Halted,
    /// Settling — contract expired, positions being settled at settlement price.
    Settling,
    /// Permanently delisted — positions may still exist for settlement.
    Delisted,
}

/// Structured API error code returned alongside human-readable messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApiErrorCode {
    InvalidOrder,
    DuplicateOrderId,
    OrderNotFound,
    MarketClosed,
    QueueFull,
    KillSwitchActive,
    PriceBandBreached,
    InsufficientLiquidity,
    SelfTradePrevented,
    LedgerError,
    PersistenceError,
    RateLimited,
    Unauthorized,
    TickSizeViolation,
    LotSizeViolation,
    BelowMinAmount,
    ExceedsMaxNotional,
    AccountFrozen,
    InvalidStateTransition,
    InternalError,
    FatFingerRejected,
    MarketKillSwitchActive,
    CircuitBreakerTriggered,
    MarketMakerProtectionTriggered,
    // ── Coinbase / Kraken parity codes ──
    InsufficientMargin,
    InsufficientFunds,
    ExceedsMaxLeverage,
    ExceedsPositionLimit,
    ReduceOnlyViolation,
    PostOnlyWouldTrade,
    InvalidTimeInForce,
    InvalidTriggerPrice,
    OrderExpired,
    MarketNotFound,
    InstrumentHalted,
    InstrumentDelisted,
    InvalidAmendment,
    SessionExpired,
    IpRateLimited,
    MaintenanceMode,
    AuthBanned,
    BruteForceDetected,
    InvalidAccountMode,
    CollateralIneligible,
    LiquidationInProgress,
    ExpirySettlementActive,
    AggregateExposureExceeded,
}

impl fmt::Display for ApiErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiErrorCode::InvalidOrder => write!(f, "INVALID_ORDER"),
            ApiErrorCode::DuplicateOrderId => write!(f, "DUPLICATE_ORDER_ID"),
            ApiErrorCode::OrderNotFound => write!(f, "ORDER_NOT_FOUND"),
            ApiErrorCode::MarketClosed => write!(f, "MARKET_CLOSED"),
            ApiErrorCode::QueueFull => write!(f, "QUEUE_FULL"),
            ApiErrorCode::KillSwitchActive => write!(f, "KILL_SWITCH_ACTIVE"),
            ApiErrorCode::PriceBandBreached => write!(f, "PRICE_BAND_BREACHED"),
            ApiErrorCode::InsufficientLiquidity => write!(f, "INSUFFICIENT_LIQUIDITY"),
            ApiErrorCode::SelfTradePrevented => write!(f, "SELF_TRADE_PREVENTED"),
            ApiErrorCode::LedgerError => write!(f, "LEDGER_ERROR"),
            ApiErrorCode::PersistenceError => write!(f, "PERSISTENCE_ERROR"),
            ApiErrorCode::RateLimited => write!(f, "RATE_LIMITED"),
            ApiErrorCode::Unauthorized => write!(f, "UNAUTHORIZED"),
            ApiErrorCode::TickSizeViolation => write!(f, "TICK_SIZE_VIOLATION"),
            ApiErrorCode::LotSizeViolation => write!(f, "LOT_SIZE_VIOLATION"),
            ApiErrorCode::BelowMinAmount => write!(f, "BELOW_MIN_AMOUNT"),
            ApiErrorCode::ExceedsMaxNotional => write!(f, "EXCEEDS_MAX_NOTIONAL"),
            ApiErrorCode::AccountFrozen => write!(f, "ACCOUNT_FROZEN"),
            ApiErrorCode::InvalidStateTransition => write!(f, "INVALID_STATE_TRANSITION"),
            ApiErrorCode::InternalError => write!(f, "INTERNAL_ERROR"),
            ApiErrorCode::FatFingerRejected => write!(f, "FAT_FINGER_REJECTED"),
            ApiErrorCode::MarketKillSwitchActive => write!(f, "MARKET_KILL_SWITCH_ACTIVE"),
            ApiErrorCode::CircuitBreakerTriggered => write!(f, "CIRCUIT_BREAKER_TRIGGERED"),
            ApiErrorCode::MarketMakerProtectionTriggered => {
                write!(f, "MARKET_MAKER_PROTECTION_TRIGGERED")
            }
            ApiErrorCode::InsufficientMargin => write!(f, "INSUFFICIENT_MARGIN"),
            ApiErrorCode::InsufficientFunds => write!(f, "INSUFFICIENT_FUNDS"),
            ApiErrorCode::ExceedsMaxLeverage => write!(f, "EXCEEDS_MAX_LEVERAGE"),
            ApiErrorCode::ExceedsPositionLimit => write!(f, "EXCEEDS_POSITION_LIMIT"),
            ApiErrorCode::ReduceOnlyViolation => write!(f, "REDUCE_ONLY_VIOLATION"),
            ApiErrorCode::PostOnlyWouldTrade => write!(f, "POST_ONLY_WOULD_TRADE"),
            ApiErrorCode::InvalidTimeInForce => write!(f, "INVALID_TIME_IN_FORCE"),
            ApiErrorCode::InvalidTriggerPrice => write!(f, "INVALID_TRIGGER_PRICE"),
            ApiErrorCode::OrderExpired => write!(f, "ORDER_EXPIRED"),
            ApiErrorCode::MarketNotFound => write!(f, "MARKET_NOT_FOUND"),
            ApiErrorCode::InstrumentHalted => write!(f, "INSTRUMENT_HALTED"),
            ApiErrorCode::InstrumentDelisted => write!(f, "INSTRUMENT_DELISTED"),
            ApiErrorCode::InvalidAmendment => write!(f, "INVALID_AMENDMENT"),
            ApiErrorCode::SessionExpired => write!(f, "SESSION_EXPIRED"),
            ApiErrorCode::IpRateLimited => write!(f, "IP_RATE_LIMITED"),
            ApiErrorCode::MaintenanceMode => write!(f, "MAINTENANCE_MODE"),
            ApiErrorCode::AuthBanned => write!(f, "AUTH_BANNED"),
            ApiErrorCode::BruteForceDetected => write!(f, "BRUTE_FORCE_DETECTED"),
            ApiErrorCode::InvalidAccountMode => write!(f, "INVALID_ACCOUNT_MODE"),
            ApiErrorCode::CollateralIneligible => write!(f, "COLLATERAL_INELIGIBLE"),
            ApiErrorCode::LiquidationInProgress => write!(f, "LIQUIDATION_IN_PROGRESS"),
            ApiErrorCode::ExpirySettlementActive => write!(f, "EXPIRY_SETTLEMENT_ACTIVE"),
            ApiErrorCode::AggregateExposureExceeded => write!(f, "AGGREGATE_EXPOSURE_EXCEEDED"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandLifecycle {
    Received,
    Sequenced,
    WalAppended,
    RiskReserved,
    Routed,
    PartitionAccepted,
    Executed,
    Settled,
    Completed,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandMetadata {
    pub request_id: String,
    pub command_seq: Option<u64>,
    pub lifecycle: CommandLifecycle,
    pub received_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CommandMetadata {
    pub fn new(request_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            request_id: request_id.into(),
            command_seq: None,
            lifecycle: CommandLifecycle::Received,
            received_at: now,
            updated_at: now,
        }
    }

    pub fn advance(&mut self, lifecycle: CommandLifecycle) {
        self.lifecycle = lifecycle;
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewOrderCommand {
    pub metadata: CommandMetadata,
    pub client_order_id: String,
    pub user_id: String,
    pub session_id: Option<String>,
    pub market_id: String,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub price: Option<i64>,
    pub amount: i64,
    pub outcome: i32,
    pub post_only: bool,
    pub reduce_only: bool,
    #[serde(default)]
    pub leverage: Option<u32>,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub stp_mode: StpMode,
    #[serde(default)]
    pub trigger_price: Option<i64>,
    #[serde(default)]
    pub trigger_type: Option<TriggerType>,
    /// Iceberg order: visible (display) quantity. Total quantity is `amount`.
    /// When the visible part fills, the engine auto-replenishes from the hidden reserve.
    /// `None` or `0` means the entire amount is visible (standard order).
    #[serde(default)]
    pub display_qty: Option<i64>,
    /// Minimum quantity per individual fill. If a potential match would produce
    /// a fill smaller than this, the fill is skipped (the resting order remains).
    /// `None` or `0` means no minimum.
    #[serde(default)]
    pub min_fill_qty: Option<i64>,
    /// Self-trade prevention group identifier (firm/sub-account level).
    /// When set, STP checks match on `stp_group_id` in addition to `user_id`.
    #[serde(default)]
    pub stp_group_id: Option<String>,
    /// Whether the submitter is a designated market maker for this instrument.
    /// Enables market-maker protections (delta/notional circuit breakers).
    #[serde(default)]
    pub is_market_maker: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrderCommand {
    pub metadata: CommandMetadata,
    pub user_id: String,
    pub market_id: String,
    pub outcome: Option<i32>,
    pub order_id: String,
    pub client_order_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceOrderCommand {
    pub metadata: CommandMetadata,
    pub user_id: String,
    pub market_id: String,
    pub outcome: Option<i32>,
    pub order_id: String,
    pub new_client_order_id: Option<String>,
    pub new_price: Option<i64>,
    pub new_amount: Option<i64>,
    pub new_time_in_force: Option<TimeInForce>,
    pub post_only: Option<bool>,
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub new_leverage: Option<u32>,
    pub new_expires_at: Option<DateTime<Utc>>,
    /// Replace iceberg display quantity. `None` keeps current value.
    #[serde(default)]
    pub new_display_qty: Option<i64>,
    /// Replace minimum fill quantity. `None` keeps current value.
    #[serde(default)]
    pub new_min_fill_qty: Option<i64>,
    /// Replace trigger price for conditional orders. `None` keeps current value.
    #[serde(default)]
    pub new_trigger_price: Option<i64>,
    /// Replace trigger type for conditional orders. `None` keeps current value.
    #[serde(default)]
    pub new_trigger_type: Option<TriggerType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MassCancelByUserCommand {
    pub metadata: CommandMetadata,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MassCancelBySessionCommand {
    pub metadata: CommandMetadata,
    pub user_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MassCancelByMarketCommand {
    pub metadata: CommandMetadata,
    pub market_id: String,
    /// Cancel only a specific side. `None` cancels both sides.
    #[serde(default)]
    pub side: Option<Side>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminAction {
    SetMarketState {
        market_id: String,
        outcome: Option<i32>,
        state: MarketState,
    },
    KillSwitch {
        enabled: bool,
    },
    /// Per-market kill switch — halts a specific market without affecting others.
    MarketKillSwitch {
        market_id: String,
        enabled: bool,
    },
    /// Update an instrument's specification (e.g. leverage, fees, status).
    UpdateInstrument {
        spec: Box<InstrumentSpec>,
    },
    /// Freeze a user account — cancel all orders, reject new orders.
    FreezeAccount {
        user_id: String,
        reason: String,
    },
    /// Unfreeze a previously frozen account.
    UnfreezeAccount {
        user_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminCommand {
    pub metadata: CommandMetadata,
    pub actor_id: String,
    pub action: AdminAction,
}

/// Configuration for insurance fund behaviour during liquidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsuranceFundConfig {
    /// Minimum reserve the insurance fund must retain (absolute amount).
    /// Liquidation will not draw the fund below this threshold.
    #[serde(default)]
    pub min_reserve: i64,
    /// Percentage of liquidation penalty directed to the insurance fund (0–100).
    /// Remainder goes to the liquidator. Default: 10 (i.e. 10%).
    #[serde(default = "default_insurance_capture_pct")]
    pub penalty_capture_pct: i64,
}

fn default_insurance_capture_pct() -> i64 {
    0
}

impl Default for InsuranceFundConfig {
    fn default() -> Self {
        Self {
            min_reserve: 0,
            penalty_capture_pct: default_insurance_capture_pct(),
        }
    }
}

/// Per-user risk limits, allowing differentiated position/notional caps
/// (e.g. institutional vs retail).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserRiskLimits {
    /// Maximum absolute position notional across all instruments (0 = unlimited).
    #[serde(default)]
    pub max_total_notional: i64,
    /// Maximum absolute position notional per instrument (0 = unlimited).
    #[serde(default)]
    pub max_instrument_notional: i64,
    /// Maximum number of concurrent open orders (0 = unlimited).
    #[serde(default)]
    pub max_open_orders: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalRole {
    User,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedPrincipal {
    pub subject: String,
    pub role: PrincipalRole,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RiskReserveIds {
    pub cash_op_id: Option<String>,
    pub position_op_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskCheckedCommand {
    pub command_seq: u64,
    pub reserve_ids: RiskReserveIds,
    pub principal: AuthenticatedPrincipal,
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReplayCursor {
    pub snapshot_seq: Option<u64>,
    pub next_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    NewOrder(NewOrderCommand),
    CancelOrder(CancelOrderCommand),
    ReplaceOrder(ReplaceOrderCommand),
    MassCancelByUser(MassCancelByUserCommand),
    MassCancelBySession(MassCancelBySessionCommand),
    MassCancelByMarket(MassCancelByMarketCommand),
    Admin(AdminCommand),
}

impl Command {
    pub fn metadata(&self) -> &CommandMetadata {
        match self {
            Command::NewOrder(command) => &command.metadata,
            Command::CancelOrder(command) => &command.metadata,
            Command::ReplaceOrder(command) => &command.metadata,
            Command::MassCancelByUser(command) => &command.metadata,
            Command::MassCancelBySession(command) => &command.metadata,
            Command::MassCancelByMarket(command) => &command.metadata,
            Command::Admin(command) => &command.metadata,
        }
    }

    pub fn metadata_mut(&mut self) -> &mut CommandMetadata {
        match self {
            Command::NewOrder(command) => &mut command.metadata,
            Command::CancelOrder(command) => &mut command.metadata,
            Command::ReplaceOrder(command) => &mut command.metadata,
            Command::MassCancelByUser(command) => &mut command.metadata,
            Command::MassCancelBySession(command) => &mut command.metadata,
            Command::MassCancelByMarket(command) => &mut command.metadata,
            Command::Admin(command) => &mut command.metadata,
        }
    }

    pub fn request_id(&self) -> &str {
        &self.metadata().request_id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub market_id: String,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub price: i64,
    pub amount: i64,
    pub filled_amount: i64,
    pub outcome: i32,
    pub status: OrderState,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub client_order_id: Option<String>,
    #[serde(default)]
    pub trigger_price: Option<i64>,
    #[serde(default)]
    pub trigger_type: Option<TriggerType>,
    /// Cumulative fees paid in quote units.
    #[serde(default)]
    pub cumulative_fee: i64,
    /// Average fill price (volume-weighted).
    #[serde(default)]
    pub avg_fill_price: Option<i64>,
}

/// A single trade execution (fill) resulting from order matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub id: String,
    pub intent_id: String,
    pub user_id: String,
    pub market_id: String,
    pub side: Side,
    pub price: i64,
    pub amount: i64,
    pub outcome: i32,
    pub timestamp: DateTime<Utc>,
    pub op_id: String,
    /// Fee charged in quote units (always non-negative).
    #[serde(default)]
    pub fee: i64,
    /// Fee rate applied in basis points.
    #[serde(default)]
    pub fee_bps: i64,
    /// True if this fill was a maker (resting order), false if taker (incoming order).
    #[serde(default)]
    pub is_maker: bool,
    /// The side that initiated (aggressed) the trade — always the incoming order's side.
    #[serde(default)]
    pub aggressor_side: Option<Side>,
    /// Sequence number of this fill within the matching cycle (0-based).
    #[serde(default)]
    pub fill_index: u32,
    /// Settlement status of this fill.
    #[serde(default)]
    pub settlement_status: SettlementStatus,
}

/// Settlement lifecycle for a trade fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SettlementStatus {
    /// Settlement not yet attempted (default).
    #[default]
    Pending,
    /// Settlement committed to ledger.
    Settled,
    /// Settlement failed — needs retry or manual intervention.
    Failed,
}

/// Domain events published to the event bus after state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    IntentReceived(Intent),
    IntentCancelled(Intent),
    FillCreated(Fill),
    LedgerCommitted(LedgerDelta),
    LedgerRejected { op_id: String, reason: RejectReason },
    /// Observer-only order-flow trace event. Carries no business state;
    /// consumers (monitor projector, JSONL writer, future WS endpoint) are
    /// fire-and-forget. Producer call sites must never block on, await on,
    /// or return errors from the publish of this variant. See
    /// `docs/MONITOR_DESIGN.md`.
    OrderTrace(OrderTraceEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RejectReason {
    InsufficientFunds,
    VersionConflict,
    DuplicateOp,
    InvalidEntry,
    MarketClosed,
    KillSwitchActive,
    InsufficientMargin,
    ExposureExceeded,
    OrderNotFound,
    InvalidPrice,
    InvalidAmount,
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RejectReason::InsufficientFunds => write!(f, "INSUFFICIENT_FUNDS"),
            RejectReason::VersionConflict => write!(f, "VERSION_CONFLICT"),
            RejectReason::DuplicateOp => write!(f, "DUPLICATE_OP"),
            RejectReason::InvalidEntry => write!(f, "INVALID_ENTRY"),
            RejectReason::MarketClosed => write!(f, "MARKET_CLOSED"),
            RejectReason::KillSwitchActive => write!(f, "KILL_SWITCH_ACTIVE"),
            RejectReason::InsufficientMargin => write!(f, "INSUFFICIENT_MARGIN"),
            RejectReason::ExposureExceeded => write!(f, "EXPOSURE_EXCEEDED"),
            RejectReason::OrderNotFound => write!(f, "ORDER_NOT_FOUND"),
            RejectReason::InvalidPrice => write!(f, "INVALID_PRICE"),
            RejectReason::InvalidAmount => write!(f, "INVALID_AMOUNT"),
        }
    }
}

/// Generate a new UUID v4 identifier (36-char hyphenated hex format).
pub fn generate_id() -> String {
    Uuid::new_v4().to_string()
}

/// Generate an operation ID with a descriptive prefix: `{prefix}_{uuid}`.
pub fn generate_op_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_metadata_starts_received() {
        let metadata = CommandMetadata::new("req-1");

        assert_eq!(metadata.request_id, "req-1");
        assert_eq!(metadata.command_seq, None);
        assert_eq!(metadata.lifecycle, CommandLifecycle::Received);
        assert!(metadata.updated_at >= metadata.received_at);
    }

    #[test]
    fn matching_mode_serializes_windowed_fba() {
        let mode = MatchingMode::FrequentBatchAuction { window_ms: 500 };
        let json = serde_json::to_string(&mode).unwrap();

        assert!(json.contains("frequent_batch_auction"));
        assert!(json.contains("window_ms"));
        assert!(json.contains("500"));
    }

    #[test]
    fn command_request_id_is_read_from_embedded_metadata() {
        let command = Command::MassCancelByUser(MassCancelByUserCommand {
            metadata: CommandMetadata::new("req-2"),
            user_id: "user-1".to_string(),
        });

        assert_eq!(command.request_id(), "req-2");
    }

    #[test]
    fn side_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Side::Buy).unwrap(), "\"buy\"");
        assert_eq!(serde_json::to_string(&Side::Sell).unwrap(), "\"sell\"");
        let round: Side = serde_json::from_str("\"buy\"").unwrap();
        assert_eq!(round, Side::Buy);
    }

    #[test]
    fn order_type_round_trips() {
        for variant in [OrderType::Limit, OrderType::Market] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: OrderType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn time_in_force_round_trips() {
        for variant in [
            TimeInForce::Gtc,
            TimeInForce::Ioc,
            TimeInForce::Fok,
            TimeInForce::Gtd,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: TimeInForce = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn market_state_round_trips() {
        for variant in [
            MarketState::PreOpen,
            MarketState::Normal,
            MarketState::Stress,
            MarketState::AuctionCall,
            MarketState::CancelOnly,
            MarketState::Halted,
            MarketState::Maintenance,
            MarketState::Closed,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: MarketState = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back);
        }
    }

    #[test]
    fn instrument_kind_derivative_check() {
        assert!(!InstrumentKind::Spot.is_derivative());
        assert!(InstrumentKind::Margin.is_derivative());
        assert!(InstrumentKind::Perpetual.is_derivative());
        assert!(InstrumentKind::Future.is_derivative());
        assert!(InstrumentKind::Option.is_derivative());
    }

    #[test]
    fn instrument_kind_supports_funding_only_perpetual() {
        assert!(InstrumentKind::Perpetual.supports_funding());
        assert!(!InstrumentKind::Spot.supports_funding());
        assert!(!InstrumentKind::Margin.supports_funding());
        assert!(!InstrumentKind::Future.supports_funding());
    }

    #[test]
    fn command_lifecycle_advances() {
        let mut meta = CommandMetadata::new("req-3");
        assert_eq!(meta.lifecycle, CommandLifecycle::Received);
        let t0 = meta.updated_at;

        meta.advance(CommandLifecycle::Sequenced);
        assert_eq!(meta.lifecycle, CommandLifecycle::Sequenced);
        assert!(meta.updated_at >= t0);

        meta.advance(CommandLifecycle::Completed);
        assert_eq!(meta.lifecycle, CommandLifecycle::Completed);
    }

    #[test]
    fn generate_id_is_uuid_format() {
        let id = generate_id();
        assert_eq!(id.len(), 36);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn generate_op_id_includes_prefix() {
        let op = generate_op_id("trade");
        assert!(op.starts_with("trade_"));
        assert!(op.len() > 6);
    }

    #[test]
    fn reject_reason_display() {
        assert_eq!(
            RejectReason::InsufficientFunds.to_string(),
            "INSUFFICIENT_FUNDS"
        );
        assert_eq!(RejectReason::DuplicateOp.to_string(), "DUPLICATE_OP");
        assert_eq!(
            RejectReason::InsufficientMargin.to_string(),
            "INSUFFICIENT_MARGIN"
        );
        assert_eq!(
            RejectReason::ExposureExceeded.to_string(),
            "EXPOSURE_EXCEEDED"
        );
    }

    #[test]
    fn instrument_status_default_is_active() {
        let status: InstrumentStatus = Default::default();
        assert_eq!(status, InstrumentStatus::Active);
    }

    #[test]
    fn settlement_status_default_is_pending() {
        let status: SettlementStatus = Default::default();
        assert_eq!(status, SettlementStatus::Pending);
    }

    #[test]
    fn side_can_be_used_as_hash_key() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Side::Buy);
        set.insert(Side::Sell);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn contract_multiplier_defaults_to_one() {
        let json = r#"{"instrument_id":"test","kind":"spot","quote_asset":"USDC","margin_mode":null,"max_leverage":null,"tick_size":1,"lot_size":1,"price_band_bps":1000,"risk_policy_id":"test"}"#;
        let spec: InstrumentSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.contract_multiplier, 1);
        assert_eq!(spec.funding_interval_secs, 0);
        assert_eq!(spec.status, InstrumentStatus::Active);
        assert_eq!(spec.base_asset, "");
    }

    #[test]
    fn api_error_code_display() {
        assert_eq!(ApiErrorCode::InvalidOrder.to_string(), "INVALID_ORDER");
        assert_eq!(ApiErrorCode::RateLimited.to_string(), "RATE_LIMITED");
        assert_eq!(
            ApiErrorCode::TickSizeViolation.to_string(),
            "TICK_SIZE_VIOLATION"
        );
    }

    #[test]
    fn command_metadata_mut_allows_setting_seq() {
        let mut cmd = Command::NewOrder(NewOrderCommand {
            metadata: CommandMetadata::new("req-4"),
            client_order_id: "c1".into(),
            user_id: "u1".into(),
            session_id: None,
            market_id: "m1".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: Some(100),
            amount: 10,
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
        });
        cmd.metadata_mut().command_seq = Some(42);
        assert_eq!(cmd.metadata().command_seq, Some(42));
    }

    #[test]
    fn replay_cursor_default_starts_at_zero() {
        let cursor = ReplayCursor::default();
        assert_eq!(cursor.next_seq, 0);
        assert_eq!(cursor.snapshot_seq, None);
    }

    #[test]
    fn market_state_valid_transitions() {
        use MarketState::*;
        // Normal transitions
        assert!(Normal.can_transition_to(Stress));
        assert!(Normal.can_transition_to(Halted));
        assert!(Normal.can_transition_to(Closed));
        assert!(Normal.can_transition_to(CancelOnly));
        assert!(Normal.can_transition_to(Maintenance));
        // Halted can go to PreOpen or Normal
        assert!(Halted.can_transition_to(PreOpen));
        assert!(Halted.can_transition_to(Normal));
        // PreOpen → Normal
        assert!(PreOpen.can_transition_to(Normal));
        // CancelOnly → Normal
        assert!(CancelOnly.can_transition_to(Normal));
        // Closed → PreOpen (reopen cycle)
        assert!(Closed.can_transition_to(PreOpen));
        // Identity transitions
        assert!(Normal.can_transition_to(Normal));
        assert!(Halted.can_transition_to(Halted));
    }

    #[test]
    fn market_state_invalid_transitions() {
        use MarketState::*;
        // Closed → Normal directly is invalid (must go through PreOpen)
        assert!(!Closed.can_transition_to(Normal));
        assert!(!Closed.can_transition_to(Stress));
        // PreOpen → Stress is invalid
        assert!(!PreOpen.can_transition_to(Stress));
        // Maintenance → Stress is invalid
        assert!(!Maintenance.can_transition_to(Stress));
    }

    #[test]
    fn insurance_fund_config_defaults() {
        let config = InsuranceFundConfig::default();
        assert_eq!(config.min_reserve, 0);
        assert_eq!(config.penalty_capture_pct, 0);
    }

    #[test]
    fn account_mode_default_is_simple() {
        let mode: AccountMode = Default::default();
        assert_eq!(mode, AccountMode::Simple);
    }

    #[test]
    fn account_mode_round_trips() {
        for mode in [
            AccountMode::Simple,
            AccountMode::SingleCurrencyMargin,
            AccountMode::MultiCurrencyMargin,
            AccountMode::PortfolioMargin,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: AccountMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn circuit_breaker_config_defaults() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.stress_threshold_bps, 500);
        assert_eq!(config.cancel_only_threshold_bps, 800);
        assert_eq!(config.halt_threshold_bps, 1200);
        assert_eq!(config.cooldown_secs, 300);
        assert_eq!(config.vol_lookback_trades, 50);
    }

    #[test]
    fn collateral_asset_serializes() {
        let asset = CollateralAsset {
            asset_id: "BTC".into(),
            haircut_bps: 500,
            eligible: true,
            concentration_cap: 0,
        };
        let json = serde_json::to_string(&asset).unwrap();
        assert!(json.contains("\"asset_id\":\"BTC\""));
        assert!(json.contains("\"haircut_bps\":500"));
    }

    #[test]
    fn market_maker_protection_defaults() {
        let mmp = MarketMakerProtection::default();
        assert_eq!(mmp.max_delta_qty, 0);
        assert_eq!(mmp.max_notional_window, 0);
        assert_eq!(mmp.window_secs, 1);
    }

    #[test]
    fn new_api_error_codes_display() {
        assert_eq!(
            ApiErrorCode::FatFingerRejected.to_string(),
            "FAT_FINGER_REJECTED"
        );
        assert_eq!(
            ApiErrorCode::MarketKillSwitchActive.to_string(),
            "MARKET_KILL_SWITCH_ACTIVE"
        );
        assert_eq!(
            ApiErrorCode::CircuitBreakerTriggered.to_string(),
            "CIRCUIT_BREAKER_TRIGGERED"
        );
        assert_eq!(
            ApiErrorCode::MarketMakerProtectionTriggered.to_string(),
            "MARKET_MAKER_PROTECTION_TRIGGERED"
        );
    }

    #[test]
    fn instrument_spec_with_circuit_breaker() {
        let json = r#"{
            "instrument_id":"BTC-USDC","kind":"perpetual","quote_asset":"USDC",
            "margin_mode":null,"max_leverage":null,"tick_size":1,"lot_size":1,
            "price_band_bps":1000,"risk_policy_id":"perp",
            "circuit_breaker":{"stress_threshold_bps":400,"cancel_only_threshold_bps":700,"halt_threshold_bps":1000,"cooldown_secs":60,"vol_lookback_trades":30}
        }"#;
        let spec: InstrumentSpec = serde_json::from_str(json).unwrap();
        let cb = spec.circuit_breaker.unwrap();
        assert_eq!(cb.stress_threshold_bps, 400);
        assert_eq!(cb.cooldown_secs, 60);
    }

    #[test]
    fn admin_action_unfreeze_serializes() {
        let action = AdminAction::UnfreezeAccount {
            user_id: "u1".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("UnfreezeAccount"));
    }

    #[test]
    fn admin_action_market_kill_switch_serializes() {
        let action = AdminAction::MarketKillSwitch {
            market_id: "BTC-USDC".into(),
            enabled: true,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("MarketKillSwitch"));
    }

    #[test]
    fn account_mode_on_account_defaults_to_simple() {
        let json = r#"{"id":"a1","balance":100,"version":1,"account_type":"user"}"#;
        let account: Account = serde_json::from_str(json).unwrap();
        assert_eq!(account.account_mode, AccountMode::Simple);
    }

    #[test]
    fn account_with_explicit_mode_round_trips() {
        let account = Account {
            id: "a1".into(),
            balance: 100,
            version: 1,
            account_type: "user".into(),
            account_mode: AccountMode::PortfolioMargin,
        };
        let json = serde_json::to_string(&account).unwrap();
        let back: Account = serde_json::from_str(&json).unwrap();
        assert_eq!(back.account_mode, AccountMode::PortfolioMargin);
    }

    #[test]
    fn fee_schedule_serializes() {
        let schedule = FeeSchedule {
            name: "Standard".into(),
            tiers: vec![
                FeeTier {
                    min_volume: 0,
                    maker_fee_bps: 10,
                    taker_fee_bps: 20,
                },
                FeeTier {
                    min_volume: 1_000_000,
                    maker_fee_bps: 5,
                    taker_fee_bps: 15,
                },
            ],
            withdrawal_fee: 100,
            mm_rebate_enabled: true,
        };
        let json = serde_json::to_string(&schedule).unwrap();
        let back: FeeSchedule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tiers.len(), 2);
        assert_eq!(back.tiers[1].maker_fee_bps, 5);
        assert!(back.mm_rebate_enabled);
    }

    #[test]
    fn fee_schedule_resolve_picks_highest_qualifying_tier() {
        let schedule = FeeSchedule {
            name: "Tiered".into(),
            tiers: vec![
                FeeTier {
                    min_volume: 0,
                    maker_fee_bps: 10,
                    taker_fee_bps: 20,
                },
                FeeTier {
                    min_volume: 100_000,
                    maker_fee_bps: 8,
                    taker_fee_bps: 16,
                },
                FeeTier {
                    min_volume: 1_000_000,
                    maker_fee_bps: 5,
                    taker_fee_bps: 12,
                },
            ],
            withdrawal_fee: 0,
            mm_rebate_enabled: false,
        };
        // Zero volume → base tier
        assert_eq!(schedule.resolve(0, 99, 99), (10, 20));
        // 50k → base tier (below 100k)
        assert_eq!(schedule.resolve(50_000, 99, 99), (10, 20));
        // 100k → second tier
        assert_eq!(schedule.resolve(100_000, 99, 99), (8, 16));
        // 500k → second tier (below 1M)
        assert_eq!(schedule.resolve(500_000, 99, 99), (8, 16));
        // 1M → third tier
        assert_eq!(schedule.resolve(1_000_000, 99, 99), (5, 12));
        // 10M → third tier (highest)
        assert_eq!(schedule.resolve(10_000_000, 99, 99), (5, 12));
    }

    #[test]
    fn fee_schedule_resolve_empty_tiers_returns_defaults() {
        let schedule = FeeSchedule {
            name: "Empty".into(),
            tiers: vec![],
            withdrawal_fee: 0,
            mm_rebate_enabled: false,
        };
        assert_eq!(schedule.resolve(0, 10, 20), (10, 20));
    }

    #[test]
    fn margin_rule_round_trips() {
        let rule = MarginRule {
            initial_margin_bps: 1000,
            maintenance_margin_bps: 500,
            liquidation_penalty_bps: 100,
            max_leverage: 20,
            auto_deleverage_enabled: true,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: MarginRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.initial_margin_bps, 1000);
        assert_eq!(back.max_leverage, 20);
        assert!(back.auto_deleverage_enabled);
    }

    #[test]
    fn liquidation_rule_round_trips() {
        let rule = LiquidationRule {
            penalty_bps: 100,
            insurance_fund_share_bps: 5000,
            adl_enabled: true,
            auction_duration_secs: 0,
            partial_liquidation_enabled: true,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: LiquidationRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.penalty_bps, 100);
        assert!(back.adl_enabled);
    }

    #[test]
    fn rate_limit_rule_defaults() {
        let json = r#"{"max_orders_per_second":50,"max_cancels_per_second":100,"max_messages_per_second":200}"#;
        let rule: RateLimitRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.max_orders_per_second, 50);
        assert_eq!(rule.order_weight, 1);
        assert_eq!(rule.cancel_weight, 1);
    }

    #[test]
    fn order_type_rule_serializes() {
        let rule = OrderTypeRule {
            allowed_order_types: vec![OrderType::Limit, OrderType::Market],
            allowed_tif: vec![TimeInForce::Gtc, TimeInForce::Ioc, TimeInForce::Fok],
            post_only_allowed: true,
            reduce_only_allowed: true,
            iceberg_allowed: true,
            conditional_allowed: false,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: OrderTypeRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.allowed_order_types.len(), 2);
        assert!(back.post_only_allowed);
        assert!(!back.conditional_allowed);
    }

    #[test]
    fn new_api_error_codes_parity_display() {
        assert_eq!(
            ApiErrorCode::InsufficientMargin.to_string(),
            "INSUFFICIENT_MARGIN"
        );
        assert_eq!(
            ApiErrorCode::ExceedsMaxLeverage.to_string(),
            "EXCEEDS_MAX_LEVERAGE"
        );
        assert_eq!(
            ApiErrorCode::ExceedsPositionLimit.to_string(),
            "EXCEEDS_POSITION_LIMIT"
        );
        assert_eq!(
            ApiErrorCode::ReduceOnlyViolation.to_string(),
            "REDUCE_ONLY_VIOLATION"
        );
        assert_eq!(
            ApiErrorCode::PostOnlyWouldTrade.to_string(),
            "POST_ONLY_WOULD_TRADE"
        );
        assert_eq!(
            ApiErrorCode::InvalidTimeInForce.to_string(),
            "INVALID_TIME_IN_FORCE"
        );
        assert_eq!(
            ApiErrorCode::InvalidTriggerPrice.to_string(),
            "INVALID_TRIGGER_PRICE"
        );
        assert_eq!(ApiErrorCode::OrderExpired.to_string(), "ORDER_EXPIRED");
        assert_eq!(ApiErrorCode::MarketNotFound.to_string(), "MARKET_NOT_FOUND");
        assert_eq!(
            ApiErrorCode::InstrumentHalted.to_string(),
            "INSTRUMENT_HALTED"
        );
        assert_eq!(
            ApiErrorCode::InstrumentDelisted.to_string(),
            "INSTRUMENT_DELISTED"
        );
        assert_eq!(
            ApiErrorCode::InvalidAmendment.to_string(),
            "INVALID_AMENDMENT"
        );
        assert_eq!(ApiErrorCode::SessionExpired.to_string(), "SESSION_EXPIRED");
        assert_eq!(ApiErrorCode::IpRateLimited.to_string(), "IP_RATE_LIMITED");
        assert_eq!(
            ApiErrorCode::MaintenanceMode.to_string(),
            "MAINTENANCE_MODE"
        );
        assert_eq!(
            ApiErrorCode::InvalidAccountMode.to_string(),
            "INVALID_ACCOUNT_MODE"
        );
        assert_eq!(
            ApiErrorCode::CollateralIneligible.to_string(),
            "COLLATERAL_INELIGIBLE"
        );
        assert_eq!(
            ApiErrorCode::LiquidationInProgress.to_string(),
            "LIQUIDATION_IN_PROGRESS"
        );
    }

    #[test]
    fn instrument_spec_order_type_rule_defaults_to_none() {
        let json = r#"{"instrument_id":"test","kind":"spot","quote_asset":"USDC","margin_mode":null,"max_leverage":null,"tick_size":1,"lot_size":1,"price_band_bps":1000,"risk_policy_id":"test"}"#;
        let spec: InstrumentSpec = serde_json::from_str(json).unwrap();
        assert!(spec.order_type_rule.is_none());
        assert!(spec.margin_rule.is_none());
        assert!(spec.liquidation_rule.is_none());
    }

    #[test]
    fn instrument_spec_with_margin_rule_round_trips() {
        let mut spec: InstrumentSpec = serde_json::from_str(r#"{"instrument_id":"test","kind":"perpetual","quote_asset":"USDC","margin_mode":"isolated","max_leverage":20,"tick_size":1,"lot_size":1,"price_band_bps":1000,"risk_policy_id":"test"}"#).unwrap();
        spec.margin_rule = Some(MarginRule {
            initial_margin_bps: 1000,
            maintenance_margin_bps: 500,
            liquidation_penalty_bps: 100,
            max_leverage: 20,
            auto_deleverage_enabled: true,
        });
        let json = serde_json::to_string(&spec).unwrap();
        let back: InstrumentSpec = serde_json::from_str(&json).unwrap();
        let rule = back.margin_rule.unwrap();
        assert_eq!(rule.initial_margin_bps, 1000);
        assert_eq!(rule.max_leverage, 20);
    }

    #[test]
    fn instrument_spec_with_liquidation_rule_round_trips() {
        let mut spec: InstrumentSpec = serde_json::from_str(r#"{"instrument_id":"test","kind":"perpetual","quote_asset":"USDC","margin_mode":"isolated","max_leverage":20,"tick_size":1,"lot_size":1,"price_band_bps":1000,"risk_policy_id":"test"}"#).unwrap();
        spec.liquidation_rule = Some(LiquidationRule {
            penalty_bps: 100,
            insurance_fund_share_bps: 5000,
            adl_enabled: true,
            auction_duration_secs: 0,
            partial_liquidation_enabled: true,
        });
        let json = serde_json::to_string(&spec).unwrap();
        let back: InstrumentSpec = serde_json::from_str(&json).unwrap();
        let rule = back.liquidation_rule.unwrap();
        assert_eq!(rule.penalty_bps, 100);
        assert!(rule.adl_enabled);
    }
}
