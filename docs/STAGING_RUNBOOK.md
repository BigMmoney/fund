# Production / Staging Runbook

> Operational guide for deploying, smoke-testing, monitoring, rolling back, and responding to incidents on the rust-exchange backend in a staging or production environment.
>
> **Branch this runbook tracks:** `p0-recovery-20260430` (and successors).
> **Last updated:** 2026-05-02.
> **Related docs:** `docs/P0_DEPLOYMENT_READINESS.md`, `docs/benchmarks/2026-05-01/BENCHMARK_REPORT.md`, `docs/SECURITY_REVIEW_2026-04-07_SUMMARY.md`.

## 1. Preconditions

A staging deploy must NOT proceed unless all of the following are satisfied:

- [ ] Branch CI is green (or has only documented soft failures recorded in the PR).
- [ ] `cargo build --release --bin api` succeeds locally on a representative builder.
- [ ] `cargo test --workspace` PASS — at minimum 540 tests, 0 failed.
- [ ] `scripts/run_p0_full.ps1` (or its CI-runner equivalent) is exit 0 — covers cargo build/test, E2E, WAL replay, restart-after-errors, WAL backup/restore drill.
- [ ] `scripts/demo_trade_journey.ps1` exit 0 against the candidate binary — proves end-to-end match + settle + invariants.
- [ ] Security review summary (`docs/SECURITY_REVIEW_2026-04-07_SUMMARY.md`) shows all P0 + P1 findings closed (Fixed or Accepted-with-mitigation).
- [ ] Deployment Secret values are real — placeholders (`CHANGE_ME`, `replace-me`, `dev-secret-…`) are NOT acceptable in production.
- [ ] WAL backup of the previous build's `data/` PVC has been taken via `scripts/wal_backup.ps1` and verified via `run_wal_restore_drill.ps1`. Skip only on truly first-time deploys.

If any item is unchecked, halt and resolve before continuing.

## 2. Build the release artifact

From `rust-exchange/`:

```bash
cargo build --release --bin api
docker build -t <registry>/rust-exchange:<tag> .
docker push <registry>/rust-exchange:<tag>
```

Tag convention: `rc-X.Y[-N]` for release candidates, `vX.Y.Z` for production.
Image must NOT use `:latest` in production overlays.

## 3. Deploy paths

### 3.1 Kubernetes (preferred)

The Kustomize layout under `rust-exchange/deploy/k8s/` supports three overlays:

| Overlay | Purpose |
|---|---|
| `base/` | Namespace + ServiceAccount + PVC + Deployment + Service. Default values are placeholders. |
| `overlays/docker-desktop/` | Local Docker Desktop k8s. Ships with a dev secret. **Never use this overlay against staging or prod.** |
| `exchange.yaml` (single-file) | Reference manifest with all credential fields as `CHANGE_ME` placeholders. |

Production deploy:

1. Author a private overlay or kustomization that supplies real Secret values via file references (NOT env vars):
   ```yaml
   # overlays/prod/secret.yaml — kept in private storage, NEVER committed here
   # mounts internal_auth.secret + role_mapping.json under /var/run/secrets/exchange/
   ```
2. Apply:
   ```bash
   kubectl apply -k <private-overlay-path>
   ```
3. Watch rollout:
   ```bash
   kubectl rollout status deployment/exchange -n exchange --timeout=300s
   ```
4. Verify pod is `Ready 1/1`:
   ```bash
   kubectl get pods -n exchange -o wide
   ```

### 3.2 Docker Compose (fallback / single-node)

`rust-exchange/docker-compose.yml` is suitable for single-node staging. It mounts `./secrets/internal_auth.secret` from the host. Operator must provide a real secret file before `docker compose up`. Container hardening: `init: true`, `read_only: true`, `tmpfs: /tmp`, `cap_drop: ALL`, `security_opt: no-new-privileges`. Healthcheck pings `/ready` every 10 s.

## 4. Smoke validation

Mandatory after every deploy.

### 4.1 Liveness + readiness

```bash
curl -fsS http://<host>:<port>/health | jq .   # status==ok
curl -fsS http://<host>:<port>/ready  | jq .   # balance_invariant==true && frontier_consistency==true
```

If either probe returns non-200 or shows `balance_invariant: false` / `frontier_consistency: false`, halt and roll back.

### 4.2 Trade journey demo

```bash
powershell -ExecutionPolicy Bypass -File rust-exchange/scripts/demo_trade_journey.ps1 \
    -BaseUri http://<host>:<port> -Json
```

Required outcome: `passed: true`, `frontiers_consistent: true`, `balance_invariant: true`, `match_e2e_us` < 5000.

### 4.3 RTO/RPO drill (optional but recommended)

```bash
powershell -ExecutionPolicy Bypass -File rust-exchange/scripts/measure_rto_rpo.ps1 \
    -Iterations 3 -CommandCount 200
```

Required outcome: `rpo_worst_loss_count == 0`, `rto_seconds_p99 < 1.5`.

If RTO breaches budget, do NOT block the deploy — file a P2 follow-up. RTO above the §11 baseline is observational, not a release-blocking failure on first-time hardware.

## 5. Monitoring — what to watch

### 5.1 Health and durability

| Endpoint | Field | Healthy | Page on |
|---|---|---|---|
| `/health` | `status` | `ok` | not `ok` |
| `/health` | `frontiers.consistent` | `true` | `false` for >30 s |
| `/health` | `kill_switch` | `false` | `true` (operator decision needed) |
| `/health` | `bridge_alive` | `true` | `false` (event bus stalled) |
| `/ready` | `balance_invariant` | `true` | `false` immediately |
| `/ready` | `frontier_consistency` | `true` | `false` for >30 s |

### 5.2 Sequencer + ledger frontiers

Healthy frontiers move forward together. If `sequencer_command_seq` advances but `ledger_command_seq` stalls for >5 s under live traffic, the ledger commit path is stuck.

```bash
watch -n 5 'curl -s http://<host>:<port>/health | jq .frontiers'
```

### 5.3 Prometheus / metrics

`/metrics/prometheus` exports counters and per-stage histograms. Key dashboards:

- `http_requests_total`, `http_errors_total`, `http_request_latency` (per-path)
- `partition_orders`, `partition_fills`, queue depth per matching partition
- WAL append latency p99 (target: < 100 µs single-append at group_commit≥64)
- API submit-order match_e2e_us p99 (target: < 5 ms at concurrency 8)

### 5.4 Pod and container

```bash
kubectl top pod -n exchange
kubectl logs -n exchange -l app=exchange --tail=200
```

RSS growth >10% in 30 minutes of sustained traffic indicates a leak — file an incident.

## 6. Rollback procedure

A rollback must be safe, fast, and durable.

### 6.1 Same-version rollback (k8s)

```bash
kubectl rollout undo deployment/exchange -n exchange
kubectl rollout status deployment/exchange -n exchange --timeout=180s
```

### 6.2 Specific-version rollback

```bash
kubectl set image deployment/exchange -n exchange exchange=<registry>/rust-exchange:<old-tag>
```

### 6.3 If rollback panics on existing WAL state

If the older binary lacks the WAL-replay determinism fix landed in `1d6ac04` (commit `fix(rust-exchange/api): skip terminal commands during WAL replay recovery`), it may panic at `bootstrap.rs:166` when replaying a WAL containing `Settled` lifecycle commands.

Recovery steps:

1. Scale deployment to 0:
   ```bash
   kubectl scale deployment/exchange -n exchange --replicas=0
   ```
2. Restore the most-recent verified WAL backup over the PVC:
   ```bash
   powershell -File rust-exchange/scripts/run_wal_restore_drill.ps1 \
       -BackupArchive <path/to/wal-YYYYMMDD-HHMMSS.tar.gz>
   ```
3. Push the restored WAL to the PVC (specifics depend on storage class — typically a `kubectl cp` or a sidecar pod).
4. Choose: (a) re-deploy the current (post-fix) version and continue; or (b) accept the WAL state and let the older version replay. The fix is small and additive — prefer (a) unless rollback is blocked.

### 6.4 Roll-forward instead of rollback

If the rollback target is older than `1d6ac04`, prefer **roll-forward** — fix the regression on the current branch and redeploy. Rolling back across the WAL determinism fix is risky because the older binary may not boot on the newer WAL.

## 7. Incident response

### 7.1 Pod CrashLoopBackOff at startup

Most common cause: WAL replay failure.

```bash
kubectl logs -n exchange <pod> --previous --tail=100
```

Look for these signatures:

| Log signature | Meaning | Action |
|---|---|---|
| `FATAL: command replay after snapshot failed — cannot guarantee matching engine consistency: ... lifecycle=Settled` | Pre-2e (1d6ac04) bug. Sequencer WAL replay re-applies a Settled command. | Deploy current branch (≥`1d6ac04`). |
| `FATAL: ledger WAL recovery failed — refusing to start with empty state ... insufficient balance` | Ledger WAL has invariant-violating debit. WAL is corrupt OR a prior write was interrupted mid-flush. | Inspect last WAL backup; restore. If first-time deploy, wipe `data/` PVC and re-seed. |
| `failed to initialize ledger WAL at ...` | Disk full, permissions, or PVC unmounted. | Inspect node disk usage and PVC mount. |
| `FATAL: sequencer WAL recovery failed` | Sequencer WAL has gaps or corrupt frames. | Restore from backup. |

### 7.2 `/ready` reports `balance_invariant: false`

The ledger has internally inconsistent state (sum of accounts ≠ 0). This is a P0.

1. **Stop accepting new orders immediately:**
   ```bash
   curl -X POST http://<host>:<port>/admin/kill-switch -d '{"enabled":true}' \
        -H "Content-Type: application/json" \
        -H "<HMAC headers>"
   ```
2. Snapshot all WAL files:
   ```bash
   kubectl exec -n exchange <pod> -- /opt/scripts/wal_backup.ps1
   ```
3. Page on-call security + engineering. Do not roll back without RCA — rollback may obscure the root cause.

### 7.3 `frontiers.consistent: false`

Sub-systems (sequencer, ledger, order projection, trade log, settlement log) disagree about the highest-applied command sequence number. Causes: matching engine queue stalled, ledger commit path stuck, snapshot writer locked.

1. Check pod CPU and threads — a stalled tokio task may be blocked on disk I/O.
2. Inspect the gap:
   ```bash
   curl -s /health | jq .frontiers
   ```
3. If sequencer is ahead of ledger by N for >30 s and stable: the commit path is dead. Proceed to incident escalation.
4. If frontiers reconverge within 30 s: transient, log and continue.

### 7.4 RTO breach

If a deploy/restart took longer than the §11 budget (~1 s on dev hardware), check:

- WAL size — large WAL takes longer to replay (≈1.5 s per million commands per §4.2 of the benchmark report).
- Storage performance — kind/PVC under heavy IO contention will slow WAL parse.
- Auto-snapshot disabled — snapshots cap replay cost. Confirm config `wal.snapshot_interval_commands > 0`.

### 7.5 Custody/withdrawal anomaly

The withdrawal pipeline has multiple safety layers:

- Address whitelist with per-user policies
- Velocity tracker (per-vault rate limit)
- Withdrawal delay policy (cooldown)
- Custody circuit breaker
- Dual-auth governance for policy changes

If a withdrawal looks anomalous:

1. Open the breaker:
   ```bash
   curl -X POST http://<host>:<port>/admin/custody/breaker/reset \
        -d '{"open":true}' -H "Content-Type: application/json" -H "<HMAC>"
   ```
2. Inspect the audit log: `/admin/custody/audit/events`.
3. Page on-call security.

## 8. Common operations

### 8.1 Kill switch (halt all order ingestion)

```bash
POST /admin/kill-switch  body: {"enabled": true|false}
```

Pending governance approval may be required (dual-auth) — see `/admin/risk/governance/actions`.

### 8.2 Mass cancel

- Single user: `POST /mass-cancel/user`
- Single session: `POST /mass-cancel/session`
- Whole market: `POST /mass-cancel/market` (admin only)

### 8.3 Market state transitions

```
POST /admin/market-state  body: {"market_id":"<m>","state":"normal|stress|auction_call|cancel_only|halted|maintenance|closed"}
```

State transitions are validated against the allowed graph (see `MarketState` enum in `crates/types`). Pending governance approval applies.

### 8.4 Funding settlement

`POST /admin/risk/funding/settle` to manually settle funding between users (admin operation, dual-auth-eligible).

### 8.5 Reconciliation snapshots

- `GET /admin/risk/reconciliation/settlements` — flat dump of settlement records vs trade journal.
- `GET /admin/risk/reconciliation/core-chain` — frontier view across sequencer, order projection, trade log, ledger.

Use these when investigating frontier-inconsistency incidents (§7.3).

## 9. Disaster recovery

### 9.1 WAL backup

Scheduled via `scripts/wal_backup.ps1`. Outputs a tar.gz to `artifacts/wal-backups/` with a manifest. Default retention 14 archives.

### 9.2 WAL restore drill

`scripts/run_wal_restore_drill.ps1 -BackupArchive <path>` extracts a backup to a sibling `restore-drill/` directory and runs an integrity check. **Run weekly** in staging to validate that backups are restorable.

### 9.3 Full-data wipe + seed (last resort)

If `data/` PVC is unrecoverable:

1. Scale deployment to 0.
2. Delete PVC `exchange-data` (the bench artifacts PVC can stay).
3. Re-apply the overlay — fresh PVC binds.
4. Deployment scales back to 1; api boots on empty WAL with seeded demo balances per `crates/api/src/main.rs::seed_demo_balances`.
5. Fund production accounts via admin `POST /deposit`. **Tracked in private secrets vault.**

This loses all transactional history. Use only when WAL is verifiably destroyed and no backup exists.

## 10. Contacts and escalation

| Role | Reach | When to page |
|---|---|---|
| On-call backend engineer | _to fill_ | Pod crashlooping, frontiers inconsistent, RTO breach |
| On-call security engineer | _to fill_ | `balance_invariant: false`, custody anomaly, suspected exploitation |
| Engineering manager | _to fill_ | Rollback decision required, prolonged outage (>30 min) |
| Product / business owner | _to fill_ | Trading halt, public communication required |

## 11. Post-incident

After any P0 or P1 incident:

1. File an incident report under `docs/incidents/<YYYY-MM-DD>-<short-title>.md`.
2. Update §7 of this runbook if a new failure mode is observed.
3. Update the security review summary if the incident was security-relevant.
4. Open follow-up issues for any code or doc fixes.

## 12. Runbook maintenance

- Review this document quarterly even without incidents.
- Each new operational endpoint or admin command should land with a runbook update in the same PR.
- Stale steps (e.g., scripts referenced here that no longer exist) are blockers — when you find one, fix the runbook before the next deploy.
