# NeurIPS Track

This directory is a parallel paper track. It does not replace the existing `docs/arxiv/` manuscript.

## Purpose

The original paper line is a market-infrastructure systems paper. This track upgrades the repo toward a benchmark/simulator paper with:

- a seedable agent-based market simulator
- multiple market-design regimes
- a step-wise `Reset/Step/Observe/Metrics` API
- adapter-driven control baselines
- an expanded control surface: batch window, risk scale, tie-break mode, release cadence, and price aggression
- ledger-aware settlement semantics
- reproducible benchmark artifacts
- mechanism and agent/workload sweeps

## New Components

- `simulator/`: benchmark environment, agent models, metrics, step API, and tests
- `simulator/adapter.go`: minimal gym-style adapter with five runtime control channels and reward-bearing timesteps
- `docs/benchmarks/simulator_benchmark_profile.*`: generated single-seed outputs
- `docs/benchmarks/simulator_multiseed_profile.*`: multi-seed aggregate outputs
- `docs/benchmarks/simulator_ablation_profile.*`: mechanism ablation outputs
- `docs/benchmarks/simulator_agent_ablation_profile.*`: agent and workload sweep outputs
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
- `Policy-BurstAware-100-250ms`: `1343.7 orders/s`, `p50 80 ms`, `p99 440 ms`
- `Policy-LearnedBandit-100-250ms`: `1347.6 orders/s`, `p50 50 ms`, `p99 180 ms`
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
- `Policy-BurstAware-100-250ms`: `1338.89 +/- 2.97 orders/s`, `policy mean window 250.00 ms`, `p99 400.00 +/- 57.25 ms`
- `Policy-LearnedBandit-100-250ms`: `1337.60 +/- 3.88 orders/s`, `policy mean window 100.00 ms`, `p99 155.00 +/- 14.70 ms`
- `FBA-250ms-Stress`: `1769.25 +/- 6.04 orders/s`, `p50 97.50 +/- 5.75 ms`, `p99 373.75 +/- 70.24 ms`

Measured observations:

- immediate execution keeps the lowest latency tail but also the widest quoted spread (`1.98`)
- the `50 ms` speed-bump baseline lands between immediate and batched regimes on latency (`60 ms`) but keeps the immediate-style queue-advantage proxy (`0.0742 +/- 0.0078`)
- the `250 ms` batch lowers mean queue-priority advantage to `0.0273 +/- 0.0182`, below both immediate and speed-bump baselines
- the balanced adaptive heuristic settles around a `207.14 ms` mean window and reduces arbitrage-profit proxy to `522.00 +/- 86.23`
- the burst-aware policy drives the adapter to the slow end of the window range (`250 ms`) and improves fairness-adjacent proxies relative to the learned bandit
- the learned bandit drives the adapter to a fast release profile (`100 ms` mean window), improving fills (`743.06 +/- 24.50`) and tail latency (`p99 155.00 +/- 14.70 ms`), but worsening queue-advantage and arbitrage-profit proxies (`0.0423`, `896.50`)
- the stress configuration raises throughput to `1769.25 orders/s` and arbitrage profit to `2057.00`

## Step API

The simulator now exposes a step-wise control surface:

- `Reset()`
- `Observe()`
- `Step()`
- `Metrics()`

The repository also includes an adapter layer:

- `NewAdapter(cfg)` for scenario-backed environment construction
- `Reset()` returning observation, metrics, reward, done, and info
- `Step(action)` with:
  - batch-window control
  - risk-scale control
  - tie-break toggle
  - release-cadence control
  - price-aggression bias
- `ActionSpec()` advertising which controls a scenario supports

Current policy baselines:

- `Policy-BurstAware-100-250ms`
  - hand-written controller over the expanded adapter action space
  - settles on slower windows and better fairness-adjacent proxy scores
- `Policy-LearnedBandit-100-250ms`
  - contextual bandit over discrete action bundles
  - optimizes toward stronger latency/fill outcomes, not fairness-adjacent proxy minima

## Ablation and Sweep Snapshot

From `docs/benchmarks/simulator_ablation_profile.json` over seeds `[13, 17, 19, 23]`:

- `Ablation-RelaxedRisk`: `1770.24 orders/s`, `0` risk rejections
- `Ablation-RandomTieBreak`: `p99 402.50 ms`, worse tail than control `320.00 ms`
- `Ablation-NoSettlementChecks`: no throughput gain over control

From `docs/benchmarks/simulator_agent_ablation_profile.json` over seeds `[43, 47, 53, 59]`:

- `AgentAblation-NoArbitrageurs`: drives arbitrage-profit proxy to `0.00`
- `AgentSweep-RetailBurst`: pushes throughput to `2532.94 orders/s`
- `AgentSweep-ArbIntensityHigh`: raises arbitrage-profit proxy to `1730.75`
- `AgentSweep-MakersWide`: pushes p99 out to `485.00 ms`

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

$env:RUN_SIM_AGENT_ABLATION='1'
go test ./simulator -run TestGenerateSimulatorAgentAblationArtifacts -v
```
