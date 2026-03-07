# NeurIPS Track

This directory is a parallel benchmark-paper line. It does not replace the original systems-paper material in `docs/PAPER_MANUSCRIPT.md` or `docs/arxiv/`.

The scope of this paper line is intentionally narrow. It is centered on one benchmark question: how mechanism and controller choices trade off latency/fills against retail outcomes and transfer-to-arbitrageur under explicit settlement constraints.

## Purpose

This track upgrades the repo toward a benchmark/simulator paper with:

- a seedable agent-based market simulator
- multiple market-design regimes
- a step-wise `Reset/Step/Observe/Metrics` API
- a gym-style adapter with runtime controls for batch window, risk scale, tie-break mode, release cadence, and price aggression
- adapter-driven policy baselines, including an offline contextual controller
- ledger-aware settlement semantics and explicit invariant checks
- reproducible benchmark artifacts, sweeps, and appendix figures
- a paper-facing welfare decomposition built around `retail_surplus_per_unit`, `retail_adverse_selection_rate`, and `surplus_transfer_gap`

## Key Components

- `simulator/`: environment, agent models, metrics, adapter, and benchmark tests
- `docs/benchmarks/simulator_benchmark_profile.*`: generated single-seed outputs
- `docs/benchmarks/simulator_multiseed_profile.*`: multi-seed aggregate outputs
- `docs/benchmarks/simulator_ablation_profile.*`: mechanism ablation outputs
- `docs/benchmarks/simulator_agent_ablation_profile.*`: agent/workload ablation outputs
- `docs/benchmarks/simulator_parameter_grid_profile.*`: arbitrage x maker grid
- `docs/benchmarks/simulator_parameter_cube_profile.*`: retail x informed x maker cube
- `docs/benchmarks/simulator_parameter_hypercube_profile.*`: arbitrage x retail x informed x maker unified sweep
- `docs/benchmarks/simulator_parameter_hypercube_summary.*`: compact main-effect and high-low contrast summary for the unified sweep
- `NEURIPS_BENCHMARK_MANUSCRIPT.md`: benchmark-oriented manuscript draft
- `ENVIRONMENT_SCHEMA.md`: observation, action, reward, and metrics schema
- `APPENDIX_TABLES.md`: appendix-ready controller, ablation, and sweep tables
- `APPENDIX_FIGURES.md`: repository-hosted figure set
- `arxiv/`: isolated LaTeX source and compiled PDF for this paper line

## Current Single-Seed Snapshot

From `docs/benchmarks/simulator_benchmark_profile.json`:

- `Immediate-Surrogate`: `1360.0 orders/s`, `p50 10 ms`, `p99 10 ms`, retail surplus `-0.3237`
- `SpeedBump-50ms`: `1305.6 orders/s`, `p50 60 ms`, `p99 60 ms`, retail adverse rate `0.5078`
- `FBA-250ms`: `1347.6 orders/s`, `p50 80 ms`, `p99 490 ms`, retail surplus `-0.4599`
- `Policy-LearnedLinUCB-100-250ms`: `1347.6 orders/s`, `p50 50 ms`, `p99 130 ms`, retail surplus `0.3975`
- `Policy-LearnedTinyMLP-100-250ms`: `1347.6 orders/s`, `p50 60 ms`, `p99 300 ms`, price impact `4.24`
- `Policy-LearnedOfflineContextual-100-250ms`: `1349.2 orders/s`, `p50 80 ms`, `p99 200 ms`, impact `3.18`, retail adverse rate `0.4014`
- `FBA-250ms-Stress`: `1761.1 orders/s`, `p50 100 ms`, `p99 590 ms`

All generated scenarios currently report:

- `0` negative-balance violations
- `0` conservation breaches

## Multi-Seed Snapshot

From `docs/benchmarks/simulator_multiseed_profile.json`, aggregated over seeds `[7, 11, 19, 23, 29, 31, 37, 41]` and reported as `mean +/- CI95`:

- `Immediate-Surrogate`: `1348.23 +/- 3.99 orders/s`, `p99 10.00 +/- 0.00 ms`, retail surplus `-0.3710 +/- 0.1264`, welfare gap `2.0430 +/- 0.2444`
- `FBA-250ms`: `1337.60 +/- 3.88 orders/s`, `p99 452.50 +/- 16.16 ms`, queue `0.0273 +/- 0.0182`, welfare gap `0.8896 +/- 0.8264`
- `Adaptive-100-250ms`: `1337.60 +/- 3.88 orders/s`, adaptive mean `207.14 ms`, impact `4.71 +/- 0.49`, welfare gap `0.0278 +/- 0.6078`
- `Policy-BurstAware-100-250ms`: `1338.89 +/- 2.97 orders/s`, `p99 400.00 +/- 57.25 ms`, arb `621.00 +/- 94.21`, retail adverse `0.5019 +/- 0.0203`
- `Policy-LearnedLinUCB-100-250ms`: `1337.60 +/- 3.88 orders/s`, `755.65 +/- 27.48 fills/s`, `p99 155.00 +/- 17.32 ms`, retail surplus `0.0795 +/- 0.1353`, welfare gap `2.1694 +/- 0.7433`
- `Policy-LearnedTinyMLP-100-250ms`: `1337.60 +/- 3.88 orders/s`, `769.35 +/- 20.85 fills/s`, `p99 221.25 +/- 57.40 ms`, arb `856.13 +/- 107.16`, retail surplus `-0.3128 +/- 0.1535`
- `Policy-LearnedOfflineContextual-100-250ms`: `1337.40 +/- 3.91 orders/s`, `762.80 +/- 36.22 fills/s`, `p99 215.00 +/- 47.25 ms`, impact `4.94 +/- 0.57`, queue `0.0294 +/- 0.0156`, arb `771.25 +/- 113.73`, retail surplus `-0.1090 +/- 0.1191`
- `FBA-250ms-Stress`: `1769.25 +/- 6.04 orders/s`, `900.50 +/- 23.67 fills/s`, `p99 373.75 +/- 70.24 ms`, arb `2057.00 +/- 235.64`

Current controller interpretation:

- `LinUCB` remains the fastest learned controller on tail latency, but pays the highest welfare gap (`2.1694`) among the learned baselines.
- `TinyMLP` improves fills, but still leaves retail surplus negative and keeps arbitrage capture high.
- `OfflineContextual` is the most balanced learned baseline in the current repo: it keeps p99 far below burst-aware, cuts price impact below both `LinUCB` and `TinyMLP`, and brings queue advantage close to the batch-style heuristics.

## Welfare / Behavior Metrics

The repository still records several welfare/behavior diagnostics, but the paper line now emphasizes three primary welfare metrics:

- `retail_surplus_per_unit`
- `retail_adverse_selection_rate`
- `surplus_transfer_gap`

These three metrics are the most interpretable decomposition for the current benchmark:

- `retail_surplus_per_unit`: how retail flow performs against the synthetic fundamental
- `retail_adverse_selection_rate`: how often retail flow trades at negative ex post surplus
- `surplus_transfer_gap`: how much more per-unit surplus arbitrageurs capture than retail flow

Secondary diagnostics such as `arbitrageur_surplus_per_unit` and `welfare_dispersion` remain in the artifacts, but they are no longer the center of the paper claim.

## Sweeps

The benchmark now exposes three sweep families and one compact summary artifact:

1. `parameter_grid_profile`
- arbitrageur intensity `{0, 1, 2, 3}`
- maker quote width `{1, 2, 3}`

2. `parameter_cube_profile`
- retail intensity `{1, 2, 3}`
- informed intensity `{1, 2, 3}`
- maker quote width `{1, 2, 3}`

3. `parameter_hypercube_profile`
- arbitrageur intensity `{0, 1, 2, 3}`
- retail intensity `{1, 2, 3}`
- informed intensity `{1, 2, 3}`
- maker quote width `{1, 2, 3}`

4. `parameter_hypercube_summary`
- factor-level main effects over the unified hypercube
- high-low contrasts for arbitrage, retail, informed, and maker-width factors
- retail-conditioned `(arb=3) - (arb=0)` welfare deltas

Selected compact contrasts from `docs/benchmarks/simulator_parameter_hypercube_summary.json` over seeds `[101, 103, 107, 109]`:

- `arbitrageur_intensity 0 -> 3`: `+176.26 orders/s`, `-0.1804` retail surplus, `+0.0117` retail adverse, `+1.2099` welfare gap
- `retail_intensity 1 -> 3`: `+780.94 orders/s`, `+0.0772` retail surplus, `+0.0018` retail adverse, `+0.0781` welfare gap
- `maker_quote_width 1 -> 3`: `+0.00 orders/s`, `-0.1250` retail surplus, `+0.0085` retail adverse, `+0.2653` welfare gap
- `informed_intensity 1 -> 3`: `+150.91 orders/s`, `-0.1986` retail surplus, `+0.0131` retail adverse, `+0.0876` welfare gap

Retail-conditioned arbitrage deltas show the same pattern:

- `retail x1`: `(arb=3) - (arb=0)` gives `+2023.08` arb profit and `+1.1748` welfare gap
- `retail x2`: `(arb=3) - (arb=0)` gives `+2157.81` arb profit and `+1.2262` welfare gap
- `retail x3`: `(arb=3) - (arb=0)` gives `+2223.69` arb profit and `+1.2287` welfare gap

For reference, selected raw hypercube cells remain in `docs/benchmarks/simulator_parameter_hypercube_profile.json`:

- `(arb=0, retail=1, informed=2, maker=1)`: `1350.60 orders/s`, arb profit `0.00`, welfare gap `0.2376`
- `(arb=3, retail=1, informed=2, maker=1)`: `1542.86 orders/s`, arb profit `1982.00`, welfare gap `1.2839`
- `(arb=0, retail=3, informed=2, maker=1)`: `2147.02 orders/s`, `1105.75 fills/s`, welfare gap `0.1335`
- `(arb=3, retail=3, informed=2, maker=3)`: `2291.67 orders/s`, `1005.95 fills/s`, retail adverse `0.5434`, welfare gap `1.8005`

## Visualizations

Generated by `scripts/generate_neurips_figures.py`:

![Throughput comparison](figures/throughput.svg)

![Latency profile](figures/latency.svg)

![Fairness proxy comparison](figures/fairness.svg)

![Welfare and behavior comparison](figures/welfare.svg)

![Mechanism ablation snapshot](figures/ablation.svg)

![Agent and workload sweep snapshot](figures/agent_sweeps.svg)

![Parameter grid p99 heatmap](figures/grid_p99_heatmap.svg)

![Parameter grid arbitrage heatmap](figures/grid_arb_heatmap.svg)

Full appendix figure set: `APPENDIX_FIGURES.md`

## Regeneration

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

$env:RUN_SIM_HYPER="1"
go test ./simulator -run TestGenerateSimulatorParameterHypercubeArtifacts -v

python scripts/generate_neurips_figures.py
```
