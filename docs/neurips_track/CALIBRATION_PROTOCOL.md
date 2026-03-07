# Calibration Protocol

This document defines the realism-upgrade path for the NeurIPS benchmark line.

## Goal

Use real public market data to calibrate the synthetic generator against a compact set of stylized facts, then test whether the benchmark's main latency-welfare tension survives under that calibration target.

## Data Source Strategy

Primary source:

- Binance Spot public market-data endpoints

Rationale:

- public, no API key required for market data
- enough depth/trade cadence to extract microstructure-style facts
- easy to reproduce across multiple symbols

This is a calibration source, not a claim that crypto spot markets are identical to all market venues.

## Stylized Facts

The first calibration bundle targets:

1. Spread distribution
2. Depth profile
3. Order-sign autocorrelation
4. Impact curve
5. Volatility clustering
6. Inter-arrival distribution

## Artifact Layout

Raw market data:

- `data/market_calibration/binance_spot/<profile>/<symbol>/agg_trades.json`
- `data/market_calibration/binance_spot/<profile>/<symbol>/klines.json`
- `data/market_calibration/binance_spot/<profile>/<symbol>/depth_snapshots.json`

Computed facts:

- `docs/benchmarks/binance_spot_<profile>_facts.json`
- `docs/benchmarks/binance_spot_<profile>_facts.md`

Current published profiles:

- `smoke`: 60-minute BTC/ETH slice for end-to-end pipeline validation
- `multimarket`: 6-hour 8-symbol slice (`BTCUSDT`, `ETHUSDT`, `SOLUSDT`, `BNBUSDT`, `XRPUSDT`, `ADAUSDT`, `DOGEUSDT`, `AVAXUSDT`) for the first calibration envelope

## Initial Target Envelope

From `docs/benchmarks/binance_spot_multimarket_facts.*`, the first cross-symbol calibration envelope is:

- spread mean range: `0.000010 -> 0.010000`
- order-sign lag-1 range: `0.2831 -> 0.8056`
- inter-arrival mean range: `125.88 ms -> 7808.87 ms`
- abs-return volatility-clustering lag-1 range: `0.0522 -> 0.3892`
- top impact-bucket mean range: `-0.00075385 -> 0.97178892`

These are not final simulator targets. They are the first empirical envelope the synthetic generator must be compared against.

## Calibration Loop

1. Download raw market slices
2. Compute stylized-fact bundle
3. Compare synthetic generator outputs against target ranges
4. Adjust generator parameters
5. Re-run benchmark and check whether:
   - invariants remain zero-breach
   - latency-welfare tension still appears

## Execution Entry Points

- smoke pipeline:
  - `powershell -ExecutionPolicy Bypass -File scripts/run_calibration_pipeline.ps1 -ConfigPath configs/calibration/binance_spot_smoke.json`
- multimarket pipeline:
  - `powershell -ExecutionPolicy Bypass -File scripts/run_calibration_pipeline.ps1 -ConfigPath configs/calibration/binance_spot_multimarket.json`

## Comparison Targets

The synthetic benchmark should not be forced to exactly reproduce every market statistic.
The goal is narrower:

- match the shape or range of the stylized facts well enough to reject the "toy world" critique
- preserve benchmark repeatability and controllable stress surfaces

## Reviewer-Facing Use

This protocol is intended to support three paper claims:

1. the environment is synthetic but calibrated rather than arbitrary
2. the main latency-welfare tradeoff persists after calibration
3. the benchmark is useful precisely because it combines calibration, settlement constraints, mechanism choice, and learning control in one loop
