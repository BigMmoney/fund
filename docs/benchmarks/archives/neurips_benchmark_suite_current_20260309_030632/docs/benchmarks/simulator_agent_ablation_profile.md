# Simulator Agent/Workload Ablation Profile

Seeds: `[43 47 53 59]`

| Scenario | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit |
|---|---:|---:|---:|---:|---:|---:|
| AgentAblation-Control | 1343.25 | 661.31 | 377.50 | 5.40 | 0.0400 | 718.25 |
| AgentAblation-NoArbitrageurs | 1276.59 | 649.01 | 392.50 | 5.38 | -0.6016 | 0.00 |
| AgentAblation-NoInformed | 1257.14 | 608.33 | 352.50 | 5.43 | 0.0327 | 748.00 |
| AgentSweep-RetailBurst | 2532.94 | 1339.68 | 325.00 | 5.55 | 0.0549 | 797.00 |
| AgentSweep-ArbIntensityHigh | 1409.92 | 717.06 | 405.00 | 5.76 | 0.0383 | 1811.75 |
| AgentSweep-InformedIntensityHigh | 1426.79 | 754.17 | 397.50 | 5.59 | 0.0330 | 716.00 |
| AgentSweep-MakersWide | 1343.25 | 577.38 | 397.50 | 5.61 | 0.0408 | 733.75 |
