# Simulator Calibrated Policy Protocol

This artifact defines the formal calibrated-learning protocol used in the paper: train on a calibrated adaptive market, select checkpoints on validation regimes, and report held-out performance separately.

- Train seeds: `[1103 1109 1117 1123]`
- Validation seeds: `[1129 1151]`
- Held-out seeds: `[1153 1163 1171 1181]`
- Validation regimes: `Calibrated-Protocol-Adaptive-1-3s, Calibrated-Validation-HighArb, Calibrated-Validation-RetailBurst`
- Held-out regimes: `Calibrated-HeldOut-CompositeStress, Calibrated-HeldOut-HighArbWideMaker, Calibrated-HeldOut-InformedWide, Calibrated-HeldOut-RetailBurst`

| Policy | Split | Regimes | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap | Neg. Bal. | Conservation |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| burst_aware | validation | 3 | 6 | 36.94 +/- 1.66 | 40.31 +/- 2.09 | 6166.67 +/- 298.20 | 8.45 +/- 0.18 | 1.3240 +/- 0.1710 | 0.4210 +/- 0.0082 | 5.3157 +/- 0.1615 | 0 | 0 |
| fitted_q | validation | 3 | 6 | 36.94 +/- 1.66 | 56.83 +/- 1.75 | 2333.33 +/- 754.40 | 3.60 +/- 0.42 | 0.0428 +/- 0.1249 | 0.4296 +/- 0.0045 | 2.9975 +/- 0.3854 | 0 | 0 |
| ppo_clip | validation | 3 | 6 | 36.94 +/- 1.66 | 40.52 +/- 1.81 | 6000.00 +/- 0.00 | 8.34 +/- 0.27 | 1.1204 +/- 0.1386 | 0.4160 +/- 0.0086 | 5.5169 +/- 0.1257 | 0 | 0 |
| iql | validation | 3 | 6 | 36.94 +/- 1.66 | 57.72 +/- 2.14 | 1000.00 +/- 0.00 | 3.50 +/- 0.36 | 0.0314 +/- 0.1097 | 0.4289 +/- 0.0040 | 2.9917 +/- 0.3085 | 0 | 0 |
| burst_aware | heldout | 4 | 16 | 40.09 +/- 1.76 | 47.16 +/- 3.43 | 6000.00 +/- 0.00 | 8.92 +/- 0.24 | 1.2463 +/- 0.2007 | 0.4248 +/- 0.0092 | 5.8322 +/- 0.1675 | 0 | 0 |
| fitted_q | heldout | 4 | 16 | 40.09 +/- 1.76 | 56.86 +/- 1.89 | 2625.00 +/- 382.51 | 5.48 +/- 0.36 | 0.1831 +/- 0.1832 | 0.4206 +/- 0.0140 | 3.9602 +/- 0.2672 | 0 | 0 |
| ppo_clip | heldout | 4 | 16 | 40.09 +/- 1.76 | 40.54 +/- 1.85 | 6000.00 +/- 0.00 | 10.07 +/- 0.46 | 1.1023 +/- 0.2079 | 0.4229 +/- 0.0098 | 6.6590 +/- 0.3586 | 0 | 0 |
| iql | heldout | 4 | 16 | 40.09 +/- 1.76 | 58.86 +/- 1.72 | 1500.00 +/- 245.00 | 5.29 +/- 0.41 | 0.0993 +/- 0.1649 | 0.4170 +/- 0.0161 | 3.9602 +/- 0.2590 | 0 | 0 |

## PPO Validation Trace

| Episode | Mean Train Reward | Validation Score |
|---:|---:|---:|
| 0 | 0.0000 | -597.8130 |
| 10 | 1236.5745 | -597.8130 |
| 20 | 1238.0808 | -597.8130 |
| 30 | 1223.5769 | -597.8130 |
| 40 | 1317.9668 | -597.8130 |
| 50 | 1315.0887 | -597.8130 |
| 60 | 1241.5015 | -597.8130 |
| 70 | 1257.7991 | -597.8130 |
| 80 | 1307.1003 | -587.0182 |

## IQL Summary

- Iterations: `6`
- Expectile: `0.70`
- Beta: `0.80`
- Best validation score: `-67.5256`
