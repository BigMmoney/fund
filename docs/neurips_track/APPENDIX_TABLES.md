# Appendix Tables

This appendix summarizes the auxiliary benchmark tables used by the NeurIPS-track manuscript.

## Controller Comparison

Source: `docs/benchmarks/simulator_multiseed_profile.*`

| Controller | Orders/s | Fills/s | Mean Window (ms) | p99 (ms) | Impact | Queue Adv. | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Policy-BurstAware-100-250ms | 1338.89 | 670.83 | 250.00 | 400.00 | 5.31 | 0.0305 | 621.00 | -0.2258 | 0.5019 | 0.7579 |
| Policy-LearnedLinUCB-100-250ms | 1337.60 | 755.65 | 100.00 | 155.00 | 5.90 | 0.0513 | 976.63 | 0.0795 | 0.4690 | 2.1694 |
| Policy-LearnedTinyMLP-100-250ms | 1337.60 | 769.35 | 150.00 | 221.25 | 5.22 | 0.0336 | 856.13 | -0.3128 | 0.4924 | 1.9719 |
| Policy-LearnedOfflineContextual-100-250ms | 1337.40 | 762.80 | 138.59 | 215.00 | 4.94 | 0.0294 | 771.25 | -0.1090 | 0.4980 | 1.3769 |
| Policy-LearnedFittedQ-100-250ms | 1337.70 | 746.23 | 100.58 | 145.00 | 5.88 | 0.0451 | 966.38 | 0.0742 | 0.4738 | 2.2036 |
| Policy-LearnedOnlineDQN-100-250ms | 1337.60 | 740.77 | 100.00 | 145.00 | 5.92 | 0.0431 | 963.75 | 0.0662 | 0.4714 | 2.2472 |

Interpretation:

- `FittedQ` is now best on in-distribution tail latency, but it still widens the surplus transfer gap relative to `OfflineContextual`.
- `TinyMLP` still improves fills, but it remains welfare-weak on retail surplus.
- `OfflineContextual` is the most balanced learned baseline in the current repo: it keeps p99 and fills close to the learned policies while cutting impact, queue advantage, arbitrage-profit proxy, and welfare gap.
- `FittedQ` now provides the clearest offline-learning training story and the strongest in-distribution p99, but it still behaves more like `LinUCB` than `OfflineContextual` on surplus-transfer gap.
- `OnlineDQN` reaches the same in-distribution p99 band as `FittedQ`, but on held-out regimes it behaves more like a fast controller than a welfare-balanced one.

## Held-Out Policy Generalization

Source: `docs/benchmarks/simulator_heldout_policy_profile.*`

| Policy | Regimes | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| burst_aware | 4 | 1929.41 | 834.08 | 376.88 | 5.32 | -0.4042 | 0.5200 | 1.6286 |
| learned_linucb | 4 | 1930.41 | 978.52 | 171.25 | 5.76 | 0.1215 | 0.4916 | 2.4466 |
| learned_offline_contextual | 4 | 1930.85 | 961.61 | 275.62 | 5.38 | -0.0732 | 0.4703 | 1.9535 |
| learned_fitted_q | 4 | 1930.65 | 946.78 | 159.38 | 5.92 | 0.1110 | 0.4952 | 2.3158 |
| learned_online_dqn | 4 | 1930.70 | 986.61 | 172.50 | 5.70 | 0.0937 | 0.4910 | 2.2189 |

Interpretation:

- `FittedQ` improves on `LinUCB` on both `p99` and welfare gap in `HeldOut-HighArbWideMaker`, `HeldOut-RetailBurst`, and `HeldOut-CompositeStress`.
- In `HeldOut-InformedWide`, `FittedQ` gives up some tail latency versus `LinUCB` but still lowers welfare gap.
- `OfflineContextual` remains the most welfare-balanced learned baseline on held-out regimes, but it is materially slower on tail latency than both `LinUCB` and `FittedQ`.
- `OnlineDQN` adds the strongest held-out fills in the current repo, but that gain comes with a larger welfare gap than `OfflineContextual`.

## Fitted-Q Learning Curve

Source: `docs/benchmarks/simulator_fittedq_learning_curve.*`

| Iteration | Bellman MSE | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0.0000 | 875.99 +/- 159.10 | 341.25 +/- 22.97 | -0.7067 +/- 0.1909 | 0.5293 +/- 0.0121 | 3.2117 +/- 0.5538 |
| 1 | 45.1571 | 960.57 +/- 182.21 | 198.75 +/- 25.39 | -0.0251 +/- 0.1070 | 0.4999 +/- 0.0179 | 2.0790 +/- 0.4767 |
| 8 | 6.5753 | 944.25 +/- 176.56 | 155.62 +/- 12.84 | 0.1177 +/- 0.1193 | 0.4961 +/- 0.0154 | 2.4226 +/- 0.5400 |

Interpretation:

- the first Bellman update captures most of the held-out welfare-gap gain
- later iterations continue to reduce Bellman error and held-out p99
- the later controller is faster on tail latency, but it gives back some of the earliest welfare improvement

## Online DQN Learning Curve

Source: `docs/benchmarks/simulator_online_dqn_training_curve.*`

| Episode | Mean Train Reward | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0.0000 | 1014.48 +/- 187.09 | 200.00 +/- 25.46 | -0.1220 +/- 0.1360 | 0.4814 +/- 0.0215 | 1.5898 +/- 0.3869 |
| 20 | 935.6874 | 944.25 +/- 176.56 | 155.62 +/- 12.84 | 0.1177 +/- 0.1193 | 0.4961 +/- 0.0154 | 2.4226 +/- 0.5400 |
| 160 | 923.9582 | 944.25 +/- 176.56 | 155.62 +/- 12.84 | 0.1177 +/- 0.1193 | 0.4961 +/- 0.0154 | 2.4226 +/- 0.5400 |

Interpretation:

- the online controller converges quickly, with most of the movement complete by episode `20`
- the converged controller is faster on held-out tail latency than the untrained checkpoint
- the same convergence also widens the surplus-transfer gap, which is exactly the benchmark-plus-learning trade-off the paper now exposes

## Prioritized Double-DQN Learning Curve

Source: `docs/benchmarks/simulator_double_dqn_training_curve.*`

| Episode | Mean Train Reward | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0.0000 | 875.55 +/- 145.75 | 336.25 +/- 37.31 | -0.7330 +/- 0.2313 | 0.5320 +/- 0.0152 | 3.2567 +/- 0.6388 |
| 20 | 893.9220 | 933.43 +/- 176.88 | 163.12 +/- 13.94 | 0.0433 +/- 0.1247 | 0.5058 +/- 0.0146 | 2.5657 +/- 0.5204 |
| 80 | 895.6865 | 1079.32 +/- 183.72 | 177.50 +/- 21.88 | -0.0961 +/- 0.1065 | 0.4690 +/- 0.0258 | 1.6813 +/- 0.4221 |
| 200 | 915.6253 | 933.68 +/- 178.96 | 161.88 +/- 14.82 | 0.0339 +/- 0.1241 | 0.5060 +/- 0.0146 | 2.5926 +/- 0.5294 |

Interpretation:

- the stronger online-RL recipe exposes a genuine checkpoint-selection tradeoff
- intermediate checkpoints improve fills and welfare gap substantially relative to the untrained policy
- later optimization pushes the controller back toward the latency-favoring regime

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

## Strategic-Agent Robustness

Source: `docs/benchmarks/simulator_strategic_agent_profile.*`

| Scenario | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|
| Strategic-Control | 1761.11 +/- 12.09 | 1040.67 +/- 35.03 | 392.50 +/- 91.77 | 5.34 +/- 0.63 | -0.2623 +/- 0.2991 | 0.4805 +/- 0.0311 | 1.2515 +/- 0.6668 |
| Strategic-HighArb | 1995.63 +/- 18.23 | 1193.45 +/- 37.64 | 430.00 +/- 110.22 | 5.34 +/- 0.45 | -0.5348 +/- 0.1569 | 0.5145 +/- 0.0533 | 1.4123 +/- 0.1500 |
| Strategic-RetailBurst | 2693.85 +/- 8.77 | 1608.73 +/- 110.35 | 427.50 +/- 90.98 | 5.53 +/- 0.77 | -0.2243 +/- 0.1652 | 0.5076 +/- 0.0181 | 1.4467 +/- 0.3559 |

Interpretation:

- the richer state-dependent population preserves the same qualitative trade-off
- higher arbitrage pressure still worsens retail outcome
- stronger retail burst still loads throughput more than it reduces welfare transfer

## Parameter Grid Sweep

Source: `docs/benchmarks/simulator_parameter_grid_profile.*`

Grid definition:

- rows: arbitrageur intensity multiplier `{0, 1, 2, 3}`
- columns: maker quote-width multiplier `{1, 2, 3}`

Selected cells:

| Arb Multiplier | Maker Width | Orders/s | Fills/s | p99 (ms) | Queue Adv. | Arb Profit | Retail Surplus | Retail Adverse |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 1 | 1272.42 | 612.50 | 390.00 | -0.6093 | 0.00 | -0.0415 | 0.5045 |
| 1 | 1 | 1338.29 | 670.83 | 345.00 | 0.0075 | 676.00 | -0.1160 | 0.5057 |
| 2 | 3 | 1404.17 | 580.75 | 462.50 | 0.0001 | 1534.25 | -0.3538 | 0.4913 |
| 3 | 3 | 1470.04 | 682.54 | 462.50 | -0.0080 | 2151.00 | -0.2109 | 0.4879 |

## Parameter Cube Sweep

Source: `docs/benchmarks/simulator_parameter_cube_profile.*`

Cube definition:

- retail intensity multiplier `{1, 2, 3}`
- informed intensity multiplier `{1, 2, 3}`
- maker quote-width multiplier `{1, 2, 3}`

Selected cells:

| Retail Mult. | Informed Mult. | Maker Width | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit | Retail Surplus |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1 | 1 | 1339.68 | 650.60 | 400.00 | 4.40 | 0.0060 | 508.50 | -0.0079 |
| 1 | 3 | 1 | 1502.18 | 742.66 | 367.50 | 4.33 | 0.0222 | 445.75 | -0.1271 |
| 3 | 1 | 1 | 2132.54 | 1049.40 | 297.50 | 4.34 | 0.0326 | 549.25 | -0.1520 |
| 3 | 3 | 3 | 2292.86 | 956.75 | 397.50 | 4.10 | 0.0224 | 461.75 | -0.4057 |

## Unified Hypercube Sweep

Source: `docs/benchmarks/simulator_parameter_hypercube_profile.*`
Compact summary source: `docs/benchmarks/simulator_parameter_hypercube_summary.*`
Response-surface source: `docs/benchmarks/simulator_parameter_hypercube_response_surface.*`

Hypercube definition:

- arbitrageur intensity multiplier `{0, 1, 2, 3}`
- retail intensity multiplier `{1, 2, 3}`
- informed intensity multiplier `{1, 2, 3}`
- maker quote-width multiplier `{1, 2, 3}`

Selected cells:

| Arb | Retail | Informed | Maker Width | Orders/s | Fills/s | p99 (ms) | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 1 | 2 | 1 | 1350.60 | 676.39 | 412.50 | 0.00 | -0.2376 | 0.4963 | 0.2376 |
| 3 | 1 | 2 | 1 | 1542.86 | 806.94 | 362.50 | 1982.00 | -0.5444 | 0.4823 | 1.2839 |
| 0 | 3 | 2 | 1 | 2147.02 | 1105.75 | 397.50 | 0.00 | -0.1335 | 0.4819 | 0.1335 |
| 3 | 3 | 2 | 3 | 2291.67 | 1005.95 | 402.50 | 2285.50 | -0.4598 | 0.5434 | 1.8005 |
| 3 | 3 | 3 | 3 | 2325.20 | 1017.86 | 310.00 | 2188.50 | -0.4978 | 0.4899 | 1.5473 |

Interpretation:

- adding arbitrage pressure widens the welfare gap even when throughput rises
- higher retail intensity is mostly a throughput and fill-rate lever; it does not neutralize adverse selection on its own
- the unified hypercube makes it possible to separate "high activity" from "high transfer-to-arbitrageur" regimes under one artifact family

## Hypercube Response Surface

This response surface fits standardized main effects and pairwise interactions over the unified `arb x retail x informed x maker` hypercube.

### Welfare-Gap Fit

| Metric | R^2 | RMSE | Top Partial Effects |
|---|---:|---:|---|
| surplus_transfer_gap | 0.3495 | 0.5397 | `arbitrageur_intensity = 0.3157`, `maker_quote_width = 0.0291`, `retail_intensity = 0.0057`, `informed_intensity = 0.0055` |

### Retail-Surplus Fit

| Metric | R^2 | RMSE | Top Partial Effects |
|---|---:|---:|---|
| retail_surplus_per_unit | 0.7061 | 0.0810 | `informed_intensity = 0.3121`, `arbitrageur_intensity = 0.2374`, `maker_quote_width = 0.1195`, `retail_intensity = 0.0672` |

Interpretation:

- the response surface confirms that arbitrage pressure is the dominant welfare-gap driver
- retail surplus is more strongly shaped by informed-flow and arbitrage intensity than by retail-flow itself
- this is the paper's compact statistical layer, beyond purely descriptive slice analysis

## Compact Hypercube Summary

This summary compresses the `4 x 3 x 3 x 3` hypercube into paper-facing contrasts over the three primary welfare metrics: retail surplus, retail adverse selection, and surplus-transfer gap.

### Factor High-Low Contrasts

| Factor | Low -> High | Delta Orders/s | Delta Retail Surplus | Delta Retail Adverse | Delta Welfare Gap |
|---|---:|---:|---:|---:|---:|
| arbitrageur_intensity | `0 -> 3` | 176.26 | -0.1804 | 0.0117 | 1.2099 |
| retail_intensity | `1 -> 3` | 780.94 | 0.0772 | 0.0018 | 0.0781 |
| informed_intensity | `1 -> 3` | 150.91 | -0.1986 | 0.0131 | 0.0876 |
| maker_quote_width | `1 -> 3` | 0.00 | -0.1250 | 0.0085 | 0.2653 |

### Retail-Conditioned Arbitrage Effect

Each row is the average `(arb=3) - (arb=0)` delta at fixed retail intensity, averaged across informed intensity and maker width.

| Retail Level | Delta Orders/s | Delta Arb Profit | Delta Retail Surplus | Delta Retail Adverse | Delta Welfare Gap |
|---:|---:|---:|---:|---:|---:|
| 1 | 192.26 | 2023.08 | -0.2430 | -0.0074 | 1.1748 |
| 2 | 192.26 | 2157.81 | -0.1816 | 0.0190 | 1.2262 |
| 3 | 144.25 | 2223.69 | -0.1166 | 0.0235 | 1.2287 |

Interpretation:

- arbitrage intensity is the dominant driver of welfare-gap expansion in the unified sweep
- this effect persists across retail intensities
- higher retail intensity mostly loads throughput; it does not offset transfer-to-arbitrageur
- wider maker quotes do not change throughput here, but they still worsen retail outcomes and welfare gap
