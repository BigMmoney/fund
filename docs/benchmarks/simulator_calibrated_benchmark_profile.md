# Simulator Calibrated Benchmark Profile

Seeds: `[701 709 719 727]`

| Scenario | Orders/s | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap | Safety |
|---|---:|---:|---:|---:|---:|---:|---|
| Calibrated-Immediate-Surrogate | 34.84 +/- 0.01 | 28.53 +/- 0.08 | 1000.00 +/- 0.00 | -0.8331 +/- 0.0929 | 0.5389 +/- 0.0083 | 4.3321 +/- 0.1398 | zero-breach |
| Calibrated-FBA-2s | 34.77 +/- 0.01 | 42.41 +/- 0.42 | 2000.00 +/- 0.00 | 0.0185 +/- 0.1146 | 0.4344 +/- 0.0119 | 3.3927 +/- 0.1593 | zero-breach |
| Calibrated-Adaptive-1-3s | 34.77 +/- 0.01 | 40.10 +/- 0.59 | 3000.00 +/- 0.00 | 0.2110 +/- 0.2015 | 0.4314 +/- 0.0133 | 4.2403 +/- 0.3230 | zero-breach |
| Calibrated-Policy-LearnedOfflineContextual-1-3s | 34.77 +/- 0.01 | 52.14 +/- 1.11 | 1500.00 +/- 490.00 | -0.1211 +/- 0.0918 | 0.4370 +/- 0.0130 | 3.1656 +/- 0.1037 | zero-breach |
| Calibrated-Policy-LearnedFittedQ-1-3s | 34.77 +/- 0.01 | 52.35 +/- 1.19 | 1000.00 +/- 0.00 | -0.1018 +/- 0.1107 | 0.4330 +/- 0.0146 | 3.1301 +/- 0.1299 | zero-breach |
