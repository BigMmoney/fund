# Simulator Parameter Hypercube Summary

Seeds: `[101 103 107 109]`

Primary welfare metrics emphasized in the paper line:

- `retail_surplus_per_unit`
- `retail_adverse_selection_rate`
- `surplus_transfer_gap`

## Main Effects

| Factor | Level | Cells | Orders/s | p99 (ms) | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| arbitrageur_intensity | 0 | 27 | 1748.02 | 381.02 | 0.00 | -0.2326 | 0.4953 | 0.2326 |
| arbitrageur_intensity | 1 | 27 | 1812.10 | 395.37 | 666.17 | -0.2956 | 0.5005 | 1.8780 |
| arbitrageur_intensity | 2 | 27 | 1871.36 | 402.50 | 1524.93 | -0.3826 | 0.5095 | 1.5900 |
| arbitrageur_intensity | 3 | 27 | 1924.27 | 382.96 | 2134.86 | -0.4130 | 0.5070 | 1.4425 |
| retail_intensity | 1 | 36 | 1446.46 | 395.28 | 1027.26 | -0.3479 | 0.4993 | 1.2123 |
| retail_intensity | 2 | 36 | 1842.96 | 384.65 | 1094.30 | -0.3744 | 0.5087 | 1.3546 |
| retail_intensity | 3 | 36 | 2227.40 | 391.46 | 1122.90 | -0.2706 | 0.5012 | 1.2904 |
| informed_intensity | 1 | 36 | 1763.19 | 395.76 | 1105.23 | -0.2125 | 0.4947 | 1.2172 |
| informed_intensity | 2 | 36 | 1839.52 | 393.06 | 1084.90 | -0.3694 | 0.5067 | 1.3353 |
| informed_intensity | 3 | 36 | 1914.10 | 382.57 | 1054.33 | -0.4110 | 0.5078 | 1.3048 |
| maker_quote_width | 1 | 36 | 1838.94 | 391.18 | 1069.47 | -0.2825 | 0.5027 | 1.1495 |
| maker_quote_width | 2 | 36 | 1838.94 | 383.12 | 1093.72 | -0.3029 | 0.4952 | 1.2932 |
| maker_quote_width | 3 | 36 | 1838.94 | 397.08 | 1081.27 | -0.4075 | 0.5113 | 1.4147 |

## High-Low Contrasts

| Factor | Low | High | Delta Orders/s | Delta p99 (ms) | Delta Arb Profit | Delta Retail Surplus | Delta Retail Adverse | Delta Welfare Gap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| maker_quote_width | 1 | 3 | 0.00 | 5.90 | 11.80 | -0.1250 | 0.0085 | 0.2653 |
| arbitrageur_intensity | 0 | 3 | 176.26 | 1.94 | 2134.86 | -0.1804 | 0.0117 | 1.2099 |
| retail_intensity | 1 | 3 | 780.94 | -3.82 | 95.64 | 0.0772 | 0.0018 | 0.0781 |
| informed_intensity | 1 | 3 | 150.91 | -13.19 | -50.90 | -0.1986 | 0.0131 | 0.0876 |

## Retail-Conditioned Arbitrage Effect

Each row reports the average `(arb=3) - (arb=0)` delta at a fixed retail-intensity level, averaged over informed intensity and maker width.

| Retail Level | Delta Orders/s | Delta p99 (ms) | Delta Arb Profit | Delta Retail Surplus | Delta Retail Adverse | Delta Welfare Gap |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 192.26 | -4.17 | 2023.08 | -0.2430 | -0.0074 | 1.1748 |
| 2 | 192.26 | 5.56 | 2157.81 | -0.1816 | 0.0190 | 1.2262 |
| 3 | 144.25 | 4.44 | 2223.69 | -0.1166 | 0.0235 | 1.2287 |
