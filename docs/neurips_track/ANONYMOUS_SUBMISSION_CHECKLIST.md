# NeurIPS Anonymous Submission Checklist

## Positioning

- Present the paper as a benchmark + learning-evaluation paper.
- Do not present it as a new RL algorithm paper.
- Keep the core claim singular:
  - infrastructure optimization can conflict with retail welfare under real-data-calibrated synthetic market constraints

## Narrative

- Keep the paper organized around three keywords:
  - `constraint-aware`
  - `calibrated`
  - `counterfactual`
- Avoid adding new mechanism names or secondary metrics to the main narrative.

## Artifact Boundary

- `binance_spot_*`: real-data artifacts derived from public Binance Spot downloads
- `simulator_*`: synthetic benchmark outputs
- `simulator_calibration_target_table.*` and `simulator_calibrated_vs_market.*`: mixed calibration artifacts

## Anonymity

- Keep placeholder author information in `docs/neurips_track/arxiv/main.tex`, or replace with anonymous submission text if required by venue.
- Do not include repository URLs in the anonymous PDF.
- Do not include identifying comments or acknowledgements in the paper-facing PDF.

## Learning Protocol Fairness

- State explicitly that `ppo_clip`, `iql`, and `fitted_q` share:
  - the same observation schema
  - the same discrete action bundle
  - the same train / validation / held-out split
  - the same evaluation budget
- State explicitly that they differ only in:
  - learning rule
  - update schedule
  - checkpoint selection

## Calibration Scope

- Describe calibration as `first-pass`.
- Do not describe the environment as a high-fidelity market simulator.
- Use the phrase `real-data-calibrated synthetic market constraints` rather than stronger realism claims.

## Counterfactual Interpretation

- `matching_only`: mechanism simplification control
- `no_settlement`: correctness / admissibility / trust-boundary control
- `no_welfare_reward`: reward-side control

## Final Checks

- Rebuild `docs/neurips_track/arxiv/main.pdf`
- Run:
  - `go test ./simulator -count=1`
- Confirm `git status --short` is empty before packaging
