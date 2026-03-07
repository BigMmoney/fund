# NeurIPS Track Roadmap TODO

This file is the durable execution backlog for the benchmark + learning paper.
It exists to prevent the main upgrade path from being lost across small iterative commits.

## Current Position

Already completed:

- ledger-aware benchmark environment
- immediate / speed-bump / fixed-batch / adaptive mechanisms
- invariant enforcement and runtime adapter
- offline and online learned baselines
- held-out regime evaluation
- response-surface summary over the unified stress hypercube
- strategic-agent robustness artifact

The next stage is not more breadth. The next stage is making the benchmark harder to dismiss.

## Priority Order

### P0: Realism Moat

- [ ] Build a calibrated market-data pipeline backed by real public market data.
- [ ] Compute stylized facts from real data:
  - spread distribution
  - depth profile
  - order-sign autocorrelation
  - impact curve
  - volatility clustering
  - inter-arrival distribution
- [ ] Add a calibration target bundle for the synthetic generator.
- [ ] Show that the latency-welfare tension persists after calibration.

Acceptance:

- a reproducible script downloads raw market slices
- a reproducible script computes the stylized-fact bundle
- the paper can point to a calibration artifact, not only a synthetic generator
- the initial cross-symbol calibration envelope is stored in-repo and versioned

### P1: Formal Learning Protocol

- [ ] Add PPO baseline.
- [ ] Add one stronger offline-RL baseline:
  - [ ] CQL
  - [ ] IQL
- [ ] Freeze benchmark train / validation / held-out splits.
- [ ] Freeze observation schema, action bundle, and budget.
- [ ] Produce leaderboard-style comparison tables.

Acceptance:

- learned baselines are evaluated under one shared protocol
- learning section reads like benchmark protocol, not case-study collection

### P2: Counterfactual Controls

- [ ] Add matching-only / no-settlement control.
- [ ] Add no-welfare-objective reward control.
- [ ] Add calibrated-synthetic vs replay-like slice control.

Acceptance:

- the paper can show that the main finding depends on the infrastructure-aware setting
- reviewers can see direct counterfactual evidence, not only framing

## Immediate Execution Plan

### Step 1

- [x] Add durable roadmap file
- [x] Add calibration protocol doc
- [x] Add market-data download script
- [x] Add stylized-fact computation script
- [x] Add smoke-test market-data config
- [x] Add multimarket config stub

### Step 2

- [x] Run and store a small real-data smoke artifact
- [x] Run and store a multi-symbol real-data calibration artifact
- [x] Add calibration artifact index to `docs/neurips_track/README.md`
- [x] Define initial target ranges for simulator calibration

### Step 2.5

- [ ] Expand from the current 8-symbol cross-section to a wider universe profile.
- [ ] Add calibration-to-simulator comparison tables once generator parameters are retuned.
- [ ] Re-run the main latency-welfare experiments against the calibrated target envelope.

### Step 3

- [ ] Implement PPO baseline
- [ ] Implement one offline-RL baseline
- [ ] Publish protocol table in manuscript and appendix

## Design Constraints

- Do not chase raw system-speed competition with JAX-LOB.
- Do not keep adding more mechanisms, heuristics, or metrics unless they directly support P0-P2.
- Agent upgrades should remain stateful and stylized, not turn into a full economic society simulator.

## Paper Claim To Preserve

The benchmark should converge toward this claim:

`Infrastructure optimization can systematically conflict with retail welfare under realistic market constraints.`
