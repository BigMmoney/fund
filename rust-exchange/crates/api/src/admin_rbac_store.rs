// Step 1B scaffold: stores compile and are exercised by unit tests.
// 1C will plug them into the authz service; 1D wires REST handlers
// against them; 1E flips the existing protected endpoints over.
#![allow(dead_code)]

//! Backoffice RBAC storage layer.
//!
//! Step 1B of the RBAC MVP delivery (per docs/BACKOFFICE_RBAC_DESIGN.md
//! §8.4). Provides three append-only JSONL-backed stores with an
//! in-memory cache, mirroring the existing `OrderStateProjectionStore`
//! pattern in `order_state_projection.rs`:
//!
//! - `AdminEmployeeStore` — keyed on `EmployeeId`. Latest record wins
//!   on recovery (status / display_name / last login can change).
//! - `AdminGrantStore` — keyed on `GrantId`. Latest record wins on
//!   recovery; status flips (Provisional -> Active -> Revoked /
//!   Expired) are appended.
//! - `ApprovalRequestStore` — keyed on `ApprovalRequestId`. Latest
//!   record wins on recovery.
//!
//! Out of scope for this commit:
//! - The authorization service (1C) — `AuthzService::is_allowed`
//!   queries these stores but is not implemented here.
//! - REST handlers (1D) — `/admin/employees`, `/admin/approval-
//!   requests`, etc. are not mounted yet.
//! - Integration with existing `require_admin` checks (1E).
//!
//! All store mutators are pub(crate); no public surface is exposed
//! outside the api crate.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use parking_lot::Mutex;

use persistence::JsonlFileWal;
use types::{
    ApprovalRequest, ApprovalRequestId, ApprovalRequestStatus, Employee, EmployeeId,
    EmployeeStatus, Grant, GrantId, GrantStatus,
};

const ADMIN_LOCK_SHARDS: usize = 32;

fn lock_shard(key: &str) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % ADMIN_LOCK_SHARDS
}

// ── Employee store ──────────────────────────────────────────────────

pub(crate) struct AdminEmployeeStore {
    by_id: DashMap<EmployeeId, Employee>,
    store: Arc<dyn persistence::WalStore<Employee>>,
    write_locks: Vec<Mutex<()>>,
}

impl AdminEmployeeStore {
    pub(crate) fn new(store: Arc<dyn persistence::WalStore<Employee>>) -> anyhow::Result<Self> {
        let result = Self {
            by_id: DashMap::new(),
            store,
            write_locks: (0..ADMIN_LOCK_SHARDS).map(|_| Mutex::new(())).collect(),
        };
        // Latest entry per employee_id wins on recovery.
        for entry in result.store.entries()? {
            result.by_id.insert(entry.employee_id.clone(), entry);
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<Employee>> = Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub(crate) fn get(&self, employee_id: &str) -> Option<Employee> {
        self.by_id.get(employee_id).map(|e| e.value().clone())
    }

    pub(crate) fn list(&self) -> Vec<Employee> {
        let mut out: Vec<Employee> = self.by_id.iter().map(|e| e.value().clone()).collect();
        out.sort_by(|a, b| a.employee_id.cmp(&b.employee_id));
        out
    }

    pub(crate) fn count(&self) -> usize {
        self.by_id.len()
    }

    /// Insert a new employee record (PendingInvite or Active). Returns
    /// `Err` if the employee already exists (use `update` for status
    /// or last-login changes).
    pub(crate) fn create(&self, mut employee: Employee) -> anyhow::Result<()> {
        let key = employee.employee_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if self.by_id.contains_key(&key) {
            anyhow::bail!("employee already exists: {key}");
        }
        let now = Utc::now();
        if employee.created_at == chrono::DateTime::<Utc>::default() {
            employee.created_at = now;
        }
        employee.updated_at = now;
        self.store.append(&employee)?;
        self.by_id.insert(key, employee);
        Ok(())
    }

    /// Append an updated record for an existing employee. Returns
    /// `Err` if the employee does not exist.
    pub(crate) fn update(&self, mut employee: Employee) -> anyhow::Result<()> {
        let key = employee.employee_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if !self.by_id.contains_key(&key) {
            anyhow::bail!("employee not found: {key}");
        }
        employee.updated_at = Utc::now();
        self.store.append(&employee)?;
        self.by_id.insert(key, employee);
        Ok(())
    }

    /// Convenience: flip an employee to Suspended / Revoked.
    pub(crate) fn set_status(
        &self,
        employee_id: &str,
        status: EmployeeStatus,
    ) -> anyhow::Result<()> {
        let mut current = self
            .get(employee_id)
            .ok_or_else(|| anyhow::anyhow!("employee not found: {employee_id}"))?;
        current.status = status;
        self.update(current)
    }
}

// ── Grant store ─────────────────────────────────────────────────────

pub(crate) struct AdminGrantStore {
    by_id: DashMap<GrantId, Grant>,
    store: Arc<dyn persistence::WalStore<Grant>>,
    write_locks: Vec<Mutex<()>>,
}

impl AdminGrantStore {
    pub(crate) fn new(store: Arc<dyn persistence::WalStore<Grant>>) -> anyhow::Result<Self> {
        let result = Self {
            by_id: DashMap::new(),
            store,
            write_locks: (0..ADMIN_LOCK_SHARDS).map(|_| Mutex::new(())).collect(),
        };
        for entry in result.store.entries()? {
            result.by_id.insert(entry.grant_id.clone(), entry);
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<Grant>> = Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub(crate) fn get(&self, grant_id: &str) -> Option<Grant> {
        self.by_id.get(grant_id).map(|g| g.value().clone())
    }

    /// Return all grants (active + non-active) for an employee. The
    /// authz service filters by status + expires_at.
    pub(crate) fn for_employee(&self, employee_id: &str) -> Vec<Grant> {
        self.by_id
            .iter()
            .filter(|g| g.value().employee_id == employee_id)
            .map(|g| g.value().clone())
            .collect()
    }

    /// Return all grants currently in `Active` status whose
    /// `expires_at` is strictly after `now`.
    pub(crate) fn active_for_employee(&self, employee_id: &str) -> Vec<Grant> {
        let now = Utc::now();
        self.by_id
            .iter()
            .filter(|g| {
                let v = g.value();
                v.employee_id == employee_id
                    && v.status == GrantStatus::Active
                    && v.expires_at > now
            })
            .map(|g| g.value().clone())
            .collect()
    }

    pub(crate) fn count(&self) -> usize {
        self.by_id.len()
    }

    /// Insert a fresh grant (Provisional or Active depending on
    /// whether maker-checker is required).
    pub(crate) fn create(&self, grant: Grant) -> anyhow::Result<()> {
        let key = grant.grant_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if self.by_id.contains_key(&key) {
            anyhow::bail!("grant already exists: {key}");
        }
        self.store.append(&grant)?;
        self.by_id.insert(key, grant);
        Ok(())
    }

    /// Append an updated record. Used to flip status (Provisional ->
    /// Active on second-approver commit; Active -> Revoked on explicit
    /// revoke; Active -> Expired on TTL sweep).
    pub(crate) fn update(&self, grant: Grant) -> anyhow::Result<()> {
        let key = grant.grant_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if !self.by_id.contains_key(&key) {
            anyhow::bail!("grant not found: {key}");
        }
        self.store.append(&grant)?;
        self.by_id.insert(key, grant);
        Ok(())
    }

    pub(crate) fn set_status(&self, grant_id: &str, status: GrantStatus) -> anyhow::Result<()> {
        let mut current = self
            .get(grant_id)
            .ok_or_else(|| anyhow::anyhow!("grant not found: {grant_id}"))?;
        current.status = status;
        self.update(current)
    }
}

// ── Approval request store ──────────────────────────────────────────

pub(crate) struct ApprovalRequestStore {
    by_id: DashMap<ApprovalRequestId, ApprovalRequest>,
    store: Arc<dyn persistence::WalStore<ApprovalRequest>>,
    write_locks: Vec<Mutex<()>>,
}

impl ApprovalRequestStore {
    pub(crate) fn new(
        store: Arc<dyn persistence::WalStore<ApprovalRequest>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            by_id: DashMap::new(),
            store,
            write_locks: (0..ADMIN_LOCK_SHARDS).map(|_| Mutex::new(())).collect(),
        };
        for entry in result.store.entries()? {
            result
                .by_id
                .insert(entry.approval_request_id.clone(), entry);
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<ApprovalRequest>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub(crate) fn get(&self, id: &str) -> Option<ApprovalRequest> {
        self.by_id.get(id).map(|r| r.value().clone())
    }

    /// All requests sorted by submitted_at desc (newest first).
    pub(crate) fn list(&self) -> Vec<ApprovalRequest> {
        let mut out: Vec<ApprovalRequest> =
            self.by_id.iter().map(|r| r.value().clone()).collect();
        out.sort_by(|a, b| b.submitted_at.cmp(&a.submitted_at));
        out
    }

    /// Pending requests (not yet approved / rejected / expired).
    pub(crate) fn pending(&self) -> Vec<ApprovalRequest> {
        let now = Utc::now();
        let mut out: Vec<ApprovalRequest> = self
            .by_id
            .iter()
            .filter(|r| {
                let v = r.value();
                v.status == ApprovalRequestStatus::Pending && v.expires_at > now
            })
            .map(|r| r.value().clone())
            .collect();
        out.sort_by(|a, b| a.submitted_at.cmp(&b.submitted_at));
        out
    }

    pub(crate) fn count(&self) -> usize {
        self.by_id.len()
    }

    pub(crate) fn create(&self, request: ApprovalRequest) -> anyhow::Result<()> {
        let key = request.approval_request_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if self.by_id.contains_key(&key) {
            anyhow::bail!("approval request already exists: {key}");
        }
        self.store.append(&request)?;
        self.by_id.insert(key, request);
        Ok(())
    }

    pub(crate) fn update(&self, request: ApprovalRequest) -> anyhow::Result<()> {
        let key = request.approval_request_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if !self.by_id.contains_key(&key) {
            anyhow::bail!("approval request not found: {key}");
        }
        self.store.append(&request)?;
        self.by_id.insert(key, request);
        Ok(())
    }

    /// Sweep pending requests past their TTL, flipping them to
    /// `Expired`. Returns the number expired. Caller is responsible
    /// for invoking this periodically (e.g. from a tokio interval
    /// task in 1D when the handlers are mounted); this commit only
    /// provides the operation.
    pub(crate) fn sweep_expired(&self) -> anyhow::Result<usize> {
        let now = Utc::now();
        let to_expire: Vec<ApprovalRequest> = self
            .by_id
            .iter()
            .filter(|r| {
                let v = r.value();
                v.status == ApprovalRequestStatus::Pending && v.expires_at <= now
            })
            .map(|r| r.value().clone())
            .collect();
        let n = to_expire.len();
        for mut req in to_expire {
            req.status = ApprovalRequestStatus::Expired;
            req.decided_at = Some(now);
            self.update(req)?;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use persistence::InMemoryWal;
    use types::{
        BackofficeAction, BackofficeRole, GrantScope, MfaMethod, ResourceRef, RoleLevel,
        BACKOFFICE_SCHEMA_VERSION,
    };

    fn ts(secs: i64) -> chrono::DateTime<Utc> {
        chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000 + secs, 0).unwrap()
    }

    fn employee(id: &str) -> Employee {
        Employee {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            employee_id: id.into(),
            display_name: format!("display: {id}"),
            status: EmployeeStatus::Active,
            created_at: ts(0),
            updated_at: ts(0),
            last_mfa_method: Some(MfaMethod::Webauthn),
            last_login_at: Some(ts(0)),
        }
    }

    fn grant(id: &str, employee_id: &str, role: BackofficeRole, expires_in_days: i64) -> Grant {
        Grant {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            grant_id: id.into(),
            employee_id: employee_id.into(),
            role,
            level: RoleLevel::Act,
            scope: GrantScope::Global,
            status: GrantStatus::Active,
            granted_by: "secadmin@operator.example".into(),
            granted_at: ts(0),
            expires_at: Utc::now() + Duration::days(expires_in_days),
            reason: "test grant".into(),
            approval_request_id: None,
        }
    }

    fn approval_request(id: &str, submitter: &str) -> ApprovalRequest {
        ApprovalRequest {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            approval_request_id: id.into(),
            action: BackofficeAction::WithdrawalsApprove,
            resource: ResourceRef {
                kind: "withdrawal".into(),
                id: "wd-1".into(),
            },
            scope: GrantScope::Global,
            submitter_employee_id: submitter.into(),
            submitter_reason: "test reason that meets the 16-char min".into(),
            submitted_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(24),
            status: ApprovalRequestStatus::Pending,
            approver_employee_id: None,
            approver_reason: None,
            decided_at: None,
            action_payload: serde_json::json!({ "withdrawal_id": "wd-1" }),
        }
    }

    // ── Employee store ─────────────────────────────────────────────

    #[test]
    fn employee_create_then_get() {
        let s = AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(employee("alice")).unwrap();
        assert_eq!(s.count(), 1);
        let got = s.get("alice").expect("present");
        assert_eq!(got.employee_id, "alice");
        assert_eq!(got.status, EmployeeStatus::Active);
    }

    #[test]
    fn employee_create_duplicate_rejected() {
        let s = AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(employee("alice")).unwrap();
        let err = s.create(employee("alice")).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn employee_set_status_appends_new_record() {
        let wal: Arc<dyn persistence::WalStore<Employee>> = Arc::new(InMemoryWal::new());
        let s = AdminEmployeeStore::new(wal.clone()).unwrap();
        s.create(employee("alice")).unwrap();
        s.set_status("alice", EmployeeStatus::Suspended).unwrap();
        // In-memory cache reflects latest.
        assert_eq!(s.get("alice").unwrap().status, EmployeeStatus::Suspended);
        // WAL has both versions; recovery picks the latest.
        let entries = wal.entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].status, EmployeeStatus::Active);
        assert_eq!(entries[1].status, EmployeeStatus::Suspended);
    }

    #[test]
    fn employee_recovery_replays_latest_status() {
        let wal: Arc<dyn persistence::WalStore<Employee>> = Arc::new(InMemoryWal::new());
        {
            let s = AdminEmployeeStore::new(wal.clone()).unwrap();
            s.create(employee("alice")).unwrap();
            s.set_status("alice", EmployeeStatus::Revoked).unwrap();
        }
        // Re-open the store and confirm the latest status survived.
        let s2 = AdminEmployeeStore::new(wal).unwrap();
        assert_eq!(s2.count(), 1);
        assert_eq!(s2.get("alice").unwrap().status, EmployeeStatus::Revoked);
    }

    #[test]
    fn employee_list_is_sorted_by_id() {
        let s = AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(employee("charlie")).unwrap();
        s.create(employee("alice")).unwrap();
        s.create(employee("bob")).unwrap();
        let ids: Vec<String> = s.list().into_iter().map(|e| e.employee_id).collect();
        assert_eq!(ids, vec!["alice", "bob", "charlie"]);
    }

    // ── Grant store ────────────────────────────────────────────────

    #[test]
    fn grant_active_for_employee_filters_status_and_expiry() {
        let s = AdminGrantStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(grant("g-active", "alice", BackofficeRole::TradingOps, 30))
            .unwrap();
        s.create(grant("g-active-other", "bob", BackofficeRole::RiskOps, 30))
            .unwrap();
        // Past-expiry grant: still in store but not "active".
        let mut expired = grant("g-expired", "alice", BackofficeRole::FinanceOps, 30);
        expired.expires_at = Utc::now() - Duration::days(1);
        s.create(expired).unwrap();
        // Revoked grant on alice.
        let mut revoked = grant("g-revoked", "alice", BackofficeRole::AuditorReadonly, 30);
        revoked.status = GrantStatus::Revoked;
        s.create(revoked).unwrap();

        let active = s.active_for_employee("alice");
        let ids: Vec<String> = active.iter().map(|g| g.grant_id.clone()).collect();
        assert_eq!(active.len(), 1, "expected only the live trading_ops grant");
        assert!(ids.contains(&"g-active".to_string()));
    }

    #[test]
    fn grant_set_status_round_trip_through_recovery() {
        let wal: Arc<dyn persistence::WalStore<Grant>> = Arc::new(InMemoryWal::new());
        {
            let s = AdminGrantStore::new(wal.clone()).unwrap();
            s.create(grant("g-1", "alice", BackofficeRole::TradingOps, 30))
                .unwrap();
            s.set_status("g-1", GrantStatus::Revoked).unwrap();
        }
        let s2 = AdminGrantStore::new(wal).unwrap();
        assert_eq!(s2.get("g-1").unwrap().status, GrantStatus::Revoked);
    }

    #[test]
    fn grant_for_employee_returns_all_statuses() {
        let s = AdminGrantStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(grant("g-1", "alice", BackofficeRole::TradingOps, 30))
            .unwrap();
        let mut g2 = grant("g-2", "alice", BackofficeRole::RiskOps, 30);
        g2.status = GrantStatus::Revoked;
        s.create(g2).unwrap();
        let all = s.for_employee("alice");
        assert_eq!(all.len(), 2);
    }

    // ── Approval-request store ────────────────────────────────────

    #[test]
    fn approval_pending_excludes_expired_and_decided() {
        let s = ApprovalRequestStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(approval_request("appr-1", "alice")).unwrap();

        // An already-approved request.
        let mut decided = approval_request("appr-2", "alice");
        decided.status = ApprovalRequestStatus::Approved;
        decided.approver_employee_id = Some("bob".into());
        decided.decided_at = Some(Utc::now());
        s.create(decided).unwrap();

        // A request that's past its TTL.
        let mut expired = approval_request("appr-3", "alice");
        expired.expires_at = Utc::now() - Duration::seconds(1);
        s.create(expired).unwrap();

        let pending = s.pending();
        let ids: Vec<String> = pending.iter().map(|r| r.approval_request_id.clone()).collect();
        assert_eq!(pending.len(), 1);
        assert_eq!(ids[0], "appr-1");
    }

    #[test]
    fn approval_sweep_expired_flips_status() {
        let s = ApprovalRequestStore::new(Arc::new(InMemoryWal::new())).unwrap();
        let mut e = approval_request("appr-tt", "alice");
        e.expires_at = Utc::now() - Duration::seconds(1);
        s.create(e).unwrap();
        let n = s.sweep_expired().unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            s.get("appr-tt").unwrap().status,
            ApprovalRequestStatus::Expired
        );
        // Idempotent: a second sweep finds nothing.
        assert_eq!(s.sweep_expired().unwrap(), 0);
    }

    #[test]
    fn approval_create_duplicate_rejected() {
        let s = ApprovalRequestStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(approval_request("appr-1", "alice")).unwrap();
        let err = s.create(approval_request("appr-1", "alice")).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn approval_recovery_replays_latest_state() {
        let wal: Arc<dyn persistence::WalStore<ApprovalRequest>> = Arc::new(InMemoryWal::new());
        {
            let s = ApprovalRequestStore::new(wal.clone()).unwrap();
            s.create(approval_request("appr-1", "alice")).unwrap();
            let mut updated = s.get("appr-1").unwrap();
            updated.status = ApprovalRequestStatus::Approved;
            updated.approver_employee_id = Some("bob".into());
            updated.decided_at = Some(Utc::now());
            updated.approver_reason = Some("ok".into());
            s.update(updated).unwrap();
        }
        let s2 = ApprovalRequestStore::new(wal).unwrap();
        let r = s2.get("appr-1").unwrap();
        assert_eq!(r.status, ApprovalRequestStatus::Approved);
        assert_eq!(r.approver_employee_id.as_deref(), Some("bob"));
    }
}
