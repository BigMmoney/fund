# Simulator Statistical Review

Each row is a paired comparison over shared seeds or shared regime-seed cells. `aligned_effect` is positive when the left side is better under the declared metric direction.

| Experiment | Left | Right | Metric | Direction | N | Left Mean | Right Mean | Mean Diff | CI95 Diff | Aligned Effect | Cohen's d | Exact p | Left Wins | Right Wins | Ties |
|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| multiseed_core | Immediate-Surrogate | FBA-250ms | surplus_transfer_gap | lower_is_better | 8 | 2.0038 | 0.8338 | 1.1701 | 0.5633 | -1.1701 | -1.4395 | 0.015625 | 1 | 7 | 0 |
| multiseed_core | Adaptive-100-250ms | Immediate-Surrogate | surplus_transfer_gap | lower_is_better | 8 | -0.0342 | 2.0038 | -2.0380 | 0.5645 | 2.0380 | 2.5017 | 0.007812 | 8 | 0 | 0 |
| multiseed_core | Adaptive-100-250ms | Immediate-Surrogate | p99_latency_ms | lower_is_better | 8 | 430.0000 | 10.0000 | 420.0000 | 50.3295 | -420.0000 | -5.7828 | 0.007812 | 0 | 8 | 0 |
| multiseed_core | Policy-LearnedOfflineContextual-100-250ms | Policy-LearnedLinUCB-100-250ms | surplus_transfer_gap | lower_is_better | 8 | 0.5314 | 2.2936 | -1.7623 | 0.5332 | 1.7623 | 2.2905 | 0.007812 | 8 | 0 | 0 |
| multiseed_core | Policy-LearnedOfflineContextual-100-250ms | Policy-LearnedLinUCB-100-250ms | average_price_impact | lower_is_better | 8 | 4.2155 | 5.9168 | -1.7013 | 0.6641 | 1.7013 | 1.7752 | 0.015625 | 7 | 1 | 0 |
| heldout_generalization | learned_fitted_q | learned_linucb | surplus_transfer_gap | lower_is_better | 16 | 2.4481 | 2.2584 | 0.1898 | 0.3194 | -0.1898 | -0.2911 | 0.278137 | 7 | 9 | 0 |
| heldout_generalization | learned_fitted_q | learned_linucb | p99_latency_ms | lower_is_better | 16 | 176.2500 | 197.5000 | -21.2500 | 34.1630 | 21.2500 | 0.3048 | 0.331787 | 7 | 8 | 1 |
| calibrated_protocol | iql | ppo_clip | surplus_transfer_gap | lower_is_better | 16 | 3.9602 | 6.6590 | -2.6987 | 0.2328 | 2.6987 | 5.6808 | 0.000031 | 16 | 0 | 0 |
| calibrated_protocol | iql | ppo_clip | p99_latency_ms | lower_is_better | 16 | 1500.0000 | 6000.0000 | -4500.0000 | 245.0000 | 4500.0000 | 9.0000 | 0.000031 | 16 | 0 | 0 |
