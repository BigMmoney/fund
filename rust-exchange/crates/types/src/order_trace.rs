//! Order trace events — type model for the real-time order flow monitor.
//!
//! See `docs/MONITOR_DESIGN.md` for the architecture, lifecycle stages, and
//! emission ladder. This module only defines the wire/storage shape; emission
//! sites and the projector live in follow-up commits.
//!
//! Design intent:
//! - One `OrderTraceEvent` is recorded per stage transition for a given order.
//! - Optional fields stay absent in JSON when unset, so per-stage payloads stay
//!   compact (a `MatchingResting` event does not carry a `reject_code`, etc.).
//! - `detail` is a free-form `serde_json::Value` for stage-specific extras
//!   (e.g. fill counterparties, replay source offsets) without forcing every
//!   producer to extend the strict schema.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiErrorCode, CommandLifecycle, Side};

/// Current schema version for `OrderTraceEvent`. Bump on breaking field
/// changes so old replay logs can be matched against the producing version.
pub const ORDER_TRACE_SCHEMA_VERSION: u32 = 1;

/// Per-stage label for an order's journey through the system.
///
/// Variants are ordered roughly along the happy-path timeline: api ingress →
/// sequencer → matching → projection/ledger → persistence, with recovery
/// stages tacked on for replay. They are *not* a totally-ordered enum — an
/// order may skip stages (e.g. an immediately-rejected order goes
/// `ApiReceived → ApiValidated? → ApiRejected` and stops there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderTraceStage {
    ApiReceived,
    ApiValidated,
    ApiRejected,
    SequencerAccepted,
    SequencerPersisted,
    MatchingResting,
    MatchingPartiallyFilled,
    MatchingFilled,
    MatchingCancelled,
    ProjectionUpdated,
    LedgerSettled,
    WalAppended,
    RecoveryReplayed,
    RecoverySkippedTerminal,
    RecoveryCompleted,
}

/// A single observation of one order at one stage.
///
/// Fields default to `None` when not applicable; producers populate only what
/// they know at the emission point. `detail` is reserved for stage-specific
/// JSON that does not warrant a strict-schema field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderTraceEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub recorded_at: DateTime<Utc>,

    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub client_order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub command_seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub market_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<i32>,

    pub stage: OrderTraceStage,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lifecycle: Option<CommandLifecycle>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub side: Option<Side>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub price: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remaining_amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub filled_amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fee: Option<i64>,

    #[serde(default, skip_serializing_if = "is_null_value")]
    pub detail: serde_json::Value,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reject_code: Option<ApiErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reject_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub elapsed_us_since_request: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub trace_id: Option<String>,
}

fn is_null_value(v: &serde_json::Value) -> bool {
    v.is_null()
}

impl OrderTraceEvent {
    /// Construct a minimal event for `stage` on `order_id`, stamped at the
    /// current wall-clock time with a freshly-generated `event_id`.
    pub fn new(stage: OrderTraceStage, order_id: impl Into<String>) -> Self {
        Self {
            schema_version: ORDER_TRACE_SCHEMA_VERSION,
            event_id: Uuid::new_v4().to_string(),
            recorded_at: Utc::now(),
            order_id: order_id.into(),
            client_order_id: None,
            user_id: None,
            session_id: None,
            request_id: None,
            command_seq: None,
            market_id: None,
            outcome: None,
            stage,
            lifecycle: None,
            side: None,
            price: None,
            amount: None,
            remaining_amount: None,
            filled_amount: None,
            fee: None,
            detail: serde_json::Value::Null,
            reject_code: None,
            reject_message: None,
            elapsed_us_since_request: None,
            trace_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stage_serializes_as_snake_case() {
        let s = serde_json::to_string(&OrderTraceStage::MatchingPartiallyFilled).unwrap();
        assert_eq!(s, "\"matching_partially_filled\"");
    }

    #[test]
    fn minimal_event_round_trip() {
        let mut ev = OrderTraceEvent::new(OrderTraceStage::ApiReceived, "ord-1");
        // Pin the timestamp for deterministic equality.
        ev.recorded_at = DateTime::parse_from_rfc3339("2026-04-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        ev.event_id = "evt-1".into();

        let json = serde_json::to_string(&ev).unwrap();
        let back: OrderTraceEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(back.schema_version, ORDER_TRACE_SCHEMA_VERSION);
        assert_eq!(back.event_id, "evt-1");
        assert_eq!(back.order_id, "ord-1");
        assert_eq!(back.stage, OrderTraceStage::ApiReceived);
        assert!(back.client_order_id.is_none());
        assert!(back.detail.is_null());
    }

    #[test]
    fn optional_fields_omitted_in_payload() {
        let ev = OrderTraceEvent::new(OrderTraceStage::ApiReceived, "ord-2");
        let json = serde_json::to_value(&ev).unwrap();
        // Optional fields and the null `detail` should not appear in the wire form.
        assert!(json.get("client_order_id").is_none());
        assert!(json.get("price").is_none());
        assert!(json.get("reject_code").is_none());
        assert!(json.get("detail").is_none());
        // Required fields are present.
        assert!(json.get("schema_version").is_some());
        assert!(json.get("event_id").is_some());
        assert!(json.get("order_id").is_some());
        assert!(json.get("stage").is_some());
        assert!(json.get("recorded_at").is_some());
    }

    #[test]
    fn populated_event_round_trip() {
        let mut ev = OrderTraceEvent::new(OrderTraceStage::MatchingPartiallyFilled, "ord-3");
        ev.recorded_at = DateTime::parse_from_rfc3339("2026-04-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        ev.event_id = "evt-3".into();
        ev.client_order_id = Some("cli-3".into());
        ev.user_id = Some("alice".into());
        ev.session_id = Some("sess-7".into());
        ev.request_id = Some("req-9".into());
        ev.command_seq = Some(42);
        ev.market_id = Some("btc-usdt".into());
        ev.outcome = Some(1);
        ev.lifecycle = Some(CommandLifecycle::Executed);
        ev.side = Some(Side::Buy);
        ev.price = Some(50_000);
        ev.amount = Some(10);
        ev.remaining_amount = Some(4);
        ev.filled_amount = Some(6);
        ev.fee = Some(3);
        ev.detail = json!({ "counterparty_order_id": "ord-4" });
        ev.elapsed_us_since_request = Some(1_523);
        ev.trace_id = Some("trace-aaa".into());

        let bytes = serde_json::to_vec(&ev).unwrap();
        let back: OrderTraceEvent = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(back.stage, OrderTraceStage::MatchingPartiallyFilled);
        assert_eq!(back.command_seq, Some(42));
        assert_eq!(back.lifecycle, Some(CommandLifecycle::Executed));
        assert_eq!(back.side, Some(Side::Buy));
        assert_eq!(back.price, Some(50_000));
        assert_eq!(back.remaining_amount, Some(4));
        assert_eq!(back.filled_amount, Some(6));
        assert_eq!(back.detail["counterparty_order_id"], "ord-4");
        assert_eq!(back.elapsed_us_since_request, Some(1_523));
    }

    #[test]
    fn rejected_event_carries_error_code() {
        let mut ev = OrderTraceEvent::new(OrderTraceStage::ApiRejected, "ord-bad");
        ev.reject_code = Some(ApiErrorCode::InsufficientFunds);
        ev.reject_message = Some("balance 0".into());

        let json = serde_json::to_string(&ev).unwrap();
        let back: OrderTraceEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.reject_code, Some(ApiErrorCode::InsufficientFunds));
        assert_eq!(back.reject_message.as_deref(), Some("balance 0"));
        // SCREAMING_SNAKE_CASE rendering from the parent enum is preserved.
        assert!(json.contains("\"INSUFFICIENT_FUNDS\""));
    }
}
