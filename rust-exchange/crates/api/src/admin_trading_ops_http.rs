// Step 5: Trading Ops 基础动作 wired through RBAC + maker-checker.
// Two endpoints in this commit (halt/resume); mass-cancel is a follow-up.
#![allow(dead_code)]

//! Backoffice Trading Ops actions.
//!
//! Step 5 of the RBAC MVP — wires `MarketHalt` and `MarketResume`
//! through the maker-checker flow. Each handler:
//!   1. Looks up the calling employee's verdict for the action.
//!   2. If `Allow` (super_admin_break_glass single-actor for halt;
//!      otherwise the matrix never grants single-actor): commit
//!      directly. Audit row records `Committed`.
//!   3. If `RequiresApproval`: look for a previously-Approved
//!      ApprovalRequest by the same submitter for the same
//!      action+resource via `find_committed_approval`. If found,
//!      commit; else 404 with audit row recording `DeniedAuthz` and
//!      reason "no committed approval found".
//!   4. If `Deny`: 404 with audit row.
//!
//! Routes mounted under `/admin/trading-ops/*` so the existing
//! `/admin/market-state` (governance-store-based, not RBAC-based)
//! continues to work in parallel during rollout.

use std::sync::Arc;

use serde::Deserialize;
use warp::{filters::BoxedFilter, Filter, Rejection};

use matching::partitioned::PartitionedMatchingEngine;
use sequencer::Sequencer;
use types::{
    AdminAction, AdminCommand, AuditApprovalSummary, AuditDecision, AuditOutcome,
    AuthenticatedPrincipal, BackofficeAction, BackofficeActionVerdict, Command, CommandMetadata,
    GrantScope, MarketState, MfaMethod, ResourceRef,
};

use crate::admin_approvals_http::find_committed_approval;
use crate::admin_authz::AuthzService;
use crate::admin_rbac_audit::{AdminRbacAuditStore, DecisionRecord};
use crate::admin_rbac_store::ApprovalRequestStore;

#[derive(Debug, Deserialize)]
pub(crate) struct MarketHaltBody {
    #[serde(default)]
    pub outcome: i32,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MarketResumeBody {
    #[serde(default)]
    pub outcome: i32,
    pub reason: String,
}

fn with_arc<T: Clone + Send + Sync>(
    value: T,
) -> impl Filter<Extract = (T,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || value.clone())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_admin_trading_ops_routes<F>(
    engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
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
    let halt = warp::path!("admin" / "trading-ops" / "markets" / String / "halt")
        .and(warp::post())
        .and(auth.clone())
        .and(warp::body::json())
        .and(with_arc(engine.clone()))
        .and(with_arc(sequencer.clone()))
        .and(with_arc(requests.clone()))
        .and(with_arc(authz.clone()))
        .and(with_arc(audit.clone()))
        .and_then(handle_market_halt)
        .boxed();

    let resume = warp::path!("admin" / "trading-ops" / "markets" / String / "resume")
        .and(warp::post())
        .and(auth)
        .and(warp::body::json())
        .and(with_arc(engine))
        .and(with_arc(sequencer))
        .and(with_arc(requests))
        .and(with_arc(authz))
        .and(with_arc(audit))
        .and_then(handle_market_resume)
        .boxed();

    halt.or(resume).unify().boxed()
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_market_halt(
    market_id: String,
    principal: AuthenticatedPrincipal,
    body: MarketHaltBody,
    engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    requests: Arc<ApprovalRequestStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    handle_market_state_change(
        market_id,
        body.outcome,
        body.reason,
        principal,
        BackofficeAction::MarketHalt,
        MarketState::Halted,
        engine,
        sequencer,
        requests,
        authz,
        audit,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_market_resume(
    market_id: String,
    principal: AuthenticatedPrincipal,
    body: MarketResumeBody,
    engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    requests: Arc<ApprovalRequestStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    handle_market_state_change(
        market_id,
        body.outcome,
        body.reason,
        principal,
        BackofficeAction::MarketResume,
        MarketState::Normal,
        engine,
        sequencer,
        requests,
        authz,
        audit,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn handle_market_state_change(
    market_id: String,
    outcome: i32,
    reason: String,
    principal: AuthenticatedPrincipal,
    action: BackofficeAction,
    target_state: MarketState,
    engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    requests: Arc<ApprovalRequestStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    let resource = ResourceRef {
        kind: "market".into(),
        id: market_id.clone(),
    };
    if reason.trim().len() < 16 {
        let _ = audit.record(deny_record(
            &principal,
            action,
            resource.clone(),
            &reason,
            "reason too short (min 16 non-whitespace chars)",
        ));
        return Err(warp::reject::not_found());
    }
    let verdict = authz.is_allowed(&principal.subject, action, &GrantScope::Global);
    let approval_summary = match verdict {
        BackofficeActionVerdict::Deny => {
            let _ = audit.record(deny_record(
                &principal,
                action,
                resource.clone(),
                &reason,
                "no grant for action",
            ));
            return Err(warp::reject::not_found());
        }
        BackofficeActionVerdict::Allow => None,
        BackofficeActionVerdict::RequiresApproval => {
            // Need a previously-approved request from the same submitter.
            match find_committed_approval(
                requests.as_ref(),
                action,
                &resource,
                &principal.subject,
            ) {
                None => {
                    let _ = audit.record(deny_record(
                        &principal,
                        action,
                        resource.clone(),
                        &reason,
                        "no committed approval found for action+resource+submitter",
                    ));
                    return Err(warp::reject::not_found());
                }
                Some(req) => Some(AuditApprovalSummary {
                    submitter_employee_id: req.submitter_employee_id.clone(),
                    submitter_reason: req.submitter_reason.clone(),
                    approver_employee_id: req
                        .approver_employee_id
                        .clone()
                        .unwrap_or_default(),
                    approver_reason: req.approver_reason.clone().unwrap_or_default(),
                    approved_at: req.decided_at.unwrap_or_else(chrono::Utc::now),
                }),
            }
        }
    };

    let request_id = format!("trading-ops-{}", uuid::Uuid::new_v4());
    let admin_action = AdminAction::SetMarketState {
        market_id: market_id.clone(),
        outcome: Some(outcome),
        state: target_state,
    };
    let cmd = match sequence_admin(&sequencer, request_id.clone(), &principal.subject, admin_action) {
        Ok(c) => c,
        Err(e) => {
            let _ = audit.record(deny_record(
                &principal,
                action,
                resource.clone(),
                &reason,
                &format!("sequence_admin failed: {e}"),
            ));
            return Err(warp::reject::not_found());
        }
    };
    if let Err(e) = engine.submit_admin(cmd).await {
        let _ = audit.record(deny_record(
            &principal,
            action,
            resource.clone(),
            &reason,
            &format!("engine.submit_admin failed: {e}"),
        ));
        return Err(warp::reject::not_found());
    }

    let _ = audit.record(DecisionRecord {
        principal: &principal,
        remote_ip: "",
        user_agent: "",
        mfa_method: MfaMethod::Webauthn,
        action,
        resource: resource.clone(),
        scope: GrantScope::Global,
        reason: &reason,
        decision: AuditDecision::Committed,
        decision_reason: None,
        approval_request_id: None,
        approval: approval_summary,
        break_glass_session_id: None,
        incident_reference: None,
        outcome: AuditOutcome::Success,
        outcome_detail: None,
    });

    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "market_id": market_id,
        "outcome": outcome,
        "state": target_state,
        "request_id": request_id,
    })))
}

fn deny_record<'a>(
    principal: &'a AuthenticatedPrincipal,
    action: BackofficeAction,
    resource: ResourceRef,
    reason: &'a str,
    decision_reason: &'a str,
) -> DecisionRecord<'a> {
    DecisionRecord {
        principal,
        remote_ip: "",
        user_agent: "",
        mfa_method: MfaMethod::Webauthn,
        action,
        resource,
        scope: GrantScope::Global,
        reason,
        decision: AuditDecision::DeniedAuthz,
        decision_reason: Some(decision_reason.to_string()),
        approval_request_id: None,
        approval: None,
        break_glass_session_id: None,
        incident_reference: None,
        outcome: AuditOutcome::Failure,
        outcome_detail: None,
    }
}

fn sequence_admin(
    sequencer: &Sequencer,
    request_id: String,
    actor_id: &str,
    action: AdminAction,
) -> Result<AdminCommand, String> {
    match sequencer
        .sequence_and_append(Command::Admin(AdminCommand {
            metadata: CommandMetadata::new(request_id),
            actor_id: actor_id.to_string(),
            action,
        }))
        .map_err(|e| e.to_string())?
    {
        Command::Admin(c) => Ok(c),
        _ => Err("sequencer returned non-admin command".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin_rbac_store::{AdminEmployeeStore, AdminGrantStore};
    use chrono::{Duration, Utc};
    use persistence::InMemoryWal;
    use types::{
        ApprovalRequest, ApprovalRequestStatus, BackofficeRole, Employee, EmployeeStatus, Grant,
        GrantStatus, RoleLevel, BACKOFFICE_SCHEMA_VERSION,
    };

    fn employee(id: &str) -> Employee {
        Employee {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            employee_id: id.into(),
            display_name: id.into(),
            status: EmployeeStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_mfa_method: Some(MfaMethod::Webauthn),
            last_login_at: None,
        }
    }

    fn grant_for(id: &str, employee_id: &str, role: BackofficeRole) -> Grant {
        Grant {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            grant_id: id.into(),
            employee_id: employee_id.into(),
            role,
            level: RoleLevel::Act,
            scope: GrantScope::Global,
            status: GrantStatus::Active,
            granted_by: "secadmin".into(),
            granted_at: Utc::now(),
            expires_at: Utc::now() + Duration::days(30),
            reason: "test grant".into(),
            approval_request_id: None,
        }
    }

    fn approved_request(action: BackofficeAction, market_id: &str, submitter: &str) -> ApprovalRequest {
        ApprovalRequest {
            schema_version: BACKOFFICE_SCHEMA_VERSION,
            approval_request_id: format!("appr-test-{}", uuid::Uuid::new_v4()),
            action,
            resource: ResourceRef {
                kind: "market".into(),
                id: market_id.into(),
            },
            scope: GrantScope::Global,
            submitter_employee_id: submitter.into(),
            submitter_reason: "test halt request reason 16+".into(),
            submitted_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(24),
            status: ApprovalRequestStatus::Approved,
            approver_employee_id: Some("approver".into()),
            approver_reason: Some("verified per ticket OPS-1 after risk review".into()),
            decided_at: Some(Utc::now()),
            action_payload: serde_json::json!({ "market_id": market_id }),
        }
    }

    /// Build the world without an engine — for tests of authz / approval
    /// path that should reject before reaching the engine.
    fn make_authz_world(
        bootstrap_subject: &str,
        role: BackofficeRole,
    ) -> (
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
        employees.create(employee(bootstrap_subject)).unwrap();
        grants
            .create(grant_for("g-test", bootstrap_subject, role))
            .unwrap();
        (employees, grants, authz, audit, requests)
    }

    #[test]
    fn deny_record_carries_decision_reason() {
        let principal = AuthenticatedPrincipal {
            subject: "alice".into(),
            role: types::PrincipalRole::Admin,
            session_id: None,
        };
        let resource = ResourceRef {
            kind: "market".into(),
            id: "btc-usdt".into(),
        };
        let dr = deny_record(
            &principal,
            BackofficeAction::MarketHalt,
            resource,
            "test halt reason long enough for validator",
            "no grant for action",
        );
        assert_eq!(dr.decision, AuditDecision::DeniedAuthz);
        assert_eq!(dr.outcome, AuditOutcome::Failure);
        assert_eq!(dr.decision_reason.as_deref(), Some("no grant for action"));
    }

    #[test]
    fn find_committed_approval_required_when_verdict_is_requires_approval() {
        // Trading_ops on MarketHalt = RequiresApproval per the v1 matrix.
        let (_, _, authz, _, requests) = make_authz_world("alice", BackofficeRole::TradingOps);
        let verdict = authz.is_allowed(
            "alice",
            BackofficeAction::MarketHalt,
            &GrantScope::Global,
        );
        assert_eq!(verdict, BackofficeActionVerdict::RequiresApproval);
        // No committed approval yet.
        let resource = ResourceRef {
            kind: "market".into(),
            id: "btc-usdt".into(),
        };
        assert!(find_committed_approval(
            requests.as_ref(),
            BackofficeAction::MarketHalt,
            &resource,
            "alice",
        )
        .is_none());
        // Seed an approved request.
        requests
            .create(approved_request(BackofficeAction::MarketHalt, "btc-usdt", "alice"))
            .unwrap();
        assert!(find_committed_approval(
            requests.as_ref(),
            BackofficeAction::MarketHalt,
            &resource,
            "alice",
        )
        .is_some());
    }

    #[test]
    fn find_committed_approval_does_not_match_other_market() {
        let (_, _, _, _, requests) = make_authz_world("alice", BackofficeRole::TradingOps);
        requests
            .create(approved_request(BackofficeAction::MarketHalt, "btc-usdt", "alice"))
            .unwrap();
        let other = ResourceRef {
            kind: "market".into(),
            id: "eth-usdt".into(),
        };
        assert!(find_committed_approval(
            requests.as_ref(),
            BackofficeAction::MarketHalt,
            &other,
            "alice",
        )
        .is_none());
    }

    #[test]
    fn break_glass_grants_single_actor_market_halt() {
        // super_admin_break_glass on MarketHalt = Allow per the v1 matrix.
        let (_, _, authz, _, _) = make_authz_world("oncall", BackofficeRole::SuperAdminBreakGlass);
        let verdict = authz.is_allowed(
            "oncall",
            BackofficeAction::MarketHalt,
            &GrantScope::Global,
        );
        assert_eq!(verdict, BackofficeActionVerdict::Allow);
    }
}
