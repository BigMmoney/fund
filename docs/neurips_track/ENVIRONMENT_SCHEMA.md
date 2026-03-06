# Environment Schema

This document defines the benchmark-facing interface for the `simulator/` environment used by the NeurIPS-track manuscript.

## Environment API

The primary control surface is:

- `Reset() AdapterTimestep`
- `Observe() AdapterTimestep`
- `Step(action ControlAction) AdapterTimestep`
- `Metrics() MetricsSnapshot`

The adapter is created through:

```go
adapter := simulator.NewAdapter(cfg)
```

## Observation Schema

`Observation` is the state exposed after each environment step.

| Field | Type | Meaning |
|---|---|---|
| `step` | `int` | Current simulator step. |
| `done` | `bool` | Episode termination flag. |
| `mode` | `MatchingMode` | Active mechanism: immediate, batch, speed bump, adaptive batch. |
| `current_batch_window_steps` | `int` | Active batch window in step units. |
| `speed_bump_steps` | `int` | Configured speed-bump delay in step units. |
| `current_release_cadence_steps` | `int` | Runtime release cadence override in step units. |
| `current_price_aggression_bias` | `int64` | Runtime quote bias used by the action layer. |
| `pending_orders` | `int` | Orders held back for delayed release or batching. |
| `buy_depth` | `int` | Current buy-book depth. |
| `sell_depth` | `int` | Current sell-book depth. |
| `spread` | `int64` | Best ask minus best bid in price ticks. |
| `fundamental` | `int64` | Synthetic latent reference price. |
| `orders_submitted` | `int` | Total submitted orders in the episode. |
| `orders_accepted` | `int` | Total accepted orders in the episode. |
| `fills` | `int` | Total fills in the episode. |
| `risk_rejections` | `int` | Total risk rejections in the episode. |

## Action Schema

`ControlAction` defines the runtime controls accepted by the adapter.

| Field | Type | Meaning |
|---|---|---|
| `target_batch_window_steps` | `*int` | Adaptive batch-window override. Only active in adaptive scenarios. |
| `risk_limit_scale` | `*float64` | Runtime multiplier applied to risk thresholds. |
| `randomize_tie_break` | `*bool` | Toggle for randomized tie-breaking in eligible scenarios. |
| `release_cadence_steps` | `*int` | Runtime release cadence for delayed release paths. |
| `price_aggression_bias` | `*int64` | Quote bias in ticks applied before matching. Positive values cross more aggressively. |

Action availability is scenario-dependent. Query it through `ActionSpec()`.

## ActionSpec Schema

| Field | Type | Meaning |
|---|---|---|
| `supports_batch_window_control` | `bool` | Whether adaptive window overrides are allowed. |
| `min_batch_window_steps` / `max_batch_window_steps` | `int` | Valid range for batch-window actions. |
| `supports_risk_limit_scale` | `bool` | Whether risk scaling is available. |
| `min_risk_limit_scale` / `max_risk_limit_scale` | `float64` | Valid risk-scale range. |
| `supports_tie_break_toggle` | `bool` | Whether randomized tie-break is controllable. |
| `supports_release_cadence_control` | `bool` | Whether delayed-release cadence can be overridden. |
| `min_release_cadence_steps` / `max_release_cadence_steps` | `int` | Valid release-cadence range. |
| `supports_price_aggression_control` | `bool` | Whether quote aggression bias can be set. |
| `min_price_aggression_bias` / `max_price_aggression_bias` | `int64` | Valid price-bias range in ticks. |

## Metrics Schema

`MetricsSnapshot` is the aggregate benchmark state after each step.

| Field | Type | Meaning |
|---|---|---|
| `orders_submitted` | `int` | Total submitted orders. |
| `orders_accepted` | `int` | Total accepted orders. |
| `fills` | `int` | Total fills. |
| `average_spread` | `float64` | Mean quoted spread over the episode. |
| `average_price_impact` | `float64` | Mean execution impact proxy. |
| `queue_priority_advantage` | `float64` | Fairness-adjacent proxy for queue ordering advantage. |
| `latency_arbitrage_profit` | `float64` | Aggregate profit proxy captured by arbitrageurs. |
| `execution_dispersion` | `float64` | Dispersion of fills across agent classes. |
| `negative_balance_violations` | `int` | Count of post-settlement negative states. |
| `conservation_breaches` | `int` | Count of conservation failures. |
| `risk_rejections` | `int` | Count of rejected orders from risk checks. |

## Adapter Timestep Schema

`AdapterTimestep` bundles the benchmark interaction tuple:

- `observation`
- `metrics`
- `reward`
- `done`
- `info`

`info` also carries the applied action, action spec, metric deltas, current runtime control values, and scenario name. This is the object used by the controller baselines and by any downstream policy-learning loop.

## Benchmark Result Schema

`BenchmarkResult` is the per-run artifact written to `docs/benchmarks/`.

Important fields:

- scenario metadata: `name`, `mode`, `batch_window_ms`, `speed_bump_ms`
- adaptive-window summary: `adaptive_window_min_ms`, `adaptive_window_max_ms`, `adaptive_window_mean_ms`
- systems metrics: `orders_per_sec`, `fills_per_sec`, `p50_latency_ms`, `p95_latency_ms`, `p99_latency_ms`
- quality and fairness proxies: `average_spread`, `average_price_impact`, `queue_priority_advantage`, `latency_arbitrage_profit`, `execution_dispersion`
- safety counters: `negative_balance_violations`, `conservation_breaches`, `risk_rejections`

## Current Learned Controller Interface

The current learned policy baseline is `Policy-LearnedLinUCB-100-250ms`.

It operates over the action bundle:

- batch window
- risk scale
- tie-break toggle
- release cadence
- price aggression bias

The current observation features used by the controller are:

- normalized queue depth
- normalized buy/sell imbalance
- spread
- pending orders
- risk rejections
- episode progress

This is still a lightweight learned baseline. It is intended as a benchmark-control reference, not as a claim of state-of-the-art policy learning.
