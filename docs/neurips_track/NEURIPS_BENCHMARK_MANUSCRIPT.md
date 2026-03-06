# A Ledger-Aware Benchmark for Market Infrastructure under Frequent Batch Auctions

## Abstract

We present a benchmark-oriented extension of a ledger-first market infrastructure prototype for studying market-design and execution behavior under settlement and risk constraints. The environment combines seedable agent-based order flow, configurable immediate, speed-bump, and frequent-batch-auction matching regimes, double-entry style account state transitions, and deterministic safety checks for conservation and non-negativity. We expose reproducible benchmark tasks that compare latency, throughput, spread, price impact, queue-priority advantage, and latency-arbitrage profit across market designs. In the current implementation, all generated scenarios preserve settlement invariants, while mechanism changes induce clear latency and market-quality tradeoffs. The result is a benchmark draft for market-infrastructure research that is separate from the original systems-paper line and is intended as a foundation for stronger benchmark-style evaluation.

## 1. Introduction

Electronic markets are shaped jointly by matching rules, latency structure, and settlement semantics. Frequent batch auction arguments motivate short batching windows as a response to latency-driven distortions in continuous markets. In practice, however, many evaluation artifacts study matching rules in isolation and do not explicitly couple them to settlement safety, replay behavior, or account-state correctness.

This manuscript defines a benchmark-oriented layer on top of an existing ledger-first market-infrastructure prototype. The goal is not to replace the original systems paper. The goal is to establish a reusable evaluation environment in which mechanism comparisons are made under explicit settlement constraints.

The current benchmark line makes four concrete contributions:

1. It defines a ledger-aware benchmark environment in which market-design experiments are coupled to deterministic settlement checks.
2. It provides a seedable multi-agent order-flow generator with four agent classes, twelve benchmark scenarios, explicit agent/workload sweeps, a full parameter grid over arbitrage intensity and maker quote width, and a three-dimensional parameter cube over retail intensity, informed intensity, and maker quote width.
3. It reports both single-seed and eight-seed aggregate benchmark outputs over throughput, latency, spread, price impact, queue-priority advantage, and arbitrage-profit proxies.
4. It exposes a step-wise `Reset/Step/Observe/Metrics` API, a gym-style adapter with five runtime control channels, three adapter-driven policy baselines, documented observation/action/metrics schemas, and ablation suites for both market-structure toggles and agent/workload perturbations.

## 2. Research Question

Can a ledger-aware benchmark environment make market-design tradeoffs measurable under realistic settlement constraints, instead of evaluating matching mechanisms in isolation?

The current artifact focuses on three concrete questions:

1. How do immediate, speed-bump, adaptive, and batch execution regimes differ in latency and fill behavior?
2. Can fairness-adjacent proxies such as queue-priority advantage and arbitrage profit be measured in a reproducible environment?
3. Do settlement invariants remain intact while the environment is stressed with heterogeneous agents and burstier order flow?

## 3. Problem Setting

We study a discrete-time environment in which the market state evolves step by step. At each step, the state consists of:

- the buy book
- the sell book
- the vector of account states
- the latent synthetic fundamental used by the order-flow generator

Each scenario fixes:

- a matching mode
- a batch-window size
- risk thresholds
- a population of agents

The current artifact is not a learning benchmark in the narrow sense of supplying a reward function for a trained policy. Instead, it produces a structured set of metrics that later work can optimize against.

## 4. Environment Design

The benchmark environment lives in `simulator/` and consists of:

- `types.go`: scenario, order, fill, and result schemas
- `agents.go`: market-maker, retail, informed, and latency-arbitrageur order generation
- `matching.go`: immediate, speed-bump, adaptive, and batch-clearing execution paths
- `env.go`: state progression, settlement application, and invariant checks
- `metrics.go`: market-quality and fairness-proxy measurements
- `benchmark_test.go`: deterministic regression and artifact generation

The environment also exposes a step-wise control surface:

- `Reset()`
- `Observe()`
- `Step()`
- `Metrics()`

On top of that control surface, the repository now includes an adapter layer that returns reward-bearing timesteps and advertises per-scenario action support. In the current version, the adapter exposes five explicit control channels:

- batch-window override for adaptive scenarios
- risk-limit scaling
- tie-break randomization
- release-cadence control
- price-aggression bias

The design keeps ledger correctness in the loop. Benchmark results are only accepted if generated scenarios preserve:

- conservation of cash and inventory
- non-negative account balances and units
- deterministic results for the same seed

## 5. Agent Models

The current environment includes four agent classes:

- market makers
- latency arbitrageurs
- retail traders
- informed traders

These agents are deliberately simple. They are not intended to be a full behavioral model of real markets. Their purpose is to create heterogeneous and controllable order flow so that matching rules can be compared under a repeatable workload.

## 6. Tasks and Baselines

The current benchmark suite exposes twelve scenarios:

1. `Immediate-Surrogate`
2. `SpeedBump-50ms`
3. `FBA-100ms`
4. `FBA-250ms`
5. `FBA-500ms`
6. `Adaptive-100-250ms`
7. `Adaptive-OrderFlow-100-250ms`
8. `Adaptive-QueueLoad-100-250ms`
9. `Policy-BurstAware-100-250ms`
10. `Policy-LearnedLinUCB-100-250ms`
11. `Policy-LearnedTinyMLP-100-250ms`
12. `FBA-250ms-Stress`

These are not yet a complete NeurIPS-scale benchmark suite, but they are sufficient to establish a reproducible baseline for future agent-control or adaptive-window work.

The benchmark now includes three heuristic adaptive-window baselines and three adapter-driven policy controllers. The burst-aware controller is hand-written. The learned baselines are a contextual linear bandit and a small two-layer policy network with burst-aware supervised warm-start and gradient-based policy updates over the same discrete action bundle set.

## 7. Metrics

The current artifact reports four groups of signals.

### 7.1 Systems Metrics

- orders per second
- fills per second
- p50, p95, and p99 latency

### 7.2 Market Quality Metrics

- average spread
- average price impact

### 7.3 Fairness Proxies

- queue-priority advantage
- latency-arbitrage profit
- execution dispersion across agent classes

### 7.4 Safety Metrics

- negative-balance violations
- conservation breaches
- risk rejections

## 8. Experimental Setup

We report two result layers:

1. a single-seed reproducibility snapshot
2. an eight-seed aggregate profile

The aggregate profile uses seeds:

`[7, 11, 19, 23, 29, 31, 37, 41]`

All reported scenarios use the same simulator implementation and differ only in matching mode, batch-window size, seed, and population intensity. The current setup uses a discrete-time step duration of `10 ms`.

The repository also materializes three figure assets for the current aggregate profile:

- `docs/neurips_track/figures/throughput.svg`
- `docs/neurips_track/figures/latency.svg`
- `docs/neurips_track/figures/fairness.svg`

## 9. Current Results

We report two layers of results:

1. a single-seed reproducibility snapshot in `docs/benchmarks/simulator_benchmark_profile.*`
2. an eight-seed aggregate profile in `docs/benchmarks/simulator_multiseed_profile.*`, reported as `mean +/- CI95`

### 9.1 Multi-Seed Summary

| Scenario | Runs | Orders/s (mean +/- CI95) | Fills/s (mean +/- CI95) | p50 (mean +/- CI95) | p95 (mean +/- CI95) | p99 (mean +/- CI95) | Spread (mean +/- CI95) | Impact (mean +/- CI95) | Queue Adv. (mean +/- CI95) | Arb Profit (mean +/- CI95) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Immediate-Surrogate | 8 | 1348.23 +/- 3.99 | 813.12 +/- 18.47 | 10.00 +/- 0.00 | 10.00 +/- 0.00 | 10.00 +/- 0.00 | 1.98 +/- 0.17 | 3.38 +/- 0.23 | 0.0742 +/- 0.0078 | 669.62 +/- 34.30 |
| SpeedBump-50ms | 8 | 1294.30 +/- 3.84 | 780.60 +/- 17.73 | 60.00 +/- 0.00 | 60.00 +/- 0.00 | 60.00 +/- 0.00 | 1.96 +/- 0.17 | 5.96 +/- 0.22 | 0.0742 +/- 0.0078 | 1346.25 +/- 59.17 |
| FBA-100ms | 8 | 1337.09 +/- 3.96 | 798.66 +/- 21.71 | 46.25 +/- 3.35 | 100.00 +/- 0.00 | 146.25 +/- 35.83 | 1.00 +/- 0.00 | 5.96 +/- 0.41 | 0.0571 +/- 0.0219 | 1015.75 +/- 44.06 |
| FBA-250ms | 8 | 1337.60 +/- 3.88 | 686.41 +/- 17.80 | 97.50 +/- 4.58 | 300.00 +/- 56.08 | 452.50 +/- 16.16 | 1.00 +/- 0.00 | 5.36 +/- 0.60 | 0.0273 +/- 0.0182 | 627.25 +/- 107.67 |
| FBA-500ms | 8 | 1338.82 +/- 3.24 | 626.90 +/- 19.72 | 213.75 +/- 24.48 | 505.00 +/- 30.40 | 835.00 +/- 84.37 | 1.00 +/- 0.00 | 5.08 +/- 1.17 | 0.0341 +/- 0.0191 | 827.62 +/- 294.22 |
| Adaptive-100-250ms | 8 | 1337.60 +/- 3.88 | 714.29 +/- 22.20 | 81.25 +/- 6.42 | 236.25 +/- 10.92 | 360.00 +/- 69.38 | 1.00 +/- 0.00 | 4.71 +/- 0.49 | 0.0375 +/- 0.0165 | 522.00 +/- 86.23 |
| Adaptive-OrderFlow-100-250ms | 8 | 1337.60 +/- 3.88 | 671.43 +/- 15.81 | 90.00 +/- 4.90 | 247.50 +/- 6.71 | 406.25 +/- 46.22 | 1.00 +/- 0.00 | 5.93 +/- 0.42 | 0.0244 +/- 0.0209 | 746.75 +/- 115.27 |
| Adaptive-QueueLoad-100-250ms | 8 | 1337.60 +/- 3.88 | 691.57 +/- 27.02 | 90.00 +/- 7.75 | 261.25 +/- 44.83 | 386.25 +/- 65.09 | 1.00 +/- 0.00 | 4.88 +/- 0.59 | 0.0375 +/- 0.0239 | 624.25 +/- 56.37 |
| Policy-BurstAware-100-250ms | 8 | 1338.89 +/- 2.97 | 670.83 +/- 28.54 | 98.75 +/- 4.15 | 263.75 +/- 47.63 | 400.00 +/- 57.25 | 1.00 +/- 0.00 | 5.31 +/- 0.52 | 0.0305 +/- 0.0214 | 621.00 +/- 94.21 |
| Policy-LearnedLinUCB-100-250ms | 8 | 1337.60 +/- 3.88 | 745.24 +/- 34.21 | 47.50 +/- 3.00 | 101.25 +/- 2.29 | 158.75 +/- 15.66 | 1.00 +/- 0.00 | 5.94 +/- 0.40 | 0.0447 +/- 0.0223 | 959.62 +/- 63.59 |
| Policy-LearnedTinyMLP-100-250ms | 8 | 1338.39 +/- 2.62 | 689.78 +/- 28.40 | 58.75 +/- 2.29 | 220.00 +/- 37.32 | 305.00 +/- 49.00 | 1.00 +/- 0.00 | 8.86 +/- 0.51 | 0.0278 +/- 0.0214 | 1229.00 +/- 108.83 |
| FBA-250ms-Stress | 8 | 1769.25 +/- 6.04 | 900.50 +/- 23.67 | 97.50 +/- 5.75 | 248.75 +/- 21.76 | 373.75 +/- 70.24 | 1.00 +/- 0.00 | 5.35 +/- 0.51 | 0.0269 +/- 0.0272 | 2057.00 +/- 235.64 |

### 9.2 Observations

- Immediate execution retains the lowest latency profile, with `10 ms` mean p50, p95, and p99 under the current discrete-time simulator.
- The `50 ms` speed-bump baseline introduces a fixed `60 ms` latency profile and lower throughput (`1294.30 +/- 3.84 orders/s`) while keeping the immediate-style queue-priority-advantage proxy (`0.0742 +/- 0.0078`).
- The `250 ms` batch regime materially reduces mean queue-priority advantage to `0.0273 +/- 0.0182`, compared with `0.0742 +/- 0.0078` for immediate execution and the speed-bump baseline, and `0.0571 +/- 0.0219` for `100 ms` batches.
- The balanced adaptive heuristic settles around a `207.14 ms` mean window and reduces arbitrage-profit proxy to `522.00 +/- 86.23`, below both `FBA-100ms` and `FBA-250ms`.
- The burst-aware policy controller saturates the mean window at `250.00 ms`, yielding lower queue-priority advantage (`0.0305 +/- 0.0214`) and lower arbitrage-profit proxy (`621.00 +/- 94.21`) than the learned controller, but at much higher latency tails.
- The learned LinUCB controller collapses to a fast control policy (`100.00 ms` mean window), improving fills (`745.24 +/- 34.21`) and reducing p99 to `158.75 +/- 15.66 ms`, but worsening queue-priority advantage (`0.0447 +/- 0.0223`) and arbitrage-profit proxy (`959.62 +/- 63.59`) relative to the burst-aware controller.
- The gradient-trained TinyMLP controller settles at a slower `200.00 ms` mean window, slightly improves fills over burst-aware (`689.78 +/- 28.40` versus `670.83 +/- 28.54`), and reduces p99 to `305.00 +/- 49.00 ms`, but it also pays materially higher price impact (`8.86 +/- 0.51`) and arbitrage-profit proxy (`1229.00 +/- 108.83`) than either burst-aware or LinUCB.
- The stress scenario increases throughput to `1769.25 +/- 6.04 orders/s` and fill throughput to `900.50 +/- 23.67 fills/s`, but also lifts mean arbitrage-profit proxy to `2057.00 +/- 235.64`.
- Across all `12 x 8 = 96` measured runs, the simulator reports `0` negative-balance violations and `0` conservation breaches.

### 9.3 Visual Summary

![Throughput comparison](figures/throughput.svg)

![Latency profile](figures/latency.svg)

![Fairness proxy comparison](figures/fairness.svg)

![Mechanism ablation snapshot](figures/ablation.svg)

![Agent and workload sweep snapshot](figures/agent_sweeps.svg)

![Parameter-grid p99 heatmap](figures/grid_p99_heatmap.svg)

![Parameter-grid arbitrage heatmap](figures/grid_arb_heatmap.svg)

The full appendix figure set, including the three-dimensional cube slices, is collected in `APPENDIX_FIGURES.md`.

### 9.4 Mechanism Ablation Snapshot

From `docs/benchmarks/simulator_ablation_profile.*` over seeds `[13, 17, 19, 23]`:

| Scenario | Orders/s | Fills/s | p99 (ms) | Queue Adv. | Arb Profit | Risk Rejects | Safety Violations |
|---|---:|---:|---:|---:|---:|---:|---:|
| Ablation-Control | 1190.48 | 605.36 | 320.00 | -0.0509 | 685.25 | 2922 | 0 |
| Ablation-RelaxedRisk | 1770.24 | 925.79 | 302.50 | 0.0372 | 2198.50 | 0 | 0 |
| Ablation-RandomTieBreak | 1190.48 | 595.63 | 402.50 | -0.0540 | 738.50 | 2908 | 0 |
| Ablation-NoSettlementChecks | 1190.48 | 605.36 | 320.00 | -0.0509 | 685.25 | 2922 | 0 |

These ablations show three concrete effects:

- relaxing risk limits sharply increases throughput and removes rejections, but it also lifts the arbitrage-profit proxy
- randomizing tie-breaks worsens the latency tail and slightly reduces fill throughput under the stressed constrained setup
- disabling settlement checks does not improve throughput in this setup, so invariant enforcement is not the dominant bottleneck

### 9.5 Agent and Workload Sweep Snapshot

From `docs/benchmarks/simulator_agent_ablation_profile.*` over seeds `[43, 47, 53, 59]`:

| Scenario | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit |
|---|---:|---:|---:|---:|---:|---:|
| AgentAblation-Control | 1343.25 | 659.52 | 367.50 | 5.49 | 0.0408 | 754.00 |
| AgentAblation-NoArbitrageurs | 1276.59 | 622.62 | 367.50 | 5.38 | -0.5958 | 0.00 |
| AgentAblation-NoInformed | 1257.14 | 622.62 | 335.00 | 5.54 | 0.0260 | 750.25 |
| AgentSweep-RetailBurst | 2532.94 | 1427.18 | 467.50 | 5.70 | 0.0404 | 821.75 |
| AgentSweep-ArbIntensityHigh | 1409.92 | 718.45 | 302.50 | 5.80 | 0.0314 | 1730.75 |
| AgentSweep-InformedIntensityHigh | 1426.79 | 724.01 | 360.00 | 5.62 | 0.0111 | 685.50 |
| AgentSweep-MakersWide | 1343.25 | 595.63 | 485.00 | 5.81 | 0.0469 | 754.50 |

These sweeps make the benchmark more than a mechanism toggle suite:

- removing arbitrageurs collapses arbitrage-profit proxy to `0.00` and flips queue advantage negative
- doubling arbitrage intensity raises arbitrage-profit proxy to `1730.75` while reducing p99 to `302.50 ms`
- widening maker quotes pushes p99 to `485.00 ms` and reduces fills to `595.63`
- increasing informed intensity raises throughput modestly while keeping arbitrage-profit proxy much closer to control than the arbitrage sweep

### 9.6 Parameter Grid Snapshot

From `docs/benchmarks/simulator_parameter_grid_profile.*` over seeds `[61, 67, 71, 73]`, the benchmark now exposes a full `4 x 3` parameter grid:

- arbitrageur intensity multiplier `{0, 1, 2, 3}`
- maker quote-width multiplier `{1, 2, 3}`

This grid shows three concrete effects:

- removing arbitrageurs collapses the arbitrage-profit proxy to `0.00` across the entire first row and flips queue advantage strongly negative
- increasing arbitrage intensity from `1` to `3` consistently raises throughput and arbitrage-profit proxy, reaching `2151.00 +/- 392.11` in the `(arb=3, maker=3)` cell
- wider maker quotes raise p99 tails and suppress fills, especially once arbitrage pressure is already elevated

### 9.7 Parameter Cube Snapshot

From `docs/benchmarks/simulator_parameter_cube_profile.*` over seeds `[79, 83, 89, 97]`, the benchmark now also exposes a full `3 x 3 x 3` cube over:

- retail intensity multiplier `{1, 2, 3}`
- informed intensity multiplier `{1, 2, 3}`
- maker quote-width multiplier `{1, 2, 3}`

This cube shows three useful effects:

- retail intensity is the strongest throughput lever in the current cube, pushing orders/s from `1339.68` in `(retail=1, informed=1, maker=1)` to `2132.54` in `(retail=3, informed=1, maker=1)`
- informed intensity adds fills on top of retail pressure, reaching `2292.86 orders/s` and `1159.92 fills/s` in `(retail=3, informed=3, maker=1)`
- wider maker quotes systematically suppress fills inside each retail slice; for example, under `(retail=3, informed=3)`, fills fall from `1159.92` at `maker=1` to `956.75` at `maker=3`

## 10. Limitations

The current artifact is still short of a strong top-tier benchmark submission. The most important limitations are:

- proxy fairness metrics rather than richer behavioral or welfare metrics
- the adapter action space is still narrow compared with a full exchange-control environment
- the strongest learned controller is still a small gradient-trained policy network rather than a richer offline-RL or larger policy-learning setup
- the current benchmark separates arbitrage-intensity sweeps from the retail / informed / maker cube instead of exposing a unified higher-dimensional stress surface

## 11. Related Work

This NeurIPS-track manuscript should be treated as a separate line from the existing systems-paper manuscript in `docs/PAPER_MANUSCRIPT.md` and the original arXiv sources in `docs/arxiv/`. The systems paper argues for a ledger-first market-infrastructure design. This benchmark paper argues for a reusable evaluation environment built on top of those same settlement constraints.

The benchmark is motivated by frequent-batch-auction market design and by the broader practice of reusable benchmark environments in machine learning. The present contribution sits between those two traditions: it is neither a pure market-design theory paper nor a standard reinforcement-learning benchmark, but an infrastructure-aware evaluation layer.

## 12. Conclusion

This NeurIPS-track benchmark line establishes a reproducible, ledger-aware evaluation environment for market-design experiments without overwriting the original systems-paper line. The current results already show measurable latency and market-quality tradeoffs across immediate, speed-bump, adaptive, adapter-driven policy, and batch matching regimes, while preserving executable settlement invariants.

## 13. Next Upgrade Path

To push this track toward a stronger benchmark paper:

1. replace the current TinyMLP baseline with a stronger learned controller on the same action space
2. add explicit agent-behavior experiments for queue advantage and arbitrage capture
3. broaden the policy interface beyond batch window, risk scale, release cadence, price aggression, and tie-break toggles
4. merge arbitrage intensity into the higher-dimensional sweep suite and keep expanding appendix-level confidence-interval plots
