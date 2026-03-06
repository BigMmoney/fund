# Market Simulator Benchmark

This package adds a benchmark-oriented market simulator track on top of the existing repository.

Scope:

- immediate-clearing surrogate baseline
- fixed-delay speed-bump baseline
- frequent batch auction scenarios
- adaptive-window heuristic baselines, including balanced, order-flow, and queue-load variants
- adapter-driven policy baselines (`Policy-BurstAware-100-250ms`, `Policy-LearnedLinUCB-100-250ms`, `Policy-LearnedTinyMLP-100-250ms`)
- agent-based order flow
- ledger-aware settlement checks
- fairness and market-quality proxy metrics
- step-wise `Reset/Step/Observe/Metrics` API for downstream control loops
- minimal gym-style adapter via `NewAdapter`, with batch-window, risk-scale, tie-break, release-cadence, and price-aggression controls plus reward-bearing timesteps
- schema documentation in `docs/neurips_track/ENVIRONMENT_SCHEMA.md`
- mechanism ablations and agent/workload ablations
- parameter-grid sweeps over arbitrage intensity and maker quote width
- parameter-cube sweeps over retail intensity, informed intensity, and maker quote width

The current TinyMLP controller is no longer search-only. It uses a burst-aware supervised warm-start followed by gradient-based policy updates over the discrete adapter action bundle.

Key outputs:

- `docs/benchmarks/simulator_benchmark_profile.json`
- `docs/benchmarks/simulator_benchmark_profile.md`
- `docs/benchmarks/simulator_benchmark_profile.csv`
- `docs/benchmarks/simulator_multiseed_profile.*`
- `docs/benchmarks/simulator_ablation_profile.*`
- `docs/benchmarks/simulator_agent_ablation_profile.*`
- `docs/benchmarks/simulator_parameter_grid_profile.*`
- `docs/benchmarks/simulator_parameter_cube_profile.*`

To generate artifacts:

```powershell
$env:RUN_SIM_BENCH="1"
go test ./simulator -run TestGenerateSimulatorBenchmarkArtifacts -v

$env:RUN_SIM_BENCH_MULTI="1"
go test ./simulator -run TestGenerateSimulatorMultiSeedArtifacts -v

$env:RUN_SIM_ABLATION="1"
go test ./simulator -run TestGenerateSimulatorAblationArtifacts -v

$env:RUN_SIM_AGENT_ABLATION="1"
go test ./simulator -run TestGenerateSimulatorAgentAblationArtifacts -v

$env:RUN_SIM_GRID="1"
go test ./simulator -run TestGenerateSimulatorParameterGridArtifacts -v

$env:RUN_SIM_CUBE="1"
go test ./simulator -run TestGenerateSimulatorParameterCubeArtifacts -v

python scripts/generate_neurips_figures.py
```
