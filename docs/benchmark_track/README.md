# Benchmark Track

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
- `BENCHMARK_PAPER_MANUSCRIPT.md`: benchmark-oriented manuscript draft
- `arxiv/`: isolated LaTeX source for the benchmark-track paper

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

## Regeneration

```powershell
$env:RUN_SIM_BENCH='1'
go test ./simulator -run TestGenerateSimulatorBenchmarkArtifacts -v
```
