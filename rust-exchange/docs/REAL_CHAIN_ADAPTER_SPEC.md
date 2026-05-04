# Real Chain Adapter Specification

> The ETH chain adapter that replaces `wallet::InMemoryChainAdapter` for production.
> Owns nonce, gas pricing, broadcast, confirmation tracking, RPC failover, and reorg handling.
> Gate **P0-FUND-1** in PRODUCTION_LAUNCH_CHECKLIST.md.
>
> Branch `p0-recovery-20260430` · HEAD `c6b790f`

---

## 1. Scope

This spec covers the ETH adapter only. BTC and SOL follow the same shape (gates P2-FUND-1, P2-FUND-2) but with chain-specific differences (UTXO vs account model; recent-blockhash vs nonce). The contract — `wallet::ChainAdapter` trait — is identical across chains.

In scope:
- Signing (private key in process memory)
- Nonce management
- Gas pricing (EIP-1559)
- Broadcast with retry
- Confirmation polling
- Reorg detection and rollback
- Multi-RPC failover
- Per-chain ledger-unit divisor

Out of scope:
- Cold-storage signing (separate HSM workflow)
- MEV protection (use a private-mempool RPC like Flashbots Protect; configurable as `WALLET_ETH_RPC_PRIVATE`)
- Chain-level attacks (51%, deeper-than-threshold reorgs)

---

## 2. Trait surface (already in `wallet::chain`)

The new adapter must implement the existing trait without changing it:

```rust
pub trait ChainAdapter: Send + Sync {
    fn chain_id(&self) -> ChainId;

    /// Build + sign + broadcast. Returns the tx hash. Idempotent on
    /// (from_address, nonce): repeated calls with the same nonce
    /// MUST return the same tx hash.
    fn build_sign_and_broadcast(
        &self,
        from_address: &str,
        to_address: &str,
        amount: i128,
        urgency: FeeUrgency,
        idempotency_seed: &str,
    ) -> Result<SignedTx, ChainError>;

    /// Current confirmation depth for `tx_hash`. Returns 0 if not in
    /// any block yet, or `None` if the adapter doesn't know the tx
    /// (e.g. dropped from mempool).
    fn confirmations_for(&self, tx_hash: &str) -> Option<u64>;

    /// Current available balance at `address` in the smallest chain
    /// unit (wei for ETH).
    fn balance_of(&self, address: &str) -> Result<i128, ChainError>;
}
```

The hot-wallet worker calls `build_sign_and_broadcast` to advance `Approved → Broadcast`, then `confirmations_for` to advance `Broadcast → Confirmed`. The trait is sync because the in-memory adapter is sync; the real adapter wraps an async RPC client behind `tokio::runtime::Handle::block_on` — the worker runs on its own task and the call duration is bounded.

---

## 3. Configuration

| Env var | Default | Purpose |
|---|---|---|
| `WALLET_ETH_RPC_PRIMARY` | (required) | Primary HTTPS RPC (e.g. infura) |
| `WALLET_ETH_RPC_SECONDARY` | (optional) | First failover (e.g. alchemy) |
| `WALLET_ETH_RPC_TERTIARY` | (optional) | Second failover (your own node) |
| `WALLET_ETH_RPC_PRIVATE` | (optional) | Private-mempool RPC for broadcast (Flashbots Protect) |
| `WALLET_ETH_HOT_ADDRESS` | (required) | Hot wallet address (0x…) |
| `WALLET_ETH_HOT_PRIVATE_KEY` | (required) | KMS-sealed private key; loaded once, never logged |
| `WALLET_ETH_CONFIRMATIONS_REQUIRED` | `25` | Block depth for `Confirmed` |
| `WALLET_ETH_GAS_LIMIT` | `21000` | Standard ETH transfer |
| `WALLET_ETH_MAX_FEE_PER_GAS_GWEI` | `300` | Hard ceiling on max_fee_per_gas |
| `WALLET_ETH_PRIORITY_FEE_GWEI_NORMAL` | `1.5` | Tip for `FeeUrgency::Normal` |
| `WALLET_ETH_PRIORITY_FEE_GWEI_FAST` | `3.0` | Tip for `FeeUrgency::Fast` |
| `WALLET_ETH_LEDGER_DIVISOR` | `1_000_000_000_000` | wei → micro-eth (1e12); makes amount fit i64 |
| `WALLET_ETH_RPC_TIMEOUT_MS` | `5000` | Per-call timeout |
| `WALLET_ETH_RPC_MAX_RETRIES` | `3` | Per-RPC retries before failover |
| `WALLET_ETH_REORG_TOLERANCE` | `5` | Re-check depth on every poll |

All of these must be loaded once at startup; the adapter must NOT re-read env on every tx.

---

## 4. RPC failover

### 4.1 Topology

```
                  ┌──────────────────┐
broadcast call → │  primary  RPC   │ ──► success → return
                  └─────┬────────────┘
                        │ timeout / 5xx / ratelimit
                        ▼
                  ┌──────────────────┐
                  │ secondary  RPC  │ ──► success → return + flag primary as degraded
                  └─────┬────────────┘
                        │ timeout / 5xx / ratelimit
                        ▼
                  ┌──────────────────┐
                  │  tertiary  RPC  │ ──► success → return + sev-2 alert
                  └─────┬────────────┘
                        │ all failed
                        ▼
                       ChainError::Rpc — record stays at Approved; next worker tick retries
```

### 4.2 Per-RPC retry budget

Each RPC gets `WALLET_ETH_RPC_MAX_RETRIES` (default 3) attempts with exponential backoff (200ms, 600ms, 1.8s) before failing over. Total budget per call: ≈8 seconds across all three RPCs. The hot-wallet worker tick interval (`WALLET_WORKER_TICK_MS`, default 5000ms) is intentionally larger than the per-RPC retry but smaller than the total failover budget so a single tick is bounded.

### 4.3 Rotation

A degraded RPC (3 consecutive failovers in a 5-minute window) is removed from the rotation for 60 seconds and re-evaluated. This is process-local; no coordination across api instances (gate **P2-SCALE-3** introduces shared state).

### 4.4 Read vs broadcast endpoints

`broadcast` should prefer `WALLET_ETH_RPC_PRIVATE` (Flashbots Protect or equivalent) to avoid mempool-MEV. Reads (`balance_of`, `confirmations_for`, `getTransactionByHash`) use the primary/secondary/tertiary chain.

---

## 5. Nonce management

ETH requires monotonic per-address nonce. Two failure modes the adapter MUST prevent:

| Bug | Consequence |
|---|---|
| Duplicate nonce across two txs | one tx replaces the other; one withdrawal silently doesn't go on chain |
| Gap in nonce sequence | later txs stuck in mempool until the gap is filled |

### 5.1 Source of truth

The adapter keeps an in-process `next_nonce: AtomicU64` initialized at boot from:

```
max(
  rpc.eth_getTransactionCount(hot_address, "pending"),
  highest_nonce_in_local_broadcast_log + 1
)
```

The local broadcast log lives at `data/wallet/eth_nonce.jsonl` — one line per `(nonce, tx_hash, withdrawal_id, broadcast_at)`. This file is append-only and is the local authority for "did we already send a tx with this nonce."

### 5.2 Allocation

`build_sign_and_broadcast`:

1. `nonce = self.next_nonce.fetch_add(1, SeqCst)` — atomic claim
2. Sign tx with this nonce
3. Append `(nonce, withdrawal_id, "pending", planned_tx_hash)` to `eth_nonce.jsonl` BEFORE broadcasting
4. Broadcast (with retry/failover §4)
5. On success: append `(nonce, withdrawal_id, "broadcast", actual_tx_hash)` to the same log
6. On failure: append `(nonce, withdrawal_id, "failed", reason)`. The nonce is now BURNED — see §5.4

### 5.3 Idempotency

`idempotency_seed` (passed by the worker as `withdrawal_id`) is used to detect replay:

- Adapter checks `eth_nonce.jsonl` for an existing row with `withdrawal_id == idempotency_seed`
- If present and status `broadcast`: return the cached `(tx_hash, nonce)` — do NOT re-broadcast
- If present and status `pending`: re-broadcast the same signed tx (same nonce) — chain dedups by `(from, nonce, sig)`
- If present and status `failed`: this is a recovery path; allocate a fresh nonce

### 5.4 Burned nonce recovery

A nonce is burned if we allocated it, signed something, but never got a tx hash on chain (RPC timeout AND no observable broadcast). Recovery:

1. Send a self-transfer of 0 wei from `hot_address` to itself with the burned nonce, gas-bumped to clear the queue
2. Verify the self-transfer confirms
3. Mark the burned row as `recovered` in the local log
4. Subsequent withdrawals continue with the next nonce

Self-transfer recovery is idempotent (replaying it succeeds) and observable (one extra tx per burn, dust-cost). Operators get a sev-3 alert on each burn so collusion of burns can be investigated.

---

## 6. Gas pricing (EIP-1559)

For each broadcast:

```
base_fee = (block.base_fee_per_gas of latest block)
priority_fee = WALLET_ETH_PRIORITY_FEE_GWEI_<urgency>
max_fee_per_gas = min(2 * base_fee + priority_fee, WALLET_ETH_MAX_FEE_PER_GAS_GWEI)
max_priority_fee_per_gas = priority_fee
```

| `FeeUrgency` | priority_fee gwei | typical inclusion |
|---|---|---|
| `Normal` | 1.5 | ≤ 2 blocks |
| `Fast` | 3.0 | ≤ 1 block |

If `max_fee_per_gas` would exceed `WALLET_ETH_MAX_FEE_PER_GAS_GWEI`, the adapter returns `ChainError::FeeCeiling` and the worker leaves the record at `Approved`. This is a soft block; on-call gets a sev-2 alert if it persists more than 10 minutes (the chain is probably under stress; check separately).

### 6.1 Fee-bump on stuck broadcast

If a tx stays unmined for `2 * fee_check_interval_blocks` (default 6 blocks ≈ 72s), the adapter:

1. Re-builds the tx with the SAME nonce and bumped gas (`max_fee_per_gas *= 1.25`)
2. Re-broadcasts
3. Records the bump in `eth_nonce.jsonl` so reconciliation can attribute the cost

Bump is bounded by `WALLET_ETH_MAX_FEE_PER_GAS_GWEI`. If the bump would exceed the ceiling, leave the original tx and log `gas_bump_ceiling_reached`.

---

## 7. Confirmation tracking

`confirmations_for(tx_hash)`:

1. `eth_getTransactionByHash(tx_hash)` — if `None`, the tx is not in any block AND not in our seen-mempool list → return `None`
2. If present, `block_number` is set
3. `latest_block = eth_blockNumber()`
4. Return `latest_block - block_number + 1`

Returns `None` for an unknown tx. Returns `0` for a known-but-not-yet-mined tx (not currently distinguishable on this trait — covered by polling `eth_getTransactionByHash` first).

The hot-wallet worker:

```
let depth = adapter.confirmations_for(&record.tx_hash);
if depth.is_none() && now - record.broadcast_at > tx_dropped_grace { /* re-broadcast */ }
if depth >= record.confirmations_required { /* flip to Confirmed */ }
```

Default `confirmations_required = 25` is conservative for ETH (≈5 minutes); reorgs deeper than this are extraordinary events handled in §8.

---

## 8. Reorg handling

### 8.1 Detection

On every poll:

1. Fetch `tx.block_hash` via `eth_getTransactionByHash`
2. Fetch the canonical block at that block_number via `eth_getBlockByNumber`
3. If `tx.block_hash != canonical.hash` → REORG DETECTED for this tx

### 8.2 Action

| State at detection | Action |
|---|---|
| `Confirmed` (depth ≥ required) | Flip `Confirmed → Broadcast`; sev-2 alert; worker will re-poll until either re-Confirmed at the new block or dropped |
| `Broadcast` (depth < required) | No state change; worker continues polling; the tx is either in another block now OR back in mempool |

The transition `Confirmed → Broadcast` is allowed by `wallet::is_valid_transition` (gate **P0-CORR-4** added the Stuck variants but the reorg path was already there).

### 8.3 Tolerance window

The adapter checks the **last `WALLET_ETH_REORG_TOLERANCE` blocks** (default 5) on every confirmation poll. Reorgs deeper than 5 blocks are exceptional; if observed, the adapter raises `ChainError::DeepReorg` — the worker leaves the record at its current state and on-call is paged sev-1 (likely chain-level event; manual review).

### 8.4 Settled records and reorg

Once a record is `Settled`, the ledger debit is committed. A reorg that orphans a settled tx requires manual reconciliation (RECONCILIATION_AND_RECOVERY_RUNBOOK.md §5):

1. Detect via INV-5 mismatch
2. Top up customer cash via internal transfer (RequiresApproval)
3. Flip `Settled → Rejected` with `rejection_reason = ReorgOrphaned` (transition allowed)

This is by design: irreversible commits stay irreversible at the ledger; corrective entries are auditable.

---

## 9. Failure mode catalogue

Every error returns `ChainError`. The worker's reaction is documented:

| `ChainError` | Cause | Worker action |
|---|---|---|
| `Rpc(msg)` | All RPCs failed | record stays at current state; next tick retries |
| `Timeout` | Per-call deadline hit | RPC failover §4 |
| `InsufficientHotBalance` | Hot wallet balance < amount + gas | record stays at `Approved`; sev-2 alert; refill workflow |
| `FeeCeiling` | Required gas exceeds ceiling | record stays at current state; sev-2 alert if > 10 min |
| `NonceTooLow` | Chain says our nonce was already used (we got out of sync) | rebuild `next_nonce` from chain; retry |
| `NonceTooHigh` | We tried to use a future nonce (probably restart issue) | rebuild `next_nonce`; sev-3 alert |
| `DeepReorg` | Reorg deeper than tolerance | sev-1 alert; manual review |
| `Signing(msg)` | Local signing failure | sev-1 alert (likely key issue); pause hot wallet worker |
| `Other(msg)` | Catch-all | sev-2 alert; record stays |

A `ChainError` is NEVER an error the customer sees; the customer's withdrawal stays at the current internal status and the next tick advances it (or doesn't). The customer-facing surface remains `WalletError::Internal` only on local bugs, not on chain-side transient issues.

---

## 10. Per-chain ledger-unit divisor

ETH amounts in wei exceed `i64::MAX` for any sum > ~9.2 ETH. The settlement ledger uses `i64`. The adapter therefore exposes a divisor:

| Chain | Ledger unit | Divisor (chain unit / ledger unit) |
|---|---|---|
| ETH | µETH (1e-6 ETH) | `1_000_000_000_000` (1e12 wei → 1 µETH) |
| BTC | sat / 100 (≈1.6e-9 BTC) | `100` (1 sat → 0.01 ledger unit; or change to direct sat) |
| SOL | µSOL (1e-6 SOL) | `1_000` (1e3 lamport → 1 µSOL) |

Settlement worker:

```
ledger_amount_i64 = (record.amount_wei / divisor) as i64
remainder_wei      = record.amount_wei % divisor   // tracked for fee accounting
```

The divisor is **fixed per chain** — changing it later breaks INV-4. v1's single `SYS:ONCHAIN_VAULT:USDC` account uses a 1:1 divisor (USDC has 6 decimals; ledger unit is 1e-6 USDC). The per-chain accounts (gate **P0-FUND-2**) introduce per-chain divisors atomically with the account split.

---

## 11. Acceptance tests

Before flipping the production env to the real adapter:

| Test | Acceptance |
|---|---|
| Sign + broadcast a 0.001 ETH testnet tx end-to-end | Broadcast succeeds; confirmations advance; `Settled` reached |
| Kill primary RPC mid-broadcast | Failover to secondary; same tx hash returned; only one on-chain tx |
| Kill primary AND secondary | Tertiary used; sev-2 alert fired |
| Replay same `idempotency_seed` | Returns cached tx hash; no second broadcast |
| Bump gas on stuck tx | Same nonce, higher fee; original replaced; one on-chain tx confirms |
| Re-org test: re-org tx deeper than 1 block but shallower than tolerance | `Confirmed → Broadcast` then `Broadcast → Confirmed` again; no double-settle |
| Re-org test: deeper than tolerance | `DeepReorg` returned; sev-1 alert |
| Burned-nonce recovery | Self-transfer of 0 wei completes; subsequent withdrawals proceed |
| Boot adapter against existing `eth_nonce.jsonl` | `next_nonce` correctly resumes from last broadcast |
| INV-5 reconciliation passes after every test | always |

---

## 12. Implementation hint

Likely shape for the new module under `crates/wallet/src/eth_rpc.rs`:

```rust
pub struct EthRpcAdapter {
    rpc_pool: Arc<RpcPool>,                      // §4
    private_rpc: Option<Arc<RpcClient>>,         // §4.4
    signer: Arc<EthSigner>,                      // §3 sealed key
    next_nonce: AtomicU64,                       // §5
    nonce_log: Arc<JsonlFileWal<NonceRow>>,      // §5
    chain_id: u64,
    hot_address: String,
    config: EthAdapterConfig,                    // §3
    runtime: tokio::runtime::Handle,             // for sync trait
}

impl ChainAdapter for EthRpcAdapter {
    fn chain_id(&self) -> ChainId { ChainId::Eth }

    fn build_sign_and_broadcast(...) -> Result<SignedTx, ChainError> {
        self.runtime.block_on(self.do_broadcast_async(...))
    }

    fn confirmations_for(&self, tx_hash: &str) -> Option<u64> {
        self.runtime.block_on(self.do_confirmations_async(tx_hash)).ok().flatten()
    }

    fn balance_of(&self, address: &str) -> Result<i128, ChainError> {
        self.runtime.block_on(self.do_balance_async(address))
    }
}
```

`ethers-rs` 2.x or `alloy` are the obvious crate choices; `alloy` is the modern path. Either way, gate behind `--features eth-rpc` so the test build keeps using `InMemoryChainAdapter`.

---

## 13. Out of scope (explicitly)

- ERC-20 transfers (only native ETH in v1; ERC-20 is a v1.2 feature)
- L2 chains (arb / opt / base) — separate adapters reusing this spec
- Account abstraction (4337) — not in v1
- Smart-contract wallets — out of scope; hot wallet is an EOA
- Gas-station / paymaster — not in v1

---

*Last updated 2026-05-04. Owner UNASSIGNED — gate P0-FUND-1 in PRODUCTION_LAUNCH_CHECKLIST.md.*
