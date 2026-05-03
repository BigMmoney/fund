// Step 3 scaffold: handlers compile + tests verify the flow. 1E will
// mount these into main.rs's route assembly. The downstream action
// handlers (e.g. Step 5 market.halt) consult ApprovalRequestStore via
// `find_committed_approval` before committing.
#![allow(dead_code)]

//! Backoffice maker-checker approval flow.
//!
//! Step 3 of the RBAC MVP (per docs/BACKOFFICE_RBAC_DESIGN.md §5
//! and §7). Three endpoints over the ApprovalRequestStore from
//! Step 1B + the audit log from Step 2:
//!
//! - `POST /admin/approval-requests` — submit a new request.
//! - `POST /admin/approval-requests/{id}/approve` — approver commits.
//! - `POST /admin/approval-requests/{id}/reject` — approver rejects.
//! - `GET  /admin/approval-requests` — list pending (newest first).
//!
//! Server checks per design §5.2:
//! - Submitter must hold a grant that satisfies the target action
//!   (Allow or RequiresApproval) — Deny means they can't submit
//!   either.
//! - Reason 16-512 chars, non-whitespace.
//! - Approver must hold `act+MC` for the action (RequiresApproval
//!   verdict).
//! - Approver MUST NOT equal submitter (`denied_self_approval`).
//! - Request must still be `Pending` (not expired, not already
//!   approved/rejected).
//!
//! Underlying-action commit is intentionally NOT performed inside
//! the approve handler. The approve handler flips the request status
//! to Approved; the actual action's downstream handler (e.g. Step 5
//! `POST /admin/market/halt`) checks for a matching Approved request
//! before committing. This 3-step flow keeps the approval layer
//! generic across action types.

use std::sync::Arc;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use warp::{filters::BoxedFilter, Filter, Rejection};

use types::{
    ApprovalRequest, ApprovalRequestStatus, AuditApprovalSummary, AuditDecision, AuditOutcome,
    AuthenticatedPrincipal, BackofficeAction, BackofficeActionVerdict, GrantScope, MfaMethod,
    ResourceRef, BACKOFFICE_SCHEMA_VERSION,
};

use crate::admin_authz::AuthzService;
use crate::admin_rbac_audit::{AdminRbacAuditStore, DecisionRecord};
use crate::admin_rbac_store::ApprovalRequestStore;

const MIN_REASON_LEN: usize = 16;
const MAX_REASON_LEN: usize = 512;
const DEFAULT_EXPIRES_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Deserialize)]
pub(crate) struct SubmitApprovalRequestBody {
    pub action: BackofficeAction,
    pub resource: ResourceRef,
    #[serde(default = "default_scope")]
    pub scope: GrantScope,
    pub reason: String,
    pub action_payload: serde_json::Value,
    #[serde(default)]
    pub expires_in_seconds: Option<i64>,
}

fn default_scope() -> GrantScope {
    GrantScope::Global
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApproveOrRejectBody {
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApprovalRequestSummary {
    pub status: String,
    pub approval_request_id: String,
    pub expires_at: chrono::DateTime<Utc>,
}

fn fresh_request_id() -> String {
    format!("appr-{}", uuid::Uuid::new_v4())
}

fn validate_reason(reason: &str) -> Result<(), String> {
    let trimmed = reason.trim();
    if trimmed.len() < MIN_REASON_LEN {
        return Err(format!(
            "reason too short: must be ≥{MIN_REASON_LEN} non-whitespace chars"
        ));
    }
    if reason.len() > MAX_REASON_LEN {
        return Err(format!("reason too long: must be ≤{MAX_REASON_LEN} chars"));
    }
    Ok(())
}

fn with_arc<T: Clone + Send + Sync>(
    value: T,
) -> impl Filter<Extract = (T,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || value.clone())
}

pub(crate) fn build_admin_approvals_routes<F>(
    requests: Arc<ApprovalRequestStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
    auth: F,
) -> BoxedFilter<(warp::reply::Json,)>
where
    F: Filter<Extract = (AuthenticatedPrincipal,), Error = Rejection>
        + Clone
        + Send
        + Sync
        + 'static,
{
    let submit = warp::path!("admin" / "approval-requests")
        .and(warp::post())
        .and(auth.clone())
        .and(warp::body::json())
        .and(with_arc(requests.clone()))
        .and(with_arc(authz.clone()))
        .and(with_arc(audit.clone()))
        .and_then(handle_submit)
        .boxed();

    let approve = warp::path!("admin" / "approval-requests" / String / "approve")
        .and(warp::post())
        .and(auth.clone())
        .and(warp::body::json())
        .and(with_arc(requests.clone()))
        .and(with_arc(authz.clone()))
        .and(with_arc(audit.clone()))
        .and_then(handle_approve)
        .boxed();

    let reject = warp::path!("admin" / "approval-requests" / String / "reject")
        .and(warp::post())
        .and(auth.clone())
        .and(warp::body::json())
        .and(with_arc(requests.clone()))
        .and(with_arc(audit.clone()))
        .and_then(handle_reject)
        .boxed();

    let list = warp::path!("admin" / "approval-requests")
        .and(warp::get())
        .and(auth)
        .and(with_arc(requests))
        .and_then(handle_list_pending)
        .boxed();

    submit.or(approve).unify().or(reject).unify().or(list).unify().boxed()
}

pub(crate) async fn handle_submit(
    principal: AuthenticatedPrincipal,
    body: SubmitApprovalRequestBody,
    requests: Arc<ApprovalRequestStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    if let Err(msg) = validate_reason(&body.reason) {
        let _ = audit_simple(
            &audit,
            &principal,
            body.action,
            body.resource.clone(),
            body.scope.clone(),
            &body.reason,
            AuditDecision::DeniedAuthz,
            Some(msg),
            AuditOutcome::Failure,
        );
        return Err(warp::reject::not_found());
    }
    let verdict = authz.is_allowed(&principal.subject, body.action, &body.scope);
    if verdict == BackofficeActionVerdict::Deny {
        let _ = audit_simple(
            &audit,
            &principal,
            body.action,
            body.resource.clone(),
            body.scope.clone(),
            &body.reason,
            AuditDecision::DeniedAuthz,
            Some("submitter has no grant for action".into()),
            AuditOutcome::Failure,
        );
        return Err(warp::reject::not_found());
    }
    let now = Utc::now();
    let expires_secs = body
        .expires_in_seconds
        .unwrap_or(DEFAULT_EXPIRES_SECONDS)
        .max(60)
        .min(7 * 24 * 60 * 60);
    let req = ApprovalRequest {
        schema_version: BACKOFFICE_SCHEMA_VERSION,
        approval_request_id: fresh_request_id(),
        action: body.action,
        resource: body.resource.clone(),
        scope: body.scope.clone(),
        submitter_employee_id: principal.subject.clone(),
        submitter_reason: body.reason.clone(),
        submitted_at: now,
        expires_at: now + Duration::seconds(expires_secs),
        status: ApprovalRequestStatus::Pending,
        approver_employee_id: None,
        approver_reason: None,
        decided_at: None,
        action_payload: body.action_payload.clone(),
    };
    if let Err(e) = requests.create(req.clone()) {
        let _ = audit_simple(
            &audit,
            &principal,
            body.action,
            body.resource.clone(),
            body.scope.clone(),
            &body.reason,
            AuditDecision::DeniedAuditWriteFailure,
            Some(e.to_string()),
            AuditOutcome::Failure,
        );
        return Err(warp::reject::not_found());
    }
    let _ = audit.record(DecisionRecord {
        principal: &principal,
        remote_ip: "",
        user_agent: "",
        mfa_method: MfaMethod::Webauthn,
        action: body.action,
        resource: body.resource,
        scope: body.scope,
        reason: &body.reason,
        decision: AuditDecision::PendingApproval,
        decision_reason: None,
        approval_request_id: Some(req.approval_request_id.clone()),
        approval: None,
        break_glass_session_id: None,
        incident_reference: None,
        outcome: AuditOutcome::Success,
        outcome_detail: None,
    });
    Ok(warp::reply::json(&ApprovalRequestSummary {
        status: "pending".into(),
        approval_request_id: req.approval_request_id,
        expires_at: req.expires_at,
    }))
}

pub(crate) async fn handle_approve(
    request_id: String,
    principal: AuthenticatedPrincipal,
    body: ApproveOrRejectBody,
    requests: Arc<ApprovalRequestStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    if let Err(_msg) = validate_reason(&body.reason) {
        return Err(warp::reject::not_found());
    }
    let mut req = match requests.get(&request_id) {
        Some(r) => r,
        None => return Err(warp::reject::not_found()),
    };
    if req.status != ApprovalRequestStatus::Pending || req.expires_at <= Utc::now() {
        return Err(warp::reject::not_found());
    }
    // Self-approval prohibited (design §9 rule 3).
    if req.submitter_employee_id == principal.subject {
        let _ = audit_simple(
            &audit,
            &principal,
            req.action,
            req.resource.clone(),
            req.scope.clone(),
            &body.reason,
            AuditDecision::DeniedSelfApproval,
            Some("approver_employee_id == submitter_employee_id".into()),
            AuditOutcome::Failure,
        );
        return Err(warp::reject::not_found());
    }
    // Approver must hold act+MC for the action (a RequiresApproval
    // verdict means they could only commit via this exact path; an
    // Allow verdict means they could do it single-actor too — also
    // acceptable as an approver).
    let approver_verdict = authz.is_allowed(&principal.subject, req.action, &req.scope);
    if approver_verdict == BackofficeActionVerdict::Deny {
        let _ = audit_simple(
            &audit,
            &principal,
            req.action,
            req.resource.clone(),
            req.scope.clone(),
            &body.reason,
            AuditDecision::DeniedAuthz,
            Some("approver lacks act+MC for action".into()),
            AuditOutcome::Failure,
        );
        return Err(warp::reject::not_found());
    }
    let now = Utc::now();
    req.status = ApprovalRequestStatus::Approved;
    req.approver_employee_id = Some(principal.subject.clone());
    req.approver_reason = Some(body.reason.clone());
    req.decided_at = Some(now);
    if let Err(e) = requests.update(req.clone()) {
        let _ = audit_simple(
            &audit,
            &principal,
            req.action,
            req.resource.clone(),
            req.scope.clone(),
            &body.reason,
            AuditDecision::DeniedAuditWriteFailure,
            Some(e.to_string()),
            AuditOutcome::Failure,
        );
        return Err(warp::reject::not_found());
    }
    let approval_summary = AuditApprovalSummary {
        submitter_employee_id: req.submitter_employee_id.clone(),
        submitter_reason: req.submitter_reason.clone(),
        approver_employee_id: principal.subject.clone(),
        approver_reason: body.reason.clone(),
        approved_at: now,
    };
    let _ = audit.record(DecisionRecord {
        principal: &principal,
        remote_ip: "",
        user_agent: "",
        mfa_method: MfaMethod::Webauthn,
        action: req.action,
        resource: req.resource.clone(),
        scope: req.scope.clone(),
        reason: &body.reason,
        decision: AuditDecision::Committed,
        decision_reason: None,
        approval_request_id: Some(req.approval_request_id.clone()),
        approval: Some(approval_summary),
        break_glass_session_id: None,
        incident_reference: None,
        outcome: AuditOutcome::Success,
        outcome_detail: None,
    });
    Ok(warp::reply::json(&serde_json::json!({
        "status": "approved",
        "approval_request_id": req.approval_request_id,
    })))
}

pub(crate) async fn handle_reject(
    request_id: String,
    principal: AuthenticatedPrincipal,
    body: ApproveOrRejectBody,
    requests: Arc<ApprovalRequestStore>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    if let Err(_msg) = validate_reason(&body.reason) {
        return Err(warp::reject::not_found());
    }
    let mut req = match requests.get(&request_id) {
        Some(r) => r,
        None => return Err(warp::reject::not_found()),
    };
    if req.status != ApprovalRequestStatus::Pending || req.expires_at <= Utc::now() {
        return Err(warp::reject::not_found());
    }
    // Reject does NOT require non-self-approval — a submitter can
    // withdraw their own request by rejecting it.
    let now = Utc::now();
    req.status = ApprovalRequestStatus::Rejected;
    req.approver_employee_id = Some(principal.subject.clone());
    req.approver_reason = Some(body.reason.clone());
    req.decided_at = Some(now);
    if let Err(e) = requests.update(req.clone()) {
        return Err(warp::reject::custom(crate::admin_approvals_http::AuditWriteFail(
            e.to_string(),
        )));
    }
    let _ = audit.record(DecisionRecord {
        principal: &principal,
        remote_ip: "",
        user_agent: "",
        mfa_method: MfaMethod::Webauthn,
        action: req.action,
        resource: req.resource,
        scope: req.scope,
        reason: &body.reason,
        decision: AuditDecision::RejectedByApprover,
        decision_reason: None,
        approval_request_id: Some(req.approval_request_id.clone()),
        approval: None,
        break_glass_session_id: None,
        incident_reference: None,
        outcome: AuditOutcome::Success,
        outcome_detail: None,
    });
    Ok(warp::reply::json(&serde_json::json!({
        "status": "rejected",
        "approval_request_id": req.approval_request_id,
    })))
}

pub(crate) async fn handle_list_pending(
    _principal: AuthenticatedPrincipal,
    requests: Arc<ApprovalRequestStore>,
) -> Result<warp::reply::Json, Rejection> {
    let pending = requests.pending();
    Ok(warp::reply::json(&serde_json::json!({
        "pending": pending,
        "total": pending.len(),
    })))
}

#[derive(Debug)]
struct AuditWriteFail(String);
impl warp::reject::Reject for AuditWriteFail {}

fn audit_simple(
    audit: &AdminRbacAuditStore,
    principal: &AuthenticatedPrincipal,
    action: BackofficeAction,
    resource: ResourceRef,
    scope: GrantScope,
    reason: &str,
    decision: AuditDecision,
    decision_reason: Option<String>,
    outcome: AuditOutcome,
) -> anyhow::Result<()> {
    audit.record(DecisionRecord {
        principal,
        remote_ip: "",
        user_agent: "",
        mfa_method: MfaMethod::Webauthn,
        action,
        resource,
        scope,
        reason,
        decision,
        decision_reason,
        approval_request_id: None,
        approval: None,
        break_glass_session_id: None,
        incident_reference: None,
        outcome,
        outcome_detail: None,
    })?;
    Ok(())
}

/// Look up a committed-and-not-yet-consumed approval for the given
/// (action, resource, requested-by) tuple. Used by downstream action
/// handlers (Step 5+) to gate their commit on a matching Approved
/// request. v1 does NOT mark the request as consumed — a single
/// Approved request can underpin multiple commits, which is a v1
/// limitation noted in the design's open questions.
pub(crate) fn find_committed_approval(
    requests: &ApprovalRequestStore,
    action: BackofficeAction,
    resource: &ResourceRef,
    requested_by: &str,
) -> Option<ApprovalRequest> {
    requests
        .list()
        .into_iter()
        .find(|r| {
            r.status == ApprovalRequestStatus::Approved
                && r.action == action
                && &r.resource == resource
                && r.submitter_employee_id == requested_by
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::InMemoryWal;
    use types::{
        BackofficeRole, Employee, EmployeeStatus, Grant, GrantStatus, PrincipalRole, RoleLevel,
    };
    use crate::admin_rbac_store::{AdminEmployeeStore, AdminGrantStore};

    fn ts(secs: i64) -> chrono::DateTime<Utc> {
        chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000 + secs, 0).unwrap()
    }

    fn employee(id: &str) -> Employee {
        Employee {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            employee_id: id.into(),
            display_name: id.into(),
            status: EmployeeStatus::Active,
            created_at: ts(0),
            updated_at: ts(0),
            last_mfa_method: Some(MfaMethod::Webauthn),
            last_login_at: Some(ts(0)),
        }
    }

    fn grant(id: &str, employee_id: &str, role: BackofficeRole, level: RoleLevel) -> Grant {
        Grant {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            grant_id: id.into(),
            employee_id: employee_id.into(),
            role,
            level,
            scope: GrantScope::Global,
            status: GrantStatus::Active,
            granted_by: "secadmin".into(),
            granted_at: ts(0),
            expires_at: Utc::now() + Duration::days(30),
            reason: "test grant".into(),
            approval_request_id: None,
        }
    }

    fn principal(subject: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            subject: subject.into(),
            role: PrincipalRole::Admin,
            session_id: None,
        }
    }

    fn make_world() -> (
        Arc<AdminEmployeeStore>,
        Arc<AdminGrantStore>,
        Arc<AuthzService>,
        Arc<AdminRbacAuditStore>,
        Arc<ApprovalRequestStore>,
    ) {
        let employees = Arc::new(AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let grants = Arc::new(AdminGrantStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let authz = Arc::new(AuthzService::new(employees.clone(), grants.clone()));
        let audit = Arc::new(AdminRbacAuditStore::new(Arc::new(InMemoryWal::new())));
        let requests = Arc::new(ApprovalRequestStore::new(Arc::new(InMemoryWal::new())).unwrap());
        (employees, grants, authz, audit, requests)
    }

    fn submit_body(action: BackofficeAction, reason: &str) -> SubmitApprovalRequestBody {
        SubmitApprovalRequestBody {
            action,
            resource: ResourceRef {
                kind: "market".into(),
                id: "btc-usdt".into(),
            },
            scope: GrantScope::Global,
            reason: reason.into(),
            action_payload: serde_json::json!({ "market_id": "btc-usdt" }),
            expires_in_seconds: None,
        }
    }

    #[tokio::test]
    async fn submit_rejects_short_reason() {
        let (_, _, authz, audit, requests) = make_world();
        let p = principal("alice");
        let body = submit_body(BackofficeAction::MarketHalt, "too short");
        let r = handle_submit(p.clone(), body, requests.clone(), authz, audit.clone()).await;
        assert!(r.is_err());
        // Audit row written for the denial.
        let rows = audit.entries().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].decision, AuditDecision::DeniedAuthz);
    }

    #[tokio::test]
    async fn submit_denied_when_no_grant_for_action() {
        let (employees, _grants, authz, audit, requests) = make_world();
        employees.create(employee("alice")).unwrap();
        // No grant — every action verdict is Deny.
        let p = principal("alice");
        let body = submit_body(
            BackofficeAction::MarketHalt,
            "halting for sanctions screening per ticket SEC-1",
        );
        let r = handle_submit(p, body, requests, authz, audit.clone()).await;
        assert!(r.is_err());
        assert_eq!(audit.entries().unwrap()[0].decision, AuditDecision::DeniedAuthz);
    }

    #[tokio::test]
    async fn submit_creates_pending_request_for_trading_ops_market_halt() {
        let (employees, grants, authz, audit, requests) = make_world();
        employees.create(employee("alice")).unwrap();
        grants
            .create(grant("g-1", "alice", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        let body = submit_body(
            BackofficeAction::MarketHalt,
            "halting btc-usdt per incident OPS-991 for risk review",
        );
        let reply = handle_submit(
            principal("alice"),
            body,
            requests.clone(),
            authz,
            audit.clone(),
        )
        .await
        .unwrap();
        let bytes = warp::hyper::body::to_bytes(warp::Reply::into_response(reply).into_body())
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"].as_str().unwrap(), "pending");
        let id = json["approval_request_id"].as_str().unwrap();
        let stored = requests.get(id).unwrap();
        assert_eq!(stored.status, ApprovalRequestStatus::Pending);
        assert_eq!(stored.submitter_employee_id, "alice");
        assert_eq!(stored.action, BackofficeAction::MarketHalt);
        // Audit: one row, PendingApproval / Success.
        let rows = audit.entries().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].decision, AuditDecision::PendingApproval);
        assert_eq!(rows[0].outcome, AuditOutcome::Success);
        assert_eq!(rows[0].approval_request_id.as_deref(), Some(id));
    }

    #[tokio::test]
    async fn approve_rejects_self_approval() {
        let (employees, grants, authz, audit, requests) = make_world();
        employees.create(employee("alice")).unwrap();
        grants
            .create(grant("g-1", "alice", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        // Alice submits.
        let submit_reply = handle_submit(
            principal("alice"),
            submit_body(
                BackofficeAction::MarketHalt,
                "halting btc-usdt per incident OPS-991 for risk review",
            ),
            requests.clone(),
            authz.clone(),
            audit.clone(),
        )
        .await
        .unwrap();
        let bytes = warp::hyper::body::to_bytes(warp::Reply::into_response(submit_reply).into_body())
            .await
            .unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(&bytes)
            .unwrap()["approval_request_id"]
            .as_str()
            .unwrap()
            .to_string();
        // Alice tries to approve her own request — denied.
        let r = handle_approve(
            id.clone(),
            principal("alice"),
            ApproveOrRejectBody {
                reason: "self-approving for the win, please".into(),
            },
            requests.clone(),
            authz,
            audit.clone(),
        )
        .await;
        assert!(r.is_err());
        // Request still pending.
        assert_eq!(requests.get(&id).unwrap().status, ApprovalRequestStatus::Pending);
        // Audit row recorded the self-approval denial.
        assert!(audit
            .entries()
            .unwrap()
            .iter()
            .any(|r| r.decision == AuditDecision::DeniedSelfApproval));
    }

    #[tokio::test]
    async fn approve_succeeds_when_different_employee_holds_action_grant() {
        let (employees, grants, authz, audit, requests) = make_world();
        employees.create(employee("alice")).unwrap();
        employees.create(employee("bob")).unwrap();
        grants
            .create(grant("g-1", "alice", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        grants
            .create(grant("g-2", "bob", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        let submit_reply = handle_submit(
            principal("alice"),
            submit_body(
                BackofficeAction::MarketHalt,
                "halting btc-usdt per incident OPS-991 for risk review",
            ),
            requests.clone(),
            authz.clone(),
            audit.clone(),
        )
        .await
        .unwrap();
        let bytes = warp::hyper::body::to_bytes(warp::Reply::into_response(submit_reply).into_body())
            .await
            .unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(&bytes)
            .unwrap()["approval_request_id"]
            .as_str()
            .unwrap()
            .to_string();
        // Bob approves.
        let approve_reply = handle_approve(
            id.clone(),
            principal("bob"),
            ApproveOrRejectBody {
                reason: "verified per OPS-991 incident notes confirming risk review".into(),
            },
            requests.clone(),
            authz,
            audit.clone(),
        )
        .await
        .unwrap();
        let abytes = warp::hyper::body::to_bytes(
            warp::Reply::into_response(approve_reply).into_body(),
        )
        .await
        .unwrap();
        let aj: serde_json::Value = serde_json::from_slice(&abytes).unwrap();
        assert_eq!(aj["status"].as_str().unwrap(), "approved");
        let stored = requests.get(&id).unwrap();
        assert_eq!(stored.status, ApprovalRequestStatus::Approved);
        assert_eq!(stored.approver_employee_id.as_deref(), Some("bob"));
        // Audit: a Committed row with the approval block.
        let committed = audit
            .entries()
            .unwrap()
            .into_iter()
            .find(|r| r.decision == AuditDecision::Committed)
            .expect("committed audit row");
        assert!(committed.approval.is_some());
        assert_eq!(committed.approval.as_ref().unwrap().approver_employee_id, "bob");
    }

    #[tokio::test]
    async fn approve_denied_when_request_not_pending() {
        let (employees, grants, authz, audit, requests) = make_world();
        employees.create(employee("alice")).unwrap();
        employees.create(employee("bob")).unwrap();
        grants
            .create(grant("g-1", "alice", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        grants
            .create(grant("g-2", "bob", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        // Submit + approve.
        let r = handle_submit(
            principal("alice"),
            submit_body(
                BackofficeAction::MarketHalt,
                "halting btc-usdt per incident OPS-991 for risk review",
            ),
            requests.clone(),
            authz.clone(),
            audit.clone(),
        )
        .await
        .unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(
            &warp::hyper::body::to_bytes(warp::Reply::into_response(r).into_body())
                .await
                .unwrap(),
        )
        .unwrap()["approval_request_id"]
            .as_str()
            .unwrap()
            .to_string();
        let _ = handle_approve(
            id.clone(),
            principal("bob"),
            ApproveOrRejectBody {
                reason: "verified per OPS-991 incident notes confirming risk review".into(),
            },
            requests.clone(),
            authz.clone(),
            audit.clone(),
        )
        .await
        .unwrap();
        // A second approve attempt fails (not pending anymore).
        let r2 = handle_approve(
            id,
            principal("bob"),
            ApproveOrRejectBody {
                reason: "trying to double-approve which should fail".into(),
            },
            requests,
            authz,
            audit,
        )
        .await;
        assert!(r2.is_err());
    }

    #[tokio::test]
    async fn reject_flips_status_and_writes_audit_row() {
        let (employees, grants, authz, audit, requests) = make_world();
        employees.create(employee("alice")).unwrap();
        employees.create(employee("bob")).unwrap();
        grants
            .create(grant("g-1", "alice", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        grants
            .create(grant("g-2", "bob", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        let r = handle_submit(
            principal("alice"),
            submit_body(
                BackofficeAction::MarketHalt,
                "halting btc-usdt per incident OPS-991 for risk review",
            ),
            requests.clone(),
            authz,
            audit.clone(),
        )
        .await
        .unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(
            &warp::hyper::body::to_bytes(warp::Reply::into_response(r).into_body())
                .await
                .unwrap(),
        )
        .unwrap()["approval_request_id"]
            .as_str()
            .unwrap()
            .to_string();
        let rej = handle_reject(
            id.clone(),
            principal("bob"),
            ApproveOrRejectBody {
                reason: "incident is already resolved per OPS-991 closure note".into(),
            },
            requests.clone(),
            audit.clone(),
        )
        .await
        .unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &warp::hyper::body::to_bytes(warp::Reply::into_response(rej).into_body())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["status"].as_str().unwrap(), "rejected");
        assert_eq!(
            requests.get(&id).unwrap().status,
            ApprovalRequestStatus::Rejected
        );
        assert!(audit
            .entries()
            .unwrap()
            .iter()
            .any(|r| r.decision == AuditDecision::RejectedByApprover));
    }

    #[tokio::test]
    async fn list_pending_excludes_decided() {
        let (employees, grants, authz, audit, requests) = make_world();
        employees.create(employee("alice")).unwrap();
        employees.create(employee("bob")).unwrap();
        grants
            .create(grant("g-1", "alice", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        grants
            .create(grant("g-2", "bob", BackofficeRole::TradingOps, RoleLevel::Act))
            .unwrap();
        // Submit two requests.
        for tag in &["one", "two"] {
            let _ = handle_submit(
                principal("alice"),
                submit_body(
                    BackofficeAction::MarketHalt,
                    &format!("halting btc-usdt per incident OPS-991 case {tag}"),
                ),
                requests.clone(),
                authz.clone(),
                audit.clone(),
            )
            .await
            .unwrap();
        }
        // Approve one.
        let pending_ids: Vec<String> = requests
            .pending()
            .into_iter()
            .map(|r| r.approval_request_id)
            .collect();
        let _ = handle_approve(
            pending_ids[0].clone(),
            principal("bob"),
            ApproveOrRejectBody {
                reason: "verified per OPS-991 incident notes confirming risk review".into(),
            },
            requests.clone(),
            authz,
            audit,
        )
        .await
        .unwrap();
        // List should now have exactly one pending.
        let reply = handle_list_pending(principal("bob"), requests).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &warp::hyper::body::to_bytes(warp::Reply::into_response(reply).into_body())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["total"].as_u64().unwrap(), 1);
    }

    #[test]
    fn find_committed_approval_matches_action_resource_submitter() {
        let store = ApprovalRequestStore::new(Arc::new(InMemoryWal::new())).unwrap();
        let req = ApprovalRequest {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            approval_request_id: "appr-x".into(),
            action: BackofficeAction::MarketHalt,
            resource: ResourceRef {
                kind: "market".into(),
                id: "btc-usdt".into(),
            },
            scope: GrantScope::Global,
            submitter_employee_id: "alice".into(),
            submitter_reason: "halting per OPS-991 for risk review of book".into(),
            submitted_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(24),
            status: ApprovalRequestStatus::Approved,
            approver_employee_id: Some("bob".into()),
            approver_reason: Some("verified per OPS-991 incident notes confirming review".into()),
            decided_at: Some(Utc::now()),
            action_payload: serde_json::json!({ "market_id": "btc-usdt" }),
        };
        store.create(req).unwrap();
        let resource = ResourceRef {
            kind: "market".into(),
            id: "btc-usdt".into(),
        };
        assert!(find_committed_approval(
            &store,
            BackofficeAction::MarketHalt,
            &resource,
            "alice"
        )
        .is_some());
        // Different submitter — no match.
        assert!(find_committed_approval(
            &store,
            BackofficeAction::MarketHalt,
            &resource,
            "carol"
        )
        .is_none());
        // Different resource — no match.
        let other = ResourceRef {
            kind: "market".into(),
            id: "eth-usdt".into(),
        };
        assert!(find_committed_approval(
            &store,
            BackofficeAction::MarketHalt,
            &other,
            "alice"
        )
        .is_none());
    }
}
