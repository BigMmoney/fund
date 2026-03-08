# Binance Spot Stylized Facts (smoke)

This artifact summarizes the first calibration bundle extracted from Binance Spot public market data.

## Profile Summary

- symbols: `2`
- total trades: `38576`
- spread-mean range: `0.010000` -> `0.010000`
- spread-mean bps range: `0.0015` -> `0.0509`
- order-sign lag1 range: `0.7409` -> `0.8167`
- inter-arrival mean range ms: `158.26` -> `227.42`
- volatility abs-return lag1 range: `-0.0432` -> `0.0831`
- top impact bucket mean range: `0.03752150` -> `1.11687162`
- top impact bucket mean bps range: `0.1657` -> `0.1907`

## BTCUSDT

- trades: `22748`
- klines: `60`
- depth snapshots: `6`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- spread bps mean/median/p90: `0.0015` / `0.0015` / `0.0015`
- order-sign autocorr lag1/lag5/lag10: `0.7409` / `0.6563` / `0.5783`
- inter-arrival mean/median/p90 ms: `158.26` / `0.00` / `550.00`
- volatility clustering abs/sq lag1: `-0.0432` / `-0.0817`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 0.587305 | 1.621083 | 1.0000 | 1.0000 |
| 2 | 0.000715 | 0.000160 | 0.0012 | 0.0001 |
| 3 | 0.000145 | 0.009120 | 0.0002 | 0.0056 |
| 4 | 0.006128 | 0.020153 | 0.0104 | 0.0124 |
| 5 | 0.000198 | 0.035785 | 0.0003 | 0.0221 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | 1.66243882 | 3.41000000 | 0.2467 | 0.5059 |
| q2 | 1.19604152 | 3.21000000 | 0.1775 | 0.4761 |
| q3 | 1.17307782 | 3.08000000 | 0.1741 | 0.4574 |
| q4 | 1.11687162 | 2.99000000 | 0.1657 | 0.4438 |

## ETHUSDT

- trades: `15828`
- klines: `60`
- depth snapshots: `6`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- spread bps mean/median/p90: `0.0509` / `0.0509` / `0.0509`
- order-sign autocorr lag1/lag5/lag10: `0.8167` / `0.7072` / `0.6009`
- inter-arrival mean/median/p90 ms: `227.42` / `0.00` / `674.00`
- volatility clustering abs/sq lag1: `0.0831` / `0.0612`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 27.754133 | 20.089717 | 1.0000 | 1.0000 |
| 2 | 0.076083 | 0.959583 | 0.0027 | 0.0478 |
| 3 | 0.074250 | 0.307167 | 0.0027 | 0.0153 |
| 4 | 0.221950 | 0.009200 | 0.0080 | 0.0005 |
| 5 | 0.071650 | 0.061983 | 0.0026 | 0.0031 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | 0.06668178 | 0.11000000 | 0.3390 | 0.5595 |
| q2 | 0.06704822 | 0.11000000 | 0.3409 | 0.5591 |
| q3 | 0.05752465 | 0.11000000 | 0.2924 | 0.5590 |
| q4 | 0.03752150 | 0.10000000 | 0.1907 | 0.5088 |

