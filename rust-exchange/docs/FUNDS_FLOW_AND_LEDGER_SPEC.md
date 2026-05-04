# Funds Flow & Ledger Specification

> **Authoritative reference** for how money moves through `rust-exchange`, what the system accounts are, what each ledger entry looks like, and the reconciliation invariants that **must** hold at all times.
> Branch `p0-recovery-20260430` · HEAD `c6b790f`

---

## 1. Core invariants

The ledger is double-entry. At any commit boundary the following invariants MUST hold; if any breaks, the deploy is unsafe and on-call must roll back.

| ID | Invariant | Why |
|---|---|---|
| **INV-1** | Σ(all account balances) == 0 | Double-entry: every credit has a matching debit. System accounts can go negative; user accounts cannot. |
| **INV-2** | For every user account `U`, `available(U) == balance(U) − sum_reservations(U)` | Available is what the user can spend right now; reservations are pre-committed funds. |
| **INV-3** | For every `op_id` ever submitted, `LedgerService.transfer(op_id, …)` is applied at most once | Idempotency. Re-submitting the same `op_id` is a no-op. |
| **INV-4** | For every withdrawal at status ≥ `Settled`, exactly one ledger debit `op_id = wd-settle-{withdrawal_id}` exists on the user's cash account | One on-chain payout ↔ one ledger debit. |
| **INV-5** | For every chain `C`, `hot_wallet_onchain_balance(C) == −SYS:WALLET:HOT:C` | The hot wallet's negative ledger position equals what it actually holds on chain. |
| **INV-6** | `withdrawal.status ∈ {Settled, SettlementStuck}` IFF its tx hit `confirmations_required` on chain | Settlement happens after, never before, on-chain confirmation. |

Reconciliation jobs (see RECONCILIATION_AND_RECOVERY_RUNBOOK.md) re-check INV-1, INV-4, INV-5 daily.

---

## 2. System accounts

System accounts hold no customer; they exist to keep the ledger balanced when funds move across the boundary of the system. **Only system accounts are allowed to go negative.**

| Account | Role | Sign convention | Owner |
|---|---|---|---|
| `SYS:ONCHAIN_VAULT:USDC` | Single front-account for every customer deposit/withdrawal in v1. Will be split per-chain in v1.1 (gate **P0-FUND-2**). | Negative when customers hold credited funds | platform |
| `SYS:WALLET:HOT:eth` *(planned, P0-FUND-2)* | The hot wallet's ledger mirror for ETH | Negative when on-chain hot wallet holds funds | wallet ops |
| `SYS:WALLET:HOT:btc` *(planned)* | Same for BTC | Negative | wallet ops |
| `SYS:FEE:WITHDRAW:eth` *(planned)* | Records the gas/fee debited per withdrawal | Positive | finance |
| `SYS:FEE:TRADING` | Trading-fee revenue | Positive | finance |
| `SYS:LIQUIDATION:INSURANCE` | Insurance fund for liquidations | Positive | risk |
| `SYS:ADL:SOCIALIZED` | Auto-deleverage settlement bucket | ±  | risk |

In v1 every withdrawal credits `SYS:ONCHAIN_VAULT:USDC` because per-chain accounting is a v1.1 follow-up. The reconciliation job knows this and applies the relaxed form of INV-5 until P0-FUND-2 lands.

### Account naming

`SYS:` prefix is reserved. Customer accounts use the bare `user_id` as the cash account key, plus per-instrument keys for positions:

| Pattern | Example | Crate function |
|---|---|---|
| `<user_id>` | `alice` | `LedgerService::cash_account(&user)` returns the user_id directly |
| `pos:<user>:<market>:<outcome>` | `pos:alice:btc-usdt:0` | `LedgerService::position_account(...)` |

---

## 3. Funds-flow scenarios

Each scenario lists the externally observable cause, the resulting `LedgerDelta` entries (debit / credit pairs), and the `op_id` shape used for idempotency.

### 3.1 Customer deposit (on-chain → ledger credit)

**Trigger:** chain adapter observes a confirmed inbound tx OR an admin POSTs `/deposit`.

**op_id:** `dep-{chain}-{tx_hash}` (chain-observed) or caller-supplied.

```
SYS:ONCHAIN_VAULT:USDC   −amount    (account goes more negative)
<user_id>                +amount    (user cash credited)
```

Code path: `LedgerService::process_deposit(user, amount, op_id)` — this is the canonical funding entrypoint and the only one that exists in v1. The wallet test seed and the smoke harness both use it.

### 3.2 Customer withdrawal (ledger debit → on-chain payout)

**Trigger:** customer POSTs `/v2/wallet/withdraw`; settlement worker sees record at `Confirmed`.

**op_id:** `wd-settle-{withdrawal_id}`.

```
<user_id>                −amount    (user cash debited; balance check at submit time prevents negative)
SYS:ONCHAIN_VAULT:USDC   +amount    (vault becomes less negative)
```

Settlement is the **mirror image** of deposit. Same single system account in v1.

If the debit fails (e.g. user balance went negative between submit and settle), the worker flips the record to `SettlementStuck` (gate **P0-CORR-4**); no further ledger movement happens until an operator reconciles. See WITHDRAWAL_RISK_AND_CUSTODY.md §5 for stuck-handling.

### 3.3 Trading fill (cash + position transfer)

**Trigger:** matching engine produces a `Fill`.

**op_ids:**
- cash leg: `fill-cash-{trade_id}-{side}`
- position leg: `fill-pos-{trade_id}-{side}`
- fee leg: `fill-fee-{trade_id}-{side}`

```
Buyer cash:        −price·qty            (debit cash for what they paid)
Seller cash:       +price·qty            (credit cash for what they received)
Buyer position:    +qty                  (gain position at this market/outcome)
Seller position:   −qty                  (lose / open short)
Buyer cash:        −fee_buyer            (taker/maker fee)
Seller cash:       −fee_seller
SYS:FEE:TRADING:   +(fee_buyer + fee_seller)
```

Each leg is a separate `LedgerService.transfer` with its own `op_id` so a partial settlement can be re-played safely.

### 3.4 Trading cancel (release reservation)

**Trigger:** customer POSTs `/cancel-order` OR matching engine emits cancel.

**op_id:** `cancel-{order_id}`.

```
<user_id> reservation released (no balance change; available rises)
```

Reservations live in `RiskEngine`, not the ledger. The ledger sees no movement on a cancel — the reservation simply stops counting against `available`.

### 3.5 Liquidation

**Trigger:** `RiskEngine` decides a position must be force-closed.

**op_ids:** `liq-{liquidation_id}-{leg}`.

```
Liquidatee position:        ±qty                 (close out)
Liquidatee cash:            −loss                (realize PnL)
SYS:LIQUIDATION:INSURANCE:  +clawback / −backstop
SYS:ADL:SOCIALIZED:         + remainder if insurance fund insufficient
```

Liquidation can put the liquidatee's cash account negative briefly during settlement; the next risk tick then either tops up from insurance or socializes via ADL.

### 3.6 Funding payment (perpetual)

**Trigger:** funding-tick scheduler.

**op_id:** `fund-{market}-{epoch}-{user}`.

```
<long-side users>:     ∓funding_per_qty·qty
<short-side users>:    ±funding_per_qty·qty
```

Funding is zero-sum across longs and shorts in the same market; no system account is touched.

### 3.7 Internal transfer (admin, requires maker-checker)

**Trigger:** admin POSTs `/admin/transfers` with `RequiresApproval` resolution.

**op_id:** `xfer-{approval_request_id}`.

```
<from>:   −amount
<to>:     +amount
```

Cannot proceed until a different admin commits the approval (`ApprovalRequestStore.find_committed_approval`). Audited in `data/admin/rbac_audit.jsonl`.

---

## 4. Reservations vs balance

The ledger tracks **balance**. The risk engine tracks **reservations** (pending order cash + margin). Customers see **available**:

```
available(U) = balance(U) − Σ active_reservations(U)
```

| Source | Affects balance? | Affects reservation? |
|---|---|---|
| `process_deposit` | yes (+) | no |
| Order placed | no | yes (+) |
| Order canceled | no | yes (−) |
| Order filled | yes (cash leg) | yes (− reservation, + actual debit) |
| Withdrawal submit | no | no — `/v2/wallet/withdraw` checks `available ≥ amount + fee` and proceeds; no separate reservation in v1 |
| Withdrawal settle | yes (−) | no |

**Known gap (P1):** withdrawals do NOT take a reservation in v1, so a customer can submit two withdrawals, both pass the balance check at submit time, and the second one's settlement will land in `SettlementStuck`. Mitigation: the velocity-tracker `try_record` is atomic and provides a per-day cap; full reservation-on-submit is a v1.1 follow-up.

---

## 5. The `LedgerDelta` record

Every state change is one `LedgerDelta` written to `data/ledger/deltas.jsonl`:

```rust
pub struct LedgerDelta {
    pub op_id: String,          // idempotency key
    pub schema_version: u32,
    pub at: DateTime<Utc>,
    pub from_account: String,
    pub to_account: String,
    pub amount: i64,            // smallest indivisible unit (subunit)
    pub note: Option<String>,
    pub principal_subject: Option<String>,
    pub trace_id: Option<String>,
}
```

Replay rule: `LedgerService::with_wal_store(...)` reads every delta in order; `seen_op_ids` is rebuilt; account balances are summed. Any duplicate `op_id` is silently skipped (INV-3).

### Why `i64` and not `i128`

- Ledger amounts are denominated in the smallest **ledger** unit. v1 uses 1 USDC subunit ≈ 1e-6 USDC, well within i64.
- Wallet amounts (wei, satoshi, lamport) use `i128` because chains can exceed i64.
- The customer-wallet handler enforces `amount + estimated_fee ≤ i64::MAX` via `max_amount_i128` (gate **M1**); per-chain divisors (gate **P0-FUND-3**) replace this with a precise per-chain ceiling.

---

## 6. Reconciliation formulas

These are the formulas the daily reconciliation job (RECONCILIATION_AND_RECOVERY_RUNBOOK.md §3) executes.

### 6.1 Global balance closure (INV-1)

```
Σ ledger_delta.amount  per direction  ==  0  for every recorded op_id
Σ all account balances                ==  0
```

Computed from `data/ledger/deltas.jsonl`. Any non-zero residual is a P0 alert.

### 6.2 Withdrawal ↔ ledger correspondence (INV-4)

```
∀ wd ∈ WithdrawalStore where wd.status == Settled:
   ∃! delta ∈ LedgerDeltas with op_id == "wd-settle-{wd.withdrawal_id}"
   AND delta.from_account == wd.user_id
   AND delta.amount == wd.amount   (after per-chain divisor)
```

Stores: `data/wallet/withdrawals.jsonl` join `data/ledger/deltas.jsonl`.

### 6.3 Hot wallet on-chain ↔ ledger (INV-5)

```
∀ chain C:
   onchain_balance(C, hot_address) == −ledger_balance("SYS:WALLET:HOT:" + C)
```

Until P0-FUND-2 ships per-chain accounts, the relaxed form is:

```
Σ_chain onchain_balance(chain, hot_address) ==  initial_seed
                                                + Σ deposits_credited
                                                − Σ withdrawals_settled
```

### 6.4 Trading fee bucket (sanity)

```
SYS:FEE:TRADING_balance == Σ over all fills (fee_buyer + fee_seller)
```

### 6.5 Outstanding reservations

```
∀ user U:
   reservation_total(U) == Σ open_orders.cash_reserved + Σ open_positions.margin_reserved
```

If `RiskEngine.recompute_from_orders()` produces a different number, projections are stale → restart projector.

---

## 7. Edge cases

| Case | Handling |
|---|---|
| **Duplicate `op_id`** | `LedgerService` returns "already applied"; caller treats as success. Settlement worker's idempotent branch detects this and flips status to `Settled` without re-debiting. |
| **Negative user balance** | Disallowed at the ledger level except via system-account overdraft. `transfer_cash` returns `InsufficientBalance`. The customer-wallet handler pre-checks (gate **P0-CORR-1**) so the failure surfaces at submit, not at settle. |
| **Negative system account** | Allowed for `SYS:ONCHAIN_VAULT:*` and (planned) `SYS:WALLET:HOT:*`. All other `SYS:*` accounts are non-negative; ledger rejects. |
| **Amount overflow** | At submit: `max_amount_i128` rejects with `AmountTooLarge`. At settle: per-chain divisor (planned) prevents wei overflow. |
| **Reorg-orphaned tx** | Withdrawal flips `Confirmed → Broadcast` (re-attempt confirm). If confirms never recover: operator manually flips `Broadcast → Rejected` and credits the user back via internal transfer. |
| **SettlementStuck** | On-chain happened, ledger debit failed. Operator runbook: top-up user cash, then `Stuck → Settled` via admin endpoint (planned). |

---

## 8. Quick-reference op_id catalogue

| Domain | Pattern | Where assigned |
|---|---|---|
| Deposit | `dep-{chain}-{tx_hash}` or caller | chain adapter / admin endpoint |
| Withdrawal settle | `wd-settle-{withdrawal_id}` | `SettlementWorker` |
| Trade fill cash | `fill-cash-{trade_id}-{side}` | `TradeSettler` |
| Trade fill position | `fill-pos-{trade_id}-{side}` | `TradeSettler` |
| Trade fee | `fill-fee-{trade_id}-{side}` | `TradeSettler` |
| Liquidation | `liq-{liquidation_id}-{leg}` | `LiquidationCircuitBreaker` |
| Funding | `fund-{market}-{epoch}-{user}` | funding scheduler |
| Internal transfer | `xfer-{approval_request_id}` | `/admin/transfers` handler |
| Position deposit | `pos-dep-{user}-{market}-{op_seed}` | admin endpoint |

If a new op_id pattern is added, add it here AND ensure the reconciliation job knows about it.

---

*Last updated 2026-05-04. Owners: ledger b.greifen, wallet b.greifen, risk b.greifen, finance UNASSIGNED.*
