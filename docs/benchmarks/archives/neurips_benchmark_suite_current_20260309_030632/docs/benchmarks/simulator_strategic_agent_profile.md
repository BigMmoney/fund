# Strategic Agent Profile

Seeds: `[521 523 541 547]`

These scenarios use inventory-aware market makers, signal-scaled informed traders, trend-reactive retail flow, and dislocation-sensitive arbitrageurs.

| Scenario | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Strategic-Control | 4 | 1761.11 +/- 12.09 | 1035.91 +/- 44.87 | 440.00 +/- 74.63 | 4.80 +/- 0.52 | -0.0691 +/- 0.1686 | 0.4714 +/- 0.0114 | 1.2832 +/- 0.5021 |
| Strategic-HighArb | 4 | 1995.63 +/- 18.23 | 1318.25 +/- 41.36 | 470.00 +/- 30.21 | 4.48 +/- 0.68 | -0.3370 +/- 0.1580 | 0.4410 +/- 0.0477 | 1.2387 +/- 0.7350 |
| Strategic-RetailBurst | 4 | 2693.85 +/- 8.77 | 1508.93 +/- 67.32 | 315.00 +/- 99.34 | 4.70 +/- 0.64 | 0.0189 +/- 0.1292 | 0.4781 +/- 0.0299 | 1.4009 +/- 0.4961 |
