# Simulator Policy Leaderboard

- Task: `calibrated_heldout_latency_welfare_protocol`
- Observation features: `7`
- Discrete actions: `6`
- Train seeds: `[1103 1109 1117 1123]`
- Validation seeds: `[1129 1151]`
- Held-out seeds: `[1153 1163 1171 1181]`
- Validation regimes: `Calibrated-Protocol-Adaptive-1-3s, Calibrated-Validation-HighArb, Calibrated-Validation-RetailBurst`
- Held-out regimes: `Calibrated-HeldOut-CompositeStress, Calibrated-HeldOut-HighArbWideMaker, Calibrated-HeldOut-InformedWide, Calibrated-HeldOut-RetailBurst`
- Score: `z(fills_per_sec) - z(p99_latency_ms) + z(retail_surplus_per_unit) - z(retail_adverse_selection_rate) - z(surplus_transfer_gap)`

| Rank | Policy | Family | Budget | Score | Frontier | Safety | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | iql | offline_value | 6 iql iterations (expectile 0.70, beta 0.80) | 3.7179 | true | true | 58.86 | 1500.00 | 5.29 | 0.0993 | 0.4170 | 3.9602 |
| 2 | fitted_q | offline_value | 8 fitted-q iterations | 1.8170 | true | true | 56.86 | 2625.00 | 5.48 | 0.1831 | 0.4206 | 3.9602 |
| 3 | burst_aware | heuristic | no_training | -2.1695 | false | true | 47.16 | 6000.00 | 8.92 | 1.2463 | 0.4248 | 5.8322 |
| 4 | ppo_clip | online_policy | 80 episodes x 3 epochs | -3.3654 | false | true | 40.54 | 6000.00 | 10.07 | 1.1023 | 0.4229 | 6.6590 |
