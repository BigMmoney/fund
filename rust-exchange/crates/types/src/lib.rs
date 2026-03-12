use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub balance: i64,
    pub version: i64,
    pub account_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub debit_account: String,
    pub credit_account: String,
    pub amount: i64,
    pub op_id: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerDelta {
    pub op_id: String,
    pub entries: Vec<LedgerEntry>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: String,
    pub user_id: String,
    pub market_id: String,
    pub side: Side,
    pub price: i64,
    pub amount: i64,
    pub outcome: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub status: IntentStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Limit,
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Fok,
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
    Normal,
    Stress,
    AuctionCall,
    CancelOnly,
    Halted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchingMode {
    ContinuousClob,
    FrequentBatchAuction { window_ms: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    Spot,
    Margin,
    Perpetual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarginMode {
    Isolated,
    Cross,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentSpec {
    pub instrument_id: String,
    pub kind: InstrumentKind,
    pub quote_asset: String,
    pub margin_mode: Option<MarginMode>,
    pub max_leverage: Option<u32>,
    pub tick_size: i64,
    pub lot_size: i64,
    pub price_band_bps: i64,
    pub risk_policy_id: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminCommand {
    pub metadata: CommandMetadata,
    pub actor_id: String,
    pub action: AdminAction,
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
    pub price: i64,
    pub amount: i64,
    pub outcome: i32,
    pub status: OrderState,
    pub created_at: DateTime<Utc>,
}

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    IntentReceived(Intent),
    IntentCancelled(Intent),
    FillCreated(Fill),
    LedgerCommitted(LedgerDelta),
    LedgerRejected { op_id: String, reason: RejectReason },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RejectReason {
    InsufficientFunds,
    VersionConflict,
    DuplicateOp,
    InvalidEntry,
    MarketClosed,
    KillSwitchActive,
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
        }
    }
}

pub fn generate_id() -> String {
    Uuid::new_v4().to_string()
}

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
}
