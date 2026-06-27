// Trade surveillance: consumes the EventBus and flags suspicious
// patterns that the matching engine cannot catch in-line.
//
// Why this module exists separate from `matching/`:
//   1. Surveillance is OUT-OF-BAND. False positives must not block the
//      hot path. Detection runs on a broadcast subscriber that can
//      lag without affecting matching latency.
//   2. Rules are policy. They tune per market / regulatory regime.
//      Keeping them in api/ means we don't churn the matching crate
//      to adjust thresholds.
//   3. Surveillance is a CONSUMER of `Event::FillCreated` and
//      `Event::OrderTrace` — it observes the firehose, it doesn't
//      produce.
//
// Rules implemented in v1:
//
//   * round_trip_wash — same user has both Buy and Sell on the same
//     `(market_id, outcome)` within `wash_window`, with the smaller
//     leg ≥ 90% of the larger. Pattern: user inflating volume by
//     trading with themselves (STP should have caught the within-user
//     case; this rule fires when STP somehow let it through, or when
//     the trade was via two sub-accounts of the same user that share
//     a session/principal but were misconfigured for STP).
//
//   * rapid_cancel — user submitted ≥ `rapid_cancel_min_count` orders
//     in the last `rapid_cancel_window` where each was cancelled
//     within < `rapid_cancel_lifetime_ms` of submission and saw zero
//     fills. Pattern: spoofing / quote-stuffing — putting up size to
//     move the book then pulling it before getting hit.
//
//   * high_cancel_ratio — within the last `cancel_ratio_window`,
//     user's cancel-to-submit ratio is > `cancel_ratio_threshold`
//     with at least `cancel_ratio_min_events` events seen. Pattern:
//     layering — repeated stacking + pulling without intent to fill.
//
// Alerts are tracing::warn + a Prometheus counter
// (`surveillance_alerts_total{rule}`). They do not block, do not
// freeze accounts, do not pause the user. Operators triage from the
// alerts dashboard. Auto-action is intentionally out of scope for
// v1 — a false positive auto-freezing a customer is worse than a
// missed detection.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use eventbus::EventBus;
use types::{Event, OrderTraceStage, Side};

#[derive(Debug, Clone)]
pub struct SurveillanceConfig {
    pub enabled: bool,

    pub wash_window: Duration,
    pub wash_overlap_pct: u32,

    pub rapid_cancel_window: Duration,
    pub rapid_cancel_lifetime_ms: u64,
    pub rapid_cancel_min_count: usize,

    pub cancel_ratio_window: Duration,
    pub cancel_ratio_threshold_pct: u32,
    pub cancel_ratio_min_events: usize,

    /// Hard cap on per-user history queue length to bound memory.
    pub max_events_per_user: usize,
}

impl Default for SurveillanceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            wash_window: Duration::from_secs(60),
            wash_overlap_pct: 90,
            rapid_cancel_window: Duration::from_secs(60),
            rapid_cancel_lifetime_ms: 500,
            rapid_cancel_min_count: 10,
            cancel_ratio_window: Duration::from_secs(300),
            cancel_ratio_threshold_pct: 90,
            cancel_ratio_min_events: 50,
            max_events_per_user: 1_000,
        }
    }
}

impl SurveillanceConfig {
    /// Override defaults from env. All env vars are optional.
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("SURVEILLANCE_ENABLED") {
            cfg.enabled = matches!(v.to_lowercase().as_str(), "1" | "true" | "yes");
        }
        if let Some(v) = parse_env_u64("SURVEILLANCE_WASH_WINDOW_SECS") {
            cfg.wash_window = Duration::from_secs(v.max(1));
        }
        if let Some(v) = parse_env_u32("SURVEILLANCE_WASH_OVERLAP_PCT") {
            cfg.wash_overlap_pct = v.clamp(50, 100);
        }
        if let Some(v) = parse_env_u64("SURVEILLANCE_RAPID_CANCEL_WINDOW_SECS") {
            cfg.rapid_cancel_window = Duration::from_secs(v.max(1));
        }
        if let Some(v) = parse_env_u64("SURVEILLANCE_RAPID_CANCEL_LIFETIME_MS") {
            cfg.rapid_cancel_lifetime_ms = v.max(50);
        }
        if let Some(v) = parse_env_usize("SURVEILLANCE_RAPID_CANCEL_MIN_COUNT") {
            cfg.rapid_cancel_min_count = v.max(2);
        }
        if let Some(v) = parse_env_u64("SURVEILLANCE_CANCEL_RATIO_WINDOW_SECS") {
            cfg.cancel_ratio_window = Duration::from_secs(v.max(1));
        }
        if let Some(v) = parse_env_u32("SURVEILLANCE_CANCEL_RATIO_PCT") {
            cfg.cancel_ratio_threshold_pct = v.clamp(50, 100);
        }
        if let Some(v) = parse_env_usize("SURVEILLANCE_CANCEL_RATIO_MIN_EVENTS") {
            cfg.cancel_ratio_min_events = v.max(2);
        }
        if let Some(v) = parse_env_usize("SURVEILLANCE_MAX_EVENTS_PER_USER") {
            cfg.max_events_per_user = v.max(100);
        }
        cfg
    }
}

fn parse_env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok().and_then(|v| v.parse::<u64>().ok())
}
fn parse_env_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok().and_then(|v| v.parse::<u32>().ok())
}
fn parse_env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok().and_then(|v| v.parse::<usize>().ok())
}

/// Single fill observation for the wash-trade rule.
#[derive(Debug, Clone)]
struct FillRecord {
    timestamp: DateTime<Utc>,
    market_id: String,
    outcome: i32,
    side: Side,
    amount: i64,
}

/// Submit/cancel pair tracking for the rapid_cancel rule.
#[derive(Debug, Clone)]
struct OrderLifecycle {
    submitted_at: Option<DateTime<Utc>>,
    cancelled_at: Option<DateTime<Utc>>,
    filled_amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AlertRule {
    RoundTripWash,
    RapidCancel,
    HighCancelRatio,
}

impl AlertRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertRule::RoundTripWash => "round_trip_wash",
            AlertRule::RapidCancel => "rapid_cancel",
            AlertRule::HighCancelRatio => "high_cancel_ratio",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Alert {
    pub rule: AlertRule,
    pub user_id: String,
    pub recorded_at: DateTime<Utc>,
    pub detail: String,
}

#[derive(Default)]
struct UserState {
    fills: VecDeque<FillRecord>,
    orders: std::collections::HashMap<String, OrderLifecycle>,
    /// Rolling submit/cancel counts within the cancel-ratio window.
    /// `(timestamp, kind)` where kind: 0 = submit, 1 = cancel.
    events: VecDeque<(DateTime<Utc>, u8)>,
}

pub struct Surveillance {
    config: SurveillanceConfig,
    per_user: DashMap<String, UserState>,
    alerts: parking_lot::Mutex<VecDeque<Alert>>,
    /// On-alert callback. Default behaviour logs via `tracing::warn`
    /// and increments the Prometheus metric — wired in
    /// `record_alert_default`.
    on_alert: Arc<dyn Fn(&Alert) + Send + Sync>,
}

impl Surveillance {
    pub fn new(config: SurveillanceConfig) -> Self {
        Self::with_callback(config, Arc::new(record_alert_default))
    }

    pub fn with_callback(
        config: SurveillanceConfig,
        on_alert: Arc<dyn Fn(&Alert) + Send + Sync>,
    ) -> Self {
        Self {
            config,
            per_user: DashMap::new(),
            alerts: parking_lot::Mutex::new(VecDeque::with_capacity(256)),
            on_alert,
        }
    }

    /// Number of alerts kept in the rolling buffer (for /metrics endpoint
    /// queries; not for alert delivery).
    pub fn recent_alert_count(&self) -> usize {
        self.alerts.lock().len()
    }

    pub fn drain_recent_alerts(&self) -> Vec<Alert> {
        let mut guard = self.alerts.lock();
        guard.drain(..).collect()
    }

    /// Process a fill event. Updates per-user fill history and runs the
    /// wash-trade rule. The fill itself does NOT carry buyer+seller
    /// identity (the matching engine emits one Fill per side), so we
    /// track the side from `fill.side` and look for the complementary
    /// side in the user's recent history.
    pub fn on_fill(&self, fill: &types::Fill) {
        if !self.config.enabled {
            return;
        }
        let now = fill.timestamp;
        let mut entry = self.per_user.entry(fill.user_id.clone()).or_default();
        entry.fills.push_back(FillRecord {
            timestamp: now,
            market_id: fill.market_id.clone(),
            outcome: fill.outcome,
            side: fill.side,
            amount: fill.amount,
        });
        evict_old_fills(&mut entry.fills, now, self.config.wash_window);
        truncate_to_cap(&mut entry.fills, self.config.max_events_per_user);

        // Mark the order as having filled (consumes any pending
        // rapid_cancel suspicion).
        if let Some(lifecycle) = entry.orders.get_mut(&fill.intent_id) {
            lifecycle.filled_amount = lifecycle.filled_amount.saturating_add(fill.amount);
        }

        // Apply wash rule.
        if let Some(alert) = self.check_round_trip_wash(&fill.user_id, &entry) {
            self.fire(alert);
        }
    }

    /// Process an order trace event. Only `MatchingResting` /
    /// `MatchingPartiallyFilled` and `MatchingCancelled` are
    /// interesting for the rapid-cancel rule.
    pub fn on_trace(&self, trace: &types::OrderTraceEvent) {
        if !self.config.enabled {
            return;
        }
        let user_id = match &trace.user_id {
            Some(u) => u.clone(),
            None => return,
        };
        let order_id = match &trace.order_id {
            Some(o) => o.clone(),
            None => return,
        };
        let now = trace.recorded_at;

        let mut entry = self.per_user.entry(user_id.clone()).or_default();

        match trace.stage {
            OrderTraceStage::MatchingResting | OrderTraceStage::MatchingPartiallyFilled => {
                let lifecycle = entry.orders.entry(order_id.clone()).or_insert(OrderLifecycle {
                    submitted_at: Some(now),
                    cancelled_at: None,
                    filled_amount: trace.filled_amount.unwrap_or(0),
                });
                if lifecycle.submitted_at.is_none() {
                    lifecycle.submitted_at = Some(now);
                }
                entry.events.push_back((now, 0));
            }
            OrderTraceStage::MatchingCancelled => {
                let lifecycle = entry.orders.entry(order_id.clone()).or_insert(OrderLifecycle {
                    submitted_at: None,
                    cancelled_at: Some(now),
                    filled_amount: 0,
                });
                lifecycle.cancelled_at = Some(now);
                entry.events.push_back((now, 1));
            }
            _ => return,
        }

        evict_old_events(&mut entry.events, now, self.config.cancel_ratio_window);
        truncate_to_cap_events(&mut entry.events, self.config.max_events_per_user);
        truncate_orders_map(&mut entry.orders, self.config.max_events_per_user);

        if let Some(alert) = self.check_rapid_cancel(&user_id, &entry, now) {
            self.fire(alert);
        }
        if let Some(alert) = self.check_high_cancel_ratio(&user_id, &entry) {
            self.fire(alert);
        }
    }

    fn fire(&self, alert: Alert) {
        (self.on_alert)(&alert);
        let mut guard = self.alerts.lock();
        guard.push_back(alert);
        // Cap the in-memory buffer so /metrics queries don't grow
        // unbounded between drains.
        while guard.len() > 256 {
            guard.pop_front();
        }
    }

    fn check_round_trip_wash(&self, user_id: &str, state: &UserState) -> Option<Alert> {
        use std::collections::HashMap;
        // Per (market_id, outcome), sum Buy amount + Sell amount in window.
        let mut totals: HashMap<(String, i32), (i64, i64)> = HashMap::new();
        for record in &state.fills {
            let entry = totals
                .entry((record.market_id.clone(), record.outcome))
                .or_default();
            match record.side {
                Side::Buy => entry.0 = entry.0.saturating_add(record.amount),
                Side::Sell => entry.1 = entry.1.saturating_add(record.amount),
            }
        }
        for ((market_id, outcome), (buy, sell)) in totals {
            if buy == 0 || sell == 0 {
                continue;
            }
            let smaller = buy.min(sell);
            let larger = buy.max(sell);
            if larger == 0 {
                continue;
            }
            let overlap_pct = (smaller.saturating_mul(100) / larger) as u32;
            if overlap_pct >= self.config.wash_overlap_pct {
                return Some(Alert {
                    rule: AlertRule::RoundTripWash,
                    user_id: user_id.to_string(),
                    recorded_at: Utc::now(),
                    detail: format!(
                        "market={market_id} outcome={outcome} buy={buy} sell={sell} overlap_pct={overlap_pct}"
                    ),
                });
            }
        }
        None
    }

    fn check_rapid_cancel(
        &self,
        user_id: &str,
        state: &UserState,
        now: DateTime<Utc>,
    ) -> Option<Alert> {
        let window_start = now - chrono::Duration::from_std(self.config.rapid_cancel_window).ok()?;
        let lifetime_threshold =
            chrono::Duration::milliseconds(self.config.rapid_cancel_lifetime_ms as i64);
        let mut suspicious = 0usize;
        for lifecycle in state.orders.values() {
            let (Some(submit), Some(cancel)) = (lifecycle.submitted_at, lifecycle.cancelled_at)
            else {
                continue;
            };
            if submit < window_start {
                continue;
            }
            if cancel - submit > lifetime_threshold {
                continue;
            }
            if lifecycle.filled_amount > 0 {
                continue;
            }
            suspicious += 1;
        }
        if suspicious >= self.config.rapid_cancel_min_count {
            return Some(Alert {
                rule: AlertRule::RapidCancel,
                user_id: user_id.to_string(),
                recorded_at: Utc::now(),
                detail: format!(
                    "{suspicious} unfilled orders cancelled within {}ms in last {}s",
                    self.config.rapid_cancel_lifetime_ms,
                    self.config.rapid_cancel_window.as_secs()
                ),
            });
        }
        None
    }

    fn check_high_cancel_ratio(&self, user_id: &str, state: &UserState) -> Option<Alert> {
        let total = state.events.len();
        if total < self.config.cancel_ratio_min_events {
            return None;
        }
        let cancels = state.events.iter().filter(|(_, kind)| *kind == 1).count();
        let pct = (cancels.saturating_mul(100) / total.max(1)) as u32;
        if pct >= self.config.cancel_ratio_threshold_pct {
            return Some(Alert {
                rule: AlertRule::HighCancelRatio,
                user_id: user_id.to_string(),
                recorded_at: Utc::now(),
                detail: format!(
                    "cancel/submit ratio = {cancels}/{total} = {pct}% in last {}s",
                    self.config.cancel_ratio_window.as_secs()
                ),
            });
        }
        None
    }
}

fn record_alert_default(alert: &Alert) {
    crate::observability::METRICS.record_surveillance_alert(alert.rule.as_str());
    tracing::warn!(
        rule = alert.rule.as_str(),
        user_id = %alert.user_id,
        detail = %alert.detail,
        "surveillance alert"
    );
}

fn evict_old_fills(fills: &mut VecDeque<FillRecord>, now: DateTime<Utc>, window: Duration) {
    let cutoff = match chrono::Duration::from_std(window) {
        Ok(d) => now - d,
        Err(_) => return,
    };
    while fills.front().is_some_and(|f| f.timestamp < cutoff) {
        fills.pop_front();
    }
}

fn evict_old_events(
    events: &mut VecDeque<(DateTime<Utc>, u8)>,
    now: DateTime<Utc>,
    window: Duration,
) {
    let cutoff = match chrono::Duration::from_std(window) {
        Ok(d) => now - d,
        Err(_) => return,
    };
    while events.front().is_some_and(|(ts, _)| *ts < cutoff) {
        events.pop_front();
    }
}

fn truncate_to_cap(fills: &mut VecDeque<FillRecord>, cap: usize) {
    while fills.len() > cap {
        fills.pop_front();
    }
}

fn truncate_to_cap_events(events: &mut VecDeque<(DateTime<Utc>, u8)>, cap: usize) {
    while events.len() > cap {
        events.pop_front();
    }
}

fn truncate_orders_map(orders: &mut std::collections::HashMap<String, OrderLifecycle>, cap: usize) {
    if orders.len() <= cap {
        return;
    }
    // Drop oldest by submitted_at if available, else by cancelled_at.
    let drop_count = orders.len() - cap;
    let mut keys: Vec<_> = orders
        .iter()
        .map(|(k, v)| (k.clone(), v.submitted_at.or(v.cancelled_at)))
        .collect();
    keys.sort_by_key(|(_, ts)| *ts);
    for (k, _) in keys.into_iter().take(drop_count) {
        orders.remove(&k);
    }
}

/// Spawn a long-running task that subscribes to `fill.created` and
/// `order.trace` and feeds events to the surveillance instance.
pub fn spawn_surveillance_task(surveillance: Arc<Surveillance>, bus: &EventBus) {
    if !surveillance.config.enabled {
        tracing::info!("surveillance disabled by config");
        return;
    }
    let bus_fill = bus.clone();
    let bus_trace = bus.clone();
    let surv_fill = surveillance.clone();
    let surv_trace = surveillance.clone();

    tokio::spawn(async move {
        let mut rx = bus_fill.subscribe("fill.created");
        loop {
            match rx.recv().await {
                Ok(Event::FillCreated(fill)) => {
                    surv_fill.on_fill(&fill);
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "surveillance: fill subscriber lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    tokio::spawn(async move {
        let mut rx = bus_trace.subscribe("order.trace");
        loop {
            match rx.recv().await {
                Ok(Event::OrderTrace(trace)) => {
                    surv_trace.on_trace(&trace);
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "surveillance: trace subscriber lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    tracing::info!("surveillance task started");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use types::{Fill, OrderTraceEvent, SettlementStatus};

    fn cfg() -> SurveillanceConfig {
        SurveillanceConfig {
            enabled: true,
            wash_window: Duration::from_secs(60),
            wash_overlap_pct: 90,
            rapid_cancel_window: Duration::from_secs(60),
            rapid_cancel_lifetime_ms: 500,
            rapid_cancel_min_count: 3,
            cancel_ratio_window: Duration::from_secs(60),
            cancel_ratio_threshold_pct: 90,
            cancel_ratio_min_events: 5,
            max_events_per_user: 1000,
        }
    }

    fn fill(user: &str, side: Side, amount: i64) -> Fill {
        Fill {
            id: format!("f-{}-{}-{}", user, amount, chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)),
            intent_id: format!("o-{user}-{amount}"),
            user_id: user.to_string(),
            market_id: "BTC-USD".to_string(),
            side,
            price: 100,
            amount,
            outcome: 0,
            timestamp: Utc::now(),
            op_id: "op-1".to_string(),
            fee: 0,
            fee_bps: 0,
            is_maker: false,
            aggressor_side: None,
            fill_index: 0,
            settlement_status: SettlementStatus::default(),
        }
    }

    fn counter() -> (Arc<AtomicUsize>, Arc<dyn Fn(&Alert) + Send + Sync>) {
        let c = Arc::new(AtomicUsize::new(0));
        let c2 = c.clone();
        let cb: Arc<dyn Fn(&Alert) + Send + Sync> = Arc::new(move |_a: &Alert| {
            c2.fetch_add(1, Ordering::Relaxed);
        });
        (c, cb)
    }

    #[test]
    fn round_trip_wash_fires_when_buy_and_sell_overlap() {
        let (count, cb) = counter();
        let surv = Surveillance::with_callback(cfg(), cb);
        surv.on_fill(&fill("alice", Side::Buy, 100));
        surv.on_fill(&fill("alice", Side::Sell, 95));
        assert!(count.load(Ordering::Relaxed) >= 1, "wash rule should fire");
    }

    #[test]
    fn round_trip_wash_does_not_fire_for_one_sided() {
        let (count, cb) = counter();
        let surv = Surveillance::with_callback(cfg(), cb);
        surv.on_fill(&fill("alice", Side::Buy, 100));
        surv.on_fill(&fill("alice", Side::Buy, 200));
        assert_eq!(count.load(Ordering::Relaxed), 0, "one-sided is not wash");
    }

    #[test]
    fn round_trip_wash_does_not_fire_for_small_overlap() {
        let (count, cb) = counter();
        let surv = Surveillance::with_callback(cfg(), cb);
        surv.on_fill(&fill("alice", Side::Buy, 100));
        surv.on_fill(&fill("alice", Side::Sell, 10));
        assert_eq!(count.load(Ordering::Relaxed), 0, "10% overlap is below threshold");
    }

    #[test]
    fn rapid_cancel_fires_when_threshold_crossed() {
        let (count, cb) = counter();
        let surv = Surveillance::with_callback(cfg(), cb);
        for i in 0..5 {
            let now = Utc::now();
            let id = format!("ord-{i}");
            let submit = OrderTraceEvent {
                schema_version: 1,
                event_id: format!("e-sub-{i}"),
                recorded_at: now,
                order_id: Some(id.clone()),
                client_order_id: None,
                user_id: Some("bob".into()),
                session_id: None,
                request_id: None,
                command_seq: None,
                market_id: None,
                outcome: None,
                stage: OrderTraceStage::MatchingResting,
                lifecycle: None,
                side: None,
                price: None,
                amount: Some(1000),
                remaining_amount: None,
                filled_amount: None,
                fee: None,
                detail: serde_json::Value::Null,
                reject_code: None,
                reject_message: None,
                elapsed_us_since_request: None,
                trace_id: None,
            };
            surv.on_trace(&submit);
            let cancel = OrderTraceEvent {
                stage: OrderTraceStage::MatchingCancelled,
                recorded_at: now + chrono::Duration::milliseconds(100),
                ..submit
            };
            surv.on_trace(&cancel);
        }
        assert!(
            count.load(Ordering::Relaxed) >= 1,
            "rapid_cancel should fire on 5 sub-then-cancel-within-100ms pairs"
        );
    }

    #[test]
    fn rapid_cancel_does_not_fire_when_cancel_is_slow() {
        let (count, cb) = counter();
        let surv = Surveillance::with_callback(cfg(), cb);
        for i in 0..5 {
            let now = Utc::now();
            let id = format!("ord-{i}");
            let submit = OrderTraceEvent {
                schema_version: 1,
                event_id: format!("e-{i}"),
                recorded_at: now,
                order_id: Some(id.clone()),
                client_order_id: None,
                user_id: Some("bob".into()),
                session_id: None,
                request_id: None,
                command_seq: None,
                market_id: None,
                outcome: None,
                stage: OrderTraceStage::MatchingResting,
                lifecycle: None,
                side: None,
                price: None,
                amount: Some(1000),
                remaining_amount: None,
                filled_amount: None,
                fee: None,
                detail: serde_json::Value::Null,
                reject_code: None,
                reject_message: None,
                elapsed_us_since_request: None,
                trace_id: None,
            };
            surv.on_trace(&submit);
            let cancel = OrderTraceEvent {
                stage: OrderTraceStage::MatchingCancelled,
                recorded_at: now + chrono::Duration::milliseconds(5000),
                ..submit
            };
            surv.on_trace(&cancel);
        }
        assert_eq!(
            count.load(Ordering::Relaxed),
            0,
            "5s lifetime is way above 500ms threshold — should not fire"
        );
    }

    #[test]
    fn disabled_config_does_not_fire() {
        let (count, cb) = counter();
        let mut c = cfg();
        c.enabled = false;
        let surv = Surveillance::with_callback(c, cb);
        surv.on_fill(&fill("alice", Side::Buy, 100));
        surv.on_fill(&fill("alice", Side::Sell, 100));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }
}
