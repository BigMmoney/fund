# Simulator Ablation Profile

Seeds: `[13 17 19 23]`

| Scenario | Orders/s | Fills/s | p99 (ms) | Queue Adv. | Arb Profit | Risk Rejects | Safety Violations |
|---|---:|---:|---:|---:|---:|---:|---:|
| Ablation-Control | 1190.48 | 586.31 | 372.50 | -0.0503 | 730.25 | 2922 | 0 |
| Ablation-RelaxedRisk | 1770.24 | 943.25 | 282.50 | 0.0404 | 2244.75 | 0 | 0 |
| Ablation-RandomTieBreak | 1190.48 | 591.87 | 382.50 | -0.0544 | 730.00 | 2927 | 0 |
| Ablation-NoSettlementChecks | 1190.48 | 586.31 | 372.50 | -0.0503 | 730.25 | 2922 | 0 |
