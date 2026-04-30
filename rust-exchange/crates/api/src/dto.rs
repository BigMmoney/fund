use super::*;
use serde::Deserialize;

#[derive(serde::Deserialize)]
pub(crate) struct DepositRequest {
    pub(crate) user_id: String,
    pub(crate) amount: i64,
    pub(crate) op_id: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct PositionDepositRequest {
    pub(crate) user_id: String,
    pub(crate) market_id: String,
    pub(crate) outcome: i32,
    pub(crate) amount: i64,
    pub(crate) op_id: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct IntentRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) client_order_id: Option<String>,
    pub(crate) market_id: String,
    pub(crate) side: Side,
    pub(crate) price: i64,
    pub(crate) amount: i64,
    pub(crate) outcome: i32,
}

#[derive(serde::Deserialize)]
pub(crate) struct OrderRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) client_order_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) market_id: String,
    pub(crate) side: Side,
    pub(crate) order_type: Option<OrderType>,
    pub(crate) time_in_force: Option<TimeInForce>,
    pub(crate) price: Option<i64>,
    pub(crate) amount: i64,
    pub(crate) outcome: i32,
    pub(crate) post_only: Option<bool>,
    pub(crate) reduce_only: Option<bool>,
    pub(crate) leverage: Option<u32>,
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) stp_mode: Option<types::StpMode>,
    pub(crate) trigger_price: Option<i64>,
    pub(crate) trigger_type: Option<types::TriggerType>,
}

#[derive(serde::Deserialize)]
pub(crate) struct BatchOrderRequest {
    #[serde(deserialize_with = "deserialize_batch_orders")]
    pub(crate) orders: Vec<OrderRequest>,
}

fn deserialize_batch_orders<'de, D>(deserializer: D) -> Result<Vec<OrderRequest>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let orders = Vec::<OrderRequest>::deserialize(deserializer)?;
    const MAX_BATCH_SIZE: usize = 100;
    if orders.len() > MAX_BATCH_SIZE {
        return Err(serde::de::Error::custom(format!(
            "batch size {} exceeds maximum of {}",
            orders.len(),
            MAX_BATCH_SIZE
        )));
    }
    Ok(orders)
}

#[derive(serde::Deserialize)]
pub(crate) struct CancelOrderRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) order_id: String,
    pub(crate) client_order_id: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct MassCancelByUserRequest {
    pub(crate) request_id: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct MassCancelBySessionRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) session_id: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct MassCancelByMarketRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) market_id: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct LeverageAdjustRequest {
    pub(crate) market_id: String,
    pub(crate) leverage: u32,
}

#[derive(serde::Deserialize)]
pub(crate) struct TradeExportQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) from: Option<DateTime<Utc>>,
    pub(crate) to: Option<DateTime<Utc>>,
    pub(crate) format: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct KillSwitchRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) enabled: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct SetMarketStateRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) state: MarketState,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ReferencePriceRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) market_id: String,
    pub(crate) outcome: i32,
    pub(crate) source: Option<String>,
    pub(crate) reference_price: i64,
}

#[derive(serde::Deserialize)]
pub(crate) struct ReplaceOrderRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) order_id: String,
    pub(crate) new_client_order_id: Option<String>,
    pub(crate) new_price: Option<i64>,
    pub(crate) new_amount: Option<i64>,
    pub(crate) new_time_in_force: Option<TimeInForce>,
    pub(crate) post_only: Option<bool>,
    pub(crate) reduce_only: Option<bool>,
    pub(crate) new_leverage: Option<u32>,
    pub(crate) new_expires_at: Option<DateTime<Utc>>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct LiquidationExecuteRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) user_id: String,
    pub(crate) liquidator_user_id: String,
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) mark_price: i64,
    pub(crate) leverage: Option<u32>,
    pub(crate) maintenance_margin_bps: Option<i64>,
    pub(crate) penalty_bps: Option<i64>,
}

#[derive(serde::Deserialize)]
pub(crate) struct FundingSettlementRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) long_user_id: String,
    pub(crate) short_user_id: String,
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) mark_price: i64,
    pub(crate) funding_rate_ppm: i64,
}

#[derive(serde::Deserialize)]
pub(crate) struct InsuranceFundDepositRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) amount: i64,
    /// If set, deposit into the per-instrument fund; otherwise deposit into the global fund.
    pub(crate) market_id: Option<String>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct BookQuery {
    pub(crate) outcome: Option<i32>,
    pub(crate) depth: Option<usize>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct TradesQuery {
    pub(crate) user_id: Option<String>,
    pub(crate) outcome: Option<i32>,
    pub(crate) limit: Option<usize>,
    /// Cursor-based pagination: only return trades recorded before this timestamp.
    pub(crate) before: Option<DateTime<Utc>>,
    /// Only return trades recorded after this timestamp.
    pub(crate) after: Option<DateTime<Utc>>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct OrdersQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) outcome: Option<i32>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct OrderLookupQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) outcome: Option<i32>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct LedgerViewQuery {
    pub(crate) include_zero: Option<bool>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct FillsQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) outcome: Option<i32>,
    pub(crate) limit: Option<usize>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct HistoryQuery {
    pub(crate) outcome: Option<i32>,
    pub(crate) limit: Option<usize>,
    pub(crate) before: Option<String>,
    pub(crate) after: Option<String>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct MarginQuery {
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) mark_price: Option<i64>,
    pub(crate) leverage: Option<u32>,
    pub(crate) maintenance_margin_bps: Option<i64>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct PnlQuery {
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) entry_price: Option<i64>,
    pub(crate) mark_price: Option<i64>,
}

#[derive(serde::Deserialize)]
pub(crate) struct FundingRateUpsertRequest {
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) funding_rate_ppm: i64,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct AdminActionAuditQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) action: Option<String>,
    pub(crate) subject: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct AdminBetaControlPlaneUpdateRequest {
    pub(crate) enabled: Option<bool>,
    pub(crate) require_whitelist: Option<bool>,
}

#[derive(serde::Deserialize)]
pub(crate) struct AdminBetaUserControlUpdateRequest {
    pub(crate) whitelisted: Option<bool>,
    pub(crate) max_cash_balance: Option<i64>,
    pub(crate) max_open_orders: Option<u32>,
}

#[derive(serde::Deserialize)]
pub(crate) struct AdminBetaMarketControlUpdateRequest {
    pub(crate) max_order_notional: Option<i64>,
    pub(crate) max_leverage: Option<u32>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct FundingRatesQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) outcome: Option<i32>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct RiskEventsQuery {
    pub(crate) limit: Option<usize>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct LiquidationQueueQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) status: Option<String>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct LiquidationAuctionsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) status: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct LiquidationAuctionBidRequest {
    pub(crate) bid_price: i64,
    pub(crate) bid_quantity: i64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct AdlGovernanceUpdateRequest {
    pub(crate) maintenance_margin_bps: Option<i64>,
    pub(crate) leverage_weight_bps: Option<i64>,
    pub(crate) bankruptcy_distance_weight_bps: Option<i64>,
    pub(crate) size_weight_bps: Option<i64>,
    pub(crate) buffer_weight_bps: Option<i64>,
    pub(crate) max_candidates: Option<usize>,
    pub(crate) max_socialized_loss_share_bps_per_candidate: Option<i64>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct LiquidationPolicyUpdateRequest {
    pub(crate) auction_window_secs: Option<i64>,
    pub(crate) retry_backoff_secs: Option<Vec<i64>>,
    pub(crate) max_retry_tiers: Option<u32>,
    pub(crate) max_auction_rounds: Option<u32>,
    pub(crate) auction_reserve_step_bps: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct IndexPriceUpsertRequest {
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) index_price: i64,
    pub(crate) source: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct IndexSourcePolicyUpdateRequest {
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) source: String,
    pub(crate) status: String,
    pub(crate) weight_bps: Option<i64>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct FairPriceQuery {
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct MarketStateQuery {
    pub(crate) outcome: Option<i32>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct IndexSourcePolicyQuery {
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct LiquidationQueueOverrideRequest {
    pub(crate) action: String,
    pub(crate) liquidator_user_id: Option<String>,
    pub(crate) retry_tier: Option<u32>,
    pub(crate) next_attempt_secs: Option<i64>,
}

// ── Order History ────────────────────────────────────────────

#[derive(Default, serde::Deserialize)]
pub(crate) struct OrderHistoryQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) outcome: Option<i32>,
    pub(crate) side: Option<String>,
    pub(crate) limit: Option<usize>,
}

// ── Ticker ───────────────────────────────────────────────────

#[derive(Default, serde::Deserialize)]
pub(crate) struct TickerQuery {
    pub(crate) outcome: Option<i32>,
}

// ── Funding History ──────────────────────────────────────────

#[derive(Default, serde::Deserialize)]
pub(crate) struct FundingHistoryQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) outcome: Option<i32>,
    pub(crate) limit: Option<usize>,
}

// ── Klines ───────────────────────────────────────────────────

#[derive(Default, serde::Deserialize)]
pub(crate) struct KlineQuery {
    pub(crate) outcome: Option<i32>,
    pub(crate) interval: Option<String>,
    pub(crate) limit: Option<usize>,
}

// ── Withdrawals ──────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct WithdrawalRequest {
    pub(crate) amount: i64,
    pub(crate) destination_address: String,
    pub(crate) asset: Option<String>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct WithdrawalQuery {
    pub(crate) status: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(serde::Deserialize)]
pub(crate) struct WithdrawalApproveRequest {
    pub(crate) withdrawal_id: String,
}

// ── Transfers ────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct TransferRequest {
    pub(crate) to_user_id: String,
    pub(crate) amount: i64,
    pub(crate) asset: Option<String>,
    pub(crate) memo: Option<String>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct TransferQuery {
    pub(crate) limit: Option<usize>,
}
