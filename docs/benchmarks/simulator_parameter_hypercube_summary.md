# Simulator Parameter Hypercube Summary

Seeds: `[101 103 107 109]`

Primary welfare metrics emphasized in the paper line:

- `retail_surplus_per_unit`
- `retail_adverse_selection_rate`
- `surplus_transfer_gap`

## Main Effects

| Factor | Level | Cells | Orders/s | p99 (ms) | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| arbitrageur_intensity | 0 | 27 | 1748.02 | 384.72 | 0.00 | -0.2190 | 0.4983 | 0.2190 |
| arbitrageur_intensity | 1 | 27 | 1812.10 | 400.74 | 668.57 | -0.2676 | 0.5058 | 1.8095 |
| arbitrageur_intensity | 2 | 27 | 1871.36 | 396.76 | 1518.94 | -0.3876 | 0.5082 | 1.6252 |
| arbitrageur_intensity | 3 | 27 | 1924.27 | 375.19 | 2119.25 | -0.4064 | 0.5033 | 1.4556 |
| retail_intensity | 1 | 36 | 1446.46 | 386.04 | 1014.11 | -0.3281 | 0.5015 | 1.1633 |
| retail_intensity | 2 | 36 | 1842.96 | 390.97 | 1085.97 | -0.3179 | 0.5032 | 1.2941 |
| retail_intensity | 3 | 36 | 2227.40 | 391.04 | 1129.99 | -0.3144 | 0.5069 | 1.3746 |
| informed_intensity | 1 | 36 | 1763.19 | 392.85 | 1109.90 | -0.2123 | 0.5004 | 1.2503 |
| informed_intensity | 2 | 36 | 1839.52 | 393.12 | 1092.15 | -0.3691 | 0.5059 | 1.3501 |
| informed_intensity | 3 | 36 | 1914.10 | 382.08 | 1028.01 | -0.3790 | 0.5054 | 1.2316 |
| maker_quote_width | 1 | 36 | 1838.94 | 386.53 | 1066.86 | -0.2763 | 0.5037 | 1.1393 |
| maker_quote_width | 2 | 36 | 1838.94 | 384.38 | 1094.61 | -0.2895 | 0.4969 | 1.3102 |
| maker_quote_width | 3 | 36 | 1838.94 | 397.15 | 1068.60 | -0.3946 | 0.5110 | 1.3825 |

## High-Low Contrasts

| Factor | Low | High | Delta Orders/s | Delta p99 (ms) | Delta Arb Profit | Delta Retail Surplus | Delta Retail Adverse | Delta Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| informed_intensity | 1 | 3 | 150.91 | -10.76 | -81.89 | -0.1667 | 0.0050 | -0.0186 |
| maker_quote_width | 1 | 3 | 0.00 | 10.62 | 1.74 | -0.1183 | 0.0073 | 0.2433 |
| arbitrageur_intensity | 0 | 3 | 176.26 | -9.54 | 2119.25 | -0.1874 | 0.0050 | 1.2366 |
| retail_intensity | 1 | 3 | 780.94 | 5.00 | 115.88 | 0.0137 | 0.0055 | 0.2113 |

## Retail-Conditioned Arbitrage Effect

Each row reports the average `(arb=3) - (arb=0)` delta at a fixed retail-intensity level, averaged over informed intensity and maker width.

| Retail Level | Delta Orders/s | Delta p99 (ms) | Delta Arb Profit | Delta Retail Surplus | Delta Retail Adverse | Delta Welfare Gap |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 192.26 | -12.50 | 1979.53 | -0.2298 | -0.0056 | 1.0785 |
| 2 | 192.26 | -20.28 | 2138.22 | -0.1832 | 0.0042 | 1.2607 |
| 3 | 144.25 | 4.17 | 2240.00 | -0.1493 | 0.0164 | 1.3707 |
