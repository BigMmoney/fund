# Backend Reliability RC 0.1 — Release Notes

> **Status:** Release Candidate. NOT a production release. Suitable for staging deploy and stakeholder demos.
> **Branch:** `p0-recovery-20260430`
> **Tag:** `rc-0.1` (pending sign-off)
> **Date:** 2026-05-02
> **Commits since `origin/main`:** 36
> **Last commit at tag time:** `32dc842 docs: add production/staging runbook`

## 1. What this release is

The first release-candidate cut focused on **backend reliability**: fixing the type-check + WAL-replay blockers that prevented the api crate from building and starting cleanly, restoring the documented operational harness, validating P0 end-to-end, capturing a formal benchmark baseline (RTO/RPO; throughput deferred), and producing the operational artifacts needed to deploy and demo the backend.

It is NOT yet a production release. Throughput baselines have not been captured on dedicated hardware; the security audit closure is pending; CI runner-green status is pending observation.

## 2. Highlights

| Area | What changed |
|---|---|
| **api crate compiles + builds** | Route boxing across 6 large `build_*_routes` modules cut type-check from infinity (>10 min SIGTERM) to <30 s. Cold release build: 6m 27s under MSVC. |
| **WAL replay determinism** | Bootstrap no longer panics when re-applying terminally-settled commands. The previously-corrupt `data/` snapshot from a prior session boots cleanly. |
| **P0 harness reliability** | Phase 4 (`test_wal_recovery.ps1`) now actually exercises replay instead of wiping WAL. Phase 5 (`test_restart_after_errors.ps1`) propagates per-scenario results and avoids orphan `api.exe` from cargo-wrapper service control. |
| **One-shot P0 driver** | `run_p0_full.ps1` orchestrates Steps 1-6 with aggregated JSON report. |
| **Benchmark tooling** | `wal_append`, `replay_scaling` criterion benches; `measure_rto_rpo.ps1`; `run_full_benchmark_suite.ps1` orchestrator; `bench_compare.ps1` comparator with fixture-based self-test. |
| **First baseline** | `benches/baselines/2026-05-01.json` — RTO/RPO only (throughput deferred until dedicated bench host). |
| **CI workflows** | Recovery drill Python script restored from `.pyc`; backend-resilience and recovery-drills workflows landed. |
| **Frontend rewrite** | Legacy components/contexts/hooks/i18n retired; minimal JSON-panel shell committed. |
| **Demo + docs** | Trade-journey demo script (end-to-end match cycle, narrated). Architecture snapshot, benchmark report (with stability assessment + soak section + harness latency decomposition), security review summary skeleton, production/staging runbook. |

## 3. Validation summary

### 3.1 P0 (`scripts/run_p0_full.ps1` equivalent)

| Step | Tool | Result |
|---|---|---|
| 1 | `cargo build --release --bin api` | PASS — 6m 27s cold MSVC, EXIT=0 |
| 2 | `cargo test --workspace` | PASS — 540 tests, 0 failed |
| 3 | E2E (`e2e_trading_test.ps1`) | PASS — 5/5 checks, sell maker, buy taker, stress, metrics delta, WAL growth |
| 4 | WAL replay (manual + harness) | PASS — 297 cmds replayed clean; `test_wal_recovery.ps1` 3-phase across 2 restarts |
| 5 | Restart-after-errors (manual + harness) | PASS — 5/5 scenarios, post-restart valid order accepted, `frontiers.consistent` true throughout |
| 6 | WAL backup + restore drill | PASS (prior run, retained) |

### 3.2 Stability

- **30-min soak** (8 concurrency, 35,872 successful submissions, 0 failures, tail-latency degradation +1%) — `BENCHMARK_REPORT.md §11`.
- **3-run criterion stability** captured for wal_append and replay_scaling — variance 19-48% on dev hardware (insufficient for CI gating; deferred as exploratory).
- **RTO p99** = 0.787 s, **RPO loss** = 0 across 15 hard-kill cycles. CI-gateable.

### 3.3 Latency decomposition

The §11 soak's 253 ms client-side P99 is **dominated by `curl.exe` process spawn + Windows runspace overhead** (~93%), not api processing. Real api hot-path P99: server-side <1 ms; Go HTTP client end-to-end: 17.5 ms. See `BENCHMARK_REPORT.md §11.6`.

### 3.4 Trade journey demo

`scripts/demo_trade_journey.ps1` — end-to-end match cycle on a fresh local api: alice (taker) buys, bob (maker) sells, settlement direction correct, fees collected, frontiers consistent, balance invariant ok, match_e2e ≈ 1.5 ms.

### 3.5 Docker Desktop k8s deployment

The api builds and runs cleanly on Docker Desktop's KIND-based k8s (node `desktop-control-plane`):

- Image: `rust-exchange-local:dev` from current branch HEAD.
- Pod state: `Running 1/1`, 0 restarts.
- Liveness/readiness probes return 200.
- `/health` and `/ready` via `kubectl port-forward` confirm `frontiers.consistent: true`, `balance_invariant: true`.

## 4. Commits in RC 0.1 (in topological order, oldest first)

> 36 commits total. Hashes are abbreviated.

| Hash | Type | Subject |
|---|---|---|
| 70af2a7 | cleanup | remove archived benchmark and paper version artifacts |
| ca0e701 | feat | rust-exchange runtime core and delivery tooling alignment |
| dabeb86 | feat | frontend consolidate routed app shell and quality gates |
| d82a444 | feat | go-services compatibility services and benchmark coverage refresh |
| da858a5 | chore | ignore local build and audit artifacts |
| bdd28bd | chore | P0 WAL backup + restore drill scripts and runbook |
| 6da0cec | refactor | normalize exchange API service casing |
| 7503cc8 | docs | refresh architecture, security, and deployment notes |
| b0610cd | docs | drop references to retiring root-level scripts |
| 124e909 | chore | remove legacy root-level startup and test scripts |
| f8df1b1 | feat | retire hft-stream/price-service; add HTTP bench service |
| 31ca355 | deploy | k8s base + overlays/docker-desktop + observability + benchmarks |
| 24d98d8 | feat | extend instruments, ledger, persistence, sequencer, types crates |
| 037e9a1 | feat | matching partitioned engine refinements + tests + bench |
| **b25252e** | chore | retire legacy frontend-modern components/contexts/services/pages |
| **8b43964** | **feat** | **harden order validation and box warp routes** *(api crate compile blocker fix)* |
| **c7d8ca3** | build | refresh Cargo.lock for API and matching dev dependencies |
| **1d6ac04** | **fix** | **skip terminal commands during WAL replay recovery** *(P0 startup-panic fix)* |
| 0bd5f1f | chore | restore recovery drill script and add backend-resilience workflows |
| 205a93e | feat | frontend-modern minimal JSON-panel shell |
| 699dd9d | chore | restore documented runbook + security-audit harness |
| b7c7f35 | chore | ignore local artifacts, data backups, IDE solution files |
| fa59bd4 | docs | architecture and deployment reality snapshot |
| acf5c45 | fix | real WAL replay test, restart-after-errors counting, no orphan api.exe |
| 5a421cf | feat | one-shot P0 wrapper with aggregated JSON report |
| 5427842 | feat | WAL append, replay scaling, RTO/RPO harnesses |
| 2aeca44 | feat | benchmark orchestrator with JSON summary |
| 3759ce8 | feat | baseline comparator with fixtures and self-test |
| 1f21485 | docs | 2026-05-01 backend benchmark report |
| a67efb3 | docs | stability assessment + RTO/RPO baseline (throughput deferred) |
| f387496 | fix | correct soak harness secret and metric aggregation |
| d3ea1ea | docs | 30-minute soak stability section |
| ff3a4fc | docs | decompose soak P99 — server <1 ms, harness adds 236 ms |
| eaf4542 | feat | trade-journey demo script |
| 14333a3 | docs | public-safe security review summary skeleton |
| 32dc842 | docs | production/staging runbook |

The two **bold** code commits (`8b43964`, `1d6ac04`) are the headline fixes.

## 5. Known limitations

- **Throughput baseline is exploratory**, not CI-gated. Run-to-run variance on the dev capture host was 19-48% for criterion benches. Re-capture on dedicated bench hardware required before tightening thresholds.
- **CI runner observation pending.** Branch is pushed to `origin/p0-recovery-20260430` but CI status (Linux Ubuntu) has not been visually confirmed by an authenticated session. Likely cross-platform issue: `Start-ExchangeService` falls back to `target/release/api.exe` on Linux runners; needs a `target/release/api` (no `.exe`) branch added before CI is reliably green.
- **Frontend post-rewrite not built.** `npm run build` and `npm run lint` have not been re-run since the rewrite landed in `b25252e`+`205a93e`. Imports resolve in static inspection.
- **Security audit closure pending.** The skeleton at `docs/SECURITY_REVIEW_2026-04-07_SUMMARY.md` is empty. Original audit `DEEP_SECURITY_AUDIT_2026-04-07.md` remains untracked (private). All 22 findings are unsigned in the public summary as of this RC.
- **Performance regression CI gate not wired.** `bench_compare.ps1` works against fixtures but is not yet invoked from `rust-ci.yml`. Wiring requires the throughput baseline above.
- **Test harness scripts have known cosmetic issues** documented but not blocking: `cancel_storm_test.ps1` has the same short-secret bug fixed in `f387496` for `soak_test_v2.ps1` — pending follow-up commit.
- **30+ orphan PowerShell scripts** under `rust-exchange/scripts/` and root `scripts/` are untracked, awaiting per-author triage.
- **`stress.rs` orphan module** declared `mod stress;` in api `main.rs` but never imported via `use stress::*`. Cleanup candidate.

## 6. Recommended deploy targets for RC 0.1

- **Suitable:** Local Docker Desktop k8s, single-node staging via docker-compose, dev-team integration sandbox.
- **NOT suitable:** Customer-facing staging, production, or any environment that gates on signed security audit closure or CI-enforced regression thresholds.

## 7. Upgrade path from `origin/main`

For an existing operator running an older build:

1. Pull `p0-recovery-20260430`.
2. Read `docs/STAGING_RUNBOOK.md`, especially §6.3 (rollback safety with WAL state) and §7.1 (CrashLoopBackOff diagnosis).
3. Take a WAL backup of the existing `data/` PVC (`scripts/wal_backup.ps1`).
4. Build + push the RC image.
5. Deploy. The 1d6ac04 fix means RC 0.1 can boot on WAL state that older binaries panicked on.

**Do not roll back across `1d6ac04`** — the older binary cannot read forward-evolved WALs that contain Settled-lifecycle records produced by post-fix runs.

## 8. Tag procedure

When this RC is approved for tagging:

```bash
git tag -a rc-0.1 32dc842 -m "Backend Reliability RC 0.1 — see docs/releases/RC_0.1_RELEASE_NOTES.md"
git push origin rc-0.1
```

## 9. RC 0.2 candidate scope

Items deferred from RC 0.1 that should land in RC 0.2:

- CI runner-green confirmation + cross-platform `Start-ExchangeService` fix.
- Frontend `npm run build` / `lint` verification.
- Throughput baseline on dedicated bench host (not dev workstation).
- CI performance regression gate wiring.
- Security review summary fully filled in (all 22 findings classified, P0+P1 closed).
- `cancel_storm_test.ps1` secret + metric fix (parallel to `f387496`).
- Orphan-script triage commit (delete or keep, with author sign-off).
- `stress.rs` decision (wire into a runnable harness or delete).
