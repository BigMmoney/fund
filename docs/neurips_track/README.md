# NeurIPS Track

This directory is a parallel paper track. It does not replace the existing `docs/arxiv/` manuscript.

## Purpose

The original paper line is a market-infrastructure systems paper. This track upgrades the repo toward a benchmark/simulator paper with:

- a seedable agent-based market simulator
- multiple market-design regimes
- ledger-aware settlement semantics
- reproducible benchmark artifacts

## New Components

- `simulator/`: benchmark environment, agent models, metrics, and tests
- `docs/benchmarks/simulator_benchmark_profile.*`: generated experiment outputs
- `docs/benchmarks/simulator_multiseed_profile.*`: multi-seed aggregate outputs
- `NEURIPS_BENCHMARK_MANUSCRIPT.md`: benchmark-oriented manuscript draft
- `arxiv/`: isolated LaTeX source for the NeurIPS-track paper

## Current Benchmark Snapshot

From `docs/benchmarks/simulator_benchmark_profile.json`:

- `Immediate-Surrogate`: `1360.0 orders/s`, `p50 10 ms`, `p99 10 ms`
- `SpeedBump-50ms`: `1305.6 orders/s`, `p50 60 ms`, `p99 60 ms`
- `FBA-100ms`: `1360.0 orders/s`, `p50 50 ms`, `p99 310 ms`
- `FBA-250ms`: `1358.4 orders/s`, `p50 80 ms`, `p99 490 ms`
- `FBA-500ms`: `1358.7 orders/s`, `p50 190 ms`, `p99 910 ms`
- `FBA-250ms-Stress`: `1775.2 orders/s`, `p50 100 ms`, `p99 590 ms`

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
- `FBA-250ms-Stress`: `1769.25 +/- 6.04 orders/s`, `p50 97.50 +/- 5.75 ms`, `p99 373.75 +/- 70.24 ms`

Measured observations:

- immediate execution keeps the lowest latency tail but also the widest quoted spread (`1.98`)
- the `50 ms` speed-bump baseline lands between immediate and batched regimes on latency (`60 ms`) but keeps the immediate-style queue-advantage proxy (`0.0742 +/- 0.0078`)
- the `100 ms` batch closes the spread to `1.00 +/- 0.00` while increasing mean arbitrage profit to `1015.75 +/- 44.06`
- the `250 ms` batch lowers mean queue-priority advantage to `0.0273 +/- 0.0182`, below both immediate (`0.0742 +/- 0.0078`) and the speed-bump baseline (`0.0742 +/- 0.0078`)
- the stress configuration raises throughput to `1769.25 orders/s` and arbitrage profit to `2057.00`

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
```
