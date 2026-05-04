//! Wallet type model — addresses, withdrawals, sanctions, chain ids.
//!
//! Wire/storage shape only. The implementation that consumes these
//! types lives in the api crate (storage + REST handlers) and the
//! hot-wallet daemon (signing).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Schema version stamped on every persisted wallet record. Bump on
/// breaking field changes so older WAL entries can be matched against
/// the producing version.
pub const WALLET_SCHEMA_VERSION: u32 = 1;

// ── Chain id ─────────────────────────────────────────────────────────

/// Identifier for a supported chain. v1 list is a closed set; v1.1+
/// will add layer-2s and additional L1s. Kept as a serde-tagged enum
/// (rather than a string) so misspelled chain ids fail to deserialize
/// at the api boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainId {
    /// Ethereum mainnet — ETH plus ERC-20 tokens. v1 ships this.
    Eth,
    /// Bitcoin mainnet. v1 stretch.
    Btc,
    /// Solana mainnet — SOL plus SPL tokens. v1.2 deferred.
    Sol,
}

impl std::fmt::Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_string(self).unwrap_or_else(|_| "?".into());
        write!(f, "{}", s.trim_matches('"'))
    }
}

// ── Custody tier ─────────────────────────────────────────────────────

/// One of the three custody tiers per design §2. Used by reconciliation
/// reports and the `/admin/wallet/balances` endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalletTier {
    /// Long-term reserve. 3-of-5 multi-sig, air-gapped signing.
    Cold,
    /// Refill buffer. 2-of-3 multi-sig, distributed ceremony.
    Warm,
    /// Customer-withdrawal pool. Single-key, daemon-signed.
    Hot,
}

// ── Address book ─────────────────────────────────────────────────────

/// Stable identifier for a whitelisted withdrawal address.
pub type WithdrawalAddressId = String;

/// Lifecycle of a customer-supplied withdrawal address per design §4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddressStatus {
    /// In the 24h cool-down window after creation.
    PendingCooldown,
    /// Past cool-down; available as a withdrawal destination.
    Active,
    /// Compliance-flagged; cannot be a destination, record retained
    /// for audit. Re-activation is maker-checker (compliance_ops +
    /// finance_ops per design §4.2).
    Suspended,
    /// Customer-removed; record retained but cannot be re-used. A new
    /// add must go through a fresh cool-down.
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SanctionsScreenStatus {
    /// Not yet checked (record just created; provider call pending).
    Pending,
    /// Provider returned no hit.
    Clear,
    /// Provider returned a hit; address is blocked from any use.
    Hit,
    /// Provider call failed; treated as a soft-block until re-checked.
    Error,
}

/// One sanctions-provider response, embedded on the address record so
/// audit can replay the decision later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanctionsHit {
    pub list_name: String,
    pub matched_entity: String,
    pub score: u8,
    pub matched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanctionsCheckResult {
    pub status: SanctionsScreenStatus,
    pub provider: String,
    pub checked_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit: Option<SanctionsHit>,
    /// Provider-supplied opaque correlation id for replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithdrawalAddress {
    pub schema_version: u32,
    pub address_id: WithdrawalAddressId,
    pub user_id: String,
    pub chain: ChainId,
    /// Chain-specific encoding: hex with 0x prefix for ETH, base58 for
    /// BTC legacy / bech32 for BTC native segwit, base58 for SOL.
    pub address: String,
    pub label: String,
    pub status: AddressStatus,
    pub added_at: DateTime<Utc>,
    /// Active when `cooldown_until <= now`.
    pub cooldown_until: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
    pub sanctions_check: SanctionsCheckResult,
    /// `user_id` for self-add; operator id for compliance-add.
    pub added_by: String,
}

// ── Withdrawal ───────────────────────────────────────────────────────

/// Lifecycle of a customer withdrawal per design §5.1. `Rejected` is a
/// terminal state; transitions out of it are not permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalStatus {
    /// Customer POSTed `/withdraw`; address book lookup succeeded;
    /// basic validation (amount > 0, sufficient available balance)
    /// passed.
    Submitted,
    /// Pre-flight checks pass: address is `Active`, amount within
    /// caps, sanctions re-check clears, hot-wallet has balance or a
    /// refill is queued.
    Validated,
    /// In `WithdrawalQueue` waiting for the next sweep. Customer sees
    /// status `pending`.
    Queued,
    /// Hit a maker-checker threshold; living in the
    /// `ApprovalRequestStore` with `action = WithdrawalsApprove`.
    AwaitingApproval,
    /// Operator approved (or auto-approved under threshold); ready to
    /// sign.
    Approved,
    /// Hot-wallet daemon picked it up; building / signing.
    Signing,
    /// Tx broadcast; we have a tx hash but not yet confirmation depth.
    Broadcast,
    /// Tx reached the chain's confirmation threshold.
    Confirmed,
    /// Internal ledger debit committed; flow is done.
    Settled,
    /// On-chain tx confirmed but the customer-side ledger debit
    /// failed (e.g. balance went negative between submit and settle —
    /// or the per-chain divisor pushed amount past i64). The funds
    /// have already left the hot wallet, so this is an OPERATIONAL
    /// alert — the record stays here until an operator reconciles.
    /// Distinct from Rejected because the on-chain side already
    /// committed; you cannot just "release the reservation" (H6).
    SettlementStuck,
    /// Failed at any stage; reason recorded; ledger reservation
    /// released. Terminal — not re-attemptable under the same
    /// withdrawal_id.
    Rejected,
}

/// Why a withdrawal was rejected. Carried on the withdrawal record's
/// `rejection_reason` field when status is `Rejected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalRejectReason {
    /// `address_id` does not resolve, or the literal `address` is not
    /// in the customer's whitelist.
    DestinationNotWhitelisted,
    /// Address found but its status is not `Active` (cool-down, suspended,
    /// or removed).
    DestinationNotActive,
    /// Address sanctions-screen returned a hit at submit time OR the
    /// re-check at validate time flipped from clear to hit.
    SanctionsHit,
    /// Customer's `available` balance is below `amount + estimated_fee`.
    InsufficientBalance,
    /// Per-customer per-day cap exceeded.
    AmountCapExceeded,
    /// Approver rejected the request via the maker-checker flow.
    ApprovalRejected,
    /// Maker-checker request expired without a decision.
    ApprovalExpired,
    /// Hot wallet daemon refused (e.g. circuit-breaker breach).
    DaemonRejected,
    /// Broadcast failed across all configured RPC providers after
    /// retries.
    BroadcastFailed,
    /// Tx was confirmed then dropped by a deeper reorg than the
    /// configured threshold can recover from.
    ReorgOrphaned,
    /// Operator manually rejected at the queue layer.
    OperatorRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithdrawalRecord {
    pub schema_version: u32,
    pub withdrawal_id: String,
    pub user_id: String,
    pub chain: ChainId,
    pub address_id: WithdrawalAddressId,
    /// Resolved on-chain destination address at submit time. Cached
    /// here so a later mutation of the address book does not change
    /// the historical record.
    pub destination_address: String,
    /// Smallest indivisible chain unit (wei / satoshi / lamport).
    pub amount: i128,
    /// Estimated fee at submit time. The actual fee paid lands in
    /// `actual_fee` once `Confirmed`.
    pub estimated_fee: i128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_fee: Option<i128>,
    pub status: WithdrawalStatus,
    pub submitted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Set when status reaches `Approved` (auto or maker-checker).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<DateTime<Utc>>,
    /// Set when the daemon broadcasts the tx.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadcast_at: Option<DateTime<Utc>>,
    /// Set when confirmation depth is reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<DateTime<Utc>>,
    /// Set when the ledger debit commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<DateTime<Utc>>,
    /// Tx hash; chain-specific encoding (hex with 0x prefix for ETH,
    /// hex for BTC, base58 for SOL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    /// Number of confirmations observed at last poll.
    #[serde(default)]
    pub confirmations: u32,
    /// Minimum confirmation depth required to advance from `Broadcast`
    /// to `Confirmed`. Captured at submit time so a config change does
    /// not retroactively change the bar for in-flight withdrawals.
    pub confirmations_required: u32,
    /// Approval request id when the withdrawal went through maker-
    /// checker. Lookup via `crate::api::admin_approvals_http`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<WithdrawalRejectReason>,
    /// Free-form additional context attached at any state change for
    /// post-mortem review (operator notes, RPC error strings, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

// ── Fee urgency (chain-adapter input) ────────────────────────────────

/// Hint to the chain adapter when estimating fees. Covers v1's three-
/// tier fee policy (cheap retail, normal, urgent for refills + manual
/// pushes by finance_ops).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeeUrgency {
    /// Cheapest fee tier; for non-time-sensitive customer withdrawals.
    Economy,
    /// Default fee tier; normal customer withdrawals.
    Standard,
    /// Top fee tier; for refills, sweeps, manual operator pushes.
    Urgent,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-04T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn sample_check() -> SanctionsCheckResult {
        SanctionsCheckResult {
            status: SanctionsScreenStatus::Clear,
            provider: "chainalysis".into(),
            checked_at: ts(),
            hit: None,
            correlation_id: Some("ca-corr-123".into()),
        }
    }

    #[test]
    fn chain_id_serialises_as_snake_case() {
        assert_eq!(serde_json::to_string(&ChainId::Eth).unwrap(), "\"eth\"");
        assert_eq!(serde_json::to_string(&ChainId::Btc).unwrap(), "\"btc\"");
        assert_eq!(serde_json::to_string(&ChainId::Sol).unwrap(), "\"sol\"");
    }

    #[test]
    fn chain_id_display_matches_wire_form() {
        assert_eq!(ChainId::Eth.to_string(), "eth");
    }

    #[test]
    fn wallet_tier_round_trip() {
        for tier in [WalletTier::Cold, WalletTier::Warm, WalletTier::Hot] {
            let s = serde_json::to_string(&tier).unwrap();
            let back: WalletTier = serde_json::from_str(&s).unwrap();
            assert_eq!(back, tier);
        }
    }

    #[test]
    fn address_status_wire_form_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&AddressStatus::PendingCooldown).unwrap(),
            "\"pending_cooldown\""
        );
        assert_eq!(
            serde_json::to_string(&AddressStatus::Suspended).unwrap(),
            "\"suspended\""
        );
    }

    #[test]
    fn sanctions_check_round_trip_clear() {
        let c = sample_check();
        let s = serde_json::to_string(&c).unwrap();
        let back: SanctionsCheckResult = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
        // `hit` is None and serde-skipped.
        assert!(!s.contains("\"hit\""));
    }

    #[test]
    fn sanctions_check_round_trip_with_hit() {
        let c = SanctionsCheckResult {
            status: SanctionsScreenStatus::Hit,
            provider: "chainalysis".into(),
            checked_at: ts(),
            hit: Some(SanctionsHit {
                list_name: "OFAC SDN".into(),
                matched_entity: "test entity".into(),
                score: 95,
                matched_at: ts(),
            }),
            correlation_id: None,
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: SanctionsCheckResult = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
        assert!(s.contains("\"hit\""));
        assert!(s.contains("\"OFAC SDN\""));
    }

    #[test]
    fn withdrawal_address_round_trip() {
        let a = WithdrawalAddress {
            schema_version: WALLET_SCHEMA_VERSION,
            address_id: "addr-1".into(),
            user_id: "alice".into(),
            chain: ChainId::Eth,
            address: "0xabc123".into(),
            label: "exchange wallet".into(),
            status: AddressStatus::Active,
            added_at: ts(),
            cooldown_until: ts(),
            last_used_at: None,
            sanctions_check: sample_check(),
            added_by: "alice".into(),
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: WithdrawalAddress = serde_json::from_str(&s).unwrap();
        assert_eq!(back, a);
        // `last_used_at` is None and skipped.
        assert!(!s.contains("\"last_used_at\""));
    }

    #[test]
    fn withdrawal_status_serialises_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&WithdrawalStatus::AwaitingApproval).unwrap(),
            "\"awaiting_approval\""
        );
        assert_eq!(
            serde_json::to_string(&WithdrawalStatus::Settled).unwrap(),
            "\"settled\""
        );
    }

    #[test]
    fn withdrawal_record_round_trip_minimal() {
        let r = WithdrawalRecord {
            schema_version: WALLET_SCHEMA_VERSION,
            withdrawal_id: "wd-1".into(),
            user_id: "alice".into(),
            chain: ChainId::Eth,
            address_id: "addr-1".into(),
            destination_address: "0xabc123".into(),
            amount: 1_000_000_000_000_000_000_i128, // 1 ETH in wei
            estimated_fee: 21_000_i128 * 30_000_000_000_i128, // 21k gas × 30 gwei
            actual_fee: None,
            status: WithdrawalStatus::Submitted,
            submitted_at: ts(),
            updated_at: ts(),
            approved_at: None,
            broadcast_at: None,
            confirmed_at: None,
            settled_at: None,
            tx_hash: None,
            confirmations: 0,
            confirmations_required: 25,
            approval_request_id: None,
            rejection_reason: None,
            notes: None,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: WithdrawalRecord = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
        // Per-shot None fields are omitted.
        assert!(!s.contains("\"actual_fee\""));
        assert!(!s.contains("\"tx_hash\""));
        assert!(!s.contains("\"rejection_reason\""));
    }

    #[test]
    fn withdrawal_record_round_trip_full_lifecycle_settled() {
        let r = WithdrawalRecord {
            schema_version: WALLET_SCHEMA_VERSION,
            withdrawal_id: "wd-2".into(),
            user_id: "bob".into(),
            chain: ChainId::Eth,
            address_id: "addr-2".into(),
            destination_address: "0xdef456".into(),
            amount: 500_000_000_000_000_000_i128,
            estimated_fee: 21_000_i128 * 30_000_000_000_i128,
            actual_fee: Some(21_000_i128 * 31_000_000_000_i128),
            status: WithdrawalStatus::Settled,
            submitted_at: ts(),
            updated_at: ts(),
            approved_at: Some(ts()),
            broadcast_at: Some(ts()),
            confirmed_at: Some(ts()),
            settled_at: Some(ts()),
            tx_hash: Some("0xtx-hash-here".into()),
            confirmations: 30,
            confirmations_required: 25,
            approval_request_id: Some("appr-7".into()),
            rejection_reason: None,
            notes: Some("settled cleanly".into()),
        };
        let bytes = serde_json::to_vec(&r).unwrap();
        let back: WithdrawalRecord = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn withdrawal_record_round_trip_rejected() {
        let r = WithdrawalRecord {
            schema_version: WALLET_SCHEMA_VERSION,
            withdrawal_id: "wd-rej".into(),
            user_id: "carol".into(),
            chain: ChainId::Eth,
            address_id: "addr-x".into(),
            destination_address: "0xnope".into(),
            amount: 1_i128,
            estimated_fee: 1_i128,
            actual_fee: None,
            status: WithdrawalStatus::Rejected,
            submitted_at: ts(),
            updated_at: ts(),
            approved_at: None,
            broadcast_at: None,
            confirmed_at: None,
            settled_at: None,
            tx_hash: None,
            confirmations: 0,
            confirmations_required: 25,
            approval_request_id: None,
            rejection_reason: Some(WithdrawalRejectReason::SanctionsHit),
            notes: Some("destination on OFAC SDN at submit-time recheck".into()),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: WithdrawalRecord = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
        assert!(s.contains("\"sanctions_hit\""));
    }

    #[test]
    fn fee_urgency_serialises_as_snake_case() {
        assert_eq!(serde_json::to_string(&FeeUrgency::Economy).unwrap(), "\"economy\"");
        assert_eq!(serde_json::to_string(&FeeUrgency::Standard).unwrap(), "\"standard\"");
        assert_eq!(serde_json::to_string(&FeeUrgency::Urgent).unwrap(), "\"urgent\"");
    }

    #[test]
    fn amount_can_hold_btc_supply_in_satoshis() {
        // 21M BTC × 1e8 sat = 2.1e15. i128 holds this with ~120 bits to spare.
        let max_supply_sat: i128 = 21_000_000_i128 * 100_000_000_i128;
        assert!(max_supply_sat < i128::MAX / 1000);
    }
}
