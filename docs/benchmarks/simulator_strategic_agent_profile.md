# Strategic Agent Profile

Seeds: `[521 523 541 547]`

These scenarios use inventory-aware market makers, signal-scaled informed traders, trend-reactive retail flow, and dislocation-sensitive arbitrageurs.

| Scenario | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Strategic-Control | 4 | 1761.11 +/- 12.09 | 1040.67 +/- 35.03 | 392.50 +/- 91.77 | 5.34 +/- 0.63 | -0.2623 +/- 0.2991 | 0.4805 +/- 0.0311 | 1.2515 +/- 0.6668 |
| Strategic-HighArb | 4 | 1995.63 +/- 18.23 | 1193.45 +/- 37.64 | 430.00 +/- 110.22 | 5.34 +/- 0.45 | -0.5348 +/- 0.1569 | 0.5145 +/- 0.0533 | 1.4123 +/- 0.1500 |
| Strategic-RetailBurst | 4 | 2693.85 +/- 8.77 | 1608.73 +/- 110.35 | 427.50 +/- 90.98 | 5.53 +/- 0.77 | -0.2243 +/- 0.1652 | 0.5076 +/- 0.0181 | 1.4467 +/- 0.3559 |
