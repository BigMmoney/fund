# Market Simulator Benchmark

This package adds a benchmark-oriented market simulator track on top of the existing repository.

Scope:

- immediate-clearing surrogate baseline
- frequent batch auction scenarios
- agent-based order flow
- ledger-aware settlement checks
- fairness and market-quality proxy metrics

Key outputs:

- `docs/benchmarks/simulator_benchmark_profile.json`
- `docs/benchmarks/simulator_benchmark_profile.md`
- `docs/benchmarks/simulator_benchmark_profile.csv`

To generate artifacts:

```powershell
$env:RUN_SIM_BENCH="1"
go test ./simulator -run TestGenerateSimulatorBenchmarkArtifacts -v
```
