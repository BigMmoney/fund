// Step 2 scaffold: store + builder compile and are exercised by unit
// tests. The Step 1D handler is updated in this same commit to record
// every authz decision; future RBAC-aware handlers (Step 5 trading-
// ops actions, Step 3 maker-checker submission/approval) will record
// through the same builder.
#![allow(dead_code)]

//! Backoffice RBAC audit log writer.
//!
//! Step 2 of the RBAC MVP delivery (per docs/BACKOFFICE_RBAC_DESIGN.md
//! §6). Append-only JSONL store of `types::AdminAuditRow` records.
//! One row per authorization decision — successes and failures alike.
//!
//! This is the new RBAC-aware audit log. The pre-existing
//! `AdminActionAuditStore` (in admin_audit.rs) predates RBAC and
//! continues to record the legacy coarse `(action, subject, role)`
//! shape for older code paths; new RBAC-aware handlers should use
//! this module instead.
//!
//! Design intent (§6, §9 rule 6):
//! - Every authorization decision (Allow committed, RequiresApproval
//!   pending, Deny) produces exactly one row.
//! - Every row is committed BEFORE the action's first observable
//!   side effect. If audit write fails, the action is denied with
//!   `denied_audit_write_failure`.
//! - Records are append-only — no in-place modification, no delete.
//!   A `redact` action (future) creates a NEW row marking the
//!   original; the writer here is unaware of redaction.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use parking_lot::Mutex;

use persistence::JsonlFileWal;
use types::{
    AdminAuditRow, ApprovalRequestId, AuditApprovalSummary, AuditDecision, AuditOutcome,
    AuthenticatedPrincipal, BackofficeAction, GrantScope, MfaMethod, ResourceRef,
    BACKOFFICE_SCHEMA_VERSION,
};

/// Lightweight ULID-ish string id for audit rows. Stable enough for
/// log correlation; the underlying source is uuid::Uuid::new_v4 since
/// the workspace doesn't pull v7. Time ordering comes from
/// `recorded_at`.
fn fresh_event_id() -> String {
    format!("audit-{}", uuid::Uuid::new_v4())
}

pub(crate) struct AdminRbacAuditStore {
    store: Arc<dyn persistence::WalStore<AdminAuditRow>>,
    write_lock: Mutex<()>,
}

impl AdminRbacAuditStore {
    pub(crate) fn new(store: Arc<dyn persistence::WalStore<AdminAuditRow>>) -> Self {
        Self {
            store,
            write_lock: Mutex::new(()),
        }
    }

    pub(crate) fn open_jsonl(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<AdminAuditRow>> =
            Arc::new(JsonlFileWal::new(path)?);
        Ok(Self::new(store))
    }

    /// Append one row. Serialised under a single mutex so per-process
    /// ordering matches per-row `recorded_at` ordering even under
    /// multi-thread bursts; the underlying `WalStore::append` is
    /// already atomic per-record but the mutex gives us a stable
    /// ordering for the in-process tail.
    pub(crate) fn append(&self, row: AdminAuditRow) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store.append(&row)
    }

    /// Read all rows from underlying storage. For tests + the future
    /// `GET /admin/audit/actions` endpoint.
    pub(crate) fn entries(&self) -> anyhow::Result<Vec<AdminAuditRow>> {
        self.store.entries()
    }

    pub(crate) fn count(&self) -> anyhow::Result<usize> {
        Ok(self.store.entries()?.len())
    }
}

/// Inputs the handler layer assembles to record one decision. Helper
/// keeps the call site terse and lets us evolve the row shape without
/// touching every handler.
pub(crate) struct DecisionRecord<'a> {
    pub principal: &'a AuthenticatedPrincipal,
    pub remote_ip: &'a str,
    pub user_agent: &'a str,
    pub mfa_method: MfaMethod,
    pub action: BackofficeAction,
    pub resource: ResourceRef,
    pub scope: GrantScope,
    pub reason: &'a str,
    pub decision: AuditDecision,
    pub decision_reason: Option<String>,
    pub approval_request_id: Option<ApprovalRequestId>,
    pub approval: Option<AuditApprovalSummary>,
    pub break_glass_session_id: Option<String>,
    pub incident_reference: Option<String>,
    pub outcome: AuditOutcome,
    pub outcome_detail: Option<String>,
}

impl AdminRbacAuditStore {
    /// Build + write one `AdminAuditRow` from a `DecisionRecord`.
    /// Convenience wrapper around `append`. Returns `Err` if the
    /// underlying WAL write fails — design §9 rule 6 says callers
    /// must treat that as `denied_audit_write_failure` and refuse
    /// the action.
    pub(crate) fn record(&self, dr: DecisionRecord<'_>) -> anyhow::Result<AdminAuditRow> {
        let now = Utc::now();
        let row = AdminAuditRow {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            event_id: fresh_event_id(),
            recorded_at: now,
            employee_id: dr.principal.subject.clone(),
            session_id: dr.principal.session_id.clone().unwrap_or_default(),
            mfa_method: dr.mfa_method,
            remote_ip: dr.remote_ip.to_string(),
            user_agent: dr.user_agent.to_string(),
            action: dr.action,
            resource: dr.resource,
            scope: dr.scope,
            reason: dr.reason.to_string(),
            requested_at: now,
            decision: dr.decision,
            decision_reason: dr.decision_reason,
            approval_request_id: dr.approval_request_id,
            approval: dr.approval,
            break_glass_session_id: dr.break_glass_session_id,
            incident_reference: dr.incident_reference,
            outcome: dr.outcome,
            outcome_detail: dr.outcome_detail,
        };
        self.append(row.clone())?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::InMemoryWal;
    use types::PrincipalRole;

    fn principal(subject: &str, role: PrincipalRole) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            subject: subject.into(),
            role,
            session_id: Some("sess-1".into()),
        }
    }

    fn deny_record<'a>(
        principal: &'a AuthenticatedPrincipal,
        action: BackofficeAction,
    ) -> DecisionRecord<'a> {
        DecisionRecord {
            principal,
            remote_ip: "10.0.1.42",
            user_agent: "BackofficeWeb/2.0",
            mfa_method: MfaMethod::Webauthn,
            action,
            resource: ResourceRef {
                kind: "endpoint".into(),
                id: "/admin/employees".into(),
            },
            scope: GrantScope::Global,
            reason: "list employees attempt for audit page",
            decision: AuditDecision::DeniedAuthz,
            decision_reason: Some("no_grant".into()),
            approval_request_id: None,
            approval: None,
            break_glass_session_id: None,
            incident_reference: None,
            outcome: AuditOutcome::Failure,
            outcome_detail: None,
        }
    }

    #[test]
    fn record_writes_row_with_canonical_shape() {
        let store = AdminRbacAuditStore::new(Arc::new(InMemoryWal::new()));
        let p = principal("alice", PrincipalRole::Admin);
        let row = store.record(deny_record(&p, BackofficeAction::EmployeesList)).unwrap();
        assert_eq!(row.employee_id, "alice");
        assert_eq!(row.session_id, "sess-1");
        assert_eq!(row.action, BackofficeAction::EmployeesList);
        assert_eq!(row.decision, AuditDecision::DeniedAuthz);
        assert_eq!(row.outcome, AuditOutcome::Failure);
        // recorded_at and event_id are server-generated and present.
        assert!(!row.event_id.is_empty());
    }

    #[test]
    fn entries_round_trip_through_underlying_wal() {
        let wal: Arc<dyn persistence::WalStore<AdminAuditRow>> = Arc::new(InMemoryWal::new());
        let store = AdminRbacAuditStore::new(wal.clone());
        let p = principal("alice", PrincipalRole::Admin);
        store.record(deny_record(&p, BackofficeAction::EmployeesList)).unwrap();
        store.record(deny_record(&p, BackofficeAction::AuditLogRead)).unwrap();
        let entries = store.entries().unwrap();
        assert_eq!(entries.len(), 2);
        // Re-open: data survives via the underlying WAL.
        let store2 = AdminRbacAuditStore::new(wal);
        assert_eq!(store2.count().unwrap(), 2);
    }

    #[test]
    fn missing_session_id_serialises_as_empty_string_not_null() {
        let store = AdminRbacAuditStore::new(Arc::new(InMemoryWal::new()));
        let mut p = principal("alice", PrincipalRole::Admin);
        p.session_id = None;
        let row = store.record(deny_record(&p, BackofficeAction::EmployeesList)).unwrap();
        // Per the row schema in §6.1 session_id is a String (not Option).
        assert_eq!(row.session_id, "");
    }

    #[test]
    fn approval_block_round_trips_when_present() {
        let store = AdminRbacAuditStore::new(Arc::new(InMemoryWal::new()));
        let p = principal("bob", PrincipalRole::Admin);
        let mut dr = deny_record(&p, BackofficeAction::WithdrawalsApprove);
        dr.decision = AuditDecision::Committed;
        dr.outcome = AuditOutcome::Success;
        dr.approval_request_id = Some("appr-99".into());
        dr.approval = Some(AuditApprovalSummary {
            submitter_employee_id: "alice".into(),
            submitter_reason: "KYC re-verified".into(),
            approver_employee_id: "bob".into(),
            approver_reason: "verified per SUP-12345".into(),
            approved_at: Utc::now(),
        });
        let row = store.record(dr).unwrap();
        assert_eq!(row.approval_request_id.as_deref(), Some("appr-99"));
        assert!(row.approval.is_some());
    }
}
