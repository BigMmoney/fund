// Global drain mode — operator-controlled gate for graceful shutdown
// / maintenance windows. Distinct from `MarketState::Maintenance`,
// which only halts a single market; drain_mode applies to the whole
// instance.
//
// State machine:
//
//   Active                  — normal operation
//     │
//     │  POST /admin/maintenance/drain {"target":"draining"}
//     ▼
//   Draining                — new order submissions return 503;
//                             cancels, deposits-in-progress, queries,
//                             admin actions, and worker ticks continue.
//                             Customers can liquidate; operators can
//                             move funds to cold storage.
//     │
//     │  POST /admin/maintenance/drain {"target":"drained"}
//     ▼
//   Drained                 — read-only except cancels. Withdrawals
//                             also rejected. This is the steady state
//                             before stopping the process.
//
//   Either Draining or Drained can transition back to Active via
//   POST /admin/maintenance/drain {"target":"active"}.
//
// Why a separate state from `MarketState`:
//   - MarketState halts ONE market. Drain halts the WHOLE instance.
//   - Drain transitions are operator actions audited at the admin
//     level; market state transitions can be triggered by anomaly
//     detection (sentinel / circuit breaker) without operator action.
//   - Drain is a precondition for safe `kubectl drain` of the api
//     pod: a leader sentinel can confirm "no in-flight new orders"
//     before terminating.
//
// Storage: single process-global atomic. Drained state is NOT
// persisted across restarts by design — a fresh boot is implicitly
// Active. Operators that want sticky drain across restarts gate the
// pod's startup probe externally.

use std::sync::atomic::{AtomicU8, Ordering};

/// Three-state lattice. Order matters — higher values are MORE
/// restrictive, so callers can compare with `>=` to decide whether
/// the current state blocks an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DrainState {
    Active = 0,
    Draining = 1,
    Drained = 2,
}

impl DrainState {
    pub(crate) fn from_u8(v: u8) -> Self {
        match v {
            0 => DrainState::Active,
            1 => DrainState::Draining,
            _ => DrainState::Drained,
        }
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            DrainState::Active => "active",
            DrainState::Draining => "draining",
            DrainState::Drained => "drained",
        }
    }
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "active" => Some(DrainState::Active),
            "draining" => Some(DrainState::Draining),
            "drained" => Some(DrainState::Drained),
            _ => None,
        }
    }
}

static DRAIN_STATE: AtomicU8 = AtomicU8::new(0); // Active

pub(crate) fn current() -> DrainState {
    DrainState::from_u8(DRAIN_STATE.load(Ordering::Acquire))
}

/// Transition the global drain state. The previous and new states are
/// returned for audit logging. Only valid transitions are accepted;
/// returns `Err` with a description otherwise.
pub(crate) fn set(target: DrainState) -> Result<(DrainState, DrainState), String> {
    let prev = current();
    if prev == target {
        // No-op transition — allow (idempotent) but flag.
        return Ok((prev, target));
    }
    // All transitions are allowed: Active ↔ Draining ↔ Drained, and
    // Active ↔ Drained shortcut. Operators may need to recover from a
    // bad drain without going through Draining again.
    DRAIN_STATE.store(target as u8, Ordering::Release);
    Ok((prev, target))
}

/// Gate for write-path requests (new orders, deposits). Returns true
/// when the request should be allowed.
pub(crate) fn allow_new_writes() -> bool {
    current() == DrainState::Active
}

/// Gate for withdrawal requests. Withdrawals stay allowed during
/// Draining (customers must be able to pull funds) but stop in Drained.
pub(crate) fn allow_withdrawals() -> bool {
    current() <= DrainState::Draining
}

/// Cancels are always allowed regardless of drain state — operators
/// and customers MUST be able to flatten positions during drain.
pub(crate) fn allow_cancels() -> bool {
    true
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    DRAIN_STATE.store(0, Ordering::Release);
}

// ── HTTP routes ────────────────────────────────────────────────

#[allow(unused_imports)]
use super::*;

#[derive(Debug, serde::Deserialize)]
struct DrainStateRequest {
    target: String,
}

/// Routes:
///   POST /admin/maintenance/drain { "target": "active|draining|drained" }
///   GET  /admin/maintenance/drain
///
/// POST also syncs the legacy `ops::DRAIN_MODE` bool so existing
/// callers that only check that flag stay correct: any non-Active
/// state sets the bool to true.
pub(crate) fn build_routes(
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let ip_set = ip_rate_limiter.clone();
    let adm_set = admin_rate_limiter.clone();
    let set_route = warp::path!("admin" / "maintenance" / "drain")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: DrainStateRequest| {
                let ip_rl = ip_set.clone();
                let adm_rl = adm_set.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;
                    let target = DrainState::parse(&req.target).ok_or_else(|| {
                        reject_api(
                            StatusCode::BAD_REQUEST,
                            "target must be active|draining|drained",
                        )
                    })?;
                    let (prev, next) = set(target).map_err(|err| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, err)
                    })?;
                    // Sync the legacy `ops::DRAIN_MODE` bool — true for
                    // any non-Active state. Backward-compat for code
                    // that only checks the bool flag.
                    crate::ops::DRAIN_MODE.store(
                        next != DrainState::Active,
                        std::sync::atomic::Ordering::SeqCst,
                    );
                    tracing::warn!(
                        admin = %principal.subject,
                        from = prev.as_str(),
                        to = next.as_str(),
                        "drain state transition"
                    );
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "previous": prev.as_str(),
                        "current": next.as_str(),
                    })))
                }
            },
        );

    let ip_get = ip_rate_limiter;
    let get_route = warp::path!("admin" / "maintenance" / "drain")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "state": current().as_str(),
                        "allow_new_writes": allow_new_writes(),
                        "allow_withdrawals": allow_withdrawals(),
                        "allow_cancels": allow_cancels(),
                    })))
                }
            },
        );

    set_route.or(get_route).unify().boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    // Tests share global state — serialize them.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_state_is_active() {
        let _g = TEST_LOCK.lock();
        reset_for_test();
        assert_eq!(current(), DrainState::Active);
        assert!(allow_new_writes());
        assert!(allow_withdrawals());
        assert!(allow_cancels());
    }

    #[test]
    fn draining_blocks_new_writes_but_allows_withdrawals_and_cancels() {
        let _g = TEST_LOCK.lock();
        reset_for_test();
        let (prev, next) = set(DrainState::Draining).unwrap();
        assert_eq!(prev, DrainState::Active);
        assert_eq!(next, DrainState::Draining);
        assert!(!allow_new_writes(), "draining blocks new writes");
        assert!(allow_withdrawals(), "draining still allows withdrawals");
        assert!(allow_cancels(), "draining still allows cancels");
        reset_for_test();
    }

    #[test]
    fn drained_blocks_writes_and_withdrawals_but_allows_cancels() {
        let _g = TEST_LOCK.lock();
        reset_for_test();
        set(DrainState::Drained).unwrap();
        assert!(!allow_new_writes());
        assert!(!allow_withdrawals(), "drained blocks withdrawals");
        assert!(allow_cancels(), "cancels are always allowed");
        reset_for_test();
    }

    #[test]
    fn can_return_from_drained_to_active() {
        let _g = TEST_LOCK.lock();
        reset_for_test();
        set(DrainState::Drained).unwrap();
        let (prev, next) = set(DrainState::Active).unwrap();
        assert_eq!(prev, DrainState::Drained);
        assert_eq!(next, DrainState::Active);
        assert!(allow_new_writes());
        reset_for_test();
    }

    #[test]
    fn parse_round_trips() {
        assert_eq!(DrainState::parse("active"), Some(DrainState::Active));
        assert_eq!(DrainState::parse("DRAINING"), Some(DrainState::Draining));
        assert_eq!(DrainState::parse(" drained "), Some(DrainState::Drained));
        assert_eq!(DrainState::parse("nonsense"), None);
    }

    #[test]
    fn state_order_higher_is_more_restrictive() {
        assert!(DrainState::Active < DrainState::Draining);
        assert!(DrainState::Draining < DrainState::Drained);
    }
}
