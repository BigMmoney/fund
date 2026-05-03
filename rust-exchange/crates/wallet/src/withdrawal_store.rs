//! Withdrawal store + state-machine validator.
//!
//! Step 7B (continued) of the wallet implementation track. JSONL-backed,
//! in-memory cached, append-only. Latest record per `withdrawal_id`
//! wins on recovery. Companion to `AddressBookStore`.
//!
//! State transitions are validated by `is_valid_transition` so a
//! reckless caller can't push a Settled record back to Submitted.
//! Terminal states (Settled / Rejected) accept no further updates.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use parking_lot::Mutex;

use persistence::{JsonlFileWal, WalStore};

use crate::types::{WithdrawalRecord, WithdrawalStatus};

const WITHDRAWAL_LOCK_SHARDS: usize = 32;

fn lock_shard(key: &str) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % WITHDRAWAL_LOCK_SHARDS
}

/// Returns true when `next` is a valid transition from `current`.
/// Encodes the design §5.1 lifecycle. Refer to the diagram there.
pub fn is_valid_transition(current: WithdrawalStatus, next: WithdrawalStatus) -> bool {
    use WithdrawalStatus::*;
    if current == next {
        return true;
    }
    match (current, next) {
        // Submit path.
        (Submitted, Validated) => true,
        (Submitted, Rejected) => true,
        (Validated, Queued) => true,
        (Validated, Rejected) => true,
        // Queue path.
        (Queued, AwaitingApproval) => true,
        (Queued, Approved) => true,
        (Queued, Rejected) => true,
        // Approval path.
        (AwaitingApproval, Approved) => true,
        (AwaitingApproval, Rejected) => true,
        // Sign / broadcast path.
        (Approved, Signing) => true,
        (Approved, Rejected) => true,
        (Signing, Broadcast) => true,
        (Signing, Rejected) => true,
        // Confirm path. A confirmed tx can be rolled back to
        // Broadcast by a deeper-than-expected reorg.
        (Broadcast, Confirmed) => true,
        (Broadcast, Rejected) => true,
        (Confirmed, Settled) => true,
        (Confirmed, Broadcast) => true,
        (Confirmed, Rejected) => true,
        // Settled and terminal Rejected are sinks.
        _ => false,
    }
}

pub struct WithdrawalStore {
    by_id: DashMap<String, WithdrawalRecord>,
    store: Arc<dyn WalStore<WithdrawalRecord>>,
    write_locks: Vec<Mutex<()>>,
}

impl WithdrawalStore {
    pub fn new(store: Arc<dyn WalStore<WithdrawalRecord>>) -> anyhow::Result<Self> {
        let result = Self {
            by_id: DashMap::new(),
            store,
            write_locks: (0..WITHDRAWAL_LOCK_SHARDS).map(|_| Mutex::new(())).collect(),
        };
        for entry in result.store.entries()? {
            result.by_id.insert(entry.withdrawal_id.clone(), entry);
        }
        Ok(result)
    }

    pub fn open_jsonl(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn WalStore<WithdrawalRecord>> = Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub fn get(&self, withdrawal_id: &str) -> Option<WithdrawalRecord> {
        self.by_id.get(withdrawal_id).map(|e| e.value().clone())
    }

    /// All withdrawals owned by `user_id`, sorted by `submitted_at` desc.
    pub fn for_user(&self, user_id: &str) -> Vec<WithdrawalRecord> {
        let mut out: Vec<WithdrawalRecord> = self
            .by_id
            .iter()
            .filter(|e| e.value().user_id == user_id)
            .map(|e| e.value().clone())
            .collect();
        out.sort_by(|a, b| b.submitted_at.cmp(&a.submitted_at));
        out
    }

    /// All withdrawals currently at `target` status, oldest first by
    /// `submitted_at`. Used by the hot-wallet worker to scan for
    /// `Approved` and `Broadcast` records to drive forward.
    pub fn by_status(&self, target: WithdrawalStatus) -> Vec<WithdrawalRecord> {
        let mut out: Vec<WithdrawalRecord> = self
            .by_id
            .iter()
            .filter(|e| e.value().status == target)
            .map(|e| e.value().clone())
            .collect();
        out.sort_by(|a, b| a.submitted_at.cmp(&b.submitted_at));
        out
    }

    /// All withdrawals currently in the queue layer (Queued or
    /// AwaitingApproval), oldest first — the natural order operators
    /// process them in.
    pub fn pending(&self) -> Vec<WithdrawalRecord> {
        let mut out: Vec<WithdrawalRecord> = self
            .by_id
            .iter()
            .filter(|e| {
                matches!(
                    e.value().status,
                    WithdrawalStatus::Queued | WithdrawalStatus::AwaitingApproval
                )
            })
            .map(|e| e.value().clone())
            .collect();
        out.sort_by(|a, b| a.submitted_at.cmp(&b.submitted_at));
        out
    }

    pub fn count(&self) -> usize {
        self.by_id.len()
    }

    /// Insert a brand-new withdrawal record. Initial status MUST be
    /// `Submitted` (created from the customer-facing /withdraw
    /// handler). Returns `Err` if the id is already in use OR if the
    /// initial status is not `Submitted`.
    pub fn create(&self, record: WithdrawalRecord) -> anyhow::Result<()> {
        if record.status != WithdrawalStatus::Submitted {
            anyhow::bail!(
                "withdrawal create requires status=Submitted, got {:?}",
                record.status
            );
        }
        let key = record.withdrawal_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if self.by_id.contains_key(&key) {
            anyhow::bail!("withdrawal already exists: {key}");
        }
        self.store.append(&record)?;
        self.by_id.insert(key, record);
        Ok(())
    }

    /// Append an updated record. The new status MUST be a valid
    /// transition from the current one (per `is_valid_transition`)
    /// otherwise the call returns `Err` and nothing is written.
    pub fn update(&self, mut record: WithdrawalRecord) -> anyhow::Result<()> {
        let key = record.withdrawal_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        let current = self
            .by_id
            .get(&key)
            .map(|e| e.value().status)
            .ok_or_else(|| anyhow::anyhow!("withdrawal not found: {key}"))?;
        if !is_valid_transition(current, record.status) {
            anyhow::bail!(
                "invalid withdrawal transition for {key}: {:?} -> {:?}",
                current,
                record.status
            );
        }
        record.updated_at = Utc::now();
        self.store.append(&record)?;
        self.by_id.insert(key, record);
        Ok(())
    }

    /// Convenience: advance status only. All other fields preserved.
    /// Returns the post-update record.
    pub fn advance_status(
        &self,
        withdrawal_id: &str,
        next: WithdrawalStatus,
    ) -> anyhow::Result<WithdrawalRecord> {
        let mut current = self
            .get(withdrawal_id)
            .ok_or_else(|| anyhow::anyhow!("withdrawal not found: {withdrawal_id}"))?;
        current.status = next;
        self.update(current.clone())?;
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChainId, WithdrawalRejectReason, WALLET_SCHEMA_VERSION};
    use persistence::InMemoryWal;

    fn ts() -> chrono::DateTime<Utc> {
        chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000, 0).unwrap()
    }

    fn make_record(id: &str, user: &str) -> WithdrawalRecord {
        WithdrawalRecord {
            schema_version: WALLET_SCHEMA_VERSION,
            withdrawal_id: id.into(),
            user_id: user.into(),
            chain: ChainId::Eth,
            address_id: "addr-1".into(),
            destination_address: "0xabc".into(),
            amount: 1_000_000_000_000_000_000_i128,
            estimated_fee: 1_000_000_i128,
            actual_fee: None,
            status: WithdrawalStatus::Submitted,
            submitted_at: ts(),
            updated_at: ts(),
            approved_at: None,
            broadcast_at: None,
            confirmed_at: None,
            settled_at: None,
            tx_hash: None,
            confirmations: 0,
            confirmations_required: 25,
            approval_request_id: None,
            rejection_reason: None,
            notes: None,
        }
    }

    #[test]
    fn create_then_get_smoke() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-1", "alice")).unwrap();
        assert_eq!(s.get("wd-1").unwrap().user_id, "alice");
    }

    #[test]
    fn create_rejects_non_submitted_initial_status() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        let mut r = make_record("wd-2", "alice");
        r.status = WithdrawalStatus::Queued;
        let err = s.create(r).unwrap_err();
        assert!(err.to_string().contains("status=Submitted"));
    }

    #[test]
    fn create_duplicate_rejected() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-d", "alice")).unwrap();
        let err = s.create(make_record("wd-d", "alice")).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn advance_status_walks_happy_path() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-1", "alice")).unwrap();
        s.advance_status("wd-1", WithdrawalStatus::Validated).unwrap();
        s.advance_status("wd-1", WithdrawalStatus::Queued).unwrap();
        s.advance_status("wd-1", WithdrawalStatus::Approved).unwrap();
        s.advance_status("wd-1", WithdrawalStatus::Signing).unwrap();
        s.advance_status("wd-1", WithdrawalStatus::Broadcast).unwrap();
        s.advance_status("wd-1", WithdrawalStatus::Confirmed).unwrap();
        s.advance_status("wd-1", WithdrawalStatus::Settled).unwrap();
        assert_eq!(s.get("wd-1").unwrap().status, WithdrawalStatus::Settled);
    }

    #[test]
    fn invalid_transition_rejected() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-x", "alice")).unwrap();
        // Submitted -> Settled is not a legal transition.
        let err = s.advance_status("wd-x", WithdrawalStatus::Settled).unwrap_err();
        assert!(err.to_string().contains("invalid withdrawal transition"));
        // The store state is unchanged.
        assert_eq!(s.get("wd-x").unwrap().status, WithdrawalStatus::Submitted);
    }

    #[test]
    fn settled_is_a_sink() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-s", "alice")).unwrap();
        for next in [
            WithdrawalStatus::Validated,
            WithdrawalStatus::Queued,
            WithdrawalStatus::Approved,
            WithdrawalStatus::Signing,
            WithdrawalStatus::Broadcast,
            WithdrawalStatus::Confirmed,
            WithdrawalStatus::Settled,
        ] {
            s.advance_status("wd-s", next).unwrap();
        }
        // Try to push out of Settled — must fail.
        let err = s.advance_status("wd-s", WithdrawalStatus::Broadcast).unwrap_err();
        assert!(err.to_string().contains("invalid withdrawal transition"));
    }

    #[test]
    fn rejected_is_a_sink() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-r", "alice")).unwrap();
        s.advance_status("wd-r", WithdrawalStatus::Rejected).unwrap();
        let err = s.advance_status("wd-r", WithdrawalStatus::Validated).unwrap_err();
        assert!(err.to_string().contains("invalid withdrawal transition"));
    }

    #[test]
    fn confirmed_can_revert_to_broadcast_for_reorg() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-reorg", "alice")).unwrap();
        for next in [
            WithdrawalStatus::Validated,
            WithdrawalStatus::Queued,
            WithdrawalStatus::Approved,
            WithdrawalStatus::Signing,
            WithdrawalStatus::Broadcast,
            WithdrawalStatus::Confirmed,
        ] {
            s.advance_status("wd-reorg", next).unwrap();
        }
        // Reorg path: Confirmed -> Broadcast.
        s.advance_status("wd-reorg", WithdrawalStatus::Broadcast).unwrap();
        // Then back to Confirmed once the new head re-confirms.
        s.advance_status("wd-reorg", WithdrawalStatus::Confirmed).unwrap();
    }

    #[test]
    fn pending_includes_only_queued_and_awaiting_approval() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-q", "alice")).unwrap();
        s.advance_status("wd-q", WithdrawalStatus::Validated).unwrap();
        s.advance_status("wd-q", WithdrawalStatus::Queued).unwrap();

        s.create(make_record("wd-a", "alice")).unwrap();
        s.advance_status("wd-a", WithdrawalStatus::Validated).unwrap();
        s.advance_status("wd-a", WithdrawalStatus::Queued).unwrap();
        s.advance_status("wd-a", WithdrawalStatus::AwaitingApproval).unwrap();

        s.create(make_record("wd-done", "alice")).unwrap();
        for next in [
            WithdrawalStatus::Validated,
            WithdrawalStatus::Queued,
            WithdrawalStatus::Approved,
            WithdrawalStatus::Signing,
            WithdrawalStatus::Broadcast,
            WithdrawalStatus::Confirmed,
            WithdrawalStatus::Settled,
        ] {
            s.advance_status("wd-done", next).unwrap();
        }

        let pending: Vec<String> = s
            .pending()
            .into_iter()
            .map(|r| r.withdrawal_id)
            .collect();
        assert_eq!(pending.len(), 2);
        assert!(pending.contains(&"wd-q".to_string()));
        assert!(pending.contains(&"wd-a".to_string()));
        assert!(!pending.contains(&"wd-done".to_string()));
    }

    #[test]
    fn for_user_filters_and_sorts_newest_first() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        let mut a = make_record("wd-a", "alice");
        a.submitted_at = chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000, 0).unwrap();
        let mut b = make_record("wd-b", "alice");
        b.submitted_at = chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_100, 0).unwrap();
        let c = make_record("wd-c", "bob");
        s.create(a).unwrap();
        s.create(b).unwrap();
        s.create(c).unwrap();
        let listed = s.for_user("alice");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].withdrawal_id, "wd-b");
        assert_eq!(listed[1].withdrawal_id, "wd-a");
    }

    #[test]
    fn recovery_replays_latest_status() {
        let wal: Arc<dyn WalStore<WithdrawalRecord>> = Arc::new(InMemoryWal::new());
        {
            let s = WithdrawalStore::new(wal.clone()).unwrap();
            s.create(make_record("wd-1", "alice")).unwrap();
            s.advance_status("wd-1", WithdrawalStatus::Validated).unwrap();
            s.advance_status("wd-1", WithdrawalStatus::Queued).unwrap();
        }
        let s2 = WithdrawalStore::new(wal).unwrap();
        assert_eq!(s2.get("wd-1").unwrap().status, WithdrawalStatus::Queued);
    }

    #[test]
    fn rejection_reason_round_trips_via_full_record_update() {
        let s = WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_record("wd-r", "alice")).unwrap();
        let mut r = s.get("wd-r").unwrap();
        r.status = WithdrawalStatus::Rejected;
        r.rejection_reason = Some(WithdrawalRejectReason::SanctionsHit);
        r.notes = Some("SDN match at validate-time recheck".into());
        s.update(r.clone()).unwrap();
        let back = s.get("wd-r").unwrap();
        assert_eq!(back.status, WithdrawalStatus::Rejected);
        assert_eq!(back.rejection_reason, Some(WithdrawalRejectReason::SanctionsHit));
    }
}
