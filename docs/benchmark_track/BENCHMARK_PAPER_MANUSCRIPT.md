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

The generated profile in `docs/benchmarks/simulator_benchmark_profile.md` shows:

- immediate execution preserves the lowest median latency (`10 ms`)
- wider batch windows increase latency tails (`p99 310 ms` at `100 ms`, `910 ms` at `500 ms`)
- stress scenarios raise throughput (`1775.2 orders/s`) and fill throughput (`987.2 fills/s`)
- all current scenarios remain settlement-safe (`0` negative-balance violations, `0` conservation breaches)

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
