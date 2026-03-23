#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Rollback Drills — Recovery Verification & WAL Cleanup
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Supplements crash_recovery_drill and WAL rotation with:
//
//  - WAL backup cleanup policy (retain N most recent .bak files)
//  - State diff / comparison (pre vs post-recovery verification)
//  - Rollback runbook (structured operator guidance)
//  - Recovery readiness check
//  - `/admin/rollback/status`   — recovery readiness & backup inventory
//  - `/admin/rollback/cleanup`  — trigger WAL backup cleanup
//  - `/admin/rollback/runbook`  — operator rollback procedures

use super::*;

// ── WAL backup inventory ─────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct WalBackupEntry {
    pub(crate) filename: String,
    pub(crate) size_bytes: u64,
    pub(crate) modified_at: String,
}

pub(crate) fn scan_wal_backups(data_dir: &str) -> Vec<WalBackupEntry> {
    let dir = std::path::Path::new(data_dir);
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(".bak") {
                if let Ok(meta) = entry.metadata() {
                    let modified = meta
                        .modified()
                        .ok()
                        .map(|t| {
                            let dt: DateTime<Utc> = t.into();
                            dt.to_rfc3339()
                        })
                        .unwrap_or_else(|| "unknown".into());
                    entries.push(WalBackupEntry {
                        filename: name,
                        size_bytes: meta.len(),
                        modified_at: modified,
                    });
                }
            }
        }
    }
    entries.sort_by(|a, b| a.filename.cmp(&b.filename));
    entries
}

// ── Backup cleanup policy ────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct RetentionPolicy {
    pub(crate) max_backups: usize,
    pub(crate) max_total_bytes: u64,
    pub(crate) max_age_secs: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_backups: 10,
            max_total_bytes: 10_737_418_240, // 10 GB
            max_age_secs: 7 * 24 * 3600,     // 7 days
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CleanupResult {
    pub(crate) files_before: usize,
    pub(crate) files_removed: usize,
    pub(crate) bytes_freed: u64,
    pub(crate) files_remaining: usize,
    pub(crate) dry_run: bool,
}

pub(crate) fn cleanup_wal_backups(
    data_dir: &str,
    policy: &RetentionPolicy,
    dry_run: bool,
) -> CleanupResult {
    let mut backups = scan_wal_backups(data_dir);
    let files_before = backups.len();
    let mut files_removed = 0usize;
    let mut bytes_freed = 0u64;

    // Remove oldest first if exceeding max count.
    if backups.len() > policy.max_backups {
        let to_remove = backups.len() - policy.max_backups;
        let removable: Vec<WalBackupEntry> = backups.drain(..to_remove).collect();
        for entry in &removable {
            if !dry_run {
                let path = std::path::Path::new(data_dir).join(&entry.filename);
                if std::fs::remove_file(&path).is_ok() {
                    files_removed += 1;
                    bytes_freed += entry.size_bytes;
                    tracing::info!(file = %entry.filename, "removed old WAL backup");
                }
            } else {
                files_removed += 1;
                bytes_freed += entry.size_bytes;
            }
        }
    }

    // Remove by total size limit.
    if policy.max_total_bytes > 0 {
        let mut total_size: u64 = backups.iter().map(|b| b.size_bytes).sum();
        while total_size > policy.max_total_bytes && !backups.is_empty() {
            let oldest = backups.remove(0);
            total_size -= oldest.size_bytes;
            if !dry_run {
                let path = std::path::Path::new(data_dir).join(&oldest.filename);
                if std::fs::remove_file(&path).is_ok() {
                    files_removed += 1;
                    bytes_freed += oldest.size_bytes;
                    tracing::info!(file = %oldest.filename, "removed WAL backup (size limit)");
                }
            } else {
                files_removed += 1;
                bytes_freed += oldest.size_bytes;
            }
        }
    }

    CleanupResult {
        files_before,
        files_removed,
        bytes_freed,
        files_remaining: files_before - files_removed,
        dry_run,
    }
}

// ── State diff ───────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct StateDiff {
    pub(crate) accounts_match: bool,
    pub(crate) account_count_before: usize,
    pub(crate) account_count_after: usize,
    pub(crate) invariant_before: bool,
    pub(crate) invariant_after: bool,
    pub(crate) message: String,
}

pub(crate) fn compute_state_diff(
    before_account_count: usize,
    before_invariant: bool,
    ledger_after: &LedgerService,
) -> StateDiff {
    let after_count = ledger_after.account_count();
    let after_invariant = ledger_after.verify_global_invariant().is_ok();
    let accounts_match = before_account_count == after_count;
    let message = if accounts_match && after_invariant {
        "state consistent after recovery".into()
    } else if !accounts_match {
        format!("account count mismatch: before={before_account_count} after={after_count}")
    } else {
        "invariant violation after recovery".into()
    };
    StateDiff {
        accounts_match,
        account_count_before: before_account_count,
        account_count_after: after_count,
        invariant_before: before_invariant,
        invariant_after: after_invariant,
        message,
    }
}

// ── Rollback runbook ─────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct RunbookStep {
    pub(crate) step: usize,
    pub(crate) action: String,
    pub(crate) detail: String,
    pub(crate) critical: bool,
}

pub(crate) fn rollback_runbook() -> Vec<RunbookStep> {
    vec![
        RunbookStep {
            step: 1,
            action: "Enable drain mode".into(),
            detail: "POST /admin/ops/drain {\"enable\":true}".into(),
            critical: true,
        },
        RunbookStep {
            step: 2,
            action: "Wait for in-flight orders to clear".into(),
            detail: "Poll GET /health/partitions until inflight counts reach 0".into(),
            critical: true,
        },
        RunbookStep {
            step: 3,
            action: "Create checkpoint".into(),
            detail: "POST /admin/ops/checkpoint".into(),
            critical: true,
        },
        RunbookStep {
            step: 4,
            action: "Stop the exchange process".into(),
            detail: "Send SIGTERM / stop container".into(),
            critical: true,
        },
        RunbookStep {
            step: 5,
            action: "Backup current WAL files".into(),
            detail: "Copy data/*.wal.jsonl to timestamped backup directory".into(),
            critical: true,
        },
        RunbookStep {
            step: 6,
            action: "Restore previous version binary".into(),
            detail: "Replace exchange binary or switch Docker image tag".into(),
            critical: true,
        },
        RunbookStep {
            step: 7,
            action: "Restore WAL files if needed".into(),
            detail: "If WAL format changed, restore .bak files from backup".into(),
            critical: false,
        },
        RunbookStep {
            step: 8,
            action: "Start the exchange".into(),
            detail: "Start process; WAL recovery pipeline will replay from snapshot".into(),
            critical: true,
        },
        RunbookStep {
            step: 9,
            action: "Verify recovery".into(),
            detail: "GET /ready and GET /health to confirm healthy state".into(),
            critical: true,
        },
        RunbookStep {
            step: 10,
            action: "Disable drain mode".into(),
            detail: "POST /admin/ops/drain {\"enable\":false}".into(),
            critical: true,
        },
        RunbookStep {
            step: 11,
            action: "Monitor post-rollback".into(),
            detail: "Watch /metrics/prometheus and /admin/sentinel/posture for 30 min".into(),
            critical: false,
        },
    ]
}

// ── Recovery readiness ───────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct RecoveryReadiness {
    pub(crate) snapshot_available: bool,
    pub(crate) wal_files_present: bool,
    pub(crate) backup_count: usize,
    pub(crate) total_backup_bytes: u64,
    pub(crate) oldest_backup: Option<String>,
    pub(crate) newest_backup: Option<String>,
    pub(crate) recovery_mode: String,
    pub(crate) invariant_ok: bool,
    pub(crate) ready: bool,
}

pub(crate) fn assess_recovery_readiness(ledger: &LedgerService) -> RecoveryReadiness {
    let data_dir = &cfg().wal.data_dir;
    let backups = scan_wal_backups(data_dir);

    let snapshot_path = std::path::Path::new(&cfg().wal.matching_snapshot);
    let snapshot_available = snapshot_path.exists();

    let ledger_wal = std::path::Path::new(&cfg().wal.ledger);
    let seq_wal = std::path::Path::new(&cfg().wal.sequencer);
    let wal_files_present = ledger_wal.exists() && seq_wal.exists();

    let total_bytes: u64 = backups.iter().map(|b| b.size_bytes).sum();
    let oldest = backups.first().map(|b| b.filename.clone());
    let newest = backups.last().map(|b| b.filename.clone());

    let invariant_ok = ledger.verify_global_invariant().is_ok();

    let recovery_mode = std::env::var("WAL_RECOVERY_MODE").unwrap_or_else(|_| "strict".into());

    let ready = wal_files_present && invariant_ok;

    RecoveryReadiness {
        snapshot_available,
        wal_files_present,
        backup_count: backups.len(),
        total_backup_bytes: total_bytes,
        oldest_backup: oldest,
        newest_backup: newest,
        recovery_mode,
        invariant_ok,
        ready,
    }
}

// ── Admin routes ─────────────────────────────────────────────

pub(crate) fn build_rollback_routes(
    ledger: Arc<LedgerService>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let ledger1 = ledger.clone();
    let status_route = warp::path!("admin" / "rollback" / "status")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                let ledger = ledger1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let readiness = assess_recovery_readiness(&ledger);
                    let backups = scan_wal_backups(&cfg().wal.data_dir);
                    let policy = RetentionPolicy::default();

                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "readiness": readiness,
                        "backups": backups,
                        "retention_policy": policy,
                    })))
                }
            },
        );

    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();
    let cleanup_route = warp::path!("admin" / "rollback" / "cleanup")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: CleanupRequest| {
                let ip_rl = ip2.clone();
                let adm_rl = adm2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    let policy = RetentionPolicy {
                        max_backups: req.max_backups.unwrap_or(10),
                        ..RetentionPolicy::default()
                    };
                    let dry_run = req.dry_run.unwrap_or(true);
                    let result = cleanup_wal_backups(&cfg().wal.data_dir, &policy, dry_run);
                    tracing::info!(
                        admin = %principal.subject,
                        files_removed = result.files_removed,
                        bytes_freed = result.bytes_freed,
                        dry_run,
                        "WAL backup cleanup"
                    );

                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!(result)))
                }
            },
        );

    let ip3 = ip_rate_limiter.clone();
    let adm3 = admin_rate_limiter.clone();
    let runbook_route = warp::path!("admin" / "rollback" / "runbook")
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

                    let steps = rollback_runbook();
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "title": "Exchange Rollback Runbook",
                        "version": "1.0",
                        "total_steps": steps.len(),
                        "critical_steps": steps.iter().filter(|s| s.critical).count(),
                        "steps": steps,
                    })))
                }
            },
        );

    let backup_route = build_backup_route(ip_rate_limiter, admin_rate_limiter);

    status_route
        .or(cleanup_route)
        .unify()
        .or(runbook_route)
        .unify()
        .or(backup_route)
        .unify()
        .boxed()
}

// ── Remote backup route ──────────────────────────────────────

fn build_backup_route(
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> impl Filter<Extract = (warp::reply::Json,), Error = Rejection> + Clone {
    let ip = ip_rate_limiter;
    let adm = admin_rate_limiter;
    warp::path!("admin" / "rollback" / "backup")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: BackupRequest| {
                let ip_rl = ip.clone();
                let adm_rl = adm.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    let config = RemoteBackupConfig::from_env().ok_or_else(|| {
                        reject_api(
                            StatusCode::PRECONDITION_FAILED,
                            "BACKUP_REMOTE_DESTINATION not configured",
                        )
                    })?;

                    let dry_run = req.dry_run.unwrap_or(true);
                    let result = sync_backups_to_remote(&cfg().wal.data_dir, &config, dry_run);
                    tracing::info!(
                        admin = %principal.subject,
                        files_copied = result.files_copied,
                        bytes_copied = result.bytes_copied,
                        dry_run,
                        "remote WAL backup sync"
                    );

                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!(result)))
                }
            },
        )
}

// ── Remote backup ────────────────────────────────────────────

/// Remote backup destination configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RemoteBackupConfig {
    /// Destination directory or URL prefix (e.g. "/mnt/nfs/backups" or "s3://bucket/prefix").
    pub(crate) destination: String,
    /// Type of remote storage backend.
    pub(crate) backend: RemoteBackendType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum RemoteBackendType {
    /// Local/NFS — simple file copy to another mount path.
    LocalPath,
    /// Placeholder for S3-compatible object storage.
    S3,
}

impl RemoteBackupConfig {
    /// Build from environment variables.
    pub(crate) fn from_env() -> Option<Self> {
        let destination = std::env::var("BACKUP_REMOTE_DESTINATION").ok()?;
        if destination.trim().is_empty() {
            return None;
        }
        let backend = if destination.starts_with("s3://") {
            RemoteBackendType::S3
        } else {
            RemoteBackendType::LocalPath
        };
        Some(Self {
            destination,
            backend,
        })
    }
}

/// Result of a remote backup operation.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct RemoteBackupResult {
    pub(crate) destination: String,
    pub(crate) backend: RemoteBackendType,
    pub(crate) files_copied: usize,
    pub(crate) bytes_copied: u64,
    pub(crate) files_failed: usize,
    pub(crate) errors: Vec<String>,
    pub(crate) dry_run: bool,
}

/// Copy WAL backup files to a remote destination.
pub(crate) fn sync_backups_to_remote(
    data_dir: &str,
    config: &RemoteBackupConfig,
    dry_run: bool,
) -> RemoteBackupResult {
    let backups = scan_wal_backups(data_dir);
    let mut files_copied = 0usize;
    let mut bytes_copied = 0u64;
    let mut files_failed = 0usize;
    let mut errors = Vec::new();

    match config.backend {
        RemoteBackendType::LocalPath => {
            let dest = std::path::Path::new(&config.destination);
            if !dry_run {
                if let Err(e) = std::fs::create_dir_all(dest) {
                    errors.push(format!("create destination dir: {e}"));
                    return RemoteBackupResult {
                        destination: config.destination.clone(),
                        backend: config.backend,
                        files_copied: 0,
                        bytes_copied: 0,
                        files_failed: backups.len(),
                        errors,
                        dry_run,
                    };
                }
            }
            for entry in &backups {
                let src = std::path::Path::new(data_dir).join(&entry.filename);
                let dst = dest.join(&entry.filename);
                if dry_run {
                    files_copied += 1;
                    bytes_copied += entry.size_bytes;
                    continue;
                }
                match std::fs::copy(&src, &dst) {
                    Ok(n) => {
                        files_copied += 1;
                        bytes_copied += n;
                        tracing::info!(
                            file = %entry.filename,
                            bytes = n,
                            dest = %dst.display(),
                            "remote backup: copied"
                        );
                    }
                    Err(e) => {
                        files_failed += 1;
                        errors.push(format!("{}: {e}", entry.filename));
                        tracing::error!(
                            file = %entry.filename,
                            error = %e,
                            "remote backup: copy failed"
                        );
                    }
                }
            }
        }
        RemoteBackendType::S3 => {
            // S3 uploads require an async HTTP client (aws-sdk-s3 or rusoto).
            // This stub logs the intent; wire in your S3 SDK when deploying.
            for entry in &backups {
                let key = format!(
                    "{}/{}",
                    config.destination.trim_end_matches('/'),
                    entry.filename
                );
                if dry_run {
                    files_copied += 1;
                    bytes_copied += entry.size_bytes;
                } else {
                    tracing::warn!(
                        file = %entry.filename,
                        s3_key = %key,
                        "remote backup: S3 upload not yet wired — add aws-sdk-s3 dependency"
                    );
                    files_failed += 1;
                    errors.push(format!("{}: S3 upload not implemented", entry.filename));
                }
            }
        }
    }

    RemoteBackupResult {
        destination: config.destination.clone(),
        backend: config.backend,
        files_copied,
        bytes_copied,
        files_failed,
        errors,
        dry_run,
    }
}

// ── DTOs ─────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct CleanupRequest {
    pub(crate) dry_run: Option<bool>,
    pub(crate) max_backups: Option<usize>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct BackupRequest {
    pub(crate) dry_run: Option<bool>,
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_policy_defaults() {
        let p = RetentionPolicy::default();
        assert_eq!(p.max_backups, 10);
        assert_eq!(p.max_total_bytes, 10_737_418_240);
        assert_eq!(p.max_age_secs, 7 * 24 * 3600);
    }

    #[test]
    fn scan_backups_nonexistent_dir() {
        let backups = scan_wal_backups("/nonexistent_dir_xyz_42");
        assert!(backups.is_empty());
    }

    #[test]
    fn cleanup_nonexistent_dir_no_panic() {
        let policy = RetentionPolicy::default();
        let result = cleanup_wal_backups("/nonexistent_dir_xyz_42", &policy, true);
        assert_eq!(result.files_before, 0);
        assert_eq!(result.files_removed, 0);
        assert!(result.dry_run);
    }

    #[test]
    fn rollback_runbook_steps() {
        let steps = rollback_runbook();
        assert!(steps.len() >= 10);
        assert!(steps[0].action.contains("drain"));
        let critical_count = steps.iter().filter(|s| s.critical).count();
        assert!(critical_count > steps.len() / 2);
    }

    #[test]
    fn rollback_runbook_step_ordering() {
        let steps = rollback_runbook();
        for (i, step) in steps.iter().enumerate() {
            assert_eq!(step.step, i + 1);
        }
    }

    #[test]
    fn state_diff_matching() {
        let ledger = LedgerService::new(EventBus::new());
        let diff = compute_state_diff(0, true, &ledger);
        assert!(diff.accounts_match);
        assert!(diff.invariant_after);
        assert!(diff.message.contains("consistent"));
    }

    #[test]
    fn state_diff_account_mismatch() {
        let ledger = LedgerService::new(EventBus::new());
        let diff = compute_state_diff(5, true, &ledger);
        assert!(!diff.accounts_match);
        assert!(diff.message.contains("mismatch"));
    }

    #[test]
    fn cleanup_result_dry_run_flag() {
        let result = CleanupResult {
            files_before: 15,
            files_removed: 5,
            bytes_freed: 1024,
            files_remaining: 10,
            dry_run: true,
        };
        assert!(result.dry_run);
        assert_eq!(
            result.files_remaining,
            result.files_before - result.files_removed
        );
    }

    #[test]
    fn scan_wal_backups_in_temp_dir() {
        let tmp = std::env::temp_dir().join("rollback_test_wal_scan");
        let _ = std::fs::create_dir_all(&tmp);
        let bak_path = tmp.join("ledger.wal.jsonl.bak.20260316T120000");
        std::fs::write(&bak_path, "test").unwrap();
        let normal_path = tmp.join("ledger.wal.jsonl");
        std::fs::write(&normal_path, "test").unwrap();

        let backups = scan_wal_backups(tmp.to_str().unwrap());
        assert_eq!(backups.len(), 1);
        assert!(backups[0].filename.contains(".bak"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cleanup_respects_max_backups() {
        let tmp = std::env::temp_dir().join("rollback_test_cleanup");
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::create_dir_all(&tmp);

        for i in 0..5 {
            let path = tmp.join(format!("test.bak.{i:04}"));
            std::fs::write(&path, format!("data{i}")).unwrap();
        }

        let policy = RetentionPolicy {
            max_backups: 2,
            max_total_bytes: 0,
            max_age_secs: 0,
        };
        let result = cleanup_wal_backups(tmp.to_str().unwrap(), &policy, false);
        assert_eq!(result.files_before, 5);
        assert_eq!(result.files_removed, 3);
        assert_eq!(result.files_remaining, 2);

        let remaining = scan_wal_backups(tmp.to_str().unwrap());
        assert_eq!(remaining.len(), 2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remote_backup_config_from_env_missing() {
        let cfg = RemoteBackupConfig::from_env();
        // Unless BACKUP_REMOTE_DESTINATION is set in test env, should be None.
        if std::env::var("BACKUP_REMOTE_DESTINATION").is_err() {
            assert!(cfg.is_none());
        }
    }

    #[test]
    fn remote_backup_config_detects_s3() {
        let cfg = RemoteBackupConfig {
            destination: "s3://bucket/prefix".into(),
            backend: RemoteBackendType::S3,
        };
        assert_eq!(cfg.backend, RemoteBackendType::S3);
    }

    #[test]
    fn remote_backup_config_detects_local() {
        let cfg = RemoteBackupConfig {
            destination: "/mnt/nfs/backups".into(),
            backend: RemoteBackendType::LocalPath,
        };
        assert_eq!(cfg.backend, RemoteBackendType::LocalPath);
    }

    #[test]
    fn sync_backups_local_dry_run() {
        let src = std::env::temp_dir().join("rollback_test_remote_src");
        let dst = std::env::temp_dir().join("rollback_test_remote_dst");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
        let _ = std::fs::create_dir_all(&src);

        // Create a fake .bak file.
        std::fs::write(src.join("ledger.bak.001"), "data").unwrap();

        let config = RemoteBackupConfig {
            destination: dst.to_str().unwrap().into(),
            backend: RemoteBackendType::LocalPath,
        };

        let result = sync_backups_to_remote(src.to_str().unwrap(), &config, true);
        assert!(result.dry_run);
        assert_eq!(result.files_copied, 1);
        assert_eq!(result.files_failed, 0);
        // Destination should NOT exist because it's a dry run.
        assert!(!dst.exists());

        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn sync_backups_local_real_copy() {
        let src = std::env::temp_dir().join("rollback_test_remote_real_src");
        let dst = std::env::temp_dir().join("rollback_test_remote_real_dst");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
        let _ = std::fs::create_dir_all(&src);

        let content = "wal backup data for test";
        std::fs::write(src.join("test.bak.001"), content).unwrap();

        let config = RemoteBackupConfig {
            destination: dst.to_str().unwrap().into(),
            backend: RemoteBackendType::LocalPath,
        };

        let result = sync_backups_to_remote(src.to_str().unwrap(), &config, false);
        assert!(!result.dry_run);
        assert_eq!(result.files_copied, 1);
        assert_eq!(result.files_failed, 0);
        // Verify the file was actually copied.
        assert!(dst.join("test.bak.001").exists());
        let copied = std::fs::read_to_string(dst.join("test.bak.001")).unwrap();
        assert_eq!(copied, content);

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn sync_backups_s3_stub_fails() {
        let src = std::env::temp_dir().join("rollback_test_s3_stub");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::create_dir_all(&src);
        std::fs::write(src.join("test.bak.001"), "data").unwrap();

        let config = RemoteBackupConfig {
            destination: "s3://bucket/prefix".into(),
            backend: RemoteBackendType::S3,
        };

        let result = sync_backups_to_remote(src.to_str().unwrap(), &config, false);
        // S3 is not implemented yet, so all files should fail.
        assert_eq!(result.files_failed, 1);
        assert!(!result.errors.is_empty());

        let _ = std::fs::remove_dir_all(&src);
    }
}
