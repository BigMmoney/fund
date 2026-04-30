# Technical Architecture and Deployment Reality — 2026-04-30

> **Status update — 2026-05-01.** Sections updated to reflect commits landed during the recovery session: Groups 2c/2d/2e/4b plus frontend completion, runbook scripts restoration, and `.gitignore` housekeeping.

## 1. Executive Summary

The repository (root: `<repo>/`, with `rust-exchange/` as a subdirectory and not a nested repo) hosts a Rust matching/exchange backend, a residual Go services layer, a React/Vite frontend, and a Kubernetes/Docker deployment surface. Active branch: `p0-recovery-20260430` (no upstream), 25 commits ahead of `origin/main`.

**P0 has passed through Step 5.** The recovery session resolved the api codegen pathology (Group 2c, `8b43964`), refreshed the dependency lockfile (Group 2d, `c7d8ca3`), fixed a separate WAL-replay determinism bug surfaced by Step 5 (Group 2e, `1d6ac04`), and landed the deferred CI workflows (Group 4b, `0bd5f1f`). With those in:

- `cargo build --release --bin api` succeeds in 6m 27s under MSVC (cold).
- `cargo test --workspace` passes — 540 tests, 0 failed.
- E2E (`e2e_trading_test.ps1`) passes 5/5 checks.
- Real WAL-replay recovery passes (297 sequencer commands replayed clean).
- Manual restart-after-errors cycle (Phase A → restart → Phase B → restart → Phase C) passes; pre-stop and post-restart `sequencer_command_seq` match exactly across both restarts.

Steps 4 and 5 of the *original* PowerShell test harness (`test_wal_recovery.ps1`, `test_restart_after_errors.ps1`) remain partially inert (the first wipes the WAL it is supposed to test, the second leaks orphan `api.exe` processes via its `cargo run --release` wrapper). The equivalent business semantics were validated by the manual cycle. A follow-up fix to `rust-exchange/scripts/test_lib.ps1::Start-ExchangeService` is queued.

Treat this codebase as **pre-merge**: the v1 core's build/test/E2E gates have all passed in a single coherent session. Remaining items are mostly housekeeping — test-harness fixes, frontend post-rewrite validation, doc cleanup, and deciding what to do with `DEEP_SECURITY_AUDIT_2026-04-07.md`.

## 2. Main Components

| Layer | Path | Status |
|---|---|---|
| Rust workspace (10 crates) | `rust-exchange/crates/{api, matching, ledger, persistence, sequencer, instruments, types, eventbus, risk, projections}` | Active. `instruments`, `ledger`, `persistence`, `sequencer`, `types` committed in `24d98d8`; `matching` in `037e9a1`. The `api` crate (route boxing + 3 new modules `admin_audit.rs` / `beta_controls.rs` / `order_state_projection.rs`) committed in `8b43964` (Group 2c); `Cargo.lock` refreshed in `c7d8ca3` (Group 2d); WAL-replay terminal-command skip fix landed in `1d6ac04` (Group 2e). |
| HTTP/WS API entry point | `rust-exchange/crates/api` (binary `api`) | Builds via `cargo build -p api --release`. Cargo check ~10s incremental (~2m cold under MSVC). Release build verified at 6m 27s post-Group-2c. |
| Matching engine | `rust-exchange/crates/matching/src/{partitioned.rs, high_performance.rs, lib.rs}` | Partitioned-engine refinements + 4 examples + 3 integration tests + 1 bench landed in `037e9a1`. `matching/examples/crash_recovery_drill.rs` exists at HEAD (used by recovery drill). |
| Go services (residual) | `api/`, `matching/` (top-level Go), `simulator/`, `benchmark/` | `hft-stream/` and `price-service/` retired in `f8df1b1`. New HTTP benchmark service at `benchmark/cmd/exchange_http_bench/`. Module `pre_trading`, Go 1.21. |
| Frontend | `frontend-modern/` (React + Vite + TypeScript) | Legacy `pages/services/contexts/hooks/lib/components` deletions committed in `b25252e`; new minimal JSON-panel-driven shell (`AppShell.tsx`, `BusinessPage.tsx`, `ControlPage.tsx`, `SystemPage.tsx`, `JsonPanel.tsx`, `Panel.tsx`) committed in `205a93e`. `npm run build` and `npm run lint` post-rewrite have **not** been re-run in this session. |
| Deployment manifests | `rust-exchange/deploy/k8s/{base, overlays/docker-desktop, observability, benchmarks}/`, `rust-exchange/Dockerfile`, `rust-exchange/docker-compose.yml` | Committed in `31ca355`. |
| Operator scripts | `rust-exchange/scripts/{wal_backup, run_wal_restore_drill, test_wal_recovery, e2e_trading_test}.ps1`, plus benchmark/test PS1 scripts | P0 scripts committed in `bdd28bd`. CI helpers (top-level `scripts/run_backend_recovery_checks.ps1`, `scripts/run_backend_resilience_benchmarks.ps1`) and `rust-exchange/scripts/run_recovery_drill.py` (reconstructed from `.pyc` disassembly) committed in `0bd5f1f` (Group 4b). Documented runbook scripts (`test_lib.ps1`, `test_insufficient_funds.ps1`, `test_restart_after_errors.ps1`, `quick_perf_test.ps1`, `benchmark_suite.ps1`, `soak_test_v2.ps1`, `cancel_storm_test.ps1`, `prepare_k8s_local_images.ps1`, `backend_resilience_lib.ps1`) plus `scripts/security_audit.ps1` committed in `699dd9d`. ~30 orphan harness scripts remain untracked, pending per-script triage. |

## 3. Runtime / Order Flow

Based on `crates/api`, `crates/matching`, `crates/persistence`, and the committed `docs/REAL_ARCHITECTURE_AND_DATA_FLOW_ZH.md`:

```
HTTP/WS request
  → api crate (warp routes; auth via HMAC-SHA256, role mapping, rate limiting)
  → submit_order / cancel_order command
  → sequencer (per-shard atomic seq assignment, idempotency)
  → persistence (JSONL WAL append + CRC32, file-rotation backups)
  → matching engine (PartitionedMatchingEngine, batch-window FBA, depth/snapshot)
  → ledger (debit/credit balance + position deltas; financial source of truth)
  → eventbus (broadcast Fill / OrderState / MarkPrice)
  → WS hub (per-market trade/orderbook/ticker/user/liquidation/mark-price feeds)
```

The default API port is `3030`. Internal auth uses HMAC-SHA256 over `timestamp + method + path + body_sha256` with a 32-byte+ shared secret loaded from a file.

## 4. Auth and Security

- **HMAC-SHA256 internal auth** with a minimum 32-character shared secret. Secret loaded preferentially from `data/internal_auth.secret` (file mode `0600`) or env var `INTERNAL_AUTH_SHARED_SECRET` (deprecated path).
- **Role-based access** via a separate `role_mapping.json` file; roles include `admin`, `operator`, `user`, `bench-admin`. Filter-level checks: `require_admin()`, `require_operator()`, `require_user()`.
- **Per-IP and per-user rate limiting** on order submission (Group 2c, committed in `8b43964`). Auth failure tracking with timed bans is described in `rust-exchange/SECURITY.md` (committed in `7503cc8`).
- **Secret schema migration in flight**: the committed `deploy/k8s/exchange.yaml` Secret was reshaped from env-var keys (`INTERNAL_AUTH_SHARED_SECRET`) to file-mounted keys (`internal_auth.secret`, `role_mapping.json`). Operators upgrading from older manifests must update Secret keys before next apply.
- **Sensitive findings live in `rust-exchange/DEEP_SECURITY_AUDIT_2026-04-07.md`** (660 lines; 22 findings P0–P3, 8 resolved per the doc's own status table). This file is **deliberately untracked** because its still-open exploit scenarios are not appropriate for public-repo distribution. It must be moved to private storage or have its still-open exploit sections redacted before any future commit. Do not commit this file as-is.

## 5. Local Docker Deployment

`rust-exchange/Dockerfile` (multi-stage, Debian-slim runtime) and `rust-exchange/docker-compose.yml` (committed `31ca355`) define the local single-node deployment:

- Container hardening: `init: true`, `read_only: true` filesystem, `tmpfs: /tmp`, `security_opt: no-new-privileges`, `cap_drop: ALL`.
- Auth via either `INTERNAL_AUTH_SHARED_SECRET_FILE` (preferred, mounted from `./secrets:/run/secrets/exchange:ro`) or `INTERNAL_AUTH_SHARED_SECRET` env (legacy; both passthrough only — no secret committed).
- Healthcheck: `curl -fsS http://localhost:3030/ready` on a 10s interval.
- WAL paths under `/app/data/...jsonl` for ledger, sequencer, matching, position-cost, governance.
- Port 3030 exposed.

Local deployment requires the operator to provide `secrets/internal_auth.secret` (32-byte+ random) before `docker compose up`.

## 6. Kubernetes Deployment

Kustomize layout under `rust-exchange/deploy/k8s/` (committed `31ca355`):

- **`base/`** — Namespace `exchange`, ServiceAccount (`automountServiceAccountToken: false`), PVC, Deployment (runAsNonRoot 1000, seccompProfile RuntimeDefault), Service.
- **`overlays/docker-desktop/`** — local-cluster patches: NodePort service, `standard` storageClass, image rename `ghcr.io/OWNER/rust-exchange → rust-exchange-local:dev`. Includes a **dev-only Secret** (`internal_auth.secret = "deployment-acceptance-secret-32-bytes!!"`) explicitly for docker-desktop use only.
- **`observability/`** — Prometheus `ServiceMonitor` (CRD from `monitoring.coreos.com/v1`).
- **`benchmarks/`** — `staircase-benchmark-job` Job + artifacts PVC + docker-desktop overlay.
- **`exchange.yaml`** — single-file manifest (Namespace + ServiceAccount + ConfigMap + Secret template + PVC + Deployment + Service + ServiceMonitor) with all credential fields as explicit placeholders (`CHANGE_ME`, `replace-me`, empty AWS keys).
- **`exchange-restore-job.yaml`** — Job for WAL restore from S3.

Production overlays must supply their own real Secret (file-mounted, not env-var). The committed Secret schema uses `internal_auth.secret` and `role_mapping.json` keys, not the older `INTERNAL_AUTH_SHARED_SECRET` env-var key.

`kubectl kustomize` against any of the 5 entry points resolves cleanly (every `kustomization.yaml` reference is satisfied within the committed tree). Image references contain a literal `OWNER` placeholder that the operator must rewrite before applying.

## 7. CI/CD Status

`.github/workflows/`:

| Workflow | Status |
|---|---|
| `ci.yml` | Modified to add `./api` to `go vet` / `go test` lists. Committed in `0bd5f1f` (Group 4b). |
| `rust-ci.yml` | Adds `audit` (cargo-audit), `guardrails` (calls `run_recovery_drill.py`), and `benchmark-guardrails` jobs. Committed in `0bd5f1f`. |
| `backend-resilience.yml` | New: PR + push + daily 02:00 UTC schedule + manual dispatch. Committed in `0bd5f1f`. |
| `recovery-drills.yml` | New: manual + weekly Monday 03:00 UTC. Committed in `0bd5f1f`. |
| `bench.yml`, `release.yml`, `rust-release.yml`, `system-invariants.yml` | Existing, tracked, untouched. |

Group 4b is **landed**. `rust-exchange/scripts/run_recovery_drill.py` was reconstructed 1:1 from its `.pyc` bytecode (Python 3.13.5) and committed alongside the workflows; `--help` invocation has been verified. The api crate's Linux-CI compile path is now feasible with Group 2c in place; the next CI run on `main` will be the first end-to-end validation under the actual GitHub-Actions Ubuntu runner.

## 8. Build/Test/P0 Status

**Recovery-session P0 run** (`rust-exchange/artifacts/p0_run_20260430_141142/`, post Group 2c/2d/2e/4b):

| # | Step | Status | Evidence |
|---|---|---|---|
| 1 | `cargo build --release --bin api` | ✅ Pass | 6m 27s, MSVC `[optimized]`, EXIT=0. `01_build.log`. Binary `target/release/api.exe` ~14 MB. |
| 2 | `cargo test --workspace` | ✅ Pass | 540 tests, 0 failed, 0 ignored. `02_test.log`. Test compile 32.20 s. |
| 3 | `e2e_trading_test.ps1` | ✅ Pass | 5/5 checks (sell maker, buy taker, stress, metrics delta, WAL growth). 30/100 stress success rate is above the script's threshold; 57/100 are validation-rejected (expected). `03_e2e_clean_data.log`. |
| 4 | WAL replay recovery (manual, real) | ✅ Pass | 297 sequencer cmds replayed cleanly from post-E2E backup; frontiers consistent, balance invariant holds. `04b_wal_replay_api_server.log`. |
| 4 | `test_wal_recovery.ps1` (script) | ⚠ Inert | Script wipes the WAL it is supposed to test, then runs a clean-restart smoke. Returns 0 but does not exercise replay. |
| 5 | Restart-after-errors (manual) | ✅ Pass | Two consecutive restarts on same `data/` after a mix of valid/invalid orders; `sequencer_command_seq` matches pre-stop and post-restart on both restarts; post-restart valid orders accepted. |
| 5 | `test_restart_after_errors.ps1` (script) | ⚠ Inert | `Start-ExchangeService` in `test_lib.ps1` launches via `Start-Process cargo run --release` and captures the cargo wrapper as `$Script:ExchangeProcess`; subsequent `Stop-ExchangeService` kills only cargo, leaving an orphan `api.exe` holding the port. Script reports 0/0 scenarios. |
| 6 | `wal_backup.ps1` → `run_wal_restore_drill.ps1` | ✅ Pass (prior run) | `wal-20260429-224701.tar.gz` (24 files), `restore-drill/restore_drill_report.json`. |

Subsequent in-session validations:

| Check | Result |
|---|---|
| `cargo check -p api` (incremental) | ~10 s, EXIT=0 |
| `cargo check -p api` (cold MSVC) | 2m 18s, EXIT=0 |
| `cargo check -p matching` | exit 0, ~4 s (`02b_check_matching.log`) |
| `go vet ./...` | exit 0, no diagnostics |

The `api` release binary (`target/release/api.exe`) is fresh as of this session and is the artifact that exercised P0 Steps 1-5. WAL state at end of session is healthy through `sequencer_command_seq=161` (includes the diagnostic tiny order from the bug-fix verification).

## 9. Frontend Status

`frontend-modern/` rewrite is committed but **not validated**:

- ~70 legacy files (entire `src/{components,contexts,hooks,i18n,lib,pages,services}/` trees) deleted in `b25252e`. Modifications to `App.tsx`, `AppShell.tsx`, `index.css`, `main.tsx`, `index.html`, `tsconfig.app.json`, `vite.config.ts`, `services/exchangeApi.ts` landed in the same commit.
- New minimal shell — `JsonPanel.tsx`, `Panel.tsx`, `BusinessPage.tsx`, `ControlPage.tsx`, `SystemPage.tsx` — committed in `205a93e` (5 files, +1,572 lines).
- Casing collision resolved in `6da0cec` (`exchangeAPI.ts → exchangeApi.ts`).
- `frontend-modern/dist/*` (Vite build output) is gitignored.
- `npm run build`, `npm run lint`, and TypeScript type-checking have **not** been run since the rewrite landed. Validation is still pending.

The frontend rewrite compiles in import-graph terms (every `App.tsx` import resolves to a committed file) but hasn't been actually built or type-checked end-to-end in this session.

## 10. Known Risks and Blockers

1. ~~`api` release build codegen pathology.~~ **Resolved** by Group 2c (per-leaf `.boxed()` across 6 large `build_*_routes` modules, ~67 leaves). Release build now finishes in 6m 27s under MSVC.
2. ~~`.cargo/config.toml` MinGW pin.~~ **Resolved** by reverting the working-tree change to HEAD's MSVC config. No commit needed (the working-tree pin was a workaround for the now-fixed codegen pathology). MSVC `/DEBUG:NONE` link-arg workaround retained.
3. ~~Workflow merge order / `run_recovery_drill.py` missing.~~ **Resolved** by Group 4b. Script reconstructed from `.pyc` disassembly; `--help` verified.
4. **WAL replay determinism (`bootstrap.rs:166`)** — *was* a real defect: `should_skip_replay_record` used to skip only `Rejected | Cancelled`. After ledger recovery, re-running a `Settled` command's `submit_new_order` would fail preflight (post-settlement available cash < original notional) and panic the bootstrap. **Fixed** in Group 2e (`1d6ac04`) by extending the skip set to include `Settled | Completed`. The previously-panicking `data/` snapshot now boots cleanly.
5. **`DEEP_SECURITY_AUDIT_2026-04-07.md`** — 660 lines including detailed exploit scenarios for 14 still-open findings. Not safe to commit to a public repo. Awaiting private-handling decision. **Open.**
6. **Stale doc references** — committed `ARCHITECTURE_REALITY_ZH.md` and `BACKEND_README_ZH.md` (`7503cc8`) reference now-retired `hft-stream/` and `price-service/` directories in 11 places. A docs-refresh pass is queued. **Open.**
7. **Frontend not validated** — `npm run build` and `npm run lint` have not been re-run since the rewrite committed. Imports resolve in static inspection but no build verification. **Open.**
8. **Test harness bugs** —
    - `rust-exchange/scripts/test_lib.ps1::Start-ExchangeService` launches via `Start-Process cargo run --release` and captures cargo (not the api child). `Stop-ExchangeService` then kills only cargo, leaving `api.exe` orphaned on port 3030. **Open.**
    - `rust-exchange/scripts/test_restart_after_errors.ps1` does not propagate scenario results into `$phaseResults`, so it always reports `Scenarios passed: 0/0` regardless of outcome. **Open.**
    - `rust-exchange/scripts/test_wal_recovery.ps1` is mis-named — it wipes the WAL it should replay against, then runs a clean smoke. **Open.**
9. **Docker bench dependency** — `benchmark/Dockerfile.exchange-http-bench` expects a pre-built `benchmark/bin/exchange_http_bench_linux_amd64` which is not in-repo. CI must build it before `docker build`. **Open.**
10. **Orphan scripts under `rust-exchange/scripts/`** — ~30 PS1 / .bat / sub-directories with no doc, runbook, or CI reference. Each needs a per-author "still useful?" review before commit-or-delete. **Open.**
11. **Orphan `stress.rs` module** in `rust-exchange/crates/api/src/` — declared `mod stress;` in `main.rs:77`, marked `#![allow(dead_code)]`, no `use stress::*` anywhere, no other module references it. Compiles in but is fully dark code. Cleanup candidate. **Open.**
12. **Codex agent local logs** under `rust-exchange/.codex-logs/` are gitignored but suggest earlier automated edits to `trading.rs` outside this session. Provenance of all in-flight changes should be reviewed before push.

## 11. Safe vs Unsafe Claims

**Safe to claim:**
- The `api` crate compiles on this machine in release mode under MSVC (6m 27s cold).
- `cargo test --workspace` passes — 540 tests, 0 failed.
- E2E order flow (maker, taker, stress, metrics, WAL growth) passes end-to-end against a freshly-built api binary.
- Real WAL-replay recovery passes (297 sequenced commands replayed cleanly; frontiers consistent; balance invariant holds).
- Manual restart-after-errors cycle survives two consecutive restarts without panic; pre-stop and post-restart `sequencer_command_seq` match exactly.
- Lower-stack Rust crates (`instruments`, `ledger`, `persistence`, `sequencer`, `types`, `matching`, `risk`, `projections`, `eventbus`) compile and pass their unit tests.
- The Go workspace passes `go vet ./...`.
- WAL backup and restore round-trip works on the committed P0 scripts.
- Kustomize manifests resolve cleanly.
- All committed Secret values are explicit placeholders.

**Not safe to claim:**
- The frontend builds, lints, or type-checks (no `npm run build` post-rewrite).
- The original PowerShell test harness's Steps 4 and 5 work as advertised (they are inert; manual equivalents pass).
- The system passes a fully-automated P0 from a single script (currently requires the manual restart-after-errors cycle).
- The k8s manifests have been applied to a real cluster (no `kubectl apply` was run).
- Docker images build (no `docker build` was run).
- The new CI workflows (`backend-resilience.yml`, `recovery-drills.yml`) will succeed on the actual GitHub-Actions Ubuntu runner — they have only been validated in source.

## 12. Recommended Next Steps

1. **Validate the frontend rewrite** — run `cd frontend-modern && npm run build && npm run lint`. The rewrite is committed (`b25252e` + `205a93e`) but no build/lint has been re-run since.
2. **Fix the test-harness bugs** (Risk #8): repoint `test_lib.ps1::Start-ExchangeService` at `target/release/api.exe` directly (skip the `cargo run` wrapper), fix `test_restart_after_errors.ps1` to populate `$phaseResults` from each scenario, and either rename or rewrite `test_wal_recovery.ps1` so it actually exercises replay.
3. **Decide on `DEEP_SECURITY_AUDIT_2026-04-07.md`** — move to private storage, or redact still-open exploit sections and rename to `docs/SECURITY_REVIEW_2026-04-07.md` before any commit.
4. **Triage the ~30 orphan scripts** under `rust-exchange/scripts/` — per-script "still useful?" review with the original author. Commit the keepers, delete the rest.
5. **Docs-refresh commit** to clean up the 11 stale `hft-stream` / `price-service` references in the root-level Chinese READMEs (`ARCHITECTURE_REALITY_ZH.md`, `BACKEND_README_ZH.md`).
6. **Decide on the orphan `stress.rs` module** — either start using it (wire to a `cargo run --bin stress`-style harness) or delete the file plus its `mod stress;` declaration.
7. **Push the branch** and observe the new CI workflows on a real Ubuntu runner. The recovery-drill cron schedules (daily 02:00 UTC, weekly Mon 03:00 UTC) become live once the workflows are on `main`.
8. **End-to-end P0 from a single command** — once #2 lands, the official PowerShell harness should drive Steps 1-5 without manual intervention. Re-run and capture the artifact set in `rust-exchange/artifacts/p0_run_<timestamp>/`.
9. **Operator deployment dry run** — `kubectl kustomize deploy/k8s/overlays/docker-desktop | kubectl apply -f -` against a local cluster to validate the manifests. No live cluster has been touched in this session.
10. **Bench/Docker validation** — actually build `benchmark/Dockerfile.exchange-http-bench` (currently expects a pre-built binary that isn't in-repo) and run the `docker compose up` smoke locally.

## 13. Evidence Inspected

This document was assembled from read-only inspection of:

- `git log --oneline origin/main..HEAD` → 14 commits ahead, branch `p0-recovery-20260430`.
- `git status --short --branch` (sampled).
- `git diff` of pending files in `rust-exchange/crates/api/`, `frontend-modern/`, `.github/workflows/`.
- `rust-exchange/artifacts/p0_run_20260429_223531/{00_p0_status.md, 01_build.log, 01_build_after_boxed.log, 01b_check_after_groups.log, 02b_check_matching.log}`.
- `rust-exchange/artifacts/wal-backups/` (manifest + tarball).
- `rust-exchange/SECURITY.md`, `rust-exchange/README.md`, `rust-exchange/README_ZH_BACKEND.md`, `rust-exchange/REAL_CLUSTER_RUNBOOK_ZH_2026-04-11.md`, `docs/REAL_ARCHITECTURE_AND_DATA_FLOW_ZH.md`, `docs/PROJECT_STATUS_FOR_AI.md` (all committed in `7503cc8`).
- `docs/P0_DEPLOYMENT_READINESS.md` (committed in `bdd28bd`).
- `rust-exchange/Dockerfile`, `docker-compose.yml`, `deploy/k8s/**` (committed in `31ca355`).
- `.github/workflows/{ci.yml, rust-ci.yml, backend-resilience.yml, recovery-drills.yml}` (committed + on-disk).
- `rust-exchange/crates/{api, matching, ledger, persistence, sequencer, instruments, types}/src/lib.rs` and per-module sources.
- `crates/api/src/{main.rs, trading.rs, security.rs, websocket.rs}` diffs vs HEAD.
- `crates/matching/src/{lib.rs, partitioned.rs, high_performance.rs}` and Cargo.toml.
- `frontend-modern/src/services/exchangeApi.ts` (post-rename), `frontend-modern/src/{App.tsx, components/AppShell.tsx, pages/*Page.tsx}`.
- Process state: `Get-Process cargo,rustc,gcc,ld,link,collect2,lld,lld-link` returned none after the in-flight stall was killed earlier in this session.

No `cargo build`, `cargo test`, `npm run build`, `npm run lint`, `kubectl apply`, or `docker build` was executed during the assembly of this document.
