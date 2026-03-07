# Online DQN Reward Sensitivity

Training seeds: `[307 311 313 317 331 337]`

Held-out seeds: `[223 227 229 233]`

Held-out regimes: `HeldOut-HighArbWideMaker, HeldOut-RetailBurst, HeldOut-InformedWide, HeldOut-CompositeStress`

Each row retrains the online DQN-style controller under a different reward-weight profile and evaluates the final checkpoint on the held-out regime set.

| Profile | Mean Train Reward | Runs | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|
| default | 923.9582 | 16 | 944.25 +/- 176.56 | 155.62 +/- 12.84 | 0.1177 +/- 0.1193 | 0.4961 +/- 0.0154 | 2.4226 +/- 0.5400 |
| latency_heavy | 1899.1326 | 16 | 944.25 +/- 176.56 | 155.62 +/- 12.84 | 0.1177 +/- 0.1193 | 0.4961 +/- 0.0154 | 2.4226 +/- 0.5400 |
| welfare_heavy | -108.5693 | 16 | 944.25 +/- 176.56 | 155.62 +/- 12.84 | 0.1177 +/- 0.1193 | 0.4961 +/- 0.0154 | 2.4226 +/- 0.5400 |
