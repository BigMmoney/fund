# NeurIPS Track

This directory is a parallel paper track. It does not replace the existing `docs/arxiv/` manuscript.

## Purpose

The original paper line is a market-infrastructure systems paper. This track upgrades the repo toward a benchmark/simulator paper with:

- a seedable agent-based market simulator
- multiple market-design regimes
- a step-wise `Reset/Step/Observe/Metrics` API
- ledger-aware settlement semantics
- reproducible benchmark artifacts
- targeted ablation outputs

## New Components

- `simulator/`: benchmark environment, agent models, metrics, step API, and tests
- `simulator/adapter.go`: minimal gym-style adapter with batch-window, risk-scale, and tie-break controls plus reward-bearing timesteps
- `docs/benchmarks/simulator_benchmark_profile.*`: generated experiment outputs
- `docs/benchmarks/simulator_multiseed_profile.*`: multi-seed aggregate outputs
- `docs/benchmarks/simulator_ablation_profile.*`: ablation outputs
- `NEURIPS_BENCHMARK_MANUSCRIPT.md`: benchmark-oriented manuscript draft
- `arxiv/`: isolated LaTeX source for the NeurIPS-track paper

## Current Benchmark Snapshot

From `docs/benchmarks/simulator_benchmark_profile.json`:

- `Immediate-Surrogate`: `1360.0 orders/s`, `p50 10 ms`, `p99 10 ms`
- `SpeedBump-50ms`: `1305.6 orders/s`, `p50 60 ms`, `p99 60 ms`
- `FBA-100ms`: `1348.8 orders/s`, `p50 50 ms`, `p99 170 ms`
- `FBA-250ms`: `1347.6 orders/s`, `p50 80 ms`, `p99 490 ms`
- `FBA-500ms`: `1349.7 orders/s`, `p50 190 ms`, `p99 910 ms`
- `Adaptive-100-250ms`: `1347.6 orders/s`, `p50 80 ms`, `p99 430 ms`
- `Adaptive-OrderFlow-100-250ms`: `1347.6 orders/s`, `p50 90 ms`, `p99 430 ms`
- `Adaptive-QueueLoad-100-250ms`: `1347.6 orders/s`, `p50 90 ms`, `p99 400 ms`
- `Policy-BurstAware-100-250ms`: `1340.5 orders/s`, `p50 80 ms`, `p99 450 ms`
- `FBA-250ms-Stress`: `1761.1 orders/s`, `p50 100 ms`, `p99 590 ms`

All generated scenarios currently report:

- `0` negative-balance violations
- `0` conservation breaches

## Multi-Seed Experimental Snapshot

From `docs/benchmarks/simulator_multiseed_profile.json`, aggregated over seeds `[7, 11, 19, 23, 29, 31, 37, 41]` and reported as `mean +/- CI95`:

- `Immediate-Surrogate`: `1348.23 +/- 3.99 orders/s`, `p50 10.00 +/- 0.00 ms`, `p99 10.00 +/- 0.00 ms`
- `SpeedBump-50ms`: `1294.30 +/- 3.84 orders/s`, `p50 60.00 +/- 0.00 ms`, `p99 60.00 +/- 0.00 ms`
- `FBA-100ms`: `1337.09 +/- 3.96 orders/s`, `p50 46.25 +/- 3.35 ms`, `p99 146.25 +/- 35.83 ms`
- `FBA-250ms`: `1337.60 +/- 3.88 orders/s`, `p50 97.50 +/- 4.58 ms`, `p99 452.50 +/- 16.16 ms`
- `FBA-500ms`: `1338.82 +/- 3.24 orders/s`, `p50 213.75 +/- 24.48 ms`, `p99 835.00 +/- 84.37 ms`
- `Adaptive-100-250ms`: `1337.60 +/- 3.88 orders/s`, `adaptive mean window 207.14 ms`, `p99 360.00 +/- 69.38 ms`
- `Adaptive-OrderFlow-100-250ms`: `1337.60 +/- 3.88 orders/s`, `adaptive mean window 216.67 ms`, `p99 406.25 +/- 46.22 ms`
- `Adaptive-QueueLoad-100-250ms`: `1337.60 +/- 3.88 orders/s`, `adaptive mean window 209.29 ms`, `p99 386.25 +/- 65.09 ms`
- `Policy-BurstAware-100-250ms`: `1337.70 +/- 2.82 orders/s`, `policy mean window 250.00 ms`, `p99 406.25 +/- 57.03 ms`
- `FBA-250ms-Stress`: `1769.25 +/- 6.04 orders/s`, `p50 97.50 +/- 5.75 ms`, `p99 373.75 +/- 70.24 ms`

Measured observations:

- immediate execution keeps the lowest latency tail but also the widest quoted spread (`1.98`)
- the `50 ms` speed-bump baseline lands between immediate and batched regimes on latency (`60 ms`) but keeps the immediate-style queue-advantage proxy (`0.0742 +/- 0.0078`)
- the `100 ms` batch closes the spread to `1.00 +/- 0.00` while increasing mean arbitrage profit to `1015.75 +/- 44.06`
- the `250 ms` batch lowers mean queue-priority advantage to `0.0273 +/- 0.0182`, below both immediate (`0.0742 +/- 0.0078`) and the speed-bump baseline (`0.0742 +/- 0.0078`)
- the adaptive heuristic settles around a `207.14 ms` mean window and reduces arbitrage-profit proxy to `522.00 +/- 86.23`, below both `FBA-100ms` and `FBA-250ms`
- the order-flow adaptive variant pushes to a larger `216.67 ms` mean window, lowers queue advantage to `0.0244 +/- 0.0209`, but gives back arbitrage-profit performance versus the balanced adaptive baseline
- the queue-load adaptive variant settles at `209.29 ms`, improves price impact to `4.88 +/- 0.59`, and keeps a lower p99 than the order-flow adaptive variant
- the stress configuration raises throughput to `1769.25 orders/s` and arbitrage profit to `2057.00`

## Step API

The simulator now exposes a step-wise control surface:

- `Reset()`
- `Observe()`
- `Step()`
- `Metrics()`

This keeps the benchmark runnable as a batch artifact while also letting future work wrap the environment as a control or RL-style loop.

The repository now includes a minimal adapter layer:

- `NewAdapter(cfg)` for scenario-backed environment construction
- `Reset()` returning observation, metrics, reward, done, and info
- `Step(action)` with batch-window, risk-scale, and tie-break control hooks
- `ActionSpec()` advertising which controls a scenario supports

The current policy baseline is adapter-driven rather than hard-wired inside `Environment.Run()`:

- `Policy-BurstAware-100-250ms`
  - uses `Adapter.Step(action)`
  - controls batch window, risk scale, and tie-break mode
  - produces a distinct benchmark line in the generated artifacts

## Ablation Snapshot

From `docs/benchmarks/simulator_ablation_profile.json` over seeds `[13, 17, 19, 23]`:

- `Ablation-Control`: `1190.48 orders/s`, `2922` risk rejections
- `Ablation-RelaxedRisk`: `1770.24 orders/s`, `0` risk rejections
- `Ablation-RandomTieBreak`: `p99 402.50 ms`, worse tail than control `320.00 ms`
- `Ablation-NoSettlementChecks`: no throughput gain over control, so safety checks are not the dominant bottleneck in this setup

## Visualizations

Generated from `docs/benchmarks/simulator_multiseed_profile.json`:

![Throughput comparison](figures/throughput.svg)

![Latency profile](figures/latency.svg)

![Fairness proxy comparison](figures/fairness.svg)

## Regeneration

```powershell
$env:RUN_SIM_BENCH='1'
go test ./simulator -run TestGenerateSimulatorBenchmarkArtifacts -v

$env:RUN_SIM_BENCH_MULTI='1'
go test ./simulator -run TestGenerateSimulatorMultiSeedArtifacts -v

$env:RUN_SIM_ABLATION='1'
go test ./simulator -run TestGenerateSimulatorAblationArtifacts -v
```
