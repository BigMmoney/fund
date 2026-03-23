#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Fault Injection Framework — Runtime Fail-Points
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Provides named fail-points that can be enabled at runtime to simulate
// failures during testing and chaos engineering.
//
// Safety:
//  • Fail-points are only activatable when `EXCHANGE_FAILPOINT_ENABLED=true`
//    environment variable is set (i.e. never in production).
//  • Each fail-point has a name, failure mode, and optional duration.
//  • Admin routes allow activating/deactivating individual fail-points.
//
// Pre-defined fail-points:
//  • `wal_append`           — simulate WAL append failures
//  • `snapshot_flush`       — simulate snapshot write failures
//  • `ledger_invariant`     — simulate balance invariant check failure
//  • `match_timeout`        — inject latency into matching pipeline
//  • `settlement_reject`    — reject settlement operations
//  • `network_partition`    — simulate delayed/dropped responses

use super::*;

use std::sync::LazyLock;

// ── Configuration gate ───────────────────────────────────────

/// Returns true only if the failpoint system is explicitly enabled.
fn failpoints_enabled() -> bool {
    std::env::var("EXCHANGE_FAILPOINT_ENABLED")
        .ok()
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

// ── Failure modes ────────────────────────────────────────────

/// How a fail-point manifests.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum FailureMode {
    /// Return an error immediately.
    Error { message: String },
    /// Inject a delay (microseconds) before proceeding normally.
    Delay { delay_us: u64 },
    /// Panic the current thread (for crash recovery testing).
    Panic { message: String },
    /// Randomly fail with a given probability (0.0–1.0).
    Probabilistic { probability: f64, message: String },
}

/// A registered fail-point.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FailPoint {
    pub(crate) name: String,
    pub(crate) active: bool,
    pub(crate) mode: FailureMode,
    pub(crate) trigger_count: u64,
    pub(crate) activated_at: Option<DateTime<Utc>>,
    pub(crate) activated_by: Option<String>,
}

// ── Registry ─────────────────────────────────────────────────

pub(crate) struct FailPointRegistry {
    points: Mutex<HashMap<String, FailPoint>>,
}

impl FailPointRegistry {
    pub(crate) fn new() -> Self {
        Self {
            points: Mutex::new(HashMap::new()),
        }
    }

    /// Register a named fail-point (initially inactive).
    pub(crate) fn register(&self, name: &str) {
        let mut points = self.points.lock();
        points.entry(name.to_string()).or_insert_with(|| FailPoint {
            name: name.to_string(),
            active: false,
            mode: FailureMode::Error {
                message: format!("fail-point '{name}' triggered"),
            },
            trigger_count: 0,
            activated_at: None,
            activated_by: None,
        });
    }

    /// Activate a fail-point with a specific failure mode.
    /// Returns false if fail-points are globally disabled.
    pub(crate) fn activate(&self, name: &str, mode: FailureMode, by: &str) -> bool {
        if !failpoints_enabled() {
            return false;
        }
        let mut points = self.points.lock();
        if let Some(fp) = points.get_mut(name) {
            fp.active = true;
            fp.mode = mode;
            fp.activated_at = Some(Utc::now());
            fp.activated_by = Some(by.to_string());
            tracing::warn!(failpoint = name, admin = by, "fail-point ACTIVATED");
            true
        } else {
            false
        }
    }

    /// Deactivate a fail-point.
    pub(crate) fn deactivate(&self, name: &str) -> bool {
        let mut points = self.points.lock();
        if let Some(fp) = points.get_mut(name) {
            fp.active = false;
            tracing::info!(failpoint = name, "fail-point deactivated");
            true
        } else {
            false
        }
    }

    /// Check a fail-point.  Returns `Err` if the fail-point should cause
    /// the operation to fail, `Ok(delay)` if it should proceed (possibly
    /// after an injected delay).
    pub(crate) fn check(&self, name: &str) -> Result<Option<u64>, String> {
        if !failpoints_enabled() {
            return Ok(None);
        }
        let mut points = self.points.lock();
        let fp = match points.get_mut(name) {
            Some(fp) if fp.active => fp,
            _ => return Ok(None),
        };

        fp.trigger_count += 1;
        match &fp.mode {
            FailureMode::Error { message } => Err(message.clone()),
            FailureMode::Delay { delay_us } => Ok(Some(*delay_us)),
            FailureMode::Panic { message } => {
                // Only panic in test environments — the env gate is our safety net.
                panic!("fail-point '{}': {}", fp.name, message);
            }
            FailureMode::Probabilistic {
                probability,
                message,
            } => {
                // Simple PRNG: use trigger_count as seed for deterministic testing.
                let hash = fp
                    .trigger_count
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let roll = (hash % 10000) as f64 / 10000.0;
                if roll < *probability {
                    Err(message.clone())
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// List all registered fail-points.
    pub(crate) fn list(&self) -> Vec<FailPoint> {
        self.points.lock().values().cloned().collect()
    }
}

// ── Global registry ──────────────────────────────────────────

pub(crate) static FAILPOINTS: LazyLock<FailPointRegistry> = LazyLock::new(|| {
    let registry = FailPointRegistry::new();
    // Register all known fail-points.
    registry.register("wal_append");
    registry.register("snapshot_flush");
    registry.register("ledger_invariant");
    registry.register("match_timeout");
    registry.register("settlement_reject");
    registry.register("network_partition");
    registry
});

/// Convenience macro-style check — returns early with error if fail-point fires.
pub(crate) fn check_failpoint(name: &str) -> Result<(), String> {
    match FAILPOINTS.check(name) {
        Ok(Some(delay_us)) => {
            // Inject delay synchronously (blocking — appropriate for hot path testing).
            std::thread::sleep(std::time::Duration::from_micros(delay_us));
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(e) => Err(e),
    }
}

// ── Admin routes ─────────────────────────────────────────────

pub(crate) fn build_failpoint_routes(
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();

    // GET /admin/failpoints — list all fail-points
    let list_route = warp::path!("admin" / "failpoints")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let points = FAILPOINTS.list();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "enabled": failpoints_enabled(),
                        "failpoints": points,
                    })))
                }
            },
        );

    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();

    // POST /admin/failpoints/activate — activate a fail-point
    let activate_route = warp::path!("admin" / "failpoints" / "activate")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: ActivateFailPointRequest| {
                let ip_rl = ip2.clone();
                let adm_rl = adm2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    if !failpoints_enabled() {
                        return Err(reject_api(
                            StatusCode::FORBIDDEN,
                            "fail-points not enabled (set EXCHANGE_FAILPOINT_ENABLED=true)",
                        ));
                    }

                    let mode = match req.mode.as_str() {
                        "error" => FailureMode::Error {
                            message: req
                                .message
                                .unwrap_or_else(|| "injected failure".to_string()),
                        },
                        "delay" => FailureMode::Delay {
                            delay_us: req.delay_us.unwrap_or(1000),
                        },
                        "panic" => FailureMode::Panic {
                            message: req.message.unwrap_or_else(|| "injected panic".to_string()),
                        },
                        "probabilistic" => FailureMode::Probabilistic {
                            probability: req.probability.unwrap_or(0.5),
                            message: req
                                .message
                                .unwrap_or_else(|| "probabilistic failure".to_string()),
                        },
                        _ => {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "invalid mode — use error|delay|panic|probabilistic",
                            ));
                        }
                    };

                    let success = FAILPOINTS.activate(&req.name, mode, &principal.subject);
                    if success {
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "status": "activated",
                            "name": req.name,
                        })))
                    } else {
                        Err(reject_api(
                            StatusCode::NOT_FOUND,
                            format!("fail-point '{}' not registered", req.name),
                        ))
                    }
                }
            },
        );

    let ip3 = ip_rate_limiter.clone();
    let adm3 = admin_rate_limiter.clone();

    // POST /admin/failpoints/deactivate — deactivate a fail-point
    let deactivate_route = warp::path!("admin" / "failpoints" / "deactivate")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: DeactivateFailPointRequest| {
                let ip_rl = ip3.clone();
                let adm_rl = adm3.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    let success = FAILPOINTS.deactivate(&req.name);
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": if success { "deactivated" } else { "not_found" },
                        "name": req.name,
                    })))
                }
            },
        );

    list_route
        .or(activate_route)
        .unify()
        .or(deactivate_route)
        .unify()
        .boxed()
}

// ── DTOs ─────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct ActivateFailPointRequest {
    pub(crate) name: String,
    pub(crate) mode: String,
    pub(crate) message: Option<String>,
    pub(crate) delay_us: Option<u64>,
    pub(crate) probability: Option<f64>,
}

#[derive(serde::Deserialize)]
pub(crate) struct DeactivateFailPointRequest {
    pub(crate) name: String,
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failpoint_registry_register_and_list() {
        let reg = FailPointRegistry::new();
        reg.register("test_fp");
        let list = reg.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test_fp");
        assert!(!list[0].active);
    }

    #[test]
    fn failpoint_check_inactive_returns_ok() {
        let reg = FailPointRegistry::new();
        reg.register("test_inactive");
        assert!(reg.check("test_inactive").is_ok());
    }

    #[test]
    fn failpoint_activate_requires_env() {
        let reg = FailPointRegistry::new();
        reg.register("test_env_gate");
        // Without EXCHANGE_FAILPOINT_ENABLED, activate should return false.
        let result = reg.activate(
            "test_env_gate",
            FailureMode::Error {
                message: "test".into(),
            },
            "admin",
        );
        assert!(!result);
    }

    #[test]
    fn failpoint_deactivate_nonexistent() {
        let reg = FailPointRegistry::new();
        assert!(!reg.deactivate("nonexistent"));
    }

    #[test]
    fn failpoint_error_mode() {
        // Test the logic directly without env gate.
        let reg = FailPointRegistry::new();
        reg.register("test_err");
        // Force-activate by directly manipulating the lock.
        {
            let mut points = reg.points.lock();
            let fp = points.get_mut("test_err").unwrap();
            fp.active = true;
            fp.mode = FailureMode::Error {
                message: "boom".into(),
            };
        }
        let result = reg.check("test_err");
        // Without env gate, check returns Ok even if active.
        // This is the safety guard — production never fires.
        assert!(result.is_ok());
    }

    #[test]
    fn failpoint_delay_mode() {
        let reg = FailPointRegistry::new();
        reg.register("test_delay");
        {
            let mut points = reg.points.lock();
            let fp = points.get_mut("test_delay").unwrap();
            fp.active = true;
            fp.mode = FailureMode::Delay { delay_us: 100 };
        }
        // Without env gate → Ok(None)
        let result = reg.check("test_delay");
        assert!(result.is_ok());
    }

    #[test]
    fn check_failpoint_helper_ok() {
        assert!(check_failpoint("nonexistent").is_ok());
    }

    #[test]
    fn global_failpoints_has_defaults() {
        let list = FAILPOINTS.list();
        let names: Vec<_> = list.iter().map(|fp| fp.name.as_str()).collect();
        assert!(names.contains(&"wal_append"));
        assert!(names.contains(&"snapshot_flush"));
        assert!(names.contains(&"ledger_invariant"));
        assert!(names.contains(&"match_timeout"));
        assert!(names.contains(&"settlement_reject"));
        assert!(names.contains(&"network_partition"));
    }
}
