package simulator

import (
	"encoding/json"
	"fmt"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

type protocolSurface struct {
	Name                    string
	ObservationFeatureNames []string
	FeatureFn               func(Observation) []float64
	ActionFn                func(ActionSpec) []learnedActionCandidate
	BCHiddenDim             int
	PPOHiddenDim            int
	IQLHiddenDim            int
	DoubleDQNHiddenDim      int
	DoubleDQNEpisodes       int
}

type protocolSurfaceSummary struct {
	Name                    string   `json:"name"`
	ObservationFeatureCount int      `json:"observation_feature_count"`
	ObservationFeatureNames []string `json:"observation_feature_names"`
	ActionCount             int      `json:"action_count"`
	ActionNames             []string `json:"action_names"`
}

type protocolSurfaceDiagnostics struct {
	Surface              string                       `json:"surface"`
	BehaviorClone        behaviorCloneTrainingSummary `json:"behavior_clone"`
	PPOTrace             []ppoTrainingSnapshot        `json:"ppo_trace"`
	IQL                  iqlTrainingSummary           `json:"iql"`
	DoubleDQNTrace       []ppoTrainingSnapshot        `json:"double_dqn_trace"`
	DoubleDQNBestEpisode int                          `json:"double_dqn_best_episode"`
	DoubleDQNBestScore   float64                      `json:"double_dqn_best_score"`
	DoubleDQNFinalScore  float64                      `json:"double_dqn_final_score"`
}

type richerProtocolResult struct {
	Surface                        string  `json:"surface"`
	Policy                         string  `json:"policy"`
	Split                          string  `json:"split"`
	RegimeCount                    int     `json:"regime_count"`
	Runs                           int     `json:"runs"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	CI95OrdersPerSec               float64 `json:"ci95_orders_per_sec"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	CI95FillsPerSec                float64 `json:"ci95_fills_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	CI95P99LatencyMs               float64 `json:"ci95_p99_latency_ms"`
	MeanAveragePriceImpact         float64 `json:"mean_average_price_impact"`
	CI95AveragePriceImpact         float64 `json:"ci95_average_price_impact"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	CI95RetailSurplusPerUnit       float64 `json:"ci95_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	CI95RetailAdverseSelectionRate float64 `json:"ci95_retail_adverse_selection_rate"`
	MeanSurplusTransferGap         float64 `json:"mean_surplus_transfer_gap"`
	CI95SurplusTransferGap         float64 `json:"ci95_surplus_transfer_gap"`
	NegativeBalanceViolationsTotal int     `json:"negative_balance_violations_total"`
	ConservationBreachesTotal      int     `json:"conservation_breaches_total"`
	BenchmarkScore                 float64 `json:"benchmark_score"`
	Rank                           int     `json:"rank"`
}

type richerProtocolAgreementRow struct {
	Surface              string  `json:"surface"`
	Split                string  `json:"split"`
	ReferencePolicy      string  `json:"reference_policy"`
	LeftPolicy           string  `json:"left_policy"`
	RightPolicy          string  `json:"right_policy"`
	Samples              int     `json:"samples"`
	ExactActionAgreement float64 `json:"exact_action_agreement"`
	LeftUniqueActions    int     `json:"left_unique_actions"`
	RightUniqueActions   int     `json:"right_unique_actions"`
	LeftActionEntropy    float64 `json:"left_action_entropy"`
	RightActionEntropy   float64 `json:"right_action_entropy"`
}

type richerProtocolArtifact struct {
	Surfaces      []protocolSurfaceSummary     `json:"surfaces"`
	Diagnostics   []protocolSurfaceDiagnostics `json:"diagnostics"`
	Results       []richerProtocolResult       `json:"results"`
	AgreementRows []richerProtocolAgreementRow `json:"agreement_rows"`
}

type protocolTransferRow struct {
	TrainVariant                   string  `json:"train_variant"`
	EvalVariant                    string  `json:"eval_variant"`
	Policy                         string  `json:"policy"`
	Runs                           int     `json:"runs"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	CI95OrdersPerSec               float64 `json:"ci95_orders_per_sec"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	CI95FillsPerSec                float64 `json:"ci95_fills_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	CI95P99LatencyMs               float64 `json:"ci95_p99_latency_ms"`
	MeanAveragePriceImpact         float64 `json:"mean_average_price_impact"`
	CI95AveragePriceImpact         float64 `json:"ci95_average_price_impact"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	CI95RetailSurplusPerUnit       float64 `json:"ci95_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	CI95RetailAdverseSelectionRate float64 `json:"ci95_retail_adverse_selection_rate"`
	MeanSurplusTransferGap         float64 `json:"mean_surplus_transfer_gap"`
	CI95SurplusTransferGap         float64 `json:"ci95_surplus_transfer_gap"`
	NegativeBalanceViolationsTotal int     `json:"negative_balance_violations_total"`
	ConservationBreachesTotal      int     `json:"conservation_breaches_total"`
	BenchmarkScore                 float64 `json:"benchmark_score"`
	Rank                           int     `json:"rank"`
}

type protocolTransferShift struct {
	ShiftType             string   `json:"shift_type"`
	LeftGroup             string   `json:"left_group"`
	RightGroup            string   `json:"right_group"`
	CommonPolicies        []string `json:"common_policies"`
	KendallTau            float64  `json:"kendall_tau"`
	PoliciesWithRankShift int      `json:"policies_with_rank_shift"`
}

type protocolTransferArtifact struct {
	Rows        []protocolTransferRow   `json:"rows"`
	EvalShifts  []protocolTransferShift `json:"eval_shifts"`
	TrainShifts []protocolTransferShift `json:"train_shifts"`
}

type metricSetSpec struct {
	Name                string  `json:"name"`
	FillsWeight         float64 `json:"fills_weight"`
	P99Weight           float64 `json:"p99_weight"`
	RetailSurplusWeight float64 `json:"retail_surplus_weight"`
	RetailAdverseWeight float64 `json:"retail_adverse_weight"`
	SurplusGapWeight    float64 `json:"surplus_gap_weight"`
}

type metricRobustnessRow struct {
	TrainVariant string  `json:"train_variant"`
	EvalVariant  string  `json:"eval_variant"`
	MetricSet    string  `json:"metric_set"`
	Policy       string  `json:"policy"`
	Score        float64 `json:"score"`
	Rank         int     `json:"rank"`
}

type metricRobustnessShift struct {
	ShiftType             string   `json:"shift_type"`
	LeftGroup             string   `json:"left_group"`
	RightGroup            string   `json:"right_group"`
	MetricSet             string   `json:"metric_set,omitempty"`
	EvalVariant           string   `json:"eval_variant,omitempty"`
	CommonPolicies        []string `json:"common_policies"`
	KendallTau            float64  `json:"kendall_tau"`
	PoliciesWithRankShift int      `json:"policies_with_rank_shift"`
}

type metricRobustnessArtifact struct {
	MetricSets   []metricSetSpec         `json:"metric_sets"`
	Rows         []metricRobustnessRow   `json:"rows"`
	EvalShifts   []metricRobustnessShift `json:"eval_shifts"`
	MetricShifts []metricRobustnessShift `json:"metric_shifts"`
}

type doubleDQNTrainingSummary struct {
	Episodes              int     `json:"episodes"`
	BestValidationEpisode int     `json:"best_validation_episode"`
	BestValidationScore   float64 `json:"best_validation_score"`
	FinalValidationScore  float64 `json:"final_validation_score"`
}

func TestGenerateSimulatorRicherProtocolArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_RICHER_PROTOCOL") != "1" {
		t.Skip("set RUN_SIM_RICHER_PROTOCOL=1 to generate richer protocol artifacts")
	}

	cfg := neuripsDBProtocolConfig()
	base := calibratedAdaptiveProtocolBaseScenario()
	validationRegimes := neuripsDBValidationRegimes(base)
	heldOutRegimes := neuripsDBHeldOutRegimes(base)
	surfaces := []protocolSurface{defaultProtocolSurface(), richerProtocolSurface()}

	artifact := richerProtocolArtifact{
		Surfaces:      make([]protocolSurfaceSummary, 0, len(surfaces)),
		Diagnostics:   make([]protocolSurfaceDiagnostics, 0, len(surfaces)),
		Results:       make([]richerProtocolResult, 0, len(surfaces)*12),
		AgreementRows: make([]richerProtocolAgreementRow, 0, len(surfaces)*6),
	}

	for _, surface := range surfaces {
		artifact.Surfaces = append(artifact.Surfaces, summarizeProtocolSurface(surface, base, cfg.RewardWeights))
		bcModel, bcSummary := trainProtocolBehaviorCloneForSurface(base, cfg, validationRegimes, surface)
		ppoTrace, ppoModel := trainProtocolPPOForSurface(base, cfg, validationRegimes, surface)
		iqlModel, iqlSummary := trainProtocolIQLForSurface(base, cfg, validationRegimes, surface)
		doubleTrace, doubleBest, doubleFinal, doubleSummary := trainProtocolDoubleDQNForSurface(base, cfg, validationRegimes, surface)
		artifact.Diagnostics = append(artifact.Diagnostics, protocolSurfaceDiagnostics{
			Surface:              surface.Name,
			BehaviorClone:        bcSummary,
			PPOTrace:             ppoTrace,
			IQL:                  iqlSummary,
			DoubleDQNTrace:       doubleTrace,
			DoubleDQNBestEpisode: doubleSummary.BestValidationEpisode,
			DoubleDQNBestScore:   doubleSummary.BestValidationScore,
			DoubleDQNFinalScore:  doubleSummary.FinalValidationScore,
		})

		artifact.Results = append(artifact.Results,
			summarizeProtocolRunsForSurface(surface.Name, "burst_aware", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, burstAwareChooser()),
			summarizeProtocolRunsForSurface(surface.Name, "behavior_clone", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(bcModel, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "ppo_clip", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(ppoModel, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "iql", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(iqlModel, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "double_dqn_best", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, doubleDQNChooserWithFeatureFn(doubleBest, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "double_dqn_final", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, doubleDQNChooserWithFeatureFn(doubleFinal, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "burst_aware", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, burstAwareChooser()),
			summarizeProtocolRunsForSurface(surface.Name, "behavior_clone", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(bcModel, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "ppo_clip", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(ppoModel, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "iql", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(iqlModel, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "double_dqn_best", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, doubleDQNChooserWithFeatureFn(doubleBest, surface.FeatureFn)),
			summarizeProtocolRunsForSurface(surface.Name, "double_dqn_final", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, doubleDQNChooserWithFeatureFn(doubleFinal, surface.FeatureFn)),
		)

		validationObs := collectSharedObservations(validationRegimes, cfg.ValidationSeeds, burstAwareChooser())
		heldOutObs := collectSharedObservations(heldOutRegimes, cfg.HeldOutSeeds, burstAwareChooser())
		choosers := map[string]func(ActionSpec, Observation) ControlAction{
			"behavior_clone":  tinyChooserWithFeatureFn(bcModel, surface.FeatureFn),
			"iql":             tinyChooserWithFeatureFn(iqlModel, surface.FeatureFn),
			"double_dqn_best": doubleDQNChooserWithFeatureFn(doubleBest, surface.FeatureFn),
		}
		pairs := [][2]string{{"behavior_clone", "iql"}, {"behavior_clone", "double_dqn_best"}, {"iql", "double_dqn_best"}}
		artifact.AgreementRows = append(artifact.AgreementRows,
			buildRicherAgreementRows(surface.Name, "validation", "burst_aware", validationObs, choosers, pairs)...,
		)
		artifact.AgreementRows = append(artifact.AgreementRows,
			buildRicherAgreementRows(surface.Name, "heldout", "burst_aware", heldOutObs, choosers, pairs)...,
		)
	}

	assignRicherProtocolScoresAndRanks(artifact.Results)
	if err := writeSimulatorRicherProtocolArtifacts(artifact); err != nil {
		t.Fatalf("write richer protocol artifacts: %v", err)
	}
}

func TestGenerateSimulatorProtocolTransferArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_PROTOCOL_TRANSFER") != "1" {
		t.Skip("set RUN_SIM_PROTOCOL_TRANSFER=1 to generate protocol transfer artifacts")
	}
	artifact := buildProtocolTransferArtifact()
	if err := writeSimulatorProtocolTransferArtifacts(artifact); err != nil {
		t.Fatalf("write protocol transfer artifacts: %v", err)
	}
}

func TestGenerateSimulatorMetricRobustnessMatrixArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_METRIC_MATRIX") != "1" {
		t.Skip("set RUN_SIM_METRIC_MATRIX=1 to generate metric-robustness matrix artifacts")
	}
	transfer := buildProtocolTransferArtifact()
	artifact := buildMetricRobustnessArtifact(transfer)
	if err := writeSimulatorMetricRobustnessArtifacts(artifact); err != nil {
		t.Fatalf("write metric-robustness matrix artifacts: %v", err)
	}
}

func neuripsDBProtocolConfig() calibratedProtocolConfig {
	cfg := defaultCalibratedProtocolConfig()
	cfg.TrainSeeds = []int64{1103, 1109, 1117}
	cfg.ValidationSeeds = []int64{1129}
	cfg.HeldOutSeeds = []int64{1153, 1163}
	cfg.BCEpochs = 8
	cfg.PPOEpisodes = 48
	cfg.PPOPolicyEpochs = 2
	cfg.IQLIterations = 4
	return cfg
}

func neuripsDBValidationRegimes(base ScenarioConfig) []ScenarioConfig {
	regimes := counterfactualValidationRegimes(base)
	if len(regimes) > 2 {
		regimes = regimes[:2]
	}
	return append([]ScenarioConfig(nil), regimes...)
}

func neuripsDBHeldOutRegimes(base ScenarioConfig) []ScenarioConfig {
	regimes := counterfactualHeldOutRegimes(base)
	if len(regimes) > 2 {
		regimes = regimes[:2]
	}
	return append([]ScenarioConfig(nil), regimes...)
}

func defaultProtocolSurface() protocolSurface {
	return protocolSurface{
		Name:                    "matched_7x6",
		ObservationFeatureNames: []string{"bias", "queue_depth", "imbalance_abs", "spread", "pending", "risk", "progress"},
		FeatureFn:               observationFeatures,
		ActionFn:                candidateBanditActions,
		BCHiddenDim:             10,
		PPOHiddenDim:            12,
		IQLHiddenDim:            10,
		DoubleDQNHiddenDim:      14,
		DoubleDQNEpisodes:       80,
	}
}

func richerProtocolSurface() protocolSurface {
	return protocolSurface{
		Name: "richer_14x10",
		ObservationFeatureNames: []string{
			"bias", "buy_depth", "sell_depth", "imbalance_signed", "queue_depth", "spread", "pending", "risk", "batch_window", "release_cadence", "price_aggression", "accepted", "fills", "progress",
		},
		FeatureFn:          richerObservationFeatures,
		ActionFn:           richerCandidateBanditActions,
		BCHiddenDim:        14,
		PPOHiddenDim:       16,
		IQLHiddenDim:       14,
		DoubleDQNHiddenDim: 18,
		DoubleDQNEpisodes:  96,
	}
}

func richerObservationFeatures(observation Observation) []float64 {
	queueDepth := float64(observation.BuyDepth + observation.SellDepth + observation.PendingOrders)
	imbalanceSigned := float64(observation.BuyDepth-observation.SellDepth) / 8.0
	batchWindow := float64(observation.CurrentBatchWindowStep) / 12.0
	releaseCadence := float64(observation.CurrentReleaseCadence) / 12.0
	priceAggression := float64(observation.CurrentPriceAggression) / 2.0
	progress := 0.0
	if observation.Step > 0 {
		progress = float64(observation.Step) / 125.0
	}
	return []float64{1.0, float64(observation.BuyDepth) / 8.0, float64(observation.SellDepth) / 8.0, imbalanceSigned, queueDepth / 16.0, float64(observation.Spread) / 4.0, float64(observation.PendingOrders) / 8.0, float64(observation.RiskRejections) / 4.0, batchWindow, releaseCadence, priceAggression, float64(observation.OrdersAccepted) / 8.0, float64(observation.Fills) / 8.0, progress}
}

func richerCandidateBanditActions(spec ActionSpec) []learnedActionCandidate {
	minWindow := spec.MinBatchWindowSteps
	midWindow := minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+10)
	fastWindow := minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+3)
	slowWindow := minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+14)
	maxWindow := spec.MaxBatchWindowSteps
	releaseMid := minInt(spec.MaxReleaseCadenceSteps, maxInt(0, minWindow+5))
	releaseSlow := minInt(spec.MaxReleaseCadenceSteps, maxInt(releaseMid, minWindow+8))
	releaseMax := minInt(spec.MaxReleaseCadenceSteps, maxWindow)
	return []learnedActionCandidate{
		{Name: "fast_passive", Action: makeAction(&minWindow, floatPtr(0.85), boolPtr(false), intPtr(0), int64Ptr(-1))},
		{Name: "balanced_mid", Action: makeAction(&midWindow, floatPtr(1.00), boolPtr(true), intPtr(releaseMid), int64Ptr(0))},
		{Name: "fair_delay", Action: makeAction(&maxWindow, floatPtr(0.95), boolPtr(true), intPtr(releaseMax), int64Ptr(0))},
		{Name: "aggressive_fast", Action: makeAction(&fastWindow, floatPtr(1.10), boolPtr(true), intPtr(0), int64Ptr(1))},
		{Name: "latency_tail_guard", Action: makeAction(intPtr(minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+5)), floatPtr(0.95), boolPtr(false), intPtr(releaseMid), int64Ptr(0))},
		{Name: "pressure_release", Action: makeAction(&midWindow, floatPtr(1.05), boolPtr(true), intPtr(releaseMax), int64Ptr(1))},
		{Name: "price_improve_fair", Action: makeAction(&slowWindow, floatPtr(0.92), boolPtr(true), intPtr(releaseSlow), int64Ptr(-1))},
		{Name: "maker_soft", Action: makeAction(&midWindow, floatPtr(0.90), boolPtr(false), intPtr(releaseMid), int64Ptr(-2))},
		{Name: "microburst_capture", Action: makeAction(&minWindow, floatPtr(1.15), boolPtr(true), intPtr(0), int64Ptr(2))},
		{Name: "deep_stability", Action: makeAction(&maxWindow, floatPtr(0.85), boolPtr(false), intPtr(releaseMax), int64Ptr(0))},
	}
}

func summarizeProtocolSurface(surface protocolSurface, base ScenarioConfig, weights RewardWeights) protocolSurfaceSummary {
	spec := NewAdapterWithRewardWeights(base, weights).ActionSpec()
	actions := surface.ActionFn(spec)
	names := make([]string, 0, len(actions))
	for _, action := range actions {
		names = append(names, action.Name)
	}
	return protocolSurfaceSummary{Name: surface.Name, ObservationFeatureCount: len(surface.FeatureFn(Observation{})), ObservationFeatureNames: append([]string(nil), surface.ObservationFeatureNames...), ActionCount: len(actions), ActionNames: names}
}

func chooseTinyMLPActionWithFeatureFn(spec ActionSpec, observation Observation, model tinyMLPModel, featureFn func(Observation) []float64) ControlAction {
	bestIdx := argmaxFloats(qValuesFromTinyMLP(model, featureFn(observation)))
	if bestIdx < 0 || bestIdx >= len(model.Actions) {
		return fallbackBanditAction(spec)
	}
	return model.Actions[bestIdx].Action
}

func tinyChooserWithFeatureFn(model tinyMLPModel, featureFn func(Observation) []float64) func(ActionSpec, Observation) ControlAction {
	return func(spec ActionSpec, observation Observation) ControlAction {
		return chooseTinyMLPActionWithFeatureFn(spec, observation, model, featureFn)
	}
}

func chooseOnlineDQNActionWithFeatureFn(spec ActionSpec, observation Observation, policy learnedOnlineDQNPolicy, featureFn func(Observation) []float64) ControlAction {
	if len(policy.Model.Actions) == 0 {
		return fallbackBanditAction(spec)
	}
	qValues := qValuesFromTinyMLP(policy.Model, featureFn(observation))
	bestIdx := argmaxFloats(qValues)
	if bestIdx < 0 || bestIdx >= len(policy.Model.Actions) {
		return fallbackBanditAction(spec)
	}
	return policy.Model.Actions[bestIdx].Action
}

func doubleDQNChooserWithFeatureFn(policy learnedDoubleDQNPolicy, featureFn func(Observation) []float64) func(ActionSpec, Observation) ControlAction {
	return func(spec ActionSpec, observation Observation) ControlAction {
		return chooseOnlineDQNActionWithFeatureFn(spec, observation, learnedOnlineDQNPolicy{Model: policy.Model}, featureFn)
	}
}
func collectBurstAwareDatasetForSurface(cfg ScenarioConfig, actions []learnedActionCandidate, seeds []int64, featureFn func(Observation) []float64) ([][]float64, []int) {
	features := make([][]float64, 0, len(seeds)*cfg.TotalSteps)
	labels := make([]int, 0, len(seeds)*cfg.TotalSteps)
	for _, seed := range seeds {
		trainingCfg := cfg
		trainingCfg.Seed = seed
		adapter := NewAdapter(trainingCfg)
		timestep := adapter.Reset()
		spec := adapter.ActionSpec()
		for !timestep.Done {
			target := burstAwareAction(spec, timestep.Observation)
			features = append(features, featureFn(timestep.Observation))
			labels = append(labels, nearestCandidateIndex(spec, target, actions))
			timestep = adapter.Step(target)
		}
	}
	return features, labels
}

func collectProtocolBehaviorCloneDatasetForSurface(base ScenarioConfig, seeds []int64, actions []learnedActionCandidate, rng *rand.Rand, featureFn func(Observation) []float64) ([][]float64, []int, []string) {
	linucb := cachedLinUCBPolicy(base)
	tiny := cachedTinyMLPPolicy(base)
	offline := cachedOfflineContextualPolicy(base)
	behaviors := []PolicyController{PolicyBurstAware, PolicyLearnedLinUCB, PolicyLearnedTinyMLP, PolicyLearnedOfflineContextual}
	features := make([][]float64, 0, len(seeds)*base.TotalSteps*(len(behaviors)+1))
	labels := make([]int, 0, len(seeds)*base.TotalSteps*(len(behaviors)+1))
	sources := make([]string, 0, len(behaviors)+1)
	for _, behavior := range behaviors {
		sources = append(sources, string(behavior))
		for _, seed := range seeds {
			trainingCfg := base
			trainingCfg.Seed = seed
			adapter := NewAdapter(trainingCfg)
			spec := adapter.ActionSpec()
			timestep := adapter.Reset()
			for !timestep.Done {
				var action ControlAction
				switch behavior {
				case PolicyBurstAware:
					action = burstAwareAction(spec, timestep.Observation)
				case PolicyLearnedLinUCB:
					action = chooseLinUCBAction(spec, timestep.Observation, linucb)
				case PolicyLearnedTinyMLP:
					action = chooseTinyMLPAction(spec, timestep.Observation, tiny)
				case PolicyLearnedOfflineContextual:
					action = chooseOfflineContextualAction(spec, timestep.Observation, offline)
				default:
					action = actions[rng.Intn(len(actions))].Action
				}
				features = append(features, append([]float64(nil), featureFn(timestep.Observation)...))
				labels = append(labels, nearestCandidateIndex(spec, action, actions))
				timestep = adapter.Step(action)
			}
		}
	}
	if len(actions) > 6 {
		sources = append(sources, "random_explore")
		for _, seed := range seeds {
			trainingCfg := base
			trainingCfg.Seed = seed + 7001
			adapter := NewAdapter(trainingCfg)
			timestep := adapter.Reset()
			for !timestep.Done {
				actionIdx := rng.Intn(len(actions))
				features = append(features, append([]float64(nil), featureFn(timestep.Observation)...))
				labels = append(labels, actionIdx)
				timestep = adapter.Step(actions[actionIdx].Action)
			}
		}
	}
	return features, labels, sources
}

func trainProtocolBehaviorCloneForSurface(base ScenarioConfig, cfg calibratedProtocolConfig, validationRegimes []ScenarioConfig, surface protocolSurface) (tinyMLPModel, behaviorCloneTrainingSummary) {
	spec := NewAdapterWithRewardWeights(base, cfg.RewardWeights).ActionSpec()
	actions := surface.ActionFn(spec)
	rng := rand.New(rand.NewSource(trainingRandomSeed(base) + 2103 + int64(len(actions))))
	model := initTinyMLPModel(actions, len(surface.FeatureFn(Observation{})), surface.BCHiddenDim, rng)
	features, labels, sources := collectProtocolBehaviorCloneDatasetForSurface(base, cfg.TrainSeeds, actions, rng, surface.FeatureFn)
	trainTinyMLPSupervised(&model, features, labels, cfg.BCEpochs, 0.022, 1e-4)
	summary := behaviorCloneTrainingSummary{Epochs: cfg.BCEpochs, TrainSamples: len(labels), BehaviorSources: sources, ValidationScore: evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(model, surface.FeatureFn))}
	return model, summary
}

func trainProtocolPPOForSurface(base ScenarioConfig, cfg calibratedProtocolConfig, validationRegimes []ScenarioConfig, surface protocolSurface) ([]ppoTrainingSnapshot, tinyMLPModel) {
	spec := NewAdapterWithRewardWeights(base, cfg.RewardWeights).ActionSpec()
	actions := surface.ActionFn(spec)
	rng := rand.New(rand.NewSource(trainingRandomSeed(base) + 2603 + int64(len(actions))))
	model := initTinyMLPModel(actions, len(surface.FeatureFn(Observation{})), surface.PPOHiddenDim, rng)
	supervisedFeatures, supervisedLabels := collectBurstAwareDatasetForSurface(base, actions, cfg.TrainSeeds, surface.FeatureFn)
	trainTinyMLPSupervised(&model, supervisedFeatures, supervisedLabels, 10, 0.025, 1e-4)

	bestModel := copyTinyMLPModel(model)
	bestScore := math.Inf(-1)
	recentRewards := make([]float64, 0, 10)
	trace := make([]ppoTrainingSnapshot, 0, cfg.PPOEpisodes/10+2)
	trace = append(trace, ppoTrainingSnapshot{Episode: 0, MeanTrainReward: 0, ValidationScore: evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(model, surface.FeatureFn))})

	for episode := 1; episode <= cfg.PPOEpisodes; episode++ {
		trainingCfg := base
		seedBase := cfg.TrainSeeds[(episode-1)%len(cfg.TrainSeeds)]
		seedOffset := int64((episode - 1) / len(cfg.TrainSeeds))
		trainingCfg.Seed = seedBase + seedOffset*1009
		adapter := NewAdapterWithRewardWeights(trainingCfg, cfg.RewardWeights)
		timestep := adapter.Reset()
		trajectory := make([]policyStepSample, 0, trainingCfg.TotalSteps)
		for !timestep.Done {
			features := surface.FeatureFn(timestep.Observation)
			hidden, _, probs := forwardTinyMLP(model, features)
			actionIdx := sampleCategorical(probs, rng)
			next := adapter.Step(model.Actions[actionIdx].Action)
			trajectory = append(trajectory, policyStepSample{features: append([]float64(nil), features...), hidden: append([]float64(nil), hidden...), probs: append([]float64(nil), probs...), action: actionIdx, reward: next.Reward})
			timestep = next
		}
		returns := discountedReturns(trajectory, 0.97)
		meanAdv, stdAdv := meanStd(returns)
		for epoch := 0; epoch < cfg.PPOPolicyEpochs; epoch++ {
			for idx, sample := range trajectory {
				hidden, _, probs := forwardTinyMLP(model, sample.features)
				advantage := returns[idx] - meanAdv
				if stdAdv > 1e-9 {
					advantage /= stdAdv
				}
				advantage = clampFloat(advantage, -4, 4)
				oldProb := maxFloat(sample.probs[sample.action], 1e-6)
				newProb := maxFloat(probs[sample.action], 1e-6)
				ratio := newProb / oldProb
				scale := advantage
				if advantage >= 0 && ratio > 1+cfg.PPOClipEpsilon {
					scale = 0
				}
				if advantage < 0 && ratio < 1-cfg.PPOClipEpsilon {
					scale = 0
				}
				dlogits := make([]float64, len(probs))
				for out := range probs {
					dlogits[out] = -probs[out] * scale
				}
				dlogits[sample.action] += scale
				applyTinyMLPGradients(&model, sample.features, hidden, dlogits, 0.010, 1e-4)
			}
		}
		recentRewards = append(recentRewards, meanFloatSlice(returns))
		if len(recentRewards) > 10 {
			recentRewards = recentRewards[1:]
		}
		if episode%10 == 0 || episode == cfg.PPOEpisodes {
			score := evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(model, surface.FeatureFn))
			trace = append(trace, ppoTrainingSnapshot{Episode: episode, MeanTrainReward: meanFloatSlice(recentRewards), ValidationScore: score})
			if score > bestScore {
				bestScore = score
				bestModel = copyTinyMLPModel(model)
			}
		}
	}
	return trace, bestModel
}

func collectOfflineTransitionTrajectoryWithSurface(cfg ScenarioConfig, seed int64, weights RewardWeights, behavior PolicyController, actions []learnedActionCandidate, linucb *learnedLinUCBPolicy, tiny *tinyMLPModel, offline *learnedOfflineContextualPolicy, rng *rand.Rand, featureFn func(Observation) []float64) []offlineTransitionSample {
	trainingCfg := cfg
	trainingCfg.Seed = seed
	adapter := NewAdapterWithRewardWeights(trainingCfg, weights)
	spec := adapter.ActionSpec()
	timestep := adapter.Reset()
	trajectory := make([]offlineTransitionSample, 0, trainingCfg.TotalSteps)
	for !timestep.Done {
		var action ControlAction
		switch behavior {
		case PolicyBurstAware:
			action = burstAwareAction(spec, timestep.Observation)
		case PolicyLearnedLinUCB:
			action = chooseLinUCBAction(spec, timestep.Observation, *linucb)
		case PolicyLearnedTinyMLP:
			action = chooseTinyMLPAction(spec, timestep.Observation, *tiny)
		case PolicyLearnedOfflineContextual:
			action = chooseOfflineContextualAction(spec, timestep.Observation, *offline)
		default:
			action = actions[rng.Intn(len(actions))].Action
		}
		actionIdx := nearestCandidateIndex(spec, action, actions)
		features := featureFn(timestep.Observation)
		next := adapter.Step(action)
		trajectory = append(trajectory, offlineTransitionSample{features: append([]float64(nil), features...), action: actionIdx, reward: next.Reward, nextFeatures: append([]float64(nil), featureFn(next.Observation)...), done: next.Done})
		timestep = next
	}
	return trajectory
}

func collectOfflineRandomTransitionTrajectoryWithSurface(cfg ScenarioConfig, seed int64, weights RewardWeights, actions []learnedActionCandidate, rng *rand.Rand, featureFn func(Observation) []float64) []offlineTransitionSample {
	trainingCfg := cfg
	trainingCfg.Seed = seed
	adapter := NewAdapterWithRewardWeights(trainingCfg, weights)
	timestep := adapter.Reset()
	trajectory := make([]offlineTransitionSample, 0, trainingCfg.TotalSteps)
	for !timestep.Done {
		actionIdx := rng.Intn(len(actions))
		features := featureFn(timestep.Observation)
		next := adapter.Step(actions[actionIdx].Action)
		trajectory = append(trajectory, offlineTransitionSample{features: append([]float64(nil), features...), action: actionIdx, reward: next.Reward, nextFeatures: append([]float64(nil), featureFn(next.Observation)...), done: next.Done})
		timestep = next
	}
	return trajectory
}
func trainProtocolIQLForSurface(base ScenarioConfig, cfg calibratedProtocolConfig, validationRegimes []ScenarioConfig, surface protocolSurface) (tinyMLPModel, iqlTrainingSummary) {
	spec := NewAdapterWithRewardWeights(base, cfg.RewardWeights).ActionSpec()
	actions := surface.ActionFn(spec)
	rng := rand.New(rand.NewSource(trainingRandomSeed(base) + 3203 + int64(len(actions))))
	actor := initTinyMLPModel(actions, len(surface.FeatureFn(Observation{})), surface.IQLHiddenDim, rng)
	supervisedFeatures, supervisedLabels := collectBurstAwareDatasetForSurface(base, actions, cfg.TrainSeeds, surface.FeatureFn)
	trainTinyMLPSupervised(&actor, supervisedFeatures, supervisedLabels, 8, 0.020, 1e-4)

	linucb := cachedLinUCBPolicy(base)
	tiny := cachedTinyMLPPolicy(base)
	offline := cachedOfflineContextualPolicy(base)
	transitions := make([]offlineTransitionSample, 0, len(cfg.TrainSeeds)*base.TotalSteps*8)
	for _, seed := range cfg.TrainSeeds {
		for _, behavior := range []PolicyController{PolicyBurstAware, PolicyLearnedLinUCB, PolicyLearnedTinyMLP, PolicyLearnedOfflineContextual} {
			transitions = append(transitions, collectOfflineTransitionTrajectoryWithSurface(base, seed, cfg.RewardWeights, behavior, actions, &linucb, &tiny, &offline, rng, surface.FeatureFn)...)
		}
		transitions = append(transitions, collectOfflineRandomTransitionTrajectoryWithSurface(base, seed+4049, cfg.RewardWeights, actions, rng, surface.FeatureFn)...)
	}
	qModels := initLinearArmModels(actions, len(surface.FeatureFn(Observation{})), 2.0)
	valueModel := iqlValueModel{Weights: make([]float64, len(surface.FeatureFn(Observation{})))}
	summary := iqlTrainingSummary{Iterations: cfg.IQLIterations, Expectile: cfg.IQLExpectile, Beta: cfg.IQLBeta}
	for iter := 0; iter < cfg.IQLIterations; iter++ {
		weighted := make([]weightedOfflineLabel, 0, len(transitions))
		for _, transition := range transitions {
			stateQ := make([]float64, len(qModels))
			for idx := range qModels {
				stateQ[idx] = dot(qModels[idx].Theta, transition.features)
			}
			targetV := expectileValue(stateQ, cfg.IQLExpectile)
			valuePred := dot(valueModel.Weights, transition.features)
			deltaV := targetV - valuePred
			for idx, feature := range transition.features {
				valueModel.Weights[idx] += 0.020 * deltaV * feature
			}
			target := transition.reward
			if !transition.done {
				nextV := dot(valueModel.Weights, transition.nextFeatures)
				target += 0.97 * nextV
			}
			qsa := dot(qModels[transition.action].Theta, transition.features)
			err := clampFloat(target-qsa, -6, 6)
			arm := &qModels[transition.action]
			arm.A = outerAdd(arm.A, transition.features)
			addScaledInPlace(arm.B, transition.features, qsa+err)
			arm.Theta = solveLinearSystem(arm.A, arm.B)
			arm.Updates++
			advantage := clampFloat((qsa-valuePred)/cfg.IQLBeta, -4, 4)
			weight := math.Exp(advantage)
			if weight > 25 {
				weight = 25
			}
			weighted = append(weighted, weightedOfflineLabel{features: transition.features, label: transition.action, weight: weight})
		}
		trainTinyMLPWeightedClassification(&actor, weighted, 1, 0.012, 1e-4)
	}
	summary.ValidationScore = evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooserWithFeatureFn(actor, surface.FeatureFn))
	return actor, summary
}

func trainProtocolDoubleDQNForSurface(base ScenarioConfig, cfg calibratedProtocolConfig, validationRegimes []ScenarioConfig, surface protocolSurface) ([]ppoTrainingSnapshot, learnedDoubleDQNPolicy, learnedDoubleDQNPolicy, doubleDQNTrainingSummary) {
	spec := NewAdapterWithRewardWeights(base, cfg.RewardWeights).ActionSpec()
	actions := surface.ActionFn(spec)
	inputDim := len(surface.FeatureFn(Observation{}))
	rng := rand.New(rand.NewSource(trainingRandomSeed(base) + 5107 + int64(len(actions))))
	model := initTinyMLPModel(actions, inputDim, surface.DoubleDQNHiddenDim, rng)
	targetModel := copyTinyMLPModel(model)
	policy := learnedDoubleDQNPolicy{Model: copyTinyMLPModel(model), Gamma: 0.97, Episodes: surface.DoubleDQNEpisodes, TrainingSeeds: append([]int64(nil), cfg.TrainSeeds...), HeldOutSeeds: append([]int64(nil), cfg.HeldOutSeeds...), HeldOutRegimes: scenarioNames(neuripsDBHeldOutRegimes(base)), PrioritizedReplay: true, TargetMixTau: 0.08}
	bestPolicy := learnedDoubleDQNPolicy{Model: copyTinyMLPModel(model), Gamma: policy.Gamma, Episodes: policy.Episodes, TrainingSeeds: append([]int64(nil), policy.TrainingSeeds...), HeldOutSeeds: append([]int64(nil), policy.HeldOutSeeds...), HeldOutRegimes: append([]string(nil), policy.HeldOutRegimes...), PrioritizedReplay: true, TargetMixTau: policy.TargetMixTau}
	bestScore := evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, doubleDQNChooserWithFeatureFn(bestPolicy, surface.FeatureFn))
	bestEpisode := 0
	trace := []ppoTrainingSnapshot{{Episode: 0, MeanTrainReward: 0, ValidationScore: bestScore}}
	replay := make([]dqnReplaySample, 0, 8000)
	recentRewards := make([]float64, 0, 20)
	for episode := 1; episode <= policy.Episodes; episode++ {
		seedBase := cfg.TrainSeeds[(episode-1)%len(cfg.TrainSeeds)]
		seedOffset := int64((episode - 1) / len(cfg.TrainSeeds))
		trainingCfg := base
		trainingCfg.Seed = seedBase + seedOffset*1297
		adapter := NewAdapterWithRewardWeights(trainingCfg, cfg.RewardWeights)
		timestep := adapter.Reset()
		episodeReward := 0.0
		epsilon := 0.20 - (0.17 * float64(episode-1) / float64(maxInt(policy.Episodes-1, 1)))
		if epsilon < 0.02 {
			epsilon = 0.02
		}
		for !timestep.Done {
			features := surface.FeatureFn(timestep.Observation)
			actionIdx := 0
			if rng.Float64() < epsilon {
				actionIdx = rng.Intn(len(model.Actions))
			} else {
				actionIdx = argmaxFloats(qValuesFromTinyMLP(model, features))
			}
			next := adapter.Step(model.Actions[actionIdx].Action)
			replay = append(replay, dqnReplaySample{features: append([]float64(nil), features...), action: actionIdx, reward: next.Reward, nextFeatures: append([]float64(nil), surface.FeatureFn(next.Observation)...), done: next.Done, priority: 1})
			if len(replay) > 8000 {
				replay = replay[len(replay)-8000:]
			}
			episodeReward += next.Reward
			if len(replay) >= 64 {
				for update := 0; update < 6; update++ {
					idx := samplePrioritizedReplayIndex(replay, rng)
					sample := replay[idx]
					target := sample.reward
					if !sample.done {
						nextAction := argmaxFloats(qValuesFromTinyMLP(model, sample.nextFeatures))
						targetValues := qValuesFromTinyMLP(targetModel, sample.nextFeatures)
						target += policy.Gamma * targetValues[nextAction]
					}
					target = clampFloat(target, -35, 35)
					tdErr := applyTinyMLPQUpdate(&model, sample.features, sample.action, target, 0.0030, 1e-4)
					replay[idx].priority = math.Abs(tdErr) + 0.05
				}
				polyakMixTinyMLP(&targetModel, model, policy.TargetMixTau)
			}
			timestep = next
		}
		recentRewards = append(recentRewards, episodeReward)
		if len(recentRewards) > 20 {
			recentRewards = recentRewards[1:]
		}
		if episode%20 == 0 || episode == policy.Episodes {
			snapshotPolicy := learnedDoubleDQNPolicy{Model: copyTinyMLPModel(model), Gamma: policy.Gamma, Episodes: policy.Episodes, TrainingSeeds: append([]int64(nil), policy.TrainingSeeds...), HeldOutSeeds: append([]int64(nil), policy.HeldOutSeeds...), HeldOutRegimes: append([]string(nil), policy.HeldOutRegimes...), PrioritizedReplay: true, TargetMixTau: policy.TargetMixTau}
			score := evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, doubleDQNChooserWithFeatureFn(snapshotPolicy, surface.FeatureFn))
			trace = append(trace, ppoTrainingSnapshot{Episode: episode, MeanTrainReward: meanFloatSlice(recentRewards), ValidationScore: score})
			if score > bestScore {
				bestScore = score
				bestEpisode = episode
				bestPolicy = snapshotPolicy
			}
		}
	}
	finalPolicy := learnedDoubleDQNPolicy{Model: copyTinyMLPModel(model), Gamma: policy.Gamma, Episodes: policy.Episodes, TrainingSeeds: append([]int64(nil), policy.TrainingSeeds...), HeldOutSeeds: append([]int64(nil), policy.HeldOutSeeds...), HeldOutRegimes: append([]string(nil), policy.HeldOutRegimes...), PrioritizedReplay: true, TargetMixTau: policy.TargetMixTau}
	finalScore := evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, doubleDQNChooserWithFeatureFn(finalPolicy, surface.FeatureFn))
	return trace, bestPolicy, finalPolicy, doubleDQNTrainingSummary{Episodes: policy.Episodes, BestValidationEpisode: bestEpisode, BestValidationScore: bestScore, FinalValidationScore: finalScore}
}

func summarizeProtocolRunsForSurface(surface, policy, split string, regimes []ScenarioConfig, seeds []int64, rewardWeights RewardWeights, chooser func(ActionSpec, Observation) ControlAction) richerProtocolResult {
	runs := runChooserAcrossRegimes(regimes, seeds, rewardWeights, chooser, policy)
	orders := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.OrdersPerSec })
	fills := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.FillsPerSec })
	p99Vals := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.P99LatencyMs })
	impact := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.AveragePriceImpact })
	surplus := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit })
	adverse := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate })
	gap := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })
	_, ordersStd := meanStd(orders)
	_, fillsStd := meanStd(fills)
	_, p99Std := meanStd(p99Vals)
	_, impactStd := meanStd(impact)
	_, surplusStd := meanStd(surplus)
	_, adverseStd := meanStd(adverse)
	_, gapStd := meanStd(gap)
	return richerProtocolResult{Surface: surface, Policy: policy, Split: split, RegimeCount: len(regimes), Runs: len(runs), MeanOrdersPerSec: meanFloatSlice(orders), CI95OrdersPerSec: ci95HalfWidth(ordersStd, len(orders)), MeanFillsPerSec: meanFloatSlice(fills), CI95FillsPerSec: ci95HalfWidth(fillsStd, len(fills)), MeanP99LatencyMs: meanFloatSlice(p99Vals), CI95P99LatencyMs: ci95HalfWidth(p99Std, len(p99Vals)), MeanAveragePriceImpact: meanFloatSlice(impact), CI95AveragePriceImpact: ci95HalfWidth(impactStd, len(impact)), MeanRetailSurplusPerUnit: meanFloatSlice(surplus), CI95RetailSurplusPerUnit: ci95HalfWidth(surplusStd, len(surplus)), MeanRetailAdverseSelectionRate: meanFloatSlice(adverse), CI95RetailAdverseSelectionRate: ci95HalfWidth(adverseStd, len(adverse)), MeanSurplusTransferGap: meanFloatSlice(gap), CI95SurplusTransferGap: ci95HalfWidth(gapStd, len(gap)), NegativeBalanceViolationsTotal: sumIntMetric(runs, func(result BenchmarkResult) int { return result.NegativeBalanceViolations }), ConservationBreachesTotal: sumIntMetric(runs, func(result BenchmarkResult) int { return result.ConservationBreaches })}
}

func assignRicherProtocolScoresAndRanks(rows []richerProtocolResult) {
	groups := make(map[string][]*richerProtocolResult)
	for idx := range rows {
		key := rows[idx].Surface + "|" + rows[idx].Split
		groups[key] = append(groups[key], &rows[idx])
	}
	for _, group := range groups {
		assignRicherGroupScores(group)
	}
}

func assignRicherGroupScores(group []*richerProtocolResult) {
	fills := make([]float64, 0, len(group))
	p99 := make([]float64, 0, len(group))
	surplus := make([]float64, 0, len(group))
	adverse := make([]float64, 0, len(group))
	gap := make([]float64, 0, len(group))
	for _, row := range group {
		fills = append(fills, row.MeanFillsPerSec)
		p99 = append(p99, row.MeanP99LatencyMs)
		surplus = append(surplus, row.MeanRetailSurplusPerUnit)
		adverse = append(adverse, row.MeanRetailAdverseSelectionRate)
		gap = append(gap, row.MeanSurplusTransferGap)
	}
	fillMean, fillStd := meanStd(fills)
	p99Mean, p99Std := meanStd(p99)
	surplusMean, surplusStd := meanStd(surplus)
	adverseMean, adverseStd := meanStd(adverse)
	gapMean, gapStd := meanStd(gap)
	for _, row := range group {
		row.BenchmarkScore = zScore(row.MeanFillsPerSec, fillMean, fillStd) - zScore(row.MeanP99LatencyMs, p99Mean, p99Std) + zScore(row.MeanRetailSurplusPerUnit, surplusMean, surplusStd) - zScore(row.MeanRetailAdverseSelectionRate, adverseMean, adverseStd) - zScore(row.MeanSurplusTransferGap, gapMean, gapStd)
	}
	sort.Slice(group, func(i, j int) bool {
		if math.Abs(group[i].BenchmarkScore-group[j].BenchmarkScore) <= 1e-9 {
			return group[i].Policy < group[j].Policy
		}
		return group[i].BenchmarkScore > group[j].BenchmarkScore
	})
	currentRank := 0
	for idx, row := range group {
		if idx == 0 || math.Abs(row.BenchmarkScore-group[idx-1].BenchmarkScore) > 1e-9 {
			currentRank = idx + 1
		}
		row.Rank = currentRank
	}
}

func buildRicherAgreementRows(surface string, split string, reference string, observations []sharedEvalObservation, choosers map[string]func(ActionSpec, Observation) ControlAction, pairs [][2]string) []richerProtocolAgreementRow {
	cache := make(map[string][]int, len(choosers))
	for name, chooser := range choosers {
		cache[name] = chooserActionIndicesForSurface(surface, observations, chooser)
	}
	rows := make([]richerProtocolAgreementRow, 0, len(pairs))
	for _, pair := range pairs {
		leftEntropy, leftUnique := actionEntropy(cache[pair[0]])
		rightEntropy, rightUnique := actionEntropy(cache[pair[1]])
		rows = append(rows, richerProtocolAgreementRow{
			Surface:              surface,
			Split:                split,
			ReferencePolicy:      reference,
			LeftPolicy:           pair[0],
			RightPolicy:          pair[1],
			Samples:              len(observations),
			ExactActionAgreement: exactActionAgreement(cache[pair[0]], cache[pair[1]]),
			LeftUniqueActions:    leftUnique,
			RightUniqueActions:   rightUnique,
			LeftActionEntropy:    leftEntropy,
			RightActionEntropy:   rightEntropy,
		})
	}
	return rows
}

func chooserActionIndicesForSurface(surface string, observations []sharedEvalObservation, chooser func(ActionSpec, Observation) ControlAction) []int {
	indices := make([]int, 0, len(observations))
	for _, item := range observations {
		actions := surfaceActionCandidates(surface, item.Spec)
		action := chooser(item.Spec, item.Observation)
		indices = append(indices, nearestCandidateIndex(item.Spec, action, actions))
	}
	return indices
}

func surfaceActionCandidates(surface string, spec ActionSpec) []learnedActionCandidate {
	if surface == richerProtocolSurface().Name {
		return richerCandidateBanditActions(spec)
	}
	return candidateBanditActions(spec)
}

func buildProtocolTransferArtifact() protocolTransferArtifact {
	cfg := neuripsDBProtocolConfig()
	surface := defaultProtocolSurface()
	fullBase := calibratedAdaptiveProtocolBaseScenario()
	matchingBase := calibratedMatchingOnlyScenario()
	settlementBase := calibratedNoSettlementScenario()

	trainVariants := []struct {
		Name          string
		Base          ScenarioConfig
		Validation    []ScenarioConfig
		RewardWeights RewardWeights
	}{
		{Name: "full_protocol", Base: fullBase, Validation: neuripsDBValidationRegimes(fullBase), RewardWeights: cfg.RewardWeights},
		{Name: "matching_only", Base: matchingBase, Validation: neuripsDBValidationRegimes(matchingBase), RewardWeights: cfg.RewardWeights},
		{Name: "no_settlement", Base: settlementBase, Validation: neuripsDBValidationRegimes(settlementBase), RewardWeights: cfg.RewardWeights},
		{Name: "no_welfare_reward", Base: fullBase, Validation: neuripsDBValidationRegimes(fullBase), RewardWeights: noWelfareRewardWeights()},
	}
	evalVariants := []struct {
		Name          string
		HeldOut       []ScenarioConfig
		RewardWeights RewardWeights
	}{
		{Name: "full_protocol", HeldOut: neuripsDBHeldOutRegimes(fullBase), RewardWeights: cfg.RewardWeights},
		{Name: "matching_only", HeldOut: neuripsDBHeldOutRegimes(matchingBase), RewardWeights: cfg.RewardWeights},
		{Name: "no_settlement", HeldOut: neuripsDBHeldOutRegimes(settlementBase), RewardWeights: cfg.RewardWeights},
	}

	artifact := protocolTransferArtifact{Rows: make([]protocolTransferRow, 0, len(trainVariants)*len(evalVariants)*5)}
	for _, trainVariant := range trainVariants {
		variantCfg := cfg
		variantCfg.RewardWeights = trainVariant.RewardWeights
		bcModel, _ := trainProtocolBehaviorCloneForSurface(trainVariant.Base, variantCfg, trainVariant.Validation, surface)
		_, ppoModel := trainProtocolPPOForSurface(trainVariant.Base, variantCfg, trainVariant.Validation, surface)
		iqlModel, _ := trainProtocolIQLForSurface(trainVariant.Base, variantCfg, trainVariant.Validation, surface)
		_, doubleBest, doubleFinal, _ := trainProtocolDoubleDQNForSurface(trainVariant.Base, variantCfg, trainVariant.Validation, surface)
		policies := map[string]func(ActionSpec, Observation) ControlAction{
			"behavior_clone":   tinyChooserWithFeatureFn(bcModel, surface.FeatureFn),
			"ppo_clip":         tinyChooserWithFeatureFn(ppoModel, surface.FeatureFn),
			"iql":              tinyChooserWithFeatureFn(iqlModel, surface.FeatureFn),
			"double_dqn_best":  doubleDQNChooserWithFeatureFn(doubleBest, surface.FeatureFn),
			"double_dqn_final": doubleDQNChooserWithFeatureFn(doubleFinal, surface.FeatureFn),
		}
		for _, evalVariant := range evalVariants {
			for policyName, chooser := range policies {
				artifact.Rows = append(artifact.Rows, summarizeProtocolTransferRuns(trainVariant.Name, evalVariant.Name, policyName, evalVariant.HeldOut, cfg.HeldOutSeeds, evalVariant.RewardWeights, chooser))
			}
		}
	}
	assignProtocolTransferScoresAndRanks(artifact.Rows)
	artifact.EvalShifts = buildProtocolTransferShifts(artifact.Rows, "eval")
	artifact.TrainShifts = buildProtocolTransferShifts(artifact.Rows, "train")
	return artifact
}

func summarizeProtocolTransferRuns(trainVariant, evalVariant, policy string, regimes []ScenarioConfig, seeds []int64, rewardWeights RewardWeights, chooser func(ActionSpec, Observation) ControlAction) protocolTransferRow {
	runs := runChooserAcrossRegimes(regimes, seeds, rewardWeights, chooser, policy)
	orders := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.OrdersPerSec })
	fills := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.FillsPerSec })
	p99Vals := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.P99LatencyMs })
	impact := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.AveragePriceImpact })
	surplus := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit })
	adverse := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate })
	gap := collectBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })
	_, ordersStd := meanStd(orders)
	_, fillsStd := meanStd(fills)
	_, p99Std := meanStd(p99Vals)
	_, impactStd := meanStd(impact)
	_, surplusStd := meanStd(surplus)
	_, adverseStd := meanStd(adverse)
	_, gapStd := meanStd(gap)
	return protocolTransferRow{TrainVariant: trainVariant, EvalVariant: evalVariant, Policy: policy, Runs: len(runs), MeanOrdersPerSec: meanFloatSlice(orders), CI95OrdersPerSec: ci95HalfWidth(ordersStd, len(orders)), MeanFillsPerSec: meanFloatSlice(fills), CI95FillsPerSec: ci95HalfWidth(fillsStd, len(fills)), MeanP99LatencyMs: meanFloatSlice(p99Vals), CI95P99LatencyMs: ci95HalfWidth(p99Std, len(p99Vals)), MeanAveragePriceImpact: meanFloatSlice(impact), CI95AveragePriceImpact: ci95HalfWidth(impactStd, len(impact)), MeanRetailSurplusPerUnit: meanFloatSlice(surplus), CI95RetailSurplusPerUnit: ci95HalfWidth(surplusStd, len(surplus)), MeanRetailAdverseSelectionRate: meanFloatSlice(adverse), CI95RetailAdverseSelectionRate: ci95HalfWidth(adverseStd, len(adverse)), MeanSurplusTransferGap: meanFloatSlice(gap), CI95SurplusTransferGap: ci95HalfWidth(gapStd, len(gap)), NegativeBalanceViolationsTotal: sumIntMetric(runs, func(result BenchmarkResult) int { return result.NegativeBalanceViolations }), ConservationBreachesTotal: sumIntMetric(runs, func(result BenchmarkResult) int { return result.ConservationBreaches })}
}

func assignProtocolTransferScoresAndRanks(rows []protocolTransferRow) {
	groups := make(map[string][]*protocolTransferRow)
	for idx := range rows {
		key := rows[idx].TrainVariant + "|" + rows[idx].EvalVariant
		groups[key] = append(groups[key], &rows[idx])
	}
	for _, group := range groups {
		assignProtocolTransferGroupScores(group)
	}
}

func assignProtocolTransferGroupScores(group []*protocolTransferRow) {
	fills := make([]float64, 0, len(group))
	p99 := make([]float64, 0, len(group))
	surplus := make([]float64, 0, len(group))
	adverse := make([]float64, 0, len(group))
	gap := make([]float64, 0, len(group))
	for _, row := range group {
		fills = append(fills, row.MeanFillsPerSec)
		p99 = append(p99, row.MeanP99LatencyMs)
		surplus = append(surplus, row.MeanRetailSurplusPerUnit)
		adverse = append(adverse, row.MeanRetailAdverseSelectionRate)
		gap = append(gap, row.MeanSurplusTransferGap)
	}
	fillMean, fillStd := meanStd(fills)
	p99Mean, p99Std := meanStd(p99)
	surplusMean, surplusStd := meanStd(surplus)
	adverseMean, adverseStd := meanStd(adverse)
	gapMean, gapStd := meanStd(gap)
	for _, row := range group {
		row.BenchmarkScore = zScore(row.MeanFillsPerSec, fillMean, fillStd) - zScore(row.MeanP99LatencyMs, p99Mean, p99Std) + zScore(row.MeanRetailSurplusPerUnit, surplusMean, surplusStd) - zScore(row.MeanRetailAdverseSelectionRate, adverseMean, adverseStd) - zScore(row.MeanSurplusTransferGap, gapMean, gapStd)
	}
	sort.Slice(group, func(i, j int) bool {
		if math.Abs(group[i].BenchmarkScore-group[j].BenchmarkScore) <= 1e-9 {
			return group[i].Policy < group[j].Policy
		}
		return group[i].BenchmarkScore > group[j].BenchmarkScore
	})
	currentRank := 0
	for idx, row := range group {
		if idx == 0 || math.Abs(row.BenchmarkScore-group[idx-1].BenchmarkScore) > 1e-9 {
			currentRank = idx + 1
		}
		row.Rank = currentRank
	}
}

func buildProtocolTransferShifts(rows []protocolTransferRow, axis string) []protocolTransferShift {
	shifts := make([]protocolTransferShift, 0, 12)
	groupRows := make(map[string][]protocolTransferRow)
	for _, row := range rows {
		key := row.TrainVariant + "|" + row.EvalVariant
		groupRows[key] = append(groupRows[key], row)
	}
	if axis == "eval" {
		comparisons := []string{"matching_only", "no_settlement"}
		trainSeen := make(map[string]struct{})
		for _, row := range rows {
			trainSeen[row.TrainVariant] = struct{}{}
		}
		trainNames := make([]string, 0, len(trainSeen))
		for name := range trainSeen {
			trainNames = append(trainNames, name)
		}
		sort.Strings(trainNames)
		for _, trainName := range trainNames {
			leftKey := trainName + "|full_protocol"
			leftRanks := make(map[string]int, len(groupRows[leftKey]))
			for _, row := range groupRows[leftKey] {
				leftRanks[row.Policy] = row.Rank
			}
			for _, evalName := range comparisons {
				rightKey := trainName + "|" + evalName
				rightRanks := make(map[string]int, len(groupRows[rightKey]))
				for _, row := range groupRows[rightKey] {
					rightRanks[row.Policy] = row.Rank
				}
				common := intersectPolicies(leftRanks, rightRanks)
				if len(common) == 0 {
					continue
				}
				shiftCount := 0
				for _, policy := range common {
					if leftRanks[policy] != rightRanks[policy] {
						shiftCount++
					}
				}
				shifts = append(shifts, protocolTransferShift{ShiftType: "eval", LeftGroup: fmt.Sprintf("train=%s|eval=full_protocol", trainName), RightGroup: fmt.Sprintf("train=%s|eval=%s", trainName, evalName), CommonPolicies: common, KendallTau: kendallTauFromRanks(common, leftRanks, rightRanks), PoliciesWithRankShift: shiftCount})
			}
		}
	} else {
		comparisons := []string{"matching_only", "no_settlement", "no_welfare_reward"}
		evalSeen := make(map[string]struct{})
		for _, row := range rows {
			evalSeen[row.EvalVariant] = struct{}{}
		}
		evalNames := make([]string, 0, len(evalSeen))
		for name := range evalSeen {
			evalNames = append(evalNames, name)
		}
		sort.Strings(evalNames)
		for _, evalName := range evalNames {
			leftKey := "full_protocol|" + evalName
			leftRanks := make(map[string]int, len(groupRows[leftKey]))
			for _, row := range groupRows[leftKey] {
				leftRanks[row.Policy] = row.Rank
			}
			for _, trainName := range comparisons {
				rightKey := trainName + "|" + evalName
				rightRanks := make(map[string]int, len(groupRows[rightKey]))
				for _, row := range groupRows[rightKey] {
					rightRanks[row.Policy] = row.Rank
				}
				common := intersectPolicies(leftRanks, rightRanks)
				if len(common) == 0 {
					continue
				}
				shiftCount := 0
				for _, policy := range common {
					if leftRanks[policy] != rightRanks[policy] {
						shiftCount++
					}
				}
				shifts = append(shifts, protocolTransferShift{ShiftType: "train", LeftGroup: fmt.Sprintf("train=full_protocol|eval=%s", evalName), RightGroup: fmt.Sprintf("train=%s|eval=%s", trainName, evalName), CommonPolicies: common, KendallTau: kendallTauFromRanks(common, leftRanks, rightRanks), PoliciesWithRankShift: shiftCount})
			}
		}
	}
	sort.Slice(shifts, func(i, j int) bool {
		if shifts[i].ShiftType == shifts[j].ShiftType {
			if shifts[i].LeftGroup == shifts[j].LeftGroup {
				return shifts[i].RightGroup < shifts[j].RightGroup
			}
			return shifts[i].LeftGroup < shifts[j].LeftGroup
		}
		return shifts[i].ShiftType < shifts[j].ShiftType
	})
	return shifts
}

func intersectPolicies(leftRanks, rightRanks map[string]int) []string {
	common := make([]string, 0, minInt(len(leftRanks), len(rightRanks)))
	for policy := range leftRanks {
		if _, ok := rightRanks[policy]; ok {
			common = append(common, policy)
		}
	}
	sort.Strings(common)
	return common
}

func buildMetricRobustnessArtifact(transfer protocolTransferArtifact) metricRobustnessArtifact {
	metricSets := []metricSetSpec{
		{Name: "canonical", FillsWeight: 1.0, P99Weight: 1.0, RetailSurplusWeight: 1.0, RetailAdverseWeight: 1.0, SurplusGapWeight: 1.0},
		{Name: "latency_focused", FillsWeight: 1.0, P99Weight: 1.5, RetailSurplusWeight: 0.5, RetailAdverseWeight: 0.5, SurplusGapWeight: 0.5},
		{Name: "welfare_focused", FillsWeight: 0.5, P99Weight: 0.5, RetailSurplusWeight: 1.5, RetailAdverseWeight: 1.25, SurplusGapWeight: 1.5},
		{Name: "market_quality", FillsWeight: 0.75, P99Weight: 1.0, RetailSurplusWeight: 1.0, RetailAdverseWeight: 1.0, SurplusGapWeight: 1.25},
	}
	artifact := metricRobustnessArtifact{MetricSets: metricSets, Rows: make([]metricRobustnessRow, 0, len(transfer.Rows)*len(metricSets)), EvalShifts: make([]metricRobustnessShift, 0, 24), MetricShifts: make([]metricRobustnessShift, 0, 24)}
	for _, metricSet := range metricSets {
		artifact.Rows = append(artifact.Rows, scoreProtocolTransferRows(transfer.Rows, metricSet)...)
	}
	artifact.EvalShifts = buildMetricEvaluatorShifts(artifact.Rows)
	artifact.MetricShifts = buildMetricSetShifts(artifact.Rows)
	return artifact
}

func scoreProtocolTransferRows(transferRows []protocolTransferRow, metricSet metricSetSpec) []metricRobustnessRow {
	groups := make(map[string][]protocolTransferRow)
	for _, row := range transferRows {
		key := row.TrainVariant + "|" + row.EvalVariant
		groups[key] = append(groups[key], row)
	}
	out := make([]metricRobustnessRow, 0, len(transferRows))
	for _, group := range groups {
		fills := make([]float64, 0, len(group))
		p99 := make([]float64, 0, len(group))
		surplus := make([]float64, 0, len(group))
		adverse := make([]float64, 0, len(group))
		gap := make([]float64, 0, len(group))
		for _, row := range group {
			fills = append(fills, row.MeanFillsPerSec)
			p99 = append(p99, row.MeanP99LatencyMs)
			surplus = append(surplus, row.MeanRetailSurplusPerUnit)
			adverse = append(adverse, row.MeanRetailAdverseSelectionRate)
			gap = append(gap, row.MeanSurplusTransferGap)
		}
		fillMean, fillStd := meanStd(fills)
		p99Mean, p99Std := meanStd(p99)
		surplusMean, surplusStd := meanStd(surplus)
		adverseMean, adverseStd := meanStd(adverse)
		gapMean, gapStd := meanStd(gap)
		rows := make([]metricRobustnessRow, 0, len(group))
		for _, row := range group {
			score := metricSet.FillsWeight*zScore(row.MeanFillsPerSec, fillMean, fillStd) - metricSet.P99Weight*zScore(row.MeanP99LatencyMs, p99Mean, p99Std) + metricSet.RetailSurplusWeight*zScore(row.MeanRetailSurplusPerUnit, surplusMean, surplusStd) - metricSet.RetailAdverseWeight*zScore(row.MeanRetailAdverseSelectionRate, adverseMean, adverseStd) - metricSet.SurplusGapWeight*zScore(row.MeanSurplusTransferGap, gapMean, gapStd)
			rows = append(rows, metricRobustnessRow{TrainVariant: row.TrainVariant, EvalVariant: row.EvalVariant, MetricSet: metricSet.Name, Policy: row.Policy, Score: score})
		}
		sort.Slice(rows, func(i, j int) bool {
			if math.Abs(rows[i].Score-rows[j].Score) <= 1e-9 {
				return rows[i].Policy < rows[j].Policy
			}
			return rows[i].Score > rows[j].Score
		})
		currentRank := 0
		for idx := range rows {
			if idx == 0 || math.Abs(rows[idx].Score-rows[idx-1].Score) > 1e-9 {
				currentRank = idx + 1
			}
			rows[idx].Rank = currentRank
		}
		out = append(out, rows...)
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].MetricSet == out[j].MetricSet {
			if out[i].TrainVariant == out[j].TrainVariant {
				if out[i].EvalVariant == out[j].EvalVariant {
					return out[i].Rank < out[j].Rank
				}
				return out[i].EvalVariant < out[j].EvalVariant
			}
			return out[i].TrainVariant < out[j].TrainVariant
		}
		return out[i].MetricSet < out[j].MetricSet
	})
	return out
}

func buildMetricEvaluatorShifts(rows []metricRobustnessRow) []metricRobustnessShift {
	groups := make(map[string]map[string]int)
	for _, row := range rows {
		key := row.MetricSet + "|" + row.TrainVariant + "|" + row.EvalVariant
		if _, ok := groups[key]; !ok {
			groups[key] = make(map[string]int)
		}
		groups[key][row.Policy] = row.Rank
	}
	shifts := make([]metricRobustnessShift, 0, 24)
	comparisons := []string{"matching_only", "no_settlement"}
	metricSeen := make(map[string]struct{})
	trainSeen := make(map[string]struct{})
	for _, row := range rows {
		metricSeen[row.MetricSet] = struct{}{}
		trainSeen[row.TrainVariant] = struct{}{}
	}
	metrics := make([]string, 0, len(metricSeen))
	for name := range metricSeen {
		metrics = append(metrics, name)
	}
	trains := make([]string, 0, len(trainSeen))
	for name := range trainSeen {
		trains = append(trains, name)
	}
	sort.Strings(metrics)
	sort.Strings(trains)
	for _, metric := range metrics {
		for _, train := range trains {
			leftKey := metric + "|" + train + "|full_protocol"
			leftRanks := groups[leftKey]
			if len(leftRanks) == 0 {
				continue
			}
			for _, eval := range comparisons {
				rightKey := metric + "|" + train + "|" + eval
				rightRanks := groups[rightKey]
				common := intersectPolicies(leftRanks, rightRanks)
				if len(common) == 0 {
					continue
				}
				shiftCount := 0
				for _, policy := range common {
					if leftRanks[policy] != rightRanks[policy] {
						shiftCount++
					}
				}
				shifts = append(shifts, metricRobustnessShift{ShiftType: "eval", LeftGroup: fmt.Sprintf("train=%s|eval=full_protocol", train), RightGroup: fmt.Sprintf("train=%s|eval=%s", train, eval), MetricSet: metric, CommonPolicies: common, KendallTau: kendallTauFromRanks(common, leftRanks, rightRanks), PoliciesWithRankShift: shiftCount})
			}
		}
	}
	sort.Slice(shifts, func(i, j int) bool {
		if shifts[i].MetricSet == shifts[j].MetricSet {
			if shifts[i].LeftGroup == shifts[j].LeftGroup {
				return shifts[i].RightGroup < shifts[j].RightGroup
			}
			return shifts[i].LeftGroup < shifts[j].LeftGroup
		}
		return shifts[i].MetricSet < shifts[j].MetricSet
	})
	return shifts
}

func buildMetricSetShifts(rows []metricRobustnessRow) []metricRobustnessShift {
	groups := make(map[string]map[string]int)
	for _, row := range rows {
		key := row.TrainVariant + "|" + row.EvalVariant + "|" + row.MetricSet
		if _, ok := groups[key]; !ok {
			groups[key] = make(map[string]int)
		}
		groups[key][row.Policy] = row.Rank
	}
	shifts := make([]metricRobustnessShift, 0, 24)
	comparisons := []string{"latency_focused", "market_quality", "welfare_focused"}
	trainEvalSeen := make(map[string]struct{})
	for _, row := range rows {
		trainEvalSeen[row.TrainVariant+"|"+row.EvalVariant] = struct{}{}
	}
	keys := make([]string, 0, len(trainEvalSeen))
	for key := range trainEvalSeen {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		parts := strings.Split(key, "|")
		trainVariant, evalVariant := parts[0], parts[1]
		leftKey := trainVariant + "|" + evalVariant + "|canonical"
		leftRanks := groups[leftKey]
		if len(leftRanks) == 0 {
			continue
		}
		for _, metric := range comparisons {
			rightKey := trainVariant + "|" + evalVariant + "|" + metric
			rightRanks := groups[rightKey]
			common := intersectPolicies(leftRanks, rightRanks)
			if len(common) == 0 {
				continue
			}
			shiftCount := 0
			for _, policy := range common {
				if leftRanks[policy] != rightRanks[policy] {
					shiftCount++
				}
			}
			shifts = append(shifts, metricRobustnessShift{ShiftType: "metric_set", LeftGroup: fmt.Sprintf("train=%s|eval=%s|metric=canonical", trainVariant, evalVariant), RightGroup: fmt.Sprintf("train=%s|eval=%s|metric=%s", trainVariant, evalVariant, metric), EvalVariant: evalVariant, CommonPolicies: common, KendallTau: kendallTauFromRanks(common, leftRanks, rightRanks), PoliciesWithRankShift: shiftCount})
		}
	}
	sort.Slice(shifts, func(i, j int) bool {
		if shifts[i].LeftGroup == shifts[j].LeftGroup {
			return shifts[i].RightGroup < shifts[j].RightGroup
		}
		return shifts[i].LeftGroup < shifts[j].LeftGroup
	})
	return shifts
}

func collectBenchmarkMetric(runs []BenchmarkResult, metric func(BenchmarkResult) float64) []float64 {
	values := make([]float64, 0, len(runs))
	for _, run := range runs {
		values = append(values, metric(run))
	}
	return values
}

func sumIntMetric(runs []BenchmarkResult, metric func(BenchmarkResult) int) int {
	total := 0
	for _, run := range runs {
		total += metric(run)
	}
	return total
}

func writeSimulatorRicherProtocolArtifacts(artifact richerProtocolArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_richer_policy_protocol.json")
	mdPath := filepath.Join(base, "simulator_richer_policy_protocol.md")
	csvPath := filepath.Join(base, "simulator_richer_policy_protocol.csv")
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	results := append([]richerProtocolResult(nil), artifact.Results...)
	sort.Slice(results, func(i, j int) bool {
		if results[i].Surface == results[j].Surface {
			if results[i].Split == results[j].Split {
				return results[i].Rank < results[j].Rank
			}
			return results[i].Split < results[j].Split
		}
		return results[i].Surface < results[j].Surface
	})
	var md strings.Builder
	md.WriteString("# Simulator Richer Policy Protocol\n\n")
	md.WriteString("This artifact compares the matched paper-facing control surface with a richer observation/action protocol to test whether the top tie is a low-dimensional protocol artifact.\n\n")
	md.WriteString("## Surfaces\n\n")
	md.WriteString("| Surface | Obs Features | Actions | Action Names |\n")
	md.WriteString("|---|---:|---:|---|\n")
	for _, surface := range artifact.Surfaces {
		md.WriteString(fmt.Sprintf("| %s | %d | %d | %s |\n", surface.Name, surface.ObservationFeatureCount, surface.ActionCount, strings.Join(surface.ActionNames, ", ")))
	}
	md.WriteString("\n## Ranked Results\n\n")
	md.WriteString("| Surface | Split | Rank | Policy | Score | Fills/s | p99 (ms) | Surplus | Adverse | Gap |\n")
	md.WriteString("|---|---|---:|---|---:|---:|---:|---:|---:|---:|\n")
	var csv strings.Builder
	csv.WriteString("section,surface,split,rank,policy,benchmark_score,mean_orders_per_sec,mean_fills_per_sec,mean_p99_latency_ms,mean_average_price_impact,mean_retail_surplus_per_unit,mean_retail_adverse_selection_rate,mean_surplus_transfer_gap,negative_balance_violations_total,conservation_breaches_total,reference_policy,left_policy,right_policy,samples,exact_action_agreement,left_unique_actions,right_unique_actions,left_action_entropy,right_action_entropy\n")
	for _, row := range results {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %s | %.4f | %.2f | %.2f | %.4f | %.4f | %.4f |\n", row.Surface, row.Split, row.Rank, row.Policy, row.BenchmarkScore, row.MeanFillsPerSec, row.MeanP99LatencyMs, row.MeanRetailSurplusPerUnit, row.MeanRetailAdverseSelectionRate, row.MeanSurplusTransferGap))
		csv.WriteString(fmt.Sprintf("result,%s,%s,%d,%s,%.6f,%.4f,%.4f,%.4f,%.6f,%.6f,%.6f,%.6f,%d,%d,,,,,,,,,\n", row.Surface, row.Split, row.Rank, row.Policy, row.BenchmarkScore, row.MeanOrdersPerSec, row.MeanFillsPerSec, row.MeanP99LatencyMs, row.MeanAveragePriceImpact, row.MeanRetailSurplusPerUnit, row.MeanRetailAdverseSelectionRate, row.MeanSurplusTransferGap, row.NegativeBalanceViolationsTotal, row.ConservationBreachesTotal))
	}
	md.WriteString("\n## Agreement Diagnostics\n\n")
	md.WriteString("| Surface | Split | Left | Right | Samples | Agreement | Left Entropy | Right Entropy |\n")
	md.WriteString("|---|---|---|---|---:|---:|---:|---:|\n")
	for _, row := range artifact.AgreementRows {
		md.WriteString(fmt.Sprintf("| %s | %s | %s | %s | %d | %.4f | %.4f | %.4f |\n", row.Surface, row.Split, row.LeftPolicy, row.RightPolicy, row.Samples, row.ExactActionAgreement, row.LeftActionEntropy, row.RightActionEntropy))
		csv.WriteString(fmt.Sprintf("agreement,%s,%s,0,,0,0,0,0,0,0,0,0,0,0,%s,%s,%s,%d,%.6f,%d,%d,%.6f,%.6f\n", row.Surface, row.Split, row.ReferencePolicy, row.LeftPolicy, row.RightPolicy, row.Samples, row.ExactActionAgreement, row.LeftUniqueActions, row.RightUniqueActions, row.LeftActionEntropy, row.RightActionEntropy))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorProtocolTransferArtifacts(artifact protocolTransferArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_protocol_transfer_matrix.json")
	mdPath := filepath.Join(base, "simulator_protocol_transfer_matrix.md")
	csvPath := filepath.Join(base, "simulator_protocol_transfer_matrix.csv")
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	rows := append([]protocolTransferRow(nil), artifact.Rows...)
	sort.Slice(rows, func(i, j int) bool {
		if rows[i].TrainVariant == rows[j].TrainVariant {
			if rows[i].EvalVariant == rows[j].EvalVariant {
				return rows[i].Rank < rows[j].Rank
			}
			return rows[i].EvalVariant < rows[j].EvalVariant
		}
		return rows[i].TrainVariant < rows[j].TrainVariant
	})
	var md strings.Builder
	md.WriteString("# Simulator Protocol Transfer Matrix\n\n")
	md.WriteString("This artifact fixes a policy family and evaluates it under different protocol variants to separate training-objective misspecification from evaluator misspecification.\n\n")
	md.WriteString("## Transfer Rows\n\n")
	md.WriteString("| Train Variant | Eval Variant | Rank | Policy | Score | Fills/s | p99 (ms) | Surplus | Adverse | Gap | Neg. Bal. | Conservation |\n")
	md.WriteString("|---|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	var csv strings.Builder
	csv.WriteString("section,train_variant,eval_variant,policy,rank,benchmark_score,mean_orders_per_sec,mean_fills_per_sec,mean_p99_latency_ms,mean_average_price_impact,mean_retail_surplus_per_unit,mean_retail_adverse_selection_rate,mean_surplus_transfer_gap,negative_balance_violations_total,conservation_breaches_total,left_group,right_group,kendall_tau,policies_with_rank_shift,common_policies\n")
	for _, row := range rows {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %s | %.4f | %.2f | %.2f | %.4f | %.4f | %.4f | %d | %d |\n", row.TrainVariant, row.EvalVariant, row.Rank, row.Policy, row.BenchmarkScore, row.MeanFillsPerSec, row.MeanP99LatencyMs, row.MeanRetailSurplusPerUnit, row.MeanRetailAdverseSelectionRate, row.MeanSurplusTransferGap, row.NegativeBalanceViolationsTotal, row.ConservationBreachesTotal))
		csv.WriteString(fmt.Sprintf("row,%s,%s,%s,%d,%.6f,%.4f,%.4f,%.4f,%.6f,%.6f,%.6f,%.6f,%d,%d,,,,,\n", row.TrainVariant, row.EvalVariant, row.Policy, row.Rank, row.BenchmarkScore, row.MeanOrdersPerSec, row.MeanFillsPerSec, row.MeanP99LatencyMs, row.MeanAveragePriceImpact, row.MeanRetailSurplusPerUnit, row.MeanRetailAdverseSelectionRate, row.MeanSurplusTransferGap, row.NegativeBalanceViolationsTotal, row.ConservationBreachesTotal))
	}
	md.WriteString("\n## Evaluator Rank Shifts\n\n")
	md.WriteString("| Left Group | Right Group | Common Policies | Kendall Tau | Policies With Rank Shift |\n")
	md.WriteString("|---|---|---:|---:|---:|\n")
	for _, shift := range artifact.EvalShifts {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %.4f | %d |\n", shift.LeftGroup, shift.RightGroup, len(shift.CommonPolicies), shift.KendallTau, shift.PoliciesWithRankShift))
		csv.WriteString(fmt.Sprintf("eval_shift,,,,,0,0,0,0,0,0,0,0,0,0,%s,%s,%.6f,%d,%s\n", shift.LeftGroup, shift.RightGroup, shift.KendallTau, shift.PoliciesWithRankShift, strings.Join(shift.CommonPolicies, ";")))
	}
	md.WriteString("\n## Training Rank Shifts\n\n")
	md.WriteString("| Left Group | Right Group | Common Policies | Kendall Tau | Policies With Rank Shift |\n")
	md.WriteString("|---|---|---:|---:|---:|\n")
	for _, shift := range artifact.TrainShifts {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %.4f | %d |\n", shift.LeftGroup, shift.RightGroup, len(shift.CommonPolicies), shift.KendallTau, shift.PoliciesWithRankShift))
		csv.WriteString(fmt.Sprintf("train_shift,,,,,0,0,0,0,0,0,0,0,0,0,%s,%s,%.6f,%d,%s\n", shift.LeftGroup, shift.RightGroup, shift.KendallTau, shift.PoliciesWithRankShift, strings.Join(shift.CommonPolicies, ";")))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorMetricRobustnessArtifacts(artifact metricRobustnessArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_metric_set_robustness.json")
	mdPath := filepath.Join(base, "simulator_metric_set_robustness.md")
	csvPath := filepath.Join(base, "simulator_metric_set_robustness.csv")
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	rows := append([]metricRobustnessRow(nil), artifact.Rows...)
	sort.Slice(rows, func(i, j int) bool {
		if rows[i].MetricSet == rows[j].MetricSet {
			if rows[i].TrainVariant == rows[j].TrainVariant {
				if rows[i].EvalVariant == rows[j].EvalVariant {
					return rows[i].Rank < rows[j].Rank
				}
				return rows[i].EvalVariant < rows[j].EvalVariant
			}
			return rows[i].TrainVariant < rows[j].TrainVariant
		}
		return rows[i].MetricSet < rows[j].MetricSet
	})
	var md strings.Builder
	md.WriteString("# Simulator Metric Set Robustness\n\n")
	md.WriteString("This artifact recomputes rankings under multiple metric sets to test whether the protocol conclusions are stable to welfare weighting.\n\n")
	md.WriteString("## Metric Sets\n\n")
	md.WriteString("| Metric Set | Fills | p99 | Retail Surplus | Adverse | Gap |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|\n")
	for _, metricSet := range artifact.MetricSets {
		md.WriteString(fmt.Sprintf("| %s | %.2f | %.2f | %.2f | %.2f | %.2f |\n", metricSet.Name, metricSet.FillsWeight, metricSet.P99Weight, metricSet.RetailSurplusWeight, metricSet.RetailAdverseWeight, metricSet.SurplusGapWeight))
	}
	md.WriteString("\n## Ranked Rows\n\n")
	md.WriteString("| Metric Set | Train Variant | Eval Variant | Rank | Policy | Score |\n")
	md.WriteString("|---|---|---|---:|---|---:|\n")
	var csv strings.Builder
	csv.WriteString("section,metric_set,train_variant,eval_variant,policy,score,rank,left_group,right_group,kendall_tau,policies_with_rank_shift,common_policies\n")
	for _, row := range rows {
		md.WriteString(fmt.Sprintf("| %s | %s | %s | %d | %s | %.4f |\n", row.MetricSet, row.TrainVariant, row.EvalVariant, row.Rank, row.Policy, row.Score))
		csv.WriteString(fmt.Sprintf("row,%s,%s,%s,%s,%.6f,%d,,,,,\n", row.MetricSet, row.TrainVariant, row.EvalVariant, row.Policy, row.Score, row.Rank))
	}
	md.WriteString("\n## Evaluator Shifts\n\n")
	md.WriteString("| Metric Set | Left Group | Right Group | Common Policies | Kendall Tau | Rank Shift |\n")
	md.WriteString("|---|---|---|---:|---:|---:|\n")
	for _, shift := range artifact.EvalShifts {
		md.WriteString(fmt.Sprintf("| %s | %s | %s | %d | %.4f | %d |\n", shift.MetricSet, shift.LeftGroup, shift.RightGroup, len(shift.CommonPolicies), shift.KendallTau, shift.PoliciesWithRankShift))
		csv.WriteString(fmt.Sprintf("eval_shift,%s,,,,0,%s,%s,%.6f,%d,%s\n", shift.MetricSet, shift.LeftGroup, shift.RightGroup, shift.KendallTau, shift.PoliciesWithRankShift, strings.Join(shift.CommonPolicies, ";")))
	}
	md.WriteString("\n## Metric-Set Shifts\n\n")
	md.WriteString("| Eval Variant | Left Group | Right Group | Common Policies | Kendall Tau | Rank Shift |\n")
	md.WriteString("|---|---|---|---:|---:|---:|\n")
	for _, shift := range artifact.MetricShifts {
		md.WriteString(fmt.Sprintf("| %s | %s | %s | %d | %.4f | %d |\n", shift.EvalVariant, shift.LeftGroup, shift.RightGroup, len(shift.CommonPolicies), shift.KendallTau, shift.PoliciesWithRankShift))
		csv.WriteString(fmt.Sprintf("metric_shift,,,%s,,0,%s,%s,%.6f,%d,%s\n", shift.EvalVariant, shift.LeftGroup, shift.RightGroup, shift.KendallTau, shift.PoliciesWithRankShift, strings.Join(shift.CommonPolicies, ";")))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}
