#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// On-Call Mechanism — Alerting, Escalation, Runbooks, Incident Lifecycle
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Supplements the existing SystemSentinel (sentinel.rs) with:
//
//  • AlertChannel — webhook/log-based external notification hooks
//  • EscalationPolicy — automated severity → action mapping
//  • OnCallSchedule — rotation config (primary/secondary/escalation)
//  • IncidentTimeline — structured incident lifecycle events
//  • RunbookRegistry — per-incident-type runbook references
//  • DeadManSwitch — periodic heartbeat check for silent failures
//  • `/admin/oncall/status`      — on-call rotation & alert channel health
//  • `/admin/oncall/escalation`  — escalation policy & history
//  • `/admin/oncall/runbooks`    — runbook registry lookup
//
// All notification hooks are trait-based for pluggability (webhook, log,
// PagerDuty, Slack, etc.). Default: log-only channel.

use super::*;

// ── Alert channels ───────────────────────────────────────────

/// Trait for sending alerts to external systems.
pub(crate) trait AlertChannel: Send + Sync {
    /// Send an alert notification. Returns Ok(()) on success.
    fn send(&self, alert: &AlertNotification) -> Result<(), String>;
    /// Channel name for display.
    fn name(&self) -> &str;
    /// Whether this channel is healthy/reachable.
    fn healthy(&self) -> bool;
}

/// An alert notification payload.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AlertNotification {
    pub(crate) severity: AlertLevel,
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) source: String,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) incident_id: Option<String>,
    pub(crate) runbook_url: Option<String>,
}

/// Alert severity levels for on-call notification.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum AlertLevel {
    Info,
    Warning,
    Critical,
    Emergency,
}

/// Log-only alert channel (default — no external dependencies).
pub(crate) struct LogAlertChannel;

impl AlertChannel for LogAlertChannel {
    fn send(&self, alert: &AlertNotification) -> Result<(), String> {
        match alert.severity {
            AlertLevel::Info => tracing::info!(
                title = %alert.title,
                source = %alert.source,
                "ALERT [INFO]: {}",
                alert.message
            ),
            AlertLevel::Warning => tracing::warn!(
                title = %alert.title,
                source = %alert.source,
                "ALERT [WARNING]: {}",
                alert.message
            ),
            AlertLevel::Critical => tracing::error!(
                title = %alert.title,
                source = %alert.source,
                "ALERT [CRITICAL]: {}",
                alert.message
            ),
            AlertLevel::Emergency => tracing::error!(
                title = %alert.title,
                source = %alert.source,
                "ALERT [EMERGENCY]: {}",
                alert.message
            ),
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "log"
    }

    fn healthy(&self) -> bool {
        true
    }
}

/// Webhook alert channel — POST JSON to a configured URL.
/// URL is read from env var at construction time for safety.
pub(crate) struct WebhookAlertChannel {
    name: String,
    url: String,
}

impl WebhookAlertChannel {
    /// Create from environment variable name (e.g. "ONCALL_WEBHOOK_URL").
    /// Returns None if the env var is not set.
    pub(crate) fn from_env(env_var: &str) -> Option<Self> {
        let url = std::env::var(env_var).ok()?;
        if url.trim().is_empty() {
            return None;
        }
        Some(Self {
            name: format!("webhook({env_var})"),
            url,
        })
    }
}

impl AlertChannel for WebhookAlertChannel {
    fn send(&self, alert: &AlertNotification) -> Result<(), String> {
        let body = serde_json::to_string(alert).map_err(|e| e.to_string())?;
        match post_webhook(&self.url, &body) {
            Ok(status) => {
                tracing::info!(
                    channel = %self.name,
                    url = %self.url,
                    status,
                    severity = ?alert.severity,
                    title = %alert.title,
                    "WEBHOOK ALERT sent"
                );
                if (200..300).contains(&status) {
                    Ok(())
                } else {
                    Err(format!("webhook returned HTTP {status}"))
                }
            }
            Err(e) => {
                tracing::error!(
                    channel = %self.name,
                    url = %self.url,
                    error = %e,
                    "WEBHOOK ALERT delivery failed"
                );
                Err(e)
            }
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn healthy(&self) -> bool {
        !self.url.is_empty()
    }
}

/// Fire a blocking HTTP POST with JSON body to the given URL.
/// Returns the HTTP status code on success.
fn post_webhook(url: &str, json_body: &str) -> Result<u16, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    // Parse URL — only support http:// for simplicity; HTTPS requires TLS.
    let stripped = url.strip_prefix("http://").ok_or_else(|| {
        "only http:// URLs are supported (use a sidecar proxy for TLS)".to_string()
    })?;

    let (host_port, path) = match stripped.find('/') {
        Some(i) => (&stripped[..i], &stripped[i..]),
        None => (stripped, "/"),
    };

    let addr = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{host_port}:80")
    };

    let mut stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|e: std::net::AddrParseError| e.to_string())?,
        Duration::from_secs(5),
    )
    .map_err(|e| format!("connect to {addr}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host_port}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {json_body}",
        json_body.len()
    );

    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).map_err(|e| format!("read: {e}"))?;
    let response = String::from_utf8_lossy(&buf[..n]);

    // Parse "HTTP/1.x STATUS ..."
    let status_str = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "invalid HTTP response".to_string())?;

    status_str
        .parse::<u16>()
        .map_err(|e| format!("parse status: {e}"))
}

// ── Escalation policy ────────────────────────────────────────

/// Maps alert severity to required actions.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct EscalationRule {
    pub(crate) level: AlertLevel,
    pub(crate) action: String,
    pub(crate) notify_channels: Vec<String>,
    pub(crate) auto_escalate_after_secs: u64,
    pub(crate) requires_ack: bool,
}

/// Full escalation policy.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct EscalationPolicy {
    pub(crate) rules: Vec<EscalationRule>,
}

impl Default for EscalationPolicy {
    fn default() -> Self {
        Self {
            rules: vec![
                EscalationRule {
                    level: AlertLevel::Info,
                    action: "Log only — no notification".into(),
                    notify_channels: vec!["log".into()],
                    auto_escalate_after_secs: 0, // no escalation
                    requires_ack: false,
                },
                EscalationRule {
                    level: AlertLevel::Warning,
                    action: "Notify primary on-call via webhook/Slack".into(),
                    notify_channels: vec!["log".into(), "webhook".into()],
                    auto_escalate_after_secs: 1800, // 30 min
                    requires_ack: false,
                },
                EscalationRule {
                    level: AlertLevel::Critical,
                    action: "Page primary on-call — immediate response required".into(),
                    notify_channels: vec!["log".into(), "webhook".into(), "pager".into()],
                    auto_escalate_after_secs: 900, // 15 min → escalate to secondary
                    requires_ack: true,
                },
                EscalationRule {
                    level: AlertLevel::Emergency,
                    action: "Page ALL on-call + engineering lead — war room".into(),
                    notify_channels: vec![
                        "log".into(),
                        "webhook".into(),
                        "pager".into(),
                        "phone".into(),
                    ],
                    auto_escalate_after_secs: 300, // 5 min → escalate to management
                    requires_ack: true,
                },
            ],
        }
    }
}

impl EscalationPolicy {
    /// Find the escalation rule for a given severity.
    pub(crate) fn rule_for(&self, level: AlertLevel) -> Option<&EscalationRule> {
        self.rules.iter().find(|r| r.level == level)
    }
}

// ── On-call schedule ─────────────────────────────────────────

/// An on-call responder.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct OnCallResponder {
    pub(crate) name: String,
    pub(crate) role: ResponderRole,
    pub(crate) contact: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ResponderRole {
    Primary,
    Secondary,
    EscalationLead,
    EngineeringManager,
}

/// On-call rotation schedule.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct OnCallSchedule {
    pub(crate) current_rotation: Vec<OnCallResponder>,
    pub(crate) rotation_period: String,
    pub(crate) timezone: String,
}

impl Default for OnCallSchedule {
    fn default() -> Self {
        // Default schedule — read from env or use placeholders.
        let primary_name =
            std::env::var("ONCALL_PRIMARY_NAME").unwrap_or_else(|_| "unassigned".into());
        let primary_contact =
            std::env::var("ONCALL_PRIMARY_CONTACT").unwrap_or_else(|_| "unassigned".into());
        let secondary_name =
            std::env::var("ONCALL_SECONDARY_NAME").unwrap_or_else(|_| "unassigned".into());
        let secondary_contact =
            std::env::var("ONCALL_SECONDARY_CONTACT").unwrap_or_else(|_| "unassigned".into());
        let lead_name = std::env::var("ONCALL_LEAD_NAME").unwrap_or_else(|_| "unassigned".into());
        let lead_contact =
            std::env::var("ONCALL_LEAD_CONTACT").unwrap_or_else(|_| "unassigned".into());

        Self {
            current_rotation: vec![
                OnCallResponder {
                    name: primary_name,
                    role: ResponderRole::Primary,
                    contact: primary_contact,
                },
                OnCallResponder {
                    name: secondary_name,
                    role: ResponderRole::Secondary,
                    contact: secondary_contact,
                },
                OnCallResponder {
                    name: lead_name,
                    role: ResponderRole::EscalationLead,
                    contact: lead_contact,
                },
            ],
            rotation_period: "weekly".into(),
            timezone: std::env::var("ONCALL_TIMEZONE").unwrap_or_else(|_| "UTC".into()),
        }
    }
}

// ── Incident timeline ────────────────────────────────────────

/// A structured event in an incident's lifecycle.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct TimelineEvent {
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) event_type: TimelineEventType,
    pub(crate) detail: String,
    pub(crate) actor: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TimelineEventType {
    Created,
    Acknowledged,
    Escalated,
    Mitigated,
    Resolved,
    PostMortem,
}

// ── Runbook registry ─────────────────────────────────────────

/// A runbook entry linking incident types to procedures.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct RunbookEntry {
    pub(crate) incident_type: String,
    pub(crate) title: String,
    pub(crate) steps: Vec<String>,
    pub(crate) severity_hint: AlertLevel,
    pub(crate) estimated_ttm_minutes: u32,
}

/// Built-in runbook registry for common incident types.
pub(crate) fn runbook_registry() -> Vec<RunbookEntry> {
    vec![
        RunbookEntry {
            incident_type: "ledger_invariant_violation".into(),
            title: "Ledger Balance Invariant Violation".into(),
            steps: vec![
                "1. Enable kill switch: POST /admin/kill-switch".into(),
                "2. Enable drain mode: POST /admin/ops/drain".into(),
                "3. Check /ready for detailed invariant status".into(),
                "4. Force checkpoint: POST /admin/ops/checkpoint".into(),
                "5. If persistent: stop exchange, restore from last known-good snapshot".into(),
                "6. Engage engineering for root-cause analysis".into(),
            ],
            severity_hint: AlertLevel::Emergency,
            estimated_ttm_minutes: 15,
        },
        RunbookEntry {
            incident_type: "queue_shedding".into(),
            title: "Matching Engine Queue Shedding".into(),
            steps: vec![
                "1. Check /health/partitions for per-partition utilization".into(),
                "2. If single partition overloaded: investigate hot market".into(),
                "3. Consider enabling kill switch for affected market only".into(),
                "4. If system-wide: enable drain mode to shed load".into(),
                "5. Check /admin/capacity for capacity report and scaling advice".into(),
            ],
            severity_hint: AlertLevel::Critical,
            estimated_ttm_minutes: 10,
        },
        RunbookEntry {
            incident_type: "wal_corruption".into(),
            title: "WAL File Corruption Detected".into(),
            steps: vec![
                "1. Enable drain + kill switch immediately".into(),
                "2. Check WAL data directory for .bak files".into(),
                "3. Consider switching to BestEffort recovery mode: WAL_RECOVERY_MODE=best_effort".into(),
                "4. Force checkpoint of uncorrupted state".into(),
                "5. If unrecoverable: restore from last backup + replay".into(),
            ],
            severity_hint: AlertLevel::Emergency,
            estimated_ttm_minutes: 30,
        },
        RunbookEntry {
            incident_type: "custody_signing_failure".into(),
            title: "Custody Signing / HSM Failure".into(),
            steps: vec![
                "1. Sentinel will auto-restrict to Orange (limited withdrawals)".into(),
                "2. Check sentinel posture: GET /admin/sentinel/posture".into(),
                "3. Verify HSM connectivity and signing key availability".into(),
                "4. If HSM failure: contact custody provider".into(),
                "5. Resolve incident when signing is restored: POST /admin/sentinel/incidents/{id}/resolve".into(),
            ],
            severity_hint: AlertLevel::Critical,
            estimated_ttm_minutes: 20,
        },
        RunbookEntry {
            incident_type: "high_ws_connections".into(),
            title: "WebSocket Connection Saturation".into(),
            steps: vec![
                "1. Check /admin/capacity/alerts for WS connection utilization".into(),
                "2. Check for connection leak (clients not closing properly)".into(),
                "3. If legitimate load: increase WS_MAX_CONNECTIONS env var".into(),
                "4. Consider rate-limiting new WS connections".into(),
            ],
            severity_hint: AlertLevel::Warning,
            estimated_ttm_minutes: 10,
        },
        RunbookEntry {
            incident_type: "disk_space_low".into(),
            title: "Low Disk Space on WAL Directory".into(),
            steps: vec![
                "1. Check /admin/rollback/status for backup inventory".into(),
                "2. Trigger cleanup: POST /admin/rollback/cleanup {\"dry_run\":false}".into(),
                "3. If still low: archive old WAL backups to remote storage".into(),
                "4. Monitor disk utilization via /admin/capacity".into(),
            ],
            severity_hint: AlertLevel::Warning,
            estimated_ttm_minutes: 15,
        },
        RunbookEntry {
            incident_type: "sequence_gap".into(),
            title: "Sequence Gap / Replay Mismatch".into(),
            steps: vec![
                "1. Sentinel will auto-set Red level — all operations halted".into(),
                "2. This is a CRITICAL data integrity issue".into(),
                "3. Stop exchange immediately".into(),
                "4. Run crash recovery drill: cargo run --example crash_recovery_drill".into(),
                "5. If drill fails: manual WAL replay investigation required".into(),
                "6. Engage engineering lead — post-mortem mandatory".into(),
            ],
            severity_hint: AlertLevel::Emergency,
            estimated_ttm_minutes: 60,
        },
        RunbookEntry {
            incident_type: "liquidation_cascade".into(),
            title: "Liquidation Cascade / ADL Triggered".into(),
            steps: vec![
                "1. Check /admin/liquidations/queue for pending liquidations".into(),
                "2. Check /admin/sentinel/posture for degradation level".into(),
                "3. If Orange/Red: manual liquidation oversight required".into(),
                "4. Review insurance fund balance: GET /admin/treasury/insurance-funds".into(),
                "5. Consider market-specific kill switch if isolated to one market".into(),
            ],
            severity_hint: AlertLevel::Critical,
            estimated_ttm_minutes: 20,
        },
    ]
}

/// Look up a runbook by incident type.
pub(crate) fn lookup_runbook(incident_type: &str) -> Option<RunbookEntry> {
    runbook_registry()
        .into_iter()
        .find(|r| r.incident_type == incident_type)
}

// ── Dead-man's switch ────────────────────────────────────────

/// Dead-man's switch — tracks heartbeats to detect silent failures.
pub(crate) struct DeadManSwitch {
    last_heartbeat: std::sync::atomic::AtomicU64,
    threshold_secs: u64,
}

impl DeadManSwitch {
    pub(crate) fn new(threshold_secs: u64) -> Self {
        Self {
            last_heartbeat: AtomicU64::new(Utc::now().timestamp() as u64),
            threshold_secs,
        }
    }

    /// Record a heartbeat (call periodically from main loop).
    pub(crate) fn heartbeat(&self) {
        self.last_heartbeat
            .store(Utc::now().timestamp() as u64, Ordering::Relaxed);
    }

    /// Check if the switch has expired (no heartbeat within threshold).
    pub(crate) fn is_expired(&self) -> bool {
        let last = self.last_heartbeat.load(Ordering::Relaxed);
        let now = Utc::now().timestamp() as u64;
        now.saturating_sub(last) > self.threshold_secs
    }

    /// Seconds since last heartbeat.
    pub(crate) fn elapsed_secs(&self) -> u64 {
        let last = self.last_heartbeat.load(Ordering::Relaxed);
        let now = Utc::now().timestamp() as u64;
        now.saturating_sub(last)
    }
}

// ── On-call status (aggregated) ──────────────────────────────

/// Full on-call status report.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct OnCallStatus {
    pub(crate) schedule: OnCallSchedule,
    pub(crate) escalation_policy: EscalationPolicy,
    pub(crate) alert_channels: Vec<AlertChannelStatus>,
    pub(crate) dead_man_switch: DeadManSwitchStatus,
    pub(crate) runbook_count: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AlertChannelStatus {
    pub(crate) name: String,
    pub(crate) healthy: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DeadManSwitchStatus {
    pub(crate) enabled: bool,
    pub(crate) threshold_secs: u64,
    pub(crate) elapsed_secs: u64,
    pub(crate) expired: bool,
}

// ── Admin routes ─────────────────────────────────────────────

pub(crate) fn build_oncall_routes(
    dead_man_switch: Arc<DeadManSwitch>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    // GET /admin/oncall/status — full on-call status
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let dms1 = dead_man_switch.clone();
    let status_route = warp::path!("admin" / "oncall" / "status")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                let dms = dms1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let log_channel = LogAlertChannel;
                    let webhook_available =
                        WebhookAlertChannel::from_env("ONCALL_WEBHOOK_URL").is_some();
                    let mut channels = vec![AlertChannelStatus {
                        name: log_channel.name().to_string(),
                        healthy: log_channel.healthy(),
                    }];
                    if webhook_available {
                        channels.push(AlertChannelStatus {
                            name: "webhook(ONCALL_WEBHOOK_URL)".into(),
                            healthy: true,
                        });
                    }

                    let status = OnCallStatus {
                        schedule: OnCallSchedule::default(),
                        escalation_policy: EscalationPolicy::default(),
                        alert_channels: channels,
                        dead_man_switch: DeadManSwitchStatus {
                            enabled: true,
                            threshold_secs: dms.threshold_secs,
                            elapsed_secs: dms.elapsed_secs(),
                            expired: dms.is_expired(),
                        },
                        runbook_count: runbook_registry().len(),
                    };

                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!(status)))
                }
            },
        );

    // GET /admin/oncall/escalation — escalation policy detail
    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();
    let escalation_route = warp::path!("admin" / "oncall" / "escalation")
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

                    let policy = EscalationPolicy::default();
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "policy": policy,
                    })))
                }
            },
        );

    // GET /admin/oncall/runbooks — runbook registry
    let ip3 = ip_rate_limiter.clone();
    let adm3 = admin_rate_limiter.clone();
    let runbooks_route = warp::path!("admin" / "oncall" / "runbooks")
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

                    let runbooks = runbook_registry();
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "runbooks": runbooks,
                        "total": runbooks.len(),
                    })))
                }
            },
        );

    status_route
        .or(escalation_route)
        .unify()
        .or(runbooks_route)
        .unify()
        .boxed()
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_alert_channel_send() {
        let ch = LogAlertChannel;
        let alert = AlertNotification {
            severity: AlertLevel::Warning,
            title: "test alert".into(),
            message: "test message".into(),
            source: "test".into(),
            timestamp: Utc::now(),
            incident_id: None,
            runbook_url: None,
        };
        assert!(ch.send(&alert).is_ok());
        assert_eq!(ch.name(), "log");
        assert!(ch.healthy());
    }

    #[test]
    fn log_alert_all_severities() {
        let ch = LogAlertChannel;
        for severity in [
            AlertLevel::Info,
            AlertLevel::Warning,
            AlertLevel::Critical,
            AlertLevel::Emergency,
        ] {
            let alert = AlertNotification {
                severity,
                title: format!("{severity:?} test"),
                message: "testing".into(),
                source: "test".into(),
                timestamp: Utc::now(),
                incident_id: None,
                runbook_url: None,
            };
            assert!(ch.send(&alert).is_ok());
        }
    }

    #[test]
    fn escalation_policy_defaults() {
        let policy = EscalationPolicy::default();
        assert_eq!(policy.rules.len(), 4);
        assert_eq!(policy.rules[0].level, AlertLevel::Info);
        assert_eq!(policy.rules[3].level, AlertLevel::Emergency);
        // Emergency should require ack.
        assert!(policy.rules[3].requires_ack);
        // Info should not require ack.
        assert!(!policy.rules[0].requires_ack);
    }

    #[test]
    fn escalation_rule_lookup() {
        let policy = EscalationPolicy::default();
        let rule = policy.rule_for(AlertLevel::Critical).unwrap();
        assert!(rule.requires_ack);
        assert!(rule.auto_escalate_after_secs > 0);
    }

    #[test]
    fn escalation_rule_unknown_level() {
        // All four levels should have rules.
        let policy = EscalationPolicy::default();
        assert!(policy.rule_for(AlertLevel::Info).is_some());
        assert!(policy.rule_for(AlertLevel::Warning).is_some());
        assert!(policy.rule_for(AlertLevel::Critical).is_some());
        assert!(policy.rule_for(AlertLevel::Emergency).is_some());
    }

    #[test]
    fn oncall_schedule_default() {
        let schedule = OnCallSchedule::default();
        assert_eq!(schedule.current_rotation.len(), 3);
        assert_eq!(schedule.current_rotation[0].role, ResponderRole::Primary);
        assert_eq!(schedule.current_rotation[1].role, ResponderRole::Secondary);
        assert_eq!(
            schedule.current_rotation[2].role,
            ResponderRole::EscalationLead
        );
        assert_eq!(schedule.rotation_period, "weekly");
    }

    #[test]
    fn runbook_registry_non_empty() {
        let runbooks = runbook_registry();
        assert!(runbooks.len() >= 5);
        // Each runbook should have steps.
        for rb in &runbooks {
            assert!(!rb.incident_type.is_empty());
            assert!(!rb.title.is_empty());
            assert!(!rb.steps.is_empty());
            assert!(rb.estimated_ttm_minutes > 0);
        }
    }

    #[test]
    fn runbook_lookup_existing() {
        let rb = lookup_runbook("ledger_invariant_violation");
        assert!(rb.is_some());
        let rb = rb.unwrap();
        assert_eq!(rb.severity_hint, AlertLevel::Emergency);
    }

    #[test]
    fn runbook_lookup_nonexistent() {
        let rb = lookup_runbook("nonexistent_incident_type_xyz");
        assert!(rb.is_none());
    }

    #[test]
    fn dead_man_switch_fresh() {
        let dms = DeadManSwitch::new(60);
        assert!(!dms.is_expired());
        assert!(dms.elapsed_secs() <= 1);
    }

    #[test]
    fn dead_man_switch_heartbeat() {
        let dms = DeadManSwitch::new(60);
        dms.heartbeat();
        assert!(!dms.is_expired());
    }

    #[test]
    fn dead_man_switch_expired() {
        let dms = DeadManSwitch::new(0); // threshold = 0 seconds
                                         // Wait a tiny bit.
        std::thread::sleep(std::time::Duration::from_millis(10));
        // With threshold 0, any elapsed time means expired.
        // Actually elapsed_secs is integer, so 0 seconds may not trigger.
        // Use a threshold of 0 and set last_heartbeat to the past.
        dms.last_heartbeat.store(0, Ordering::Relaxed);
        assert!(dms.is_expired());
        assert!(dms.elapsed_secs() > 0);
    }

    #[test]
    fn alert_level_ordering() {
        assert!(AlertLevel::Info < AlertLevel::Warning);
        assert!(AlertLevel::Warning < AlertLevel::Critical);
        assert!(AlertLevel::Critical < AlertLevel::Emergency);
    }

    #[test]
    fn timeline_event_type_coverage() {
        // Ensure all variants can be constructed.
        let events = [
            TimelineEventType::Created,
            TimelineEventType::Acknowledged,
            TimelineEventType::Escalated,
            TimelineEventType::Mitigated,
            TimelineEventType::Resolved,
            TimelineEventType::PostMortem,
        ];
        assert_eq!(events.len(), 6);
    }

    #[test]
    fn webhook_channel_from_env_missing() {
        // This env var should not be set in tests.
        let ch = WebhookAlertChannel::from_env("ONCALL_WEBHOOK_TEST_NONEXISTENT_XYZ");
        assert!(ch.is_none());
    }

    #[test]
    fn oncall_status_report_structure() {
        let dms = DeadManSwitch::new(120);
        let status = DeadManSwitchStatus {
            enabled: true,
            threshold_secs: dms.threshold_secs,
            elapsed_secs: dms.elapsed_secs(),
            expired: dms.is_expired(),
        };
        assert!(status.enabled);
        assert_eq!(status.threshold_secs, 120);
        assert!(!status.expired);
    }

    #[test]
    fn responder_role_equality() {
        assert_eq!(ResponderRole::Primary, ResponderRole::Primary);
        assert_ne!(ResponderRole::Primary, ResponderRole::Secondary);
    }
}
