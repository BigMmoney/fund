// Step 7F scaffold: read-only handlers compile and are exercised by
// unit tests via warp::test. Mutations (POST /addresses, /withdraw,
// /refill, /sweep) land in 7G + 8 once the velocity / sanctions /
// MC plumbing is in place.
#![allow(dead_code)]

//! Backoffice wallet REST handlers (read-only v1).
//!
//! Step 7F of the wallet implementation track. Two endpoints in
//! this commit:
//!
//! - `GET /admin/wallet/balances` — per-chain hot/warm balance
//!   snapshot via the ChainAdapter for hot, plus an aggregate of
//!   outstanding (Queued + Approved + Signing + Broadcast)
//!   reservations from the WithdrawalStore. Gated on `BalancesRead`.
//! - `GET /admin/wallet/queue` — list Queued + AwaitingApproval
//!   withdrawals across all chains, oldest first. Gated on
//!   `WithdrawalsReview`.
//!
//! Both write to the rbac audit log (success path = Committed,
//! denial = DeniedAuthz) per design §9 rule 6.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use warp::{filters::BoxedFilter, Filter, Rejection};

use types::{
    AuditDecision, AuditOutcome, AuthenticatedPrincipal, BackofficeAction, BackofficeActionVerdict,
    GrantScope, MfaMethod, ResourceRef,
};
use wallet::{
    ChainAdapter, ChainId, InMemoryChainAdapter, WithdrawalRecord, WithdrawalStatus,
    WithdrawalStore, WALLET_SCHEMA_VERSION,
};

use crate::admin_authz::AuthzService;
use crate::admin_rbac_audit::{AdminRbacAuditStore, DecisionRecord};

#[derive(Debug, Serialize)]
pub(crate) struct WalletBalanceEntry {
    pub(crate) chain: ChainId,
    pub(crate) hot_address: String,
    pub(crate) hot_balance: i128,
    pub(crate) outstanding_reservations: i128,
    pub(crate) outstanding_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalletBalancesResponse {
    pub(crate) chains: Vec<WalletBalanceEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalletQueueResponse {
    pub(crate) pending: Vec<WithdrawalRecord>,
    pub(crate) total: usize,
}

/// Per-chain configuration the operator endpoints need: which adapter
/// to query and the hot wallet address that adapter manages. Built at
/// startup from environment / config and shared via Arc.
#[derive(Clone)]
pub(crate) struct WalletRuntime {
    pub(crate) per_chain: HashMap<ChainId, ChainRuntime>,
}

#[derive(Clone)]
pub(crate) struct ChainRuntime {
    pub(crate) adapter: Arc<dyn ChainAdapter>,
    pub(crate) hot_address: String,
}

impl WalletRuntime {
    pub(crate) fn empty() -> Self {
        Self {
            per_chain: HashMap::new(),
        }
    }

    pub(crate) fn with_chain(
        mut self,
        chain: ChainId,
        adapter: Arc<dyn ChainAdapter>,
        hot_address: impl Into<String>,
    ) -> Self {
        self.per_chain.insert(
            chain,
            ChainRuntime {
                adapter,
                hot_address: hot_address.into(),
            },
        );
        self
    }
}

fn with_arc<T: Clone + Send + Sync>(
    value: T,
) -> impl Filter<Extract = (T,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || value.clone())
}

/// Optional per-chain `InMemoryChainAdapter` references for the
/// test-only `POST /admin/wallet/test-confirm` endpoint. When the
/// adapter for a chain is `None`, the endpoint returns 404 — used in
/// production deployments where the chain adapter is a real ETH/BTC
/// client and confirmations come from the live chain.
pub(crate) type TestAdapters = HashMap<ChainId, Arc<InMemoryChainAdapter>>;

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_admin_wallet_routes<F>(
    runtime: WalletRuntime,
    test_adapters: TestAdapters,
    withdrawals: Arc<WithdrawalStore>,
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
    let runtime = Arc::new(runtime);
    let test_adapters = Arc::new(test_adapters);
    let balances = warp::path!("admin" / "wallet" / "balances")
        .and(warp::get())
        .and(auth.clone())
        .and(with_arc(runtime.clone()))
        .and(with_arc(withdrawals.clone()))
        .and(with_arc(authz.clone()))
        .and(with_arc(audit.clone()))
        .and_then(handle_balances)
        .boxed();

    let queue = warp::path!("admin" / "wallet" / "queue")
        .and(warp::get())
        .and(auth.clone())
        .and(with_arc(withdrawals.clone()))
        .and(with_arc(authz.clone()))
        .and(with_arc(audit.clone()))
        .and_then(handle_queue)
        .boxed();

    let test_withdrawal = warp::path!("admin" / "wallet" / "test-withdrawal")
        .and(warp::post())
        .and(auth.clone())
        .and(warp::body::json())
        .and(with_arc(withdrawals.clone()))
        .and(with_arc(authz.clone()))
        .and(with_arc(audit.clone()))
        .and_then(handle_test_withdrawal)
        .boxed();

    let test_confirm = warp::path!("admin" / "wallet" / "test-confirm")
        .and(warp::post())
        .and(auth)
        .and(warp::body::json())
        .and(with_arc(test_adapters))
        .and(with_arc(authz))
        .and(with_arc(audit))
        .and_then(handle_test_confirm)
        .boxed();

    balances
        .or(queue)
        .unify()
        .or(test_withdrawal)
        .unify()
        .or(test_confirm)
        .unify()
        .boxed()
}

/// Body for `POST /admin/wallet/test-confirm`. Bumps the in-memory
/// chain adapter's confirmation depth for `tx_hash` so the next
/// hot-wallet worker tick advances `Broadcast` -> `Confirmed`.
/// Returns 404 when the chain isn't backed by an in-memory adapter
/// (i.e. production with a real RPC client).
#[derive(Debug, Deserialize)]
pub(crate) struct TestConfirmBody {
    pub chain: ChainId,
    pub tx_hash: String,
    pub confirmations: u32,
    pub reason: String,
}

pub(crate) async fn handle_test_confirm(
    principal: AuthenticatedPrincipal,
    body: TestConfirmBody,
    test_adapters: Arc<TestAdapters>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    let resource = ResourceRef {
        kind: "endpoint".into(),
        id: "/admin/wallet/test-confirm".into(),
    };
    let verdict = authz.is_allowed(
        &principal.subject,
        BackofficeAction::WithdrawalsApprove,
        &GrantScope::Global,
    );
    if verdict == BackofficeActionVerdict::Deny {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsApprove,
            resource,
            &body.reason,
            "no WithdrawalsApprove grant",
        ));
        return Err(warp::reject::not_found());
    }
    if body.reason.trim().len() < 16 {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsApprove,
            resource,
            &body.reason,
            "reason too short",
        ));
        return Err(warp::reject::not_found());
    }
    let Some(adapter) = test_adapters.get(&body.chain) else {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsApprove,
            resource,
            &body.reason,
            "chain has no in-memory adapter (production deploy?)",
        ));
        return Err(warp::reject::not_found());
    };
    if let Err(e) = adapter.set_confirmations(&body.tx_hash, body.confirmations) {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsApprove,
            resource,
            &body.reason,
            &format!("set_confirmations failed: {e}"),
        ));
        return Err(warp::reject::not_found());
    }
    let _ = audit.record(success_record(
        &principal,
        BackofficeAction::WithdrawalsApprove,
        resource,
        &body.reason,
    ));
    Ok(warp::reply::json(&serde_json::json!({
        "status": "ok",
        "chain": body.chain,
        "tx_hash": body.tx_hash,
        "confirmations": body.confirmations,
    })))
}

/// Body for `POST /admin/wallet/test-withdrawal`. Operator-facing
/// helper that creates a synthetic withdrawal record in the wallet
/// store and immediately auto-approves it so the hot-wallet worker
/// picks it up. Strictly for end-to-end pipeline testing — production
/// withdrawals come from the customer-facing `/withdraw` endpoint
/// (separate migration track).
#[derive(Debug, Deserialize)]
pub(crate) struct TestWithdrawalBody {
    pub user_id: String,
    pub chain: ChainId,
    pub destination_address: String,
    pub amount: i128,
    #[serde(default)]
    pub estimated_fee: Option<i128>,
    #[serde(default)]
    pub confirmations_required: Option<u32>,
    pub reason: String,
}

pub(crate) async fn handle_test_withdrawal(
    principal: AuthenticatedPrincipal,
    body: TestWithdrawalBody,
    withdrawals: Arc<WithdrawalStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    // Reuse the WithdrawalsApprove permission as the v1 gate for
    // creating test withdrawals: only roles that can already commit
    // withdrawals through maker-checker may exercise this endpoint.
    let resource = ResourceRef {
        kind: "endpoint".into(),
        id: "/admin/wallet/test-withdrawal".into(),
    };
    let verdict = authz.is_allowed(
        &principal.subject,
        BackofficeAction::WithdrawalsApprove,
        &GrantScope::Global,
    );
    if verdict == BackofficeActionVerdict::Deny {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsApprove,
            resource,
            &body.reason,
            "no WithdrawalsApprove grant",
        ));
        return Err(warp::reject::not_found());
    }
    if body.reason.trim().len() < 16 {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsApprove,
            resource,
            &body.reason,
            "reason too short (min 16 non-whitespace chars)",
        ));
        return Err(warp::reject::not_found());
    }
    if body.amount <= 0 {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsApprove,
            resource,
            &body.reason,
            "amount must be > 0",
        ));
        return Err(warp::reject::not_found());
    }
    let now = Utc::now();
    let withdrawal_id = format!("wd-test-{}", uuid::Uuid::new_v4());
    let record = WithdrawalRecord {
        schema_version: WALLET_SCHEMA_VERSION,
        withdrawal_id: withdrawal_id.clone(),
        user_id: body.user_id.clone(),
        chain: body.chain,
        address_id: "test-address".into(),
        destination_address: body.destination_address.clone(),
        amount: body.amount,
        estimated_fee: body.estimated_fee.unwrap_or(1_000_000_i128),
        actual_fee: None,
        status: WithdrawalStatus::Submitted,
        submitted_at: now,
        updated_at: now,
        approved_at: None,
        broadcast_at: None,
        confirmed_at: None,
        settled_at: None,
        tx_hash: None,
        confirmations: 0,
        confirmations_required: body.confirmations_required.unwrap_or(25),
        approval_request_id: None,
        rejection_reason: None,
        notes: Some(format!("test withdrawal created by {}", principal.subject)),
    };
    if let Err(e) = withdrawals.create(record) {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsApprove,
            resource,
            &body.reason,
            &format!("withdrawal store create failed: {e}"),
        ));
        return Err(warp::reject::not_found());
    }
    // Walk to Approved synchronously so the worker picks it up on
    // its next tick. Production has the validate/queue/MC layers
    // doing this; this is a test convenience.
    for next in [
        WithdrawalStatus::Validated,
        WithdrawalStatus::Queued,
        WithdrawalStatus::Approved,
    ] {
        if let Err(e) = withdrawals.advance_status(&withdrawal_id, next) {
            let _ = audit.record(deny_record(
                &principal,
                BackofficeAction::WithdrawalsApprove,
                resource,
                &body.reason,
                &format!("advance_status to {:?} failed: {e}", next),
            ));
            return Err(warp::reject::not_found());
        }
    }
    let _ = audit.record(success_record(
        &principal,
        BackofficeAction::WithdrawalsApprove,
        resource,
        &body.reason,
    ));
    Ok(warp::reply::json(&serde_json::json!({
        "status": "approved",
        "withdrawal_id": withdrawal_id,
        "user_id": body.user_id,
        "chain": body.chain,
        "amount": body.amount,
    })))
}

pub(crate) async fn handle_balances(
    principal: AuthenticatedPrincipal,
    runtime: Arc<WalletRuntime>,
    withdrawals: Arc<WithdrawalStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    let resource = ResourceRef {
        kind: "endpoint".into(),
        id: "/admin/wallet/balances".into(),
    };
    if authz.is_allowed(
        &principal.subject,
        BackofficeAction::BalancesRead,
        &GrantScope::Global,
    ) != BackofficeActionVerdict::Allow
    {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::BalancesRead,
            resource,
            "balances endpoint",
            "no BalancesRead grant",
        ));
        return Err(warp::reject::not_found());
    }
    let _ = audit.record(success_record(
        &principal,
        BackofficeAction::BalancesRead,
        resource,
        "balances endpoint",
    ));

    // Aggregate outstanding (Queued / AwaitingApproval / Approved /
    // Signing / Broadcast) per chain. These are amounts the wallet
    // has committed to but not yet settled.
    let mut outstanding_per_chain: HashMap<ChainId, (i128, usize)> = HashMap::new();
    for w in withdrawals.pending() {
        let entry = outstanding_per_chain.entry(w.chain).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(w.amount.saturating_add(w.estimated_fee));
        entry.1 += 1;
    }

    let mut chains = Vec::with_capacity(runtime.per_chain.len());
    for (chain, rt) in runtime.per_chain.iter() {
        let hot_balance = rt.adapter.balance(&rt.hot_address).unwrap_or(0);
        let (outstanding, count) = outstanding_per_chain
            .get(chain)
            .copied()
            .unwrap_or((0, 0));
        chains.push(WalletBalanceEntry {
            chain: *chain,
            hot_address: rt.hot_address.clone(),
            hot_balance,
            outstanding_reservations: outstanding,
            outstanding_count: count,
        });
    }
    chains.sort_by_key(|e| e.chain.to_string());
    Ok(warp::reply::json(&WalletBalancesResponse { chains }))
}

pub(crate) async fn handle_queue(
    principal: AuthenticatedPrincipal,
    withdrawals: Arc<WithdrawalStore>,
    authz: Arc<AuthzService>,
    audit: Arc<AdminRbacAuditStore>,
) -> Result<warp::reply::Json, Rejection> {
    let resource = ResourceRef {
        kind: "endpoint".into(),
        id: "/admin/wallet/queue".into(),
    };
    if authz.is_allowed(
        &principal.subject,
        BackofficeAction::WithdrawalsReview,
        &GrantScope::Global,
    ) != BackofficeActionVerdict::Allow
    {
        let _ = audit.record(deny_record(
            &principal,
            BackofficeAction::WithdrawalsReview,
            resource,
            "queue endpoint",
            "no WithdrawalsReview grant",
        ));
        return Err(warp::reject::not_found());
    }
    let _ = audit.record(success_record(
        &principal,
        BackofficeAction::WithdrawalsReview,
        resource,
        "queue endpoint",
    ));
    let pending = withdrawals.pending();
    let total = pending.len();
    Ok(warp::reply::json(&WalletQueueResponse { pending, total }))
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

fn success_record<'a>(
    principal: &'a AuthenticatedPrincipal,
    action: BackofficeAction,
    resource: ResourceRef,
    reason: &'a str,
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
        decision: AuditDecision::Committed,
        decision_reason: None,
        approval_request_id: None,
        approval: None,
        break_glass_session_id: None,
        incident_reference: None,
        outcome: AuditOutcome::Success,
        outcome_detail: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin_rbac_store::{AdminEmployeeStore, AdminGrantStore};
    use chrono::{Duration, Utc};
    use persistence::InMemoryWal;
    use types::{
        BackofficeRole, Employee, EmployeeStatus, Grant, GrantStatus, PrincipalRole, RoleLevel,
        BACKOFFICE_SCHEMA_VERSION,
    };
    use wallet::{InMemoryChainAdapter, WithdrawalRecord, WithdrawalStatus, WALLET_SCHEMA_VERSION};
    use warp::Reply;

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
        // Act level satisfies both the read-rows (Act >= Read) and the
        // act-rows (e.g. FinanceOps WithdrawalsReview which the v1
        // matrix sets at Act level since their primary job is to
        // act-on, not just read, withdrawals).
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

    fn make_runtime_with_seeded_eth_balance(hot: &str, balance: i128) -> WalletRuntime {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        adapter.seed_balance(hot, balance);
        WalletRuntime::empty().with_chain(ChainId::Eth, Arc::new(adapter), hot)
    }

    fn pending_record(id: &str, user: &str, amount: i128) -> WithdrawalRecord {
        WithdrawalRecord {
            schema_version: WALLET_SCHEMA_VERSION,
            withdrawal_id: id.into(),
            user_id: user.into(),
            chain: ChainId::Eth,
            address_id: "addr-1".into(),
            destination_address: "0xdest".into(),
            amount,
            estimated_fee: 1_000_000_i128,
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
            confirmations_required: 25,
            approval_request_id: None,
            rejection_reason: None,
            notes: None,
        }
    }

    fn make_pieces(
        principal_subject: &str,
        roles: &[BackofficeRole],
    ) -> (
        Arc<AuthzService>,
        Arc<AdminRbacAuditStore>,
        Arc<WithdrawalStore>,
    ) {
        let employees = Arc::new(AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let grants = Arc::new(AdminGrantStore::new(Arc::new(InMemoryWal::new())).unwrap());
        employees.create(employee(principal_subject)).unwrap();
        for (i, role) in roles.iter().enumerate() {
            grants
                .create(grant_for(
                    &format!("g-{i}"),
                    principal_subject,
                    *role,
                ))
                .unwrap();
        }
        let authz = Arc::new(AuthzService::new(employees, grants));
        let audit = Arc::new(AdminRbacAuditStore::new(Arc::new(InMemoryWal::new())));
        let withdrawals = Arc::new(WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap());
        (authz, audit, withdrawals)
    }

    fn principal(subject: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            subject: subject.into(),
            role: PrincipalRole::Admin,
            session_id: None,
        }
    }

    async fn body_json(reply: warp::reply::Json) -> serde_json::Value {
        let bytes = warp::hyper::body::to_bytes(reply.into_response().into_body())
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn balances_denied_without_balances_read() {
        // Trading_ops doesn't have BalancesRead per the v1 matrix.
        let (authz, audit, withdrawals) = make_pieces("alice", &[BackofficeRole::TradingOps]);
        let runtime = Arc::new(make_runtime_with_seeded_eth_balance("0xhot", 1_000_000_i128));
        let r = handle_balances(
            principal("alice"),
            runtime,
            withdrawals,
            authz,
            audit.clone(),
        )
        .await;
        assert!(r.is_err());
        assert_eq!(audit.entries().unwrap()[0].decision, AuditDecision::DeniedAuthz);
    }

    #[tokio::test]
    async fn balances_allowed_for_finance_ops_returns_hot_balance_and_outstanding() {
        let (authz, audit, withdrawals) = make_pieces("fin", &[BackofficeRole::FinanceOps]);
        // Seed two pending withdrawals: their amount + fee should
        // sum into outstanding_reservations.
        let mut a = pending_record("wd-a", "alice", 100_000_i128);
        a.status = WithdrawalStatus::Submitted;
        withdrawals.create(a).unwrap();
        withdrawals.advance_status("wd-a", WithdrawalStatus::Validated).unwrap();
        withdrawals.advance_status("wd-a", WithdrawalStatus::Queued).unwrap();
        let mut b = pending_record("wd-b", "bob", 200_000_i128);
        b.status = WithdrawalStatus::Submitted;
        withdrawals.create(b).unwrap();
        withdrawals.advance_status("wd-b", WithdrawalStatus::Validated).unwrap();
        withdrawals.advance_status("wd-b", WithdrawalStatus::Queued).unwrap();
        withdrawals.advance_status("wd-b", WithdrawalStatus::AwaitingApproval).unwrap();

        let runtime = Arc::new(make_runtime_with_seeded_eth_balance("0xhot", 1_000_000_i128));
        let reply = handle_balances(principal("fin"), runtime, withdrawals, authz, audit.clone())
            .await
            .unwrap();
        let json = body_json(reply).await;
        let chains = json["chains"].as_array().unwrap();
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0]["chain"].as_str().unwrap(), "eth");
        assert_eq!(chains[0]["hot_balance"].as_i64().unwrap(), 1_000_000);
        // outstanding = 100_000 + 1_000_000 (fee) + 200_000 + 1_000_000 = 2_300_000
        assert_eq!(
            chains[0]["outstanding_reservations"].as_i64().unwrap(),
            2_300_000
        );
        assert_eq!(chains[0]["outstanding_count"].as_u64().unwrap(), 2);
        // Audit row Committed.
        assert_eq!(audit.entries().unwrap()[0].decision, AuditDecision::Committed);
    }

    #[tokio::test]
    async fn queue_denied_without_withdrawals_review() {
        // RiskOps lacks WithdrawalsReview per the v1 matrix.
        let (authz, audit, withdrawals) = make_pieces("risk", &[BackofficeRole::RiskOps]);
        let r = handle_queue(principal("risk"), withdrawals, authz, audit.clone()).await;
        assert!(r.is_err());
        assert_eq!(audit.entries().unwrap()[0].decision, AuditDecision::DeniedAuthz);
    }

    #[tokio::test]
    async fn queue_allowed_for_finance_ops_returns_pending_oldest_first() {
        let (authz, audit, withdrawals) = make_pieces("fin", &[BackofficeRole::FinanceOps]);
        // Two queued + one settled. Only the queued / awaiting-approval
        // should appear.
        let mut a = pending_record("wd-old", "alice", 1_i128);
        a.submitted_at = chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000, 0).unwrap();
        withdrawals.create(a).unwrap();
        withdrawals.advance_status("wd-old", WithdrawalStatus::Validated).unwrap();
        withdrawals.advance_status("wd-old", WithdrawalStatus::Queued).unwrap();

        let mut b = pending_record("wd-new", "alice", 1_i128);
        b.submitted_at = chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_500, 0).unwrap();
        withdrawals.create(b).unwrap();
        withdrawals.advance_status("wd-new", WithdrawalStatus::Validated).unwrap();
        withdrawals.advance_status("wd-new", WithdrawalStatus::Queued).unwrap();
        withdrawals.advance_status("wd-new", WithdrawalStatus::AwaitingApproval).unwrap();

        let mut done = pending_record("wd-done", "alice", 1_i128);
        done.submitted_at = chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_100, 0).unwrap();
        withdrawals.create(done).unwrap();
        for next in [
            WithdrawalStatus::Validated,
            WithdrawalStatus::Queued,
            WithdrawalStatus::Approved,
            WithdrawalStatus::Signing,
            WithdrawalStatus::Broadcast,
            WithdrawalStatus::Confirmed,
            WithdrawalStatus::Settled,
        ] {
            withdrawals.advance_status("wd-done", next).unwrap();
        }

        let reply = handle_queue(principal("fin"), withdrawals, authz, audit.clone())
            .await
            .unwrap();
        let json = body_json(reply).await;
        assert_eq!(json["total"].as_u64().unwrap(), 2);
        let pending = json["pending"].as_array().unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0]["withdrawal_id"].as_str().unwrap(), "wd-old");
        assert_eq!(pending[1]["withdrawal_id"].as_str().unwrap(), "wd-new");
    }

    #[tokio::test]
    async fn build_admin_wallet_routes_smoke_warp_test() {
        let (authz, audit, withdrawals) = make_pieces("fin", &[BackofficeRole::FinanceOps]);
        let runtime = make_runtime_with_seeded_eth_balance("0xhot", 5_000_000_i128);
        let auth = warp::any().and_then(|| async {
            Ok::<AuthenticatedPrincipal, Rejection>(AuthenticatedPrincipal {
                subject: "fin".into(),
                role: PrincipalRole::Admin,
                session_id: None,
            })
        });
        let routes = build_admin_wallet_routes(
            runtime,
            HashMap::new(),
            withdrawals,
            authz,
            audit,
            auth,
        );

        let resp = warp::test::request()
            .method("GET")
            .path("/admin/wallet/balances")
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), warp::http::StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["chains"].as_array().unwrap().len(), 1);

        let resp = warp::test::request()
            .method("GET")
            .path("/admin/wallet/queue")
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), warp::http::StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["total"].as_u64().unwrap(), 0);
    }
}
