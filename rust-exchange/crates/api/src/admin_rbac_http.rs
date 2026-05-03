// Step 1D scaffold: read-only handlers compile and are exercised by
// unit tests via warp::test. Write paths (POST /admin/employees,
// POST /admin/employees/{id}/roles) land in Step 3 alongside the
// maker-checker flow so they can be properly gated. 1E will mount
// these routes in main.rs's route assembly.
#![allow(dead_code)]

//! Backoffice RBAC REST handlers (read-only v1).
//!
//! Step 1D of the RBAC MVP delivery (per docs/BACKOFFICE_RBAC_DESIGN.md
//! §7). Two endpoints in this commit:
//!
//! - `GET /admin/me/permissions` — calling employee's own grants +
//!   the `(action -> verdict)` effective map for client-side button
//!   gating. Always allowed for any authenticated employee principal
//!   (a missing grant set returns an empty map, not a 403).
//! - `GET /admin/employees` — list all employees and their grants.
//!   Gated on `BackofficeAction::EmployeesList`.
//!
//! All endpoints require the existing internal-HMAC `with_principal()`
//! filter to identify the caller. The caller's `principal.subject` is
//! used as the `EmployeeId` lookup key against the employee store.
//! v1 design §7 preamble says employee endpoints additionally require
//! an MFA-gated session cookie — that hardening is deferred to a
//! follow-up commit (the auth scheme upgrade is out of scope for the
//! RBAC MVP).

use std::sync::Arc;

use serde::Serialize;
use warp::{filters::BoxedFilter, Filter, Rejection};

use types::{
    AuthenticatedPrincipal, BackofficeAction, BackofficeActionVerdict, Employee, GrantScope,
};

use crate::admin_authz::AuthzService;
use crate::admin_rbac_store::{AdminEmployeeStore, AdminGrantStore};

#[derive(Debug, Serialize)]
pub(crate) struct MyPermissionsResponse {
    pub(crate) employee_id: String,
    /// Snapshot of all grants currently on record for this employee
    /// (active + non-active). Useful for the operator workspace's
    /// "my access" page.
    pub(crate) grants: Vec<types::Grant>,
    /// Effective `action -> verdict` map. Keys are the snake_case
    /// wire form of `BackofficeAction`; values are the snake_case
    /// wire form of `BackofficeActionVerdict`.
    pub(crate) effective: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct EmployeeListEntry {
    #[serde(flatten)]
    pub(crate) employee: Employee,
    pub(crate) active_grants: Vec<types::Grant>,
}

#[derive(Debug, Serialize)]
pub(crate) struct EmployeeListResponse {
    pub(crate) employees: Vec<EmployeeListEntry>,
    pub(crate) total: usize,
}

fn with_arc<T: Clone + Send + Sync>(
    value: T,
) -> impl Filter<Extract = (T,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || value.clone())
}

/// Build the v1 read-only RBAC routes wrapped under `auth`. Caller
/// supplies the principal-extracting filter (production passes
/// `crate::security::with_principal()`; tests pass a stub that
/// injects a fixed principal).
pub(crate) fn build_admin_rbac_routes<F>(
    employees: Arc<AdminEmployeeStore>,
    grants: Arc<AdminGrantStore>,
    authz: Arc<AuthzService>,
    auth: F,
) -> BoxedFilter<(warp::reply::Json,)>
where
    F: Filter<Extract = (AuthenticatedPrincipal,), Error = Rejection>
        + Clone
        + Send
        + Sync
        + 'static,
{
    let me = warp::path!("admin" / "me" / "permissions")
        .and(warp::get())
        .and(auth.clone())
        .and(with_arc(authz.clone()))
        .and(with_arc(grants.clone()))
        .and_then(handle_my_permissions)
        .boxed();

    let list = warp::path!("admin" / "employees")
        .and(warp::get())
        .and(auth.clone())
        .and(with_arc(authz))
        .and(with_arc(employees))
        .and(with_arc(grants))
        .and_then(handle_list_employees)
        .boxed();

    me.or(list).unify().boxed()
}

pub(crate) async fn handle_my_permissions(
    principal: AuthenticatedPrincipal,
    authz: Arc<AuthzService>,
    grants: Arc<AdminGrantStore>,
) -> Result<warp::reply::Json, Rejection> {
    let employee_id = principal.subject.clone();
    let grants_snapshot = grants.for_employee(&employee_id);
    let effective_raw = authz.effective_map(&employee_id);
    // Translate the typed map into wire-form strings for stable JSON.
    let mut effective = std::collections::BTreeMap::new();
    for (action, verdict) in effective_raw {
        let action_key = strip_quotes(serde_json::to_string(&action).unwrap_or_default());
        let verdict_value = strip_quotes(serde_json::to_string(&verdict).unwrap_or_default());
        effective.insert(action_key, verdict_value);
    }
    let resp = MyPermissionsResponse {
        employee_id,
        grants: grants_snapshot,
        effective,
    };
    Ok(warp::reply::json(&resp))
}

pub(crate) async fn handle_list_employees(
    principal: AuthenticatedPrincipal,
    authz: Arc<AuthzService>,
    employees: Arc<AdminEmployeeStore>,
    grants: Arc<AdminGrantStore>,
) -> Result<warp::reply::Json, Rejection> {
    if authz.is_allowed(
        &principal.subject,
        BackofficeAction::EmployeesList,
        &GrantScope::Global,
    ) != BackofficeActionVerdict::Allow
    {
        return Err(warp::reject::not_found());
    }
    let list = employees.list();
    let total = list.len();
    let entries: Vec<EmployeeListEntry> = list
        .into_iter()
        .map(|e| {
            let active = grants.active_for_employee(&e.employee_id);
            EmployeeListEntry {
                employee: e,
                active_grants: active,
            }
        })
        .collect();
    Ok(warp::reply::json(&EmployeeListResponse {
        employees: entries,
        total,
    }))
}

fn strip_quotes(mut s: String) -> String {
    if s.starts_with('"') {
        s.remove(0);
    }
    if s.ends_with('"') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use persistence::InMemoryWal;
    use types::{
        BackofficeRole, EmployeeStatus, Grant, GrantStatus, MfaMethod, PrincipalRole, RoleLevel,
        BACKOFFICE_SCHEMA_VERSION,
    };
    use warp::http::StatusCode;
    use warp::Reply;

    fn ts(secs: i64) -> chrono::DateTime<Utc> {
        chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000 + secs, 0).unwrap()
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

    fn make_pieces() -> (Arc<AdminEmployeeStore>, Arc<AdminGrantStore>, Arc<AuthzService>) {
        let employees = Arc::new(AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let grants = Arc::new(AdminGrantStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let authz = Arc::new(AuthzService::new(employees.clone(), grants.clone()));
        (employees, grants, authz)
    }

    fn stub_principal(subject: &'static str, role: PrincipalRole) -> impl Filter<Extract = (AuthenticatedPrincipal,), Error = Rejection> + Clone {
        warp::any().and_then(move || async move {
            Ok::<_, Rejection>(AuthenticatedPrincipal {
                subject: subject.into(),
                role,
                session_id: None,
            })
        })
    }

    async fn body_json(reply: warp::reply::Json) -> serde_json::Value {
        let bytes = warp::hyper::body::to_bytes(reply.into_response().into_body())
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn expect_not_found(r: Result<warp::reply::Json, Rejection>) {
        match r {
            Ok(_) => panic!("expected NotFound, got Ok"),
            Err(rej) => assert!(rej.is_not_found(), "expected NotFound, got {rej:?}"),
        }
    }

    #[tokio::test]
    async fn me_permissions_returns_empty_for_unknown_employee() {
        let (_employees, grants, authz) = make_pieces();
        let principal = AuthenticatedPrincipal {
            subject: "ghost".into(),
            role: PrincipalRole::Admin,
            session_id: None,
        };
        let reply = handle_my_permissions(principal, authz, grants).await.unwrap();
        let json = body_json(reply).await;
        assert_eq!(json["employee_id"].as_str().unwrap(), "ghost");
        assert_eq!(json["grants"].as_array().unwrap().len(), 0);
        // Effective map is non-empty (lists every MVP action) but every
        // value is "deny" since the employee is unknown.
        let effective = json["effective"].as_object().unwrap();
        assert!(effective.len() >= 20);
        for (_action, verdict) in effective {
            assert_eq!(verdict.as_str().unwrap(), "deny");
        }
    }

    #[tokio::test]
    async fn me_permissions_returns_grants_and_allow_verdicts() {
        let (employees, grants, authz) = make_pieces();
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
        let principal = AuthenticatedPrincipal {
            subject: "alice".into(),
            role: PrincipalRole::Admin,
            session_id: None,
        };
        let reply = handle_my_permissions(principal, authz, grants).await.unwrap();
        let json = body_json(reply).await;
        assert_eq!(json["employee_id"].as_str().unwrap(), "alice");
        let snapshot = json["grants"].as_array().unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0]["role"].as_str().unwrap(), "trading_ops");
        let effective = json["effective"].as_object().unwrap();
        assert_eq!(effective["orders_cancel_single"].as_str().unwrap(), "allow");
        assert_eq!(effective["market_halt"].as_str().unwrap(), "requires_approval");
        assert_eq!(effective["withdrawals_approve"].as_str().unwrap(), "deny");
    }

    #[tokio::test]
    async fn list_employees_denied_without_permission() {
        let (employees, grants, authz) = make_pieces();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        // Alice has trading_ops, NOT auditor or break_glass — so
        // EmployeesList is denied.
        grants
            .create(grant(
                "g-1",
                "alice",
                BackofficeRole::TradingOps,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        let principal = AuthenticatedPrincipal {
            subject: "alice".into(),
            role: PrincipalRole::Admin,
            session_id: None,
        };
        expect_not_found(handle_list_employees(principal, authz, employees, grants).await);
    }

    #[tokio::test]
    async fn list_employees_allowed_for_auditor() {
        let (employees, grants, authz) = make_pieces();
        employees.create(employee("aud", EmployeeStatus::Active)).unwrap();
        employees.create(employee("alice", EmployeeStatus::Active)).unwrap();
        employees.create(employee("bob", EmployeeStatus::Suspended)).unwrap();
        grants
            .create(grant(
                "g-aud",
                "aud",
                BackofficeRole::AuditorReadonly,
                RoleLevel::Read,
                GrantScope::Global,
            ))
            .unwrap();
        grants
            .create(grant(
                "g-trade",
                "alice",
                BackofficeRole::TradingOps,
                RoleLevel::Act,
                GrantScope::Global,
            ))
            .unwrap();
        let principal = AuthenticatedPrincipal {
            subject: "aud".into(),
            role: PrincipalRole::Admin,
            session_id: None,
        };
        let reply = handle_list_employees(principal, authz, employees, grants).await.unwrap();
        let json = body_json(reply).await;
        assert_eq!(json["total"].as_u64().unwrap(), 3);
        let arr = json["employees"].as_array().unwrap();
        // Listed in employee_id sort order from the store.
        assert_eq!(arr[0]["employee_id"].as_str().unwrap(), "alice");
        assert_eq!(arr[1]["employee_id"].as_str().unwrap(), "aud");
        assert_eq!(arr[2]["employee_id"].as_str().unwrap(), "bob");
        // alice has one active grant (trading_ops); bob has none; aud has one.
        assert_eq!(arr[0]["active_grants"].as_array().unwrap().len(), 1);
        assert_eq!(arr[2]["active_grants"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn build_admin_rbac_routes_smoke_warp_test() {
        let (employees, grants, authz) = make_pieces();
        employees.create(employee("aud", EmployeeStatus::Active)).unwrap();
        grants
            .create(grant(
                "g-aud",
                "aud",
                BackofficeRole::AuditorReadonly,
                RoleLevel::Read,
                GrantScope::Global,
            ))
            .unwrap();
        let routes = build_admin_rbac_routes(
            employees,
            grants,
            authz,
            stub_principal("aud", PrincipalRole::Admin),
        );

        // GET /admin/me/permissions
        let resp = warp::test::request()
            .method("GET")
            .path("/admin/me/permissions")
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["employee_id"].as_str().unwrap(), "aud");
        assert_eq!(
            body["effective"]["orders_read"].as_str().unwrap(),
            "allow"
        );

        // GET /admin/employees
        let resp = warp::test::request()
            .method("GET")
            .path("/admin/employees")
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["total"].as_u64().unwrap(), 1);
    }
}
