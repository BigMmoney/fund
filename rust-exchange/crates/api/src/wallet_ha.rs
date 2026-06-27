// Wallet worker leader election — addresses P2-SCALE-3 (single-active
// settlement worker with passive standby).
//
// The wallet settlement worker MUST be single-writer. Running two
// instances against the same ledger races for op_ids and breaks the
// idempotency invariant. The hot-wallet on-chain broadcaster is also
// inherently single-writer (otherwise two replicas both broadcast the
// same withdrawal). So the worker tier needs leader election.
//
// Why a lease file and not etcd / consul / Raft:
//   * Zero new infrastructure dependency.
//   * Works on every k8s setup with a ReadWriteOnce PVC (which the
//     exchange already provisions).
//   * Recovery time on leader crash is bounded by `lease_ttl` (default
//     15s) — operators tune to balance failover speed vs flapping risk.
//
// Failure modes covered:
//   * Leader process dies cleanly → standby takes over within
//     `lease_ttl` once the lease mtime is stale.
//   * Leader hangs (e.g. blocked on I/O) → lease not refreshed →
//     standby takes over.
//   * Shared volume disappears → leader loses the lease but cannot
//     refresh; standby also cannot acquire (no file). Both halt the
//     worker. Operators must restore the volume; no data loss because
//     all work is WAL-backed.
//
// Failure modes NOT covered (out of scope for v1):
//   * Network partition with clock skew between leader and standby.
//     A real production deployment should pair this with a fence
//     token at the ledger layer (op_id includes lease epoch).
//   * Split-brain across distinct PVCs (e.g. operator misconfigures
//     two pods to use different volumes). Use a single PVC per
//     wallet worker pool; mount as RWO.
//
// Wire-up: `LeaseLeader::run` returns a clone-able handle whose
// `is_leader()` method gates each worker tick. The wallet worker loop
// becomes:
//
//   if leader.is_leader() {
//       worker.tick();
//   } else {
//       // do nothing this tick; standby state
//   }

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct LeaseConfig {
    pub lease_path: PathBuf,
    pub instance_id: String,
    pub lease_ttl: Duration,
    pub refresh_interval: Duration,
    pub acquire_retry_interval: Duration,
}

impl LeaseConfig {
    pub fn from_env(default_lease_dir: impl AsRef<Path>) -> Self {
        let dir = default_lease_dir.as_ref();
        let lease_path = std::env::var("WALLET_HA_LEASE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dir.join("wallet-leader.lease"));
        let instance_id = std::env::var("WALLET_HA_INSTANCE_ID")
            .unwrap_or_else(|_| default_instance_id());
        let lease_ttl_secs =
            parse_env_u64("WALLET_HA_LEASE_TTL_SECS").unwrap_or(15).max(2);
        let refresh_secs =
            parse_env_u64("WALLET_HA_REFRESH_SECS").unwrap_or(5).max(1);
        let acquire_retry_secs =
            parse_env_u64("WALLET_HA_ACQUIRE_RETRY_SECS").unwrap_or(5).max(1);
        Self {
            lease_path,
            instance_id,
            lease_ttl: Duration::from_secs(lease_ttl_secs),
            refresh_interval: Duration::from_secs(refresh_secs),
            acquire_retry_interval: Duration::from_secs(acquire_retry_secs),
        }
    }
}

fn parse_env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok().and_then(|v| v.parse::<u64>().ok())
}

fn default_instance_id() -> String {
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".into());
    let pid = std::process::id();
    format!("{hostname}#{pid}")
}

/// Handle returned to the worker loop. Cheap to clone; reads an atomic
/// flag set by the background election task.
#[derive(Clone)]
pub struct LeaseLeader {
    is_leader: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    instance_id: String,
}

impl LeaseLeader {
    /// True when this replica currently holds the lease.
    pub fn is_leader(&self) -> bool {
        self.is_leader.load(Ordering::Acquire)
    }

    /// Monotonically increasing lease epoch — useful as a fence token
    /// on persisted state so a stale leader cannot retroactively
    /// overwrite a newer leader's work.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LeaseRecord {
    instance_id: String,
    /// Unix-epoch seconds at which the lease was last refreshed.
    refreshed_at_unix_s: u64,
    /// Monotonically increasing epoch — bumped on every successful
    /// re-acquire (transition from non-leader to leader).
    epoch: u64,
}

/// Spawn a background task that continuously tries to acquire and
/// refresh the lease. Returns a `LeaseLeader` handle the worker checks
/// before each tick.
pub fn spawn_lease_election(config: LeaseConfig) -> LeaseLeader {
    let is_leader = Arc::new(AtomicBool::new(false));
    let epoch = Arc::new(AtomicU64::new(0));
    let handle = LeaseLeader {
        is_leader: is_leader.clone(),
        epoch: epoch.clone(),
        instance_id: config.instance_id.clone(),
    };
    let task_cfg = config.clone();
    tokio::spawn(async move {
        run_election_loop(task_cfg, is_leader, epoch).await;
    });
    handle
}

async fn run_election_loop(
    config: LeaseConfig,
    is_leader: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
) {
    loop {
        if is_leader.load(Ordering::Acquire) {
            // We are leader — refresh the lease.
            match refresh_lease(&config, epoch.load(Ordering::Acquire)) {
                Ok(()) => {
                    tokio::time::sleep(config.refresh_interval).await;
                }
                Err(err) => {
                    tracing::error!(
                        instance_id = %config.instance_id,
                        error = %err,
                        "wallet_ha: failed to refresh lease — stepping down"
                    );
                    is_leader.store(false, Ordering::Release);
                    tokio::time::sleep(config.acquire_retry_interval).await;
                }
            }
        } else {
            // Try to acquire.
            match try_acquire(&config) {
                Ok(Some(new_epoch)) => {
                    epoch.store(new_epoch, Ordering::Release);
                    is_leader.store(true, Ordering::Release);
                    tracing::info!(
                        instance_id = %config.instance_id,
                        epoch = new_epoch,
                        "wallet_ha: acquired leadership"
                    );
                }
                Ok(None) => {
                    // Another replica holds a fresh lease.
                }
                Err(err) => {
                    tracing::warn!(
                        instance_id = %config.instance_id,
                        error = %err,
                        "wallet_ha: acquire attempt failed"
                    );
                }
            }
            tokio::time::sleep(config.acquire_retry_interval).await;
        }
    }
}

/// Attempt to acquire the lease. Returns `Ok(Some(epoch))` if we won
/// the election, `Ok(None)` if a peer holds a still-valid lease, and
/// `Err` on IO error.
fn try_acquire(config: &LeaseConfig) -> Result<Option<u64>> {
    ensure_parent_dir(&config.lease_path)?;
    let now = unix_seconds_now()?;
    let existing = read_lease_if_present(&config.lease_path)?;
    let next_epoch = match existing {
        Some(record) => {
            let age = now.saturating_sub(record.refreshed_at_unix_s);
            if age < config.lease_ttl.as_secs()
                && record.instance_id != config.instance_id
            {
                // Peer's lease is still fresh — don't steal.
                return Ok(None);
            }
            record.epoch.saturating_add(1)
        }
        None => 1,
    };
    write_lease_atomic(
        &config.lease_path,
        &LeaseRecord {
            instance_id: config.instance_id.clone(),
            refreshed_at_unix_s: now,
            epoch: next_epoch,
        },
    )?;
    // Read-back verification — protects against a racing peer writing
    // the same lease file in the gap between our write and our return.
    let final_record = read_lease_if_present(&config.lease_path)?;
    match final_record {
        Some(record) if record.instance_id == config.instance_id => Ok(Some(record.epoch)),
        _ => Ok(None),
    }
}

fn refresh_lease(config: &LeaseConfig, current_epoch: u64) -> Result<()> {
    ensure_parent_dir(&config.lease_path)?;
    let now = unix_seconds_now()?;
    // Refresh keeps the same epoch — only acquire (transition from
    // non-leader to leader) bumps it.
    write_lease_atomic(
        &config.lease_path,
        &LeaseRecord {
            instance_id: config.instance_id.clone(),
            refreshed_at_unix_s: now,
            epoch: current_epoch.max(1),
        },
    )?;
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create lease parent {}", parent.display()))?;
        }
    }
    Ok(())
}

fn read_lease_if_present(path: &Path) -> Result<Option<LeaseRecord>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut file = File::open(path).with_context(|| format!("open lease {}", path.display()))?;
    let mut body = String::new();
    file.read_to_string(&mut body)
        .with_context(|| format!("read lease {}", path.display()))?;
    if body.trim().is_empty() {
        return Ok(None);
    }
    let record: LeaseRecord = serde_json::from_str(&body)
        .with_context(|| format!("parse lease {}", path.display()))?;
    Ok(Some(record))
}

fn write_lease_atomic(path: &Path, record: &LeaseRecord) -> Result<()> {
    // Atomic write via tmpfile + rename. On Windows the rename will
    // fail if the destination exists — fall back to remove + rename.
    let body = serde_json::to_string(record)?;
    let mut tmp = path.to_path_buf();
    let suffix = format!("tmp.{}", std::process::id());
    tmp.set_extension(suffix);
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .with_context(|| format!("open tmp lease {}", tmp.display()))?;
        file.write_all(body.as_bytes())?;
        file.sync_all().ok();
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows: rename onto an existing file fails. Remove then
            // retry. Race window here is tolerated — the read-back
            // verification in `try_acquire` catches a peer that won
            // the same race.
            let _ = std::fs::remove_file(path);
            std::fs::rename(&tmp, path)
                .with_context(|| format!("rename lease {} -> {}", tmp.display(), path.display()))
        }
    }
}

fn unix_seconds_now() -> Result<u64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before unix epoch")?;
    Ok(now.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_lease_path(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wallet_ha_{tag}_{nanos}.lease"))
    }

    fn cfg(path: &Path, instance: &str) -> LeaseConfig {
        LeaseConfig {
            lease_path: path.to_path_buf(),
            instance_id: instance.to_string(),
            lease_ttl: Duration::from_secs(15),
            refresh_interval: Duration::from_secs(5),
            acquire_retry_interval: Duration::from_secs(5),
        }
    }

    #[test]
    fn acquire_on_empty_file_returns_epoch_1() {
        let p = tmp_lease_path("acq_empty");
        let result = try_acquire(&cfg(&p, "node-a")).unwrap();
        assert_eq!(result, Some(1));
        let record = read_lease_if_present(&p).unwrap().unwrap();
        assert_eq!(record.instance_id, "node-a");
        assert_eq!(record.epoch, 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn second_node_cannot_steal_fresh_lease() {
        let p = tmp_lease_path("steal_fresh");
        try_acquire(&cfg(&p, "node-a")).unwrap();
        let result = try_acquire(&cfg(&p, "node-b")).unwrap();
        assert_eq!(result, None, "node-b should NOT steal a fresh lease");
        let record = read_lease_if_present(&p).unwrap().unwrap();
        assert_eq!(record.instance_id, "node-a");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn second_node_acquires_when_lease_expired() {
        let p = tmp_lease_path("expired");
        // Pre-write a stale lease — refreshed_at far in the past.
        let stale = LeaseRecord {
            instance_id: "node-a".into(),
            refreshed_at_unix_s: 1,
            epoch: 7,
        };
        write_lease_atomic(&p, &stale).unwrap();
        let result = try_acquire(&cfg(&p, "node-b")).unwrap();
        assert_eq!(result, Some(8), "node-b takes over with epoch+1");
        let record = read_lease_if_present(&p).unwrap().unwrap();
        assert_eq!(record.instance_id, "node-b");
        assert_eq!(record.epoch, 8);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn refresh_keeps_same_epoch() {
        let p = tmp_lease_path("refresh");
        try_acquire(&cfg(&p, "node-a")).unwrap();
        let before = read_lease_if_present(&p).unwrap().unwrap();
        refresh_lease(&cfg(&p, "node-a"), before.epoch).unwrap();
        let after = read_lease_if_present(&p).unwrap().unwrap();
        assert_eq!(after.instance_id, "node-a");
        assert_eq!(after.epoch, before.epoch, "refresh must not bump epoch");
        assert!(after.refreshed_at_unix_s >= before.refreshed_at_unix_s);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn epoch_bumps_on_transition_only() {
        let p = tmp_lease_path("epoch_bump");
        // First acquire by node-a — epoch 1.
        let e1 = try_acquire(&cfg(&p, "node-a")).unwrap().unwrap();
        // Force expire by writing stale record.
        let mut record = read_lease_if_present(&p).unwrap().unwrap();
        record.refreshed_at_unix_s = 1;
        write_lease_atomic(&p, &record).unwrap();
        // Node-b acquires — epoch 2.
        let e2 = try_acquire(&cfg(&p, "node-b")).unwrap().unwrap();
        assert_eq!(e2, e1 + 1, "acquire transition bumps epoch");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn instance_id_collision_returns_same_lease_to_owner() {
        // Same instance_id re-acquiring shouldn't be blocked by its own
        // fresh lease — useful for a process that restarts within the
        // TTL and wants to resume work without waiting.
        let p = tmp_lease_path("self_reacquire");
        try_acquire(&cfg(&p, "node-a")).unwrap();
        let result = try_acquire(&cfg(&p, "node-a")).unwrap();
        assert!(
            result.is_some(),
            "same instance_id should re-acquire (bumps epoch)"
        );
        let _ = std::fs::remove_file(&p);
    }
}
