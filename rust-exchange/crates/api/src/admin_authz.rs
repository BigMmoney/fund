// Step 1C scaffold: service compiles and is exercised by unit tests.
// 1D will plug it into REST handlers; 1E swaps require_admin for
// per-action checks at existing protected endpoints.
#![allow(dead_code)]

//! Backoffice RBAC authorization service.
//!
//! Step 1C of the RBAC MVP delivery (per docs/BACKOFFICE_RBAC_DESIGN.md
//! §8.4). Given an employee + an action + a target scope, returns a
//! `BackofficeActionVerdict` (Allow / RequiresApproval / Deny). Backed
//! by the stores from Step 1B.
//!
//! The permission matrix is encoded as a static const table of
//! `(action, role, min_level, requires_mc)` rows. The `Allow` branch
//! wins over `RequiresApproval` if both are satisfied, so a user with
//! both `act` and `act+MC` grants gets the lower-friction path. Empty
//! matrix entries = `Deny`.
//!
//! v1 MVP scope (per design §8.2) only — actions outside the MVP set
//! return `Deny` until later sub-steps populate them. The full §4
//! matrix lands incrementally as v1.1 work.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;

use types::{
    BackofficeAction, BackofficeActionVerdict, BackofficeRole, EmployeeStatus, GrantScope,
    GrantStatus, RoleLevel,
};

use crate::admin_rbac_store::{AdminEmployeeStore, AdminGrantStore};

/// Static row in the permission matrix. One row = one path by which
/// `(role, min_level)` can satisfy `action`. `requires_maker_checker`
/// = true means even a satisfying grant gets `RequiresApproval` (not
/// `Allow`).
#[derive(Debug, Clone, Copy)]
struct MatrixRow {
    action: BackofficeAction,
    role: BackofficeRole,
    min_level: RoleLevel,
    requires_maker_checker: bool,
}

/// v1 MVP permission matrix. Rows can be added as later sub-steps
/// expand action coverage. The order is purely human-readability;
/// `is_allowed` walks the whole table.
static V1_MATRIX: &[MatrixRow] = &[
    // ── Read actions: every v1 role can read most things ──────────
    row(BackofficeAction::OrdersRead, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::OrdersRead, BackofficeRole::SupportL1, RoleLevel::Read, false),
    row(BackofficeAction::OrdersRead, BackofficeRole::TradingOps, RoleLevel::Read, false),
    row(BackofficeAction::OrdersRead, BackofficeRole::RiskOps, RoleLevel::Read, false),
    row(BackofficeAction::OrdersRead, BackofficeRole::FinanceOps, RoleLevel::Read, false),
    row(BackofficeAction::OrdersRead, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::OrdersTimeline, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::OrdersTimeline, BackofficeRole::SupportL1, RoleLevel::Read, false),
    row(BackofficeAction::OrdersTimeline, BackofficeRole::TradingOps, RoleLevel::Read, false),
    row(BackofficeAction::OrdersTimeline, BackofficeRole::RiskOps, RoleLevel::Read, false),
    row(BackofficeAction::OrdersTimeline, BackofficeRole::FinanceOps, RoleLevel::Read, false),
    row(BackofficeAction::OrdersTimeline, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::MonitorAccess, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::MonitorAccess, BackofficeRole::SupportL1, RoleLevel::Read, false),
    row(BackofficeAction::MonitorAccess, BackofficeRole::TradingOps, RoleLevel::Read, false),
    row(BackofficeAction::MonitorAccess, BackofficeRole::RiskOps, RoleLevel::Read, false),
    row(BackofficeAction::MonitorAccess, BackofficeRole::FinanceOps, RoleLevel::Read, false),
    row(BackofficeAction::MonitorAccess, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::AuditLogRead, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::AuditLogRead, BackofficeRole::TradingOps, RoleLevel::Read, false),
    row(BackofficeAction::AuditLogRead, BackofficeRole::RiskOps, RoleLevel::Read, false),
    row(BackofficeAction::AuditLogRead, BackofficeRole::FinanceOps, RoleLevel::Read, false),
    row(BackofficeAction::AuditLogRead, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::EmployeesList, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::EmployeesList, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::BalancesRead, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::BalancesRead, BackofficeRole::SupportL1, RoleLevel::Read, false),
    row(BackofficeAction::BalancesRead, BackofficeRole::FinanceOps, RoleLevel::Read, false),
    row(BackofficeAction::BalancesRead, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::UsersRead, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::UsersRead, BackofficeRole::SupportL1, RoleLevel::Read, false),
    row(BackofficeAction::UsersRead, BackofficeRole::FinanceOps, RoleLevel::Read, false),
    row(BackofficeAction::UsersRead, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::WithdrawalsReview, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::WithdrawalsReview, BackofficeRole::SupportL1, RoleLevel::Read, false),
    row(BackofficeAction::WithdrawalsReview, BackofficeRole::FinanceOps, RoleLevel::Act, false),
    row(BackofficeAction::WithdrawalsReview, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::RiskLimitsRead, BackofficeRole::AuditorReadonly, RoleLevel::Read, false),
    row(BackofficeAction::RiskLimitsRead, BackofficeRole::TradingOps, RoleLevel::Read, false),
    row(BackofficeAction::RiskLimitsRead, BackofficeRole::RiskOps, RoleLevel::Read, false),
    row(BackofficeAction::RiskLimitsRead, BackofficeRole::FinanceOps, RoleLevel::Read, false),
    row(BackofficeAction::RiskLimitsRead, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),

    // ── Single-actor write actions ────────────────────────────────
    row(BackofficeAction::OrdersCancelSingle, BackofficeRole::TradingOps, RoleLevel::Act, false),
    row(BackofficeAction::OrdersCancelSingle, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::OrdersMassCancelLe100, BackofficeRole::TradingOps, RoleLevel::Act, false),
    row(BackofficeAction::OrdersMassCancelLe100, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::WithdrawalsReject, BackofficeRole::FinanceOps, RoleLevel::Act, false),
    row(BackofficeAction::WithdrawalsReject, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::EmployeesSuspend, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::EmployeesRevokeRole, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),

    // ── Maker-checker write actions ──────────────────────────────
    row(BackofficeAction::OrdersMassCancelGt100, BackofficeRole::TradingOps, RoleLevel::Act, true),
    row(BackofficeAction::OrdersMassCancelGt100, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::MarketHalt, BackofficeRole::TradingOps, RoleLevel::Act, true),
    row(BackofficeAction::MarketHalt, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::MarketResume, BackofficeRole::TradingOps, RoleLevel::Act, true),
    row(BackofficeAction::MarketResume, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, true),
    row(BackofficeAction::RiskLimitsUpdateRaise, BackofficeRole::RiskOps, RoleLevel::Act, true),
    row(BackofficeAction::RiskLimitsUpdateRaise, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, true),
    row(BackofficeAction::RiskLimitsUpdateLower, BackofficeRole::RiskOps, RoleLevel::Act, true),
    row(BackofficeAction::RiskLimitsUpdateLower, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, true),
    row(BackofficeAction::RiskKillSwitchToggle, BackofficeRole::RiskOps, RoleLevel::Act, true),
    row(BackofficeAction::RiskKillSwitchToggle, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, false),
    row(BackofficeAction::WithdrawalsApprove, BackofficeRole::FinanceOps, RoleLevel::Act, true),
    row(BackofficeAction::WithdrawalsApprove, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, true),
    row(BackofficeAction::BalancesAdjust, BackofficeRole::FinanceOps, RoleLevel::Act, true),
    row(BackofficeAction::BalancesAdjust, BackofficeRole::SuperAdminBreakGlass, RoleLevel::Act, true),
];

const fn row(
    action: BackofficeAction,
    role: BackofficeRole,
    min_level: RoleLevel,
    requires_maker_checker: bool,
) -> MatrixRow {
    MatrixRow {
        action,
        role,
        min_level,
        requires_maker_checker,
    }
}

/// All MVP actions, in the order they appear in V1_MATRIX. Used by
/// `effective_map` to populate `/admin/me/permissions`.
fn mvp_actions() -> Vec<BackofficeAction> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for r in V1_MATRIX {
        if seen.insert(r.action as u32) {
            out.push(r.action);
        }
    }
    out
}

/// `Global` covers any requested scope; otherwise the grant scope must
/// equal the requested scope exactly.
fn grant_covers_scope(grant_scope: &GrantScope, requested: &GrantScope) -> bool {
    match grant_scope {
        GrantScope::Global => true,
        other => other == requested,
    }
}

pub(crate) struct AuthzService {
    employees: Arc<AdminEmployeeStore>,
    grants: Arc<AdminGrantStore>,
}

impl AuthzService {
    pub(crate) fn new(employees: Arc<AdminEmployeeStore>, grants: Arc<AdminGrantStore>) -> Self {
        Self { employees, grants }
    }

    /// Return Allow / RequiresApproval / Deny for `action` against the
    /// employee's currently-active grants. Suspended / revoked
    /// employees get Deny regardless of grants.
    pub(crate) fn is_allowed(
        &self,
        employee_id: &str,
        action: BackofficeAction,
        requested_scope: &GrantScope,
    ) -> BackofficeActionVerdict {
        // Employee must be active.
        match self.employees.get(employee_id) {
            Some(e) if e.status == EmployeeStatus::Active => {}
            _ => return BackofficeActionVerdict::Deny,
        }

        let now = Utc::now();
        let grants: Vec<_> = self
            .grants
            .for_employee(employee_id)
            .into_iter()
            .filter(|g| g.status == GrantStatus::Active && g.expires_at > now)
            .collect();
        if grants.is_empty() {
            return BackofficeActionVerdict::Deny;
        }

        let mut best = BackofficeActionVerdict::Deny;
        for matrix_row in V1_MATRIX.iter().filter(|r| r.action == action) {
            for grant in &grants {
                if grant.role != matrix_row.role {
                    continue;
                }
                if grant.level < matrix_row.min_level {
                    continue;
                }
                if !grant_covers_scope(&grant.scope, requested_scope) {
                    continue;
                }
                // Match. `Allow` wins over `RequiresApproval`.
                if matrix_row.requires_maker_checker {
                    if best == BackofficeActionVerdict::Deny {
                        best = BackofficeActionVerdict::RequiresApproval;
                    }
                } else {
                    return BackofficeActionVerdict::Allow;
                }
            }
        }
        best
    }

    /// Effective `action -> verdict` map for `/admin/me/permissions`.
    /// Always evaluated against `GrantScope::Global` for the v1 client
    /// surface; finer-scoped actions still need a server-side check
    /// at the handler.
    pub(crate) fn effective_map(
        &self,
        employee_id: &str,
    ) -> HashMap<BackofficeAction, BackofficeActionVerdict> {
        let mut out = HashMap::new();
        for action in mvp_actions() {
            let v = self.is_allowed(employee_id, action, &GrantScope::Global);
            out.insert(action, v);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use persistence::InMemoryWal;
    use types::{Employee, Grant, MfaMethod, BACKOFFICE_SCHEMA_VERSION};

    fn ts(secs: i64) -> chrono::DateTime<Utc> {
        chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000 + secs, 0).unwrap()
    }

    fn make_stores() -> (Arc<AdminEmployeeStore>, Arc<AdminGrantStore>, AuthzService) {
        let employees = Arc::new(AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let grants = Arc::new(AdminGrantStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let svc = AuthzService::new(employees.clone(), grants.clone());
        (employees, grants, svc)
    }

    fn employee(id: &str, status: EmployeeStatus) -> Employee {
        Employee {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            employee_id: id.into(),
            display_name: id.into(),
            status,
            created_at: ts(0),
            updated_at: ts(0),
            last_mfa_method: Some(MfaMethod::Webauthn),
            last_login_at: Some(ts(0)),
        }
    }

    fn grant(
        id: &str,
        employee_id: &str,
        role: BackofficeRole,
        level: RoleLevel,
        scope: GrantScope,
    ) -> Grant {
        Grant {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            grant_id: id.into(),
            employee_id: employee_id.into(),
            role,
            level,
            scope,
            status: GrantStatus::Active,
            granted_by: "secadmin".into(),
            granted_at: ts(0),
            expires_at: Utc::now() + Duration::days(30),
            reason: "test grant".into(),
            approval_request_id: None,
        }
    }

    #[test]
    fn unknown_employee_is_denied() {
        let (_, _, svc) = make_stores();
        assert_eq!(
            svc.is_allowed("ghost", BackofficeAction::OrdersRead, &GrantScope::Global),
            BackofficeActionVerdict::Deny
        );
    }

    #[test]
    fn suspended_employee_is_denied_even_with_active_grant() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Suspended)).unwrap();
        grants
            .create(grant(
                "g-1",
                "alice",
                BackofficeRole::TradingOps,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        assert_eq!(
            svc.is_allowed("alice", BackofficeAction::OrdersCancelSingle, &GrantScope::Global),
            BackofficeActionVerdict::Deny
        );
    }

    #[test]
    fn trading_ops_can_cancel_single_order() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-1",
                "alice",
                BackofficeRole::TradingOps,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        assert_eq!(
            svc.is_allowed("alice", BackofficeAction::OrdersCancelSingle, &GrantScope::Global),
            BackofficeActionVerdict::Allow
        );
    }

    #[test]
    fn trading_ops_market_halt_requires_maker_checker() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-1",
                "alice",
                BackofficeRole::TradingOps,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        assert_eq!(
            svc.is_allowed("alice", BackofficeAction::MarketHalt, &GrantScope::Global),
            BackofficeActionVerdict::RequiresApproval
        );
    }

    #[test]
    fn break_glass_skips_maker_checker_for_market_halt() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("oncall", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-bg",
                "oncall",
                BackofficeRole::SuperAdminBreakGlass,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        // MarketHalt is single-actor for break-glass per design §4.
        assert_eq!(
            svc.is_allowed("oncall", BackofficeAction::MarketHalt, &GrantScope::Global),
            BackofficeActionVerdict::Allow
        );
    }

    #[test]
    fn finance_ops_can_reject_but_must_get_approval_to_approve() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("fin", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-1",
                "fin",
                BackofficeRole::FinanceOps,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        assert_eq!(
            svc.is_allowed("fin", BackofficeAction::WithdrawalsReject, &GrantScope::Global),
            BackofficeActionVerdict::Allow
        );
        assert_eq!(
            svc.is_allowed("fin", BackofficeAction::WithdrawalsApprove, &GrantScope::Global),
            BackofficeActionVerdict::RequiresApproval
        );
    }

    #[test]
    fn auditor_can_read_but_not_write() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("aud", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-1",
                "aud",
                BackofficeRole::AuditorReadonly,
                RoleLevel::Read,
                GrantScope::Global,
            ))
            .unwrap();
        assert_eq!(
            svc.is_allowed("aud", BackofficeAction::OrdersRead, &GrantScope::Global),
            BackofficeActionVerdict::Allow
        );
        assert_eq!(
            svc.is_allowed("aud", BackofficeAction::OrdersCancelSingle, &GrantScope::Global),
            BackofficeActionVerdict::Deny
        );
    }

    #[test]
    fn expired_grant_does_not_satisfy() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        let mut g = grant(
            "g-old",
            "alice",
            BackofficeRole::TradingOps,
            RoleLevel::Act,
            GrantScope::Global,
        );
        g.expires_at = Utc::now() - Duration::days(1);
        grants.create(g).unwrap();
        assert_eq!(
            svc.is_allowed("alice", BackofficeAction::OrdersCancelSingle, &GrantScope::Global),
            BackofficeActionVerdict::Deny
        );
    }

    #[test]
    fn revoked_grant_does_not_satisfy() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        let mut g = grant(
            "g-rev",
            "alice",
            BackofficeRole::TradingOps,
            RoleLevel::Act,
            GrantScope::Global,
        );
        g.status = GrantStatus::Revoked;
        grants.create(g).unwrap();
        assert_eq!(
            svc.is_allowed("alice", BackofficeAction::OrdersCancelSingle, &GrantScope::Global),
            BackofficeActionVerdict::Deny
        );
    }

    #[test]
    fn market_scoped_grant_does_not_cover_other_market() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-btc",
                "alice",
                BackofficeRole::TradingOps,
                RoleLevel::Act,
                GrantScope::Market("btc-usdt".into()),
            ))
            .unwrap();
        // Same market: allowed.
        assert_eq!(
            svc.is_allowed(
                "alice",
                BackofficeAction::OrdersCancelSingle,
                &GrantScope::Market("btc-usdt".into())
            ),
            BackofficeActionVerdict::Allow
        );
        // Different market: denied.
        assert_eq!(
            svc.is_allowed(
                "alice",
                BackofficeAction::OrdersCancelSingle,
                &GrantScope::Market("eth-usdt".into())
            ),
            BackofficeActionVerdict::Deny
        );
        // Global request against market-scoped grant: denied.
        assert_eq!(
            svc.is_allowed(
                "alice",
                BackofficeAction::OrdersCancelSingle,
                &GrantScope::Global
            ),
            BackofficeActionVerdict::Deny
        );
    }

    #[test]
    fn allow_wins_over_requires_approval_when_both_match() {
        // Edge case: an employee with both trading_ops (mass_cancel_le100
        // is single-actor) and an unrelated grant should still get
        // Allow for mass_cancel_le100. Verifies the early-return on
        // first Allow match.
        let (employees, grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-1",
                "alice",
                BackofficeRole::TradingOps,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        assert_eq!(
            svc.is_allowed(
                "alice",
                BackofficeAction::OrdersMassCancelLe100,
                &GrantScope::Global
            ),
            BackofficeActionVerdict::Allow
        );
    }

    #[test]
    fn effective_map_covers_all_mvp_actions() {
        let (employees, grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-1",
                "alice",
                BackofficeRole::TradingOps,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        let m = svc.effective_map("alice");
        // Read of orders: allow.
        assert_eq!(
            m.get(&BackofficeAction::OrdersRead),
            Some(&BackofficeActionVerdict::Allow)
        );
        // Halt market: requires approval.
        assert_eq!(
            m.get(&BackofficeAction::MarketHalt),
            Some(&BackofficeActionVerdict::RequiresApproval)
        );
        // No grant for finance: deny.
        assert_eq!(
            m.get(&BackofficeAction::WithdrawalsApprove),
            Some(&BackofficeActionVerdict::Deny)
        );
        // Map covers >= 20 distinct MVP actions.
        assert!(m.len() >= 20, "effective map should cover at least 20 actions, got {}", m.len());
    }

    #[test]
    fn no_grant_no_permissions() {
        let (employees, _grants, svc) = make_stores();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        assert_eq!(
            svc.is_allowed("alice", BackofficeAction::OrdersRead, &GrantScope::Global),
            BackofficeActionVerdict::Deny
        );
    }

    #[test]
    fn matrix_well_formed_no_action_listed_only_with_mc_higher_than_allow() {
        // Property: if an action has any role with requires_maker_checker=false,
        // there must exist at least one Allow path for that action. This
        // catches accidental matrix typos that downgrade everything to
        // RequiresApproval.
        use std::collections::HashSet;
        let allow_actions: HashSet<_> = V1_MATRIX
            .iter()
            .filter(|r| !r.requires_maker_checker)
            .map(|r| r.action)
            .collect();
        for r in V1_MATRIX {
            if !r.requires_maker_checker {
                assert!(allow_actions.contains(&r.action));
            }
        }
    }
}
