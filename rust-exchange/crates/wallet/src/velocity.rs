//! Withdrawal velocity tracker — per-customer-per-chain rolling caps.
//!
//! Step 8 of the wallet implementation track. The `VelocityTracker`
//! aggregates outstanding + recently-settled withdrawal amounts in a
//! sliding 24h window per `(user_id, chain)` so the validate-time
//! check can refuse a new withdrawal that would breach the daily
//! cap. Per design §5.3 v1 thresholds:
//!
//!   amount_per_24h_per_user     -> sliding window cap (default $500k
//!                                  USD-equivalent per design; this
//!                                  module is unit-agnostic and works
//!                                  in the smallest indivisible chain
//!                                  unit)
//!
//! Implementation notes:
//! - Pure in-memory; rebuilt at startup by replaying all non-Rejected
//!   withdrawals from the WithdrawalStore. Persistence is implicit
//!   via the underlying store's WAL.
//! - Sliding-window expiration is lazy: the tracker drops entries
//!   older than `window` on every read/write. This keeps the data
//!   structure bounded without a sweeping background task.
//! - The unit (wei / satoshi / lamport) is the caller's
//!   responsibility; the tracker holds amounts as i128 with no
//!   per-chain conversion. USD-equivalent caps must be applied at
//!   the api layer using a price oracle.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;

use crate::types::ChainId;

/// One outflow contribution observed within the rolling window.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VelocityEntry {
    at: DateTime<Utc>,
    amount: i128,
}

#[derive(Debug, Clone)]
struct CustomerChainBucket {
    /// In submission order (oldest at front). Newer ones get
    /// pushed_back; expired ones get pop_front.
    entries: VecDeque<VelocityEntry>,
    /// Cached sum so reads don't iterate. Invariant: equals the sum
    /// of all `entries[i].amount`.
    total: i128,
}

impl CustomerChainBucket {
    fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            total: 0,
        }
    }

    fn evict_older_than(&mut self, cutoff: DateTime<Utc>) {
        while self.entries.front().is_some_and(|e| e.at < cutoff) {
            let evicted = self.entries.pop_front().unwrap();
            self.total = self.total.saturating_sub(evicted.amount);
        }
    }

    fn insert(&mut self, entry: VelocityEntry) {
        self.total = self.total.saturating_add(entry.amount);
        self.entries.push_back(entry);
    }
}

/// Rolling-window withdrawal velocity tracker. Keyed on
/// `(user_id, chain)`. Thread-safe via a single Mutex; expected use
/// is a low-rate validate-time check, so contention is not the
/// primary concern.
pub struct VelocityTracker {
    window: Duration,
    state: Mutex<HashMap<(String, ChainId), CustomerChainBucket>>,
}

impl VelocityTracker {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Default 24h sliding window per design §5.3.
    pub fn with_default_window() -> Self {
        Self::new(Duration::from_secs(24 * 60 * 60))
    }

    /// Record a withdrawal contribution. Caller is responsible for
    /// recording exactly once per withdrawal (e.g. at submit-time).
    /// Settled withdrawals stay in the window for 24h after their
    /// `submitted_at`; this is conservative — the alternative is to
    /// only count outstanding (non-Settled) flows, which would let
    /// an attacker burst-and-pause to evade the cap.
    pub fn record(&self, user_id: &str, chain: ChainId, amount: i128, at: DateTime<Utc>) {
        let mut state = self.state.lock();
        let bucket = state
            .entry((user_id.to_string(), chain))
            .or_insert_with(CustomerChainBucket::new);
        let cutoff = at - chrono::Duration::from_std(self.window).unwrap_or_default();
        bucket.evict_older_than(cutoff);
        bucket.insert(VelocityEntry { at, amount });
    }

    /// Return the current rolling-window total for `(user_id, chain)`
    /// at wall-clock time `now`. Always evicts expired entries before
    /// reading.
    pub fn total(&self, user_id: &str, chain: ChainId, now: DateTime<Utc>) -> i128 {
        let mut state = self.state.lock();
        let key = (user_id.to_string(), chain);
        let Some(bucket) = state.get_mut(&key) else {
            return 0;
        };
        let cutoff = now - chrono::Duration::from_std(self.window).unwrap_or_default();
        bucket.evict_older_than(cutoff);
        bucket.total
    }

    /// Convenience: would `prospective_amount` push the user's chain
    /// total over `cap`? Read-only; for write-then-check use
    /// `try_record` to avoid the TOCTOU race between two concurrent
    /// submissions.
    pub fn would_exceed(
        &self,
        user_id: &str,
        chain: ChainId,
        prospective_amount: i128,
        cap: i128,
        now: DateTime<Utc>,
    ) -> bool {
        let current = self.total(user_id, chain, now);
        current.saturating_add(prospective_amount) > cap
    }

    /// Atomic check-and-record under a single mutex acquisition. If the
    /// prospective amount would push the rolling total above `cap`,
    /// returns `Err(current_total)` and does NOT record. Otherwise
    /// inserts the entry and returns Ok.
    ///
    /// Use this in any handler path where two concurrent submissions
    /// could race past `would_exceed` — the cap would silently breach
    /// because both would observe pre-insert state.
    pub fn try_record(
        &self,
        user_id: &str,
        chain: ChainId,
        amount: i128,
        cap: i128,
        at: DateTime<Utc>,
    ) -> Result<(), i128> {
        let mut state = self.state.lock();
        let bucket = state
            .entry((user_id.to_string(), chain))
            .or_insert_with(CustomerChainBucket::new);
        let cutoff = at - chrono::Duration::from_std(self.window).unwrap_or_default();
        bucket.evict_older_than(cutoff);
        if bucket.total.saturating_add(amount) > cap {
            return Err(bucket.total);
        }
        bucket.insert(VelocityEntry { at, amount });
        Ok(())
    }

    /// Number of distinct (user, chain) buckets currently tracked.
    /// For metrics / debug only.
    pub fn bucket_count(&self) -> usize {
        self.state.lock().len()
    }
}

/// Build a tracker from a slice of historical withdrawal records by
/// replaying their `submitted_at` + `amount` into the tracker. Used
/// at startup to rebuild state from the WithdrawalStore's WAL.
/// `predicate` lets the caller decide which records to count
/// (typically: include everything except Rejected).
pub fn from_records<'a, I, F>(window: Duration, records: I, predicate: F) -> Arc<VelocityTracker>
where
    I: IntoIterator<Item = &'a crate::types::WithdrawalRecord>,
    F: Fn(&crate::types::WithdrawalRecord) -> bool,
{
    let tracker = Arc::new(VelocityTracker::new(window));
    for r in records {
        if predicate(r) {
            tracker.record(&r.user_id, r.chain, r.amount, r.submitted_at);
        }
    }
    tracker
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{WithdrawalRecord, WithdrawalStatus, WALLET_SCHEMA_VERSION};

    fn ts(secs: i64) -> DateTime<Utc> {
        chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000 + secs, 0).unwrap()
    }

    fn record(id: &str, user: &str, amount: i128, at_secs: i64) -> WithdrawalRecord {
        WithdrawalRecord {
            schema_version: WALLET_SCHEMA_VERSION,
            withdrawal_id: id.into(),
            user_id: user.into(),
            chain: ChainId::Eth,
            address_id: "addr-1".into(),
            destination_address: "0xdest".into(),
            amount,
            estimated_fee: 1_i128,
            actual_fee: None,
            status: WithdrawalStatus::Submitted,
            submitted_at: ts(at_secs),
            updated_at: ts(at_secs),
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
    fn empty_tracker_returns_zero() {
        let t = VelocityTracker::with_default_window();
        assert_eq!(t.total("alice", ChainId::Eth, ts(100)), 0);
        assert_eq!(t.bucket_count(), 0);
    }

    #[test]
    fn records_inside_window_accumulate() {
        let t = VelocityTracker::new(Duration::from_secs(3600)); // 1h window
        t.record("alice", ChainId::Eth, 100, ts(0));
        t.record("alice", ChainId::Eth, 200, ts(1000));
        t.record("alice", ChainId::Eth, 300, ts(3500));
        // All three within 1h of ts(3600).
        assert_eq!(t.total("alice", ChainId::Eth, ts(3600)), 600);
    }

    #[test]
    fn records_outside_window_are_evicted() {
        let t = VelocityTracker::new(Duration::from_secs(3600));
        t.record("alice", ChainId::Eth, 100, ts(0));
        t.record("alice", ChainId::Eth, 200, ts(7200)); // 2h after first
        // At ts(7200), the first entry is 7200s old — outside the 1h
        // window. Only the 200 should remain.
        assert_eq!(t.total("alice", ChainId::Eth, ts(7200)), 200);
    }

    #[test]
    fn keys_are_per_user_per_chain() {
        let t = VelocityTracker::with_default_window();
        t.record("alice", ChainId::Eth, 100, ts(0));
        t.record("bob", ChainId::Eth, 200, ts(0));
        t.record("alice", ChainId::Btc, 300, ts(0));
        assert_eq!(t.total("alice", ChainId::Eth, ts(100)), 100);
        assert_eq!(t.total("bob", ChainId::Eth, ts(100)), 200);
        assert_eq!(t.total("alice", ChainId::Btc, ts(100)), 300);
        assert_eq!(t.total("alice", ChainId::Sol, ts(100)), 0);
        assert_eq!(t.bucket_count(), 3);
    }

    #[test]
    fn would_exceed_detects_breach() {
        let t = VelocityTracker::new(Duration::from_secs(3600));
        t.record("alice", ChainId::Eth, 800, ts(0));
        // Cap = 1000. Existing total = 800.
        assert!(!t.would_exceed("alice", ChainId::Eth, 100, 1000, ts(0))); // 900 <= 1000
        assert!(!t.would_exceed("alice", ChainId::Eth, 200, 1000, ts(0))); // 1000 == 1000
        assert!(t.would_exceed("alice", ChainId::Eth, 201, 1000, ts(0))); // 1001 > 1000
    }

    #[test]
    fn from_records_skips_rejected_when_predicate_filters_them() {
        let mut a = record("wd-a", "alice", 100, 0);
        a.status = WithdrawalStatus::Settled;
        let mut b = record("wd-b", "alice", 200, 100);
        b.status = WithdrawalStatus::Rejected;
        b.rejection_reason = Some(crate::types::WithdrawalRejectReason::SanctionsHit);
        let c = record("wd-c", "alice", 300, 200);
        let recs = vec![a, b, c];
        let tracker = from_records(Duration::from_secs(3600), recs.iter(), |r| {
            r.status != WithdrawalStatus::Rejected
        });
        // Only wd-a (100) and wd-c (300) should count; wd-b is rejected.
        assert_eq!(tracker.total("alice", ChainId::Eth, ts(300)), 400);
    }

    #[test]
    fn try_record_atomic_check_and_insert_under_cap() {
        let t = VelocityTracker::new(Duration::from_secs(3600));
        assert!(t.try_record("alice", ChainId::Eth, 400, 1000, ts(0)).is_ok());
        assert!(t.try_record("alice", ChainId::Eth, 600, 1000, ts(0)).is_ok());
        assert_eq!(t.total("alice", ChainId::Eth, ts(0)), 1000);
    }

    #[test]
    fn try_record_rejects_breach_and_does_not_insert() {
        let t = VelocityTracker::new(Duration::from_secs(3600));
        t.try_record("alice", ChainId::Eth, 800, 1000, ts(0)).unwrap();
        let err = t
            .try_record("alice", ChainId::Eth, 300, 1000, ts(0))
            .unwrap_err();
        // Returned current total = 800 (pre-insert).
        assert_eq!(err, 800);
        // Not inserted.
        assert_eq!(t.total("alice", ChainId::Eth, ts(0)), 800);
    }

    #[test]
    fn try_record_concurrent_submissions_cannot_breach_cap() {
        use std::sync::Arc;
        use std::thread;
        let t = Arc::new(VelocityTracker::new(Duration::from_secs(3600)));
        // Cap = 1000. Twenty threads each try to record 100. Only ten
        // should win; the rest must be rejected. With check-then-act
        // (would_exceed + record) this would race past 1000.
        let mut handles = vec![];
        for _ in 0..20 {
            let t = t.clone();
            handles.push(thread::spawn(move || {
                t.try_record("alice", ChainId::Eth, 100, 1000, ts(0))
            }));
        }
        let oks: usize = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|r| r.is_ok())
            .count();
        assert_eq!(oks, 10);
        assert_eq!(t.total("alice", ChainId::Eth, ts(0)), 1000);
    }

    #[test]
    fn saturating_add_prevents_overflow_panic() {
        let t = VelocityTracker::with_default_window();
        t.record("alice", ChainId::Eth, i128::MAX / 2, ts(0));
        // Adding anything large enough to overflow saturates rather
        // than panicking.
        let _ = t.would_exceed("alice", ChainId::Eth, i128::MAX / 2, i128::MAX, ts(100));
    }
}
