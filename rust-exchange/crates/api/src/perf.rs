#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Performance Profiling & SLA Monitoring
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Adds:
//  • Throughput rate computation (orders/sec, fills/sec)
//  • SLA breach counters (p99 > threshold)
//  • Hot-path timing budget tracking
//  • `/admin/perf/profile` endpoint with detailed breakdown
//  • `/admin/perf/sla` endpoint for SLA compliance report
//  • Configurable SLA thresholds

use super::*;

// ── SLA configuration ────────────────────────────────────────

/// SLA thresholds for latency monitoring (microseconds).
#[derive(Debug, Clone)]
pub(crate) struct SlaThresholds {
    /// Max acceptable p99 for end-to-end match (μs).
    pub(crate) match_p99_us: u64,
    /// Max acceptable p99 for WAL append (μs).
    pub(crate) wal_p99_us: u64,
    /// Max acceptable p99 for HTTP request (μs).
    pub(crate) http_p99_us: u64,
    /// Max acceptable p99 for queue wait (μs).
    pub(crate) queue_wait_p99_us: u64,
}

impl Default for SlaThresholds {
    fn default() -> Self {
        Self {
            match_p99_us: 1_000,    // 1ms
            wal_p99_us: 5_000,      // 5ms
            http_p99_us: 10_000,    // 10ms
            queue_wait_p99_us: 500, // 500μs
        }
    }
}

/// SLA compliance result for a single latency dimension.
#[derive(Debug, serde::Serialize)]
pub(crate) struct SlaCheck {
    pub(crate) dimension: String,
    pub(crate) threshold_us: u64,
    pub(crate) actual_p99_us: u64,
    pub(crate) compliant: bool,
    pub(crate) margin_pct: f64,
}

// ── Throughput tracker ───────────────────────────────────────

/// Sliding-window throughput tracker using atomic ring buffer.
pub(crate) struct ThroughputTracker {
    /// Ring buffer of per-second counters.
    buckets: [AtomicU64; 60],
    /// Epoch second when bucket 0 started.
    epoch_sec: AtomicU64,
}

impl ThroughputTracker {
    pub(crate) const fn new() -> Self {
        Self {
            buckets: [const { AtomicU64::new(0) }; 60],
            epoch_sec: AtomicU64::new(0),
        }
    }

    /// Record one event now.
    pub(crate) fn record(&self) {
        let now_sec = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let epoch = self.epoch_sec.load(Ordering::Relaxed);
        if epoch == 0 {
            // First call — initialize epoch.
            let _ =
                self.epoch_sec
                    .compare_exchange(0, now_sec, Ordering::Relaxed, Ordering::Relaxed);
        }
        let epoch = self.epoch_sec.load(Ordering::Relaxed);
        let offset = now_sec.saturating_sub(epoch) as usize;
        let idx = offset % 60;
        self.buckets[idx].fetch_add(1, Ordering::Relaxed);
    }

    /// Compute average throughput over the last `window_secs` seconds.
    pub(crate) fn rate(&self, window_secs: u64) -> f64 {
        let now_sec = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let epoch = self.epoch_sec.load(Ordering::Relaxed);
        if epoch == 0 {
            return 0.0;
        }
        let window = window_secs.min(60) as usize;
        let current_offset = now_sec.saturating_sub(epoch) as usize;

        let mut total = 0u64;
        let mut counted = 0usize;
        for i in 1..=window {
            if current_offset >= i {
                let idx = (current_offset - i) % 60;
                total += self.buckets[idx].load(Ordering::Relaxed);
                counted += 1;
            }
        }
        if counted == 0 {
            0.0
        } else {
            total as f64 / counted as f64
        }
    }
}

// ── Global throughput trackers ────────────────────────────────

pub(crate) static ORDER_THROUGHPUT: ThroughputTracker = ThroughputTracker::new();
pub(crate) static FILL_THROUGHPUT: ThroughputTracker = ThroughputTracker::new();
pub(crate) static SLA_BREACHES: AtomicU64 = AtomicU64::new(0);

// ── Hot-path timing budget ───────────────────────────────────

/// Track whether an order submission exceeded its timing budget.
pub(crate) struct TimingBudget {
    start: Instant,
    budget_us: u64,
}

impl TimingBudget {
    pub(crate) fn new(budget_us: u64) -> Self {
        Self {
            start: Instant::now(),
            budget_us,
        }
    }

    /// Returns (elapsed_us, exceeded).
    pub(crate) fn check(&self) -> (u64, bool) {
        let elapsed = self.start.elapsed().as_micros() as u64;
        (elapsed, elapsed > self.budget_us)
    }
}

// ── SLA evaluation ───────────────────────────────────────────

pub(crate) fn evaluate_sla(thresholds: &SlaThresholds) -> Vec<SlaCheck> {
    let dims = [
        (
            "match_e2e",
            thresholds.match_p99_us,
            &observability::METRICS.match_latency,
        ),
        (
            "wal_append",
            thresholds.wal_p99_us,
            &observability::METRICS.wal_append_latency,
        ),
        (
            "http_request",
            thresholds.http_p99_us,
            &observability::METRICS.http_request_latency,
        ),
        (
            "queue_wait",
            thresholds.queue_wait_p99_us,
            &observability::METRICS.queue_wait_latency,
        ),
    ];

    dims.iter()
        .map(|(name, threshold, tracker)| {
            let p99 = tracker.percentile_pub(0.99);
            let compliant = p99 <= *threshold;
            let margin_pct = if *threshold > 0 {
                ((1.0 - p99 as f64 / *threshold as f64) * 100.0).clamp(-999.0, 999.0)
            } else {
                0.0
            };
            if !compliant {
                SLA_BREACHES.fetch_add(1, Ordering::Relaxed);
            }
            SlaCheck {
                dimension: name.to_string(),
                threshold_us: *threshold,
                actual_p99_us: p99,
                compliant,
                margin_pct,
            }
        })
        .collect()
}

// ── Admin routes ─────────────────────────────────────────────

pub(crate) fn build_perf_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let engine1 = partitioned_engine.clone();
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();

    // GET /admin/perf/profile — full performance profile snapshot
    let profile_route = warp::path!("admin" / "perf" / "profile")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
            let engine = engine1.clone();
            let ip_rl = ip1.clone();
            let adm_rl = adm1.clone();
            async move {
                require_admin(&principal)?;
                let ip_key = remote
                    .map(|v| v.ip().to_string())
                    .unwrap_or_else(|| format!("user:{}", principal.subject));
                ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                let backpressure = engine.backpressure_signal();
                let queue_depths = engine.queue_depths();

                let sla_checks = evaluate_sla(&SlaThresholds::default());
                let all_compliant = sla_checks.iter().all(|c| c.compliant);

                let profile = serde_json::json!({
                    "throughput": {
                        "orders_per_sec_1s": ORDER_THROUGHPUT.rate(1),
                        "orders_per_sec_10s": ORDER_THROUGHPUT.rate(10),
                        "orders_per_sec_60s": ORDER_THROUGHPUT.rate(60),
                        "fills_per_sec_1s": FILL_THROUGHPUT.rate(1),
                        "fills_per_sec_10s": FILL_THROUGHPUT.rate(10),
                        "fills_per_sec_60s": FILL_THROUGHPUT.rate(60),
                    },
                    "latency": observability::METRICS.snapshot()["latency"].clone(),
                    "sla": {
                        "compliant": all_compliant,
                        "breach_count": SLA_BREACHES.load(Ordering::Relaxed),
                        "checks": sla_checks,
                    },
                    "backpressure": format!("{:?}", backpressure),
                    "partitions": queue_depths.iter().map(|d| {
                        serde_json::json!({
                            "id": d.partition_id,
                            "inflight": d.inflight,
                            "capacity": d.capacity,
                            "utilization_pct": if d.capacity > 0 {
                                (d.inflight as f64 / d.capacity as f64 * 100.0).round() as u64
                            } else { 0 },
                        })
                    }).collect::<Vec<_>>(),
                    "counters": {
                        "orders_received": observability::METRICS.orders_received.load(Ordering::Relaxed),
                        "orders_filled": observability::METRICS.orders_filled.load(Ordering::Relaxed),
                        "orders_rejected": observability::METRICS.orders_rejected.load(Ordering::Relaxed),
                        "settlements": observability::METRICS.settlements_committed.load(Ordering::Relaxed),
                        "wal_appends": observability::METRICS.wal_appends.load(Ordering::Relaxed),
                        "wal_errors": observability::METRICS.wal_errors.load(Ordering::Relaxed),
                    },
                });
                Ok::<_, warp::Rejection>(warp::reply::json(&profile))
            }
        });

    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();

    // GET /admin/perf/sla — SLA compliance report
    let sla_route = warp::path!("admin" / "perf" / "sla")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip2.clone();
                let adm_rl = adm2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let checks = evaluate_sla(&SlaThresholds::default());
                    let all_ok = checks.iter().all(|c| c.compliant);
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "sla_compliant": all_ok,
                        "total_breaches": SLA_BREACHES.load(Ordering::Relaxed),
                        "checks": checks,
                    })))
                }
            },
        );

    profile_route.or(sla_route).unify().boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throughput_tracker_rate_returns_zero_on_empty() {
        let t = ThroughputTracker::new();
        assert_eq!(t.rate(10), 0.0);
    }

    #[test]
    fn sla_thresholds_default_reasonable() {
        let sla = SlaThresholds::default();
        assert!(sla.match_p99_us > 0);
        assert!(sla.wal_p99_us > 0);
        assert!(sla.http_p99_us > 0);
        assert!(sla.queue_wait_p99_us > 0);
    }

    #[test]
    fn timing_budget_check() {
        let budget = TimingBudget::new(1_000_000); // 1s
        let (elapsed, exceeded) = budget.check();
        assert!(!exceeded);
        assert!(elapsed < 1_000_000);
    }

    #[test]
    fn sla_evaluation_runs_without_panic() {
        let checks = evaluate_sla(&SlaThresholds::default());
        assert_eq!(checks.len(), 4);
        // Each check has valid structure regardless of global metric state.
        for check in &checks {
            assert!(!check.dimension.is_empty());
            assert!(check.threshold_us > 0);
        }
    }

    #[test]
    fn sla_breach_counter_increments() {
        let initial = SLA_BREACHES.load(Ordering::Relaxed);
        // Record samples that will exceed a tight threshold
        for _ in 0..100 {
            observability::METRICS.match_latency.record(50_000); // 50ms
        }
        let tight_sla = SlaThresholds {
            match_p99_us: 1, // 1μs — will breach
            wal_p99_us: u64::MAX,
            http_p99_us: u64::MAX,
            queue_wait_p99_us: u64::MAX,
        };
        let checks = evaluate_sla(&tight_sla);
        assert!(!checks[0].compliant);
        assert!(SLA_BREACHES.load(Ordering::Relaxed) > initial);
    }
}
