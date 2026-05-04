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
        return Err(warp::reject::not_found());
    }
    if body.label.trim().is_empty() || body.label.len() > 256 {
        return Err(warp::reject::not_found());
    }
    // Per design §4.2: synchronously screen at add time; a hit goes
    // straight to Suspended (NOT PendingCooldown).
    let screen = runtime
        .sanctions
        .screen(body.chain, &body.address);
    let status = match screen.status {
        SanctionsScreenStatus::Hit => AddressStatus::Suspended,
        _ => AddressStatus::PendingCooldown,
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
    if runtime.addresses.create(record.clone()).is_err() {
        return Err(warp::reject::not_found());
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
        return Err(warp::reject::not_found());
    }
    let now = Utc::now();
    // 1. Resolve destination via address book (design §11 rule 1).
    let address = match runtime
        .addresses
        .resolve(&principal.subject, body.chain, &body.destination_address)
    {
        Some(a) => a,
        None => return Err(warp::reject::not_found()),
    };
    if address.status != AddressStatus::Active && address.cooldown_until > now {
        return Err(warp::reject::not_found());
    }
    if address.status == AddressStatus::Suspended || address.status == AddressStatus::Removed {
        return Err(warp::reject::not_found());
    }
    // 2. Re-screen at validate time (design §4.2 + §11 rule 9).
    let screen = runtime.sanctions.screen(body.chain, &address.address);
    if screen.status == SanctionsScreenStatus::Hit {
        return Err(warp::reject::not_found());
    }
    // 3. Balance pre-check (C2). Without this, the auto-walk to
    //    Approved followed by the hot-wallet broadcast can pay out
    //    funds for a customer whose cash account would go negative —
    //    the settlement debit only fires after the on-chain tx, by
    //    which point the money has already left the hot wallet.
    let estimated_fee: i128 = 1_000_000;
    let required = body.amount.saturating_add(estimated_fee);
    let required_i64: i64 = match i64::try_from(required) {
        Ok(v) => v,
        Err(_) => return Err(warp::reject::not_found()),
    };
    if runtime.ledger.cash_available_balance(&principal.subject) < required_i64 {
        return Err(warp::reject::not_found());
    }
    // 4. Atomic velocity check-and-record (C3). The previous
    //    would_exceed + later record() pair raced: two concurrent
    //    submissions could both pass the check and both insert,
    //    silently breaching the cap. try_record holds the bucket
    //    mutex across both operations.
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
        return Err(warp::reject::not_found());
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
    if runtime.withdrawals.create(record).is_err() {
        return Err(warp::reject::not_found());
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
                None => return Err(warp::reject::not_found()),
            };
            current.status = WithdrawalStatus::Rejected;
            current.rejection_reason = Some(WithdrawalRejectReason::OperatorRejected);
            current.notes = Some("auto-walk to Approved failed".into());
            let _ = runtime.withdrawals.update(current);
            return Err(warp::reject::not_found());
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
    let record = match runtime.withdrawals.get(&withdrawal_id) {
        Some(r) => r,
        None => return Err(warp::reject::not_found()),
    };
    // Customers see only their own records (design §11 rule 1).
    // Admin would have a separate /admin/wallet/withdrawals endpoint;
    // not exposed on the customer surface.
    if record.user_id != principal.subject {
        return Err(warp::reject::not_found());
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
        assert!(r.is_err());
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
        assert!(r.is_err());
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
        assert!(r.is_err());
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
        assert!(r.is_err(), "broke customer was auto-Approved — C2 regressed");
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
        // Non-owner masked as 404.
        let r2 = handle_withdrawal_status(withdrawal_id, principal("bob"), runtime).await;
        assert!(r2.is_err());
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
