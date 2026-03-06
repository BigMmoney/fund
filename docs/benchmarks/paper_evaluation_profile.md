# Paper Evaluation Profile

| Scenario | Mode | Batch Window (ms) | Orders | Fills | Orders/s | Fills/s | p50 (ms) | p95 (ms) | p99 (ms) |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Immediate Surrogate | immediate | 0 | 400 | 400 | 144744.0 | 144744.0 | 1.71 | 2.76 | 2.76 |
| FBA-100ms | batch | 100 | 400 | 400 | 4978.0 | 4978.0 | 80.35 | 80.35 | 80.35 |
| FBA-250ms | batch | 250 | 400 | 400 | 1736.2 | 1736.2 | 230.39 | 230.39 | 230.39 |
| FBA-500ms | batch | 500 | 400 | 400 | 833.4 | 833.4 | 479.96 | 479.96 | 479.96 |
