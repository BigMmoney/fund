//! Backoffice RBAC and Employee Permission types.
//!
//! Type model for the design in `docs/BACKOFFICE_RBAC_DESIGN.md`.
//! This module is wire/storage-shape only — the authorization service,
//! approval flow, REST handlers, and audit writer all live in
//! `crates/api` and consume these types.
//!
//! Sub-step 1A of the RBAC MVP delivery (per design §8.4): types only,
//! no storage, no handlers, no behaviour change. Subsequent sub-steps:
//!   1B  storage (`AdminEmployeeStore`, `AdminGrantStore`,
//!       `ApprovalRequestStore`).
//!   1C  authorization service (`AuthzService::is_allowed`).
//!   1D  REST handlers (`/admin/me/permissions`, `/admin/employees`,
//!       `/admin/approval-requests`, ...).
//!   1E  integration with existing protected endpoints (replaces the
//!       coarse `require_admin` checks with per-action permission
//!       checks).
//!   1F  unit + integration tests + smoke test.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Schema version for backoffice RBAC types — bump on breaking field
/// changes so persisted records can be matched against the producing
/// version.
pub const BACKOFFICE_SCHEMA_VERSION: u32 = 1;

// ── Employee ─────────────────────────────────────────────────────────

/// Stable employee identifier — typically the corporate email address
/// at issuance time (`alice@operator.example`). Treated as opaque
/// elsewhere; equality semantics are byte-wise.
pub type EmployeeId = String;

/// Lifecycle state of an employee account. See design §2.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmployeeStatus {
    /// Invited but not yet logged in. Cannot perform any action.
    PendingInvite,
    /// Normal: subject to per-action grant checks.
    Active,
    /// Login allowed only to clear the alert; no actions succeed.
    Suspended,
    /// Login refused. Audit history retained.
    Revoked,
}

/// Persistent employee record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Employee {
    pub schema_version: u32,
    pub employee_id: EmployeeId,
    pub display_name: String,
    pub status: EmployeeStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Method used at most recent login. None if never logged in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_mfa_method: Option<MfaMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<DateTime<Utc>>,
}

/// Multi-factor methods accepted at backoffice login. Order reflects
/// preference (WebAuthn > TOTP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MfaMethod {
    Webauthn,
    Totp,
}

// ── Roles, levels, scopes ────────────────────────────────────────────

/// Job-function role per design §3. Ten variants long-term; the v1 MVP
/// uses six (auditor_readonly, support_l1, trading_ops, risk_ops,
/// finance_ops, super_admin_break_glass — design §8.1) but the full
/// enum is defined here so the storage layer never needs a migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackofficeRole {
    AuditorReadonly,
    SupportL1,
    SupportL2,
    TradingOps,
    RiskOps,
    FinanceOps,
    ComplianceOps,
    SreOps,
    SecurityAdmin,
    SuperAdminBreakGlass,
}

/// Privilege height within a role family (design §2). Three levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleLevel {
    /// Observe only.
    Read,
    /// Perform documented day-to-day actions.
    Act,
    /// Initiate maker-checker requests for higher-impact actions.
    Escalate,
}

/// Slice of the system the grant applies to (design §2). Strings are
/// case-sensitive prefixes of the form `kind:value`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    /// Global.
    Global,
    Market(String),
    Desk(String),
    Customer(String),
}

/// Lifecycle state of a single `(role, level, scope)` grant
/// (design §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantStatus {
    /// Issued but awaiting second approver (maker-checker).
    Provisional,
    /// Fully approved, valid until expires_at.
    Active,
    /// Past TTL.
    Expired,
    /// Explicitly removed; cannot be reused.
    Revoked,
}

/// Stable identifier for a single grant. Server-generated.
pub type GrantId = String;

/// One grant tying a `(role, level, scope)` triple to an employee for
/// a bounded time window. The effective permission for an action is
/// the OR of all the employee's `Active` grants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub schema_version: u32,
    pub grant_id: GrantId,
    pub employee_id: EmployeeId,
    pub role: BackofficeRole,
    pub level: RoleLevel,
    pub scope: GrantScope,
    pub status: GrantStatus,
    pub granted_by: EmployeeId,
    pub granted_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Reason supplied at grant time (design §9 rule 4).
    pub reason: String,
    /// If the grant required maker-checker, the approval request that
    /// promoted it from `Provisional` to `Active`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_request_id: Option<ApprovalRequestId>,
}

// ── Actions ─────────────────────────────────────────────────────────

/// Every backoffice action that the authorization layer protects, per
/// the §4 permission matrix. The variant name is the canonical key
/// used in audit rows, in `/admin/me/permissions` effective maps, and
/// in approval-request payloads.
///
/// Variants distinguish thresholded forms of the same logical action
/// (e.g. `OrdersMassCancelLe100` vs `OrdersMassCancelGt100`) so the
/// matrix evaluation is a single enum lookup rather than a runtime
/// branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackofficeAction {
    OrdersRead,
    OrdersTimeline,
    OrdersCancelSingle,
    OrdersMassCancelLe100,
    OrdersMassCancelGt100,
    MonitorAccess,
    UsersRead,
    UsersFreeze,
    UsersUnfreeze,
    UsersRestrict,
    BalancesRead,
    BalancesAdjust,
    WithdrawalsReview,
    WithdrawalsApprove,
    WithdrawalsReject,
    RiskLimitsRead,
    RiskLimitsUpdateRaise,
    RiskLimitsUpdateLower,
    RiskKillSwitchToggle,
    MarketHalt,
    MarketResume,
    AuditLogRead,
    AuditLogExport,
    EmployeesList,
    EmployeesCreate,
    EmployeesGrantRole,
    EmployeesRevokeRole,
    EmployeesSuspend,
    EmployeesDelete,
}

/// Verdict returned by the authorization service / `/admin/me/permissions`.
/// The frontend uses these to gate buttons; the server enforces
/// independently on every protected handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackofficeActionVerdict {
    /// Single-actor permitted; commits synchronously.
    Allow,
    /// Permitted only via maker-checker (`POST /admin/approval-requests`).
    RequiresApproval,
    /// Denied at this employee's grant set.
    Deny,
}

// ── Approval requests ────────────────────────────────────────────────

/// Server-generated id for a maker-checker approval request.
pub type ApprovalRequestId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequestStatus {
    /// Submitted, awaiting an eligible approver.
    Pending,
    /// Approved and the underlying action has committed.
    Approved,
    /// Approver rejected; no commit.
    Rejected,
    /// Past expires_at without an approve / reject.
    Expired,
}

/// Reference to the resource the action targets. The `kind` is a
/// stable lowercase string (`"withdrawal"`, `"order"`, `"market"`,
/// `"employee_grant"`); `id` is the resource-specific identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRef {
    pub kind: String,
    pub id: String,
}

/// One pending or settled maker-checker request.
///
/// `action_payload` is opaque JSON validated by the action handler at
/// commit time, *not* by the approval layer. This keeps the approval
/// flow generic across action types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub schema_version: u32,
    pub approval_request_id: ApprovalRequestId,
    pub action: BackofficeAction,
    pub resource: ResourceRef,
    pub scope: GrantScope,

    pub submitter_employee_id: EmployeeId,
    pub submitter_reason: String,
    pub submitted_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,

    pub status: ApprovalRequestStatus,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver_employee_id: Option<EmployeeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<DateTime<Utc>>,

    /// Action-specific JSON body validated at commit time (e.g.
    /// `{ "withdrawal_id": "wd-7891" }` for `withdrawals.approve`).
    pub action_payload: serde_json::Value,
}

// ── Admin audit row ──────────────────────────────────────────────────

/// One row per admin action attempt — successes and failures alike.
/// See design §6.1 for the canonical shape; this matches it
/// field-for-field plus the `schema_version` discriminator for
/// future-proofing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminAuditRow {
    pub schema_version: u32,
    pub event_id: String,
    pub recorded_at: DateTime<Utc>,

    pub employee_id: EmployeeId,
    pub session_id: String,
    pub mfa_method: MfaMethod,
    pub remote_ip: String,
    pub user_agent: String,

    pub action: BackofficeAction,
    pub resource: ResourceRef,
    pub scope: GrantScope,
    pub reason: String,

    pub requested_at: DateTime<Utc>,
    pub decision: AuditDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_request_id: Option<ApprovalRequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<AuditApprovalSummary>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub break_glass_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_reference: Option<String>,

    pub outcome: AuditOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_detail: Option<String>,
}

/// Why the authorization service decided as it did. Mirrors the
/// design §6.2 list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDecision {
    Committed,
    DeniedAuthz,
    DeniedMfa,
    DeniedSelfApproval,
    DeniedExpiredGrant,
    DeniedAuditWriteFailure,
    PendingApproval,
    ExpiredUnapproved,
    RejectedByApprover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Success,
    Failure,
}

/// Embedded approval-summary block on an audit row when the action
/// committed (or was rejected) via maker-checker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditApprovalSummary {
    pub submitter_employee_id: EmployeeId,
    pub submitter_reason: String,
    pub approver_employee_id: EmployeeId,
    pub approver_reason: String,
    pub approved_at: DateTime<Utc>,
}

// ── Display impls (for tracing / logs) ──────────────────────────────

impl std::fmt::Display for BackofficeRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Re-use the serde representation so display matches wire form.
        let s = serde_json::to_string(self).unwrap_or_else(|_| "?".into());
        write!(f, "{}", s.trim_matches('"'))
    }
}

impl std::fmt::Display for BackofficeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_string(self).unwrap_or_else(|_| "?".into());
        write!(f, "{}", s.trim_matches('"'))
    }
}

impl std::fmt::Display for GrantScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantScope::Global => write!(f, "*"),
            GrantScope::Market(m) => write!(f, "market:{m}"),
            GrantScope::Desk(d) => write!(f, "desk:{d}"),
            GrantScope::Customer(c) => write!(f, "customer:{c}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-03T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn role_serializes_as_snake_case() {
        let s = serde_json::to_string(&BackofficeRole::TradingOps).unwrap();
        assert_eq!(s, "\"trading_ops\"");
        let s = serde_json::to_string(&BackofficeRole::SuperAdminBreakGlass).unwrap();
        assert_eq!(s, "\"super_admin_break_glass\"");
    }

    #[test]
    fn action_serializes_as_snake_case() {
        let s = serde_json::to_string(&BackofficeAction::OrdersMassCancelGt100).unwrap();
        assert_eq!(s, "\"orders_mass_cancel_gt100\"");
        let s = serde_json::to_string(&BackofficeAction::EmployeesGrantRole).unwrap();
        assert_eq!(s, "\"employees_grant_role\"");
    }

    #[test]
    fn level_orders_lexically_for_minimum_required_checks() {
        // `Read < Act < Escalate` so callers can compare with `>=`
        // when checking grant satisfies action requirement.
        assert!(RoleLevel::Read < RoleLevel::Act);
        assert!(RoleLevel::Act < RoleLevel::Escalate);
    }

    #[test]
    fn scope_serializes_as_tagged_enum() {
        let s = serde_json::to_string(&GrantScope::Global).unwrap();
        assert_eq!(s, "\"global\"");
        let s = serde_json::to_string(&GrantScope::Market("btc-usdt".into())).unwrap();
        assert_eq!(s, r#"{"market":"btc-usdt"}"#);
    }

    #[test]
    fn scope_display_matches_design_doc_format() {
        assert_eq!(GrantScope::Global.to_string(), "*");
        assert_eq!(GrantScope::Market("btc-usdt".into()).to_string(), "market:btc-usdt");
        assert_eq!(GrantScope::Customer("user-12345".into()).to_string(), "customer:user-12345");
    }

    #[test]
    fn employee_round_trip() {
        let e = Employee {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            employee_id: "alice@operator.example".into(),
            display_name: "Alice Example".into(),
            status: EmployeeStatus::Active,
            created_at: ts(),
            updated_at: ts(),
            last_mfa_method: Some(MfaMethod::Webauthn),
            last_login_at: Some(ts()),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: Employee = serde_json::from_str(&s).unwrap();
        assert_eq!(back, e);
        assert!(s.contains("\"webauthn\""));
        assert!(s.contains("\"active\""));
    }

    #[test]
    fn grant_round_trip() {
        let g = Grant {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            grant_id: "g-1".into(),
            employee_id: "alice@operator.example".into(),
            role: BackofficeRole::TradingOps,
            level: RoleLevel::Act,
            scope: GrantScope::Market("btc-usdt".into()),
            status: GrantStatus::Active,
            granted_by: "secadmin@operator.example".into(),
            granted_at: ts(),
            expires_at: ts(),
            reason: "Q3 oncall rotation per Linear OPS-441".into(),
            approval_request_id: None,
        };
        let s = serde_json::to_string(&g).unwrap();
        let back: Grant = serde_json::from_str(&s).unwrap();
        assert_eq!(back, g);
        assert!(s.contains("\"trading_ops\""));
        assert!(s.contains("\"act\""));
    }

    #[test]
    fn approval_request_round_trip_carries_action_payload() {
        let req = ApprovalRequest {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            approval_request_id: "appr-99".into(),
            action: BackofficeAction::WithdrawalsApprove,
            resource: ResourceRef {
                kind: "withdrawal".into(),
                id: "wd-7891".into(),
            },
            scope: GrantScope::Global,
            submitter_employee_id: "alice@operator.example".into(),
            submitter_reason: "customer KYC re-verified per ticket SUP-12345".into(),
            submitted_at: ts(),
            expires_at: ts(),
            status: ApprovalRequestStatus::Pending,
            approver_employee_id: None,
            approver_reason: None,
            decided_at: None,
            action_payload: serde_json::json!({ "withdrawal_id": "wd-7891" }),
        };
        let bytes = serde_json::to_vec(&req).unwrap();
        let back: ApprovalRequest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, req);
        assert_eq!(back.action_payload["withdrawal_id"], "wd-7891");
    }

    #[test]
    fn audit_row_round_trip_with_approval_block() {
        let row = AdminAuditRow {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            event_id: "audit-0192".into(),
            recorded_at: ts(),
            employee_id: "bob@operator.example".into(),
            session_id: "sess-abc".into(),
            mfa_method: MfaMethod::Totp,
            remote_ip: "10.0.1.42".into(),
            user_agent: "BackofficeWeb/2.0".into(),
            action: BackofficeAction::WithdrawalsApprove,
            resource: ResourceRef {
                kind: "withdrawal".into(),
                id: "wd-7891".into(),
            },
            scope: GrantScope::Global,
            reason: "verified per SUP-12345".into(),
            requested_at: ts(),
            decision: AuditDecision::Committed,
            decision_reason: None,
            approval_request_id: Some("appr-99".into()),
            approval: Some(AuditApprovalSummary {
                submitter_employee_id: "alice@operator.example".into(),
                submitter_reason: "customer KYC re-verified".into(),
                approver_employee_id: "bob@operator.example".into(),
                approver_reason: "verified per SUP-12345".into(),
                approved_at: ts(),
            }),
            break_glass_session_id: None,
            incident_reference: None,
            outcome: AuditOutcome::Success,
            outcome_detail: None,
        };
        let s = serde_json::to_string(&row).unwrap();
        let back: AdminAuditRow = serde_json::from_str(&s).unwrap();
        assert_eq!(back, row);
        // Optional `None` fields should be omitted from the wire form.
        assert!(!s.contains("\"break_glass_session_id\""));
        assert!(!s.contains("\"incident_reference\""));
        assert!(!s.contains("\"outcome_detail\""));
        // Required fields and the approval block are present.
        assert!(s.contains("\"approval\""));
        assert!(s.contains("\"committed\""));
    }

    #[test]
    fn audit_row_omits_approval_when_single_actor() {
        let row = AdminAuditRow {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            event_id: "audit-2".into(),
            recorded_at: ts(),
            employee_id: "alice@operator.example".into(),
            session_id: "sess-xyz".into(),
            mfa_method: MfaMethod::Webauthn,
            remote_ip: "10.0.1.42".into(),
            user_agent: "BackofficeWeb/2.0".into(),
            action: BackofficeAction::OrdersRead,
            resource: ResourceRef {
                kind: "order".into(),
                id: "ord-x".into(),
            },
            scope: GrantScope::Global,
            reason: "ticket SUP-1".into(),
            requested_at: ts(),
            decision: AuditDecision::Committed,
            decision_reason: None,
            approval_request_id: None,
            approval: None,
            break_glass_session_id: None,
            incident_reference: None,
            outcome: AuditOutcome::Success,
            outcome_detail: None,
        };
        let s = serde_json::to_string(&row).unwrap();
        assert!(!s.contains("\"approval\""));
        assert!(!s.contains("\"approval_request_id\""));
    }

    #[test]
    fn verdict_serializes_for_client_gate() {
        assert_eq!(serde_json::to_string(&BackofficeActionVerdict::Allow).unwrap(), "\"allow\"");
        assert_eq!(
            serde_json::to_string(&BackofficeActionVerdict::RequiresApproval).unwrap(),
            "\"requires_approval\""
        );
        assert_eq!(serde_json::to_string(&BackofficeActionVerdict::Deny).unwrap(), "\"deny\"");
    }
}
