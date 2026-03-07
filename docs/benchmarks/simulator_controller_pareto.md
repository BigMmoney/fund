# Simulator Controller Pareto Frontier

Seeds: `[7 11 19 23 29 31 37 41]`

Pareto axes minimize `p99 latency` and `surplus transfer gap`; `fills/s` is reported as a third axis for interpretation.

| Scenario | Category | Frontier | p99 (ms) | Welfare Gap | Fills/s |
|---|---|:---:|---:|---:|---:|
| Immediate-Surrogate | mechanism | yes | 10.00 +/- 0.00 | 2.0430 +/- 0.2444 | 813.12 +/- 18.47 |
| SpeedBump-50ms | mechanism |  | 60.00 +/- 0.00 | 4.1034 +/- 0.3793 | 780.60 +/- 17.73 |
| Policy-LearnedFittedQ-100-250ms | controller |  | 145.00 +/- 21.36 | 2.2036 +/- 0.6732 | 746.23 +/- 27.90 |
| Policy-LearnedOnlineDQN-100-250ms | controller |  | 145.00 +/- 21.36 | 2.2472 +/- 0.7078 | 740.77 +/- 26.35 |
| FBA-100ms | batch |  | 146.25 +/- 35.83 | 2.3947 +/- 0.6460 | 798.66 +/- 21.71 |
| Policy-LearnedLinUCB-100-250ms | controller |  | 155.00 +/- 17.32 | 2.1694 +/- 0.7433 | 755.65 +/- 27.48 |
| Policy-LearnedOfflineContextual-100-250ms | controller | yes | 215.00 +/- 47.25 | 1.3769 +/- 0.8055 | 762.80 +/- 36.22 |
| Policy-LearnedTinyMLP-100-250ms | controller |  | 221.25 +/- 57.40 | 1.9719 +/- 0.8498 | 769.35 +/- 20.85 |
| Adaptive-100-250ms | adaptive | yes | 360.00 +/- 69.38 | 0.0278 +/- 0.6078 | 714.29 +/- 22.20 |
| FBA-250ms-Stress | batch |  | 373.75 +/- 70.24 | 0.3805 +/- 0.8714 | 900.50 +/- 23.67 |
| Adaptive-QueueLoad-100-250ms | adaptive |  | 386.25 +/- 65.09 | 0.8310 +/- 0.6128 | 691.57 +/- 27.02 |
| Policy-BurstAware-100-250ms | controller |  | 400.00 +/- 57.25 | 0.7579 +/- 0.7681 | 670.83 +/- 28.54 |
| Adaptive-OrderFlow-100-250ms | adaptive |  | 406.25 +/- 46.22 | 1.0415 +/- 0.7764 | 671.43 +/- 15.81 |
| FBA-250ms | batch |  | 452.50 +/- 16.16 | 0.8896 +/- 0.8264 | 686.41 +/- 17.80 |
| FBA-500ms | batch |  | 835.00 +/- 84.37 | 1.8314 +/- 1.4419 | 626.90 +/- 19.72 |
