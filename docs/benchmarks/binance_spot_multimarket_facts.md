# Binance Spot Stylized Facts (multimarket)

This artifact summarizes the first calibration bundle extracted from Binance Spot public market data.

## Profile Summary

- symbols: `8`
- total trades: `402755`
- spread-mean range: `0.000010` -> `0.010000`
- spread-mean bps range: `0.0015` -> `11.2170`
- order-sign lag1 range: `0.2831` -> `0.8056`
- inter-arrival mean range ms: `125.88` -> `7808.87`
- volatility abs-return lag1 range: `0.0522` -> `0.3892`
- top impact bucket mean range: `-0.00075385` -> `0.97178892`
- top impact bucket mean bps range: `-0.8461` -> `0.6487`

## ADAUSDT

- trades: `5728`
- klines: `360`
- depth snapshots: `8`
- spread mean/median/p90: `0.000100` / `0.000100` / `0.000100`
- spread bps mean/median/p90: `3.9270` / `3.9270` / `3.9270`
- order-sign autocorr lag1/lag5/lag10: `0.2831` / `0.1000` / `0.0899`
- inter-arrival mean/median/p90 ms: `3771.61` / `1018.00` / `11185.00`
- volatility clustering abs/sq lag1: `0.3081` / `0.1872`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 57150.762500 | 81728.062500 | 1.0000 | 1.0000 |
| 2 | 96929.787500 | 176668.775000 | 1.6960 | 2.1617 |
| 3 | 166767.725000 | 194653.925000 | 2.9180 | 2.3817 |
| 4 | 176792.925000 | 179805.887500 | 3.0934 | 2.2001 |
| 5 | 367289.137500 | 147066.525000 | 6.4267 | 1.7995 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | -0.00002360 | 0.00010000 | -0.9228 | 3.9277 |
| q2 | -0.00001319 | 0.00010000 | -0.5159 | 3.9339 |
| q3 | 0.00000091 | 0.00020000 | 0.0370 | 7.8339 |
| q4 | 0.00001583 | 0.00020000 | 0.6199 | 7.8339 |

## AVAXUSDT

- trades: `2763`
- klines: `360`
- depth snapshots: `8`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- spread bps mean/median/p90: `11.2170` / `11.2170` / `11.2170`
- order-sign autocorr lag1/lag5/lag10: `0.4276` / `0.1692` / `0.1102`
- inter-arrival mean/median/p90 ms: `7808.87` / `686.50` / `25000.00`
- volatility clustering abs/sq lag1: `0.0522` / `0.0114`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 4419.333750 | 5825.261250 | 1.0000 | 1.0000 |
| 2 | 19867.033750 | 9532.547500 | 4.4955 | 1.6364 |
| 3 | 27098.120000 | 11304.730000 | 6.1317 | 1.9406 |
| 4 | 17154.190000 | 30048.940000 | 3.8816 | 5.1584 |
| 5 | 10835.510000 | 8215.240000 | 2.4518 | 1.4103 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | -0.00340146 | 0.00000000 | -3.8001 | 0.0000 |
| q2 | -0.00120592 | 0.01000000 | -1.3505 | 11.1857 |
| q3 | 0.00000000 | 0.00000000 | 0.0000 | 0.0000 |
| q4 | -0.00075385 | 0.01000000 | -0.8461 | 11.1857 |

## BNBUSDT

- trades: `38882`
- klines: `360`
- depth snapshots: `8`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- spread bps mean/median/p90: `0.1611` / `0.1611` / `0.1611`
- order-sign autocorr lag1/lag5/lag10: `0.5693` / `0.4131` / `0.3083`
- inter-arrival mean/median/p90 ms: `555.51` / `28.00` / `1777.00`
- volatility clustering abs/sq lag1: `0.2914` / `0.1941`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 0.678000 | 30.053875 | 1.0000 | 1.0000 |
| 2 | 0.058750 | 2.938625 | 0.0867 | 0.0978 |
| 3 | 0.727250 | 4.961375 | 1.0726 | 0.1651 |
| 4 | 2.939875 | 4.638500 | 4.3361 | 0.1543 |
| 5 | 0.035875 | 8.838750 | 0.0529 | 0.2941 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | 0.01658797 | 0.08000000 | 0.2667 | 1.2810 |
| q2 | 0.02132203 | 0.08000000 | 0.3427 | 1.2830 |
| q3 | 0.01825247 | 0.07000000 | 0.2934 | 1.1296 |
| q4 | 0.02032932 | 0.08000000 | 0.3268 | 1.2880 |

## BTCUSDT

- trades: `171582`
- klines: `360`
- depth snapshots: `8`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- spread bps mean/median/p90: `0.0015` / `0.0015` / `0.0015`
- order-sign autocorr lag1/lag5/lag10: `0.6944` / `0.6014` / `0.5239`
- inter-arrival mean/median/p90 ms: `125.88` / `0.00` / `427.00`
- volatility clustering abs/sq lag1: `0.2605` / `0.1295`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 2.559655 | 0.143260 | 1.0000 | 1.0000 |
| 2 | 0.010001 | 0.000530 | 0.0039 | 0.0037 |
| 3 | 0.006319 | 0.002134 | 0.0025 | 0.0149 |
| 4 | 0.122754 | 0.000140 | 0.0480 | 0.0010 |
| 5 | 0.110656 | 0.006696 | 0.0432 | 0.0467 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | 1.55336674 | 3.40000000 | 0.2303 | 0.5040 |
| q2 | 1.16436489 | 3.18000000 | 0.1726 | 0.4708 |
| q3 | 1.01706560 | 2.90000000 | 0.1508 | 0.4301 |
| q4 | 0.97178892 | 2.90000000 | 0.1441 | 0.4305 |

## DOGEUSDT

- trades: `13730`
- klines: `360`
- depth snapshots: `8`
- spread mean/median/p90: `0.000010` / `0.000010` / `0.000010`
- spread bps mean/median/p90: `1.1129` / `1.1129` / `1.1129`
- order-sign autocorr lag1/lag5/lag10: `0.3669` / `0.1125` / `0.0565`
- inter-arrival mean/median/p90 ms: `1572.97` / `305.00` / `4686.00`
- volatility clustering abs/sq lag1: `0.3892` / `0.1633`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 28053.125000 | 152404.250000 | 1.0000 | 1.0000 |
| 2 | 123095.375000 | 176383.000000 | 4.3879 | 1.1573 |
| 3 | 323930.125000 | 319762.250000 | 11.5470 | 2.0981 |
| 4 | 325725.625000 | 585395.000000 | 11.6110 | 3.8411 |
| 5 | 807546.875000 | 351568.000000 | 28.7863 | 2.3068 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | 0.00000065 | 0.00003000 | 0.0723 | 3.3359 |
| q2 | 0.00000121 | 0.00003000 | 0.1344 | 3.3408 |
| q3 | 0.00000266 | 0.00003000 | 0.2952 | 3.3385 |
| q4 | 0.00000583 | 0.00003000 | 0.6487 | 3.3553 |

## ETHUSDT

- trades: `117841`
- klines: `360`
- depth snapshots: `8`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- spread bps mean/median/p90: `0.0509` / `0.0509` / `0.0509`
- order-sign autocorr lag1/lag5/lag10: `0.8056` / `0.6907` / `0.5896`
- inter-arrival mean/median/p90 ms: `183.24` / `0.00` / `513.00`
- volatility clustering abs/sq lag1: `0.2365` / `0.0910`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 6.370800 | 19.664138 | 1.0000 | 1.0000 |
| 2 | 0.021325 | 0.359837 | 0.0033 | 0.0183 |
| 3 | 0.356000 | 0.666075 | 0.0559 | 0.0339 |
| 4 | 0.653300 | 2.341425 | 0.1025 | 0.1191 |
| 5 | 0.019013 | 0.007300 | 0.0030 | 0.0004 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | 0.06355231 | 0.12000000 | 0.3232 | 0.6100 |
| q2 | 0.06351809 | 0.11000000 | 0.3230 | 0.5626 |
| q3 | 0.04947454 | 0.11000000 | 0.2514 | 0.5585 |
| q4 | 0.04700625 | 0.11000000 | 0.2388 | 0.5603 |

## SOLUSDT

- trades: `30387`
- klines: `360`
- depth snapshots: `8`
- spread mean/median/p90: `0.010000` / `0.010000` / `0.010000`
- spread bps mean/median/p90: `1.2042` / `1.2042` / `1.2042`
- order-sign autocorr lag1/lag5/lag10: `0.3073` / `0.1378` / `0.0770`
- inter-arrival mean/median/p90 ms: `710.84` / `280.50` / `2002.00`
- volatility clustering abs/sq lag1: `0.2565` / `0.2023`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 404.319625 | 331.238750 | 1.0000 | 1.0000 |
| 2 | 581.669750 | 683.395000 | 1.4386 | 2.0631 |
| 3 | 688.782000 | 1184.964125 | 1.7036 | 3.5774 |
| 4 | 978.360500 | 1260.382250 | 2.4198 | 3.8051 |
| 5 | 1563.383000 | 1335.114750 | 3.8667 | 4.0307 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | -0.00337234 | 0.01000000 | -0.4047 | 1.2148 |
| q2 | -0.00130108 | 0.02000000 | -0.1558 | 2.4062 |
| q3 | -0.00100408 | 0.02000000 | -0.1199 | 2.4064 |
| q4 | 0.00522380 | 0.03000000 | 0.6284 | 3.5902 |

## XRPUSDT

- trades: `21842`
- klines: `360`
- depth snapshots: `8`
- spread mean/median/p90: `0.000100` / `0.000100` / `0.000100`
- spread bps mean/median/p90: `0.7373` / `0.7373` / `0.7373`
- order-sign autocorr lag1/lag5/lag10: `0.3341` / `0.1772` / `0.1207`
- inter-arrival mean/median/p90 ms: `988.81` / `370.00` / `2903.00`
- volatility clustering abs/sq lag1: `0.3393` / `0.2127`

| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |
|---:|---:|---:|---:|---:|
| 1 | 6358.762500 | 42997.637500 | 1.0000 | 1.0000 |
| 2 | 7718.462500 | 33654.375000 | 1.2138 | 0.7827 |
| 3 | 2608.287500 | 45241.350000 | 0.4102 | 1.0522 |
| 4 | 44500.612500 | 69476.800000 | 6.9983 | 1.6158 |
| 5 | 50246.987500 | 52057.937500 | 7.9020 | 1.2107 |

| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |
|---|---:|---:|---:|---:|
| q1 | -0.00002673 | 0.00020000 | -0.1968 | 1.4753 |
| q2 | 0.00001549 | 0.00030000 | 0.1146 | 2.2139 |
| q3 | 0.00003183 | 0.00030000 | 0.2351 | 2.2142 |
| q4 | 0.00008373 | 0.00030000 | 0.6180 | 2.2262 |

