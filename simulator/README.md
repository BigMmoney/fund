# Market Simulator Benchmark

This package adds a benchmark-oriented market simulator track on top of the existing repository.

Scope:

- immediate-clearing surrogate baseline
- fixed-delay speed-bump baseline
- frequent batch auction scenarios
- adaptive-window heuristic baselines, including balanced, order-flow, and queue-load variants
- agent-based order flow
- ledger-aware settlement checks
- fairness and market-quality proxy metrics
- step-wise `Reset/Step/Observe/Metrics` API for downstream control loops

Key outputs:

- `docs/benchmarks/simulator_benchmark_profile.json`
- `docs/benchmarks/simulator_benchmark_profile.md`
- `docs/benchmarks/simulator_benchmark_profile.csv`

To generate artifacts:

```powershell
$env:RUN_SIM_BENCH="1"
go test ./simulator -run TestGenerateSimulatorBenchmarkArtifacts -v

$env:RUN_SIM_BENCH_MULTI="1"
go test ./simulator -run TestGenerateSimulatorMultiSeedArtifacts -v

$env:RUN_SIM_ABLATION="1"
go test ./simulator -run TestGenerateSimulatorAblationArtifacts -v
```
