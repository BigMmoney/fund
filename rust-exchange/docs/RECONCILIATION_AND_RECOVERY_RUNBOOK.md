# Reconciliation & Recovery Runbook

> Daily reconciliation procedure, WAL recovery procedure, and the playbook for the most common operational incidents (`SettlementStuck`, ledger / on-chain divergence, partial WAL corruption).
> **This is a runbook, not a design doc.** Every section ends with a clear "if X then page Y" sentence.
>
> Branch `p0-recovery-20260430` · HEAD `c6b790f`

---

## 1. Roles

| Role | Pages? | Responsibility |
|---|---|---|
| **Primary on-call** | yes | First responder; runs §3 daily; handles §5 / §6 / §7 incidents |
| **Wallet engineer** | yes (escalation) | Authoritative on §6 SettlementStuck and §8 chain adapter |
| **Finance** | no | Signs off §3 reconciliation report; owns §4.5 backup verification |
| **Security** | yes (sev-1 only) | Investigates audit-log anomalies; key rotation |

If the primary on-call is uncertain about any step in this runbook, escalate before acting. Bad reconciliation moves are harder to undo than bad code deploys.

---

## 2. Inputs

| Source | Path | Format |
|---|---|---|
| Ledger deltas | `data/ledger/deltas.jsonl` | one `LedgerDelta` per line |
| Withdrawals | `data/wallet/withdrawals.jsonl` | one `WithdrawalRecord` per line; latest-wins per id |
| Addresses | `data/wallet/addresses.jsonl` | one `WithdrawalAddress` per line |
| Customer wallet audit | `data/wallet/customer_audit.jsonl` | one `CustomerWalletAuditRow` per line |
| RBAC audit | `data/admin/rbac_audit.jsonl` | one row per decision |
| Trade journal | `data/trade_journal.jsonl` | one `TradeJournalRecord` per fill |
| Trade settlement | `data/trade_settlement.jsonl` | one `TradeSettlementRecord` per settled fill |
| Sequencer | `data/sequencer/commands.jsonl` | one `SequencedCommandRecord` per command |
| Chain RPC | configured per chain | live `getBalance` / `getTransactionByHash` |

All JSONL files are append-only with latest-record-wins semantics on the primary key (`op_id`, `withdrawal_id`, etc.). Replay rebuilds in-memory state deterministically.

---

## 3. Daily reconciliation procedure

**Run cadence:** once per UTC day at 00:30. Owner: primary on-call. Expected duration: 15 minutes if green.

### 3.1 Snapshot inputs

```bash
# from the api host
ssh api-prod
sudo -u api bash -c '
  ts=$(date -u +%Y%m%d)
  mkdir -p /var/recon/$ts
  cp data/ledger/deltas.jsonl       /var/recon/$ts/
  cp data/wallet/withdrawals.jsonl  /var/recon/$ts/
  cp data/wallet/addresses.jsonl    /var/recon/$ts/
  cp data/admin/rbac_audit.jsonl    /var/recon/$ts/
'
```

Snapshot must precede every step that follows. If a step fails, re-run from the same snapshot to keep the analysis stable.

### 3.2 INV-1: global balance closure

**What:** sum of every ledger account must be zero.

```bash
jq -s '
  group_by(.from_account, .to_account)
  | reduce .[] as $g (
      {};
      .[$g[0].from_account] += -($g | map(.amount) | add)
      | .[$g[0].to_account] +=  ($g | map(.amount) | add)
    )
  | to_entries | map(.value) | add
' /var/recon/$ts/deltas.jsonl
```

| Result | Action |
|---|---|
| `0` | ✅ pass |
| Non-zero residual | 🚨 sev-1; do NOT mutate state; escalate to wallet engineer; investigate via §3.6 reverse lookup |

### 3.3 INV-4: withdrawal ↔ ledger correspondence

**What:** every `Settled` withdrawal has exactly one `wd-settle-{id}` ledger debit; amounts match.

```bash
# settled withdrawals
jq -r 'select(.status == "settled") | [.withdrawal_id, .user_id, .amount] | @tsv' \
  /var/recon/$ts/withdrawals.jsonl \
  | sort -u > /tmp/settled.tsv

# settle deltas
jq -r 'select(.op_id | startswith("wd-settle-")) | [(.op_id | sub("^wd-settle-"; "")), .from_account, .amount] | @tsv' \
  /var/recon/$ts/deltas.jsonl \
  | sort -u > /tmp/settle_deltas.tsv

# join + diff
join -t $'\t' /tmp/settled.tsv /tmp/settle_deltas.tsv \
  | awk -F'\t' '$2 != $4 || $3 != $5 { print }'
```

| Result | Action |
|---|---|
| Empty output | ✅ pass |
| Settled-without-delta rows | 🚨 sev-1; an on-chain payout has no ledger record; treat as a value loss; escalate |
| Delta-without-settled | 🚨 sev-2; possible duplicate settlement OR a `Stuck → Settled` recovery flip (cross-check §6 records); escalate to wallet engineer |
| Amount mismatch | 🚨 sev-1; per-chain divisor change OR a manual ledger edit; escalate |

### 3.4 INV-5: hot wallet on-chain ↔ ledger

**What:** the hot wallet's on-chain balance equals the negation of `SYS:WALLET:HOT:<chain>` ledger balance (or, in v1, the relaxed form against the seed + flow).

For each configured chain:

```bash
# v1 relaxed form (single SYS:ONCHAIN_VAULT:USDC account)
expected=$(jq -s '
  reduce .[] as $d (
    0;
    if   $d.from_account == "SYS:ONCHAIN_VAULT:USDC" then . + $d.amount
    elif $d.to_account   == "SYS:ONCHAIN_VAULT:USDC" then . - $d.amount
    else . end
  )
' /var/recon/$ts/deltas.jsonl)

actual=$(curl -s "$ETH_RPC" -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["'"$WALLET_ETH_HOT_ADDRESS"'","latest"],"id":1}' | jq -r '.result' | xargs printf '%d\n')

# both expressed in subunits / wei; tolerance = pending broadcasts not yet confirmed
echo "expected=$expected actual=$actual diff=$((actual - expected))"
```

| Diff | Action |
|---|---|
| 0 ± Σ in-flight Broadcast amounts | ✅ pass |
| Larger discrepancy | 🚨 sev-1; possible silent withdraw OR a deposit not credited; freeze customer withdrawals (§9 kill-switch); escalate |

### 3.5 Velocity tracker sanity

**What:** sum of last 24h non-Rejected withdrawals per user matches the in-process tracker.

```bash
curl -s -H "$AUTH" "http://api/admin/wallet/velocity" | jq .
# compare to:
jq -s --arg cutoff "$(date -u -d '24 hours ago' --iso-8601=seconds)" '
  map(select(.status != "rejected" and .submitted_at > $cutoff))
  | group_by(.user_id, .chain)
  | map({user: .[0].user_id, chain: .[0].chain, total: (map(.amount) | add)})
' /var/recon/$ts/withdrawals.jsonl
```

| Result | Action |
|---|---|
| Match | ✅ pass |
| Tracker low | 🚨 sev-2; restart api process to force `build_velocity_tracker` rebuild; verify on second pass |
| Tracker high | 🚨 sev-2; possible double-record bug; capture pcap; escalate |

### 3.6 Reverse op_id lookup (debug helper)

```bash
# given a suspect op_id, find every line that mentions it
grep "$OP_ID" /var/recon/$ts/*.jsonl
```

Use when §3.2-§3.4 produce a residual and you need to trace which transaction broke the invariant.

### 3.7 Sign-off

Primary on-call posts a line to `#exchange-recon` with: snapshot ts, INV-1 result, INV-4 result, INV-5 diff, anomalies. Finance acks within the day.

---

## 4. WAL recovery procedure

The api process recovers state by replaying WAL files at boot. This procedure assumes a planned restart OR a recovery from off-host backup.

### 4.1 Planned restart

```bash
# graceful: drains in-flight workers
sudo systemctl stop api
# verify no api process
pgrep -f api.exe || echo ok
sudo systemctl start api
# wait for /ready
until curl -sf http://localhost:3030/ready; do sleep 1; done
```

The boot sequence:
1. `JsonlFileWal::new(path)` reads every line; rebuilds in-memory `DashMap` index per store
2. `Sequencer` recovers `command_seq` from the highest persisted record
3. `LedgerService::with_wal_store` rebuilds `seen_op_ids` + per-account balance map
4. `WithdrawalStore` + `AddressBookStore` rebuild from JSONL
5. `build_velocity_tracker` replays non-Rejected withdrawals into a fresh `VelocityTracker`
6. `MonitorProjector` subscribes to eventbus BEFORE bootstrap so the `recovery_completed` event is observable
7. Health endpoint flips to `ok`

Acceptance: `/ready` returns 200 and `frontiers.consistent == true` within 60 seconds.

### 4.2 Recovery from off-host backup

If the local `data/` directory is unavailable (host loss, disk corruption):

```bash
# fetch latest backup
aws s3 sync s3://exchange-prod-backup/$(date -u +%Y%m%d) /var/restore/
# verify SHA256 manifest
sha256sum -c /var/restore/MANIFEST.sha256
# place at expected location
sudo systemctl stop api
sudo rm -rf /opt/api/data
sudo cp -r /var/restore /opt/api/data
sudo chown -R api:api /opt/api/data
sudo systemctl start api
```

Backups are taken every 5 minutes by an off-host cron (gate **P0-REC-2**); RPO = 5 minutes.

### 4.3 Partial corruption (last line truncated)

JSONL replay tolerates a truncated trailing line (the line is skipped and a warn-log is emitted). For mid-file corruption:

```bash
# detect: any line that fails to parse
jq -e . data/ledger/deltas.jsonl > /dev/null || echo "corruption"
# locate
awk 'NR{ if ($0 !~ /^\{.*\}$/) print NR": "$0 }' data/ledger/deltas.jsonl | head
```

If a non-trailing line is corrupt:

1. Stop api
2. Restore the affected file from backup (do NOT edit in place)
3. Replay any append-only differences from the audit log (manual cross-reference)
4. Restart api
5. Run §3 reconciliation immediately

If you cannot restore the line, escalate. Do not boot a node with a known-corrupt WAL.

### 4.4 `command_seq` divergence

If two api processes have divergent `command_seq` (split-brain after a network partition):

1. Designate the survivor by latest `last_appended_at` AND lowest sequence drift
2. Stop both
3. The survivor's WAL becomes canonical
4. Diff the loser's WAL against the survivor; manually arbitrate any commands present only on the loser
5. Restart the survivor

This is a sev-1 incident. Multi-node story (gate **P2-SCALE-1**) replaces this manual procedure with Raft.

### 4.5 Backup verification

Once a week (Sunday 03:00 UTC), the on-call:

```bash
# pull yesterday's backup
aws s3 sync s3://exchange-prod-backup/$(date -u -d 'yesterday' +%Y%m%d) /var/dr-test/
# boot a throwaway api against it
EXCHANGE_API_EXE=./target/debug/api.exe \
  WAL_DATA_DIR=/var/dr-test \
  BIND_PORT=3031 \
  ./target/debug/api.exe &
# wait for /ready
until curl -sf http://localhost:3031/ready; do sleep 1; done
# compare /admin/wallet/balances against production from the same snapshot
diff <(curl -s -H "$AUTH" http://localhost:3031/admin/wallet/balances) \
     <(cat /var/dr-test/balance-snapshot.json)
```

Acceptance: zero diff. Failure escalates to sev-2; rerun next day.

---

## 5. Incident: ledger ↔ on-chain divergence

**Signal:** §3.4 INV-5 diff exceeds Σ in-flight Broadcast amounts.

**Action:**

1. **Freeze:** trigger §9 customer-withdrawal kill switch immediately
2. **Snapshot:** copy `data/ledger/deltas.jsonl` and the chain's recent block range
3. **Bisect:** compare per-day INV-5 deltas to find the day the divergence began
4. **Categorize:**
   - **Silent withdraw** (chain decreased without ledger record) → sev-1, security incident
   - **Missed deposit** (chain increased without ledger record) → sev-2, finance corrective entry
   - **Reorg orphan** (was Confirmed; chain no longer has the tx) → sev-2, flip record `Confirmed → Broadcast` and let worker retry
5. **Document:** post-mortem within 48 hours

---

## 6. Incident: SettlementStuck

**Signal:** `wallet.settlement.stuck` warn-log fires; Prometheus counter `stuck_count` > 0; alert pages on-call.

**Confirm:**

```bash
curl -s -H "$AUTH" "http://api/admin/wallet/queue?status=settlement_stuck" | jq .
```

For each stuck record:

1. Read `WithdrawalRecord.notes` — contains the ledger error message
2. Look up the on-chain tx by `tx_hash`; confirm it is on chain at depth ≥ `confirmations_required`
3. Compute the actual customer impact: `actual_amount = wd.amount` (already on chain)
4. Top up customer cash:

```bash
# RequiresApproval — second admin must commit
curl -X POST -H "$AUTH" -d '{
  "from_account": "SYS:ONCHAIN_VAULT:USDC",
  "to_account": "<user_id>",
  "amount": <actual_amount_in_subunits>,
  "op_id": "stuck-recovery-<withdrawal_id>",
  "reason": "settlement-stuck recovery for withdrawal <withdrawal_id>"
}' http://api/admin/transfers
# second admin then approves
curl -X POST -H "$AUTH2" http://api/admin/approval-requests/<id>/approve -d '{"reason":"verified on-chain payout matches"}'
```

5. Flip `SettlementStuck → Settled` (today via direct WAL append from a recovery shell; planned admin endpoint):

```bash
# planned endpoint
curl -X POST -H "$AUTH" "http://api/admin/wallet/recover/<withdrawal_id>" -d '{"target":"settled","reason":"recovered after top-up"}'
```

6. Verify §3.3 INV-4 now passes for this `withdrawal_id`
7. File post-mortem: why the C2 balance pre-check at submit didn't catch it (the most common cause is concurrent submits eating each other's balance — until per-withdrawal cash reservation lands as a v1.1 follow-up)

**Do not** silently flip `SettlementStuck → Settled` without the top-up. The customer's cash account would stay short and the user-facing balance would lie.

---

## 7. Incident: WAL grew unexpectedly large

**Signal:** disk usage alert; `data/` > 90% capacity.

JSONL files are append-only and currently never rotate (gate **P2-SCALE-2**). Until rotation lands:

1. Identify the largest file: `du -sh data/* | sort -h | tail`
2. If it's `monitor/order_trace.jsonl` — safe to truncate older portion (Order Flow Monitor is best-effort, 90-day retention)
3. If it's a ledger / wallet / RBAC file — DO NOT TRUNCATE; expand the disk OR migrate to a larger volume
4. Open a ticket against P2-SCALE-2

---

## 8. Incident: node won't boot

**Signal:** systemd reports api in `failed` state; logs show panic.

| Panic message | Action |
|---|---|
| `failed to open ... store` | A WAL file is unreadable; restore from backup (§4.2) |
| `LedgerService::process_deposit ... duplicate op_id` | A startup deposit was attempted with a known op_id; safe to ignore (idempotent); root-cause why the boot path is replaying it |
| `address book latest record invalid transition` | Address record corruption; restore that file from backup |
| `panic at sequencer::next` | command_seq corruption; restore from backup AND escalate (rare) |

If no backup is available within RPO and the panic is non-recoverable, escalate to wallet engineer. Do NOT delete the WAL to "force a clean boot" — losing customer state is worse than downtime.

---

## 9. Kill switches

See WITHDRAWAL_RISK_AND_CUSTODY.md §6 for the full inventory. Most-used in incidents:

| Switch | Command |
|---|---|
| Pause customer withdrawals | `WALLET_CUSTOMER_PAUSED=1` env + restart (planned: live endpoint) |
| Halt market | `POST /admin/trading-ops/markets/{id}/halt` (single admin; break-glass) |
| Stop entire api | `sudo systemctl stop api` |

Document every kill in `#exchange-incidents` with timestamp, actor, reason, and ETA to unkill.

---

## 10. Acceptance: this runbook is alive

Quarterly drill (gate **P2-OPS-1**) executes one item from §4, §5, §6, §7 in staging end-to-end. Failure to execute = the runbook is dead. Update this doc after every drill.

---

*Last updated 2026-05-04. Owners: primary on-call rotation, escalation b.greifen.*
