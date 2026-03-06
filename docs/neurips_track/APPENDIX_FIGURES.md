# Appendix Figures

This appendix collects the repository-hosted figure set that supplements the NeurIPS-track manuscript.

## Core Figures

![Throughput comparison](figures/throughput.svg)

![Latency profile](figures/latency.svg)

![Fairness proxy comparison](figures/fairness.svg)

![Mechanism ablation snapshot](figures/ablation.svg)

![Agent and workload sweep snapshot](figures/agent_sweeps.svg)

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

## Reading Guide

- the `grid_*` figures isolate arbitrage intensity versus maker quote width
- the `cube_*` figures hold retail intensity fixed and show how informed-flow intensity and maker quote width reshape p99 and arbitrage-profit proxies
- the cube slices make it clear that higher retail flow mainly loads throughput and fills, while maker-width changes primarily compress fills and alter latency tails
