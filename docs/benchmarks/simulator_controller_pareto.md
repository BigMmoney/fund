# Simulator Controller Pareto Frontier

Seeds: `[7 11 19 23 29 31 37 41]`

Pareto axes minimize `p99 latency` and `surplus transfer gap`; `fills/s` is reported as a third axis for interpretation.

| Scenario | Category | Frontier | p99 (ms) | Welfare Gap | Fills/s |
|---|---|:---:|---:|---:|---:|
| Immediate-Surrogate | mechanism | yes | 10.00 +/- 0.00 | 2.0038 +/- 0.2862 | 808.54 +/- 17.39 |
| SpeedBump-50ms | mechanism |  | 60.00 +/- 0.00 | 4.0049 +/- 0.4219 | 776.20 +/- 16.69 |
| FBA-100ms | batch |  | 136.25 +/- 20.77 | 2.1907 +/- 0.8851 | 795.45 +/- 25.90 |
| Policy-LearnedLinUCB-100-250ms | controller |  | 141.25 +/- 19.43 | 2.2936 +/- 0.8384 | 749.31 +/- 27.69 |
| Policy-LearnedOnlineDQN-100-250ms | controller |  | 141.25 +/- 22.84 | 2.2973 +/- 0.8310 | 752.68 +/- 19.70 |
| Policy-LearnedFittedQ-100-250ms | controller |  | 156.25 +/- 33.58 | 2.1912 +/- 0.9548 | 755.26 +/- 26.98 |
| Policy-LearnedOfflineContextual-100-250ms | controller | yes | 283.75 +/- 57.24 | 0.5314 +/- 0.8582 | 776.69 +/- 19.61 |
| Policy-LearnedTinyMLP-100-250ms | controller |  | 311.25 +/- 60.06 | 3.0262 +/- 0.7376 | 684.82 +/- 37.90 |
| Adaptive-QueueLoad-100-250ms | adaptive |  | 351.25 +/- 64.95 | 0.9298 +/- 0.6985 | 686.51 +/- 30.87 |
| FBA-250ms-Stress | batch | yes | 370.00 +/- 65.65 | 0.3754 +/- 0.7836 | 918.35 +/- 39.26 |
| Policy-BurstAware-100-250ms | controller |  | 392.50 +/- 62.25 | 0.6877 +/- 0.8364 | 670.63 +/- 37.68 |
| FBA-250ms | batch |  | 397.50 +/- 64.89 | 0.8338 +/- 0.7458 | 674.50 +/- 28.71 |
| Adaptive-OrderFlow-100-250ms | adaptive |  | 413.75 +/- 44.50 | 0.7028 +/- 0.7035 | 674.11 +/- 13.84 |
| Adaptive-100-250ms | adaptive | yes | 430.00 +/- 50.33 | -0.0342 +/- 0.7196 | 708.23 +/- 25.14 |
| FBA-500ms | batch |  | 698.75 +/- 142.25 | 1.6878 +/- 1.3485 | 609.93 +/- 22.74 |
