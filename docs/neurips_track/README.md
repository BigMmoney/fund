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
- `FBA-100ms`: `1360.0 orders/s`, `p50 50 ms`, `p99 310 ms`
- `FBA-250ms`: `1358.4 orders/s`, `p50 80 ms`, `p99 490 ms`
- `FBA-500ms`: `1358.7 orders/s`, `p50 190 ms`, `p99 910 ms`
- `FBA-250ms-Stress`: `1775.2 orders/s`, `p50 100 ms`, `p99 590 ms`

All generated scenarios currently report:

- `0` negative-balance violations
- `0` conservation breaches

## Multi-Seed Experimental Snapshot

From `docs/benchmarks/simulator_multiseed_profile.json`, aggregated over seeds `[7, 11, 19, 23, 29, 31, 37, 41]`:

- `Immediate-Surrogate`: `1348.23 +/- 5.77 orders/s`, `p50 10.00 ms`, `p99 10.00 ms`
- `FBA-100ms`: `1348.23 +/- 5.77 orders/s`, `p50 46.25 ms`, `p99 146.25 ms`
- `FBA-250ms`: `1348.30 +/- 5.64 orders/s`, `p50 97.50 ms`, `p99 452.50 ms`
- `FBA-500ms`: `1347.75 +/- 4.70 orders/s`, `p50 213.75 ms`, `p99 835.00 ms`
- `FBA-250ms-Stress`: `1783.40 +/- 8.78 orders/s`, `p50 97.50 ms`, `p99 373.75 ms`

Measured observations:

- immediate execution keeps the lowest latency tail but also the widest quoted spread (`1.98`)
- the `100 ms` batch closes the spread to `1.00` while increasing mean arbitrage profit to `1015.75`
- the `250 ms` batch lowers mean queue-priority advantage to `0.0273`, below both immediate (`0.0742`) and `100 ms` batch (`0.0571`)
- the stress configuration raises throughput to `1783.40 orders/s` and arbitrage profit to `2057.00`

## Regeneration

```powershell
$env:RUN_SIM_BENCH='1'
go test ./simulator -run TestGenerateSimulatorBenchmarkArtifacts -v

$env:RUN_SIM_BENCH_MULTI='1'
go test ./simulator -run TestGenerateSimulatorMultiSeedArtifacts -v
```
