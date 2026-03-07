# Binance Spot Stylized Facts (smoke)

This artifact summarizes the first calibration bundle extracted from Binance Spot public market data.

## Profile Summary

- symbols: `2`
- total trades: `38576`
- spread-mean range: `0.010000` -> `0.010000`
- order-sign lag1 range: `0.7409` -> `0.8167`
- inter-arrival mean range ms: `158.26` -> `227.42`
- volatility abs-return lag1 range: `-0.0432` -> `0.0831`
- top impact bucket mean range: `0.03752150` -> `1.11687162`

## BTCUSDT

- trades: `22748`
- klines: `60`
- depth snapshots: `6`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- order-sign autocorr lag1/lag5/lag10: `0.7409` / `0.6563` / `0.5783`
- inter-arrival mean/median/p90 ms: `158.26` / `0.00` / `550.00`
- volatility clustering abs/sq lag1: `-0.0432` / `-0.0817`

| Depth Level | Mean Bid Qty | Mean Ask Qty |
|---:|---:|---:|
| 1 | 0.587305 | 1.621083 |
| 2 | 0.000715 | 0.000160 |
| 3 | 0.000145 | 0.009120 |
| 4 | 0.006128 | 0.020153 |
| 5 | 0.000198 | 0.035785 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact |
|---|---:|---:|
| q1 | 1.66243882 | 3.41000000 |
| q2 | 1.19604152 | 3.21000000 |
| q3 | 1.17307782 | 3.08000000 |
| q4 | 1.11687162 | 2.99000000 |

## ETHUSDT

- trades: `15828`
- klines: `60`
- depth snapshots: `6`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- order-sign autocorr lag1/lag5/lag10: `0.8167` / `0.7072` / `0.6009`
- inter-arrival mean/median/p90 ms: `227.42` / `0.00` / `674.00`
- volatility clustering abs/sq lag1: `0.0831` / `0.0612`

| Depth Level | Mean Bid Qty | Mean Ask Qty |
|---:|---:|---:|
| 1 | 27.754133 | 20.089717 |
| 2 | 0.076083 | 0.959583 |
| 3 | 0.074250 | 0.307167 |
| 4 | 0.221950 | 0.009200 |
| 5 | 0.071650 | 0.061983 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact |
|---|---:|---:|
| q1 | 0.06668178 | 0.11000000 |
| q2 | 0.06704822 | 0.11000000 |
| q3 | 0.05752465 | 0.11000000 |
| q4 | 0.03752150 | 0.10000000 |

