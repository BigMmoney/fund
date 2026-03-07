# Simulator Parameter Hypercube Response Surface

Seeds: `[101 103 107 109]`

Each fit uses a standardized response surface with main effects and pairwise interactions over `arb`, `retail`, `informed`, and `maker` factors.

## surplus_transfer_gap

- `R^2`: 0.4028
- `RMSE`: 0.5179

| Coefficient | Value |
|---|---:|
| intercept | 1.2773 |
| arb | 0.3942 |
| retail | 0.0862 |
| informed | -0.0076 |
| maker | 0.0993 |
| arb_x_retail | 0.0543 |
| arb_x_informed | -0.0583 |
| arb_x_maker | 0.0087 |
| retail_x_informed | 0.0300 |
| retail_x_maker | 0.0036 |
| informed_x_maker | -0.0294 |

| Effect Group | Partial R^2 |
|---|---:|
| arbitrageur_intensity | 0.3602 |
| retail_intensity | 0.0251 |
| maker_quote_width | 0.0241 |
| informed_intensity | 0.0116 |
| arb_x_informed | 0.0076 |
| arb_x_retail | 0.0066 |
| retail_x_informed | 0.0020 |
| informed_x_maker | 0.0019 |
| arb_x_maker | 0.0002 |
| retail_x_maker | 0.0000 |

## p99_latency_ms

- `R^2`: 0.0882
- `RMSE`: 46.1190

| Coefficient | Value |
|---|---:|
| intercept | 389.3518 |
| arb | -3.6440 |
| retail | 2.0412 |
| informed | -4.3943 |
| maker | 4.3376 |
| arb_x_retail | 3.2711 |
| arb_x_informed | -5.8956 |
| arb_x_maker | 0.8495 |
| retail_x_informed | -7.1528 |
| retail_x_maker | -2.6389 |
| informed_x_maker | -6.7708 |

| Effect Group | Partial R^2 |
|---|---:|
| informed_intensity | 0.0648 |
| retail_intensity | 0.0313 |
| maker_quote_width | 0.0310 |
| arbitrageur_intensity | 0.0255 |
| retail_x_informed | 0.0219 |
| informed_x_maker | 0.0197 |
| arb_x_informed | 0.0149 |
| arb_x_retail | 0.0046 |
| retail_x_maker | 0.0030 |
| arb_x_maker | 0.0003 |

## retail_surplus_per_unit

- `R^2`: 0.7219
- `RMSE`: 0.0730

| Coefficient | Value |
|---|---:|
| intercept | -0.3201 |
| arb | -0.0763 |
| retail | 0.0056 |
| informed | -0.0681 |
| maker | -0.0483 |
| arb_x_retail | 0.0109 |
| arb_x_informed | 0.0247 |
| arb_x_maker | -0.0096 |
| retail_x_informed | 0.0051 |
| retail_x_maker | 0.0107 |
| informed_x_maker | 0.0071 |

| Effect Group | Partial R^2 |
|---|---:|
| arbitrageur_intensity | 0.3466 |
| informed_intensity | 0.2776 |
| maker_quote_width | 0.1352 |
| arb_x_informed | 0.0317 |
| retail_intensity | 0.0152 |
| arb_x_retail | 0.0062 |
| retail_x_maker | 0.0060 |
| arb_x_maker | 0.0048 |
| informed_x_maker | 0.0026 |
| retail_x_informed | 0.0013 |

