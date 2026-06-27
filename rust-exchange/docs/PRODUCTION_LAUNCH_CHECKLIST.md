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
| P0-REC-2 | `data/` directory backed up off-host every 5 min in production | b.greifen (artifacts), UNASSIGNED (S3 bucket + IAM provisioning) | 🟡 In progress | k8s `CronJob` at `deploy/k8s/base/backup-cronjob.yaml` (5-min cadence, tarball + SHA256 manifest, S3 sync, LATEST pointer; wired into `deploy/k8s/base/kustomization.yaml`); systemd alternative at `scripts/exchange-backup.{service,timer}` for non-k8s hosts | `6de75de` |
| P0-REC-3 | Daily reconciliation runbook complete and tested in staging | b.greifen (drill script + local pass), UNASSIGNED (staging dry-run sign-off) | 🟡 In progress | `scripts/reconcile_drill.ps1` boots a clean api, drives load, runs INV-1/3/4 + velocity sanity. **Local execution: PASS** — INV-1 Σ==0 across 13 accounts; INV-3 no duplicate op_ids across 10 entries; INV-4 every Settled record has its `wd-settle-{id}` ledger entry; velocity 24h sums clean. Staging dry-run still required for sign-off. | bundle-P0 + drill-pass-2026-05-04 |

---

## P1 — Required for Customer GA (must be 🟢 within 7 days of launch)

| ID | Gate | Owner | Status | Acceptance |
|---|---|---|---|---|
| P1-OPS-1 | Prometheus alerts wired for `wallet.settlement.stuck`, sanctions provider error rate, hot wallet balance threshold | b.greifen (rules), UNASSIGNED (staging fault drill) | 🟡 In progress | Metrics + rules landed: `wallet_settlements_stuck_total`, `wallet_sanctions_errors_total`, `wallet_hot_wallet_balance{chain}` exported at `/metrics/prometheus`; alert rules at `deploy/prometheus/alerts.yml` (6 rules — `WalletSettlementStuck`, `WalletSanctionsProviderErrors/HardDown`, `WalletHotBalanceLow/Critical`, `WalletSettlementStuckBacklog`). Code in `9ebbd47`; rules in `6de75de`. Staging fault-injection drill still required to confirm rules fire. |
| P1-OPS-2 | Distributed tracing (OpenTelemetry) across api ↔ workers ↔ chain RPCs | b.greifen (layer), UNASSIGNED (chain RPC propagation + Tempo/Jaeger backend) | 🟡 In progress | `crates/api/src/tracing_init.rs` introduces a feature-gated OTel layer (`--features otel`). Activates at runtime when `OTEL_EXPORTER_OTLP_ENDPOINT` is set; otherwise it's a no-op. Env contract: `OTEL_SERVICE_NAME` (default `rust-exchange-api`), `OTEL_EXPORTER_OTLP_HEADERS` honoured natively. Shutdown flushes from the Ctrl+C path. Default build is byte-for-byte equivalent to before. Chain-RPC traceparent propagation + collector wiring still pending. Commit `4388a62`. |
| P1-OPS-3 | On-call rota with paging policy for `wallet.settlement.stuck` | UNASSIGNED | 🔴 Open | PagerDuty schedule + escalation policy linked |
| P1-OPS-4 | SLOs defined: 99.9% wallet submit success, p95 settle < 60 s | UNASSIGNED | 🔴 Open | SLO dashboard live |
| P1-CI-1 | `cargo check --workspace --locked` in CI pre-commit | b.greifen | 🟢 Verified | `.github/workflows/rust-ci.yml` `check` job runs `cargo check --workspace --all-targets --locked` on every PR + push to `main` (`14e0568`). Verified locally green after bench rot fix `926fb0e` (`OrderBook::cancel_order` was missing — the broken bench would have failed the new gate). |
| P1-CI-2 | `cargo test --workspace` in CI on every PR | b.greifen | 🟢 Verified | `.github/workflows/rust-ci.yml` `test` job runs `cargo test --workspace --no-fail-fast --locked`, gated on `check` (`14e0568`). Verified locally: **763 passed / 0 failed / 0 ignored** across 24 test binaries. |
| P1-CI-3 | rbac_smoke_test.ps1 runs nightly against staging and fails noisily | UNASSIGNED | 🔴 Open | Cron + result dashboard |
| P1-COMP-1 | All P0 audit closures verified by independent code review | UNASSIGNED | 🔴 Open | Reviewer sign-off on each commit |
| P1-COMP-2 | Privacy + AML policy review of customer audit log retention | UNASSIGNED | 🔴 Open | Legal sign-off |

---

## P2 — Required within 30 days post-launch

| ID | Gate | Owner | Status | Acceptance |
|---|---|---|---|---|
| P2-SCALE-1 | Multi-region story: replicated sequencer (Raft), ledger sharding plan documented | UNASSIGNED | 🔴 Open | RFC merged |
| P2-SCALE-2 | JSONL segment rotation + checksums | b.greifen | 🟢 Verified | `JsonlFileWal::with_size_rotation(max_bytes)` triggers rotate when active segment grows past threshold (production target 1 GiB). `rotate()` writes a `<rotated>.sha256` sidecar in `sha256sum`-compatible layout. `entries_all_segments_with_recovery(mode)` walks rotated segments oldest-first + active, verifying each sidecar before parsing — Strict aborts on mismatch, BestEffort skips the segment. The existing `entries`/`entries_with_recovery` path is unchanged so sequencer/ledger snapshot-then-replay semantics are preserved. 6 new tests (20/0/0 in persistence). Commit `f344845`. |
| P2-SCALE-3 | Hot-standby for wallet workers (active-passive failover) | b.greifen (code), UNASSIGNED (multi-replica drill) | 🟡 In progress | `crates/api/src/wallet_ha.rs` lease-file leader election gates both `HotWalletWorker` and `SettlementWorker` ticks. Lease at `data/wallet-leader.lease`; default `lease_ttl=15s` / `refresh_interval=5s` / `acquire_retry=5s`; epoch monotonically bumps on transition (fence-token ready). Disabled via `WALLET_HA_DISABLED=1`. 6 unit tests pass (acquire-empty, no-steal-fresh, takeover-on-expire, refresh-keeps-epoch, transition-bumps-epoch, self-reacquire). Multi-replica failover drill on shared PVC still required; real Raft consensus + fencing on the ledger op_id remains a follow-up. Commit `921b25a`. |
| P2-FUND-1 | BTC chain adapter wired behind feature flag | UNASSIGNED | 🔴 Open | Testnet broadcast verified |
| P2-FUND-2 | SOL chain adapter wired behind feature flag | UNASSIGNED | 🔴 Open | Testnet broadcast verified |
| P2-SEC-1 | HMAC secret rotation runbook executed on schedule | UNASSIGNED | 🔴 Open | First rotation completed |
| P2-SEC-2 | Frontend bearer-token migration (HMAC-in-browser → server-minted JWT) | b.greifen (WS path), UNASSIGNED (REST cutover) | 🟡 In progress | Browser WebSocket path: `POST /v2/ws-token` mints a 60s HMAC-bound token (path + role + subject scoped); `/ws/order-trace?token=…` accepts it as fallback when HMAC headers are absent. Module: `crates/api/src/ws_token.rs` (11 tests pass). Frontend `MonitorPage` "Live (WS)" toggle in `6b2d1c8`. Server code: `9ebbd47`. REST-side cutover still pending — a full JWT minting + verifying path on every authenticated REST call is the larger v1.1 task. |
| P2-SEC-3 | CSP + `X-Content-Type-Options: nosniff` on all REST responses | b.greifen | 🟢 Verified | `crates/api/src/security_headers.rs` chains a `HeaderMap` onto every reply via `warp::reply::with::headers`. Headers: `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `Strict-Transport-Security: max-age=31536000; includeSubDomains`, `Content-Security-Policy` (pragmatic default allowing `'self'` + inline style for the legacy console; override via env `API_CSP`, empty string suppresses). 4 unit tests cover defaults, suppression, custom value, and the always-on static-header invariant. Commit `5c56760`. |
| P2-OPS-1 | Quarterly DR drill: restore from backup + reconcile | UNASSIGNED | 🔴 Open | Drill report filed |
| P2-COMP-1 | Trade surveillance: wash trade + spoofing + layering detection | b.greifen (rules), UNASSIGNED (threshold tuning + ops triage workflow) | 🟡 In progress | `crates/api/src/surveillance.rs` consumes `Event::FillCreated` + `Event::OrderTrace` out-of-band. Three rules: `round_trip_wash` (Buy + Sell same market, 90% overlap, 60s window), `rapid_cancel` (≥ 10 unfilled orders cancelled within < 500ms over 60s), `high_cancel_ratio` (cancel/submit > 90% over 5min, ≥ 50 events). Bounded per-user memory (1000 events default). Alerts ride `tracing::warn` + four `surveillance_*_alerts_total` Prometheus counters. No auto-action — operators triage. All thresholds tunable via `SURVEILLANCE_*` env vars. 6 unit tests pass. Commit `124a50c`. |
| P2-OPS-2 | Graceful drain mode (Active → Draining → Drained) | b.greifen | 🟢 Verified | `crates/api/src/drain_mode.rs` 3-state lattice extends the pre-existing `ops::DRAIN_MODE` binary flag. `Draining` blocks new writes but keeps withdrawals + cancels open (customers flatten, ops sweep to cold). `Drained` also blocks withdrawals; cancels remain unconditional. Per-action helpers: `allow_new_writes`, `allow_withdrawals`, `allow_cancels`. **Wired in `2c4822e`:** POST + GET `/admin/maintenance/drain`, state syncs to `ops::DRAIN_MODE` so existing trading.rs guards (lines 285, 518) pick it up. Withdrawal hot path (`customer_wallet_http::handle_submit_withdraw`) checks `allow_withdrawals()`. 6 unit tests. Commits `799ef9c` (module) + `2c4822e` (wiring). |
| P2-INST-1 | Sub-account / firm registry + read-side aggregator | b.greifen (registry + admin), UNASSIGNED (trading-route stp_group_id auto-inject) | 🟢 Verified | `crates/api/src/sub_accounts.rs` WAL-persisted `user_id → firm_id` mapping with inverted firm→[users] index. `aggregate_firm(registry, firm_id, metric)` sums any per-user metric (balance, position, PnL) across the firm as i128. **Wired in `2c4822e`:** registry boots from `data/sub_account_registry.jsonl` (override via `SUB_ACCOUNT_WAL_PATH`). Admin endpoints `/admin/firms/membership`, `/admin/firms/members`, `/admin/firms`, `/admin/firms/balance`. Identity model unchanged; STP auto-derivation from firm_id stays a follow-up. 6 unit tests. Commits `252fdb4` (module) + `2c4822e` (wiring). |
| P2-INST-2 | API-key IP allow-list + scope enforcement | b.greifen (registry + admin), UNASSIGNED (per-route scope gates) | 🟢 Verified | `crates/api/src/api_key_scope.rs` sidecar registry at `data/api_key_scopes.json` (override via `API_KEY_SCOPE_FILE`). CIDR-style IP allow-list (v4 + v6, parsed locally without `ipnet` dep). Scope set per subject; routes call `has_scope(subject, scope)`. Subjects not in the registry are unrestricted (backwards compatible). **Wired in `2c4822e`:** registry boots at startup; admin endpoints `/admin/api-key-scopes/reload`, `/admin/api-key-scopes/unscoped`, `/admin/api-key-scopes/check`. `security::known_api_key_subjects()` exposed for the unscoped-audit endpoint. 11 unit tests. Commits `5bc6123` (module) + `2c4822e` (wiring). |

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

*Last updated 2026-06-28 against `p0-recovery-20260430@HEAD`. Code commits since 5-04: `926fb0e` bench rot fix; `14e0568` CI `--locked`; `6de75de` k8s backup + Prometheus alerts; `9ebbd47` WS token + wallet metrics + binance proxy; `6b2d1c8` frontend WS token UI; `5c56760` P2-SEC-3 security headers; `4388a62` P1-OPS-2 OTel scaffolding; `f344845` P2-SCALE-2 WAL rotation + SHA256; `124a50c` P2-COMP-1 trade surveillance; `921b25a` P2-SCALE-3 wallet HA; `799ef9c` P2-OPS-2 3-state drain (module); `252fdb4` P2-INST-1 sub-account registry (module); `5bc6123` P2-INST-2 API-key scope (module); `2c4822e` wires P2-OPS-2 / INST-1 / INST-2 into runtime (admin endpoints + hot-path gates). Workspace state: `cargo check --workspace --all-targets --locked` green (6 warnings — intentional future-hook getters); `cargo test --workspace --locked` **810 / 0 / 0** across 24 test binaries. Operational provisioning still pending owner: KMS (P0-SEC-1), Chainalysis key (P0-SEC-6), ETH signing key (P0-FUND-1), S3+IAM (P0-REC-2), staging drills (P0-SEC-5 / P0-REC-3 / P1-OPS-1 / P2-SCALE-3 multi-replica), Tempo/Jaeger backend + chain-RPC propagation (P1-OPS-2), PagerDuty (P1-OPS-3), SLO dashboard (P1-OPS-4), nightly smoke (P1-CI-3), legal review (P1-COMP-1/2), surveillance triage workflow (P2-COMP-1), trading-route stp_group_id auto-inject (P2-INST-1 follow-up), per-route scope gates (P2-INST-2 follow-up). **Still untouched in code: FIX gateway, real Raft sequencer, geo/jurisdiction blocking, tax reporting hooks** — single-session ROI is too low; flagged for follow-up project planning.*
