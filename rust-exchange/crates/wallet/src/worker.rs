//! Hot-wallet worker — drives Approved withdrawals through
//! Signing -> Broadcast -> Confirmed.
//!
//! Step 8 of the wallet implementation track. v1 ships this as an
//! in-process worker; the design's separate-process hot-wallet
//! daemon (with mTLS gRPC, key custody isolation) is v1.1.
//!
//! The worker is split from the rest of the pipeline:
//! - The api submit-time path validates + creates the WithdrawalRecord
//!   in `Submitted` state.
//! - The validate-time + queue-time logic walks it to `Approved`.
//! - This worker takes over at `Approved` and pushes through
//!   `Signing` → `Broadcast` → `Confirmed`.
//! - Settlement (ledger debit + flip to `Settled`) is the api's
//!   responsibility; this worker stops at `Confirmed`.
//!
//! Drive model: `tick()` does one pass. Production wiring in main.rs
//! spawns a tokio interval task that calls `tick` every N seconds.
//! Tests call `tick` directly for determinism.

use std::sync::Arc;

use chrono::Utc;

use crate::chain::ChainAdapter;
use crate::types::{ChainId, WithdrawalStatus};
use crate::withdrawal_store::WithdrawalStore;

/// Per-pass summary returned by `tick`. Useful for metrics + tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerTickReport {
    pub signed_count: usize,
    pub broadcast_count: usize,
    pub confirmed_count: usize,
    pub failed_count: usize,
}

pub struct HotWalletWorker {
    chain: ChainId,
    adapter: Arc<dyn ChainAdapter>,
    withdrawals: Arc<WithdrawalStore>,
    /// On-chain hot-wallet address — the `from` for every signing
    /// operation on this chain.
    hot_address: String,
}

impl HotWalletWorker {
    pub fn new(
        chain: ChainId,
        adapter: Arc<dyn ChainAdapter>,
        withdrawals: Arc<WithdrawalStore>,
        hot_address: impl Into<String>,
    ) -> Self {
        Self {
            chain,
            adapter,
            withdrawals,
            hot_address: hot_address.into(),
        }
    }

    /// One pass over Approved + Broadcast withdrawals on this worker's
    /// chain. Returns counts for metrics.
    ///
    /// Algorithm:
    ///   1. For each `Approved` withdrawal on this chain:
    ///      - flip to `Signing`
    ///      - build + sign + broadcast via the adapter
    ///      - flip to `Broadcast` with the resulting tx_hash
    ///      - on adapter error: flip to `Rejected` with
    ///        `WithdrawalRejectReason::BroadcastFailed`.
    ///   2. For each `Broadcast` withdrawal on this chain:
    ///      - poll confirmations
    ///      - if confs >= confirmations_required: flip to `Confirmed`
    ///        and stamp `confirmed_at`.
    ///
    /// Caller-side: settlement (ledger debit + flip to `Settled`)
    /// happens in a separate api-side task that reads `Confirmed`
    /// records and commits the customer-balance debit.
    pub fn tick(&self) -> WorkerTickReport {
        let mut report = WorkerTickReport::default();
        let now = Utc::now();

        // Take a snapshot of the candidates so we don't hold any
        // store-internal locks during the (potentially slow) adapter
        // calls below.
        let approved: Vec<_> = self
            .withdrawals
            .pending() // (Queued + AwaitingApproval, by store contract)
            .into_iter()
            .filter(|_| false)
            .collect();
        // pending() returns Queued + AwaitingApproval, not Approved.
        // We need Approved; iterate the store directly. Build a
        // narrower scan by snapshotting the store's by_id. For
        // simplicity at v1 we surface a typed scan via filter on the
        // public for_user list; in a follow-up the WithdrawalStore
        // gains a generic `by_status` query. For now, scan via the
        // pending+broadcast lists below which call into get() per id
        // anyway.
        drop(approved);

        // Approved → Broadcast.
        let approved_records = self.snapshot_by_status(WithdrawalStatus::Approved);
        for mut record in approved_records {
            if record.chain != self.chain {
                continue;
            }
            // Advance to Signing.
            if self
                .withdrawals
                .advance_status(&record.withdrawal_id, WithdrawalStatus::Signing)
                .is_err()
            {
                report.failed_count += 1;
                continue;
            }
            record.status = WithdrawalStatus::Signing;
            report.signed_count += 1;

            // Build + sign + broadcast.
            let unsigned = match self.adapter.build_withdrawal(
                &self.hot_address,
                &record.destination_address,
                record.amount,
                record.estimated_fee,
            ) {
                Ok(u) => u,
                Err(e) => {
                    self.fail_withdrawal(&record.withdrawal_id, &format!("build failed: {e}"));
                    report.failed_count += 1;
                    continue;
                }
            };
            let hash = match self.adapter.sign_and_broadcast(&unsigned) {
                Ok(h) => h,
                Err(e) => {
                    self.fail_withdrawal(
                        &record.withdrawal_id,
                        &format!("broadcast failed: {e}"),
                    );
                    report.failed_count += 1;
                    continue;
                }
            };
            // Update record fields and append.
            let mut updated = match self.withdrawals.get(&record.withdrawal_id) {
                Some(r) => r,
                None => {
                    report.failed_count += 1;
                    continue;
                }
            };
            updated.status = WithdrawalStatus::Broadcast;
            updated.tx_hash = Some(hash);
            updated.broadcast_at = Some(now);
            if self.withdrawals.update(updated).is_err() {
                report.failed_count += 1;
                continue;
            }
            report.broadcast_count += 1;
        }

        // Broadcast → Confirmed.
        let broadcast_records = self.snapshot_by_status(WithdrawalStatus::Broadcast);
        for mut record in broadcast_records {
            if record.chain != self.chain {
                continue;
            }
            let Some(tx_hash) = record.tx_hash.clone() else {
                continue;
            };
            let confirmations = match self.adapter.confirmations(&tx_hash) {
                Ok(c) => c,
                Err(_) => continue,
            };
            record.confirmations = confirmations;
            if confirmations >= record.confirmations_required {
                record.status = WithdrawalStatus::Confirmed;
                record.confirmed_at = Some(now);
            }
            // Update the record so confirmations counter is observable
            // even when not yet at the threshold.
            let _ = self.withdrawals.update(record.clone());
            if record.status == WithdrawalStatus::Confirmed {
                report.confirmed_count += 1;
            }
        }
        report
    }

    /// Helper: snapshot records by status. Replaces the missing
    /// `WithdrawalStore::by_status` (deferred to a follow-up commit)
    /// with a `for_user`-style scan over a known set of users would
    /// require iteration; instead we rely on `pending()` + a private
    /// scan path. For v1 simplicity this scans the store's WAL via
    /// public APIs.
    fn snapshot_by_status(&self, target: WithdrawalStatus) -> Vec<crate::types::WithdrawalRecord> {
        // Walk all records via for_user("") which the store doesn't
        // support; instead use a marker iteration. For v1 we expose
        // a `by_status` helper here that goes through the public
        // get/pending APIs combined with a follow-up store method.
        // Pragmatic shortcut: cycle through users we've seen via
        // pending(). For the smoke test where there are at most a
        // handful of users this is fine; production wires a proper
        // store-level filter in a follow-up.
        // Instead, snapshot via a fresh full-scan helper exposed by
        // the store. This commit also adds `by_status` to
        // WithdrawalStore for that purpose; see withdrawal_store.rs.
        self.withdrawals.by_status(target)
    }

    fn fail_withdrawal(&self, id: &str, note: &str) {
        if let Some(mut current) = self.withdrawals.get(id) {
            current.status = WithdrawalStatus::Rejected;
            current.rejection_reason = Some(crate::types::WithdrawalRejectReason::BroadcastFailed);
            current.notes = Some(note.to_string());
            let _ = self.withdrawals.update(current);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::InMemoryChainAdapter;
    use crate::types::{WithdrawalRecord, WithdrawalStatus, WALLET_SCHEMA_VERSION};
    use chrono::Utc;
    use persistence::InMemoryWal;

    const HOT: &str = "0xhot";
    const DEST: &str = "0xdest";

    fn make_record(id: &str, amount: i128, fee: i128, confirmations_required: u32) -> WithdrawalRecord {
        WithdrawalRecord {
            schema_version: WALLET_SCHEMA_VERSION,
            withdrawal_id: id.into(),
            user_id: "alice".into(),
            chain: ChainId::Eth,
            address_id: "addr-1".into(),
            destination_address: DEST.into(),
            amount,
            estimated_fee: fee,
            actual_fee: None,
            status: WithdrawalStatus::Submitted,
            submitted_at: Utc::now(),
            updated_at: Utc::now(),
            approved_at: None,
            broadcast_at: None,
            confirmed_at: None,
            settled_at: None,
            tx_hash: None,
            confirmations: 0,
            confirmations_required,
            approval_request_id: None,
            rejection_reason: None,
            notes: None,
        }
    }

    fn approved(store: &WithdrawalStore, id: &str, amount: i128, fee: i128, confs: u32) {
        store.create(make_record(id, amount, fee, confs)).unwrap();
        for next in [
            WithdrawalStatus::Validated,
            WithdrawalStatus::Queued,
            WithdrawalStatus::Approved,
        ] {
            store.advance_status(id, next).unwrap();
        }
    }

    fn make_pieces() -> (Arc<InMemoryChainAdapter>, Arc<WithdrawalStore>, HotWalletWorker) {
        let adapter = Arc::new(InMemoryChainAdapter::new(ChainId::Eth));
        adapter.seed_balance(HOT, 10_000_000_i128);
        let store = Arc::new(WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let worker = HotWalletWorker::new(ChainId::Eth, adapter.clone(), store.clone(), HOT);
        (adapter, store, worker)
    }

    #[test]
    fn tick_advances_approved_to_broadcast_and_records_tx_hash() {
        let (_, store, worker) = make_pieces();
        approved(&store, "wd-1", 100, 10, 25);
        let report = worker.tick();
        assert_eq!(report.broadcast_count, 1);
        let r = store.get("wd-1").unwrap();
        assert_eq!(r.status, WithdrawalStatus::Broadcast);
        assert!(r.tx_hash.is_some());
        assert!(r.broadcast_at.is_some());
    }

    #[test]
    fn tick_advances_broadcast_to_confirmed_at_threshold() {
        let (adapter, store, worker) = make_pieces();
        approved(&store, "wd-1", 100, 10, 25);
        worker.tick();
        // Now the tx is in Broadcast state.
        let hash = store.get("wd-1").unwrap().tx_hash.unwrap();
        // Bump confirmations one short of threshold first.
        adapter.set_confirmations(&hash, 24).unwrap();
        let report = worker.tick();
        assert_eq!(report.confirmed_count, 0);
        assert_eq!(store.get("wd-1").unwrap().status, WithdrawalStatus::Broadcast);
        // Bump to threshold.
        adapter.set_confirmations(&hash, 25).unwrap();
        let report = worker.tick();
        assert_eq!(report.confirmed_count, 1);
        let r = store.get("wd-1").unwrap();
        assert_eq!(r.status, WithdrawalStatus::Confirmed);
        assert_eq!(r.confirmations, 25);
        assert!(r.confirmed_at.is_some());
    }

    #[test]
    fn tick_rejects_withdrawal_when_adapter_returns_insufficient_balance() {
        let (adapter, store, worker) = make_pieces();
        // Hot wallet has 10_000_000; submit a withdrawal larger than
        // that.
        approved(&store, "wd-rich", 20_000_000, 10, 25);
        let report = worker.tick();
        assert_eq!(report.failed_count, 1);
        let r = store.get("wd-rich").unwrap();
        assert_eq!(r.status, WithdrawalStatus::Rejected);
        assert_eq!(
            r.rejection_reason,
            Some(crate::types::WithdrawalRejectReason::BroadcastFailed)
        );
        // Adapter balance should be unchanged because both build and
        // broadcast failed.
        assert_eq!(adapter.balance(HOT).unwrap(), 10_000_000);
    }

    #[test]
    fn tick_only_processes_own_chain() {
        let (_, store, worker) = make_pieces();
        // Approved record on a different chain — worker is for Eth.
        let mut btc = make_record("wd-btc", 100, 1, 6);
        btc.chain = ChainId::Btc;
        store.create(btc).unwrap();
        for next in [
            WithdrawalStatus::Validated,
            WithdrawalStatus::Queued,
            WithdrawalStatus::Approved,
        ] {
            store.advance_status("wd-btc", next).unwrap();
        }
        let report = worker.tick();
        assert_eq!(report.broadcast_count, 0);
        // BTC record still Approved.
        assert_eq!(store.get("wd-btc").unwrap().status, WithdrawalStatus::Approved);
    }

    #[test]
    fn tick_is_idempotent_per_record() {
        // Once a record is Broadcast, subsequent ticks before the
        // confirmation threshold are no-ops on its status.
        let (_, store, worker) = make_pieces();
        approved(&store, "wd-1", 100, 10, 25);
        worker.tick();
        let after_first = store.get("wd-1").unwrap();
        // Run another tick with no confirmation bump.
        worker.tick();
        let after_second = store.get("wd-1").unwrap();
        assert_eq!(after_first.status, WithdrawalStatus::Broadcast);
        assert_eq!(after_second.status, WithdrawalStatus::Broadcast);
        assert_eq!(after_first.tx_hash, after_second.tx_hash);
    }

    #[test]
    fn empty_store_yields_zero_report() {
        let (_, _, worker) = make_pieces();
        let report = worker.tick();
        assert_eq!(report, WorkerTickReport::default());
    }
}
