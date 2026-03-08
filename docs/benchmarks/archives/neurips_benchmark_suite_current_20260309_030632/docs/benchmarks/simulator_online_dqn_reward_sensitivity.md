# Online DQN Reward Sensitivity

Training seeds: `[307 311 313 317 331 337]`

Held-out seeds: `[223 227 229 233]`

Held-out regimes: `HeldOut-HighArbWideMaker, HeldOut-RetailBurst, HeldOut-InformedWide, HeldOut-CompositeStress`

Each row retrains the online DQN-style controller under a different reward-weight profile and evaluates the final checkpoint on the held-out regime set.

| Profile | Mean Train Reward | Runs | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|
| default | 880.7282 | 16 | 933.43 +/- 176.88 | 163.12 +/- 13.94 | 0.0433 +/- 0.1247 | 0.5058 +/- 0.0146 | 2.5657 +/- 0.5204 |
| latency_heavy | 1881.6688 | 16 | 954.91 +/- 179.48 | 156.88 +/- 16.32 | 0.1048 +/- 0.1054 | 0.4878 +/- 0.0124 | 2.2164 +/- 0.4719 |
| welfare_heavy | -106.6762 | 16 | 933.43 +/- 176.88 | 163.12 +/- 13.94 | 0.0433 +/- 0.1247 | 0.5058 +/- 0.0146 | 2.5657 +/- 0.5204 |
