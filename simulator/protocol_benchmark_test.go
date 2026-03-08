package simulator

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"time"
)

type calibratedProtocolConfig struct {
	RewardWeights   RewardWeights `json:"reward_weights"`
	TrainSeeds      []int64       `json:"train_seeds"`
	ValidationSeeds []int64       `json:"validation_seeds"`
	HeldOutSeeds    []int64       `json:"heldout_seeds"`
	PPOEpisodes     int           `json:"ppo_episodes"`
	PPOClipEpsilon  float64       `json:"ppo_clip_epsilon"`
	PPOPolicyEpochs int           `json:"ppo_policy_epochs"`
	IQLIterations   int           `json:"iql_iterations"`
	IQLExpectile    float64       `json:"iql_expectile"`
	IQLBeta         float64       `json:"iql_beta"`
}

type calibratedProtocolResult struct {
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
}

type counterfactualControlResult struct {
	Variant                        string  `json:"variant"`
	Policy                         string  `json:"policy"`
	Runs                           int     `json:"runs"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	CI95OrdersPerSec               float64 `json:"ci95_orders_per_sec"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	CI95FillsPerSec                float64 `json:"ci95_fills_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	CI95P99LatencyMs               float64 `json:"ci95_p99_latency_ms"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	CI95RetailSurplusPerUnit       float64 `json:"ci95_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	CI95RetailAdverseSelectionRate float64 `json:"ci95_retail_adverse_selection_rate"`
	MeanSurplusTransferGap         float64 `json:"mean_surplus_transfer_gap"`
	CI95SurplusTransferGap         float64 `json:"ci95_surplus_transfer_gap"`
	NegativeBalanceViolationsTotal int     `json:"negative_balance_violations_total"`
	ConservationBreachesTotal      int     `json:"conservation_breaches_total"`
}

type ppoTrainingSnapshot struct {
	Episode         int     `json:"episode"`
	MeanTrainReward float64 `json:"mean_train_reward"`
	ValidationScore float64 `json:"validation_score"`
}

type iqlTrainingSummary struct {
	Iterations      int     `json:"iterations"`
	Expectile       float64 `json:"expectile"`
	Beta            float64 `json:"beta"`
	ValidationScore float64 `json:"validation_score"`
}

type iqlValueModel struct {
	Weights []float64
}

type weightedOfflineLabel struct {
	features []float64
	label    int
	weight   float64
}

type marketDataManifest struct {
	ProfileName  string   `json:"profile_name"`
	Venue        string   `json:"venue"`
	DownloadedAt string   `json:"downloaded_at"`
	StartTimeUTC string   `json:"start_time_utc"`
	EndTimeUTC   string   `json:"end_time_utc"`
	BaseURLs     []string `json:"base_urls"`
	Symbols      []struct {
		Symbol         string `json:"symbol"`
		AggTrades      int    `json:"agg_trades"`
		Klines         int    `json:"klines"`
		DepthSnapshots int    `json:"depth_snapshots"`
	} `json:"symbols"`
}

type marketDataProvenanceFile struct {
	RelativePath string `json:"relative_path"`
	SizeBytes    int64  `json:"size_bytes"`
	SHA256       string `json:"sha256"`
}

type marketDataProvenanceArtifact struct {
	ProfileName    string                     `json:"profile_name"`
	Venue          string                     `json:"venue"`
	DownloadedAt   string                     `json:"downloaded_at"`
	StartTimeUTC   string                     `json:"start_time_utc"`
	EndTimeUTC     string                     `json:"end_time_utc"`
	BaseURLs       []string                   `json:"base_urls"`
	ManifestSHA256 string                     `json:"manifest_sha256"`
	SymbolCount    int                        `json:"symbol_count"`
	TradeCount     int                        `json:"trade_count"`
	Files          []marketDataProvenanceFile `json:"files"`
}

func TestGenerateSimulatorCalibratedLearningProtocolArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_CALIBRATED_PROTOCOL") != "1" {
		t.Skip("set RUN_SIM_CALIBRATED_PROTOCOL=1 to generate calibrated protocol artifacts")
	}

	cfg := defaultCalibratedProtocolConfig()
	base := calibratedAdaptiveProtocolBaseScenario()
	validationRegimes := calibratedValidationRegimes()
	heldOutRegimes := calibratedHeldOutRegimes()

	ppoTrace, ppoModel := trainProtocolPPO(base, cfg, validationRegimes)
	iqlModel, iqlSummary := trainProtocolIQL(base, cfg, validationRegimes)
	fittedQ := cachedFittedQPolicy(base)

	results := make([]calibratedProtocolResult, 0, 8)
	results = append(results,
		summarizeProtocolRuns("burst_aware", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, burstAwareChooser()),
		summarizeProtocolRuns("fitted_q", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, fittedQChooser(fittedQ)),
		summarizeProtocolRuns("ppo_clip", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooser(ppoModel)),
		summarizeProtocolRuns("iql", "validation", validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooser(iqlModel)),
		summarizeProtocolRuns("burst_aware", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, burstAwareChooser()),
		summarizeProtocolRuns("fitted_q", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, fittedQChooser(fittedQ)),
		summarizeProtocolRuns("ppo_clip", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, tinyChooser(ppoModel)),
		summarizeProtocolRuns("iql", "heldout", heldOutRegimes, cfg.HeldOutSeeds, cfg.RewardWeights, tinyChooser(iqlModel)),
	)

	if err := writeSimulatorCalibratedProtocolArtifacts(results, cfg, scenarioNames(validationRegimes), scenarioNames(heldOutRegimes), ppoTrace, iqlSummary); err != nil {
		t.Fatalf("write calibrated protocol artifacts: %v", err)
	}
}

func TestGenerateSimulatorCounterfactualControlArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_COUNTERFACTUAL") != "1" {
		t.Skip("set RUN_SIM_COUNTERFACTUAL=1 to generate counterfactual control artifacts")
	}

	baseCfg := defaultCalibratedProtocolConfig()
	variants := []struct {
		Name          string
		Base          ScenarioConfig
		HeldOut       []ScenarioConfig
		RewardWeights RewardWeights
	}{
		{Name: "control", Base: calibratedAdaptiveProtocolBaseScenario(), HeldOut: calibratedHeldOutRegimes(), RewardWeights: baseCfg.RewardWeights},
		{Name: "matching_only", Base: calibratedMatchingOnlyScenario(), HeldOut: counterfactualHeldOutRegimes(calibratedMatchingOnlyScenario()), RewardWeights: baseCfg.RewardWeights},
		{Name: "no_settlement", Base: calibratedNoSettlementScenario(), HeldOut: counterfactualHeldOutRegimes(calibratedNoSettlementScenario()), RewardWeights: baseCfg.RewardWeights},
		{Name: "no_welfare_reward", Base: calibratedAdaptiveProtocolBaseScenario(), HeldOut: calibratedHeldOutRegimes(), RewardWeights: noWelfareRewardWeights()},
	}

	results := make([]counterfactualControlResult, 0, len(variants)*3)
	for _, variant := range variants {
		cfg := baseCfg
		cfg.RewardWeights = variant.RewardWeights
		_, ppoModel := trainProtocolPPO(variant.Base, cfg, counterfactualValidationRegimes(variant.Base))
		iqlModel, _ := trainProtocolIQL(variant.Base, cfg, counterfactualValidationRegimes(variant.Base))
		results = append(results,
			summarizeCounterfactualRuns(variant.Name, "burst_aware", variant.HeldOut, cfg.HeldOutSeeds, variant.RewardWeights, burstAwareChooser()),
			summarizeCounterfactualRuns(variant.Name, "ppo_clip", variant.HeldOut, cfg.HeldOutSeeds, variant.RewardWeights, tinyChooser(ppoModel)),
			summarizeCounterfactualRuns(variant.Name, "iql", variant.HeldOut, cfg.HeldOutSeeds, variant.RewardWeights, tinyChooser(iqlModel)),
		)
	}

	if err := writeSimulatorCounterfactualArtifacts(results, variants); err != nil {
		t.Fatalf("write counterfactual artifacts: %v", err)
	}
}

func TestGenerateMarketDataProvenanceArtifacts(t *testing.T) {
	if os.Getenv("RUN_MARKET_PROVENANCE") != "1" {
		t.Skip("set RUN_MARKET_PROVENANCE=1 to generate market-data provenance artifacts")
	}
	for _, profile := range []string{"smoke", "multimarket"} {
		artifact, err := buildMarketDataProvenanceArtifact(profile)
		if err != nil {
			t.Fatalf("build provenance artifact %s: %v", profile, err)
		}
		if err := writeMarketDataProvenanceArtifacts(profile, artifact); err != nil {
			t.Fatalf("write provenance artifact %s: %v", profile, err)
		}
	}
}

func defaultCalibratedProtocolConfig() calibratedProtocolConfig {
	return calibratedProtocolConfig{
		RewardWeights:   defaultRewardWeights(),
		TrainSeeds:      []int64{1103, 1109, 1117, 1123},
		ValidationSeeds: []int64{1129, 1151},
		HeldOutSeeds:    []int64{1153, 1163, 1171, 1181},
		PPOEpisodes:     80,
		PPOClipEpsilon:  0.18,
		PPOPolicyEpochs: 3,
		IQLIterations:   6,
		IQLExpectile:    0.70,
		IQLBeta:         0.80,
	}
}

func calibratedAdaptiveProtocolBaseScenario() ScenarioConfig {
	return ScenarioConfig{
		Name:                   "Calibrated-Protocol-Adaptive-1-3s",
		Mode:                   ModeAdaptiveBatch,
		AdaptivePolicy:         AdaptiveBalanced,
		AdaptiveMinWindowSteps: 1,
		AdaptiveMaxWindowSteps: 3,
		AdaptiveOrderThreshold: 8,
		AdaptiveQueueThreshold: 10,
		StepDuration:           1000 * time.Millisecond,
		TotalSteps:             480,
		Agents:                 CalibratedPopulation(),
		Risk:                   RiskConfig{MaxOrderAmount: 20, MaxOrdersPerStep: 96},
		Fundamentals:           calibratedMarketScenario().Fundamentals,
	}
}

func calibratedMatchingOnlyScenario() ScenarioConfig {
	cfg := calibratedAdaptiveProtocolBaseScenario()
	cfg.Name = "Calibrated-MatchingOnly-Immediate"
	cfg.Mode = ModeImmediate
	cfg.AdaptivePolicy = ""
	cfg.AdaptiveMinWindowSteps = 0
	cfg.AdaptiveMaxWindowSteps = 0
	cfg.AdaptiveOrderThreshold = 0
	cfg.AdaptiveQueueThreshold = 0
	return cfg
}

func calibratedNoSettlementScenario() ScenarioConfig {
	cfg := calibratedAdaptiveProtocolBaseScenario()
	cfg.Name = "Calibrated-NoSettlement-1-3s"
	cfg.DisableSettlementApplication = true
	cfg.DisableSettlementChecks = true
	return cfg
}

func calibratedValidationRegimes() []ScenarioConfig {
	return []ScenarioConfig{
		calibratedAdaptiveProtocolBaseScenario(),
		scaleCalibratedRegime("Calibrated-Validation-HighArb", 3, 1, 1, 1),
		scaleCalibratedRegime("Calibrated-Validation-RetailBurst", 1, 2, 1, 1),
	}
}

func calibratedHeldOutRegimes() []ScenarioConfig {
	return []ScenarioConfig{
		scaleCalibratedRegime("Calibrated-HeldOut-HighArbWideMaker", 3, 1, 1, 3),
		scaleCalibratedRegime("Calibrated-HeldOut-RetailBurst", 1, 3, 1, 1),
		scaleCalibratedRegime("Calibrated-HeldOut-InformedWide", 1, 1, 3, 2),
		scaleCalibratedRegime("Calibrated-HeldOut-CompositeStress", 3, 2, 2, 3),
	}
}

func counterfactualValidationRegimes(base ScenarioConfig) []ScenarioConfig {
	regimes := []ScenarioConfig{base}
	highArb := scaleCounterfactualRegime(base, "Validation-HighArb", 3, 1, 1, 1)
	retail := scaleCounterfactualRegime(base, "Validation-RetailBurst", 1, 2, 1, 1)
	regimes = append(regimes, highArb, retail)
	return regimes
}

func counterfactualHeldOutRegimes(base ScenarioConfig) []ScenarioConfig {
	return []ScenarioConfig{
		scaleCounterfactualRegime(base, "HeldOut-HighArbWideMaker", 3, 1, 1, 3),
		scaleCounterfactualRegime(base, "HeldOut-RetailBurst", 1, 3, 1, 1),
		scaleCounterfactualRegime(base, "HeldOut-InformedWide", 1, 1, 3, 2),
		scaleCounterfactualRegime(base, "HeldOut-CompositeStress", 3, 2, 2, 3),
	}
}

func scaleCalibratedRegime(name string, arbMult, retailMult, informedMult, makerWidthMult int) ScenarioConfig {
	return scaleCounterfactualRegime(calibratedAdaptiveProtocolBaseScenario(), name, arbMult, retailMult, informedMult, makerWidthMult)
}

func scaleCounterfactualRegime(base ScenarioConfig, name string, arbMult, retailMult, informedMult, makerWidthMult int) ScenarioConfig {
	cfg := base
	cfg.Name = name
	agents := append([]AgentConfig(nil), base.Agents...)
	agents = ScaleClassIntensity(agents, AgentArbitrageur, arbMult, 1)
	agents = ScaleClassIntensity(agents, AgentRetail, retailMult, 1)
	agents = ScaleClassIntensity(agents, AgentInformed, informedMult, 1)
	agents = ScaleClassQuoteWidth(agents, AgentMarketMaker, makerWidthMult, 1)
	cfg.Agents = agents
	cfg.Risk = RiskConfig{
		MaxOrderAmount:   base.Risk.MaxOrderAmount + int64(maxInt(0, arbMult-1)+maxInt(0, retailMult-1)+maxInt(0, informedMult-1)),
		MaxOrdersPerStep: base.Risk.MaxOrdersPerStep + 8*maxInt(0, retailMult-1) + 4*maxInt(0, arbMult-1),
	}
	return cfg
}

func noWelfareRewardWeights() RewardWeights {
	weights := defaultRewardWeights()
	weights.RetailSurplusWeight = 0
	weights.AdversePenalty = 0
	weights.WelfarePenalty = 0
	weights.SurplusGapPenalty = 0
	return weights
}

func trainProtocolPPO(base ScenarioConfig, cfg calibratedProtocolConfig, validationRegimes []ScenarioConfig) ([]ppoTrainingSnapshot, tinyMLPModel) {
	spec := NewAdapterWithRewardWeights(base, cfg.RewardWeights).ActionSpec()
	actions := candidateBanditActions(spec)
	rng := rand.New(rand.NewSource(trainingRandomSeed(base) + 2603))
	model := initTinyMLPModel(actions, len(observationFeatures(Observation{})), 12, rng)
	supervisedFeatures, supervisedLabels := collectBurstAwareDataset(base, actions, cfg.TrainSeeds)
	trainTinyMLPSupervised(&model, supervisedFeatures, supervisedLabels, 10, 0.025, 1e-4)

	bestModel := copyTinyMLPModel(model)
	bestScore := math.Inf(-1)
	recentRewards := make([]float64, 0, 10)
	trace := make([]ppoTrainingSnapshot, 0, cfg.PPOEpisodes/10+2)
	trace = append(trace, ppoTrainingSnapshot{Episode: 0, MeanTrainReward: 0, ValidationScore: evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooser(model))})

	for episode := 1; episode <= cfg.PPOEpisodes; episode++ {
		trainingCfg := base
		seedBase := cfg.TrainSeeds[(episode-1)%len(cfg.TrainSeeds)]
		seedOffset := int64((episode - 1) / len(cfg.TrainSeeds))
		trainingCfg.Seed = seedBase + seedOffset*1009
		adapter := NewAdapterWithRewardWeights(trainingCfg, cfg.RewardWeights)
		timestep := adapter.Reset()
		trajectory := make([]policyStepSample, 0, trainingCfg.TotalSteps)
		for !timestep.Done {
			features := observationFeatures(timestep.Observation)
			hidden, _, probs := forwardTinyMLP(model, features)
			actionIdx := sampleCategorical(probs, rng)
			next := adapter.Step(model.Actions[actionIdx].Action)
			trajectory = append(trajectory, policyStepSample{
				features: append([]float64(nil), features...),
				hidden:   append([]float64(nil), hidden...),
				probs:    append([]float64(nil), probs...),
				action:   actionIdx,
				reward:   next.Reward,
			})
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
			score := evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooser(model))
			trace = append(trace, ppoTrainingSnapshot{Episode: episode, MeanTrainReward: meanFloatSlice(recentRewards), ValidationScore: score})
			if score > bestScore {
				bestScore = score
				bestModel = copyTinyMLPModel(model)
			}
		}
	}
	return trace, bestModel
}

func trainProtocolIQL(base ScenarioConfig, cfg calibratedProtocolConfig, validationRegimes []ScenarioConfig) (tinyMLPModel, iqlTrainingSummary) {
	spec := NewAdapterWithRewardWeights(base, cfg.RewardWeights).ActionSpec()
	actions := candidateBanditActions(spec)
	rng := rand.New(rand.NewSource(trainingRandomSeed(base) + 3203))
	actor := initTinyMLPModel(actions, len(observationFeatures(Observation{})), 10, rng)
	supervisedFeatures, supervisedLabels := collectBurstAwareDataset(base, actions, cfg.TrainSeeds)
	trainTinyMLPSupervised(&actor, supervisedFeatures, supervisedLabels, 8, 0.020, 1e-4)

	linucb := cachedLinUCBPolicy(base)
	tiny := cachedTinyMLPPolicy(base)
	offline := cachedOfflineContextualPolicy(base)
	transitions := make([]offlineTransitionSample, 0, len(cfg.TrainSeeds)*base.TotalSteps*6)
	for _, seed := range cfg.TrainSeeds {
		for _, behavior := range []PolicyController{PolicyBurstAware, PolicyLearnedLinUCB, PolicyLearnedTinyMLP, PolicyLearnedOfflineContextual} {
			transitions = append(transitions, collectOfflineTransitionTrajectoryWithRewardWeights(base, seed, cfg.RewardWeights, behavior, actions, &linucb, &tiny, &offline, rng)...)
		}
		for rollout := 0; rollout < 2; rollout++ {
			transitions = append(transitions, collectOfflineRandomTransitionTrajectoryWithRewardWeights(base, seed+int64(rollout), cfg.RewardWeights, actions, rng)...)
		}
	}

	qModels := initLinearArmModels(actions, len(observationFeatures(Observation{})), 2.0)
	valueModel := iqlValueModel{Weights: make([]float64, len(observationFeatures(Observation{})))}
	bestActor := copyTinyMLPModel(actor)
	bestScore := math.Inf(-1)
	summary := iqlTrainingSummary{Iterations: cfg.IQLIterations, Expectile: cfg.IQLExpectile, Beta: cfg.IQLBeta}

	for iter := 0; iter < cfg.IQLIterations; iter++ {
		for _, transition := range transitions {
			stateQ := qValuesFromLinearArms(qModels, transition.features)
			targetV := expectileValue(stateQ, cfg.IQLExpectile)
			updateLinearValue(&valueModel, transition.features, targetV, 0.03, 1e-4)
		}

		labels := make([]weightedOfflineLabel, 0, len(transitions))
		for _, transition := range transitions {
			nextValue := 0.0
			if !transition.done {
				nextValue = dot(valueModel.Weights, transition.nextFeatures)
			}
			targetQ := clampFloat(transition.reward+0.97*nextValue, -50, 50)
			arm := &qModels[transition.action]
			arm.A = outerAdd(arm.A, transition.features)
			addScaledInPlace(arm.B, transition.features, targetQ)
			arm.Theta = solveLinearSystem(arm.A, arm.B)
			arm.Updates++

			qsa := dot(arm.Theta, transition.features)
			value := dot(valueModel.Weights, transition.features)
			advantage := clampFloat((qsa-value)/cfg.IQLBeta, -4, 4)
			weight := math.Exp(advantage)
			if weight > 10 {
				weight = 10
			}
			labels = append(labels, weightedOfflineLabel{
				features: append([]float64(nil), transition.features...),
				label:    transition.action,
				weight:   weight,
			})
		}

		trainTinyMLPWeightedClassification(&actor, labels, 8, 0.012, 1e-4)
		score := evaluatePolicyScore(validationRegimes, cfg.ValidationSeeds, cfg.RewardWeights, tinyChooser(actor))
		if score > bestScore {
			bestScore = score
			bestActor = copyTinyMLPModel(actor)
		}
	}
	summary.ValidationScore = bestScore
	return bestActor, summary
}

func trainTinyMLPWeightedClassification(model *tinyMLPModel, labels []weightedOfflineLabel, epochs int, lr, l2 float64) {
	for epoch := 0; epoch < epochs; epoch++ {
		for _, sample := range labels {
			hidden, _, probs := forwardTinyMLP(*model, sample.features)
			dlogits := make([]float64, len(probs))
			for out := range probs {
				dlogits[out] = -probs[out] * sample.weight
			}
			dlogits[sample.label] += sample.weight
			applyTinyMLPGradients(model, sample.features, hidden, dlogits, lr, l2)
		}
	}
}

func collectOfflineTransitionTrajectoryWithRewardWeights(cfg ScenarioConfig, seed int64, weights RewardWeights, behavior PolicyController, actions []learnedActionCandidate, linucb *learnedLinUCBPolicy, tiny *tinyMLPModel, offline *learnedOfflineContextualPolicy, rng *rand.Rand) []offlineTransitionSample {
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
		features := observationFeatures(timestep.Observation)
		next := adapter.Step(action)
		trajectory = append(trajectory, offlineTransitionSample{
			features:     append([]float64(nil), features...),
			action:       actionIdx,
			reward:       next.Reward,
			nextFeatures: append([]float64(nil), observationFeatures(next.Observation)...),
			done:         next.Done,
		})
		timestep = next
	}
	return trajectory
}

func collectOfflineRandomTransitionTrajectoryWithRewardWeights(cfg ScenarioConfig, seed int64, weights RewardWeights, actions []learnedActionCandidate, rng *rand.Rand) []offlineTransitionSample {
	trainingCfg := cfg
	trainingCfg.Seed = seed
	adapter := NewAdapterWithRewardWeights(trainingCfg, weights)
	timestep := adapter.Reset()
	trajectory := make([]offlineTransitionSample, 0, trainingCfg.TotalSteps)
	for !timestep.Done {
		actionIdx := rng.Intn(len(actions))
		features := observationFeatures(timestep.Observation)
		next := adapter.Step(actions[actionIdx].Action)
		trajectory = append(trajectory, offlineTransitionSample{
			features:     append([]float64(nil), features...),
			action:       actionIdx,
			reward:       next.Reward,
			nextFeatures: append([]float64(nil), observationFeatures(next.Observation)...),
			done:         next.Done,
		})
		timestep = next
	}
	return trajectory
}

func expectileValue(values []float64, tau float64) float64 {
	if len(values) == 0 {
		return 0
	}
	current := meanFloatSlice(values)
	for iter := 0; iter < 8; iter++ {
		num := 0.0
		den := 0.0
		for _, value := range values {
			weight := tau
			if value < current {
				weight = 1 - tau
			}
			num += weight * value
			den += weight
		}
		if den <= 0 {
			break
		}
		current = num / den
	}
	return current
}

func updateLinearValue(model *iqlValueModel, features []float64, target, lr, l2 float64) {
	pred := dot(model.Weights, features)
	err := clampFloat(target-pred, -10, 10)
	for idx := range model.Weights {
		model.Weights[idx] = model.Weights[idx]*(1-lr*l2) + lr*err*features[idx]
	}
}

func qValuesFromLinearArms(models []linearArmModel, features []float64) []float64 {
	values := make([]float64, len(models))
	for idx, model := range models {
		values[idx] = dot(model.Theta, features)
	}
	return values
}

func evaluatePolicyScore(regimes []ScenarioConfig, seeds []int64, rewardWeights RewardWeights, chooser func(ActionSpec, Observation) ControlAction) float64 {
	runs := runChooserAcrossRegimes(regimes, seeds, rewardWeights, chooser, "")
	if len(runs) == 0 {
		return math.Inf(-1)
	}
	meanFills := meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.FillsPerSec })
	meanP99 := meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.P99LatencyMs })
	meanImpact := meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.AveragePriceImpact })
	meanRetailSurplus := meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit })
	meanRetailAdverse := meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate })
	meanGap := meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })
	neg := 0
	cons := 0
	for _, run := range runs {
		neg += run.NegativeBalanceViolations
		cons += run.ConservationBreaches
	}
	if neg > 0 || cons > 0 {
		return -1e9
	}
	return meanFills - 0.10*meanP99 - 0.75*meanImpact + 15*meanRetailSurplus - 12*meanRetailAdverse - 6*maxFloat(meanGap, 0)
}

func burstAwareChooser() func(ActionSpec, Observation) ControlAction {
	return func(spec ActionSpec, observation Observation) ControlAction {
		return burstAwareAction(spec, observation)
	}
}

func tinyChooser(model tinyMLPModel) func(ActionSpec, Observation) ControlAction {
	return func(spec ActionSpec, observation Observation) ControlAction {
		return chooseTinyMLPAction(spec, observation, model)
	}
}

func fittedQChooser(policy learnedFittedQPolicy) func(ActionSpec, Observation) ControlAction {
	return func(spec ActionSpec, observation Observation) ControlAction {
		return chooseFittedQAction(spec, observation, policy)
	}
}

func runChooserAcrossRegimes(regimes []ScenarioConfig, seeds []int64, rewardWeights RewardWeights, chooser func(ActionSpec, Observation) ControlAction, name string) []BenchmarkResult {
	runs := make([]BenchmarkResult, 0, len(regimes)*len(seeds))
	for _, regime := range regimes {
		for _, seed := range seeds {
			cfg := regime
			cfg.Seed = seed
			runs = append(runs, runScenarioWithChooser(cfg, rewardWeights, chooser, name))
		}
	}
	return runs
}

func runScenarioWithChooser(cfg ScenarioConfig, rewardWeights RewardWeights, chooser func(ActionSpec, Observation) ControlAction, name string) BenchmarkResult {
	start := time.Now()
	adapter := NewAdapterWithRewardWeights(cfg, rewardWeights)
	timestep := adapter.Reset()
	for !timestep.Done {
		spec := adapter.ActionSpec()
		action := ControlAction{}
		if chooser != nil {
			action = chooser(spec, timestep.Observation)
		}
		timestep = adapter.Step(action)
	}
	result := adapter.env.benchmarkResult(time.Since(start))
	if name != "" {
		result.Name = name
	}
	return result
}

func summarizeProtocolRuns(policy string, split string, regimes []ScenarioConfig, seeds []int64, rewardWeights RewardWeights, chooser func(ActionSpec, Observation) ControlAction) calibratedProtocolResult {
	runs := runChooserAcrossRegimes(regimes, seeds, rewardWeights, chooser, policy)
	orders := selectMetric(runs, func(result BenchmarkResult) float64 { return result.OrdersPerSec })
	fills := selectMetric(runs, func(result BenchmarkResult) float64 { return result.FillsPerSec })
	p99 := selectMetric(runs, func(result BenchmarkResult) float64 { return result.P99LatencyMs })
	impact := selectMetric(runs, func(result BenchmarkResult) float64 { return result.AveragePriceImpact })
	retailSurplus := selectMetric(runs, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit })
	retailAdverse := selectMetric(runs, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate })
	gap := selectMetric(runs, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })
	meanOrders, ciOrders := meanCI95(orders)
	meanFills, ciFills := meanCI95(fills)
	meanP99, ciP99 := meanCI95(p99)
	meanImpact, ciImpact := meanCI95(impact)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanGap, ciGap := meanCI95(gap)
	return calibratedProtocolResult{
		Policy:                         policy,
		Split:                          split,
		RegimeCount:                    len(regimes),
		Runs:                           len(runs),
		MeanOrdersPerSec:               meanOrders,
		CI95OrdersPerSec:               ciOrders,
		MeanFillsPerSec:                meanFills,
		CI95FillsPerSec:                ciFills,
		MeanP99LatencyMs:               meanP99,
		CI95P99LatencyMs:               ciP99,
		MeanAveragePriceImpact:         meanImpact,
		CI95AveragePriceImpact:         ciImpact,
		MeanRetailSurplusPerUnit:       meanRetailSurplus,
		CI95RetailSurplusPerUnit:       ciRetailSurplus,
		MeanRetailAdverseSelectionRate: meanRetailAdverse,
		CI95RetailAdverseSelectionRate: ciRetailAdverse,
		MeanSurplusTransferGap:         meanGap,
		CI95SurplusTransferGap:         ciGap,
		NegativeBalanceViolationsTotal: sumBenchmarkInt(runs, func(result BenchmarkResult) int { return result.NegativeBalanceViolations }),
		ConservationBreachesTotal:      sumBenchmarkInt(runs, func(result BenchmarkResult) int { return result.ConservationBreaches }),
	}
}

func summarizeCounterfactualRuns(variant string, policy string, regimes []ScenarioConfig, seeds []int64, rewardWeights RewardWeights, chooser func(ActionSpec, Observation) ControlAction) counterfactualControlResult {
	runs := runChooserAcrossRegimes(regimes, seeds, rewardWeights, chooser, policy)
	orders := selectMetric(runs, func(result BenchmarkResult) float64 { return result.OrdersPerSec })
	fills := selectMetric(runs, func(result BenchmarkResult) float64 { return result.FillsPerSec })
	p99 := selectMetric(runs, func(result BenchmarkResult) float64 { return result.P99LatencyMs })
	retailSurplus := selectMetric(runs, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit })
	retailAdverse := selectMetric(runs, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate })
	gap := selectMetric(runs, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })
	meanOrders, ciOrders := meanCI95(orders)
	meanFills, ciFills := meanCI95(fills)
	meanP99, ciP99 := meanCI95(p99)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanGap, ciGap := meanCI95(gap)
	return counterfactualControlResult{
		Variant:                        variant,
		Policy:                         policy,
		Runs:                           len(runs),
		MeanOrdersPerSec:               meanOrders,
		CI95OrdersPerSec:               ciOrders,
		MeanFillsPerSec:                meanFills,
		CI95FillsPerSec:                ciFills,
		MeanP99LatencyMs:               meanP99,
		CI95P99LatencyMs:               ciP99,
		MeanRetailSurplusPerUnit:       meanRetailSurplus,
		CI95RetailSurplusPerUnit:       ciRetailSurplus,
		MeanRetailAdverseSelectionRate: meanRetailAdverse,
		CI95RetailAdverseSelectionRate: ciRetailAdverse,
		MeanSurplusTransferGap:         meanGap,
		CI95SurplusTransferGap:         ciGap,
		NegativeBalanceViolationsTotal: sumBenchmarkInt(runs, func(result BenchmarkResult) int { return result.NegativeBalanceViolations }),
		ConservationBreachesTotal:      sumBenchmarkInt(runs, func(result BenchmarkResult) int { return result.ConservationBreaches }),
	}
}

func writeSimulatorCalibratedProtocolArtifacts(results []calibratedProtocolResult, cfg calibratedProtocolConfig, validationRegimes []string, heldOutRegimes []string, ppoTrace []ppoTrainingSnapshot, iqlSummary iqlTrainingSummary) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	payload := map[string]any{
		"config":             cfg,
		"validation_regimes": validationRegimes,
		"heldout_regimes":    heldOutRegimes,
		"results":            results,
		"ppo_trace":          ppoTrace,
		"iql_summary":        iqlSummary,
	}
	jsonPath := filepath.Join(base, "simulator_calibrated_policy_protocol.json")
	mdPath := filepath.Join(base, "simulator_calibrated_policy_protocol.md")
	csvPath := filepath.Join(base, "simulator_calibrated_policy_protocol.csv")
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Calibrated Policy Protocol\n\n")
	md.WriteString("This artifact defines the formal calibrated-learning protocol used in the paper: train on a calibrated adaptive market, select checkpoints on validation regimes, and report held-out performance separately.\n\n")
	md.WriteString(fmt.Sprintf("- Train seeds: `%v`\n", cfg.TrainSeeds))
	md.WriteString(fmt.Sprintf("- Validation seeds: `%v`\n", cfg.ValidationSeeds))
	md.WriteString(fmt.Sprintf("- Held-out seeds: `%v`\n", cfg.HeldOutSeeds))
	md.WriteString(fmt.Sprintf("- Validation regimes: `%s`\n", strings.Join(validationRegimes, ", ")))
	md.WriteString(fmt.Sprintf("- Held-out regimes: `%s`\n\n", strings.Join(heldOutRegimes, ", ")))
	md.WriteString("| Policy | Split | Regimes | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap | Neg. Bal. | Conservation |\n")
	md.WriteString("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	var csv strings.Builder
	csv.WriteString("policy,split,regime_count,runs,mean_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_average_price_impact,ci95_average_price_impact,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap,negative_balance_violations_total,conservation_breaches_total\n")
	for _, result := range results {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f | %d | %d |\n",
			result.Policy, result.Split, result.RegimeCount, result.Runs,
			result.MeanOrdersPerSec, result.CI95OrdersPerSec,
			result.MeanFillsPerSec, result.CI95FillsPerSec,
			result.MeanP99LatencyMs, result.CI95P99LatencyMs,
			result.MeanAveragePriceImpact, result.CI95AveragePriceImpact,
			result.MeanRetailSurplusPerUnit, result.CI95RetailSurplusPerUnit,
			result.MeanRetailAdverseSelectionRate, result.CI95RetailAdverseSelectionRate,
			result.MeanSurplusTransferGap, result.CI95SurplusTransferGap,
			result.NegativeBalanceViolationsTotal, result.ConservationBreachesTotal))
		csv.WriteString(fmt.Sprintf("%s,%s,%d,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%d,%d\n",
			result.Policy, result.Split, result.RegimeCount, result.Runs,
			result.MeanOrdersPerSec, result.CI95OrdersPerSec,
			result.MeanFillsPerSec, result.CI95FillsPerSec,
			result.MeanP99LatencyMs, result.CI95P99LatencyMs,
			result.MeanAveragePriceImpact, result.CI95AveragePriceImpact,
			result.MeanRetailSurplusPerUnit, result.CI95RetailSurplusPerUnit,
			result.MeanRetailAdverseSelectionRate, result.CI95RetailAdverseSelectionRate,
			result.MeanSurplusTransferGap, result.CI95SurplusTransferGap,
			result.NegativeBalanceViolationsTotal, result.ConservationBreachesTotal))
	}
	md.WriteString("\n## PPO Validation Trace\n\n")
	md.WriteString("| Episode | Mean Train Reward | Validation Score |\n|---:|---:|---:|\n")
	for _, point := range ppoTrace {
		md.WriteString(fmt.Sprintf("| %d | %.4f | %.4f |\n", point.Episode, point.MeanTrainReward, point.ValidationScore))
	}
	md.WriteString("\n## IQL Summary\n\n")
	md.WriteString(fmt.Sprintf("- Iterations: `%d`\n", iqlSummary.Iterations))
	md.WriteString(fmt.Sprintf("- Expectile: `%.2f`\n", iqlSummary.Expectile))
	md.WriteString(fmt.Sprintf("- Beta: `%.2f`\n", iqlSummary.Beta))
	md.WriteString(fmt.Sprintf("- Best validation score: `%.4f`\n", iqlSummary.ValidationScore))
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorCounterfactualArtifacts(results []counterfactualControlResult, variants []struct {
	Name          string
	Base          ScenarioConfig
	HeldOut       []ScenarioConfig
	RewardWeights RewardWeights
}) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	payload := map[string]any{
		"results": results,
		"variants": func() []map[string]any {
			rows := make([]map[string]any, 0, len(variants))
			for _, variant := range variants {
				rows = append(rows, map[string]any{
					"name":            variant.Name,
					"base_scenario":   variant.Base.Name,
					"heldout_regimes": scenarioNames(variant.HeldOut),
					"reward_weights":  variant.RewardWeights,
				})
			}
			return rows
		}(),
	}
	jsonPath := filepath.Join(base, "simulator_counterfactual_controls.json")
	mdPath := filepath.Join(base, "simulator_counterfactual_controls.md")
	csvPath := filepath.Join(base, "simulator_counterfactual_controls.csv")
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Counterfactual Controls\n\n")
	md.WriteString("This artifact reports the three counterfactual controls requested in the paper line: `matching_only`, `no_settlement`, and `no_welfare_reward`, alongside the calibrated control benchmark.\n\n")
	md.WriteString("| Variant | Policy | Runs | Orders/s | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap | Neg. Bal. | Conservation |\n")
	md.WriteString("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	var csv strings.Builder
	csv.WriteString("variant,policy,runs,mean_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap,negative_balance_violations_total,conservation_breaches_total\n")
	for _, result := range results {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f | %d | %d |\n",
			result.Variant, result.Policy, result.Runs,
			result.MeanOrdersPerSec, result.CI95OrdersPerSec,
			result.MeanFillsPerSec, result.CI95FillsPerSec,
			result.MeanP99LatencyMs, result.CI95P99LatencyMs,
			result.MeanRetailSurplusPerUnit, result.CI95RetailSurplusPerUnit,
			result.MeanRetailAdverseSelectionRate, result.CI95RetailAdverseSelectionRate,
			result.MeanSurplusTransferGap, result.CI95SurplusTransferGap,
			result.NegativeBalanceViolationsTotal, result.ConservationBreachesTotal))
		csv.WriteString(fmt.Sprintf("%s,%s,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%d,%d\n",
			result.Variant, result.Policy, result.Runs,
			result.MeanOrdersPerSec, result.CI95OrdersPerSec,
			result.MeanFillsPerSec, result.CI95FillsPerSec,
			result.MeanP99LatencyMs, result.CI95P99LatencyMs,
			result.MeanRetailSurplusPerUnit, result.CI95RetailSurplusPerUnit,
			result.MeanRetailAdverseSelectionRate, result.CI95RetailAdverseSelectionRate,
			result.MeanSurplusTransferGap, result.CI95SurplusTransferGap,
			result.NegativeBalanceViolationsTotal, result.ConservationBreachesTotal))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func buildMarketDataProvenanceArtifact(profile string) (marketDataProvenanceArtifact, error) {
	base := filepath.Join("..", "data", "market_calibration", "binance_spot", profile)
	manifestPath := filepath.Join(base, "manifest.json")
	raw, err := os.ReadFile(manifestPath)
	if err != nil {
		return marketDataProvenanceArtifact{}, err
	}
	var manifest marketDataManifest
	if err := json.Unmarshal(raw, &manifest); err != nil {
		return marketDataProvenanceArtifact{}, err
	}
	manifestHash := sha256.Sum256(raw)
	files := make([]marketDataProvenanceFile, 0, 32)
	if err := filepath.Walk(base, func(path string, info os.FileInfo, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if info.IsDir() {
			return nil
		}
		content, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		sum := sha256.Sum256(content)
		rel, err := filepath.Rel(base, path)
		if err != nil {
			return err
		}
		files = append(files, marketDataProvenanceFile{
			RelativePath: filepath.ToSlash(rel),
			SizeBytes:    info.Size(),
			SHA256:       hex.EncodeToString(sum[:]),
		})
		return nil
	}); err != nil {
		return marketDataProvenanceArtifact{}, err
	}
	sort.Slice(files, func(i, j int) bool { return files[i].RelativePath < files[j].RelativePath })
	tradeCount := 0
	for _, symbol := range manifest.Symbols {
		tradeCount += symbol.AggTrades
	}
	return marketDataProvenanceArtifact{
		ProfileName:    manifest.ProfileName,
		Venue:          manifest.Venue,
		DownloadedAt:   manifest.DownloadedAt,
		StartTimeUTC:   manifest.StartTimeUTC,
		EndTimeUTC:     manifest.EndTimeUTC,
		BaseURLs:       append([]string(nil), manifest.BaseURLs...),
		ManifestSHA256: hex.EncodeToString(manifestHash[:]),
		SymbolCount:    len(manifest.Symbols),
		TradeCount:     tradeCount,
		Files:          files,
	}, nil
}

func writeMarketDataProvenanceArtifacts(profile string, artifact marketDataProvenanceArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, fmt.Sprintf("binance_spot_%s_provenance.json", profile))
	mdPath := filepath.Join(base, fmt.Sprintf("binance_spot_%s_provenance.md", profile))
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString(fmt.Sprintf("# Binance Spot %s Provenance\n\n", strings.Title(profile)))
	md.WriteString(fmt.Sprintf("- Venue: `%s`\n", artifact.Venue))
	md.WriteString(fmt.Sprintf("- Downloaded at: `%s`\n", artifact.DownloadedAt))
	md.WriteString(fmt.Sprintf("- Window: `%s -> %s`\n", artifact.StartTimeUTC, artifact.EndTimeUTC))
	md.WriteString(fmt.Sprintf("- Symbols: `%d`\n", artifact.SymbolCount))
	md.WriteString(fmt.Sprintf("- Trade count: `%d`\n", artifact.TradeCount))
	md.WriteString(fmt.Sprintf("- Manifest SHA-256: `%s`\n\n", artifact.ManifestSHA256))
	md.WriteString("| File | Size (bytes) | SHA-256 |\n|---|---:|---|\n")
	for _, file := range artifact.Files {
		md.WriteString(fmt.Sprintf("| `%s` | %d | `%s` |\n", file.RelativePath, file.SizeBytes, file.SHA256))
	}
	return os.WriteFile(mdPath, []byte(md.String()), 0o644)
}

func scenarioNames(regimes []ScenarioConfig) []string {
	names := make([]string, 0, len(regimes))
	for _, regime := range regimes {
		names = append(names, regime.Name)
	}
	sort.Strings(names)
	return names
}

func selectMetric(results []BenchmarkResult, selector func(BenchmarkResult) float64) []float64 {
	values := make([]float64, 0, len(results))
	for _, result := range results {
		values = append(values, selector(result))
	}
	return values
}

func meanBenchmarkMetric(results []BenchmarkResult, selector func(BenchmarkResult) float64) float64 {
	return meanFloatSlice(selectMetric(results, selector))
}

func sumBenchmarkInt(results []BenchmarkResult, selector func(BenchmarkResult) int) int {
	total := 0
	for _, result := range results {
		total += selector(result)
	}
	return total
}
