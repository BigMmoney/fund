// Step 3A scaffold: no producers wired yet (those land in Steps 4–9).
// Until then, public surface here is reachable only from the unit tests in
// this file. Lift this when REST handlers / emit sites land.
#![allow(dead_code)]

//! Order Flow Monitor — in-memory projector.
//!
//! Step 3A of the implementation ladder in `docs/MONITOR_DESIGN.md`. This
//! module is observer-only: nothing here calls into matching, sequencer,
//! ledger, persistence, bootstrap, or the request path. There are no
//! producers wired in this commit; `apply_event` is invoked only by future
//! commits and by this file's own unit tests.
//!
//! Scope of this commit (Step 3A):
//! - `OrderTraceState` — per-order summary + timeline.
//! - `OrderTraceProjector` — `apply_event`, `list_orders`, `get_order`,
//!   `get_timeline`.
//! - Pre-sequencer trace-key buffer per design §3.3.1: events without an
//!   `order_id` are held in `by_trace_key` keyed by `request_id`
//!   (or `client_order_id` fallback) and flushed into the order timeline
//!   when `sequencer_accepted` carries both.
//! - Tiered eviction per design §4.1: terminal-first, then stale buckets,
//!   then non-terminal only on hard cap.
//!
//! Out of scope (later steps, do not implement here):
//! - JSONL writer task (Step 3B).
//! - REST handlers + route mounting (Step 3C).
//! - WS endpoint (Step 10).
//! - Per-stage emit sites (Steps 4–9).
//! - `/metrics` counter wiring.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

use types::{OrderTraceEvent, OrderTraceStage};

/// Default soft cap on tracked orders. Eviction starts when the projector
/// crosses this on the next write. See design §4.1.
pub(crate) const DEFAULT_SOFT_CAP: usize = 10_000;

/// Default hard cap on tracked orders. Once reached, non-terminal eviction
/// becomes allowed as a last resort.
pub(crate) const DEFAULT_HARD_CAP: usize = 20_000;

/// Default cap on pre-sequencer trace-key buckets.
pub(crate) const DEFAULT_TRACE_KEY_CAP: usize = 4_096;

/// Default per-order timeline length cap. A single order should not retain
/// an unbounded number of `matching_partially_filled` events.
pub(crate) const DEFAULT_TIMELINE_CAP: usize = 1_024;

/// Tunable knobs. All have sensible defaults; v1 callers do not need to
/// override these.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OrderTraceConfig {
    pub(crate) soft_cap: usize,
    pub(crate) hard_cap: usize,
    pub(crate) trace_key_cap: usize,
    pub(crate) timeline_cap: usize,
}

impl Default for OrderTraceConfig {
    fn default() -> Self {
        Self {
            soft_cap: DEFAULT_SOFT_CAP,
            hard_cap: DEFAULT_HARD_CAP,
            trace_key_cap: DEFAULT_TRACE_KEY_CAP,
            timeline_cap: DEFAULT_TIMELINE_CAP,
        }
    }
}

/// Tracked state for a single order.
#[derive(Debug, Clone)]
pub(crate) struct OrderTraceState {
    pub(crate) order_id: String,
    pub(crate) client_order_id: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) market_id: Option<String>,
    pub(crate) current_stage: OrderTraceStage,
    pub(crate) command_seq: Option<u64>,
    pub(crate) remaining_amount: Option<i64>,
    pub(crate) fill_count: u32,
    pub(crate) first_seen_at: DateTime<Utc>,
    pub(crate) last_updated_at: DateTime<Utc>,
    pub(crate) timeline: VecDeque<OrderTraceEvent>,
    pub(crate) terminal: bool,
}

impl OrderTraceState {
    fn new(seed: &OrderTraceEvent, order_id: String) -> Self {
        Self {
            order_id,
            client_order_id: seed.client_order_id.clone(),
            user_id: seed.user_id.clone(),
            market_id: seed.market_id.clone(),
            current_stage: seed.stage,
            command_seq: seed.command_seq,
            remaining_amount: seed.remaining_amount,
            fill_count: 0,
            first_seen_at: seed.recorded_at,
            last_updated_at: seed.recorded_at,
            timeline: VecDeque::new(),
            terminal: false,
        }
    }

    fn merge(&mut self, ev: &OrderTraceEvent, timeline_cap: usize) {
        if self.client_order_id.is_none() {
            self.client_order_id = ev.client_order_id.clone();
        }
        if self.user_id.is_none() {
            self.user_id = ev.user_id.clone();
        }
        if self.market_id.is_none() {
            self.market_id = ev.market_id.clone();
        }
        if ev.command_seq.is_some() {
            self.command_seq = ev.command_seq;
        }
        if ev.remaining_amount.is_some() {
            self.remaining_amount = ev.remaining_amount;
        }
        if ev.recorded_at >= self.last_updated_at {
            self.last_updated_at = ev.recorded_at;
            self.current_stage = ev.stage;
        }
        if matches!(
            ev.stage,
            OrderTraceStage::MatchingPartiallyFilled | OrderTraceStage::MatchingFilled
        ) {
            self.fill_count = self.fill_count.saturating_add(1);
        }
        if is_terminal_stage(ev.stage) {
            self.terminal = true;
        }
        self.timeline.push_back(ev.clone());
        while self.timeline.len() > timeline_cap {
            self.timeline.pop_front();
        }
    }
}

fn is_terminal_stage(stage: OrderTraceStage) -> bool {
    matches!(
        stage,
        OrderTraceStage::ApiRejected
            | OrderTraceStage::MatchingFilled
            | OrderTraceStage::MatchingCancelled
    )
}

/// In-memory aggregator that turns a stream of `OrderTraceEvent`s into a
/// per-order timeline plus quick lookup indices. All operations are
/// non-blocking and never await — fire-and-forget on the producer side.
pub(crate) struct OrderTraceProjector {
    by_order: DashMap<String, OrderTraceState>,
    by_trace_key: DashMap<String, TraceKeyBucket>,
    recent_order_ids: Mutex<VecDeque<String>>,
    config: OrderTraceConfig,
}

#[derive(Debug, Clone)]
struct TraceKeyBucket {
    events: Vec<OrderTraceEvent>,
    last_updated_at: DateTime<Utc>,
}

impl OrderTraceProjector {
    pub(crate) fn new() -> Arc<Self> {
        Self::with_config(OrderTraceConfig::default())
    }

    pub(crate) fn with_config(config: OrderTraceConfig) -> Arc<Self> {
        Arc::new(Self {
            by_order: DashMap::new(),
            by_trace_key: DashMap::new(),
            recent_order_ids: Mutex::new(VecDeque::new()),
            config,
        })
    }

    /// Apply one trace event. Never blocks, never awaits, never returns
    /// errors — observer-only semantics. Callers (future emit sites) must
    /// not depend on the success of this operation.
    pub(crate) fn apply_event(&self, ev: OrderTraceEvent) {
        match (ev.order_id.clone(), trace_key_of(&ev)) {
            (Some(order_id), _) => self.apply_with_order_id(ev, order_id),
            (None, Some(key)) => self.buffer_pre_sequencer(ev, key),
            (None, None) => {
                // No order_id and no correlation key — nothing to bind to.
                // Drop silently; the producer should always provide at least
                // a request_id at api ingress per design §3.3.
            }
        }
    }

    fn apply_with_order_id(&self, ev: OrderTraceEvent, order_id: String) {
        // If a pre-sequencer bucket exists for this request, drain it into
        // the order timeline before applying the current event.
        if matches!(ev.stage, OrderTraceStage::SequencerAccepted) {
            if let Some(key) = trace_key_of(&ev) {
                if let Some((_k, bucket)) = self.by_trace_key.remove(&key) {
                    self.merge_buffered_events(&order_id, bucket.events);
                }
            }
        }

        let new_inserted;
        {
            let mut entry = self
                .by_order
                .entry(order_id.clone())
                .or_insert_with(|| OrderTraceState::new(&ev, order_id.clone()));
            new_inserted = entry.timeline.is_empty()
                && entry.first_seen_at == entry.last_updated_at
                && entry.current_stage == ev.stage;
            entry.merge(&ev, self.config.timeline_cap);
        }

        if new_inserted {
            self.recent_order_ids.lock().push_back(order_id);
            self.maybe_evict();
        }
    }

    fn merge_buffered_events(&self, order_id: &str, mut events: Vec<OrderTraceEvent>) {
        events.sort_by_key(|e| e.recorded_at);
        for buffered in events {
            let mut entry = self
                .by_order
                .entry(order_id.to_string())
                .or_insert_with(|| OrderTraceState::new(&buffered, order_id.to_string()));
            entry.merge(&buffered, self.config.timeline_cap);
        }
    }

    fn buffer_pre_sequencer(&self, ev: OrderTraceEvent, key: String) {
        let recorded_at = ev.recorded_at;
        self.by_trace_key
            .entry(key)
            .and_modify(|bucket| {
                bucket.events.push(ev.clone());
                bucket.last_updated_at = recorded_at;
            })
            .or_insert_with(|| TraceKeyBucket {
                events: vec![ev],
                last_updated_at: recorded_at,
            });

        // Hard cap on the bucket index. If exceeded, evict the oldest bucket
        // by `last_updated_at`. This is best-effort — pre-sequencer
        // instrumentation must never block on monitor state.
        if self.by_trace_key.len() > self.config.trace_key_cap {
            let oldest = self
                .by_trace_key
                .iter()
                .min_by_key(|e| e.value().last_updated_at)
                .map(|e| e.key().clone());
            if let Some(key) = oldest {
                self.by_trace_key.remove(&key);
            }
        }
    }

    /// Tier-1 (terminal) and tier-3 (open under hard cap) eviction.
    /// Tier-2 (stale buckets) is age-based and runs independently.
    fn maybe_evict(&self) {
        if self.by_order.len() <= self.config.soft_cap {
            return;
        }

        // Tier 1: drop terminal entries oldest-first.
        let mut order = self.recent_order_ids.lock();
        let mut i = 0;
        while self.by_order.len() > self.config.soft_cap && i < order.len() {
            let candidate = &order[i];
            let drop = self
                .by_order
                .get(candidate)
                .map(|s| s.terminal)
                .unwrap_or(true);
            if drop {
                let removed = order.remove(i);
                if let Some(id) = removed {
                    self.by_order.remove(&id);
                }
            } else {
                i += 1;
            }
        }

        // Tier 3: only when over hard cap, evict oldest non-terminal.
        if self.by_order.len() >= self.config.hard_cap {
            while self.by_order.len() > self.config.soft_cap {
                if let Some(id) = order.pop_front() {
                    self.by_order.remove(&id);
                } else {
                    break;
                }
            }
        }
    }

    // ── Read helpers (used by future REST handlers + by tests) ──────────

    /// Return summaries of tracked orders matching the supplied filters.
    /// Output is sorted by `last_updated_at` descending.
    pub(crate) fn list_orders(&self, filter: &OrderFilter) -> Vec<OrderSummary> {
        let mut out: Vec<OrderSummary> = self
            .by_order
            .iter()
            .filter_map(|entry| {
                let s = entry.value();
                if let Some(uid) = filter.user_id.as_deref() {
                    if s.user_id.as_deref() != Some(uid) {
                        return None;
                    }
                }
                if let Some(mid) = filter.market_id.as_deref() {
                    if s.market_id.as_deref() != Some(mid) {
                        return None;
                    }
                }
                if let Some(stage) = filter.stage {
                    if s.current_stage != stage {
                        return None;
                    }
                }
                if let Some(t) = filter.terminal {
                    if s.terminal != t {
                        return None;
                    }
                }
                if let Some(since) = filter.updated_since {
                    if s.last_updated_at < since {
                        return None;
                    }
                }
                Some(OrderSummary::from(s))
            })
            .collect();
        out.sort_by(|a, b| b.last_updated_at.cmp(&a.last_updated_at));
        let limit = filter.limit.unwrap_or(100).min(500);
        out.truncate(limit);
        out
    }

    /// Single-order summary (no timeline). `None` if unknown or evicted.
    pub(crate) fn get_order(&self, order_id: &str) -> Option<OrderSummary> {
        self.by_order
            .get(order_id)
            .map(|entry| OrderSummary::from(entry.value()))
    }

    /// Timeline window. `since_event_id`: return events strictly after the
    /// matching event id; if not present in the timeline, return the full
    /// retained window. `limit` clamps the response size.
    pub(crate) fn get_timeline(
        &self,
        order_id: &str,
        since_event_id: Option<&str>,
        limit: Option<usize>,
    ) -> Option<TimelinePage> {
        let entry = self.by_order.get(order_id)?;
        let s = entry.value();
        let max = limit.unwrap_or(200).min(1000);

        let start = match since_event_id {
            Some(eid) => s
                .timeline
                .iter()
                .position(|e| e.event_id == eid)
                .map(|i| i + 1)
                .unwrap_or(0),
            None => 0,
        };

        let events: Vec<OrderTraceEvent> = s.timeline.iter().skip(start).take(max).cloned().collect();
        let next_since_event_id = events.last().map(|e| e.event_id.clone());

        Some(TimelinePage {
            order_id: s.order_id.clone(),
            current_stage: s.current_stage,
            terminal: s.terminal,
            timeline: events,
            next_since_event_id,
        })
    }

    // ── Test introspection ──────────────────────────────────────────────

    #[cfg(test)]
    pub(crate) fn order_count(&self) -> usize {
        self.by_order.len()
    }

    #[cfg(test)]
    pub(crate) fn trace_key_bucket_count(&self) -> usize {
        self.by_trace_key.len()
    }
}

fn trace_key_of(ev: &OrderTraceEvent) -> Option<String> {
    ev.request_id
        .clone()
        .or_else(|| ev.client_order_id.clone())
}

#[derive(Debug, Default, Clone)]
pub(crate) struct OrderFilter {
    pub(crate) user_id: Option<String>,
    pub(crate) market_id: Option<String>,
    pub(crate) stage: Option<OrderTraceStage>,
    pub(crate) terminal: Option<bool>,
    pub(crate) updated_since: Option<DateTime<Utc>>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct OrderSummary {
    pub(crate) order_id: String,
    pub(crate) client_order_id: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) market_id: Option<String>,
    pub(crate) current_stage: OrderTraceStage,
    pub(crate) command_seq: Option<u64>,
    pub(crate) remaining_amount: Option<i64>,
    pub(crate) fill_count: u32,
    pub(crate) first_seen_at: DateTime<Utc>,
    pub(crate) last_updated_at: DateTime<Utc>,
    pub(crate) terminal: bool,
}

impl From<&OrderTraceState> for OrderSummary {
    fn from(s: &OrderTraceState) -> Self {
        Self {
            order_id: s.order_id.clone(),
            client_order_id: s.client_order_id.clone(),
            user_id: s.user_id.clone(),
            market_id: s.market_id.clone(),
            current_stage: s.current_stage,
            command_seq: s.command_seq,
            remaining_amount: s.remaining_amount,
            fill_count: s.fill_count,
            first_seen_at: s.first_seen_at,
            last_updated_at: s.last_updated_at,
            terminal: s.terminal,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TimelinePage {
    pub(crate) order_id: String,
    pub(crate) current_stage: OrderTraceStage,
    pub(crate) terminal: bool,
    pub(crate) timeline: Vec<OrderTraceEvent>,
    pub(crate) next_since_event_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn ev(stage: OrderTraceStage, order_id: Option<&str>, secs: i64) -> OrderTraceEvent {
        let mut e = OrderTraceEvent::new(stage, order_id.unwrap_or(""));
        e.order_id = order_id.map(|s| s.to_string());
        e.event_id = format!("evt-{}-{:?}", secs, stage);
        e.recorded_at = at(secs);
        e
    }

    #[test]
    fn apply_with_order_id_creates_state_and_appends_timeline() {
        let p = OrderTraceProjector::new();
        let mut e = ev(OrderTraceStage::SequencerAccepted, Some("ord-1"), 0);
        e.user_id = Some("alice".into());
        e.market_id = Some("btc-usdt".into());
        e.command_seq = Some(7);
        p.apply_event(e);

        let s = p.get_order("ord-1").expect("present");
        assert_eq!(s.order_id, "ord-1");
        assert_eq!(s.user_id.as_deref(), Some("alice"));
        assert_eq!(s.market_id.as_deref(), Some("btc-usdt"));
        assert_eq!(s.command_seq, Some(7));
        assert_eq!(s.current_stage, OrderTraceStage::SequencerAccepted);
        assert!(!s.terminal);
        let tl = p.get_timeline("ord-1", None, None).unwrap();
        assert_eq!(tl.timeline.len(), 1);
    }

    #[test]
    fn pre_sequencer_event_is_buffered_and_flushed_on_sequencer_accepted() {
        let p = OrderTraceProjector::new();
        let mut received = ev(OrderTraceStage::ApiReceived, None, 0);
        received.request_id = Some("req-9".into());
        received.client_order_id = Some("cli-9".into());
        p.apply_event(received);

        // Buffered, not yet bound to an order.
        assert_eq!(p.order_count(), 0);
        assert_eq!(p.trace_key_bucket_count(), 1);

        let mut accepted = ev(OrderTraceStage::SequencerAccepted, Some("ord-9"), 1);
        accepted.request_id = Some("req-9".into());
        accepted.client_order_id = Some("cli-9".into());
        accepted.command_seq = Some(42);
        p.apply_event(accepted);

        assert_eq!(p.trace_key_bucket_count(), 0);
        let tl = p.get_timeline("ord-9", None, None).unwrap();
        // The buffered ApiReceived is flushed in before SequencerAccepted.
        assert_eq!(tl.timeline.len(), 2);
        assert_eq!(tl.timeline[0].stage, OrderTraceStage::ApiReceived);
        assert_eq!(tl.timeline[1].stage, OrderTraceStage::SequencerAccepted);
        let s = p.get_order("ord-9").unwrap();
        // current_stage tracks the latest by recorded_at.
        assert_eq!(s.current_stage, OrderTraceStage::SequencerAccepted);
        assert_eq!(s.command_seq, Some(42));
    }

    #[test]
    fn fills_increment_fill_count_and_terminal_marks_state() {
        let p = OrderTraceProjector::new();
        p.apply_event(ev(OrderTraceStage::SequencerAccepted, Some("ord-2"), 0));
        p.apply_event(ev(OrderTraceStage::MatchingPartiallyFilled, Some("ord-2"), 1));
        p.apply_event(ev(OrderTraceStage::MatchingPartiallyFilled, Some("ord-2"), 2));
        p.apply_event(ev(OrderTraceStage::MatchingFilled, Some("ord-2"), 3));

        let s = p.get_order("ord-2").unwrap();
        assert_eq!(s.fill_count, 3);
        assert_eq!(s.current_stage, OrderTraceStage::MatchingFilled);
        assert!(s.terminal);
    }

    #[test]
    fn rejected_marks_terminal() {
        let p = OrderTraceProjector::new();
        p.apply_event(ev(OrderTraceStage::ApiRejected, Some("ord-3"), 0));
        let s = p.get_order("ord-3").unwrap();
        assert!(s.terminal);
        assert_eq!(s.current_stage, OrderTraceStage::ApiRejected);
    }

    #[test]
    fn list_orders_filters_and_sorts_by_last_updated_desc() {
        let p = OrderTraceProjector::new();
        let mut a = ev(OrderTraceStage::SequencerAccepted, Some("ord-a"), 0);
        a.user_id = Some("alice".into());
        a.market_id = Some("btc-usdt".into());
        let mut b = ev(OrderTraceStage::SequencerAccepted, Some("ord-b"), 1);
        b.user_id = Some("bob".into());
        b.market_id = Some("eth-usdt".into());
        let mut c = ev(OrderTraceStage::SequencerAccepted, Some("ord-c"), 2);
        c.user_id = Some("alice".into());
        c.market_id = Some("btc-usdt".into());
        p.apply_event(a);
        p.apply_event(b);
        p.apply_event(c);

        // Filter by user_id, sorted newest-first.
        let f = OrderFilter {
            user_id: Some("alice".into()),
            ..Default::default()
        };
        let listed = p.list_orders(&f);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].order_id, "ord-c");
        assert_eq!(listed[1].order_id, "ord-a");

        // Filter by market_id.
        let f = OrderFilter {
            market_id: Some("eth-usdt".into()),
            ..Default::default()
        };
        let listed = p.list_orders(&f);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].order_id, "ord-b");

        // Limit clamps.
        let f = OrderFilter {
            limit: Some(1),
            ..Default::default()
        };
        let listed = p.list_orders(&f);
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn get_timeline_since_event_id_returns_strictly_after() {
        let p = OrderTraceProjector::new();
        p.apply_event(ev(OrderTraceStage::SequencerAccepted, Some("ord-7"), 0));
        p.apply_event(ev(OrderTraceStage::MatchingResting, Some("ord-7"), 1));
        p.apply_event(ev(OrderTraceStage::MatchingFilled, Some("ord-7"), 2));

        let full = p.get_timeline("ord-7", None, None).unwrap();
        assert_eq!(full.timeline.len(), 3);
        let cutoff = full.timeline[0].event_id.clone();
        let after = p.get_timeline("ord-7", Some(&cutoff), None).unwrap();
        assert_eq!(after.timeline.len(), 2);
        assert_eq!(after.timeline[0].stage, OrderTraceStage::MatchingResting);
    }

    #[test]
    fn get_order_returns_none_for_unknown_id() {
        let p = OrderTraceProjector::new();
        assert!(p.get_order("nope").is_none());
        assert!(p.get_timeline("nope", None, None).is_none());
    }

    #[test]
    fn terminal_orders_evict_first_when_soft_cap_exceeded() {
        let p = OrderTraceProjector::with_config(OrderTraceConfig {
            soft_cap: 2,
            hard_cap: 100,
            trace_key_cap: 16,
            timeline_cap: 16,
        });
        // Two terminal orders + one open order. The next insertion crosses
        // the cap and should evict a terminal one first.
        p.apply_event(ev(OrderTraceStage::MatchingFilled, Some("term-1"), 0));
        p.apply_event(ev(OrderTraceStage::MatchingFilled, Some("term-2"), 1));
        p.apply_event(ev(OrderTraceStage::SequencerAccepted, Some("open-1"), 2));
        // Force an over-cap insertion.
        p.apply_event(ev(OrderTraceStage::SequencerAccepted, Some("open-2"), 3));

        // open-1 and open-2 must remain (non-terminal protected); at least
        // one of the terminal entries was dropped.
        assert!(p.get_order("open-1").is_some(), "open order must survive eviction");
        assert!(p.get_order("open-2").is_some(), "newest order must survive");
        let term_survivors = ["term-1", "term-2"]
            .iter()
            .filter(|id| p.get_order(id).is_some())
            .count();
        assert!(term_survivors < 2, "at least one terminal order should be evicted first");
    }

    #[test]
    fn pre_sequencer_only_event_with_request_id_is_buffered() {
        let p = OrderTraceProjector::new();
        let mut received = ev(OrderTraceStage::ApiReceived, None, 0);
        received.request_id = Some("req-x".into());
        p.apply_event(received);
        // No order_id ever assigned: the bucket persists until eviction
        // and is not queryable via get_order.
        assert_eq!(p.order_count(), 0);
        assert_eq!(p.trace_key_bucket_count(), 1);
    }

    #[test]
    fn event_with_no_order_id_and_no_trace_key_is_dropped_silently() {
        let p = OrderTraceProjector::new();
        let e = ev(OrderTraceStage::ApiReceived, None, 0);
        // No request_id, no client_order_id — nothing to bind to.
        p.apply_event(e);
        assert_eq!(p.order_count(), 0);
        assert_eq!(p.trace_key_bucket_count(), 0);
    }
}
