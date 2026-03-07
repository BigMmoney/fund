# Appendix Figures

This appendix collects the repository-hosted figure set that supplements the NeurIPS-track manuscript.

## Core Figures

![Throughput comparison](figures/throughput.svg)

![Latency profile](figures/latency.svg)

![Fairness proxy comparison](figures/fairness.svg)

![Welfare and behavior comparison](figures/welfare.svg)

![Fitted-Q learning curve](figures/fittedq_learning_curve.svg)

![Online DQN learning curve](figures/online_dqn_learning_curve.svg)

![Prioritized Double-DQN learning curve](figures/double_dqn_learning_curve.svg)

![Controller Pareto frontier](figures/pareto.svg)

![Response-surface welfare effects](figures/response_surface_effects.svg)

![Mechanism ablation snapshot](figures/ablation.svg)

![Agent and workload sweep snapshot](figures/agent_sweeps.svg)

![Strategic-agent robustness](figures/strategic_agents.svg)

![Parameter grid p99 heatmap](figures/grid_p99_heatmap.svg)

![Parameter grid arbitrage heatmap](figures/grid_arb_heatmap.svg)

## Parameter Cube Slices

Each slice fixes retail intensity and renders informed intensity against maker quote width.

### Retail x1

![Parameter cube p99 heatmap retail x1](figures/cube_p99_retail1.svg)

![Parameter cube arbitrage heatmap retail x1](figures/cube_arb_retail1.svg)

### Retail x2

![Parameter cube p99 heatmap retail x2](figures/cube_p99_retail2.svg)

![Parameter cube arbitrage heatmap retail x2](figures/cube_arb_retail2.svg)

### Retail x3

![Parameter cube p99 heatmap retail x3](figures/cube_p99_retail3.svg)

![Parameter cube arbitrage heatmap retail x3](figures/cube_arb_retail3.svg)

## Unified Hypercube Slices

Each hypercube slice fixes informed intensity at `x2` and renders retail intensity against maker quote width for a given arbitrage level.

### Arb x0

![Hypercube p99 heatmap arb x0](figures/hyper_p99_arb0.svg)

![Hypercube welfare-gap heatmap arb x0](figures/hyper_welfare_arb0.svg)

### Arb x1

![Hypercube p99 heatmap arb x1](figures/hyper_p99_arb1.svg)

![Hypercube welfare-gap heatmap arb x1](figures/hyper_welfare_arb1.svg)

### Arb x2

![Hypercube p99 heatmap arb x2](figures/hyper_p99_arb2.svg)

![Hypercube welfare-gap heatmap arb x2](figures/hyper_welfare_arb2.svg)

### Arb x3

![Hypercube p99 heatmap arb x3](figures/hyper_p99_arb3.svg)

![Hypercube welfare-gap heatmap arb x3](figures/hyper_welfare_arb3.svg)

## Reading Guide

- `fairness.svg` still shows the queue-advantage and arbitrage-profit proxy layer
- `welfare.svg` adds direct welfare/behavior signals: retail surplus per traded unit and retail adverse-selection rate
- `fittedq_learning_curve.svg` shows the held-out welfare-gap trajectory across fitted-Q Bellman updates
- `online_dqn_learning_curve.svg` shows the held-out online DQN trajectory across training episodes
- `double_dqn_learning_curve.svg` shows the held-out prioritized Double-DQN trajectory and its checkpoint-selection tradeoff
- `pareto.svg` compresses controller trade-offs onto the `p99 latency` vs `surplus-transfer gap` frontier
- `response_surface_effects.svg` shows the top partial-variance contributors from the fitted hypercube response surface
- `strategic_agents.svg` shows that the richer state-dependent population preserves the same welfare-transfer pattern under heavier flow
- `grid_*` isolates arbitrage intensity versus maker quote width
- `cube_*` holds retail intensity fixed and shows how informed-flow intensity and maker quote width reshape p99 and arbitrage-profit proxy
- `hyper_*` folds arbitrage back into the unified sweep and makes the welfare-gap surface visible under the same slice convention
