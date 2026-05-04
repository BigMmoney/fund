# Withdrawal Risk & Custody

> How customer withdrawals are screened, queued, signed, broadcast, and reconciled.
> How the hot wallet is funded, capped, monitored, and killed.
> Who can stop the line and how.
>
> Branch `p0-recovery-20260430` · HEAD `c6b790f`

---

## 1. Threat model

A successful attack moves more value out of the hot wallet than the legitimate customer is owed, OR moves value to a destination the customer did not authorize. Adversaries to plan against:

| Adversary | Capability we mitigate |
|---|---|
| **Compromised customer credential** | Address book + 24h cool-down; sanctions screen; per-day velocity cap |
| **Malicious operator (single)** | Maker-checker on above-threshold actions; `denied_self_approval`; audit trail |
| **Compromised operator pair (collusion)** | Hard limits (velocity cap, daily withdrawal ceiling); kill switch; review of audit log |
| **Compromised hot-wallet signer** | Hot wallet cap on chain; cold-storage majority; rotation runbook |
| **Compromised RPC node** | Multi-RPC failover (REAL_CHAIN_ADAPTER_SPEC.md §4); confirmation depth |
| **Network attacker (replay/MITM)** | HMAC + body-SHA256; clock-skew window; TLS terminator in front |
| **Provider outage (sanctions / RPC)** | Soft-block 503 (sanctions Error → `SanctionsUnavailable`); RPC failover; circuit breaker |

Out of scope: chain-level attacks (51%, deep reorgs beyond `confirmations_required`); customer-device compromise.

---

## 2. Customer withdrawal pipeline

### 2.1 State machine

```
Submitted → Validated → Queued → (AwaitingApproval) → Approved
         ↓           ↓        ↓                       ↓
       Rejected   Rejected  Rejected               Signing → Broadcast → Confirmed → Settled
                                                                                  ↘
                                                                              SettlementStuck → Settled / Rejected
```

Encoded in `wallet::is_valid_transition`. Transitions outside this matrix are rejected with `WithdrawalStore::update` returning `Err`.

### 2.2 Stage-by-stage gates

| Stage | Code | Gate |
|---|---|---|
| **Submit** | `customer_wallet_http::handle_submit_withdraw` | (1) idempotency by `(user, chain, client_reference)`; (2) `amount > 0` and `≤ max_amount_i128`; (3) address resolves in caller's address book; (4) address status = Active or PendingCooldown past window; (5) re-screen sanctions (Hit auto-suspends address; Error → 503); (6) `cash_available_balance(user) ≥ amount + estimated_fee`; (7) atomic velocity `try_record` against per-day cap |
| **Validate** | auto-walked from Submit | post-checks if any pending; in v1 absorbed into Submit |
| **Queue** | auto-walked from Validate | record at `Queued`; visible in `/admin/wallet/queue` |
| **Approve** | auto OR maker-checker | v1 auto-approves below threshold; **gate P0-FUND-4** wires MC for above-threshold |
| **Sign / Broadcast** | `wallet::HotWalletWorker::tick()` | adapter signs with hot-wallet key; broadcasts via primary RPC; falls over to secondary on failure (REAL_CHAIN_ADAPTER_SPEC.md §4) |
| **Confirm** | same worker | adapter polls confirmation depth; flips at `confirmations_required` (default 25 for ETH) |
| **Settle** | `admin_wallet_settlement::SettlementWorker::tick()` | ledger debit `wd-settle-{id}` (idempotent); failure → `SettlementStuck` |

### 2.3 Address book

- Customer POSTs `/v2/wallet/addresses` with `(chain, address, label)`
- Synchronous sanctions screen at add time:
  - **Hit** → status `Suspended`; cannot be used
  - **Clear** → status `PendingCooldown` with `cooldown_until = now + WALLET_CUSTOMER_COOLDOWN_SECS` (default 24h)
  - **Error / Pending** → reject with `SanctionsUnavailable` (503); address not added
- 60s `sweep_cooldowns()` task auto-promotes to `Active` once `cooldown_until ≤ now`
- DELETE flips status to `Removed`; the record stays in the WAL for audit

**Design rule §11.1:** withdrawals MUST resolve via the address book. Ad-hoc destinations on `/v2/wallet/withdraw` are rejected with `AddressNotFound` (404).

**Design rule §4.2 + §11.9:** sanctions screen runs at BOTH add time AND validate time. A clear-then-bad address (provider added a hit between add and submit) is caught at validate; the address is auto-Suspended (gate **P0-CORR-6**).

### 2.4 Per-customer velocity cap

`wallet::VelocityTracker` keeps a 24h rolling sum per `(user_id, chain)`:

- Updated atomically at submit time via `try_record(user, chain, amount, cap, now)` — single mutex acquisition (gate **P0-CORR-2**); no TOCTOU race between concurrent submissions
- Default cap: `DEFAULT_VELOCITY_CAP_WEI = 500e18` (placeholder; real production cap is set per chain)
- Rebuilt at boot from `WithdrawalStore::all()` via `wallet::build_velocity_tracker` (gate **P0-CORR-3**); without this the first 24h after a restart had effectively no cap
- Exceedance returns `WalletError::VelocityExceeded` (HTTP 409)

---

## 3. Hot wallet

### 3.1 Capacity & topology

| Property | v1 (current) | Production target (P0-FUND) |
|---|---|---|
| Adapter | `wallet::InMemoryChainAdapter` | `wallet::EthRpcAdapter` (etc.) |
| Hot-wallet address | one per chain (`WALLET_ETH_HOT_ADDRESS`) | one per chain, multi-sig 2-of-3 |
| Max balance on chain | unbounded (test) | configurable cap; alert at 80% |
| Refill from cold | manual | admin endpoint `/admin/wallet/refill` (planned) requires MC |
| Drain to cold | manual | admin endpoint `/admin/wallet/drain` (planned) requires MC |

The hot wallet is the only on-chain custody surface that signs without a human in the loop. Everything else lives in cold storage.

### 3.2 Cap enforcement

**Pre-broadcast cap check (planned, P0-FUND).** Worker refuses to broadcast if `(hot_balance − amount − estimated_fee) < safety_floor`. Currently the in-memory adapter only refuses if hot is below `amount + fee`, surfaced as `WithdrawalRejectReason::DaemonRejected` and the record stays at `Approved` for the next tick.

### 3.3 Private-key handling

| Surface | Storage |
|---|---|
| Hot wallet private key | KMS-sealed env var → loaded once into worker process memory; never written to disk |
| Cold wallet | offline; HSM or air-gapped signer; out of scope for `api` crate |
| HMAC shared secret | KMS-sealed env (`INTERNAL_AUTH_SHARED_SECRET`) |
| Sanctions API key | KMS-sealed env |
| Operator MFA tokens | identity provider (planned) |

The `api` process holds the hot-wallet key in memory only. A core dump must be treated as a credential incident and the key rotated. Process shutdown should zeroize the key (planned: `zeroize` crate integration).

### 3.4 Rotation runbook (summary)

1. Generate new hot-wallet keypair offline.
2. Drain old hot to cold (admin MC).
3. Update `WALLET_ETH_HOT_ADDRESS` + sealed key env on the api host.
4. Restart api process during a scheduled freeze window.
5. Refill new hot from cold (admin MC).
6. Verify INV-5 reconciliation.

---

## 4. Maker-checker

### 4.1 Where it bites

| Action | Resolution today | Target |
|---|---|---|
| `MarketHalt` | break-glass single-actor (allow) | unchanged |
| `MarketResume` | RequiresApproval | unchanged |
| Customer withdrawal below threshold | auto-approved | unchanged |
| Customer withdrawal above `WALLET_CUSTOMER_MC_THRESHOLD` | auto-approved (gap) | RequiresApproval (**P0-FUND-4**) |
| Hot wallet refill / drain | n/a (manual today) | RequiresApproval |
| Internal transfer | RequiresApproval | unchanged |
| RBAC grant changes | RequiresApproval | unchanged |
| Settlement-stuck recovery | n/a today | RequiresApproval (planned) |

### 4.2 Mechanics

- Submitter POSTs `/admin/approval-requests` with `(action, resource, scope, reason, action_payload)`
- A second admin (NOT the submitter) POSTs `/admin/approval-requests/{id}/approve`
- `find_committed_approval(action, resource, submitter)` rejects self-approval
- Audit row `denied_self_approval` written even on rejection
- Once committed, the action handler re-checks `find_committed_approval(...)` before mutating state — the approval token is the gate, not a side channel

### 4.3 Acceptance criteria

| Property | Test |
|---|---|
| Submitter cannot self-approve | smoke phase 7 + `admin_approvals_http::approve_rejects_self_approval` |
| Approval cannot be reused for a different action | (planned regression test) |
| Approval expires after `APPROVAL_TTL_SECS` | (planned) |
| Audit row written on every submit, approve, deny | `data/admin/rbac_audit.jsonl` inspected by smoke phase 8 |

---

## 5. SettlementStuck handling

The settlement worker flips a record to `SettlementStuck` when the on-chain broadcast already happened but the customer-side ledger debit failed (e.g. balance went negative between submit and settle, or the per-chain divisor pushed amount past i64).

### 5.1 Detection

- Worker emits warn-log: `wallet.settlement.stuck — operator action required`
- `SettlementTickReport.stuck_count > 0` increments a Prometheus counter (gate **P1-OPS-1**)
- Alert fires to on-call

### 5.2 Resolution path

1. On-call retrieves `withdrawal_id` from the alert
2. Inspect `WithdrawalRecord.notes` — contains the ledger error
3. Run reconciliation runbook §6 to confirm the actual on-chain payout amount
4. Top up customer cash via `/admin/transfers` (RequiresApproval) for the shortfall
5. Flip `SettlementStuck → Settled` via admin endpoint (planned; today done by direct `WithdrawalStore.update` from a recovery shell)
6. File post-incident: why the balance check at submit didn't catch this

### 5.3 Why we don't auto-recover

The on-chain side already moved real money. Any auto-recovery path (re-debit later, charge an insurance fund) creates a value movement that bypasses the maker-checker for sums above threshold. We require human review for every stuck record so the policy decision is auditable.

---

## 6. Kill switches

### 6.1 What can be killed, by whom, and how

| Switch | Effect | Required actor | Mechanism |
|---|---|---|---|
| **Pause customer withdrawals** | All `/v2/wallet/withdraw` returns 503 | single admin (break-glass) | env `WALLET_CUSTOMER_PAUSED=1` + restart (planned: live toggle endpoint) |
| **Pause hot wallet worker** | Approved → Broadcast halts; existing Broadcast records still get confirmed | single admin (break-glass) | env `WALLET_WORKER_DISABLED=1` + restart (planned) |
| **Halt market** | All matching paused for that market | single admin (break-glass) per design §4 | `POST /admin/trading-ops/markets/{id}/halt` (already live) |
| **Resume market** | Matching resumes | RequiresApproval | `POST /admin/trading-ops/markets/{id}/resume` (already live) |
| **Mass cancel** | All open orders for a user / session / market canceled | RequiresApproval | `POST /admin/trading-ops/mass-cancel` (already live) |
| **Stop entire api** | Process exits | infrastructure | systemd / k8s replica scale-to-zero |

### 6.2 Acceptance: kill activates within 10 seconds

The market-halt path is in-process and instant. Withdrawal pause / worker pause are env-driven today which means they require a restart — adding live toggle endpoints is **gate P0-FUND** follow-up.

### 6.3 What survives a kill

| Resource | Survives? |
|---|---|
| In-flight Broadcast tx | yes — chain confirms regardless |
| Queued withdrawal | yes — visible in `/admin/wallet/queue`, picked up after unkill |
| Ledger | yes — durable in JSONL |
| Velocity tracker | yes — rebuilt from history at boot (P0-CORR-3) |
| Address book | yes |

---

## 7. Audit & retention

| Log | Path | Retention | Owner |
|---|---|---|---|
| Customer wallet audit | `data/wallet/customer_audit.jsonl` | 7 years (regulatory) | finance |
| RBAC audit | `data/admin/rbac_audit.jsonl` | 7 years | security |
| Withdrawal store | `data/wallet/withdrawals.jsonl` | 7 years | finance |
| Address book | `data/wallet/addresses.jsonl` | 7 years (incl. Removed records) | finance |
| Ledger deltas | `data/ledger/deltas.jsonl` | 7 years | finance |
| Order Flow Monitor | `data/monitor/order_trace.jsonl` | 90 days | engineering |
| Sanctions provider responses | (provider side) | per provider contract | compliance |

Retention is enforced by external archival; the application appends, never deletes.

---

## 8. Acceptance summary

A withdrawal pipeline is launch-ready when:

| | |
|---|---|
| ✓ | All P0-CORR rows in PRODUCTION_LAUNCH_CHECKLIST.md are 🟢 |
| ✓ | Real chain adapter wired (P0-FUND-1) |
| ✓ | Per-chain settlement accounts (P0-FUND-2) |
| ✓ | Above-threshold MC live (P0-FUND-4) |
| ✓ | Sanctions provider real (P0-SEC-6) |
| ✓ | Hot wallet rotation runbook executed once in staging |
| ✓ | SettlementStuck alert + paging tested via injected fault |
| ✓ | Kill-switch drill: pause customer withdrawals, observe queue accumulate, unpause, observe drain |

---

*Last updated 2026-05-04. Owners: wallet b.greifen, custody UNASSIGNED, compliance UNASSIGNED.*
