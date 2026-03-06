# A Ledger-Aware Benchmark for Market Infrastructure under Frequent Batch Auctions

## Abstract

We present a benchmark-oriented extension of a ledger-first market infrastructure prototype for studying market-design and execution behavior under settlement and risk constraints. The environment combines seedable agent-based order flow, configurable immediate, speed-bump, and frequent-batch-auction matching regimes, double-entry style account state transitions, and deterministic safety checks for conservation and non-negativity. In addition to throughput, latency, spread, price impact, queue-priority advantage, and arbitrage-profit proxies, the benchmark now reports welfare and behavior signals including retail surplus per traded unit, retail adverse-selection rate, welfare dispersion, and surplus-transfer gap. The current version exposes a step-wise `Reset/Step/Observe/Metrics` API, an adapter-driven control surface, four policy baselines including an offline contextual controller, and a unified four-dimensional stress sweep over arbitrage intensity, retail intensity, informed intensity, and maker quote width. Across all measured runs, settlement invariants remain intact while mechanism and controller choices induce clear latency, fairness, and welfare tradeoffs.

## 1. Introduction

Electronic markets are shaped jointly by matching rules, latency structure, and settlement semantics. Frequent batch auction arguments motivate short batching windows as a response to latency-driven distortions in continuous markets. In practice, however, evaluation artifacts often study matching rules in isolation and do not explicitly couple them to settlement safety, replay behavior, or account-state correctness.

This manuscript defines a benchmark-oriented layer on top of an existing ledger-first market-infrastructure prototype. The goal is not to replace the original systems paper. The goal is to establish a reusable evaluation environment in which mechanism comparisons and controller experiments are made under explicit settlement constraints and richer welfare measurements.

The current benchmark line makes five concrete contributions:

1. It defines a ledger-aware benchmark environment in which market-design experiments are coupled to deterministic settlement checks.
2. It provides a seedable multi-agent order-flow generator with four agent classes, thirteen benchmark scenarios, explicit agent/workload sweeps, and a unified four-dimensional stress surface over arbitrage, retail, informed, and maker-width multipliers.
3. It reports both single-seed and eight-seed aggregate benchmark outputs over throughput, latency, spread, price impact, queue-priority advantage, arbitrage-profit proxy, and welfare/behavior metrics.
4. It exposes a step-wise `Reset/Step/Observe/Metrics` API, a gym-style adapter with five runtime control channels, and four adapter-driven controllers, including a stronger offline contextual policy baseline.
5. It preserves executable settlement invariants across all published artifacts, so controller improvements are evaluated under non-negativity and conservation constraints rather than in a matching-only simulator.

## 2. Research Question

Can a ledger-aware benchmark environment make market-design and controller tradeoffs measurable under realistic settlement constraints, instead of evaluating matching mechanisms in isolation?

The current artifact focuses on four concrete questions:

1. How do immediate, speed-bump, adaptive, and fixed-batch execution regimes differ in latency and fill behavior?
2. Can fairness-adjacent proxies such as queue-priority advantage and arbitrage profit be measured in a reproducible environment?
3. Do welfare/behavior metrics such as retail surplus and adverse selection reveal controller tradeoffs that proxy metrics alone miss?
4. Do settlement invariants remain intact while the environment is stressed with heterogeneous agents, burstier order flow, and higher-dimensional workload sweeps?

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

The present artifact is benchmark-first rather than method-first. It provides a structured set of metrics and a controller interaction surface that later policy-learning work can optimize against.

## 4. Environment Design

The benchmark environment lives in `simulator/` and consists of:

- `types.go`: scenario, order, fill, and result schemas
- `agents.go`: market-maker, retail, informed, and latency-arbitrageur order generation
- `matching.go`: immediate, speed-bump, adaptive, and batch-clearing execution paths
- `env.go`: state progression, settlement application, and invariant checks
- `metrics.go`: market-quality, fairness-proxy, and welfare/behavior measurements
- `adapter.go`: reward-bearing timesteps, control actions, and learned-controller baselines
- `benchmark_test.go`: deterministic regression and artifact generation

The environment exposes:

- `Reset()`
- `Observe()`
- `Step()`
- `Metrics()`

On top of that control surface, the repository now includes an adapter layer with five explicit runtime control channels:

- batch-window override for adaptive scenarios
- risk-limit scaling
- tie-break randomization
- release-cadence control
- price-aggression bias

Benchmark results are only accepted if generated scenarios preserve:

- conservation of cash and inventory
- non-negative account balances and units
- deterministic results for the same seed

## 5. Agent Models

The environment includes four agent classes:

- market makers
- latency arbitrageurs
- retail traders
- informed traders

These agents are intentionally simple. They are not intended as a full behavioral model of real markets. Their purpose is to create heterogeneous and controllable order flow so that matching rules and controller choices can be compared under a repeatable workload.

## 6. Tasks and Baselines

The current benchmark suite exposes thirteen scenarios:

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
12. `Policy-LearnedOfflineContextual-100-250ms`
13. `FBA-250ms-Stress`

The controller suite now contains:

- a hand-written burst-aware baseline
- a contextual linear bandit baseline
- a gradient-trained TinyMLP baseline
- an offline contextual controller trained from logged trajectories generated by burst-aware, LinUCB, TinyMLP, and random behavior policies

The discrete action bundle shared by the learned controllers includes:

- batch window
- risk scale
- tie-break mode
- release cadence
- price aggression

## 7. Metrics

The artifact reports five groups of signals.

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

### 7.4 Welfare / Behavior Metrics

- retail surplus per traded unit
- arbitrageur surplus per traded unit
- retail adverse-selection rate
- welfare dispersion
- surplus-transfer gap

These metrics are computed directly from realized fills against the synthetic fundamental and are not simple rescalings of queue or arbitrage proxies.

### 7.5 Safety Metrics

- negative-balance violations
- conservation breaches
- risk rejections

## 8. Experimental Setup

We report two result layers:

1. a single-seed reproducibility snapshot
2. an eight-seed aggregate profile

The aggregate profile uses seeds:

`[7, 11, 19, 23, 29, 31, 37, 41]`

The mechanism ablation suite uses `[13, 17, 19, 23]`. The agent/workload sweep uses `[43, 47, 53, 59]`. The parameter grid uses `[61, 67, 71, 73]`. The parameter cube uses `[79, 83, 89, 97]`. The unified hypercube uses `[101, 103, 107, 109]`.

All reported scenarios use the same simulator implementation and differ only in matching mode, batch-window size, seed, and population intensity. The current setup uses a discrete-time step duration of `10 ms`.

## 9. Current Results

We report two layers of results:

1. a single-seed reproducibility snapshot in `docs/benchmarks/simulator_benchmark_profile.*`
2. an eight-seed aggregate profile in `docs/benchmarks/simulator_multiseed_profile.*`, reported as `mean +/- CI95`

### 9.1 Multi-Seed Summary

| Scenario | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Immediate-Surrogate | 1348.23 +/- 3.99 | 813.13 +/- 18.47 | 10.00 +/- 0.00 | 3.38 +/- 0.23 | 0.0742 +/- 0.0078 | 669.63 +/- 34.30 | -0.3710 +/- 0.1264 | 0.5109 +/- 0.0183 | 2.0430 +/- 0.2444 |
| FBA-250ms | 1337.60 +/- 3.88 | 686.41 +/- 17.80 | 452.50 +/- 16.16 | 5.36 +/- 0.60 | 0.0273 +/- 0.0182 | 627.25 +/- 107.67 | -0.3493 +/- 0.1951 | 0.5105 +/- 0.0268 | 0.8896 +/- 0.8264 |
| Adaptive-100-250ms | 1337.60 +/- 3.88 | 714.29 +/- 22.20 | 360.00 +/- 69.38 | 4.71 +/- 0.49 | 0.0375 +/- 0.0165 | 522.00 +/- 86.23 | -0.1508 +/- 0.1950 | 0.4562 +/- 0.0682 | 0.0278 +/- 0.6078 |
| Policy-BurstAware-100-250ms | 1338.89 +/- 2.97 | 670.83 +/- 28.54 | 400.00 +/- 57.25 | 5.31 +/- 0.52 | 0.0305 +/- 0.0214 | 621.00 +/- 94.21 | -0.2258 +/- 0.2212 | 0.5019 +/- 0.0203 | 0.7579 +/- 0.7681 |
| Policy-LearnedLinUCB-100-250ms | 1337.60 +/- 3.88 | 755.65 +/- 27.48 | 155.00 +/- 17.32 | 5.90 +/- 0.37 | 0.0513 +/- 0.0254 | 976.63 +/- 68.56 | 0.0795 +/- 0.1353 | 0.4690 +/- 0.0231 | 2.1694 +/- 0.7433 |
| Policy-LearnedTinyMLP-100-250ms | 1337.60 +/- 3.88 | 769.35 +/- 20.85 | 221.25 +/- 57.40 | 5.22 +/- 0.41 | 0.0336 +/- 0.0256 | 856.13 +/- 107.16 | -0.3128 +/- 0.1535 | 0.4924 +/- 0.0238 | 1.9719 +/- 0.8498 |
| Policy-LearnedOfflineContextual-100-250ms | 1337.40 +/- 3.91 | 762.80 +/- 36.22 | 215.00 +/- 47.25 | 4.94 +/- 0.57 | 0.0294 +/- 0.0156 | 771.25 +/- 113.73 | -0.1090 +/- 0.1191 | 0.4980 +/- 0.0237 | 1.3769 +/- 0.8055 |
| FBA-250ms-Stress | 1769.25 +/- 6.04 | 900.50 +/- 23.67 | 373.75 +/- 70.24 | 5.35 +/- 0.51 | 0.0269 +/- 0.0272 | 2057.00 +/- 235.64 | -0.1494 +/- 0.1905 | 0.4995 +/- 0.0234 | 0.3805 +/- 0.8714 |

### 9.2 Observations

- Immediate execution retains the lowest latency profile, but it also carries the widest spread (`1.98 +/- 0.17`) and a high welfare gap (`2.0430 +/- 0.2444`).
- The `50 ms` speed-bump baseline raises latency without reducing queue advantage; it keeps `0.0742 +/- 0.0078` queue advantage and worsens welfare gap to `4.1034 +/- 0.3793`.
- The fixed `250 ms` batch regime lowers queue advantage to `0.0273 +/- 0.0182` and compresses welfare gap relative to immediate execution, but it gives up substantial tail latency.
- The balanced adaptive heuristic is the best fixed logic in the current repo on combined welfare terms: it keeps price impact low (`4.71 +/- 0.49`), reduces arbitrage-profit proxy to `522.00 +/- 86.23`, and nearly closes the welfare gap (`0.0278 +/- 0.6078`).
- `Policy-LearnedLinUCB-100-250ms` is still best on tail latency (`155.00 +/- 17.32 ms`) and strong on fill throughput, but it does so by widening queue advantage and surplus-transfer gap.
- `Policy-LearnedTinyMLP-100-250ms` improves fills further, but leaves both retail surplus and welfare gap worse than the offline contextual baseline.
- `Policy-LearnedOfflineContextual-100-250ms` is the strongest balanced learned baseline in the current repo: compared with `LinUCB`, it gives up some tail latency but cuts price impact (`4.94` versus `5.90`), lowers queue advantage (`0.0294` versus `0.0513`), lowers arbitrage-profit proxy (`771.25` versus `976.63`), and shrinks the welfare gap (`1.3769` versus `2.1694`).

### 9.3 Visual Summary

![Throughput comparison](figures/throughput.svg)

![Latency profile](figures/latency.svg)

![Fairness proxy comparison](figures/fairness.svg)

![Welfare and behavior comparison](figures/welfare.svg)

![Mechanism ablation snapshot](figures/ablation.svg)

![Agent and workload sweep snapshot](figures/agent_sweeps.svg)

![Parameter-grid p99 heatmap](figures/grid_p99_heatmap.svg)

![Parameter-grid arbitrage heatmap](figures/grid_arb_heatmap.svg)

The full appendix figure set, including cube and hypercube slices, is collected in `APPENDIX_FIGURES.md`.

### 9.4 Mechanism Ablation Snapshot

From `docs/benchmarks/simulator_ablation_profile.*` over seeds `[13, 17, 19, 23]`:

| Scenario | Orders/s | Fills/s | p99 (ms) | Queue Adv. | Arb Profit | Risk Rejects | Safety Violations |
|---|---:|---:|---:|---:|---:|---:|---:|
| Ablation-Control | 1190.48 | 605.36 | 320.00 | -0.0509 | 685.25 | 2922 | 0 |
| Ablation-RelaxedRisk | 1770.24 | 925.79 | 302.50 | 0.0372 | 2198.50 | 0 | 0 |
| Ablation-RandomTieBreak | 1190.48 | 595.63 | 402.50 | -0.0540 | 738.50 | 2908 | 0 |
| Ablation-NoSettlementChecks | 1190.48 | 605.36 | 320.00 | -0.0509 | 685.25 | 2922 | 0 |

These ablations show that relaxing risk limits is not a free lunch: it raises throughput, but also sharply increases arbitrage capture.

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

The sweeps make clear that the benchmark is sensitive to population composition and workload intensity, not only to the matching rule.

### 9.6 Parameter Grid, Cube, and Unified Hypercube

The benchmark now exposes three sensitivity surfaces:

- a `4 x 3` arbitrage x maker grid
- a `3 x 3 x 3` retail x informed x maker cube
- a unified `4 x 3 x 3 x 3` hypercube over arbitrage, retail, informed, and maker width

Selected hypercube cells from `docs/benchmarks/simulator_parameter_hypercube_profile.*` over seeds `[101, 103, 107, 109]`:

| Arb | Retail | Informed | Maker Width | Orders/s | Fills/s | p99 (ms) | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 1 | 2 | 1 | 1350.60 | 676.39 | 412.50 | 0.00 | -0.2376 | 0.4963 | 0.2376 |
| 3 | 1 | 2 | 1 | 1542.86 | 806.94 | 362.50 | 1982.00 | -0.5444 | 0.4823 | 1.2839 |
| 0 | 3 | 2 | 1 | 2147.02 | 1105.75 | 397.50 | 0.00 | -0.1335 | 0.4819 | 0.1335 |
| 3 | 3 | 2 | 3 | 2291.67 | 1005.95 | 402.50 | 2285.50 | -0.4598 | 0.5434 | 1.8005 |

These cells show that higher throughput does not imply better welfare. Adding arbitrage pressure widens the surplus-transfer gap even when p99 falls, while high retail intensity alone mainly loads throughput and fills.

## 10. Limitations

The current artifact is stronger than the earlier benchmark draft, but it is still short of a mature top-tier benchmark submission. The most important limitations are:

- the learned-controller family is still lightweight and discrete-action
- welfare metrics are derived from a synthetic fundamental rather than real market replay
- the hypercube is slice-visualized rather than fully summarized in one compact figure family
- the agent behaviors are stylized and do not yet include richer strategic adaptation

## 11. Related Work

This NeurIPS-track manuscript should be treated as a separate line from the existing systems-paper manuscript in `docs/PAPER_MANUSCRIPT.md` and the original arXiv sources in `docs/arxiv/`. The systems paper argues for a ledger-first market-infrastructure design. This benchmark paper argues for a reusable evaluation environment built on top of the same settlement constraints.

The benchmark is motivated directly by frequent-batch-auction market design and by the broader practice of reusable benchmark environments in machine learning. The present contribution sits between those traditions: it is neither a pure market-design theory paper nor a standard reinforcement-learning benchmark, but an infrastructure-aware environment that aims to make later policy-learning work more credible.

## 12. Conclusion

This NeurIPS-track benchmark line now supports a more defensible benchmark narrative than the earlier scaffold. It preserves settlement invariants, exposes a reusable interaction API, adds a stronger offline contextual baseline, expands fairness measurement into welfare/behavior signals, and merges arbitrage pressure into a unified higher-dimensional stress surface. The current results show a clear and useful tradeoff: controllers that optimize aggressively for latency and fills widen the surplus-transfer gap, while more balanced controllers can give up some tail performance to materially improve market-quality and welfare outcomes.

## 13. Next Upgrade Path

To push this track further:

1. replace the current offline contextual value model with a stronger offline-RL or compact policy-network training loop on the same action space
2. add richer agent behavior and adaptation beyond the current stylized generators
3. expand the hypercube into broader appendix-level sensitivity plots and summary statistics
