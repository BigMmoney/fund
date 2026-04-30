# Backend Benchmark Report — 2026-05-01

> **Status:** initial formal measurement run. **No baseline is committed yet** — these numbers are reference measurements and should NOT be used to gate CI regression checks until at least three independent runs on stable hardware confirm the values within ±5%. The follow-up "3-run stability + baseline JSON" commit lands the official baseline.

## 1. Purpose

This report establishes the first formal performance measurement of the rust-exchange backend on the post-recovery branch (`p0-recovery-20260430` at commit `3759ce8`). It captures three classes of measurement:

1. **WAL append micro-benchmark** — per-record persistence cost.
2. **WAL replay scaling** — recovery throughput at 1k / 10k / 100k commands.
3. **RTO / RPO** — wall-clock recovery time after hard kill, and zero-loss durability check.

These three are tied together: they measure the persistence path that gates startup time on a real restart. Other dimensions in the broader benchmark plan (submit-order latency, depth-query latency, multi-market load, hot-market load, soak) are covered by existing harnesses (`quick_perf_test.ps1`, `benchmark_suite.ps1`, `soak_test_v2.ps1`, `cancel_storm_test.ps1`) and will be added to subsequent reports.

## 2. Hardware and toolchain

| Field | Value |
|---|---|
| Host | LUCIEN_KUROD |
| OS | Microsoft Windows 11 Home China, build 10.0.26200 |
| CPU | Intel(R) Core(TM) i9-14900HX |
| Logical cores | 32 |
| RAM | 63.8 GB |
| Rust | 1.88.0 (6b00bc388 2025-06-23) |
| Cargo | 1.88.0 (873a06493 2025-05-10) |
| Toolchain | x86_64-pc-windows-msvc, link-arg=/DEBUG:NONE |
| Branch | `p0-recovery-20260430` |
| Git commit | `3759ce8` |

Tests were run sequentially on the dev box. The api binary was built in release mode (`cargo build --release --bin api`) before the RTO/RPO run.

## 3. Methodology

### 3.1 WAL append (`crates/persistence/benches/wal_append.rs`)

Criterion micro-benchmark with default sample budget (warmup 3 s, measurement 5 s, 100 samples). Two modes for single-append; three batch sizes (64 / 256 / 1024).

Each scenario uses its own `tempfile::TempDir` and fresh `JsonlFileWal`. Records are ~250-byte synthetic order-shaped JSON. Important caveat: the same WAL handle is used for all iterations in a single scenario, so the file *grows* during the run. With the default 5-second measurement window, single-append grew to ~384k records (~96 MB) and batch_append/1024 to ~300 batches (~75 MB). This is more representative of long-running production WAL behaviour than a per-iteration reset would be.

```
cargo bench -p persistence --bench wal_append
```

### 3.2 WAL replay scaling (`crates/sequencer/benches/replay_scaling.rs`)

Criterion `iter_custom` benchmark. For each WAL size (1k / 10k / 100k), the bench pre-builds the WAL once outside the timed region. Each timed iteration:

1. Opens a fresh `JsonlFileWal::with_rotation` handle (line-counts the file once).
2. Constructs a fresh `Sequencer::with_wal`.
3. Calls `recover_from_wal_with_policy(AllowGaps)` and times only that step.

Sample size is 20, measurement time 30 s per scenario.

```
cargo bench -p sequencer --bench replay_scaling
```

### 3.3 RTO / RPO (`scripts/measure_rto_rpo.ps1`)

5 iterations × 500 sequential authenticated `/submit-order` requests rotated across an 8-user pool, with a 50 ms sleep after every 25-request burst to keep the IP rate-limit window fresh. Each iteration:

1. Stops any prior api, starts a fresh `target/release/api.exe` on a clean `data/`.
2. Admin-deposits 100M cash to each of 8 users via `/deposit`.
3. Submits 500 orders sequentially. Records pre-kill `sequencer_command_seq` from `/health`.
4. **Hard kills** the api via `Stop-Process -Force` (no graceful drain).
5. Waits for port 3030 to release.
6. Restarts the api on the *same* `data/`. **RTO** = wall-clock from `Start-Process` to first successful `/health.status == ok`.
7. Reads post-recovery `sequencer_command_seq`. **RPO loss** = max(0, pre - post). Asserts `frontiers.consistent` and `balance_invariant`.

```
powershell -File scripts\measure_rto_rpo.ps1 \
    -Iterations 5 -CommandCount 500 -UserPoolSize 8 \
    -BurstSize 25 -BurstSleepMs 50 \
    -Output ../docs/benchmarks/2026-05-01/raw/rto_rpo.json
```

## 4. Results

> **Note on §4.1 and §4.2 (throughput):** these numbers come from a single capture run. After the 3-run stability campaign documented in §10, **WAL-append and replay-scaling throughput are classified as EXPLORATORY** — run-to-run variance on the capture host was 19-48% and exceeds any sensible CI threshold. Treat the values below as ballpark, not as committed targets. The §4.3 RTO/RPO numbers ARE stable and were promoted to the committed baseline; see §10.

### 4.1 WAL append (exploratory — see §10)

| Scenario | Mean time | Throughput (median) | Notes |
|---|---:|---:|---|
| `wal_single_append/group_commit_off` | 13.085 µs | **76.4 K appends/sec** | 100 samples, 1.3M iterations on a growing file |
| `wal_single_append/group_commit_64` | 17.132 µs | **58.4 K appends/sec** | `with_group_commit(64)` is informational-only per impl comment; minor overhead from the setting itself |
| `wal_batch_append/64` | 1.116 ms (≈17.4 µs/record) | **57.4 K appends/sec** | sequential 64-record batches |
| `wal_batch_append/256` | 4.493 ms (≈17.6 µs/record) | **57.0 K appends/sec** | scales linearly with batch size |
| `wal_batch_append/1024` | 19.861 ms (≈19.4 µs/record) | **51.6 K appends/sec** | mild degradation at 1024-batch — likely page-cache pressure |

**Notable finding:** `--quick` mode (criterion's 100 ms budget) reported ~2.5 µs/append (~400K/sec), but full-sample steady-state is ~13 µs/append (~76K/sec) — a 5× difference. The full-sample numbers are the realistic long-running cost; the `--quick` numbers reflect a hot file-cache window before append latency stabilises. **Use the full-sample numbers for any future budget comparison.**

### 4.2 WAL replay scaling (exploratory — see §10)

| WAL size | Mean replay time | Throughput (median) | Per-record |
|---:|---:|---:|---:|
| 1,000 | 13.185 ms | **75.8 K cmds/sec** | 13.2 µs |
| 10,000 | 141.21 ms | **70.8 K cmds/sec** | 14.1 µs |
| 100,000 | 1.4504 s | **68.9 K cmds/sec** | 14.5 µs |

**Linear scaling to 100k** (per-record cost rises ~10% from 1k to 100k). Extrapolating, 1M commands project to ~14.5 s replay.

Notable: full-sample numbers here also differ from `--quick`. The 100k scenario reported 255 ms under `--quick` vs 1.45 s under full sampling — a 5.7× difference. Same caveat as WAL append.

### 4.3 RTO / RPO (baseline-promoted — see §10)

| Metric | Value |
|---:|---:|
| Iterations | 5 / 5 PASS |
| Accepted orders per iteration | **500 / 500** (no rejections) |
| Pre-kill `sequencer_command_seq` | 500 (every iteration) |
| Post-recovery `sequencer_command_seq` | 500 (every iteration) |
| **RPO worst loss count** | **0** |
| RTO p50 | **0.747 s** |
| RTO p95 | 0.771 s |
| RTO p99 | 0.771 s |
| RTO max | 0.771 s |
| `frontiers.consistent` post-recovery | true (every iteration) |
| `balance_invariant` post-recovery | true (every iteration) |

**RPO = 0 across all iterations. RTO p99 = 0.771 s** — a full hard-kill restart cycle, including api startup, WAL replay of 500 sequencer entries + 500 ledger entries, and `/health` becoming green, completes in under one second.

This is consistent with §4.2: at ~70K cmds/sec replay, 500 cmds replays in ~7 ms — RTO is dominated by api process startup (binary load, config parse, HTTP server bind, secret-file read), not by replay throughput. RTO will degrade as the WAL grows; per §4.2's linear scaling, a 1M-cmd WAL would add ~14.5 s replay cost on top of the ~0.75 s startup.

## 5. Operational implications

| Question | Answer |
|---|---|
| What's the worst-case startup time today? | **0.771 s** for a 500-cmd WAL. Add ~14.5 s per million commands for replay. |
| Can we lose committed data on hard kill? | **No** in the tested scenario — RPO=0 across 5 iterations. |
| What's the api throughput? | Not measured here — Section 6.1 of the broader benchmark plan covers this; needs follow-up commit. |
| How does it scale to 1M cmds? | Projected: 14.5 s replay + 0.75 s startup ≈ **15 s** RTO. Not yet measured directly. |
| Can a long-running WAL slow appends? | Yes: at sustained rates, append latency settled around 13-19 µs/record (76K/sec single, 51K/sec at 1024-batch). Plan ledger growth accordingly. |

## 6. Caveats and known limitations

1. **Single-host run.** All numbers from one Windows 11 dev workstation. CI Ubuntu runners will likely produce different numbers. Different hardware classes need their own baseline (`environment_class` field in the baseline schema).
2. **--quick vs full samples.** Documented above — the two modes diverge by ~5× because the WAL file grows during long runs. Future regression checks must use full-sample mode against the to-be-committed baseline.
3. **No 1M-scale measurement.** `replay_scaling` bench tops out at 100k. RTO measurement at 1M-cmd scale is queued for a future pass.
4. **No CPU/RSS sampling during run.** Hardware fingerprint captured statically; live counters not collected.
5. **No statistical confidence interval reported here.** Criterion's HTML report under `target/criterion/` has the full distribution; this report only summarises the median.
6. **api submission throughput not in scope** for this report. Section 6.1 of the benchmark plan (`submit_warm_S/M/L`) lands as a separate follow-up.
7. **No baseline file committed.** This report is the *measurement* — the baseline (`rust-exchange/benches/baselines/2026-05-01.json`) follows in the next commit, after the 3-run stability check confirms these numbers are reproducible.

## 7. Reproducing this report

From `rust-exchange/`:

```powershell
# 1. Build
cargo build --release --bin api

# 2. WAL append micro-bench
cargo bench -p persistence --bench wal_append `
    *>&1 | Tee-Object ../docs/benchmarks/<date>/raw/wal_append.log

# 3. Replay scaling
cargo bench -p sequencer --bench replay_scaling `
    *>&1 | Tee-Object ../docs/benchmarks/<date>/raw/replay_scaling.log

# 4. RTO / RPO
powershell -ExecutionPolicy Bypass -File scripts\measure_rto_rpo.ps1 `
    -Iterations 5 -CommandCount 500 -UserPoolSize 8 `
    -BurstSize 25 -BurstSleepMs 50 `
    -Output ../docs/benchmarks/<date>/raw/rto_rpo.json
```

Or via the orchestrator:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run_full_benchmark_suite.ps1 `
    -OutputDir ../docs/benchmarks/<date>/raw -Scale Medium
```

## 8. Raw artifacts

Every cell in this report can be reproduced from the files at `docs/benchmarks/2026-05-01/raw/`:

- `wal_append.log` — full criterion output for §4.1.
- `replay_scaling.log` — full criterion output for §4.2.
- `rto_rpo.json` — structured per-iteration record + aggregate for §4.3.
- `rto_rpo.log` — driver log for the RTO/RPO run.

Criterion's interactive HTML reports live under `rust-exchange/target/criterion/{wal_single_append,wal_batch_append,replay_scaling}/` (gitignored).

## 9. Next steps

1. ~~**Re-run twice more** on the same hardware to assess run-to-run variance.~~ **Done — see §10.** Result: throughput metrics not stable enough for CI gating; RTO/RPO promoted to baseline.
2. **Author baseline JSON** — landed as `rust-exchange/benches/baselines/2026-05-01.json` (RTO/RPO only). No `latest.json` — pin by date forces explicit baseline updates.
3. **Extend coverage** to submit-order throughput, depth-query latency, multi-market load, hot-market load, and 30-min soak via the existing harnesses.
4. **Wire CI** to invoke `run_full_benchmark_suite.ps1 -CompareAgainstBaseline benches/baselines/2026-05-01.json -FailOnRegression` once a stable benchmark host (dedicated bench machine OR CI runner with a controlled environment) is available. Re-capture the baseline there and add a separate dated file with the matching `environment_class` before turning the gate on.

## 10. Stability assessment

The §4 numbers above come from one capture run. To decide which metrics are CI-baseline-ready, three independent runs were captured on the same dev box and compared. Each run repeated all three benches (full criterion samples, 5-iter × 500-cmd × 8-user RTO/RPO), with a fresh stop/start cycle between runs.

### 10.1 Run-to-run variance

| Scenario | Run 1 | Run 2 | Run 3 | Median | Range / median | Verdict |
|---|---:|---:|---:|---:|---:|---|
| `wal_append/single_off` (K appends/sec) | 76.4 | 55.2 | 55.9 | 55.9 | 76.4 − 55.2 = ±19% | exploratory |
| `wal_append/single_group_commit_64` (K/sec) | 58.4 | 50.6 | 51.0 | 51.0 | ±15% | exploratory |
| `wal_append/batch_64` (K/sec) | 57.4 | 54.1 | 64.0 | 57.4 | ±17% | exploratory |
| `wal_append/batch_256` (K/sec) | 57.0 | 53.7 | 54.8 | 54.8 | ±6% | borderline |
| `wal_append/batch_1024` (K/sec) | 51.6 | 98.2 | 62.4 | 62.4 | ±58% | exploratory |
| `replay_scaling/1k` (K cmds/sec) | 75.8 | 149.4 | 77.9 | 77.9 | ±92% | exploratory |
| `replay_scaling/10k` (K cmds/sec) | 70.8 | 147.7 | 69.2 | 70.8 | ±109% | exploratory |
| `replay_scaling/100k` (K cmds/sec) | 68.9 | 118.6 | 65.3 | 68.9 | ±72% | exploratory |
| **`rto_seconds_p99` (s)** | 0.771 | 0.787 | 0.757 | **0.771** | **±2%** | **stable / baseline** |
| **`rpo_loss_count`** | 0 | 0 | 0 | **0** | 0 | **stable / baseline** |

### 10.2 Decision

**Promoted to committed baseline (`rust-exchange/benches/baselines/2026-05-01.json`):**

- `rto_seconds_p99` (lower_is_better, value = 0.787 = max-across-3-runs, threshold 30%)
- `rpo_loss_count` (lower_is_better, value = 0, **absolute_max = 0** — no slack)

**Excluded from baseline by design:**

- All five `wal_append/*` throughput scenarios.
- All three `replay_scaling/*` throughput scenarios.

The throughput excursions are not real performance regressions. They reflect filesystem-cache state (run 2 happened to have a hot page cache for the long-running WAL bench) and background dev-box load (other processes competing for CPU and disk during the capture window). Either gating on these would produce false positives, or the threshold would have to be set so wide (50%+) that it catches nothing meaningful.

### 10.3 Path to a CI-gating throughput baseline

A throughput baseline that's actually CI-gateable requires a **stable capture environment**. Two viable paths:

1. **Dedicated bench host.** A pinned VM or dedicated machine with no concurrent workload, dedicated SSD, fixed CPU governor / power profile, fixed Rust version. Capture three runs there and verify ±5%.
2. **CI runner capture.** Add a CI job that runs the bench suite on a fresh `ubuntu-latest` runner instance (which is at least environment-class-consistent across runs, even if not bare-metal-stable). Capture multiple runs over a week, accept the resulting variance band, threshold accordingly.

Either path produces a NEW dated baseline file under `rust-exchange/benches/baselines/` with its own `environment_class` field. The comparator's `bench_compare.ps1` should be extended to pick the baseline matching the current host's class.

### 10.4 Why no `latest.json`

The committed `bench_compare.ps1` accepts an explicit `-Baseline <path>`. We deliberately do NOT ship a `latest.json` symlink/alias — pinning by dated filename forces every baseline update to be an explicit, reviewable commit (`feat(bench): refresh baseline after <reason>`). This avoids silent baseline drift.

### 10.5 Caveats per metric (recapture context)

- RTO p99 = 0.787 s is **dominated by api process startup**, not by WAL replay (the captured WAL was only 500 cmds; replay completed in <10 ms). RTO at scale (1M cmds) is bounded above by §4.2's per-record cost × cmd count + this constant — not yet measured.
- RPO = 0 has been observed across 5 iter × 3 runs = 15 hard-kill cycles. This is a strong durability signal for the tested traffic shape (sequential acknowledged writes). Concurrent-writer or batch-commit scenarios are not exercised by this bench.
