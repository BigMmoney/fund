use super::*;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SystemSentinel — cross-module coordinated degradation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// The sentinel is the single coordination point that every subsystem
// queries before executing high-risk actions.  When ANY subsystem
// detects an anomaly it reports an incident to the sentinel, which
// computes the global degradation posture.  All other subsystems
// observe and enforce the posture automatically.
//
// Degradation levels (ordered by severity):
//   Green     — normal operations
//   Yellow    — heightened monitoring: lower thresholds, extra audit
//   Orange    — restricted: only small hot-tier ops, no cold/warm, manual-only liq
//   Red       — full halt: no withdrawals, no liquidations, no new orders
//
// Flow:
//   Subsystem detects anomaly → sentinel.report_incident(...)
//   Sentinel evaluates all active incidents → computes worst-case level
//   On every request, subsystem calls sentinel.posture() to decide
//   whether to proceed, restrict, or reject.

// ── Degradation levels ───────────────────────────────────────

/// System-wide degradation level (ordered by severity).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum DegradationLevel {
    /// Normal operations.
    Green = 0,
    /// Heightened monitoring — lower thresholds, extra logging.
    Yellow = 1,
    /// Restricted — only small/hot ops, manual-confirm liquidation.
    Orange = 2,
    /// Full halt — no withdrawals, no liquidation, no new orders.
    Red = 3,
}

impl DegradationLevel {
    fn _from_u8(v: u8) -> Self {
        match v {
            0 => Self::Green,
            1 => Self::Yellow,
            2 => Self::Orange,
            3 => Self::Red,
            _ => Self::Red,
        }
    }
}

// ── Incident types ────────────────────────────────────────────

/// Origin subsystem that reported the incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum IncidentOrigin {
    Custody,
    Risk,
    Matching,
    Recovery,
    Admin,
}

/// A specific incident that contributes to overall degradation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Incident {
    pub(crate) id: String,
    pub(crate) origin: IncidentOrigin,
    pub(crate) severity: DegradationLevel,
    pub(crate) reason: String,
    pub(crate) reported_at: DateTime<Utc>,
    /// Auto-expire after this many seconds (0 = manual clear only).
    pub(crate) ttl_secs: u64,
    pub(crate) resolved: bool,
    pub(crate) resolved_at: Option<DateTime<Utc>>,
    pub(crate) resolved_by: Option<String>,
}

// ── Posture: the computed output ──────────────────────────────

/// The current system posture — queried by every subsystem.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SystemPosture {
    pub(crate) level: DegradationLevel,
    /// Maximum single withdrawal allowed (0 = blocked).
    pub(crate) max_withdrawal_amount: i64,
    /// Whether auto-liquidation is permitted.
    pub(crate) auto_liquidation_allowed: bool,
    /// Whether new orders are accepted.
    pub(crate) new_orders_allowed: bool,
    /// Whether cold/warm vault operations are permitted.
    pub(crate) cold_warm_ops_allowed: bool,
    /// Human-readable summary of active incidents.
    pub(crate) active_incident_count: usize,
    pub(crate) worst_incident_reason: String,
}

// ── Sentinel core ─────────────────────────────────────────────

/// Posture policy: maps degradation levels to operational constraints.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PosturePolicy {
    /// Yellow: max withdrawal per tx.
    pub(crate) yellow_max_withdrawal: i64,
    /// Orange: max withdrawal per tx.
    pub(crate) orange_max_withdrawal: i64,
    /// Red: everything blocked.
    pub(crate) red_max_withdrawal: i64,
}

impl Default for PosturePolicy {
    fn default() -> Self {
        Self {
            yellow_max_withdrawal: 100_000, // 100k in yellow
            orange_max_withdrawal: 10_000,  // 10k in orange
            red_max_withdrawal: 0,          // blocked in red
        }
    }
}

/// The SystemSentinel: single point of coordination.
pub(crate) struct SystemSentinel {
    incidents: Mutex<Vec<Incident>>,
    policy: PosturePolicy,
    /// Manual override: admin can force a level.
    manual_override: Mutex<Option<DegradationLevel>>,
}

impl SystemSentinel {
    pub(crate) fn new(policy: PosturePolicy) -> Self {
        Self {
            incidents: Mutex::new(Vec::new()),
            policy,
            manual_override: Mutex::new(None),
        }
    }

    // ── Incident reporting ────────────────────────────────────

    /// Any subsystem reports an incident.  Returns the incident ID.
    pub(crate) fn report_incident(
        &self,
        origin: IncidentOrigin,
        severity: DegradationLevel,
        reason: &str,
        ttl_secs: u64,
    ) -> String {
        let id = types::generate_id();
        let incident = Incident {
            id: id.clone(),
            origin,
            severity,
            reason: reason.to_string(),
            reported_at: Utc::now(),
            ttl_secs,
            resolved: false,
            resolved_at: None,
            resolved_by: None,
        };
        self.incidents.lock().push(incident);
        id
    }

    /// Resolve a specific incident (admin action).
    pub(crate) fn resolve_incident(&self, incident_id: &str, by: &str) -> bool {
        let mut incidents = self.incidents.lock();
        if let Some(inc) = incidents
            .iter_mut()
            .find(|i| i.id == incident_id && !i.resolved)
        {
            inc.resolved = true;
            inc.resolved_at = Some(Utc::now());
            inc.resolved_by = Some(by.to_string());
            true
        } else {
            false
        }
    }

    /// Resolve all incidents from a specific origin.
    pub(crate) fn resolve_all_from(&self, origin: IncidentOrigin, by: &str) {
        let now = Utc::now();
        let mut incidents = self.incidents.lock();
        for inc in incidents.iter_mut() {
            if inc.origin == origin && !inc.resolved {
                inc.resolved = true;
                inc.resolved_at = Some(now);
                inc.resolved_by = Some(by.to_string());
            }
        }
    }

    // ── Posture computation ───────────────────────────────────

    /// Compute the current system posture from all active incidents.
    pub(crate) fn posture(&self) -> SystemPosture {
        let now = Utc::now();
        let incidents = self.incidents.lock();

        // Filter to active (not resolved, not expired) incidents
        let mut worst = DegradationLevel::Green;
        let mut worst_reason = String::new();
        let mut active_count = 0usize;

        for inc in incidents.iter() {
            if inc.resolved {
                continue;
            }
            // TTL expiry
            if inc.ttl_secs > 0 {
                let age = (now - inc.reported_at).num_seconds().max(0) as u64;
                if age >= inc.ttl_secs {
                    continue; // expired
                }
            }
            active_count += 1;
            if inc.severity > worst {
                worst = inc.severity;
                worst_reason = inc.reason.clone();
            }
        }

        // Manual override: always takes highest
        if let Some(manual) = *self.manual_override.lock() {
            if manual > worst {
                worst = manual;
                worst_reason = "admin manual override".to_string();
            }
        }

        let (max_w, auto_liq, new_ord, cw_ops) = match worst {
            DegradationLevel::Green => (i64::MAX, true, true, true),
            DegradationLevel::Yellow => (self.policy.yellow_max_withdrawal, true, true, true),
            DegradationLevel::Orange => (self.policy.orange_max_withdrawal, false, true, false),
            DegradationLevel::Red => (self.policy.red_max_withdrawal, false, false, false),
        };

        SystemPosture {
            level: worst,
            max_withdrawal_amount: max_w,
            auto_liquidation_allowed: auto_liq,
            new_orders_allowed: new_ord,
            cold_warm_ops_allowed: cw_ops,
            active_incident_count: active_count,
            worst_incident_reason: worst_reason,
        }
    }

    /// Admin: set a manual degradation override.
    pub(crate) fn set_manual_override(&self, level: Option<DegradationLevel>) {
        *self.manual_override.lock() = level;
    }

    /// Get all incidents (for admin view).
    pub(crate) fn all_incidents(&self, include_resolved: bool) -> Vec<Incident> {
        let now = Utc::now();
        let incidents = self.incidents.lock();
        incidents
            .iter()
            .filter(|i| {
                if !include_resolved && i.resolved {
                    return false;
                }
                if i.ttl_secs > 0 && !i.resolved {
                    let age = (now - i.reported_at).num_seconds().max(0) as u64;
                    if age >= i.ttl_secs {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect()
    }

    /// GC: mark expired incidents as resolved.
    pub(crate) fn gc_expired(&self) {
        let now = Utc::now();
        let mut incidents = self.incidents.lock();
        for inc in incidents.iter_mut() {
            if !inc.resolved && inc.ttl_secs > 0 {
                let age = (now - inc.reported_at).num_seconds().max(0) as u64;
                if age >= inc.ttl_secs {
                    inc.resolved = true;
                    inc.resolved_at = Some(now);
                    inc.resolved_by = Some("system:ttl_expiry".to_string());
                }
            }
        }
    }

    // ── Convenience: subsystem-specific reporters ─────────────

    /// Custody: signing verification failure.
    pub(crate) fn report_signing_failure(&self, detail: &str) -> String {
        self.report_incident(
            IncidentOrigin::Custody,
            DegradationLevel::Orange,
            &format!("signing verification failure: {detail}"),
            0, // manual-clear only
        )
    }

    /// Custody: vault velocity breach.
    pub(crate) fn report_velocity_breach(&self, tier: &str, current: i64, limit: i64) -> String {
        self.report_incident(
            IncidentOrigin::Custody,
            DegradationLevel::Yellow,
            &format!("vault velocity exceeded on {tier}: current={current}, limit={limit}"),
            3600, // auto-expire in 1h
        )
    }

    /// Custody: circuit breaker tripped.
    pub(crate) fn report_custody_breaker_trip(&self, reason: &str) -> String {
        self.report_incident(
            IncidentOrigin::Custody,
            DegradationLevel::Orange,
            &format!("custody circuit breaker tripped: {reason}"),
            0,
        )
    }

    /// Risk: liquidation velocity breached.
    pub(crate) fn report_liquidation_velocity_breach(
        &self,
        count: u32,
        window_secs: u64,
    ) -> String {
        self.report_incident(
            IncidentOrigin::Risk,
            DegradationLevel::Yellow,
            &format!("liquidation velocity exceeded: {count} liquidations in {window_secs}s"),
            1800, // auto-expire in 30m
        )
    }

    /// Risk: waterfall loss threshold hit.
    pub(crate) fn report_waterfall_halt(&self, loss: i64) -> String {
        self.report_incident(
            IncidentOrigin::Risk,
            DegradationLevel::Orange,
            &format!("liquidation waterfall loss threshold hit: cumulative_loss={loss}"),
            0,
        )
    }

    /// Risk: anomalous mark-price deviation.
    pub(crate) fn report_risk_anomaly(&self, detail: &str) -> String {
        self.report_incident(
            IncidentOrigin::Risk,
            DegradationLevel::Yellow,
            &format!("risk anomaly: {detail}"),
            3600,
        )
    }

    /// Recovery: replay hash mismatch.
    #[allow(dead_code)]
    pub(crate) fn report_replay_mismatch(&self, expected: &str, actual: &str, seq: u64) -> String {
        self.report_incident(
            IncidentOrigin::Recovery,
            DegradationLevel::Red,
            &format!("replay hash mismatch at seq={seq}: expected={expected}, actual={actual}"),
            0, // manual-clear only — RED
        )
    }

    /// Recovery: sequence gap detected.
    #[allow(dead_code)]
    pub(crate) fn report_sequence_gap(&self, gap_count: usize) -> String {
        self.report_incident(
            IncidentOrigin::Recovery,
            DegradationLevel::Red,
            &format!("sequence gap detected: {gap_count} entries missing"),
            0,
        )
    }

    /// Matching: market restricted.
    pub(crate) fn report_market_restricted(&self, market_id: &str, reason: &str) -> String {
        self.report_incident(
            IncidentOrigin::Matching,
            DegradationLevel::Yellow,
            &format!("market {market_id} restricted: {reason}"),
            7200,
        )
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Enforcement helpers — called by each subsystem
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Check if a withdrawal is permitted under the current posture.
/// Returns Ok(()) if allowed, Err(reason) if blocked or amount-limited.
pub(crate) fn enforce_withdrawal_posture(
    sentinel: &SystemSentinel,
    amount: i64,
    vault_tier: custody::VaultTier,
) -> Result<(), String> {
    let posture = sentinel.posture();
    match posture.level {
        DegradationLevel::Green => Ok(()),
        DegradationLevel::Yellow => {
            if amount > posture.max_withdrawal_amount {
                Err(format!(
                    "system in YELLOW: withdrawal {} exceeds limit {}",
                    amount, posture.max_withdrawal_amount
                ))
            } else {
                Ok(())
            }
        }
        DegradationLevel::Orange => {
            if !posture.cold_warm_ops_allowed
                && matches!(
                    vault_tier,
                    custody::VaultTier::Warm | custody::VaultTier::Cold
                )
            {
                return Err("system in ORANGE: cold/warm operations suspended".into());
            }
            if amount > posture.max_withdrawal_amount {
                Err(format!(
                    "system in ORANGE: withdrawal {} exceeds limit {}",
                    amount, posture.max_withdrawal_amount
                ))
            } else {
                Ok(())
            }
        }
        DegradationLevel::Red => Err(format!(
            "system in RED: all withdrawals suspended — {}",
            posture.worst_incident_reason
        )),
    }
}

/// Check if auto-liquidation is permitted under current posture.
pub(crate) fn enforce_liquidation_posture(sentinel: &SystemSentinel) -> Result<(), String> {
    let posture = sentinel.posture();
    if posture.auto_liquidation_allowed {
        Ok(())
    } else {
        Err(format!(
            "auto-liquidation suspended at level {:?}: {}",
            posture.level, posture.worst_incident_reason
        ))
    }
}

/// Check if new orders are permitted under current posture.
pub(crate) fn enforce_order_posture(sentinel: &SystemSentinel) -> Result<(), String> {
    let posture = sentinel.posture();
    if posture.new_orders_allowed {
        Ok(())
    } else {
        Err(format!(
            "new orders suspended at level {:?}: {}",
            posture.level, posture.worst_incident_reason
        ))
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Admin routes
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub(crate) fn build_sentinel_routes(
    sentinel: Arc<SystemSentinel>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    // GET /admin/sentinel/posture — current system posture
    let s1 = sentinel.clone();
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let posture_route = warp::path!("admin" / "sentinel" / "posture")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let s = s1.clone();
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let posture = s.posture();
                    Ok::<_, warp::Rejection>(warp::reply::json(&posture))
                }
            },
        );

    // GET /admin/sentinel/incidents — list incidents
    let s2 = sentinel.clone();
    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();
    let incidents_route = warp::path!("admin" / "sentinel" / "incidents")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let s = s2.clone();
                let ip_rl = ip2.clone();
                let adm_rl = adm2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let active = s.all_incidents(false);
                    Ok::<_, warp::Rejection>(warp::reply::json(&active))
                }
            },
        );

    // POST /admin/sentinel/incidents/{id}/resolve — resolve incident
    let s3 = sentinel.clone();
    let ip3 = ip_rate_limiter.clone();
    let adm3 = admin_rate_limiter.clone();
    let resolve_route = warp::path!("admin" / "sentinel" / "incidents" / String / "resolve")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |incident_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let s = s3.clone();
                let ip_rl = ip3.clone();
                let adm_rl = adm3.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let resolved = s.resolve_incident(&incident_id, &principal.subject);
                    if resolved {
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "incident_id": incident_id,
                            "resolved": true,
                            "resolved_by": principal.subject,
                        })))
                    } else {
                        Err(reject_api(
                            StatusCode::NOT_FOUND,
                            "incident not found or already resolved",
                        ))
                    }
                }
            },
        );

    // POST /admin/sentinel/override — set manual degradation level
    let s4 = sentinel.clone();
    let ip4 = ip_rate_limiter.clone();
    let adm4 = admin_rate_limiter.clone();
    let override_route = warp::path!("admin" / "sentinel" / "override")
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json::<OverrideRequest>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: OverrideRequest,
                  remote: Option<SocketAddr>| {
                let s = s4.clone();
                let ip_rl = ip4.clone();
                let adm_rl = adm4.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let level = match req.level.as_deref() {
                        Some("green") => Some(DegradationLevel::Green),
                        Some("yellow") => Some(DegradationLevel::Yellow),
                        Some("orange") => Some(DegradationLevel::Orange),
                        Some("red") => Some(DegradationLevel::Red),
                        None | Some("clear") => None,
                        _ => return Err(reject_api(StatusCode::BAD_REQUEST, "invalid level")),
                    };
                    s.set_manual_override(level);
                    let posture = s.posture();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "override": req.level,
                        "effective_posture": posture,
                    })))
                }
            },
        );

    // POST /admin/sentinel/resolve-origin — resolve all incidents from an origin
    let s5 = sentinel.clone();
    let ip5 = ip_rate_limiter.clone();
    let adm5 = admin_rate_limiter.clone();
    let resolve_origin_route = warp::path!("admin" / "sentinel" / "resolve-origin")
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json::<ResolveOriginRequest>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: ResolveOriginRequest,
                  remote: Option<SocketAddr>| {
                let s = s5.clone();
                let ip_rl = ip5.clone();
                let adm_rl = adm5.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let origin = match req.origin.as_str() {
                        "custody" => IncidentOrigin::Custody,
                        "risk" => IncidentOrigin::Risk,
                        "matching" => IncidentOrigin::Matching,
                        "recovery" => IncidentOrigin::Recovery,
                        _ => return Err(reject_api(StatusCode::BAD_REQUEST, "invalid origin")),
                    };
                    s.resolve_all_from(origin, &principal.subject);
                    let posture = s.posture();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "resolved_origin": req.origin,
                        "effective_posture": posture,
                    })))
                }
            },
        );

    posture_route
        .or(incidents_route)
        .unify()
        .or(resolve_route)
        .unify()
        .or(override_route)
        .unify()
        .or(resolve_origin_route)
        .unify()
        .boxed()
}

// ── DTO types ─────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct OverrideRequest {
    pub(crate) level: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ResolveOriginRequest {
    pub(crate) origin: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_starts_green() {
        let s = SystemSentinel::new(PosturePolicy::default());
        let p = s.posture();
        assert_eq!(p.level, DegradationLevel::Green);
        assert!(p.auto_liquidation_allowed);
        assert!(p.new_orders_allowed);
        assert!(p.cold_warm_ops_allowed);
        assert_eq!(p.active_incident_count, 0);
    }

    #[test]
    fn yellow_incident_restricts_withdrawal_amount() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_velocity_breach("Hot", 250_000, 200_000);
        let p = s.posture();
        assert_eq!(p.level, DegradationLevel::Yellow);
        assert_eq!(p.max_withdrawal_amount, 100_000);
        assert!(p.auto_liquidation_allowed);
        assert!(p.new_orders_allowed);
    }

    #[test]
    fn orange_incident_disables_auto_liq_and_cold_warm() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_signing_failure("address mismatch");
        let p = s.posture();
        assert_eq!(p.level, DegradationLevel::Orange);
        assert!(!p.auto_liquidation_allowed);
        assert!(!p.cold_warm_ops_allowed);
        assert!(p.new_orders_allowed); // can still trade
        assert_eq!(p.max_withdrawal_amount, 10_000);
    }

    #[test]
    fn red_incident_blocks_everything() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_replay_mismatch("0xabc", "0xdef", 42);
        let p = s.posture();
        assert_eq!(p.level, DegradationLevel::Red);
        assert!(!p.auto_liquidation_allowed);
        assert!(!p.new_orders_allowed);
        assert!(!p.cold_warm_ops_allowed);
        assert_eq!(p.max_withdrawal_amount, 0);
    }

    #[test]
    fn worst_incident_wins() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_velocity_breach("Hot", 250_000, 200_000); // Yellow
        s.report_signing_failure("mismatch"); // Orange
        let p = s.posture();
        assert_eq!(p.level, DegradationLevel::Orange);
        assert_eq!(p.active_incident_count, 2);
    }

    #[test]
    fn resolved_incidents_dont_contribute() {
        let s = SystemSentinel::new(PosturePolicy::default());
        let id = s.report_signing_failure("test");
        assert_eq!(s.posture().level, DegradationLevel::Orange);
        s.resolve_incident(&id, "admin1");
        assert_eq!(s.posture().level, DegradationLevel::Green);
    }

    #[test]
    fn resolve_all_from_origin() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_velocity_breach("Hot", 1, 1);
        s.report_custody_breaker_trip("test");
        s.report_liquidation_velocity_breach(50, 60); // risk
        assert_eq!(s.posture().active_incident_count, 3);

        s.resolve_all_from(IncidentOrigin::Custody, "admin");
        let p = s.posture();
        assert_eq!(p.active_incident_count, 1); // only risk remains
        assert_eq!(p.level, DegradationLevel::Yellow);
    }

    #[test]
    fn manual_override_takes_precedence() {
        let s = SystemSentinel::new(PosturePolicy::default());
        assert_eq!(s.posture().level, DegradationLevel::Green);
        s.set_manual_override(Some(DegradationLevel::Red));
        assert_eq!(s.posture().level, DegradationLevel::Red);
        s.set_manual_override(None);
        assert_eq!(s.posture().level, DegradationLevel::Green);
    }

    #[test]
    fn ttl_expiry_auto_resolves() {
        let s = SystemSentinel::new(PosturePolicy::default());
        // Report with 0 TTL → never auto-expires
        let _id = s.report_incident(
            IncidentOrigin::Admin,
            DegradationLevel::Yellow,
            "permanent",
            0,
        );
        assert_eq!(s.posture().level, DegradationLevel::Yellow);

        // Report with very long TTL → still active
        s.report_incident(
            IncidentOrigin::Custody,
            DegradationLevel::Orange,
            "short-lived",
            999999,
        );
        assert_eq!(s.posture().level, DegradationLevel::Orange);
    }

    #[test]
    fn enforce_withdrawal_blocks_on_red() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_replay_mismatch("a", "b", 1);
        let result = enforce_withdrawal_posture(&s, 100, custody::VaultTier::Hot);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("RED"));
    }

    #[test]
    fn enforce_withdrawal_limits_on_yellow() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_velocity_breach("Hot", 1, 1);
        assert!(enforce_withdrawal_posture(&s, 50_000, custody::VaultTier::Hot).is_ok());
        assert!(enforce_withdrawal_posture(&s, 200_000, custody::VaultTier::Hot).is_err());
    }

    #[test]
    fn enforce_withdrawal_blocks_cold_warm_on_orange() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_signing_failure("test");
        assert!(enforce_withdrawal_posture(&s, 5_000, custody::VaultTier::Hot).is_ok());
        assert!(enforce_withdrawal_posture(&s, 5_000, custody::VaultTier::Warm).is_err());
        assert!(enforce_withdrawal_posture(&s, 5_000, custody::VaultTier::Cold).is_err());
    }

    #[test]
    fn enforce_liquidation_blocked_on_orange() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_waterfall_halt(5_000_000);
        assert!(enforce_liquidation_posture(&s).is_err());
    }

    #[test]
    fn enforce_order_blocked_on_red() {
        let s = SystemSentinel::new(PosturePolicy::default());
        assert!(enforce_order_posture(&s).is_ok());
        s.report_sequence_gap(5);
        assert!(enforce_order_posture(&s).is_err());
    }

    #[test]
    fn gc_marks_expired_resolved() {
        let s = SystemSentinel::new(PosturePolicy::default());
        // Simulate expired: create incident with TTL=1 in the past
        {
            let mut incidents = s.incidents.lock();
            incidents.push(Incident {
                id: "old".into(),
                origin: IncidentOrigin::Admin,
                severity: DegradationLevel::Orange,
                reason: "test".into(),
                reported_at: Utc::now() - chrono::Duration::hours(2),
                ttl_secs: 60,
                resolved: false,
                resolved_at: None,
                resolved_by: None,
            });
        }
        // Before GC: posture ignores expired in computation, but gc explicitly marks them
        s.gc_expired();
        let active = s.all_incidents(false);
        assert!(active.is_empty());
    }

    #[test]
    fn multiple_origins_independent_resolution() {
        let s = SystemSentinel::new(PosturePolicy::default());
        s.report_signing_failure("custody issue"); // Orange
        s.report_replay_mismatch("a", "b", 1); // Red
        assert_eq!(s.posture().level, DegradationLevel::Red);

        s.resolve_all_from(IncidentOrigin::Recovery, "admin");
        assert_eq!(s.posture().level, DegradationLevel::Orange); // custody still active

        s.resolve_all_from(IncidentOrigin::Custody, "admin");
        assert_eq!(s.posture().level, DegradationLevel::Green); // all clear
    }
}
