#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Data Plane / Control Plane Decoupling
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Formalizes the separation between:
//  • Data plane  — hot trading path (order submit, cancel, query)
//  • Control plane — admin operations (governance, drain, sentinel, instruments)
//  • Ops plane   — health, readiness, metrics, Prometheus scrape
//
// Each plane has:
//  • Independent circuit breaker (data plane can fail without killing ops)
//  • Separate request counters for monitoring
//  • Distinct rejection behavior under drain/overload
//
// Admin routes (`/admin/*`) already use separate rate limiters (ip + admin).
// This module adds the circuit-breaker abstraction and plane classification.

use super::*;

// ── Request plane classification ─────────────────────────────

/// Which plane a request belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum RequestPlane {
    /// Hot trading path — orders, cancels, market queries.
    Data,
    /// Admin / governance — config changes, kill switch, instruments.
    Control,
    /// Operations — health, readiness, metrics, Prometheus.
    Ops,
}

impl RequestPlane {
    /// Classify a request path into a plane.
    pub(crate) fn from_path(path: &str) -> Self {
        if path.starts_with("/admin/") {
            RequestPlane::Control
        } else if path.starts_with("/health")
            || path.starts_with("/ready")
            || path.starts_with("/metrics")
            || path.starts_with("/prometheus")
            || path.starts_with("/version")
        {
            RequestPlane::Ops
        } else {
            RequestPlane::Data
        }
    }
}

// ── Per-plane circuit breaker ────────────────────────────────

/// Simple circuit breaker for per-plane fault isolation.
///
/// States:
///  • Closed  — requests flow normally
///  • Open    — requests rejected (after consecutive failures > threshold)
///  • HalfOpen — allow one probe request; success → Closed, failure → Open
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub(crate) struct PlaneCircuitBreaker {
    state: Mutex<CircuitState>,
    consecutive_failures: AtomicU64,
    failure_threshold: u64,
    /// Total requests seen.
    total_requests: AtomicU64,
    /// Total rejections due to open circuit.
    total_rejections: AtomicU64,
    /// Last time the circuit was opened.
    last_opened: Mutex<Option<Instant>>,
    /// How long to keep circuit open before trying half-open.
    recovery_window: Duration,
}

impl PlaneCircuitBreaker {
    pub(crate) fn new(failure_threshold: u64, recovery_window: Duration) -> Self {
        Self {
            state: Mutex::new(CircuitState::Closed),
            consecutive_failures: AtomicU64::new(0),
            failure_threshold,
            total_requests: AtomicU64::new(0),
            total_rejections: AtomicU64::new(0),
            last_opened: Mutex::new(None),
            recovery_window,
        }
    }

    /// Check if a request should be allowed.
    pub(crate) fn allow_request(&self) -> Result<(), String> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        let mut state = self.state.lock();
        match *state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                // Check if recovery window has elapsed.
                let opened = self.last_opened.lock();
                if let Some(t) = *opened {
                    if t.elapsed() >= self.recovery_window {
                        drop(opened);
                        *state = CircuitState::HalfOpen;
                        return Ok(());
                    }
                }
                self.total_rejections.fetch_add(1, Ordering::Relaxed);
                Err("circuit breaker open — plane temporarily unavailable".to_string())
            }
            CircuitState::HalfOpen => {
                // Allow one probe request in half-open state.
                Ok(())
            }
        }
    }

    /// Record a successful request — resets failure count, closes circuit.
    pub(crate) fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        let mut state = self.state.lock();
        if *state == CircuitState::HalfOpen {
            *state = CircuitState::Closed;
        }
    }

    /// Record a failed request.
    pub(crate) fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if failures >= self.failure_threshold {
            let mut state = self.state.lock();
            if *state == CircuitState::Closed || *state == CircuitState::HalfOpen {
                *state = CircuitState::Open;
                *self.last_opened.lock() = Some(Instant::now());
            }
        }
    }

    /// Get current state snapshot.
    pub(crate) fn snapshot(&self) -> serde_json::Value {
        let state = *self.state.lock();
        serde_json::json!({
            "state": state,
            "consecutive_failures": self.consecutive_failures.load(Ordering::Relaxed),
            "failure_threshold": self.failure_threshold,
            "total_requests": self.total_requests.load(Ordering::Relaxed),
            "total_rejections": self.total_rejections.load(Ordering::Relaxed),
        })
    }

    /// Manually reset the circuit breaker to closed state.
    pub(crate) fn reset(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        *self.state.lock() = CircuitState::Closed;
    }
}

// ── Per-plane counters ───────────────────────────────────────

/// Per-plane request / error counters.
pub(crate) struct PlaneMetrics {
    pub(crate) data_requests: AtomicU64,
    pub(crate) data_errors: AtomicU64,
    pub(crate) control_requests: AtomicU64,
    pub(crate) control_errors: AtomicU64,
    pub(crate) ops_requests: AtomicU64,
    pub(crate) ops_errors: AtomicU64,
}

impl PlaneMetrics {
    pub(crate) const fn new() -> Self {
        Self {
            data_requests: AtomicU64::new(0),
            data_errors: AtomicU64::new(0),
            control_requests: AtomicU64::new(0),
            control_errors: AtomicU64::new(0),
            ops_requests: AtomicU64::new(0),
            ops_errors: AtomicU64::new(0),
        }
    }

    pub(crate) fn record_request(&self, plane: RequestPlane) {
        match plane {
            RequestPlane::Data => self.data_requests.fetch_add(1, Ordering::Relaxed),
            RequestPlane::Control => self.control_requests.fetch_add(1, Ordering::Relaxed),
            RequestPlane::Ops => self.ops_requests.fetch_add(1, Ordering::Relaxed),
        };
    }

    pub(crate) fn record_error(&self, plane: RequestPlane) {
        match plane {
            RequestPlane::Data => self.data_errors.fetch_add(1, Ordering::Relaxed),
            RequestPlane::Control => self.control_errors.fetch_add(1, Ordering::Relaxed),
            RequestPlane::Ops => self.ops_errors.fetch_add(1, Ordering::Relaxed),
        };
    }

    pub(crate) fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "data": {
                "requests": self.data_requests.load(Ordering::Relaxed),
                "errors": self.data_errors.load(Ordering::Relaxed),
            },
            "control": {
                "requests": self.control_requests.load(Ordering::Relaxed),
                "errors": self.control_errors.load(Ordering::Relaxed),
            },
            "ops": {
                "requests": self.ops_requests.load(Ordering::Relaxed),
                "errors": self.ops_errors.load(Ordering::Relaxed),
            },
        })
    }
}

/// Global per-plane metrics singleton.
pub(crate) static PLANE_METRICS: PlaneMetrics = PlaneMetrics::new();

// ── Admin route for plane diagnostics ────────────────────────

pub(crate) fn build_plane_routes(
    data_breaker: Arc<PlaneCircuitBreaker>,
    control_breaker: Arc<PlaneCircuitBreaker>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let db1 = data_breaker.clone();
    let cb1 = control_breaker.clone();

    // GET /admin/planes — plane metrics + circuit breaker states
    let status_route = warp::path!("admin" / "planes")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                let db = db1.clone();
                let cb = cb1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "plane_metrics": PLANE_METRICS.snapshot(),
                        "circuit_breakers": {
                            "data_plane": db.snapshot(),
                            "control_plane": cb.snapshot(),
                        },
                    })))
                }
            },
        );

    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();
    let db2 = data_breaker.clone();
    let cb2 = control_breaker.clone();

    // POST /admin/planes/reset — reset circuit breakers
    let reset_route = warp::path!("admin" / "planes" / "reset")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: PlaneResetRequest| {
                let ip_rl = ip2.clone();
                let adm_rl = adm2.clone();
                let db = db2.clone();
                let cb = cb2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    match req.plane.as_str() {
                        "data" => db.reset(),
                        "control" => cb.reset(),
                        "all" => {
                            db.reset();
                            cb.reset();
                        }
                        _ => {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "invalid plane — use data|control|all",
                            ));
                        }
                    }

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "reset",
                        "plane": req.plane,
                    })))
                }
            },
        );

    status_route.or(reset_route).unify().boxed()
}

// ── DTOs ─────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct PlaneResetRequest {
    pub(crate) plane: String,
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_classification() {
        assert_eq!(
            RequestPlane::from_path("/admin/kill-switch"),
            RequestPlane::Control
        );
        assert_eq!(
            RequestPlane::from_path("/admin/sentinel/posture"),
            RequestPlane::Control
        );
        assert_eq!(RequestPlane::from_path("/health"), RequestPlane::Ops);
        assert_eq!(RequestPlane::from_path("/ready"), RequestPlane::Ops);
        assert_eq!(RequestPlane::from_path("/metrics"), RequestPlane::Ops);
        assert_eq!(RequestPlane::from_path("/prometheus"), RequestPlane::Ops);
        assert_eq!(RequestPlane::from_path("/version"), RequestPlane::Ops);
        assert_eq!(RequestPlane::from_path("/order/intent"), RequestPlane::Data);
        assert_eq!(
            RequestPlane::from_path("/market/BTC-USDT"),
            RequestPlane::Data
        );
        assert_eq!(RequestPlane::from_path("/ws/trades"), RequestPlane::Data);
    }

    #[test]
    fn circuit_breaker_closed_allows_requests() {
        let cb = PlaneCircuitBreaker::new(3, Duration::from_secs(5));
        assert!(cb.allow_request().is_ok());
    }

    #[test]
    fn circuit_breaker_opens_after_threshold() {
        let cb = PlaneCircuitBreaker::new(3, Duration::from_secs(60));
        // Record failures up to threshold.
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow_request().is_ok()); // Still closed.
        cb.record_failure(); // Hits threshold.
        assert!(cb.allow_request().is_err()); // Now open.
    }

    #[test]
    fn circuit_breaker_reset() {
        let cb = PlaneCircuitBreaker::new(2, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow_request().is_err());
        cb.reset();
        assert!(cb.allow_request().is_ok());
    }

    #[test]
    fn circuit_breaker_success_resets_failures() {
        let cb = PlaneCircuitBreaker::new(3, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        cb.record_success(); // Resets count.
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow_request().is_ok()); // Should still be closed (2 < 3).
    }

    #[test]
    fn plane_metrics_counts() {
        let pm = PlaneMetrics::new();
        pm.record_request(RequestPlane::Data);
        pm.record_request(RequestPlane::Data);
        pm.record_request(RequestPlane::Control);
        pm.record_error(RequestPlane::Data);

        assert_eq!(pm.data_requests.load(Ordering::Relaxed), 2);
        assert_eq!(pm.control_requests.load(Ordering::Relaxed), 1);
        assert_eq!(pm.data_errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn plane_metrics_snapshot_structure() {
        let snapshot = PLANE_METRICS.snapshot();
        assert!(snapshot["data"]["requests"].is_u64());
        assert!(snapshot["control"]["requests"].is_u64());
        assert!(snapshot["ops"]["requests"].is_u64());
    }

    #[test]
    fn circuit_breaker_snapshot_fields() {
        let cb = PlaneCircuitBreaker::new(5, Duration::from_secs(10));
        let snap = cb.snapshot();
        assert_eq!(snap["failure_threshold"], 5);
        assert!(snap["total_requests"].is_u64());
    }
}
