# Production Launch Checklist

> **Branch:** `p0-recovery-20260430` · **HEAD:** `c6b790f` · **Target launch date:** TBD
> Authoritative gate for promoting `rust-exchange` from controlled-trial to production. Each row has a single owner and a verifiable acceptance state. **No row may be skipped.** Items still red block the cutover.

---

## 0. Reading this document

| Column | Meaning |
|---|---|
| **ID** | Stable handle for the gate (cite in PR descriptions and incident reports). |
| **Severity** | `P0` = launch blocker; `P1` = required for customer GA; `P2` = required within 30 days post-launch. |
| **Owner** | The single accountable individual (NOT a team). Hand-off needs an explicit edit to this column. |
| **Status** | `🔴 Open` / `🟡 In progress` / `🟢 Verified` / `⚪ N/A`. Verified requires an evidence link. |
| **Acceptance** | Concrete, falsifiable check. "Tested" is not acceptance — name the test or runbook step. |
| **Evidence** | PR/commit/dashboard URL proving the gate passed. |

---

## P0 — Launch Blockers (must be 🟢 before any external traffic)

### Code & state correctness

| ID | Gate | Owner | Status | Acceptance | Evidence |
|---|---|---|---|---|---|
| P0-CORR-1 | Customer-side balance pre-check at submit | b.greifen | 🟢 Verified | Test `submit_blocked_when_customer_balance_below_amount_plus_fee` passes; `cargo test -p api customer_wallet_http` green | `fa39505` |
| P0-CORR-2 | Velocity tracker is atomic check-and-record | b.greifen | 🟢 Verified | 20-thread `try_record_concurrent_submissions_cannot_breach_cap` passes | `fa39505` |
| P0-CORR-3 | Velocity tracker rebuilt from history at boot | b.greifen | 🟢 Verified | `main.rs` calls `build_velocity_tracker(WithdrawalStore::all())` | `fa39505` |
| P0-CORR-4 | Settlement worker flips to `SettlementStuck` on ledger failure (not log-and-abandon) | b.greifen | 🟢 Verified | `tick_flips_to_settlement_stuck_when_ledger_balance_insufficient` passes | `4c44082` |
| P0-CORR-5 | Idempotency key on `/v2/wallet/withdraw` returns original `withdrawal_id` | b.greifen | 🟢 Verified | `submit_with_duplicate_client_reference_returns_existing_record` + smoke 11c | `4c44082`, `c6b790f` |
| P0-CORR-6 | Validate-time sanctions Hit auto-suspends address | b.greifen | 🟢 Verified | `submit_validate_time_sanctions_hit_suspends_address` passes | `4c44082` |
| P0-CORR-7 | Sanctions provider `Error` is hard-block (503), not silent pass | b.greifen | 🟢 Verified | `add_address_when_sanctions_provider_errors_returns_unavailable` + validate-time variant | `12fd5c7` |

### Security

| ID | Gate | Owner | Status | Acceptance | Evidence |
|---|---|---|---|---|---|
| P0-SEC-1 | HMAC shared secret rotated from `dev-secret-*` to a 32+-byte production secret stored in KMS / sealed env | b.greifen (code), UNASSIGNED (KMS provisioning) | 🟡 In progress | `wallet::SecretLoader` trait + `EnvSecretLoader` (default) + `KmsSecretLoader` scaffold landed; `loader_from_env` selects via `WALLET_SECRET_BACKEND`; `Secret` redacts in Debug + zeroize on drop. Provisioning runbook: `docs/KMS_SECRETS_RUNBOOK.md` | bundle-P0 |
| P0-SEC-2 | `INTERNAL_AUTH_MAX_SKEW_SECONDS` set to a per-environment value (production: 30 s, staging: 120 s) | b.greifen | 🟢 Verified | `internal_auth_max_skew_seconds()` reads env at process start; clamped to [1, 3600]; default 30 s | bundle-P0 |
| P0-SEC-3 | Per-IP rate limit active on `/v2/wallet/*` | b.greifen | 🟢 Verified | `customer_wallet_routes` wraps `ip_rate_limiter`; `RateLimited` returns 429 | `4c44082` |
| P0-SEC-4 | No-self-approval enforced and audited on every maker-checker action | b.greifen | 🟢 Verified | Smoke phase 7 confirms self-approve does not commit + `denied_self_approval` audit row | smoke + `data/admin/rbac_audit.jsonl` |
| P0-SEC-5 | Backoffice bootstrap admin documented and rotated for production | b.greifen (docs), UNASSIGNED (operational drill) | 🟡 In progress | Contract documented in `docs/IDP_INTEGRATION.md` §2 incl. demotion procedure; `bootstrap_admin_seed` audit row already emitted on first boot | bundle-P0 |
| P0-SEC-6 | Real sanctions provider (Chainalysis / TRM) wired behind feature flag with provisioned API keys | b.greifen (code), UNASSIGNED (API key) | 🟡 In progress | `--features chainalysis` ships the real HTTP body — `ureq` GET to `public.chainalysis.com/api/v1/address/{addr}` with retry + transport fall-through to `Error`. Loaded via `SecretLoader`; `unreachable_endpoint_returns_error_status` test verifies fail-closed | bundle-P0 |

### Funds movement

| ID | Gate | Owner | Status | Acceptance | Evidence |
|---|---|---|---|---|---|
| P0-FUND-1 | Real ETH chain adapter wired behind feature flag | b.greifen (read paths), UNASSIGNED (signing + KMS key + RPC URL provisioning) | 🟡 In progress | `--features eth-rpc` ships read-side: `eth_getBalance`, `eth_getTransactionByHash`, `eth_blockNumber`, `eth_feeHistory`-equivalent fee estimate; multi-RPC failover with linear backoff. Write-side (build/sign/broadcast) returns explicit "not yet wired" until KMS-sealed key lands. `unreachable_rpc_returns_rpc_error` test passes | bundle-P0 |
| P0-FUND-2 | Per-chain settlement accounts (`SYS:WALLET:HOT:eth` etc.) replace single `SYS:ONCHAIN_VAULT:USDC` | b.greifen | 🟢 Verified | `SettlementWorker::with_chains` takes a per-chain `ChainSpec` map; `per_chain_settlement_account_isolation` test verifies ETH credits land on `SYS:WALLET:HOT:eth` and BTC on `SYS:WALLET:HOT:btc`; legacy account untouched | bundle-P0 |
| P0-FUND-3 | Per-chain ledger-unit divisor (wei → micro-eth) so amounts can exceed i64 | b.greifen | 🟢 Verified | `ChainSpec::to_ledger_units(amount)` returns `(quotient_i64, remainder)`; overflow surfaces as `SettlementStuck`; `divisor_overflow_is_marked_stuck_not_settled` test passes | bundle-P0 |
| P0-FUND-4 | Maker-checker for above-threshold customer withdrawals (`WALLET_CUSTOMER_MC_THRESHOLD`) | b.greifen | 🟢 Verified | `CustomerWalletRuntime::with_mc_threshold`; submit > threshold parks at `AwaitingApproval` with response `status="awaiting_approval"`; `submit_above_mc_threshold_creates_awaiting_approval_record` test passes | bundle-P0 |

### Recovery

| ID | Gate | Owner | Status | Acceptance | Evidence |
|---|---|---|---|---|---|
| P0-REC-1 | Cold-boot WAL replay reaches the same `(command_seq, ledger_root)` as the source node | b.greifen | 🟢 Verified | Existing recovery tests + `recovery_completed` Order Flow Monitor event | `96cf916` (and ancestors) |
| P0-REC-2 | `data/` directory backed up off-host every 5 min in production | b.greifen (artifacts), UNASSIGNED (S3 bucket + IAM provisioning) | 🟡 In progress | k8s `CronJob` at `deploy/k8s/base/backup-cronjob.yaml` (5-min cadence, tarball + SHA256 manifest, S3 sync, LATEST pointer); systemd alternative at `scripts/exchange-backup.{service,timer}` for non-k8s hosts | bundle-P0 |
| P0-REC-3 | Daily reconciliation runbook complete and tested in staging | b.greifen (drill script), UNASSIGNED (staging dry-run) | 🟡 In progress | `scripts/reconcile_drill.ps1` boots a clean api, drives load, runs INV-1/3/4 + velocity sanity, reports PASS/FAIL with per-violation detail | bundle-P0 |

---

## P1 — Required for Customer GA (must be 🟢 within 7 days of launch)

| ID | Gate | Owner | Status | Acceptance |
|---|---|---|---|---|
| P1-OPS-1 | Prometheus alerts wired for `wallet.settlement.stuck`, sanctions provider error rate, hot wallet balance threshold | UNASSIGNED | 🔴 Open | Alerts fire in staging via injected fault |
| P1-OPS-2 | Distributed tracing (OpenTelemetry) across api ↔ workers ↔ chain RPCs | UNASSIGNED | 🔴 Open | Single trace ID visible end-to-end in Tempo/Jaeger |
| P1-OPS-3 | On-call rota with paging policy for `wallet.settlement.stuck` | UNASSIGNED | 🔴 Open | PagerDuty schedule + escalation policy linked |
| P1-OPS-4 | SLOs defined: 99.9% wallet submit success, p95 settle < 60 s | UNASSIGNED | 🔴 Open | SLO dashboard live |
| P1-CI-1 | `cargo check --workspace --locked` in CI pre-commit | UNASSIGNED | 🔴 Open | CI pipeline green; intentional Cargo.lock drift fails build |
| P1-CI-2 | `cargo test --workspace` in CI on every PR | UNASSIGNED | 🔴 Open | PR gate active |
| P1-CI-3 | rbac_smoke_test.ps1 runs nightly against staging and fails noisily | UNASSIGNED | 🔴 Open | Cron + result dashboard |
| P1-COMP-1 | All P0 audit closures verified by independent code review | UNASSIGNED | 🔴 Open | Reviewer sign-off on each commit |
| P1-COMP-2 | Privacy + AML policy review of customer audit log retention | UNASSIGNED | 🔴 Open | Legal sign-off |

---

## P2 — Required within 30 days post-launch

| ID | Gate | Owner | Status | Acceptance |
|---|---|---|---|---|
| P2-SCALE-1 | Multi-region story: replicated sequencer (Raft), ledger sharding plan documented | UNASSIGNED | 🔴 Open | RFC merged |
| P2-SCALE-2 | JSONL segment rotation + checksums | UNASSIGNED | 🔴 Open | Files rotate at 1 GB; SHA256 column verified on replay |
| P2-SCALE-3 | Hot-standby for wallet workers (active-passive failover) | UNASSIGNED | 🔴 Open | Failover drill < 30 s recovery |
| P2-FUND-1 | BTC chain adapter wired behind feature flag | UNASSIGNED | 🔴 Open | Testnet broadcast verified |
| P2-FUND-2 | SOL chain adapter wired behind feature flag | UNASSIGNED | 🔴 Open | Testnet broadcast verified |
| P2-SEC-1 | HMAC secret rotation runbook executed on schedule | UNASSIGNED | 🔴 Open | First rotation completed |
| P2-SEC-2 | Frontend bearer-token migration (HMAC-in-browser → server-minted JWT) | UNASSIGNED | 🔴 Open | Frontend cutover complete |
| P2-SEC-3 | CSP + `X-Content-Type-Options: nosniff` on all REST responses | UNASSIGNED | 🔴 Open | Headers present in production response |
| P2-OPS-1 | Quarterly DR drill: restore from backup + reconcile | UNASSIGNED | 🔴 Open | Drill report filed |

---

## Verification Cadence

| Cadence | Action |
|---|---|
| **Pre-deploy** | All P0 rows must be 🟢 Verified with linked evidence. PR description references the row IDs touched. |
| **Daily (post-launch)** | On-call confirms `wallet.settlement.stuck` count = 0; reconciliation runbook §3 executed. |
| **Weekly** | Audit log review: any `denied_self_approval` rows, any `sanctions_unavailable` clusters. |
| **Monthly** | P1 status review; close or escalate stragglers. |
| **Quarterly** | Full DR drill (P2-OPS-1); refresh this checklist for next launch wave. |

---

## Sign-off

Launch requires four signatures captured in the launch ticket:

| Role | Name | Date |
|---|---|---|
| Engineering lead | | |
| Security lead | | |
| Compliance lead | | |
| On-call manager | | |

A red P0 row blocks all four signatures by definition. Do not negotiate.

---

*Last updated 2026-05-04 against `p0-recovery-20260430@c6b790f`.*
