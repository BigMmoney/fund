//! Order Flow Monitor — api ingress trace helpers.
//!
//! Step 4 of `docs/MONITOR_DESIGN.md` §7. Centralizes the api-side emit
//! calls so the trading handlers do not each carry inline trace
//! construction boilerplate. All helpers take the production
//! `eventbus::EventBus` directly and publish `Event::OrderTrace`.
//!
//! Pre-sequencer events emit with `order_id = None`. The projector
//! buffers them by `request_id` in its `by_trace_key` index until
//! matching assigns the canonical `order_id` and the binding event
//! flushes the bucket — see design §3.3.1.

use eventbus::EventBus;
use types::{
    ApiErrorCode, AuthenticatedPrincipal, Event, OrderTraceEvent, OrderTraceStage, Side,
};

/// Emit `api_received` for a new-order submission. Called at the very
/// top of the handler, after `with_principal` has attached the
/// authenticated principal but before any rate limit / validation /
/// sequencing happens.
pub(crate) fn emit_new_order_received(
    event_bus: &EventBus,
    request_id: &str,
    client_order_id: Option<&str>,
    principal: &AuthenticatedPrincipal,
    market_id: &str,
    outcome: i32,
    side: Side,
    price: Option<i64>,
    amount: i64,
) {
    let mut ev = OrderTraceEvent::new_unbound(OrderTraceStage::ApiReceived);
    ev.request_id = Some(request_id.to_string());
    ev.client_order_id = client_order_id.map(String::from);
    ev.user_id = Some(principal.subject.clone());
    ev.session_id = principal.session_id.clone();
    ev.market_id = Some(market_id.to_string());
    ev.outcome = Some(outcome);
    ev.side = Some(side);
    ev.price = price;
    ev.amount = Some(amount);
    event_bus.publish(Event::OrderTrace(ev));
}

/// Emit `api_validated` after every pre-flight check (rate limit,
/// sentinel, instrument lookup, field validation, beta controls)
/// passes — i.e. immediately before the request is handed to the
/// sequencer.
pub(crate) fn emit_new_order_validated(
    event_bus: &EventBus,
    request_id: &str,
    client_order_id: Option<&str>,
    principal: &AuthenticatedPrincipal,
    market_id: &str,
    outcome: i32,
    side: Side,
    price: Option<i64>,
    amount: i64,
) {
    let mut ev = OrderTraceEvent::new_unbound(OrderTraceStage::ApiValidated);
    ev.request_id = Some(request_id.to_string());
    ev.client_order_id = client_order_id.map(String::from);
    ev.user_id = Some(principal.subject.clone());
    ev.session_id = principal.session_id.clone();
    ev.market_id = Some(market_id.to_string());
    ev.outcome = Some(outcome);
    ev.side = Some(side);
    ev.price = price;
    ev.amount = Some(amount);
    event_bus.publish(Event::OrderTrace(ev));
}

/// Emit `api_received` for a request that targets a known `order_id`
/// (cancel / replace). The projector applies these directly to the
/// existing order's timeline since `order_id` is bound from the start.
pub(crate) fn emit_for_order_received(
    event_bus: &EventBus,
    order_id: &str,
    request_id: &str,
    principal: &AuthenticatedPrincipal,
    market_id: Option<&str>,
    outcome: Option<i32>,
) {
    let mut ev = OrderTraceEvent::new(OrderTraceStage::ApiReceived, order_id);
    ev.request_id = Some(request_id.to_string());
    ev.user_id = Some(principal.subject.clone());
    ev.session_id = principal.session_id.clone();
    ev.market_id = market_id.map(String::from);
    ev.outcome = outcome;
    event_bus.publish(Event::OrderTrace(ev));
}

/// Emit `api_validated` for a known-order request.
pub(crate) fn emit_for_order_validated(
    event_bus: &EventBus,
    order_id: &str,
    request_id: &str,
    principal: &AuthenticatedPrincipal,
    market_id: Option<&str>,
    outcome: Option<i32>,
) {
    let mut ev = OrderTraceEvent::new(OrderTraceStage::ApiValidated, order_id);
    ev.request_id = Some(request_id.to_string());
    ev.user_id = Some(principal.subject.clone());
    ev.session_id = principal.session_id.clone();
    ev.market_id = market_id.map(String::from);
    ev.outcome = outcome;
    event_bus.publish(Event::OrderTrace(ev));
}

/// Emit `api_rejected` for a known-order request that fails at the
/// engine or sequencer boundary.
pub(crate) fn emit_for_order_rejected(
    event_bus: &EventBus,
    order_id: &str,
    request_id: &str,
    user_id: &str,
    code: ApiErrorCode,
    message: impl Into<String>,
) {
    let mut ev = OrderTraceEvent::new(OrderTraceStage::ApiRejected, order_id);
    ev.request_id = Some(request_id.to_string());
    ev.user_id = Some(user_id.to_string());
    ev.reject_code = Some(code);
    ev.reject_message = Some(message.into());
    event_bus.publish(Event::OrderTrace(ev));
}

/// Emit `api_rejected` for a request that has not yet been assigned a
/// canonical `order_id` — e.g. new-order submissions that fail at the
/// engine boundary or in the sequencer, OR mass-cancel commands which
/// span multiple orders. The projector correlates via `request_id`
/// (design §3.3.1).
pub(crate) fn emit_api_rejected_unbound(
    event_bus: &EventBus,
    request_id: &str,
    client_order_id: Option<&str>,
    user_id: Option<&str>,
    code: ApiErrorCode,
    message: impl Into<String>,
) {
    let mut ev = OrderTraceEvent::new_unbound(OrderTraceStage::ApiRejected);
    ev.request_id = Some(request_id.to_string());
    ev.client_order_id = client_order_id.map(String::from);
    ev.user_id = user_id.map(String::from);
    ev.reject_code = Some(code);
    ev.reject_message = Some(message.into());
    event_bus.publish(Event::OrderTrace(ev));
}
