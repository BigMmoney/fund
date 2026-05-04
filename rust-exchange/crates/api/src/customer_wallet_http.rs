// Customer-facing wallet endpoints on the NEW wallet crate stack.
// Parallel to the existing /withdraw (which still uses the older
// custody module) — the cutover from the old to the new path is a
// frontend / SDK migration, not a single big-bang commit. The old
// /withdraw stays live while customers move over.

//! Customer-facing wallet endpoints (v2).
//!
//! Three endpoints under `/v2/wallet/*`:
//! - `POST /v2/wallet/addresses` — whitelist a withdrawal address.
//!   Synchronously sanctions-screened; returns `PendingCooldown`
//!   until the design's 24h cool-down passes (in v1 the cool-down
//!   is configurable via env var; default 24h).
//! - `POST /v2/wallet/withdraw` — submit a withdrawal. Resolves the
//!   destination via the address book, re-screens for sanctions,
//!   checks the per-customer per-day velocity cap, then creates a
//!   `WithdrawalRecord` and auto-walks it Submitted → Validated →
//!   Queued → Approved so the hot-wallet worker picks it up.
//! - `GET /v2/wallet/withdrawals/{withdrawal_id}` — single-record
//!   status poll.
//!
//! Authorization: standard customer auth via `with_principal()` +
//! `require_user()`. Customers see only their own records (the
//! handler enforces `principal.subject == record.user_id`).
//!
//! Maker-checker for above-threshold amounts is NOT yet wired here.
//! v1 part 1 auto-approves every withdrawal that passes the velocity
//! and balance checks; the MC integration lands as a follow-up
//! that calls the existing ApprovalRequestStore from the api crate.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use ledger::LedgerService;
use serde::{Deserialize, Serialize};
use warp::{filters::BoxedFilter, Filter, Rejection};

use types::AuthenticatedPrincipal;
use wallet::{
    AddressBookStore, AddressStatus, ChainId, SanctionsProvider, SanctionsScreenStatus,
    VelocityTracker, WithdrawalAddress, WithdrawalRecord, WithdrawalRejectReason,
    WithdrawalStatus, WithdrawalStore, WALLET_SCHEMA_VERSION,
};

const DEFAULT_COOLDOWN_SECS: i64 = 24 * 60 * 60;
const DEFAULT_VELOCITY_CAP_WEI: i128 = 500_000_000_000_000_000_000_i128; // 500 ETH equivalent placeholder

/// Structured rejections for the customer-wallet surface (C1).
///
/// Previously every failure reduced to `warp::reject::not_found()`,
/// which (a) gave the client no way to distinguish a malformed
/// request from a sanctions hit from a server fault, and (b) on
/// already-matched routes surfaced as HTTP 500 instead of 404 in the
/// smoke harness. `WalletError` carries enough information that
/// `wallet_error_to_reply` can produce a stable JSON error envelope
/// with the correct status code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WalletError {
    BadRequest(&'static str),
    AddressNotFound,
    AddressNotActive,
    SanctionsHit,
    /// Sanctions provider returned `Error` (RPC failure / timeout).
    /// Per design §11 rule 9 we treat this as a soft-block: the
    /// caller should retry; we MUST NOT silently let the address
    /// through (H1).
    SanctionsUnavailable,
    VelocityExceeded,
    InsufficientBalance,
    AmountTooLarge,
    /// Reserved for future surface (e.g. admin-impersonation paths).
    /// Wired through `wallet_error_to_reply` and asserted on by the
    /// status-mapping test, but no production handler raises it yet.
    #[allow(dead_code)]
    Forbidden,
    Internal(String),
}

impl warp::reject::Reject for WalletError {}

#[derive(Debug, Serialize)]
struct WalletErrorBody {
    error_code: &'static str,
    message: String,
}

/// Map a `WalletError` to a JSON reply with the correct HTTP status.
/// Wired in `main.rs::handle_rejection`. The status codes are stable
/// API contract — clients and the React frontend depend on them.
pub(crate) fn wallet_error_to_reply(err: &WalletError) -> warp::reply::WithStatus<warp::reply::Json> {
    use warp::http::StatusCode;
    let (code, message, status) = match err {
        WalletError::BadRequest(msg) => ("bad_request", (*msg).to_string(), StatusCode::BAD_REQUEST),
        WalletError::AddressNotFound => (
            "address_not_found",
            "destination not whitelisted for this user".into(),
            StatusCode::NOT_FOUND,
        ),
        WalletError::AddressNotActive => (
            "address_not_active",
            "destination address is not active (cool-down, suspended, or removed)".into(),
            StatusCode::CONFLICT,
        ),
        WalletError::SanctionsHit => (
            "sanctions_hit",
            "destination matched a sanctions list".into(),
            StatusCode::FORBIDDEN,
        ),
        WalletError::SanctionsUnavailable => (
            "sanctions_unavailable",
            "sanctions provider is unavailable; retry later".into(),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        WalletError::VelocityExceeded => (
            "velocity_exceeded",
            "withdrawal would exceed the per-day cap for this user/chain".into(),
            StatusCode::CONFLICT,
        ),
        WalletError::InsufficientBalance => (
            "insufficient_balance",
            "available cash is below amount + estimated_fee".into(),
            StatusCode::CONFLICT,
        ),
        WalletError::AmountTooLarge => (
            "amount_too_large",
            "amount exceeds the per-chain ledger ceiling".into(),
            StatusCode::BAD_REQUEST,
        ),
        WalletError::Forbidden => (
            "forbidden",
            "not the owner of this resource".into(),
            StatusCode::FORBIDDEN,
        ),
        WalletError::Internal(msg) => ("internal", msg.clone(), StatusCode::INTERNAL_SERVER_ERROR),
    };
    let body = WalletErrorBody {
        error_code: code,
        message,
    };
    warp::reply::with_status(warp::reply::json(&body), status)
}

fn reject(err: WalletError) -> Rejection {
    warp::reject::custom(err)
}

#[derive(Debug, Deserialize)]
pub(crate) struct AddAddressBody {
    pub chain: ChainId,
    pub address: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubmitWithdrawBody {
    pub chain: ChainId,
    /// On-chain destination — must already exist in the customer's
    /// address book (status = Active OR PendingCooldown past
    /// cooldown_until). Look up via the address book; the api does
    /// NOT accept ad-hoc destinations per design §11 rule 1.
    pub destination_address: String,
    pub amount: i128,
    /// Optional client-supplied memo / reference id; surfaces in
    /// status-poll responses.
    #[serde(default)]
    pub client_reference: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AddAddressResponse {
    pub address_id: String,
    pub chain: ChainId,
    pub status: AddressStatus,
    pub cooldown_until: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SubmitWithdrawResponse {
    pub status: String,
    pub withdrawal_id: String,
    pub chain: ChainId,
    pub amount: i128,
    pub destination_address: String,
}

/// Per-chain customer-facing config. v1 has only Eth wired so the
/// runtime carries Eth bits; multi-chain expansion repeats this.
/// `Clone` is implemented manually at the bottom of this file because
/// the `Arc<dyn SanctionsProvider>` field doesn't benefit from
/// `#[derive(Clone)]` and the test path uses struct-update syntax
/// (`..(*runtime).clone()`).
pub(crate) struct CustomerWalletRuntime {
    pub addresses: Arc<AddressBookStore>,
    pub withdrawals: Arc<WithdrawalStore>,
    pub sanctions: Arc<dyn SanctionsProvider>,
    pub velocity: Arc<VelocityTracker>,
    /// Used for the at-submit balance pre-check (C2). Without this,
    /// `handle_submit_withdraw` would auto-walk to Approved for a
    /// customer with zero cash and the settlement worker would
    /// discover the problem only AFTER the on-chain broadcast.
    pub ledger: Arc<LedgerService>,
    pub cooldown: Duration,
    pub velocity_cap_wei: i128,
}

impl CustomerWalletRuntime {
    pub fn new(
        addresses: Arc<AddressBookStore>,
        withdrawals: Arc<WithdrawalStore>,
        sanctions: Arc<dyn SanctionsProvider>,
        velocity: Arc<VelocityTracker>,
        ledger: Arc<LedgerService>,
    ) -> Self {
        Self {
            addresses,
            withdrawals,
            sanctions,
            velocity,
            ledger,
            cooldown: Duration::from_secs(DEFAULT_COOLDOWN_SECS as u64),
            velocity_cap_wei: DEFAULT_VELOCITY_CAP_WEI,
        }
    }

    pub fn with_cooldown(mut self, cooldown: Duration) -> Self {
        self.cooldown = cooldown;
        self
    }

    #[cfg(test)]
    pub fn with_velocity_cap(mut self, cap: i128) -> Self {
        self.velocity_cap_wei = cap;
        self
    }
}

fn with_runtime(
    runtime: Arc<CustomerWalletRuntime>,
) -> impl Filter<Extract = (Arc<CustomerWalletRuntime>,), Error = std::convert::Infallible> + Clone
{
    warp::any().map(move || runtime.clone())
}

pub(crate) fn build_customer_wallet_routes<F>(
    runtime: CustomerWalletRuntime,
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

    let add_addr = warp::path!("v2" / "wallet" / "addresses")
        .and(warp::post())
        .and(auth.clone())
        .and(warp::body::json())
        .and(with_runtime(runtime.clone()))
        .and_then(handle_add_address)
        .boxed();

    let submit = warp::path!("v2" / "wallet" / "withdraw")
        .and(warp::post())
        .and(auth.clone())
        .and(warp::body::json())
        .and(with_runtime(runtime.clone()))
        .and_then(handle_submit_withdraw)
        .boxed();

    let status = warp::path!("v2" / "wallet" / "withdrawals" / String)
        .and(warp::get())
        .and(auth)
        .and(with_runtime(runtime))
        .and_then(handle_withdrawal_status)
        .boxed();

    add_addr
        .or(submit)
        .unify()
        .or(status)
        .unify()
        .boxed()
}

pub(crate) async fn handle_add_address(
    principal: AuthenticatedPrincipal,
    body: AddAddressBody,
    runtime: Arc<CustomerWalletRuntime>,
) -> Result<warp::reply::Json, Rejection> {
    if body.address.trim().is_empty() {
        return Err(reject(WalletError::BadRequest("address must not be empty")));
    }
    if body.label.trim().is_empty() {
        return Err(reject(WalletError::BadRequest("label must not be empty")));
    }
    if body.label.chars().count() > 256 {
        return Err(reject(WalletError::BadRequest(
            "label must be 256 characters or fewer",
        )));
    }
    // Per design §4.2: synchronously screen at add time. A Hit goes
    // straight to Suspended (NOT PendingCooldown). Provider Error is
    // a soft block (H1) — we MUST NOT silently let an un-screened
    // address into the book.
    let screen = runtime.sanctions.screen(body.chain, &body.address);
    let status = match screen.status {
        SanctionsScreenStatus::Hit => AddressStatus::Suspended,
        // Pending = "not yet checked"; in v1 our screen() call is
        // synchronous so this branch is defensive against future
        // async providers. Soft-block, do not let into the book.
        SanctionsScreenStatus::Pending | SanctionsScreenStatus::Error => {
            return Err(reject(WalletError::SanctionsUnavailable))
        }
        SanctionsScreenStatus::Clear => AddressStatus::PendingCooldown,
    };
    let now = Utc::now();
    let cooldown_until = match status {
        AddressStatus::PendingCooldown => {
            now + chrono::Duration::from_std(runtime.cooldown).unwrap_or_default()
        }
        _ => now,
    };
    let address_id = format!("addr-{}", uuid::Uuid::new_v4());
    let record = WithdrawalAddress {
        schema_version: WALLET_SCHEMA_VERSION,
        address_id: address_id.clone(),
        user_id: principal.subject.clone(),
        chain: body.chain,
        address: body.address.clone(),
        label: body.label.clone(),
        status,
        added_at: now,
        cooldown_until,
        last_used_at: None,
        sanctions_check: screen,
        added_by: principal.subject.clone(),
    };
    if let Err(e) = runtime.addresses.create(record.clone()) {
        return Err(reject(WalletError::Internal(format!(
            "address store create failed: {e}"
        ))));
    }
    Ok(warp::reply::json(&AddAddressResponse {
        address_id,
        chain: body.chain,
        status,
        cooldown_until,
    }))
}

pub(crate) async fn handle_submit_withdraw(
    principal: AuthenticatedPrincipal,
    body: SubmitWithdrawBody,
    runtime: Arc<CustomerWalletRuntime>,
) -> Result<warp::reply::Json, Rejection> {
    if body.amount <= 0 {
        return Err(reject(WalletError::BadRequest("amount must be > 0")));
    }
    let now = Utc::now();
    // 1. Resolve destination via address book (design §11 rule 1).
    let address = runtime
        .addresses
        .resolve(&principal.subject, body.chain, &body.destination_address)
        .ok_or_else(|| reject(WalletError::AddressNotFound))?;
    // Status / cool-down gate. Suspended or Removed = hard reject;
    // PendingCooldown still in window = also reject.
    if address.status == AddressStatus::Suspended || address.status == AddressStatus::Removed {
        return Err(reject(WalletError::AddressNotActive));
    }
    if address.status != AddressStatus::Active && address.cooldown_until > now {
        return Err(reject(WalletError::AddressNotActive));
    }
    // 2. Re-screen at validate time (design §4.2 + §11 rule 9).
    //    Both Hit AND Error are blocking here (H1).
    let screen = runtime.sanctions.screen(body.chain, &address.address);
    match screen.status {
        SanctionsScreenStatus::Hit => return Err(reject(WalletError::SanctionsHit)),
        SanctionsScreenStatus::Pending | SanctionsScreenStatus::Error => {
            return Err(reject(WalletError::SanctionsUnavailable))
        }
        SanctionsScreenStatus::Clear => {}
    }
    // 3. Balance pre-check (C2). Without this, the auto-walk to
    //    Approved followed by the hot-wallet broadcast can pay out
    //    funds for a customer whose cash account would go negative —
    //    the settlement debit only fires after the on-chain tx, by
    //    which point the money has already left the hot wallet.
    let estimated_fee: i128 = 1_000_000;
    let required = body.amount.saturating_add(estimated_fee);
    let required_i64: i64 = i64::try_from(required)
        .map_err(|_| reject(WalletError::AmountTooLarge))?;
    if runtime.ledger.cash_available_balance(&principal.subject) < required_i64 {
        return Err(reject(WalletError::InsufficientBalance));
    }
    // 4. Atomic velocity check-and-record (C3). The previous
    //    would_exceed + later record() pair raced: two concurrent
    //    submissions could both pass the read-only check, both
    //    insert, and silently breach the cap. try_record holds the
    //    bucket mutex across both operations.
    if runtime
        .velocity
        .try_record(
            &principal.subject,
            body.chain,
            body.amount,
            runtime.velocity_cap_wei,
            now,
        )
        .is_err()
    {
        return Err(reject(WalletError::VelocityExceeded));
    }
    // 5. Create record + auto-walk to Approved. Maker-checker for
    //    above-threshold amounts is a v1 follow-up; auto-approve in
    //    this commit so the smoke harness can drive a real flow.
    let withdrawal_id = format!("wd-cust-{}", uuid::Uuid::new_v4());
    let record = WithdrawalRecord {
        schema_version: WALLET_SCHEMA_VERSION,
        withdrawal_id: withdrawal_id.clone(),
        user_id: principal.subject.clone(),
        chain: body.chain,
        address_id: address.address_id.clone(),
        destination_address: address.address.clone(),
        amount: body.amount,
        estimated_fee: 1_000_000_i128,
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
        confirmations_required: 25,
        approval_request_id: None,
        rejection_reason: None,
        notes: body.client_reference.clone(),
    };
    if let Err(e) = runtime.withdrawals.create(record) {
        return Err(reject(WalletError::Internal(format!(
            "withdrawal store create failed: {e}"
        ))));
    }
    // 6. Walk Submitted → Validated → Queued → Approved.
    for next in [
        WithdrawalStatus::Validated,
        WithdrawalStatus::Queued,
        WithdrawalStatus::Approved,
    ] {
        if runtime
            .withdrawals
            .advance_status(&withdrawal_id, next)
            .is_err()
        {
            // Rare: state-machine validation failed; rollback by
            // advancing to Rejected so the customer sees the failure.
            let mut current = match runtime.withdrawals.get(&withdrawal_id) {
                Some(r) => r,
                None => {
                    return Err(reject(WalletError::Internal(
                        "withdrawal vanished mid-auto-walk".into(),
                    )))
                }
            };
            current.status = WithdrawalStatus::Rejected;
            current.rejection_reason = Some(WithdrawalRejectReason::OperatorRejected);
            current.notes = Some("auto-walk to Approved failed".into());
            let _ = runtime.withdrawals.update(current);
            return Err(reject(WalletError::Internal(
                "auto-walk to Approved failed".into(),
            )));
        }
    }
    // Velocity contribution was already recorded atomically in step 4.
    Ok(warp::reply::json(&SubmitWithdrawResponse {
        status: "approved".into(),
        withdrawal_id,
        chain: body.chain,
        amount: body.amount,
        destination_address: address.address,
    }))
}

pub(crate) async fn handle_withdrawal_status(
    withdrawal_id: String,
    principal: AuthenticatedPrincipal,
    runtime: Arc<CustomerWalletRuntime>,
) -> Result<warp::reply::Json, Rejection> {
    let record = runtime
        .withdrawals
        .get(&withdrawal_id)
        .ok_or_else(|| reject(WalletError::AddressNotFound))?;
    // Customers see only their own records (design §11 rule 1).
    // Admin would have a separate /admin/wallet/withdrawals endpoint;
    // not exposed on the customer surface. Non-owners are masked as
    // not-found rather than 403 to avoid leaking the existence of
    // another user's withdrawal_id.
    if record.user_id != principal.subject {
        return Err(reject(WalletError::AddressNotFound));
    }
    Ok(warp::reply::json(&record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventbus::EventBus;
    use persistence::InMemoryWal;
    use types::{LedgerDelta, PrincipalRole};
    use wallet::StubSanctionsProvider;
    use warp::Reply;

    fn principal(subject: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            subject: subject.into(),
            role: PrincipalRole::User,
            session_id: None,
        }
    }

    fn make_ledger() -> Arc<LedgerService> {
        Arc::new(LedgerService::with_wal_store(
            EventBus::new(),
            Arc::new(InMemoryWal::<LedgerDelta>::new()),
        ))
    }

    /// Seeded with `seed_user_cash` for "alice" so the C2 balance
    /// pre-check passes by default. Tests that want a broke user can
    /// pass a different subject.
    fn make_runtime() -> Arc<CustomerWalletRuntime> {
        // Seed must comfortably cover amount + estimated_fee (1_000_000)
        // for the happy-path tests; C2 broke-customer cases use a
        // different subject that is NOT funded.
        make_runtime_funded(&[("alice", 100_000_000)])
    }

    fn make_runtime_funded(seed: &[(&str, i64)]) -> Arc<CustomerWalletRuntime> {
        let addresses = Arc::new(AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let withdrawals = Arc::new(WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let sanctions: Arc<dyn SanctionsProvider> = Arc::new(StubSanctionsProvider::new());
        let velocity = Arc::new(VelocityTracker::with_default_window());
        let ledger = make_ledger();
        for (user, amount) in seed {
            ledger
                .process_deposit(user, *amount, format!("seed-{user}-{amount}"))
                .expect("seed deposit");
        }
        let runtime =
            CustomerWalletRuntime::new(addresses, withdrawals, sanctions, velocity, ledger)
                // Tiny cool-down for tests; real default is 24h.
                .with_cooldown(Duration::from_secs(0));
        Arc::new(runtime)
    }

    async fn body_json(reply: warp::reply::Json) -> serde_json::Value {
        let bytes = warp::hyper::body::to_bytes(reply.into_response().into_body())
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Extract the `WalletError` variant from a handler result so
    /// tests can assert on it. Panics if the handler returned Ok or
    /// the rejection wasn't a WalletError. We intentionally don't
    /// use `Result::unwrap_err` because `warp::reply::Json` doesn't
    /// implement `Debug`.
    fn expect_wallet_error<T>(r: Result<T, warp::Rejection>) -> WalletError {
        match r {
            Ok(_) => panic!("expected WalletError, got Ok"),
            Err(rej) => rej
                .find::<WalletError>()
                .cloned()
                .unwrap_or_else(|| panic!("rejection was not a WalletError: {rej:?}")),
        }
    }

    #[tokio::test]
    async fn add_address_clear_returns_pending_cooldown() {
        let runtime = make_runtime();
        let body = AddAddressBody {
            chain: ChainId::Eth,
            address: "0xclean".into(),
            label: "exchange wallet".into(),
        };
        let reply = handle_add_address(principal("alice"), body, runtime.clone())
            .await
            .unwrap();
        let json = body_json(reply).await;
        assert_eq!(json["chain"].as_str().unwrap(), "eth");
        assert_eq!(json["status"].as_str().unwrap(), "pending_cooldown");
        assert!(json["address_id"].as_str().unwrap().starts_with("addr-"));
    }

    #[tokio::test]
    async fn add_address_sanctions_hit_goes_straight_to_suspended() {
        let runtime = make_runtime();
        // Ugly cast to manipulate the underlying stub provider.
        // We rely on the test only seeing one provider and just
        // construct a fresh runtime with a pre-seeded stub.
        let stub = Arc::new(StubSanctionsProvider::new());
        stub.add_hit(ChainId::Eth, "0xbad");
        let provider: Arc<dyn SanctionsProvider> = stub;
        let r2 = Arc::new(CustomerWalletRuntime::new(
            runtime.addresses.clone(),
            runtime.withdrawals.clone(),
            provider,
            runtime.velocity.clone(),
            runtime.ledger.clone(),
        ));
        let body = AddAddressBody {
            chain: ChainId::Eth,
            address: "0xbad".into(),
            label: "looks bad".into(),
        };
        let reply = handle_add_address(principal("alice"), body, r2).await.unwrap();
        let json = body_json(reply).await;
        assert_eq!(json["status"].as_str().unwrap(), "suspended");
    }

    #[tokio::test]
    async fn submit_rejects_when_destination_not_in_address_book() {
        let runtime = make_runtime();
        let body = SubmitWithdrawBody {
            chain: ChainId::Eth,
            destination_address: "0xunknown".into(),
            amount: 100,
            client_reference: None,
        };
        let r = handle_submit_withdraw(principal("alice"), body, runtime).await;
        // C1: ad-hoc destination must surface as AddressNotFound (404),
        // not a generic 500. Maps to status 404 via wallet_error_to_reply.
        assert_eq!(expect_wallet_error(r), WalletError::AddressNotFound);
    }

    #[tokio::test]
    async fn add_address_when_sanctions_provider_errors_returns_unavailable() {
        // H1: provider Error must NOT silently let the address into
        // the book. Previously the `_ => PendingCooldown` arm masked
        // every non-Hit status as Clear.
        let addresses = Arc::new(AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let withdrawals = Arc::new(WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let stub = Arc::new(StubSanctionsProvider::new());
        stub.add_error(ChainId::Eth, "0xprovider-down");
        let sanctions: Arc<dyn SanctionsProvider> = stub;
        let velocity = Arc::new(VelocityTracker::with_default_window());
        let ledger = make_ledger();
        let runtime = Arc::new(
            CustomerWalletRuntime::new(addresses, withdrawals, sanctions, velocity, ledger)
                .with_cooldown(Duration::from_secs(0)),
        );
        let body = AddAddressBody {
            chain: ChainId::Eth,
            address: "0xprovider-down".into(),
            label: "provider down".into(),
        };
        let r = handle_add_address(principal("alice"), body, runtime).await;
        assert_eq!(
            expect_wallet_error(r),
            WalletError::SanctionsUnavailable
        );
    }

    #[tokio::test]
    async fn submit_when_sanctions_provider_errors_at_validate_returns_unavailable() {
        // H1 (validate-time path): even if the address was Clear at
        // add-time, a provider outage during the validate-time
        // re-screen must hard-block.
        let addresses = Arc::new(AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let withdrawals = Arc::new(WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let stub = Arc::new(StubSanctionsProvider::new());
        let sanctions: Arc<dyn SanctionsProvider> = stub.clone();
        let velocity = Arc::new(VelocityTracker::with_default_window());
        let ledger = make_ledger();
        ledger
            .process_deposit("alice", 100_000_000, "seed-h1-validate".to_string())
            .unwrap();
        let runtime = Arc::new(
            CustomerWalletRuntime::new(addresses, withdrawals, sanctions, velocity, ledger)
                .with_cooldown(Duration::from_secs(0)),
        );
        let _ = handle_add_address(
            principal("alice"),
            AddAddressBody {
                chain: ChainId::Eth,
                address: "0xclean-now-flaky".into(),
                label: "x".into(),
            },
            runtime.clone(),
        )
        .await
        .unwrap();
        runtime.addresses.sweep_cooldowns().unwrap();
        // Provider goes down between add and submit.
        stub.add_error(ChainId::Eth, "0xclean-now-flaky");
        let r = handle_submit_withdraw(
            principal("alice"),
            SubmitWithdrawBody {
                chain: ChainId::Eth,
                destination_address: "0xclean-now-flaky".into(),
                amount: 100,
                client_reference: None,
            },
            runtime,
        )
        .await;
        assert_eq!(
            expect_wallet_error(r),
            WalletError::SanctionsUnavailable
        );
    }

    #[tokio::test]
    async fn wallet_error_to_reply_maps_status_codes_correctly() {
        // C1: the status-code contract is API surface — clients (incl.
        // the React frontend) depend on it. Lock it in with an
        // explicit table-driven test.
        use warp::http::StatusCode;
        let cases: &[(WalletError, StatusCode, &str)] = &[
            (WalletError::BadRequest("x"), StatusCode::BAD_REQUEST, "bad_request"),
            (WalletError::AddressNotFound, StatusCode::NOT_FOUND, "address_not_found"),
            (WalletError::AddressNotActive, StatusCode::CONFLICT, "address_not_active"),
            (WalletError::SanctionsHit, StatusCode::FORBIDDEN, "sanctions_hit"),
            (WalletError::SanctionsUnavailable, StatusCode::SERVICE_UNAVAILABLE, "sanctions_unavailable"),
            (WalletError::VelocityExceeded, StatusCode::CONFLICT, "velocity_exceeded"),
            (WalletError::InsufficientBalance, StatusCode::CONFLICT, "insufficient_balance"),
            (WalletError::AmountTooLarge, StatusCode::BAD_REQUEST, "amount_too_large"),
            (WalletError::Forbidden, StatusCode::FORBIDDEN, "forbidden"),
            (WalletError::Internal("oops".into()), StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        ];
        for (err, expected_status, expected_code) in cases {
            let reply = wallet_error_to_reply(err);
            let response = reply.into_response();
            assert_eq!(response.status(), *expected_status, "{:?}", err);
            let bytes = warp::hyper::body::to_bytes(response.into_body()).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["error_code"].as_str().unwrap(), *expected_code);
        }
    }

    #[tokio::test]
    async fn submit_happy_path_creates_approved_record() {
        let runtime = make_runtime();
        // 1. Whitelist address (cool-down=0 so it's Active immediately).
        let _ = handle_add_address(
            principal("alice"),
            AddAddressBody {
                chain: ChainId::Eth,
                address: "0xclean".into(),
                label: "exchange".into(),
            },
            runtime.clone(),
        )
        .await
        .unwrap();
        // The record was added in PendingCooldown with cooldown_until=now;
        // resolve() returns it but the handler also checks cooldown.
        // Promote via sweep so it's Active.
        runtime.addresses.sweep_cooldowns().unwrap();

        // 2. Submit withdrawal.
        let body = SubmitWithdrawBody {
            chain: ChainId::Eth,
            destination_address: "0xclean".into(),
            amount: 1_000,
            client_reference: Some("ref-1".into()),
        };
        let reply = handle_submit_withdraw(principal("alice"), body, runtime.clone())
            .await
            .unwrap();
        let json = body_json(reply).await;
        assert_eq!(json["status"].as_str().unwrap(), "approved");
        let withdrawal_id = json["withdrawal_id"].as_str().unwrap().to_string();
        let record = runtime.withdrawals.get(&withdrawal_id).unwrap();
        assert_eq!(record.status, WithdrawalStatus::Approved);
        assert_eq!(record.user_id, "alice");
        assert_eq!(record.amount, 1_000);
        // Velocity recorded.
        assert_eq!(
            runtime.velocity.total("alice", ChainId::Eth, Utc::now()),
            1_000
        );
    }

    #[tokio::test]
    async fn submit_blocked_when_velocity_cap_exceeded() {
        let runtime = make_runtime();
        // Lower the cap to 500 so a single 1000 withdrawal trips it.
        let runtime = Arc::new(CustomerWalletRuntime {
            cooldown: Duration::from_secs(0),
            velocity_cap_wei: 500,
            ..(*runtime).clone()
        });
        // Whitelist + promote.
        let _ = handle_add_address(
            principal("alice"),
            AddAddressBody {
                chain: ChainId::Eth,
                address: "0xclean".into(),
                label: "exchange".into(),
            },
            runtime.clone(),
        )
        .await
        .unwrap();
        runtime.addresses.sweep_cooldowns().unwrap();
        let body = SubmitWithdrawBody {
            chain: ChainId::Eth,
            destination_address: "0xclean".into(),
            amount: 1_000,
            client_reference: None,
        };
        let r = handle_submit_withdraw(principal("alice"), body, runtime).await;
        // C1: lower the cap; second submit must surface VelocityExceeded
        // (HTTP 409), not a generic 404.
        assert_eq!(expect_wallet_error(r), WalletError::VelocityExceeded);
    }

    #[tokio::test]
    async fn submit_blocked_when_address_sanctions_hit_at_validate() {
        // Customer-side stub provider; we add a hit AFTER whitelisting.
        let addresses = Arc::new(AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let withdrawals = Arc::new(WithdrawalStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let sanctions = Arc::new(StubSanctionsProvider::new());
        let velocity = Arc::new(VelocityTracker::with_default_window());
        let ledger = make_ledger();
        ledger
            .process_deposit("alice", 100_000_000, "seed-alice-sanctions-test".to_string())
            .unwrap();
        let runtime = Arc::new(
            CustomerWalletRuntime::new(addresses, withdrawals, sanctions.clone(), velocity, ledger)
                .with_cooldown(Duration::from_secs(0)),
        );
        // Whitelist clean.
        let _ = handle_add_address(
            principal("alice"),
            AddAddressBody {
                chain: ChainId::Eth,
                address: "0xnow-bad".into(),
                label: "started clean".into(),
            },
            runtime.clone(),
        )
        .await
        .unwrap();
        runtime.addresses.sweep_cooldowns().unwrap();
        // Provider gets new info: address is now sanctioned.
        sanctions.add_hit(ChainId::Eth, "0xnow-bad");
        // Submit — the validate-time re-screen catches it.
        let body = SubmitWithdrawBody {
            chain: ChainId::Eth,
            destination_address: "0xnow-bad".into(),
            amount: 100,
            client_reference: None,
        };
        let r = handle_submit_withdraw(principal("alice"), body, runtime).await;
        // C1: validate-time sanctions hit → 403 SanctionsHit, not 404.
        assert_eq!(expect_wallet_error(r), WalletError::SanctionsHit);
    }

    #[tokio::test]
    async fn submit_blocked_when_customer_balance_below_amount_plus_fee() {
        // C2 regression: a broke customer must not get auto-Approved.
        // Seed a different funded user so make_runtime's "alice" is
        // funded but our broke subject is not.
        let runtime = make_runtime();
        // Whitelist a destination for the broke user.
        let _ = handle_add_address(
            principal("broke"),
            AddAddressBody {
                chain: ChainId::Eth,
                address: "0xclean".into(),
                label: "x".into(),
            },
            runtime.clone(),
        )
        .await
        .unwrap();
        runtime.addresses.sweep_cooldowns().unwrap();
        // Submit — must reject because broke has zero cash.
        let body = SubmitWithdrawBody {
            chain: ChainId::Eth,
            destination_address: "0xclean".into(),
            amount: 1_000,
            client_reference: None,
        };
        let r = handle_submit_withdraw(principal("broke"), body, runtime.clone()).await;
        // C1+C2: must surface InsufficientBalance (409), not a generic 404.
        assert_eq!(expect_wallet_error(r), WalletError::InsufficientBalance);
        // No record should have been created (and no velocity row).
        assert_eq!(
            runtime.velocity.total("broke", ChainId::Eth, Utc::now()),
            0,
            "velocity recorded for a rejected submit — C3 regressed"
        );
    }

    #[tokio::test]
    async fn submit_does_not_record_velocity_on_balance_reject() {
        // Combined C2+C3 regression: the velocity contribution must
        // NOT be recorded if the balance check rejects the submit.
        // This is the bug the original "record after create" ordering
        // could mask.
        let runtime = make_runtime();
        let _ = handle_add_address(
            principal("broke2"),
            AddAddressBody {
                chain: ChainId::Eth,
                address: "0xclean".into(),
                label: "x".into(),
            },
            runtime.clone(),
        )
        .await
        .unwrap();
        runtime.addresses.sweep_cooldowns().unwrap();
        for _ in 0..5 {
            let _ = handle_submit_withdraw(
                principal("broke2"),
                SubmitWithdrawBody {
                    chain: ChainId::Eth,
                    destination_address: "0xclean".into(),
                    amount: 100,
                    client_reference: None,
                },
                runtime.clone(),
            )
            .await;
        }
        assert_eq!(runtime.velocity.total("broke2", ChainId::Eth, Utc::now()), 0);
    }

    #[tokio::test]
    async fn status_endpoint_owner_can_read_others_cannot() {
        let runtime = make_runtime();
        let _ = handle_add_address(
            principal("alice"),
            AddAddressBody {
                chain: ChainId::Eth,
                address: "0xclean".into(),
                label: "x".into(),
            },
            runtime.clone(),
        )
        .await
        .unwrap();
        runtime.addresses.sweep_cooldowns().unwrap();
        let reply = handle_submit_withdraw(
            principal("alice"),
            SubmitWithdrawBody {
                chain: ChainId::Eth,
                destination_address: "0xclean".into(),
                amount: 1,
                client_reference: None,
            },
            runtime.clone(),
        )
        .await
        .unwrap();
        let withdrawal_id = body_json(reply).await["withdrawal_id"]
            .as_str()
            .unwrap()
            .to_string();
        // Owner reads.
        let r = handle_withdrawal_status(withdrawal_id.clone(), principal("alice"), runtime.clone())
            .await
            .unwrap();
        let json = body_json(r).await;
        assert_eq!(json["user_id"].as_str().unwrap(), "alice");
        // Non-owner masked as AddressNotFound (404) — matches the
        // previous behaviour of not leaking another user's IDs.
        let r2 = handle_withdrawal_status(withdrawal_id, principal("bob"), runtime).await;
        assert_eq!(expect_wallet_error(r2), WalletError::AddressNotFound);
    }
}

// Need Clone on the runtime for the test that overrides cap.
impl Clone for CustomerWalletRuntime {
    fn clone(&self) -> Self {
        Self {
            addresses: self.addresses.clone(),
            withdrawals: self.withdrawals.clone(),
            sanctions: self.sanctions.clone(),
            velocity: self.velocity.clone(),
            ledger: self.ledger.clone(),
            cooldown: self.cooldown,
            velocity_cap_wei: self.velocity_cap_wei,
        }
    }
}
