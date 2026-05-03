# Wallet, Custody, and Withdrawal Risk — Design

> **Status:** v1 design only. No code. Companion to `docs/MONITOR_DESIGN.md`
> and `docs/BACKOFFICE_RBAC_DESIGN.md`.
>
> **Scope:** On-chain custody (warm / hot / cold), per-chain abstraction,
> address book, withdrawal lifecycle, sanctions / velocity screening,
> reconciliation, and the operator surface for finance_ops to drive it.
>
> **Out of scope:** Customer-facing onboarding / KYC, fiat banking
> integrations, market-making strategies for the OTC desk, cross-chain
> bridging.
>
> **Branch:** `p0-recovery-20260430`
> **Date:** 2026-05-04

## 1. Goals and threat model

### 1.1 Goals
- Hold customer assets in a tiered custody model (cold / warm / hot)
  with an **explicit, auditable invariant** linking on-chain balances
  to internal ledger liabilities at every moment.
- Process withdrawals with **policy-driven friction**: small + low-risk
  → automatic; large + high-risk → maker-checker; sanctions hits →
  block.
- Make **address book** the single source of truth for "where can a
  given customer withdraw to". Every withdrawal MUST resolve to a
  whitelisted address; ad-hoc destinations are rejected.
- Make every key-touching operation **observable**: signing requests,
  rotation events, hot-wallet sweeps all emit structured trace events
  on the same channels the Order Flow Monitor uses.
- Survive partial outages: a chain RPC going down stops *that chain's*
  withdrawals but does not interfere with order matching, ledger, or
  other chains.

### 1.2 Non-goals (v1)
- Custodian integration (Fireblocks / Coinbase Custody / etc.). v1
  uses self-custody with operator-managed keys.
- Hardware Security Module (HSM) integration. v1 uses an air-gapped
  signing host pattern; HSM is a v1.1 hardening track.
- Layer-2 / rollup-aware withdrawals. v1 supports Ethereum L1 + Bitcoin
  + Solana; L2 onboarding is sequential per-chain after v1 lands.
- Cross-chain swaps inside the wallet. Cross-chain is an OTC product
  surface, separate track.
- Multi-party computation (MPC) signing. v1 uses single-key per hot
  wallet; threshold signing for cold wallets only (multi-sig).

### 1.3 Threat model

| Adversary | What they have | What we defend |
|---|---|---|
| External attacker, hot-wallet key compromise | Full hot-wallet signing capability | Hot wallet caps cumulative outflow per window; daily / weekly velocity gates trigger automatic halts; warm/cold are physically and procedurally isolated. |
| External attacker, hot RPC poisoning | Ability to MITM RPC responses | All on-chain reads cross-checked against ≥2 independent RPC providers per chain; mismatched responses pause that chain and page on-call. |
| Compromised finance_ops operator | Stolen MFA-aware credential | Withdrawal approval is maker-checker (design RBAC §4); single operator cannot move funds. Address book add/remove is also maker-checker. |
| Insider with full operator + ledger access | Two-person collusion | All ledger movements are recorded in the WAL with op_id; nightly reconciliation against on-chain state will surface manual ledger edits. Cold-wallet signing requires a third operator outside the trading ops chain of command. |
| Customer-side address compromise | Stolen withdrawal destination | Address book whitelist with cool-down (24h) on new addresses; the cool-down is per-customer per-chain; suspicious destinations (sanctions list match) blocked at submit time. |
| Operator with hot-wallet RPC access | Read-only chain visibility | Reads are non-confidential; nothing sensitive in RPC. Risk is RPC abuse (rate-limit, DoS); per-operator throttle in the gateway. |

### 1.4 What is OUT of the threat model
- Quantum-capable adversary against ECDSA / EdDSA. Out of scope; chain
  ecosystem-level concern.
- Side-channel attacks on the air-gapped signing host. Out of scope;
  physical security relies on operations-track controls.
- Malicious chain-level reorgs longer than the documented confirmation
  threshold. v1 trusts the chain's stated finality; deeper finality
  monitoring is a v1.1 track.

## 2. Custody architecture

Three tiers per supported chain. Each tier is a distinct address (or
address set) with distinct signing authority and operational policy.

### 2.1 Cold wallet
- **Purpose:** Long-term reserve. The bulk of customer assets sit here.
- **Signing:** Multi-sig (3-of-5 v1) with keys held by operators in
  geographically separated locations. **Signing requires an air-gapped
  ceremony** with two security_admin operators present.
- **Withdrawal frequency:** Manual, infrequent (≤ once per day in
  normal operation). Cold → warm transfers are themselves maker-checker
  via finance_ops + security_admin.
- **Address visibility:** The cold wallet address(es) are public on-
  chain but NOT exposed in api responses. Only `risk_ops` and
  `security_admin` roles see them.
- **Storage:** Each key shard sealed in an HSM-style envelope at a
  designated location; recovery procedure documented in
  `docs/runbooks/cold-wallet-recovery.md` (future).

### 2.2 Warm wallet
- **Purpose:** Refill buffer for the hot wallet. Holds 1-7 days of
  expected withdrawal volume.
- **Signing:** 2-of-3 multi-sig with keys held by `finance_ops` +
  `security_admin` operators. Signing happens in an air-gapped session
  but does NOT require physical convergence — distributed ceremony
  over an out-of-band signing channel.
- **Withdrawal frequency:** Daily refill from cold (manual) and
  daily-or-as-needed refill TO hot (semi-automated under maker-checker).
- **Address visibility:** Warm address(es) visible to `finance_ops` and
  `risk_ops`; not in customer-facing api responses.

### 2.3 Hot wallet
- **Purpose:** Customer withdrawals. Holds the smallest practical
  balance (≤ 1 day of expected outflow, plus a refill buffer).
- **Signing:** Single-key (per chain), held by the hot-wallet daemon
  process. Key material lives only in the daemon's runtime memory and
  is never written to disk; the daemon is restarted (and re-loads
  from a sealed source) at every deployment.
- **Withdrawal frequency:** Continuous. Throughput cap per minute /
  hour / day; cumulative-outflow circuit breaker.
- **Address visibility:** Hot address(es) public — they are the deposit
  destination for customer top-ups too. (Note: this is the "single
  hot wallet" model; v1.1 may move to per-customer deposit addresses
  for better attribution but that is a separate track.)

### 2.4 Tier movement rules
- **Cold → warm**: maker-checker, security_admin + finance_ops, max
  per-day per-chain limit (configurable, default 5% of cold balance).
- **Warm → hot**: maker-checker, finance_ops × 2, refills triggered
  when hot balance falls below `min_hot_balance` (per-chain config).
- **Hot → cold/warm** (sweep up): finance_ops single-actor, capped at
  `hot_overflow_threshold` (e.g. anything above 2 days of expected
  outflow). Sweep targets warm by default; sweep to cold only via
  maker-checker.

## 3. Chain abstraction

### 3.1 v1 supported chains
- **Bitcoin (BTC mainnet)** — UTXO model, withdrawal = build PSBT,
  sign, broadcast. Confirmation threshold: 6 blocks for amounts <0.1
  BTC, 12 blocks for ≥0.1 BTC.
- **Ethereum (ETH + ERC-20 mainnet)** — account model, withdrawal =
  build EIP-1559 tx, sign, broadcast. Confirmation threshold: 25
  blocks for ETH, 50 blocks for ERC-20 tokens, 100 blocks for stables
  with elevated MEV exposure (USDC, USDT).
- **Solana (SOL + SPL mainnet)** — account model, withdrawal = build
  versioned tx with recent blockhash, sign, broadcast. Confirmation
  threshold: 32 confirmations (one epoch boundary).

### 3.2 Chain-adapter trait

A `ChainAdapter` trait abstracts per-chain operations. Implementations
live under `crates/wallet/src/chains/{btc,eth,sol}.rs`. The trait
exposes:

```rust
pub trait ChainAdapter: Send + Sync {
    type Address;
    type TxHash;
    type Tx;          // unsigned tx body
    type Signed;       // signed tx ready for broadcast

    // Read-side
    fn confirmations(&self, tx: &Self::TxHash) -> Result<u32>;
    fn balance(&self, addr: &Self::Address) -> Result<i128>;  // smallest unit
    fn fee_estimate(&self, urgency: FeeUrgency) -> Result<i128>;

    // Build-side
    fn build_withdrawal(
        &self,
        from: &Self::Address,
        to: &Self::Address,
        amount: i128,
        fee: i128,
    ) -> Result<Self::Tx>;

    // Sign-side (provided by the WalletKey trait, not the chain).
    // Broadcast-side
    fn broadcast(&self, signed: &Self::Signed) -> Result<Self::TxHash>;

    // Subscription-side (deposit detection)
    fn watch_deposits(&self, addr: &Self::Address) -> impl Stream<Item = DepositEvent>;
}
```

### 3.3 RPC redundancy
- Each chain adapter is constructed with **at least 2 independent
  upstream providers** (e.g. own node + Infura for ETH; own node +
  QuickNode for SOL; own node + Blockstream for BTC).
- Reads cross-check critical responses (balance, confirmations,
  recent block hash). Mismatches at the same tip pause that chain's
  withdrawal pipeline and page on-call.
- Writes (broadcast) go to all configured providers in parallel; first
  success wins, failures are logged.

## 4. Address book

The single source of truth for "this customer can withdraw to these
addresses". Stored in `crates/api/src/address_book.rs` (or a new
crate) with the same JSONL-WAL pattern as the monitor / RBAC stores.

### 4.1 Address record
```rust
pub struct WithdrawalAddress {
    pub schema_version: u32,
    pub address_id: String,
    pub user_id: String,
    pub chain: ChainId,
    pub address: String,          // chain-specific encoding
    pub label: String,            // user-supplied
    pub status: AddressStatus,    // PendingCooldown | Active | Suspended | Removed
    pub added_at: DateTime<Utc>,
    pub cooldown_until: DateTime<Utc>, // active = cooldown_until <= now
    pub last_used_at: Option<DateTime<Utc>>,
    pub sanctions_check: SanctionsCheckResult,
    pub added_by: String,         // user_id (for self-add) or operator_id (for compliance-add)
}
```

### 4.2 Address lifecycle
- `PendingCooldown` — added by the customer, in the 24h hold-off
  window. Cannot be a withdrawal destination yet.
- `Active` — past cool-down, available for withdrawals.
- `Suspended` — flagged by compliance / fraud detection; cannot be a
  destination but record retained for audit. Re-activation requires
  maker-checker (`compliance_ops` + `finance_ops`).
- `Removed` — hard-removed by the customer; cannot be re-added without
  a fresh cool-down.

### 4.3 Cool-down rules
- Default 24h for new customer-added addresses.
- Default 0h for compliance-added addresses (rare; e.g. court-ordered
  refund destinations).
- Per-customer override: high-volume institutional accounts can have
  a custom cool-down (down to 0h) under a `compliance_ops` +
  `finance_ops` maker-checker grant. Default is 24h.

### 4.4 Sanctions screening
- On address add: synchronous check against the sanctions provider
  (Chainalysis / TRM / Elliptic). If hit, address goes straight to
  `Suspended` and the customer is notified via the in-app channel +
  the compliance team is paged.
- On withdrawal submit: re-check (in case the destination was
  sanctioned after the cool-down passed). Hit blocks the withdrawal
  and surfaces the SAR (Suspicious Activity Report) workflow.
- Provider abstraction: `SanctionsProvider` trait so the implementation
  is swappable per region / regulatory-jurisdiction.

## 5. Withdrawal lifecycle

```
   submit ──> validated ──> queued ──> approved? ──> broadcast ──> confirmed ──> settled
                                            │ no                              │
                                            └──── manual review ──────────────┘
                                                  (finance_ops MC)
```

### 5.1 Stages

- `Submitted` — customer POSTed `/withdraw`. Address book lookup
  succeeds; basic validation (amount > 0, sufficient balance) passes.
- `Validated` — pre-flight checks pass: address active (not in cool-
  down, not Suspended), amount within per-customer per-day cap,
  sanctions re-check clears, hot-wallet has sufficient balance OR a
  refill is queued.
- `Queued` — the request lives in `WithdrawalQueue` waiting for the
  next sweep. Customer sees status `pending`.
- `AwaitingApproval` — the request hit a maker-checker threshold (see
  §5.3). Lives in the `ApprovalRequestStore` from RBAC §3 with
  `action = WithdrawalsApprove`.
- `Approved` — operator approved (or auto-approved under threshold);
  ready to sign.
- `Signing` — hot-wallet daemon picked it up; building tx, fetching
  fee estimate, signing.
- `Broadcast` — tx broadcast; we have a tx hash.
- `Confirmed` — tx has reached the chain's confirmation threshold.
- `Settled` — internal ledger debit committed; deposit destination
  holds the funds.
- `Rejected` — failed at any stage; reason recorded; ledger reservation
  released.

### 5.2 Internal ledger touch points
- On `Submitted`: reserve the amount + estimated fee in the customer's
  `pending_withdrawal` sub-account (separate from `available`).
  Reservation is a ledger op_id `wd-reserve-{withdrawal_id}`.
- On `Confirmed`: commit the debit (`wd-debit-{withdrawal_id}`),
  reverse the reservation. Net effect: customer's `available` is
  unchanged (the reservation already debited it); `pending_withdrawal`
  is zero; the hot-wallet asset account is debited.
- On `Rejected` / `Failed broadcast`: reverse the reservation. Customer
  is whole.
- All ledger ops are atomic via the existing `commit_delta_if_absent`
  path; idempotent on op_id.

### 5.3 Maker-checker thresholds (defaults; configurable per chain)
- Amount ≤ $10k USD-equivalent AND address `last_used_at` within 30
  days → **auto-approve** (still queued, but no operator click).
- Amount $10k-$100k OR new address (no `last_used_at`) → **single
  finance_ops operator review**.
- Amount > $100k OR sanctions adjacency score above threshold OR
  customer flagged → **maker-checker (finance_ops × 2)**.
- Cumulative withdrawal velocity (per customer per 24h) above $500k
  → **finance_ops × 2 + risk_ops sign-off**.

### 5.4 Failure modes and recovery
- **Broadcast failure**: retry with bumped fee up to 3 times across
  separate RPC providers; if all fail, transition to `Rejected` and
  release reservation.
- **Stuck in mempool > 30 min**: rebuild with bumped fee (replace-by-
  fee for BTC, higher gas tip for ETH). Re-broadcast.
- **Reorg drops a confirmed tx**: the confirmation poll will re-detect
  this; status flips back to `Broadcast` and waits for re-confirmation.
- **Hot wallet runs dry mid-batch**: pause the queue, page finance_ops
  for warm-to-hot refill (maker-checker). In-flight withdrawals stay
  in `Queued` until balance is restored.

## 6. Hot wallet daemon

A separate process per chain (`hot-wallet-eth`, `hot-wallet-btc`,
`hot-wallet-sol`). Communicates with the api over a private gRPC
endpoint (mTLS); never exposed externally.

### 6.1 Lifecycle
- Boots, loads its signing key from a sealed source (HSM-emulated v1:
  encrypted file at a path the api process cannot read; the daemon
  runs as a separate Linux user).
- Subscribes to the api's `WithdrawalQueue` over a server-streaming
  gRPC.
- For each `Approved` withdrawal:
  1. Verify the withdrawal record's signature (the api signs every
     queue entry with a static api-side key the daemon validates).
  2. Build the tx via the chain adapter.
  3. Sign with the loaded key.
  4. Broadcast.
  5. Stream the result back over a unary gRPC `ReportBroadcast`.
- On error or timeout: report failure; the api transitions the
  withdrawal to a recovery state and surfaces it to finance_ops.

### 6.2 Hot-wallet circuit breakers
- `outflow_per_minute_cap` — sliding-window cap on cumulative
  withdrawals signed.
- `outflow_per_hour_cap` — same, hourly.
- `outflow_per_day_cap` — same, daily.
- `single_tx_cap` — hard ceiling on any one withdrawal amount.
- Breach of any cap pauses signing and pages on-call. Reset requires
  `risk_ops` + `finance_ops` maker-checker.

### 6.3 Daemon ↔ api contract (gRPC)
```proto
service HotWalletDaemon {
    rpc Subscribe(SubscribeRequest) returns (stream WithdrawalAssignment);
    rpc ReportBroadcast(BroadcastReport) returns (Ack);
    rpc Heartbeat(Heartbeat) returns (Ack);
}
```
- `Heartbeat` fires every 5s; absence > 30s flips the chain into
  `daemon_unreachable` state, blocking new withdrawals.

## 7. Reconciliation

Nightly job (target: 03:00 UTC per chain) that compares:
- Sum of customer balances per chain in the internal ledger.
- Sum of on-chain balances across hot + warm + cold wallets for that
  chain.
- Outstanding `Queued` / `AwaitingApproval` reservations.

```
on_chain_total >= ledger_total + outstanding_reservations
```

Any discrepancy beyond a configurable epsilon (default 0.01% of total
or 1 unit, whichever is larger) creates a P0 incident, pauses
withdrawals on that chain, and pages security_admin + finance_ops.

Output is a `ReconciliationReport` written to `data/recon/{chain}/{date}.json`
and surfaced via `GET /admin/wallet/reconciliation`.

## 8. API contract (operator surface)

All endpoints under `/admin/wallet/*` go through the existing
`with_principal()` filter and additionally the new RBAC authz check
(per `BackofficeAction::Wallet*` actions added in Step 7's type model
expansion).

### 8.1 `GET /admin/wallet/balances`
- Per-chain warm + hot balances (cold not exposed by default).
- Permission: `BalancesRead`.
- Response:
  ```json
  {
    "chains": [
      {
        "chain": "eth",
        "hot": { "address": "0x...", "balance_wei": "1234..." },
        "warm": { "address": "0x...", "balance_wei": "5678..." },
        "outstanding_reservations_wei": "123..."
      }
    ]
  }
  ```

### 8.2 `GET /admin/wallet/queue`
- List Queued + AwaitingApproval withdrawals.
- Permission: `WithdrawalsReview`.

### 8.3 `POST /admin/wallet/refill`
- Request a warm → hot refill. Maker-checker via the existing
  approval flow with `action = WalletRefill`.
- Body: `{ chain, amount, reason }`.

### 8.4 `POST /admin/wallet/sweep`
- Request a hot → warm sweep. Single-actor for finance_ops; maker-
  checker for hot → cold.
- Body: `{ chain, target: "warm" | "cold", amount, reason }`.

### 8.5 `GET /admin/wallet/reconciliation`
- Latest reconciliation report per chain.
- Permission: `BalancesRead` for read; `BalancesAdjust` for any manual
  adjustment endpoint (separate, maker-checker).

## 9. Customer surface (re-usable)

The existing `/withdraw` endpoint stays, with these v1 changes:
- Body adds optional `address_id` (preferred). If `address` is supplied
  instead, the api performs an address book lookup; if the literal
  address is not found in the customer's whitelist, the request is
  rejected with `WITHDRAWAL_DESTINATION_NOT_WHITELISTED`.
- Response gains `withdrawal_id` for status polling.
- New `GET /withdrawals/{user_id}/{withdrawal_id}` for detailed status.

## 10. MVP scope

### 10.1 v1 chains
- Ethereum mainnet (ETH + USDC + USDT). Highest customer demand;
  most tooling.
- (Stretch) Bitcoin mainnet. Adds UTXO complexity; if Eth lands smooth
  in v1, BTC follows in v1.1.
- Solana deferred to v1.2 — high throughput, but the validator-error
  surface area is wider; want to absorb that after Eth + BTC stabilize.

### 10.2 v1 chain adapter MVP
- ETH only.
- Real RPC providers: own node + Infura.
- Sanctions provider: Chainalysis (real); abstract behind trait so
  Elliptic / TRM can drop in.
- Cold wallet: 3-of-5 multi-sig; manual ceremony; no protocol
  automation in v1.

### 10.3 v1 hot wallet daemon MVP
- Single Linux process, separate user from api.
- Encrypted-at-rest signing key, decrypted only in process memory at
  boot from a passphrase prompted to the operator (or read from a
  sealed envelope).
- gRPC over local socket (mTLS deferred to v1.1).
- Circuit breakers active: per-minute + per-hour + single-tx + daily.

### 10.4 v1 deferred
- HSM integration (true hardware key custody). Deferred to v1.1.
- MPC signing for the hot wallet (single-key v1).
- Cross-region cold wallet replication. v1 has all cold keys in one
  region.
- Per-customer deposit addresses (v1 uses single hot address for
  deposits, attribution by tx-memo or off-chain order matching).

### 10.5 v1 deliverables
- `docs/WALLET_DESIGN.md` (this file).
- `crates/wallet` (new) — `ChainAdapter` trait, ETH adapter, address
  book store, withdrawal state machine.
- `crates/api/src/admin_wallet_http.rs` (new) — operator endpoints.
- `crates/hot_wallet_daemon` (new binary) — gRPC server, signing.
- Address book added to `BackofficeAction` enum + matrix:
  `WalletAddressbookAdd`, `WalletAddressbookSuspend` (compliance_ops
  single-actor), `WalletAddressbookReactivate` (compliance_ops +
  finance_ops MC).
- Smoke test mirroring `rbac_smoke_test.ps1` + `monitor_smoke_test.ps1`:
  boot api with a stub chain adapter (in-memory, no real RPC), submit
  a withdrawal, observe the lifecycle through Approved →
  Broadcast → Confirmed.

## 11. Security rules

These rules are invariants enforced at the relevant layer.

1. **No withdrawal commits without an active address book entry.**
   The address book lookup is the FIRST validation step; ad-hoc
   destinations cannot reach the queue.
2. **Hot wallet caps are absolute.** Breach pauses the chain; manual
   reset by `risk_ops` + `finance_ops` maker-checker.
3. **Cold-wallet operations require physical convergence of two
   security_admin operators.** The signing host has no network
   interface; transactions cross the air-gap via QR code or USB
   read-only stick.
4. **Signing key material never lands on disk in plaintext.** The hot-
   wallet daemon either reads from an encrypted file with a passphrase
   prompted at boot, or accepts an unwrap from a sealed envelope.
5. **Every signing operation produces a trace event.** The hot wallet
   daemon publishes one event per signed tx onto the same observability
   channel the Order Flow Monitor uses (a new `wallet_signed` stage on
   the trace bus). Lost events are not catastrophic but the gap is
   visible in metrics.
6. **Reconciliation discrepancies block all withdrawals on the
   affected chain.** Resume requires `security_admin` sign-off
   referencing the resolution ticket.
7. **Address book changes are append-only + maker-checker for
   compliance actions.** Customer self-add is single-actor (with
   cool-down); compliance-add / suspend / reactivate is maker-checker.
8. **All on-chain reads cross-validate against ≥2 RPC providers.**
   Mismatches pause the chain; resolution is a runbook step, not an
   automatic recovery.
9. **Sanctions checks are blocking, not advisory.** A confirmed hit
   prevents the address from ever being whitelisted; an inflight
   withdrawal with a sanctions hit is rejected and the customer is
   notified.
10. **Hot-wallet daemon failure is fail-closed.** If the daemon
    heartbeat stops, the api refuses to enqueue new withdrawals on
    that chain. Existing queued withdrawals stay queued; nothing is
    auto-released.

## 12. Open questions for v1.1

- **Per-customer deposit addresses.** Current single-hot-address
  model means deposit attribution depends on memo/tag fields, which
  not every chain supports cleanly. Per-customer addresses are
  cleaner for accounting but multiply the address management surface.
  Survey customer ops in v1.1.
- **MPC vs. single-key for hot wallet.** MPC removes the single-key
  compromise scenario but adds operational complexity (two hosts must
  agree to sign every tx, latency of cross-host signing). v1.1 is the
  right time to decide based on actual production pain points.
- **Cold-wallet remote ceremony.** Current design requires physical
  convergence; some teams prefer remote ceremonies with sealed
  hardware. Trade-off is responsiveness vs. physical-only audit
  trail.
- **Confirmation thresholds for stable-coin withdrawals.** USDC's MEV
  exposure suggests higher confirmations than ETH; calibrating those
  thresholds against historical reorg data is a v1.1 task.
- **Sanctions provider redundancy.** v1 has one provider; failures
  block all new addresses across the platform. v1.1 should add a
  fallback provider with conservative-on-mismatch semantics.
