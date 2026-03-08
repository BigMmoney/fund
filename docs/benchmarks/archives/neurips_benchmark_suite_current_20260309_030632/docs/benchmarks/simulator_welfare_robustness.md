# Simulator Welfare Robustness

## Suite Summaries

| Suite | Policy | Runs | p99 (ms) | Impact | Queue Adv. | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| base_heldout | burst_aware | 16 | 426.88 | 5.37 | 0.0337 | 1051.06 | -0.4179 | 0.5255 | 1.6954 |
| base_heldout | learned_fitted_q | 16 | 176.25 | 5.94 | 0.0557 | 1492.50 | 0.0471 | 0.5044 | 2.4481 |
| base_heldout | learned_linucb | 16 | 197.50 | 5.59 | 0.0474 | 1484.44 | 0.0190 | 0.4899 | 2.2584 |
| base_heldout | learned_offline_contextual | 16 | 286.25 | 5.22 | 0.0406 | 1151.75 | -0.1313 | 0.4920 | 1.1497 |
| base_heldout | learned_online_dqn | 16 | 163.12 | 6.06 | 0.0592 | 1556.44 | 0.0433 | 0.5058 | 2.5657 |
| strategic_population | burst_aware | 12 | 370.83 | 4.94 | 0.0871 | 3832.42 | -0.3028 | 0.4563 | 2.0131 |
| strategic_population | learned_fitted_q | 12 | 179.17 | 4.89 | 0.1707 | 4994.75 | -0.0084 | 0.4754 | 2.8205 |
| strategic_population | learned_linucb | 12 | 184.17 | 4.80 | 0.1374 | 4879.50 | 0.0494 | 0.4795 | 2.6577 |
| strategic_population | learned_offline_contextual | 12 | 266.67 | 4.18 | 0.1521 | 3846.42 | 0.0015 | 0.4737 | 1.9262 |
| strategic_population | learned_online_dqn | 12 | 182.50 | 4.78 | 0.1207 | 4799.58 | 0.0155 | 0.4746 | 2.6330 |

## Correlations

| Target | Metric | Samples | Pearson | Spearman |
|---|---|---:|---:|---:|
| surplus_transfer_gap | queue_priority_advantage | 140 | 0.3305 | 0.3154 |
| surplus_transfer_gap | latency_arbitrage_profit | 140 | 0.1657 | 0.3103 |
| surplus_transfer_gap | average_price_impact | 140 | 0.3417 | 0.3713 |
| retail_surplus_per_unit | retail_adverse_selection_rate | 140 | -0.3883 | -0.3716 |
| retail_surplus_per_unit | surplus_transfer_gap | 140 | -0.0811 | -0.0612 |

## Rank Stability

| Left Suite | Right Suite | Metric | Common Policies | Kendall Tau |
|---|---|---|---:|---:|
| base_heldout | strategic_population | surplus_transfer_gap | 5 | 0.6000 |
| base_heldout | strategic_population | retail_surplus_per_unit | 5 | 0.2000 |
