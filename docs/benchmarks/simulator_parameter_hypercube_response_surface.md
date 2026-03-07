# Simulator Parameter Hypercube Response Surface

Seeds: `[101 103 107 109]`

Each fit uses a standardized response surface with main effects and pairwise interactions over `arb`, `retail`, `informed`, and `maker` factors.

## surplus_transfer_gap

- `R^2`: 0.3495
- `RMSE`: 0.5397

| Coefficient | Value |
|---|---:|
| intercept | 1.2858 |
| arb | 0.3736 |
| retail | 0.0319 |
| informed | 0.0358 |
| maker | 0.1083 |
| arb_x_retail | 0.0237 |
| arb_x_informed | -0.0320 |
| arb_x_maker | 0.0140 |
| retail_x_informed | 0.0030 |
| retail_x_maker | 0.0311 |
| informed_x_maker | 0.0116 |

| Effect Group | Partial R^2 |
|---|---:|
| arbitrageur_intensity | 0.3157 |
| maker_quote_width | 0.0291 |
| retail_intensity | 0.0057 |
| informed_intensity | 0.0055 |
| arb_x_informed | 0.0023 |
| retail_x_maker | 0.0022 |
| arb_x_retail | 0.0013 |
| arb_x_maker | 0.0004 |
| informed_x_maker | 0.0003 |
| retail_x_informed | 0.0000 |

## p99_latency_ms

- `R^2`: 0.1020
- `RMSE`: 38.1974

| Coefficient | Value |
|---|---:|
| intercept | 390.4630 |
| arb | 1.4493 |
| retail | -1.5593 |
| informed | -5.3866 |
| maker | 2.4098 |
| arb_x_retail | 3.2838 |
| arb_x_informed | -9.9148 |
| arb_x_maker | 3.7910 |
| retail_x_informed | 0.1042 |
| retail_x_maker | 0.3819 |
| informed_x_maker | 1.6667 |

| Effect Group | Partial R^2 |
|---|---:|
| informed_intensity | 0.0801 |
| arbitrageur_intensity | 0.0773 |
| arb_x_informed | 0.0605 |
| maker_quote_width | 0.0142 |
| arb_x_maker | 0.0088 |
| retail_intensity | 0.0082 |
| arb_x_retail | 0.0066 |
| informed_x_maker | 0.0017 |
| retail_x_maker | 0.0001 |
| retail_x_informed | 0.0000 |

## retail_surplus_per_unit

- `R^2`: 0.7061
- `RMSE`: 0.0810

| Coefficient | Value |
|---|---:|
| intercept | -0.3310 |
| arb | -0.0702 |
| retail | 0.0315 |
| informed | -0.0811 |
| maker | -0.0510 |
| arb_x_retail | 0.0147 |
| arb_x_informed | 0.0121 |
| arb_x_maker | 0.0025 |
| retail_x_informed | 0.0156 |
| retail_x_maker | -0.0068 |
| informed_x_maker | 0.0034 |

| Effect Group | Partial R^2 |
|---|---:|
| informed_intensity | 0.3121 |
| arbitrageur_intensity | 0.2374 |
| maker_quote_width | 0.1195 |
| retail_intensity | 0.0672 |
| retail_x_informed | 0.0108 |
| arb_x_retail | 0.0097 |
| arb_x_informed | 0.0066 |
| retail_x_maker | 0.0021 |
| informed_x_maker | 0.0005 |
| arb_x_maker | 0.0003 |

