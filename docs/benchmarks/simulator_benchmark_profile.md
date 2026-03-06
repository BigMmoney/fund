# Simulator Benchmark Profile

| Scenario | Mode | Window (ms) | Orders/s | Fills/s | p50 (ms) | p95 (ms) | Spread | Price Impact | Queue Advantage | Arb Profit | Dispersion | Risk Rejects |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Immediate-Surrogate | immediate | 10 | 1360.00 | 829.17 | 10.00 | 10.00 | 1.82 | 3.27 | 0.0378 | 700.00 | 0.0649 | 0 |
| FBA-100ms | batch | 100 | 1360.00 | 802.50 | 50.00 | 110.00 | 1.00 | 6.82 | 0.0400 | 1295.00 | 0.0640 | 0 |
| FBA-250ms | batch | 250 | 1358.40 | 628.80 | 80.00 | 430.00 | 1.00 | 6.68 | -0.0065 | 766.00 | 0.0511 | 0 |
| FBA-500ms | batch | 500 | 1358.67 | 644.67 | 190.00 | 780.00 | 1.00 | 5.32 | 0.0236 | 1197.00 | 0.0476 | 0 |
| FBA-250ms-Stress | batch | 250 | 1775.20 | 987.20 | 100.00 | 360.00 | 1.00 | 3.74 | 0.0090 | 1176.00 | 0.0754 | 0 |
