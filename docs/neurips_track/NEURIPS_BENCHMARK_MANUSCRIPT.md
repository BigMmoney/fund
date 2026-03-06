# A Ledger-Aware Benchmark for Market Infrastructure under Frequent Batch Auctions

## Abstract

We present a benchmark-oriented extension of a ledger-first market infrastructure prototype for studying market-design and execution behavior under settlement and risk constraints. The environment combines seedable agent-based order flow, configurable immediate and frequent-batch-auction matching regimes, double-entry style account state transitions, and deterministic safety checks for conservation and non-negativity. We expose reproducible benchmark tasks that compare latency, throughput, spread, price impact, queue-priority advantage, and latency-arbitrage profit across market designs. In the current implementation, all generated scenarios preserve settlement invariants, while batch-window changes induce clear latency and market-quality tradeoffs. The result is a benchmark draft for market-infrastructure research that is separate from the original systems-paper line and is intended as a foundation for stronger benchmark-style evaluation.

## 1. Introduction

Electronic markets are shaped jointly by matching rules, latency structure, and settlement semantics. Frequent batch auction arguments motivate short batching windows as a response to latency-driven distortions in continuous markets. In practice, however, many evaluation artifacts study matching rules in isolation and do not explicitly couple them to settlement safety, replay behavior, or account-state correctness.

This manuscript defines a benchmark-oriented layer on top of an existing ledger-first market-infrastructure prototype. The goal is not to replace the original systems paper. The goal is to establish a reusable evaluation environment in which mechanism comparisons are made under explicit settlement constraints.

The current benchmark line makes four concrete contributions:

1. It defines a ledger-aware benchmark environment in which market-design experiments are coupled to deterministic settlement checks.
2. It provides a seedable multi-agent order-flow generator with four agent classes and five benchmark scenarios.
3. It reports both single-seed and eight-seed aggregate benchmark outputs over throughput, latency, spread, price impact, queue-priority advantage, and arbitrage-profit proxies.
4. It documents a direct upgrade path toward a stronger benchmark-style paper.

## 2. Research Question

Can a ledger-aware benchmark environment make market-design tradeoffs measurable under realistic settlement constraints, instead of evaluating matching mechanisms in isolation?

The current artifact focuses on three concrete questions:

1. How do immediate and batch execution regimes differ in latency and fill behavior?
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
- `matching.go`: immediate and batch-clearing execution paths
- `env.go`: state progression, settlement application, and invariant checks
- `metrics.go`: market-quality and fairness-proxy measurements
- `benchmark_test.go`: deterministic regression and artifact generation

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

These agents are deliberately simple. They are not intended to be a full behavioral model of real markets. Their purpose is to create heterogeneous and controllable order flow so that matching rules can be compared under a repeatable workload. The stress scenario extends the default population with additional retail and arbitrage activity to increase intensity.

## 6. Tasks and Baselines

The current benchmark suite exposes five scenarios:

1. `Immediate-Surrogate`
2. `FBA-100ms`
3. `FBA-250ms`
4. `FBA-500ms`
5. `FBA-250ms-Stress`

These are not yet a complete NeurIPS-scale benchmark suite, but they are sufficient to establish a reproducible baseline for future agent-control or adaptive-window work.

The current benchmark still lacks two important baselines:

- a speed-bump baseline
- an adaptive-window heuristic baseline

Those two are the most important next additions if this track is going to become a stronger benchmark paper.

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

The multi-seed profile uses seeds `7, 11, 19, 23, 29, 31, 37, 41` and gives a more defensible paper-facing summary than a single deterministic seed.

### 9.1 Multi-Seed Summary

| Scenario | Runs | Orders/s (mean +/- CI95) | Fills/s (mean +/- CI95) | p50 (mean +/- CI95) | p95 (mean +/- CI95) | p99 (mean +/- CI95) | Spread (mean +/- CI95) | Impact (mean +/- CI95) | Queue Adv. (mean +/- CI95) | Arb Profit (mean +/- CI95) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Immediate-Surrogate | 8 | 1348.23 +/- 3.99 | 813.12 +/- 18.47 | 10.00 +/- 0.00 | 10.00 +/- 0.00 | 10.00 +/- 0.00 | 1.98 +/- 0.17 | 3.38 +/- 0.23 | 0.0742 +/- 0.0078 | 669.62 +/- 34.30 |
| FBA-100ms | 8 | 1348.23 +/- 3.99 | 805.31 +/- 21.89 | 46.25 +/- 3.35 | 100.00 +/- 0.00 | 146.25 +/- 35.83 | 1.00 +/- 0.00 | 5.96 +/- 0.41 | 0.0571 +/- 0.0219 | 1015.75 +/- 44.06 |
| FBA-250ms | 8 | 1348.30 +/- 3.91 | 691.90 +/- 17.94 | 97.50 +/- 4.58 | 300.00 +/- 56.08 | 452.50 +/- 16.16 | 1.00 +/- 0.00 | 5.36 +/- 0.60 | 0.0273 +/- 0.0182 | 627.25 +/- 107.67 |
| FBA-500ms | 8 | 1347.75 +/- 3.26 | 631.08 +/- 19.85 | 213.75 +/- 24.48 | 505.00 +/- 30.40 | 835.00 +/- 84.37 | 1.00 +/- 0.00 | 5.08 +/- 1.17 | 0.0341 +/- 0.0191 | 827.62 +/- 294.22 |
| FBA-250ms-Stress | 8 | 1783.40 +/- 6.08 | 907.70 +/- 23.86 | 97.50 +/- 5.75 | 248.75 +/- 21.76 | 373.75 +/- 70.24 | 1.00 +/- 0.00 | 5.35 +/- 0.51 | 0.0269 +/- 0.0272 | 2057.00 +/- 235.64 |

### 9.2 Observations

- Immediate execution retains the lowest latency profile, with `10 ms` mean p50, p95, and p99 under the current discrete-time simulator.
- Moving to `100 ms` batches preserves mean order throughput within a narrow confidence band while increasing mean p99 latency to `146.25 +/- 35.83 ms`.
- The `250 ms` batch regime materially reduces mean queue-priority advantage to `0.0273 +/- 0.0182`, compared with `0.0742 +/- 0.0078` for immediate execution and `0.0571 +/- 0.0219` for `100 ms` batches.
- The `500 ms` batch regime pushes mean p50 latency to `213.75 +/- 24.48 ms` and mean p99 latency to `835.00 +/- 84.37 ms`, making the latency cost explicit.
- The stress scenario increases throughput to `1783.40 +/- 6.08 orders/s` and fill throughput to `907.70 +/- 23.86 fills/s`, but also lifts mean arbitrage-profit proxy to `2057.00 +/- 235.64`.
- Across all `5 x 8 = 40` measured runs, the simulator reports `0` negative-balance violations and `0` conservation breaches.

### 9.3 Visual Summary

![Throughput comparison](figures/throughput.svg)

![Latency profile](figures/latency.svg)

![Fairness proxy comparison](figures/fairness.svg)

## 10. Limitations

The current artifact is still short of a strong top-tier benchmark submission. The most important limitations are:

- fixed policies only, with no learned or adaptive baselines
- no confidence intervals or error-bar figures yet
- proxy fairness metrics rather than richer behavioral or welfare metrics
- no standardized `reset/step/observe/metrics` API for downstream learning agents
- no ablation study on tie-breaking, risk thresholds, or settlement checks

## 11. Related Work

This NeurIPS-track manuscript should be treated as a separate line from the existing systems-paper manuscript in `docs/PAPER_MANUSCRIPT.md` and the original arXiv sources in `docs/arxiv/`. The systems paper argues for a ledger-first market-infrastructure design. This benchmark paper argues for a reusable evaluation environment built on top of those same settlement constraints.

The benchmark is motivated by frequent-batch-auction market design and by the broader practice of reusable benchmark environments in machine learning. The present contribution sits between those two traditions: it is neither a pure market-design theory paper nor a standard reinforcement-learning benchmark, but an infrastructure-aware evaluation layer.

## 12. Conclusion

This NeurIPS-track benchmark line establishes a reproducible, ledger-aware evaluation environment for market-design experiments without overwriting the original systems-paper line. The current results already show measurable latency and market-quality tradeoffs across immediate and batch matching regimes, while preserving executable settlement invariants.

## 13. Next Upgrade Path

To push this track toward a stronger benchmark paper:

1. add adaptive-window and speed-bump baselines
2. add confidence intervals and error-bar figures
3. add explicit agent-behavior experiments for queue advantage and arbitrage capture
4. package the simulator behind a cleaner `reset/step/observe/metrics` API
