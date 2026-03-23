#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Release Strategy — Pre-flight Checks, Version Gate, Feature Flags
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Adds:
//  - ReleaseChecklist — structured pre-deployment verification
//  - VersionGate — compare running version vs desired, block downgrade
//  - FeatureGate — runtime feature flags (env-backed)
//  - SchemaVersion — WAL schema version tracking
//  - CanaryHealth — traffic-split health comparison stub
//  - `/admin/release/checklist` — pre-flight verification
//  - `/admin/release/version`   — version gate check
//  - `/admin/release/features`  — feature flag status

use super::*;

// ── Build metadata ───────────────────────────────────────────

pub(crate) struct BuildMeta;

impl BuildMeta {
    pub(crate) fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    pub(crate) fn pkg_name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }
}

// ── Pre-flight checklist ─────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CheckItem {
    pub(crate) name: String,
    pub(crate) passed: bool,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ReleaseChecklist {
    pub(crate) all_passed: bool,
    pub(crate) checks: Vec<CheckItem>,
    pub(crate) version: String,
    pub(crate) checked_at: DateTime<Utc>,
}

pub(crate) fn run_checklist(
    engine: &PartitionedMatchingEngine,
    ledger: &LedgerService,
) -> ReleaseChecklist {
    let mut checks = Vec::new();

    // 1. Ledger invariant must hold.
    let invariant = ledger.verify_global_invariant();
    checks.push(CheckItem {
        name: "ledger_global_invariant".into(),
        passed: invariant.is_ok(),
        detail: if invariant.is_ok() {
            "global balance invariant holds".into()
        } else {
            format!("INVARIANT VIOLATION: {:?}", invariant.err())
        },
    });

    // 2. No partition in shedding state.
    let depths = engine.queue_depths();
    let max_util = depths
        .iter()
        .map(|d| {
            if d.capacity > 0 {
                d.inflight as f64 / d.capacity as f64
            } else {
                0.0
            }
        })
        .fold(0.0_f64, f64::max);
    checks.push(CheckItem {
        name: "queue_not_shedding".into(),
        passed: max_util < 0.95,
        detail: format!("worst partition utilization: {:.1}%", max_util * 100.0),
    });

    // 3. Kill switch must be off.
    let ks = engine.kill_switch_enabled();
    checks.push(CheckItem {
        name: "kill_switch_off".into(),
        passed: !ks,
        detail: if ks {
            "kill switch is ACTIVE".into()
        } else {
            "kill switch is off".into()
        },
    });

    // 4. Drain mode must be off.
    let draining = ops::is_draining();
    checks.push(CheckItem {
        name: "drain_mode_off".into(),
        passed: !draining,
        detail: if draining {
            "system is in drain mode".into()
        } else {
            "not draining".into()
        },
    });

    // 5. WAL data directory exists.
    let data_dir = &cfg().wal.data_dir;
    let dir_ok = std::path::Path::new(data_dir).is_dir();
    checks.push(CheckItem {
        name: "wal_data_dir_writable".into(),
        passed: dir_ok,
        detail: format!("data_dir={data_dir} exists={dir_ok}"),
    });

    // 6. Config validation passes.
    let cfg_problems = cfg().validate();
    checks.push(CheckItem {
        name: "config_valid".into(),
        passed: cfg_problems.is_empty(),
        detail: if cfg_problems.is_empty() {
            "configuration valid".into()
        } else {
            format!("config problems: {}", cfg_problems.join("; "))
        },
    });

    // 7. Backpressure is normal.
    checks.push(CheckItem {
        name: "backpressure_normal".into(),
        passed: max_util < 0.60,
        detail: format!(
            "backpressure level: {}",
            if max_util < 0.60 {
                "Normal"
            } else if max_util < 0.85 {
                "Degraded"
            } else if max_util < 0.95 {
                "Critical"
            } else {
                "Shedding"
            }
        ),
    });

    let all_passed = checks.iter().all(|c| c.passed);
    ReleaseChecklist {
        all_passed,
        checks,
        version: BuildMeta::version().to_string(),
        checked_at: Utc::now(),
    }
}

// ── Version gate ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum VersionRelation {
    Same,
    Upgrade,
    Downgrade,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct VersionGateResult {
    pub(crate) current_version: String,
    pub(crate) target_version: String,
    pub(crate) relation: VersionRelation,
    pub(crate) safe_to_proceed: bool,
    pub(crate) message: String,
}

pub(crate) fn compare_version(target: &str) -> VersionGateResult {
    let current = BuildMeta::version();
    let relation = match (parse_semver(current), parse_semver(target)) {
        (Some(c), Some(t)) => {
            if c == t {
                VersionRelation::Same
            } else if c < t {
                VersionRelation::Upgrade
            } else {
                VersionRelation::Downgrade
            }
        }
        _ => VersionRelation::Unknown,
    };
    let safe = relation != VersionRelation::Downgrade;
    VersionGateResult {
        current_version: current.to_string(),
        target_version: target.to_string(),
        relation,
        safe_to_proceed: safe,
        message: match relation {
            VersionRelation::Same => "same version".into(),
            VersionRelation::Upgrade => format!("{current} -> {target} upgrade permitted"),
            VersionRelation::Downgrade => format!("{target} < {current} DOWNGRADE BLOCKED"),
            VersionRelation::Unknown => "unable to parse version".into(),
        },
    }
}

fn parse_semver(v: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() >= 3 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch_str = parts[2].split('-').next().unwrap_or(parts[2]);
        let patch = patch_str.parse().ok()?;
        Some((major, minor, patch))
    } else {
        None
    }
}

// ── Feature gates ────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct FeatureFlag {
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) source: String,
}

const FEATURE_FLAGS: &[&str] = &[
    "EXCHANGE_FEATURE_ADL",
    "EXCHANGE_FEATURE_PORTFOLIO_MARGIN",
    "EXCHANGE_FEATURE_STOP_ORDERS",
    "EXCHANGE_FEATURE_BATCH_ORDERS",
    "EXCHANGE_FEATURE_FUNDING",
    "EXCHANGE_FEATURE_LIQUIDATION_AUCTION",
    "EXCHANGE_FEATURE_OPTIONS",
    "EXCHANGE_FEATURE_WEBSOCKET_V2",
];

pub(crate) fn feature_flags() -> Vec<FeatureFlag> {
    FEATURE_FLAGS
        .iter()
        .map(|&name| {
            let val = std::env::var(name).unwrap_or_default();
            let enabled = matches!(
                val.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
            FeatureFlag {
                name: name.to_string(),
                enabled,
                source: if val.is_empty() {
                    "default(off)".into()
                } else {
                    format!("env={val}")
                },
            }
        })
        .collect()
}

pub(crate) fn is_feature_enabled(flag_name: &str) -> bool {
    std::env::var(flag_name)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

// ── Schema version ───────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct SchemaVersion {
    pub(crate) wal_format: &'static str,
    pub(crate) ledger_version: u32,
    pub(crate) sequencer_version: u32,
    pub(crate) snapshot_version: u32,
}

impl SchemaVersion {
    pub(crate) fn current() -> Self {
        Self {
            wal_format: "jsonl-v1",
            ledger_version: 1,
            sequencer_version: 1,
            snapshot_version: 1,
        }
    }
}

// ── Canary health stub ───────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CanaryHealth {
    pub(crate) enabled: bool,
    pub(crate) canary_weight_pct: u32,
    pub(crate) verdict: String,
}

impl CanaryHealth {
    pub(crate) fn not_active() -> Self {
        Self {
            enabled: false,
            canary_weight_pct: 0,
            verdict: "canary not active -- single node deployment".into(),
        }
    }
}

// ── Admin routes ─────────────────────────────────────────────

pub(crate) fn build_release_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    ledger: Arc<LedgerService>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let engine1 = partitioned_engine.clone();
    let ledger1 = ledger.clone();
    let checklist_route = warp::path!("admin" / "release" / "checklist")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                let engine = engine1.clone();
                let ledger = ledger1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    let checklist = run_checklist(&engine, &ledger);
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!(checklist)))
                }
            },
        );

    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();
    let version_route = warp::path!("admin" / "release" / "version")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and(warp::query::<VersionQuery>())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  query: VersionQuery| {
                let ip_rl = ip2.clone();
                let adm_rl = adm2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let gate = compare_version(&query.target);
                    let schema = SchemaVersion::current();
                    let canary = CanaryHealth::not_active();

                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "version_gate": gate,
                        "schema": schema,
                        "canary": canary,
                    })))
                }
            },
        );

    let ip3 = ip_rate_limiter.clone();
    let adm3 = admin_rate_limiter.clone();
    let features_route = warp::path!("admin" / "release" / "features")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip3.clone();
                let adm_rl = adm3.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let flags = feature_flags();
                    let enabled_count = flags.iter().filter(|f| f.enabled).count();
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "features": flags,
                        "total": flags.len(),
                        "enabled": enabled_count,
                    })))
                }
            },
        );

    checklist_route
        .or(version_route)
        .unify()
        .or(features_route)
        .unify()
        .boxed()
}

// ── DTOs ─────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct VersionQuery {
    #[serde(default = "default_target")]
    pub(crate) target: String,
}

fn default_target() -> String {
    BuildMeta::version().to_string()
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_semver_valid() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("10.20.30"), Some((10, 20, 30)));
    }

    #[test]
    fn parse_semver_with_prerelease() {
        assert_eq!(parse_semver("1.2.3-alpha"), Some((1, 2, 3)));
    }

    #[test]
    fn parse_semver_invalid() {
        assert_eq!(parse_semver("abc"), None);
        assert_eq!(parse_semver("1.2"), None);
        assert_eq!(parse_semver(""), None);
    }

    #[test]
    fn version_gate_same() {
        let current = BuildMeta::version();
        let result = compare_version(current);
        assert_eq!(result.relation, VersionRelation::Same);
        assert!(result.safe_to_proceed);
    }

    #[test]
    fn version_gate_upgrade() {
        let result = compare_version("99.99.99");
        assert_eq!(result.relation, VersionRelation::Upgrade);
        assert!(result.safe_to_proceed);
    }

    #[test]
    fn version_gate_downgrade() {
        let result = compare_version("0.0.1");
        if BuildMeta::version() != "0.0.1" {
            assert_eq!(result.relation, VersionRelation::Downgrade);
            assert!(!result.safe_to_proceed);
        }
    }

    #[test]
    fn version_gate_unknown() {
        let result = compare_version("not-a-version");
        assert_eq!(result.relation, VersionRelation::Unknown);
    }

    #[test]
    fn feature_flags_returns_all() {
        let flags = feature_flags();
        assert_eq!(flags.len(), FEATURE_FLAGS.len());
    }

    #[test]
    fn is_feature_enabled_default_off() {
        assert!(!is_feature_enabled("EXCHANGE_FEATURE_TEST_NONEXISTENT_XYZ"));
    }

    #[test]
    fn schema_version_current() {
        let sv = SchemaVersion::current();
        assert_eq!(sv.wal_format, "jsonl-v1");
        assert!(sv.ledger_version >= 1);
    }

    #[test]
    fn canary_health_not_active() {
        let ch = CanaryHealth::not_active();
        assert!(!ch.enabled);
        assert_eq!(ch.canary_weight_pct, 0);
    }

    #[test]
    fn build_meta_version() {
        let v = BuildMeta::version();
        assert!(!v.is_empty());
        assert!(parse_semver(v).is_some());
    }

    #[test]
    fn build_meta_pkg_name() {
        assert_eq!(BuildMeta::pkg_name(), "api");
    }

    #[test]
    fn default_version_query() {
        let target = default_target();
        assert!(!target.is_empty());
    }
}
