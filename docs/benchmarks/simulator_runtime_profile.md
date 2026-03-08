# Simulator Runtime Profile

Measurement scope: `parameter_hypercube_artifact_generation`

- scenario cells: `108`
- runs per cell: `4`
- steps per run: `125`
- step duration: `10 ms`
- wall time: `17.8690 s`
- total steps: `54000`
- steps/s: `3022.00`
- estimated order events/s: `55572.71`
- estimated fills/s: `25826.18`

Order-event throughput is estimated by summing `mean_orders_per_sec * episode_duration_seconds * runs` across the published hypercube cells and dividing by measured wall-clock time.
