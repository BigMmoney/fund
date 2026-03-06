# A Ledger-Aware Benchmark for Market Infrastructure under Frequent Batch Auctions

## Abstract

We present a benchmark-oriented extension of a ledger-first market infrastructure prototype for studying market-design and execution behavior under settlement and risk constraints. The environment combines seedable agent-based order flow, configurable immediate and frequent-batch-auction matching regimes, double-entry style account state transitions, and deterministic safety checks for conservation and non-negativity. We expose reproducible benchmark tasks that compare latency, throughput, spread, price impact, queue-priority advantage, and latency-arbitrage profit across market designs. In the current implementation, all generated scenarios preserve settlement invariants, while batch-window changes induce clear latency and market-quality tradeoffs. This benchmark track is designed as a parallel research artifact and does not replace the existing systems-paper line.

## 1. Research Question

Can a ledger-aware benchmark environment make market-design tradeoffs measurable under realistic settlement constraints, instead of evaluating matching mechanisms in isolation?

The current artifact focuses on three concrete questions:

1. How do immediate and batch execution regimes differ in latency and fill behavior?
2. Can fairness-adjacent proxies such as queue-priority advantage and arbitrage profit be measured in a reproducible environment?
3. Do settlement invariants remain intact while the environment is stressed with heterogeneous agents and burstier order flow?

## 2. Environment Design

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

## 3. Tasks and Baselines

The current benchmark suite exposes five scenarios:

1. `Immediate-Surrogate`
2. `FBA-100ms`
3. `FBA-250ms`
4. `FBA-500ms`
5. `FBA-250ms-Stress`

These are not yet a complete NeurIPS-scale benchmark suite, but they are sufficient to establish a reproducible baseline for future agent-control or adaptive-window work.

## 4. Metrics

The current artifact reports four groups of signals.

### 4.1 Systems Metrics

- orders per second
- fills per second
- p50, p95, and p99 latency

### 4.2 Market Quality Metrics

- average spread
- average price impact

### 4.3 Fairness Proxies

- queue-priority advantage
- latency-arbitrage profit
- execution dispersion across agent classes

### 4.4 Safety Metrics

- negative-balance violations
- conservation breaches
- risk rejections

## 5. Current Results

We report two layers of results:

1. a single-seed reproducibility snapshot in `docs/benchmarks/simulator_benchmark_profile.*`
2. an eight-seed aggregate profile in `docs/benchmarks/simulator_multiseed_profile.*`

The multi-seed profile uses seeds `7, 11, 19, 23, 29, 31, 37, 41` and gives a more defensible paper-facing summary than a single deterministic seed.

### 5.1 Multi-Seed Summary

| Scenario | Runs | Mean Orders/s | Mean Fills/s | Mean p50 (ms) | Mean p95 (ms) | Mean p99 (ms) | Mean Spread | Mean Impact | Mean Queue Adv. | Mean Arb Profit |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Immediate-Surrogate | 8 | 1348.23 | 813.12 | 10.00 | 10.00 | 10.00 | 1.98 | 3.38 | 0.0742 | 669.62 |
| FBA-100ms | 8 | 1348.23 | 805.31 | 46.25 | 100.00 | 146.25 | 1.00 | 5.96 | 0.0571 | 1015.75 |
| FBA-250ms | 8 | 1348.30 | 691.90 | 97.50 | 300.00 | 452.50 | 1.00 | 5.36 | 0.0273 | 627.25 |
| FBA-500ms | 8 | 1347.75 | 631.08 | 213.75 | 505.00 | 835.00 | 1.00 | 5.08 | 0.0341 | 827.62 |
| FBA-250ms-Stress | 8 | 1783.40 | 907.70 | 97.50 | 248.75 | 373.75 | 1.00 | 5.35 | 0.0269 | 2057.00 |

### 5.2 Observations

- Immediate execution retains the lowest latency profile, with `10 ms` mean p50, p95, and p99 under the current discrete-time simulator.
- Moving to `100 ms` batches preserves mean order throughput while increasing mean p99 latency to `146.25 ms`.
- The `250 ms` batch regime materially reduces mean queue-priority advantage to `0.0273`, compared with `0.0742` for immediate execution and `0.0571` for `100 ms` batches.
- The `500 ms` batch regime pushes mean p50 latency to `213.75 ms` and mean p99 latency to `835.00 ms`, making the latency cost explicit.
- The stress scenario increases throughput to `1783.40 orders/s` and fill throughput to `907.70 fills/s`, but also lifts mean arbitrage-profit proxy to `2057.00`.
- Across all `5 x 8 = 40` measured runs, the simulator reports `0` negative-balance violations and `0` conservation breaches.

These results are sufficient for a benchmark-track draft, but not yet sufficient for a top-tier ML benchmark submission. The missing pieces are:

- richer baselines such as speed-bump or adaptive-window policies
- more explicit fairness metrics
- multi-seed aggregate reporting
- agent-based market-behavior studies beyond proxy metrics

## 6. Positioning

This benchmark-track manuscript should be treated as a separate line from the existing systems-paper manuscript in `docs/PAPER_MANUSCRIPT.md`. The systems paper argues for a ledger-first market-infrastructure design. This benchmark paper argues for a reusable evaluation environment built on top of those same settlement constraints.

## 7. Next Upgrade Path

To push this track toward a stronger benchmark paper:

1. add adaptive-window and speed-bump baselines
2. add multi-seed aggregate evaluation and confidence intervals
3. add explicit agent-behavior experiments for queue advantage and arbitrage capture
4. package the simulator behind a cleaner `reset/step/observe/metrics` API
