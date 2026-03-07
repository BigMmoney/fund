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

The paper-facing benchmark claim now emphasizes three primary welfare metrics:

- `retail_surplus_per_unit`
- `retail_adverse_selection_rate`
- `surplus_transfer_gap`

`arbitrageur_surplus_per_unit` and `welfare_dispersion` remain available as secondary diagnostics.

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
| `retail_surplus_per_unit` | `float64` | Realized per-unit ex post surplus for retail flow against the synthetic fundamental. |
| `arbitrageur_surplus_per_unit` | `float64` | Realized per-unit ex post surplus for arbitrageur flow. |
| `retail_adverse_selection_rate` | `float64` | Fraction of retail traded units executed at negative ex post surplus. |
| `welfare_dispersion` | `float64` | Dispersion of per-unit surplus across active agent classes. |
| `surplus_transfer_gap` | `float64` | Arbitrageur per-unit surplus minus retail per-unit surplus. |
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
- quality, fairness, and welfare signals: `average_spread`, `average_price_impact`, `queue_priority_advantage`, `latency_arbitrage_profit`, `execution_dispersion`, `retail_surplus_per_unit`, `arbitrageur_surplus_per_unit`, `retail_adverse_selection_rate`, `welfare_dispersion`, `surplus_transfer_gap`
- safety counters: `negative_balance_violations`, `conservation_breaches`, `risk_rejections`

The manuscript and README center their controller and mechanism comparison on:

- latency / fills
- `retail_surplus_per_unit`
- `retail_adverse_selection_rate`
- `surplus_transfer_gap`

The remaining fairness and welfare fields are still part of the artifact layer and can support downstream analyses, but they are treated as secondary in the current paper line.

## Current Learned Controller Interface

The current learned policy baselines are:

- `Policy-LearnedLinUCB-100-250ms`
- `Policy-LearnedTinyMLP-100-250ms`
- `Policy-LearnedOfflineContextual-100-250ms`

All three operate over the action bundle:

- batch window
- risk scale
- tie-break toggle
- release cadence
- price aggression bias

The current observation features used by the learned controllers are:

- normalized queue depth
- normalized buy/sell imbalance
- spread
- pending orders
- risk rejections
- episode progress

`Policy-LearnedLinUCB-100-250ms` uses a contextual linear bandit over discrete action bundles.

`Policy-LearnedTinyMLP-100-250ms` uses a small two-layer policy network with a burst-aware supervised warm-start and gradient-based policy updates over the same discrete action set.

`Policy-LearnedOfflineContextual-100-250ms` uses an offline contextual value model fit to logged rollouts from burst-aware, LinUCB, TinyMLP, and random behavior policies, then acts greedily over the same discrete action set.

These are still lightweight learned baselines. They are intended as benchmark-control references, not as claims of state-of-the-art policy learning.
