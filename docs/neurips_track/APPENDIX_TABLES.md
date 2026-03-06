# Appendix Tables

This appendix summarizes the auxiliary benchmark tables used by the NeurIPS-track manuscript.

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

## Interpretation

- `Ablation-RelaxedRisk` confirms that risk gating is a real throughput bottleneck in the stressed constrained configuration.
- `AgentSweep-ArbIntensityHigh` shows that the benchmark reacts sharply to latency-sensitive flow, rather than only to mechanism toggles.
- `AgentSweep-MakersWide` increases p99 tail latency and reduces fills, making quote-width sensitivity visible in the benchmark.

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
