# Appendix Tables

This appendix summarizes the auxiliary benchmark tables used by the NeurIPS-track manuscript.

## Controller Comparison

Source: `docs/benchmarks/simulator_multiseed_profile.*`

| Controller | Orders/s | Fills/s | Mean Window (ms) | p99 (ms) | Impact | Queue Adv. | Arb Profit |
|---|---:|---:|---:|---:|---:|---:|---:|
| Policy-BurstAware-100-250ms | 1338.89 | 670.83 | 250.00 | 400.00 | 5.31 | 0.0305 | 621.00 |
| Policy-LearnedLinUCB-100-250ms | 1337.60 | 745.24 | 100.00 | 158.75 | 5.94 | 0.0447 | 959.62 |
| Policy-LearnedTinyMLP-100-250ms | 1338.39 | 689.78 | 200.00 | 305.00 | 8.86 | 0.0278 | 1229.00 |

Interpretation:

- `LinUCB` still dominates on tail latency and fill throughput.
- the gradient-trained `TinyMLP` shifts toward slower windows, bringing queue-advantage proxy closer to the batch-style baselines.
- that shift is not free: `TinyMLP` now pays materially higher price-impact and arbitrage-profit proxies than both burst-aware and LinUCB.

## Mechanism Ablation

Source: `docs/benchmarks/simulator_ablation_profile.*`

| Scenario | Orders/s | Fills/s | p99 (ms) | Queue Adv. | Arb Profit | Risk Rejects | Safety Violations |
|---|---:|---:|---:|---:|---:|---:|---:|
| Ablation-Control | 1190.48 | 605.36 | 320.00 | -0.0509 | 685.25 | 2922 | 0 |
| Ablation-RelaxedRisk | 1770.24 | 925.79 | 302.50 | 0.0372 | 2198.50 | 0 | 0 |
| Ablation-RandomTieBreak | 1190.48 | 595.63 | 402.50 | -0.0540 | 738.50 | 2908 | 0 |
| Ablation-NoSettlementChecks | 1190.48 | 605.36 | 320.00 | -0.0509 | 685.25 | 2922 | 0 |

## Agent and Workload Sweep

Source: `docs/benchmarks/simulator_agent_ablation_profile.*`

| Scenario | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit |
|---|---:|---:|---:|---:|---:|---:|
| AgentAblation-Control | 1343.25 | 659.52 | 367.50 | 5.49 | 0.0408 | 754.00 |
| AgentAblation-NoArbitrageurs | 1276.59 | 622.62 | 367.50 | 5.38 | -0.5958 | 0.00 |
| AgentAblation-NoInformed | 1257.14 | 622.62 | 335.00 | 5.54 | 0.0260 | 750.25 |
| AgentSweep-RetailBurst | 2532.94 | 1427.18 | 467.50 | 5.70 | 0.0404 | 821.75 |
| AgentSweep-ArbIntensityHigh | 1409.92 | 718.45 | 302.50 | 5.80 | 0.0314 | 1730.75 |
| AgentSweep-InformedIntensityHigh | 1426.79 | 724.01 | 360.00 | 5.62 | 0.0111 | 685.50 |
| AgentSweep-MakersWide | 1343.25 | 595.63 | 485.00 | 5.81 | 0.0469 | 754.50 |

## Parameter Grid Sweep

Source: `docs/benchmarks/simulator_parameter_grid_profile.*`

Grid definition:

- rows: arbitrageur intensity multiplier `{0, 1, 2, 3}`
- columns: maker quote-width multiplier `{1, 2, 3}`

Selected cells:

| Arb Multiplier | Maker Width | Orders/s | Fills/s | p99 (ms) | Queue Adv. | Arb Profit |
|---:|---:|---:|---:|---:|---:|---:|
| 0 | 1 | 1272.42 | 612.50 | 390.00 | -0.6093 | 0.00 |
| 1 | 1 | 1338.29 | 670.83 | 345.00 | 0.0075 | 676.00 |
| 2 | 3 | 1404.17 | 580.75 | 462.50 | 0.0001 | 1534.25 |
| 3 | 3 | 1470.04 | 682.54 | 462.50 | -0.0080 | 2151.00 |

Interpretation:

- removing arbitrageurs collapses the arbitrage-profit proxy and flips queue advantage strongly negative
- increasing arbitrage intensity consistently raises arbitrage-profit proxy and throughput
- wider maker quotes push p99 tails upward and reduce fills, especially once arbitrage pressure is already elevated

## Parameter Cube Sweep

Source: `docs/benchmarks/simulator_parameter_cube_profile.*`

Cube definition:

- retail intensity multiplier `{1, 2, 3}`
- informed intensity multiplier `{1, 2, 3}`
- maker quote-width multiplier `{1, 2, 3}`

Selected cells:

| Retail Mult. | Informed Mult. | Maker Width | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1 | 1 | 1339.68 | 650.60 | 400.00 | 4.40 | 0.0060 | 508.50 |
| 1 | 3 | 1 | 1502.18 | 742.66 | 367.50 | 4.33 | 0.0222 | 445.75 |
| 3 | 1 | 1 | 2132.54 | 1049.40 | 297.50 | 4.34 | 0.0326 | 549.25 |
| 3 | 3 | 3 | 2292.86 | 956.75 | 397.50 | 4.10 | 0.0224 | 461.75 |

Interpretation:

- retail intensity is the strongest throughput lever in the current cube, pushing orders/s from `1339.68` to `2132.54` even before informed flow is scaled
- informed intensity adds more fills without widening arbitrage-profit proxy nearly as much as the arbitrage-specific grid
- maker-width expansion systematically suppresses fills inside each retail slice, even when total orders/s keeps rising
