# Simulator Benchmark Necessity

Benchmark score components:

- `z(fills_per_sec)`
- `-z(p99_latency_ms)`
- `+z(retail_surplus_per_unit)`
- `-z(retail_adverse_selection_rate)`
- `-z(surplus_transfer_gap)`

## Variant Rankings

### control

- Base scenario: `Calibrated-Protocol-Adaptive-1-3s`
- Held-out regimes: `Calibrated-HeldOut-CompositeStress, Calibrated-HeldOut-HighArbWideMaker, Calibrated-HeldOut-InformedWide, Calibrated-HeldOut-RetailBurst`

| Rank | Policy | Score | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | iql | 3.7179 | 58.86 | 1500.00 | 0.0993 | 0.4170 | 3.9602 |
| 2 | fitted_q | 1.8170 | 56.86 | 2625.00 | 0.1831 | 0.4206 | 3.9602 |
| 3 | burst_aware | -2.1695 | 47.16 | 6000.00 | 1.2463 | 0.4248 | 5.8322 |
| 4 | ppo_clip | -3.3654 | 40.54 | 6000.00 | 1.1023 | 0.4229 | 6.6590 |

### matching_only

- Base scenario: `Calibrated-MatchingOnly-Immediate`
- Held-out regimes: `HeldOut-CompositeStress, HeldOut-HighArbWideMaker, HeldOut-InformedWide, HeldOut-RetailBurst`

| Rank | Policy | Score | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | ppo_clip | 0.4051 | 32.39 | 1000.00 | -0.4647 | 0.4397 | 5.2043 |
| 2 | burst_aware | 0.3111 | 32.39 | 1000.00 | -0.4666 | 0.4399 | 5.2132 |
| 3 | iql | 0.0657 | 32.71 | 1000.00 | -0.5730 | 0.4705 | 4.5772 |
| 4 | fitted_q | -0.7820 | 32.33 | 1000.00 | -0.5039 | 0.4443 | 5.0745 |

### no_settlement

- Base scenario: `Calibrated-NoSettlement-1-3s`
- Held-out regimes: `HeldOut-CompositeStress, HeldOut-HighArbWideMaker, HeldOut-InformedWide, HeldOut-RetailBurst`

| Rank | Policy | Score | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | iql | 3.7179 | 58.86 | 1500.00 | 0.0993 | 0.4170 | 3.9602 |
| 2 | fitted_q | 1.8170 | 56.86 | 2625.00 | 0.1831 | 0.4206 | 3.9602 |
| 3 | burst_aware | -2.1695 | 47.16 | 6000.00 | 1.2463 | 0.4248 | 5.8322 |
| 4 | ppo_clip | -3.3654 | 40.54 | 6000.00 | 1.1023 | 0.4229 | 6.6590 |

### no_welfare_reward

- Base scenario: `Calibrated-Protocol-Adaptive-1-3s`
- Held-out regimes: `Calibrated-HeldOut-CompositeStress, Calibrated-HeldOut-HighArbWideMaker, Calibrated-HeldOut-InformedWide, Calibrated-HeldOut-RetailBurst`

| Rank | Policy | Score | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | iql | 2.8481 | 59.08 | 1687.50 | 0.1095 | 0.4165 | 3.9166 |
| 2 | ppo_clip | 1.4296 | 54.49 | 3312.50 | 0.5256 | 0.4191 | 3.3874 |
| 3 | fitted_q | 0.5206 | 56.86 | 2625.00 | 0.1831 | 0.4206 | 3.9602 |
| 4 | burst_aware | -4.7983 | 47.16 | 6000.00 | 1.2463 | 0.4248 | 5.8322 |

## Rank Shifts vs Control

| Variant | Common Policies | Kendall Tau | Frontier Overlap | Policies With Rank Change |
|---|---:|---:|---:|---:|
| matching_only | 4 | -0.6667 | 1 | 4 |
| no_settlement | 4 | 1.0000 | 2 | 0 |
| no_welfare_reward | 4 | 0.3333 | 1 | 3 |
