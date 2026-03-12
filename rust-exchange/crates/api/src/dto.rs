use super::*;

#[derive(serde::Deserialize)]
pub(crate) struct DepositRequest {
    pub(crate) user_id: String,
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
pub(crate) struct KillSwitchRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) enabled: bool,
}

#[derive(serde::Deserialize)]
pub(crate) struct SetMarketStateRequest {
    pub(crate) request_id: Option<String>,
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) state: MarketState,
}

#[derive(serde::Deserialize)]
pub(crate) struct ReferencePriceRequest {
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

#[derive(serde::Deserialize)]
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
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct BookQuery {
    pub(crate) outcome: Option<i32>,
    pub(crate) depth: Option<usize>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct TradesQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) outcome: Option<i32>,
    pub(crate) limit: Option<usize>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct OrdersQuery {
    pub(crate) market_id: Option<String>,
    pub(crate) outcome: Option<i32>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct HistoryQuery {
    pub(crate) outcome: Option<i32>,
    pub(crate) limit: Option<usize>,
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
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct IndexPriceUpsertRequest {
    pub(crate) market_id: String,
    pub(crate) outcome: Option<i32>,
    pub(crate) index_price: i64,
    pub(crate) source: Option<String>,
}

#[derive(Default, serde::Deserialize)]
pub(crate) struct FairPriceQuery {
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
