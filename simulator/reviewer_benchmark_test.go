package simulator

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"time"
)

type runtimeProfileArtifact struct {
	MeasurementScope              string  `json:"measurement_scope"`
	ScenarioCells                 int     `json:"scenario_cells"`
	RunsPerCell                   int     `json:"runs_per_cell"`
	StepsPerRun                   int     `json:"steps_per_run"`
	StepDurationMs                int64   `json:"step_duration_ms"`
	WallSeconds                   float64 `json:"wall_seconds"`
	TotalSteps                    int     `json:"total_steps"`
	StepsPerSecond                float64 `json:"steps_per_second"`
	EstimatedTotalOrderEvents     float64 `json:"estimated_total_order_events"`
	EstimatedOrderEventsPerSecond float64 `json:"estimated_order_events_per_second"`
	EstimatedFillsPerSecond       float64 `json:"estimated_fills_per_second"`
}

type pairedSample struct {
	Key   string
	Left  float64
	Right float64
}

type pairwiseStatRow struct {
	Experiment     string  `json:"experiment"`
	Left           string  `json:"left"`
	Right          string  `json:"right"`
	Metric         string  `json:"metric"`
	Direction      string  `json:"direction"`
	PairedSamples  int     `json:"paired_samples"`
	LeftMean       float64 `json:"left_mean"`
	RightMean      float64 `json:"right_mean"`
	MeanDifference float64 `json:"mean_difference"`
	CI95Difference float64 `json:"ci95_difference"`
	AlignedEffect  float64 `json:"aligned_effect"`
	CohensD        float64 `json:"cohens_d"`
	ExactPValue    float64 `json:"exact_sign_flip_p_value"`
	LeftWins       int     `json:"left_wins"`
	RightWins      int     `json:"right_wins"`
	Ties           int     `json:"ties"`
}

type necessityPolicySummary struct {
	Variant                    string  `json:"variant"`
	Policy                     string  `json:"policy"`
	MeanFillsPerSec            float64 `json:"mean_fills_per_sec"`
	MeanP99LatencyMs           float64 `json:"mean_p99_latency_ms"`
	MeanRetailSurplusPerUnit   float64 `json:"mean_retail_surplus_per_unit"`
	MeanRetailAdverseSelection float64 `json:"mean_retail_adverse_selection_rate"`
	MeanSurplusTransferGap     float64 `json:"mean_surplus_transfer_gap"`
	BenchmarkScore             float64 `json:"benchmark_score"`
	Rank                       int     `json:"rank"`
}

type necessityVariantSummary struct {
	Variant        string                   `json:"variant"`
	BaseScenario   string                   `json:"base_scenario"`
	HeldOutRegimes []string                 `json:"heldout_regimes"`
	Policies       []necessityPolicySummary `json:"policies"`
}

type necessityRankShift struct {
	Variant                string   `json:"variant"`
	RelativeTo             string   `json:"relative_to"`
	CommonPolicies         []string `json:"common_policies"`
	KendallTau             float64  `json:"kendall_tau"`
	FrontierOverlap        int      `json:"frontier_overlap"`
	PoliciesWithRankChange int      `json:"policies_with_rank_change"`
}

type necessityArtifact struct {
	ScoreComponents []string                  `json:"score_components"`
	Variants        []necessityVariantSummary `json:"variants"`
	RankShifts      []necessityRankShift      `json:"rank_shifts"`
}

type welfareSuiteSummary struct {
	Suite                      string  `json:"suite"`
	Policy                     string  `json:"policy"`
	Runs                       int     `json:"runs"`
	MeanP99LatencyMs           float64 `json:"mean_p99_latency_ms"`
	MeanAveragePriceImpact     float64 `json:"mean_average_price_impact"`
	MeanQueuePriorityAdvantage float64 `json:"mean_queue_priority_advantage"`
	MeanLatencyArbitrageProfit float64 `json:"mean_latency_arbitrage_profit"`
	MeanRetailSurplusPerUnit   float64 `json:"mean_retail_surplus_per_unit"`
	MeanRetailAdverseSelection float64 `json:"mean_retail_adverse_selection_rate"`
	MeanSurplusTransferGap     float64 `json:"mean_surplus_transfer_gap"`
}

type welfareCorrelationRow struct {
	Target   string  `json:"target"`
	Metric   string  `json:"metric"`
	Samples  int     `json:"samples"`
	Pearson  float64 `json:"pearson"`
	Spearman float64 `json:"spearman"`
}

type welfareRankStabilityRow struct {
	LeftSuite    string   `json:"left_suite"`
	RightSuite   string   `json:"right_suite"`
	Metric       string   `json:"metric"`
	CommonPolicy []string `json:"common_policies"`
	KendallTau   float64  `json:"kendall_tau"`
}

type welfareRobustnessArtifact struct {
	Summaries     []welfareSuiteSummary     `json:"summaries"`
	Correlations  []welfareCorrelationRow   `json:"correlations"`
	RankStability []welfareRankStabilityRow `json:"rank_stability"`
}

type leaderboardEntry struct {
	Policy                         string  `json:"policy"`
	Family                         string  `json:"family"`
	TrainingBudget                 string  `json:"training_budget"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	MeanAveragePriceImpact         float64 `json:"mean_average_price_impact"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	MeanSurplusTransferGap         float64 `json:"mean_surplus_transfer_gap"`
	BenchmarkScore                 float64 `json:"benchmark_score"`
	Rank                           int     `json:"rank"`
	Frontier                       bool    `json:"frontier"`
	SafetyPassed                   bool    `json:"safety_passed"`
}

type leaderboardArtifact struct {
	TaskName                string             `json:"task_name"`
	ObservationFeatureCount int                `json:"observation_feature_count"`
	ActionCount             int                `json:"action_count"`
	TrainSeeds              []int64            `json:"train_seeds"`
	ValidationSeeds         []int64            `json:"validation_seeds"`
	HeldOutSeeds            []int64            `json:"heldout_seeds"`
	ValidationRegimes       []string           `json:"validation_regimes"`
	HeldOutRegimes          []string           `json:"heldout_regimes"`
	ScoreFormula            string             `json:"score_formula"`
	Entries                 []leaderboardEntry `json:"entries"`
}

type policyEvaluationBundle struct {
	Name string
	Run  func([]ScenarioConfig, []int64) []BenchmarkResult
}

func TestGenerateSimulatorRuntimeProfileArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_RUNTIME") != "1" {
		t.Skip("set RUN_SIM_RUNTIME=1 to generate simulator runtime profile artifacts")
	}

	seeds := []int64{101, 103, 107, 109}
	scenarios := parameterHypercubeScenarios()
	results := make([]hypercubeSweepResult, 0, len(scenarios))
	start := time.Now()
	for _, base := range scenarios {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		results = append(results, summarizeHypercubeRun(base, runs))
	}
	artifact := summarizeRuntimeProfile(results, scenarios, len(seeds), time.Since(start))
	if artifact.StepsPerSecond <= 0 || artifact.EstimatedOrderEventsPerSecond <= 0 {
		t.Fatalf("expected positive runtime profile, got %+v", artifact)
	}
	if err := writeSimulatorRuntimeProfileArtifacts(artifact); err != nil {
		t.Fatalf("write runtime profile artifacts: %v", err)
	}
}

func TestGenerateSimulatorStatisticalReviewArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_STATS") != "1" {
		t.Skip("set RUN_SIM_STATS=1 to generate statistical review artifacts")
	}

	coreSeeds := []int64{7, 11, 19, 23, 29, 31, 37, 41}
	immediateRuns := runScenarioFamilyByName(t, "Immediate-Surrogate", coreSeeds)
	fbaRuns := runScenarioFamilyByName(t, "FBA-250ms", coreSeeds)
	adaptiveRuns := runScenarioFamilyByName(t, "Adaptive-100-250ms", coreSeeds)
	linucbRuns := runScenarioFamilyByName(t, "Policy-LearnedLinUCB-100-250ms", coreSeeds)
	offlineRuns := runScenarioFamilyByName(t, "Policy-LearnedOfflineContextual-100-250ms", coreSeeds)

	heldOutSeeds := []int64{223, 227, 229, 233}
	regimes := heldOutRegimeScenarios()
	heldOutLinUCB := runHeldOutPolicyFamily(regimes, heldOutSeeds, PolicyLearnedLinUCB)
	heldOutFittedQ := runHeldOutPolicyFamily(regimes, heldOutSeeds, PolicyLearnedFittedQ)

	cfg := defaultCalibratedProtocolConfig()
	base := calibratedAdaptiveProtocolBaseScenario()
	validationRegimes := calibratedValidationRegimes()
	heldOutCalibrated := calibratedHeldOutRegimes()
	_, ppoModel := trainProtocolPPO(base, cfg, validationRegimes)
	iqlModel, _ := trainProtocolIQL(base, cfg, validationRegimes)
	ppoRuns := runChooserAcrossRegimes(heldOutCalibrated, cfg.HeldOutSeeds, cfg.RewardWeights, tinyChooser(ppoModel), "ppo_clip")
	iqlRuns := runChooserAcrossRegimes(heldOutCalibrated, cfg.HeldOutSeeds, cfg.RewardWeights, tinyChooser(iqlModel), "iql")

	rows := []pairwiseStatRow{
		makePairwiseStatRow("multiseed_core", "Immediate-Surrogate", "FBA-250ms", "surplus_transfer_gap", "lower_is_better", pairMetric(immediateRuns, fbaRuns, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })),
		makePairwiseStatRow("multiseed_core", "Adaptive-100-250ms", "Immediate-Surrogate", "surplus_transfer_gap", "lower_is_better", pairMetric(adaptiveRuns, immediateRuns, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })),
		makePairwiseStatRow("multiseed_core", "Adaptive-100-250ms", "Immediate-Surrogate", "p99_latency_ms", "lower_is_better", pairMetric(adaptiveRuns, immediateRuns, func(result BenchmarkResult) float64 { return result.P99LatencyMs })),
		makePairwiseStatRow("multiseed_core", "Policy-LearnedOfflineContextual-100-250ms", "Policy-LearnedLinUCB-100-250ms", "surplus_transfer_gap", "lower_is_better", pairMetric(offlineRuns, linucbRuns, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })),
		makePairwiseStatRow("multiseed_core", "Policy-LearnedOfflineContextual-100-250ms", "Policy-LearnedLinUCB-100-250ms", "average_price_impact", "lower_is_better", pairMetric(offlineRuns, linucbRuns, func(result BenchmarkResult) float64 { return result.AveragePriceImpact })),
		makePairwiseStatRow("heldout_generalization", "learned_fitted_q", "learned_linucb", "surplus_transfer_gap", "lower_is_better", pairMetric(heldOutFittedQ, heldOutLinUCB, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })),
		makePairwiseStatRow("heldout_generalization", "learned_fitted_q", "learned_linucb", "p99_latency_ms", "lower_is_better", pairMetric(heldOutFittedQ, heldOutLinUCB, func(result BenchmarkResult) float64 { return result.P99LatencyMs })),
		makePairwiseStatRow("calibrated_protocol", "iql", "ppo_clip", "surplus_transfer_gap", "lower_is_better", pairMetric(iqlRuns, ppoRuns, func(result BenchmarkResult) float64 { return result.SurplusTransferGap })),
		makePairwiseStatRow("calibrated_protocol", "iql", "ppo_clip", "p99_latency_ms", "lower_is_better", pairMetric(iqlRuns, ppoRuns, func(result BenchmarkResult) float64 { return result.P99LatencyMs })),
	}
	if err := writeSimulatorStatisticalReviewArtifacts(rows); err != nil {
		t.Fatalf("write statistical review artifacts: %v", err)
	}
}

func TestGenerateSimulatorNecessityArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_NECESSITY") != "1" {
		t.Skip("set RUN_SIM_NECESSITY=1 to generate benchmark-necessity artifacts")
	}

	cfg := defaultCalibratedProtocolConfig()
	variants := []struct {
		Name          string
		Base          ScenarioConfig
		HeldOut       []ScenarioConfig
		RewardWeights RewardWeights
	}{
		{Name: "control", Base: calibratedAdaptiveProtocolBaseScenario(), HeldOut: calibratedHeldOutRegimes(), RewardWeights: cfg.RewardWeights},
		{Name: "matching_only", Base: calibratedMatchingOnlyScenario(), HeldOut: counterfactualHeldOutRegimes(calibratedMatchingOnlyScenario()), RewardWeights: cfg.RewardWeights},
		{Name: "no_settlement", Base: calibratedNoSettlementScenario(), HeldOut: counterfactualHeldOutRegimes(calibratedNoSettlementScenario()), RewardWeights: cfg.RewardWeights},
		{Name: "no_welfare_reward", Base: calibratedAdaptiveProtocolBaseScenario(), HeldOut: calibratedHeldOutRegimes(), RewardWeights: noWelfareRewardWeights()},
	}

	summaries := make([]necessityVariantSummary, 0, len(variants))
	for _, variant := range variants {
		bundles := buildNecessityBundles(variant.Base, variant.RewardWeights)
		rows := make([]necessityPolicySummary, 0, len(bundles))
		for _, bundle := range bundles {
			runs := bundle.Run(variant.HeldOut, cfg.HeldOutSeeds)
			rows = append(rows, necessityPolicySummary{
				Variant:                    variant.Name,
				Policy:                     bundle.Name,
				MeanFillsPerSec:            meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.FillsPerSec }),
				MeanP99LatencyMs:           meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.P99LatencyMs }),
				MeanRetailSurplusPerUnit:   meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit }),
				MeanRetailAdverseSelection: meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate }),
				MeanSurplusTransferGap:     meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.SurplusTransferGap }),
			})
		}
		assignNecessityScoresAndRanks(rows)
		summaries = append(summaries, necessityVariantSummary{
			Variant:        variant.Name,
			BaseScenario:   variant.Base.Name,
			HeldOutRegimes: scenarioNames(variant.HeldOut),
			Policies:       rows,
		})
	}

	if err := writeSimulatorNecessityArtifacts(necessityArtifact{
		ScoreComponents: []string{"z(fills_per_sec)", "-z(p99_latency_ms)", "+z(retail_surplus_per_unit)", "-z(retail_adverse_selection_rate)", "-z(surplus_transfer_gap)"},
		Variants:        summaries,
		RankShifts:      buildNecessityRankShifts(summaries, "control"),
	}); err != nil {
		t.Fatalf("write necessity artifacts: %v", err)
	}
}

func TestGenerateSimulatorWelfareRobustnessArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_WELFARE_ROBUSTNESS") != "1" {
		t.Skip("set RUN_SIM_WELFARE_ROBUSTNESS=1 to generate welfare-robustness artifacts")
	}

	baseSeeds := []int64{223, 227, 229, 233}
	basePolicies := []PolicyController{
		PolicyBurstAware,
		PolicyLearnedLinUCB,
		PolicyLearnedOfflineContextual,
		PolicyLearnedFittedQ,
		PolicyLearnedOnlineDQN,
	}
	baseSuiteRuns := evaluatePolicyControllers("base_heldout", heldOutRegimeScenarios(), baseSeeds, basePolicies)

	strategicSeeds := []int64{521, 523, 541, 547}
	strategicSuiteRuns := evaluatePolicyControllers("strategic_population", strategicAgentScenarios(), strategicSeeds, basePolicies)

	summaries := append([]welfareSuiteSummary(nil), summarizeWelfareSuite("base_heldout", baseSuiteRuns)...)
	summaries = append(summaries, summarizeWelfareSuite("strategic_population", strategicSuiteRuns)...)

	allRuns := append([]BenchmarkResult(nil), baseSuiteRuns...)
	allRuns = append(allRuns, strategicSuiteRuns...)
	correlations := []welfareCorrelationRow{
		makeWelfareCorrelationRow("surplus_transfer_gap", "queue_priority_advantage", allRuns, func(result BenchmarkResult) float64 { return result.SurplusTransferGap }, func(result BenchmarkResult) float64 { return result.QueuePriorityAdvantage }),
		makeWelfareCorrelationRow("surplus_transfer_gap", "latency_arbitrage_profit", allRuns, func(result BenchmarkResult) float64 { return result.SurplusTransferGap }, func(result BenchmarkResult) float64 { return result.LatencyArbitrageProfit }),
		makeWelfareCorrelationRow("surplus_transfer_gap", "average_price_impact", allRuns, func(result BenchmarkResult) float64 { return result.SurplusTransferGap }, func(result BenchmarkResult) float64 { return result.AveragePriceImpact }),
		makeWelfareCorrelationRow("retail_surplus_per_unit", "retail_adverse_selection_rate", allRuns, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit }, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate }),
		makeWelfareCorrelationRow("retail_surplus_per_unit", "surplus_transfer_gap", allRuns, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit }, func(result BenchmarkResult) float64 { return result.SurplusTransferGap }),
	}
	rankStability := []welfareRankStabilityRow{
		makeWelfareRankStabilityRow("base_heldout", "strategic_population", "surplus_transfer_gap", summaries),
		makeWelfareRankStabilityRow("base_heldout", "strategic_population", "retail_surplus_per_unit", summaries),
	}

	if err := writeSimulatorWelfareRobustnessArtifacts(welfareRobustnessArtifact{
		Summaries:     summaries,
		Correlations:  correlations,
		RankStability: rankStability,
	}); err != nil {
		t.Fatalf("write welfare robustness artifacts: %v", err)
	}
}

func TestGenerateSimulatorLeaderboardArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_LEADERBOARD") != "1" {
		t.Skip("set RUN_SIM_LEADERBOARD=1 to generate leaderboard artifacts")
	}

	cfg := defaultCalibratedProtocolConfig()
	base := calibratedAdaptiveProtocolBaseScenario()
	validationRegimes := calibratedValidationRegimes()
	heldOutRegimes := calibratedHeldOutRegimes()

	ppoTrace, ppoModel := trainProtocolPPO(base, cfg, validationRegimes)
	iqlModel, iqlSummary := trainProtocolIQL(base, cfg, validationRegimes)
	fittedQ := cachedFittedQPolicy(base)

	bundles := []policyEvaluationBundle{
		{Name: "burst_aware", Run: func(regimes []ScenarioConfig, seeds []int64) []BenchmarkResult {
			return runChooserAcrossRegimes(regimes, seeds, cfg.RewardWeights, burstAwareChooser(), "burst_aware")
		}},
		{Name: "fitted_q", Run: func(regimes []ScenarioConfig, seeds []int64) []BenchmarkResult {
			return runChooserAcrossRegimes(regimes, seeds, cfg.RewardWeights, fittedQChooser(fittedQ), "fitted_q")
		}},
		{Name: "ppo_clip", Run: func(regimes []ScenarioConfig, seeds []int64) []BenchmarkResult {
			return runChooserAcrossRegimes(regimes, seeds, cfg.RewardWeights, tinyChooser(ppoModel), "ppo_clip")
		}},
		{Name: "iql", Run: func(regimes []ScenarioConfig, seeds []int64) []BenchmarkResult {
			return runChooserAcrossRegimes(regimes, seeds, cfg.RewardWeights, tinyChooser(iqlModel), "iql")
		}},
	}

	entries := make([]leaderboardEntry, 0, len(bundles))
	for _, bundle := range bundles {
		runs := bundle.Run(heldOutRegimes, cfg.HeldOutSeeds)
		entries = append(entries, leaderboardEntry{
			Policy:                         bundle.Name,
			Family:                         leaderboardFamily(bundle.Name),
			TrainingBudget:                 leaderboardBudget(bundle.Name, fittedQ, cfg, len(ppoTrace), iqlSummary),
			MeanFillsPerSec:                meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.FillsPerSec }),
			MeanP99LatencyMs:               meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.P99LatencyMs }),
			MeanAveragePriceImpact:         meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.AveragePriceImpact }),
			MeanRetailSurplusPerUnit:       meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit }),
			MeanRetailAdverseSelectionRate: meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate }),
			MeanSurplusTransferGap:         meanBenchmarkMetric(runs, func(result BenchmarkResult) float64 { return result.SurplusTransferGap }),
			SafetyPassed: sumBenchmarkInt(runs, func(result BenchmarkResult) int {
				return result.NegativeBalanceViolations + result.ConservationBreaches
			}) == 0,
		})
	}
	assignLeaderboardScores(entries)
	points := summarizeLeaderboardFrontier(entries)
	frontierByPolicy := make(map[string]bool, len(points))
	for _, point := range points {
		frontierByPolicy[point.Name] = point.Frontier
	}
	for idx := range entries {
		entries[idx].Frontier = frontierByPolicy[entries[idx].Policy]
	}

	artifact := leaderboardArtifact{
		TaskName:                "calibrated_heldout_latency_welfare_protocol",
		ObservationFeatureCount: len(observationFeatures(Observation{})),
		ActionCount:             len(candidateBanditActions(NewAdapterWithRewardWeights(base, cfg.RewardWeights).ActionSpec())),
		TrainSeeds:              append([]int64(nil), cfg.TrainSeeds...),
		ValidationSeeds:         append([]int64(nil), cfg.ValidationSeeds...),
		HeldOutSeeds:            append([]int64(nil), cfg.HeldOutSeeds...),
		ValidationRegimes:       scenarioNames(validationRegimes),
		HeldOutRegimes:          scenarioNames(heldOutRegimes),
		ScoreFormula:            "z(fills_per_sec) - z(p99_latency_ms) + z(retail_surplus_per_unit) - z(retail_adverse_selection_rate) - z(surplus_transfer_gap)",
		Entries:                 entries,
	}
	if err := writeSimulatorLeaderboardArtifacts(artifact); err != nil {
		t.Fatalf("write leaderboard artifacts: %v", err)
	}
}

func summarizeRuntimeProfile(results []hypercubeSweepResult, scenarios []ScenarioConfig, runsPerCell int, wall time.Duration) runtimeProfileArtifact {
	totalSteps := 0
	episodeSeconds := 0.0
	if len(scenarios) > 0 {
		totalSteps = len(scenarios) * runsPerCell * scenarios[0].TotalSteps
		episodeSeconds = float64(scenarios[0].StepDuration) * float64(scenarios[0].TotalSteps) / float64(time.Second)
	}
	orderEvents := 0.0
	fills := 0.0
	for _, result := range results {
		orderEvents += result.MeanOrdersPerSec * episodeSeconds * float64(runsPerCell)
		fills += result.MeanFillsPerSec * episodeSeconds * float64(runsPerCell)
	}
	wallSeconds := wall.Seconds()
	if wallSeconds <= 0 {
		wallSeconds = 1
	}
	return runtimeProfileArtifact{
		MeasurementScope:              "parameter_hypercube_artifact_generation",
		ScenarioCells:                 len(scenarios),
		RunsPerCell:                   runsPerCell,
		StepsPerRun:                   scenarios[0].TotalSteps,
		StepDurationMs:                scenarios[0].StepDuration.Milliseconds(),
		WallSeconds:                   wallSeconds,
		TotalSteps:                    totalSteps,
		StepsPerSecond:                float64(totalSteps) / wallSeconds,
		EstimatedTotalOrderEvents:     orderEvents,
		EstimatedOrderEventsPerSecond: orderEvents / wallSeconds,
		EstimatedFillsPerSecond:       fills / wallSeconds,
	}
}

func runScenarioFamilyByName(t *testing.T, name string, seeds []int64) []BenchmarkResult {
	t.Helper()
	base := scenarioByName(t, name)
	runs := make([]BenchmarkResult, 0, len(seeds))
	for _, seed := range seeds {
		cfg := base
		cfg.Seed = seed
		runs = append(runs, runScenario(cfg))
	}
	return runs
}

func runHeldOutPolicyFamily(regimes []ScenarioConfig, seeds []int64, policy PolicyController) []BenchmarkResult {
	runs := make([]BenchmarkResult, 0, len(regimes)*len(seeds))
	for _, regime := range regimes {
		for _, seed := range seeds {
			cfg := regime
			cfg.Seed = seed
			cfg.PolicyController = policy
			runs = append(runs, runScenario(cfg))
		}
	}
	return runs
}

func pairMetric(left, right []BenchmarkResult, selector func(BenchmarkResult) float64) []pairedSample {
	if len(left) != len(right) {
		panic("paired metric slices must have equal length")
	}
	out := make([]pairedSample, 0, len(left))
	for idx := range left {
		out = append(out, pairedSample{
			Key:   fmt.Sprintf("%d", idx),
			Left:  selector(left[idx]),
			Right: selector(right[idx]),
		})
	}
	return out
}

func makePairwiseStatRow(experiment, left, right, metric, direction string, samples []pairedSample) pairwiseStatRow {
	leftValues := make([]float64, 0, len(samples))
	rightValues := make([]float64, 0, len(samples))
	diffs := make([]float64, 0, len(samples))
	leftWins := 0
	rightWins := 0
	ties := 0
	for _, sample := range samples {
		leftValues = append(leftValues, sample.Left)
		rightValues = append(rightValues, sample.Right)
		diffs = append(diffs, sample.Left-sample.Right)
		switch direction {
		case "lower_is_better":
			switch {
			case sample.Left < sample.Right:
				leftWins++
			case sample.Left > sample.Right:
				rightWins++
			default:
				ties++
			}
		default:
			switch {
			case sample.Left > sample.Right:
				leftWins++
			case sample.Left < sample.Right:
				rightWins++
			default:
				ties++
			}
		}
	}
	meanDiff, stdDiff := meanStd(diffs)
	aligned := meanDiff
	if direction == "lower_is_better" {
		aligned = -meanDiff
	}
	cohenD := 0.0
	if stdDiff > 1e-9 {
		cohenD = aligned / stdDiff
	}
	return pairwiseStatRow{
		Experiment:     experiment,
		Left:           left,
		Right:          right,
		Metric:         metric,
		Direction:      direction,
		PairedSamples:  len(samples),
		LeftMean:       meanFloatSlice(leftValues),
		RightMean:      meanFloatSlice(rightValues),
		MeanDifference: meanDiff,
		CI95Difference: ci95HalfWidth(stdDiff, len(diffs)),
		AlignedEffect:  aligned,
		CohensD:        cohenD,
		ExactPValue:    exactSignFlipPValue(diffs),
		LeftWins:       leftWins,
		RightWins:      rightWins,
		Ties:           ties,
	}
}

func exactSignFlipPValue(diffs []float64) float64 {
	if len(diffs) == 0 {
		return 1
	}
	if len(diffs) > 20 {
		return 1
	}
	observed := math.Abs(meanFloatSlice(diffs))
	total := 1 << len(diffs)
	extreme := 0
	for mask := 0; mask < total; mask++ {
		sum := 0.0
		for idx, diff := range diffs {
			sign := 1.0
			if mask&(1<<idx) != 0 {
				sign = -1
			}
			sum += sign * diff
		}
		if math.Abs(sum/float64(len(diffs))) >= observed-1e-12 {
			extreme++
		}
	}
	return float64(extreme) / float64(total)
}

func buildNecessityBundles(base ScenarioConfig, rewardWeights RewardWeights) []policyEvaluationBundle {
	cfg := defaultCalibratedProtocolConfig()
	cfg.RewardWeights = rewardWeights
	validationRegimes := counterfactualValidationRegimes(base)
	_, ppoModel := trainProtocolPPO(base, cfg, validationRegimes)
	iqlModel, _ := trainProtocolIQL(base, cfg, validationRegimes)
	fittedQ := cachedFittedQPolicy(base)
	return []policyEvaluationBundle{
		{Name: "burst_aware", Run: func(regimes []ScenarioConfig, seeds []int64) []BenchmarkResult {
			return runChooserAcrossRegimes(regimes, seeds, rewardWeights, burstAwareChooser(), "burst_aware")
		}},
		{Name: "fitted_q", Run: func(regimes []ScenarioConfig, seeds []int64) []BenchmarkResult {
			return runChooserAcrossRegimes(regimes, seeds, rewardWeights, fittedQChooser(fittedQ), "fitted_q")
		}},
		{Name: "ppo_clip", Run: func(regimes []ScenarioConfig, seeds []int64) []BenchmarkResult {
			return runChooserAcrossRegimes(regimes, seeds, rewardWeights, tinyChooser(ppoModel), "ppo_clip")
		}},
		{Name: "iql", Run: func(regimes []ScenarioConfig, seeds []int64) []BenchmarkResult {
			return runChooserAcrossRegimes(regimes, seeds, rewardWeights, tinyChooser(iqlModel), "iql")
		}},
	}
}

func assignNecessityScoresAndRanks(rows []necessityPolicySummary) {
	if len(rows) == 0 {
		return
	}
	fills := make([]float64, 0, len(rows))
	p99 := make([]float64, 0, len(rows))
	surplus := make([]float64, 0, len(rows))
	adverse := make([]float64, 0, len(rows))
	gap := make([]float64, 0, len(rows))
	for _, row := range rows {
		fills = append(fills, row.MeanFillsPerSec)
		p99 = append(p99, row.MeanP99LatencyMs)
		surplus = append(surplus, row.MeanRetailSurplusPerUnit)
		adverse = append(adverse, row.MeanRetailAdverseSelection)
		gap = append(gap, row.MeanSurplusTransferGap)
	}
	fillMean, fillStd := meanStd(fills)
	p99Mean, p99Std := meanStd(p99)
	surplusMean, surplusStd := meanStd(surplus)
	adverseMean, adverseStd := meanStd(adverse)
	gapMean, gapStd := meanStd(gap)
	for idx := range rows {
		rows[idx].BenchmarkScore =
			zScore(rows[idx].MeanFillsPerSec, fillMean, fillStd) -
				zScore(rows[idx].MeanP99LatencyMs, p99Mean, p99Std) +
				zScore(rows[idx].MeanRetailSurplusPerUnit, surplusMean, surplusStd) -
				zScore(rows[idx].MeanRetailAdverseSelection, adverseMean, adverseStd) -
				zScore(rows[idx].MeanSurplusTransferGap, gapMean, gapStd)
	}
	sort.Slice(rows, func(i, j int) bool {
		if rows[i].BenchmarkScore == rows[j].BenchmarkScore {
			return rows[i].Policy < rows[j].Policy
		}
		return rows[i].BenchmarkScore > rows[j].BenchmarkScore
	})
	for idx := range rows {
		rows[idx].Rank = idx + 1
	}
}

func buildNecessityRankShifts(variants []necessityVariantSummary, controlName string) []necessityRankShift {
	control := necessityVariantSummary{}
	for _, variant := range variants {
		if variant.Variant == controlName {
			control = variant
			break
		}
	}
	if control.Variant == "" {
		return nil
	}
	controlRanks := make(map[string]int, len(control.Policies))
	controlFrontier := paretoSetFromNecessity(control.Policies)
	for _, policy := range control.Policies {
		controlRanks[policy.Policy] = policy.Rank
	}
	shifts := make([]necessityRankShift, 0, len(variants)-1)
	for _, variant := range variants {
		if variant.Variant == controlName {
			continue
		}
		common := make([]string, 0, len(variant.Policies))
		variantRanks := make(map[string]int, len(variant.Policies))
		for _, policy := range variant.Policies {
			if _, ok := controlRanks[policy.Policy]; ok {
				common = append(common, policy.Policy)
				variantRanks[policy.Policy] = policy.Rank
			}
		}
		sort.Strings(common)
		frontier := paretoSetFromNecessity(variant.Policies)
		frontierOverlap := 0
		rankChanged := 0
		for _, policy := range common {
			if controlRanks[policy] != variantRanks[policy] {
				rankChanged++
			}
			if controlFrontier[policy] && frontier[policy] {
				frontierOverlap++
			}
		}
		shifts = append(shifts, necessityRankShift{
			Variant:                variant.Variant,
			RelativeTo:             controlName,
			CommonPolicies:         common,
			KendallTau:             kendallTauFromRanks(common, controlRanks, variantRanks),
			FrontierOverlap:        frontierOverlap,
			PoliciesWithRankChange: rankChanged,
		})
	}
	return shifts
}

func paretoSetFromNecessity(rows []necessityPolicySummary) map[string]bool {
	out := make(map[string]bool, len(rows))
	for _, row := range rows {
		dominated := false
		for _, other := range rows {
			if other.Policy == row.Policy {
				continue
			}
			betterOrEqualP99 := other.MeanP99LatencyMs <= row.MeanP99LatencyMs
			betterOrEqualGap := other.MeanSurplusTransferGap <= row.MeanSurplusTransferGap
			strictBetter := other.MeanP99LatencyMs < row.MeanP99LatencyMs || other.MeanSurplusTransferGap < row.MeanSurplusTransferGap
			if betterOrEqualP99 && betterOrEqualGap && strictBetter {
				dominated = true
				break
			}
		}
		out[row.Policy] = !dominated
	}
	return out
}

func evaluatePolicyControllers(suite string, regimes []ScenarioConfig, seeds []int64, policies []PolicyController) []BenchmarkResult {
	runs := make([]BenchmarkResult, 0, len(regimes)*len(seeds)*len(policies))
	for _, regime := range regimes {
		for _, policy := range policies {
			for _, seed := range seeds {
				cfg := regime
				cfg.Seed = seed
				cfg.PolicyController = policy
				result := runScenario(cfg)
				result.Name = fmt.Sprintf("%s|%s|%s|%d", suite, regime.Name, policy, seed)
				runs = append(runs, result)
			}
		}
	}
	return runs
}

func summarizeWelfareSuite(suite string, runs []BenchmarkResult) []welfareSuiteSummary {
	grouped := map[string][]BenchmarkResult{}
	for _, run := range runs {
		parts := strings.Split(run.Name, "|")
		if len(parts) < 3 {
			continue
		}
		grouped[parts[2]] = append(grouped[parts[2]], run)
	}
	policies := make([]string, 0, len(grouped))
	for policy := range grouped {
		policies = append(policies, policy)
	}
	sort.Strings(policies)
	summaries := make([]welfareSuiteSummary, 0, len(policies))
	for _, policy := range policies {
		policyRuns := grouped[policy]
		summaries = append(summaries, welfareSuiteSummary{
			Suite:                      suite,
			Policy:                     policy,
			Runs:                       len(policyRuns),
			MeanP99LatencyMs:           meanBenchmarkMetric(policyRuns, func(result BenchmarkResult) float64 { return result.P99LatencyMs }),
			MeanAveragePriceImpact:     meanBenchmarkMetric(policyRuns, func(result BenchmarkResult) float64 { return result.AveragePriceImpact }),
			MeanQueuePriorityAdvantage: meanBenchmarkMetric(policyRuns, func(result BenchmarkResult) float64 { return result.QueuePriorityAdvantage }),
			MeanLatencyArbitrageProfit: meanBenchmarkMetric(policyRuns, func(result BenchmarkResult) float64 { return result.LatencyArbitrageProfit }),
			MeanRetailSurplusPerUnit:   meanBenchmarkMetric(policyRuns, func(result BenchmarkResult) float64 { return result.RetailSurplusPerUnit }),
			MeanRetailAdverseSelection: meanBenchmarkMetric(policyRuns, func(result BenchmarkResult) float64 { return result.RetailAdverseSelectionRate }),
			MeanSurplusTransferGap:     meanBenchmarkMetric(policyRuns, func(result BenchmarkResult) float64 { return result.SurplusTransferGap }),
		})
	}
	return summaries
}

func makeWelfareCorrelationRow(target, metric string, runs []BenchmarkResult, targetSelector, metricSelector func(BenchmarkResult) float64) welfareCorrelationRow {
	left := make([]float64, 0, len(runs))
	right := make([]float64, 0, len(runs))
	for _, run := range runs {
		left = append(left, targetSelector(run))
		right = append(right, metricSelector(run))
	}
	return welfareCorrelationRow{
		Target:   target,
		Metric:   metric,
		Samples:  len(runs),
		Pearson:  pearsonCorrelation(left, right),
		Spearman: spearmanCorrelation(left, right),
	}
}

func makeWelfareRankStabilityRow(leftSuite, rightSuite, metric string, summaries []welfareSuiteSummary) welfareRankStabilityRow {
	leftRanks, leftPolicies := welfareRanksBySuite(leftSuite, metric, summaries)
	rightRanks, rightPolicies := welfareRanksBySuite(rightSuite, metric, summaries)
	commonSet := map[string]bool{}
	for _, policy := range leftPolicies {
		commonSet[policy] = true
	}
	common := make([]string, 0, len(leftPolicies))
	for _, policy := range rightPolicies {
		if commonSet[policy] {
			common = append(common, policy)
		}
	}
	sort.Strings(common)
	return welfareRankStabilityRow{
		LeftSuite:    leftSuite,
		RightSuite:   rightSuite,
		Metric:       metric,
		CommonPolicy: common,
		KendallTau:   kendallTauFromRanks(common, leftRanks, rightRanks),
	}
}

func welfareRanksBySuite(suite, metric string, summaries []welfareSuiteSummary) (map[string]int, []string) {
	rows := make([]welfareSuiteSummary, 0)
	for _, summary := range summaries {
		if summary.Suite == suite {
			rows = append(rows, summary)
		}
	}
	sort.Slice(rows, func(i, j int) bool {
		switch metric {
		case "retail_surplus_per_unit":
			if rows[i].MeanRetailSurplusPerUnit == rows[j].MeanRetailSurplusPerUnit {
				return rows[i].Policy < rows[j].Policy
			}
			return rows[i].MeanRetailSurplusPerUnit > rows[j].MeanRetailSurplusPerUnit
		default:
			if rows[i].MeanSurplusTransferGap == rows[j].MeanSurplusTransferGap {
				return rows[i].Policy < rows[j].Policy
			}
			return rows[i].MeanSurplusTransferGap < rows[j].MeanSurplusTransferGap
		}
	})
	ranks := make(map[string]int, len(rows))
	policies := make([]string, 0, len(rows))
	for idx, row := range rows {
		ranks[row.Policy] = idx + 1
		policies = append(policies, row.Policy)
	}
	return ranks, policies
}

func kendallTauFromRanks(policies []string, leftRanks, rightRanks map[string]int) float64 {
	if len(policies) < 2 {
		return 1
	}
	concordant := 0
	discordant := 0
	for i := 0; i < len(policies); i++ {
		for j := i + 1; j < len(policies); j++ {
			left := leftRanks[policies[i]] - leftRanks[policies[j]]
			right := rightRanks[policies[i]] - rightRanks[policies[j]]
			product := left * right
			if product > 0 {
				concordant++
			} else if product < 0 {
				discordant++
			}
		}
	}
	total := concordant + discordant
	if total == 0 {
		return 1
	}
	return float64(concordant-discordant) / float64(total)
}

func pearsonCorrelation(left, right []float64) float64 {
	if len(left) == 0 || len(left) != len(right) {
		return 0
	}
	leftMean := meanFloatSlice(left)
	rightMean := meanFloatSlice(right)
	num := 0.0
	denLeft := 0.0
	denRight := 0.0
	for idx := range left {
		dl := left[idx] - leftMean
		dr := right[idx] - rightMean
		num += dl * dr
		denLeft += dl * dl
		denRight += dr * dr
	}
	if denLeft <= 0 || denRight <= 0 {
		return 0
	}
	return num / math.Sqrt(denLeft*denRight)
}

func spearmanCorrelation(left, right []float64) float64 {
	return pearsonCorrelation(rankValues(left), rankValues(right))
}

func rankValues(values []float64) []float64 {
	type indexedValue struct {
		Index int
		Value float64
	}
	ordered := make([]indexedValue, 0, len(values))
	for idx, value := range values {
		ordered = append(ordered, indexedValue{Index: idx, Value: value})
	}
	sort.Slice(ordered, func(i, j int) bool {
		if ordered[i].Value == ordered[j].Value {
			return ordered[i].Index < ordered[j].Index
		}
		return ordered[i].Value < ordered[j].Value
	})
	ranks := make([]float64, len(values))
	for idx := 0; idx < len(ordered); {
		end := idx + 1
		for end < len(ordered) && ordered[end].Value == ordered[idx].Value {
			end++
		}
		avgRank := (float64(idx+1) + float64(end)) / 2
		for pos := idx; pos < end; pos++ {
			ranks[ordered[pos].Index] = avgRank
		}
		idx = end
	}
	return ranks
}

func leaderboardFamily(policy string) string {
	switch policy {
	case "burst_aware":
		return "heuristic"
	case "fitted_q", "iql":
		return "offline_value"
	case "ppo_clip":
		return "online_policy"
	default:
		return "other"
	}
}

func leaderboardBudget(policy string, fittedQ learnedFittedQPolicy, cfg calibratedProtocolConfig, ppoTraceLen int, iqlSummary iqlTrainingSummary) string {
	switch policy {
	case "burst_aware":
		return "no_training"
	case "fitted_q":
		return fmt.Sprintf("%d fitted-q iterations", fittedQ.Iterations)
	case "ppo_clip":
		return fmt.Sprintf("%d episodes x %d epochs", cfg.PPOEpisodes, cfg.PPOPolicyEpochs)
	case "iql":
		return fmt.Sprintf("%d iql iterations (expectile %.2f, beta %.2f)", iqlSummary.Iterations, iqlSummary.Expectile, iqlSummary.Beta)
	default:
		return fmt.Sprintf("%d checkpoints", ppoTraceLen)
	}
}

func assignLeaderboardScores(entries []leaderboardEntry) {
	if len(entries) == 0 {
		return
	}
	fills := make([]float64, 0, len(entries))
	p99 := make([]float64, 0, len(entries))
	surplus := make([]float64, 0, len(entries))
	adverse := make([]float64, 0, len(entries))
	gap := make([]float64, 0, len(entries))
	for _, entry := range entries {
		fills = append(fills, entry.MeanFillsPerSec)
		p99 = append(p99, entry.MeanP99LatencyMs)
		surplus = append(surplus, entry.MeanRetailSurplusPerUnit)
		adverse = append(adverse, entry.MeanRetailAdverseSelectionRate)
		gap = append(gap, entry.MeanSurplusTransferGap)
	}
	fillMean, fillStd := meanStd(fills)
	p99Mean, p99Std := meanStd(p99)
	surplusMean, surplusStd := meanStd(surplus)
	adverseMean, adverseStd := meanStd(adverse)
	gapMean, gapStd := meanStd(gap)
	for idx := range entries {
		entries[idx].BenchmarkScore =
			zScore(entries[idx].MeanFillsPerSec, fillMean, fillStd) -
				zScore(entries[idx].MeanP99LatencyMs, p99Mean, p99Std) +
				zScore(entries[idx].MeanRetailSurplusPerUnit, surplusMean, surplusStd) -
				zScore(entries[idx].MeanRetailAdverseSelectionRate, adverseMean, adverseStd) -
				zScore(entries[idx].MeanSurplusTransferGap, gapMean, gapStd)
	}
	sort.Slice(entries, func(i, j int) bool {
		if entries[i].BenchmarkScore == entries[j].BenchmarkScore {
			return entries[i].Policy < entries[j].Policy
		}
		return entries[i].BenchmarkScore > entries[j].BenchmarkScore
	})
	for idx := range entries {
		entries[idx].Rank = idx + 1
	}
}

func summarizeLeaderboardFrontier(entries []leaderboardEntry) []paretoPoint {
	points := make([]paretoPoint, 0, len(entries))
	for _, entry := range entries {
		points = append(points, paretoPoint{
			Name:                   entry.Policy,
			Category:               entry.Family,
			MeanP99LatencyMs:       entry.MeanP99LatencyMs,
			MeanSurplusTransferGap: entry.MeanSurplusTransferGap,
			MeanFillsPerSec:        entry.MeanFillsPerSec,
		})
	}
	for idx := range points {
		dominated := false
		for jdx := range points {
			if idx == jdx {
				continue
			}
			left := points[jdx]
			right := points[idx]
			betterOrEqualP99 := left.MeanP99LatencyMs <= right.MeanP99LatencyMs
			betterOrEqualGap := left.MeanSurplusTransferGap <= right.MeanSurplusTransferGap
			strictBetter := left.MeanP99LatencyMs < right.MeanP99LatencyMs || left.MeanSurplusTransferGap < right.MeanSurplusTransferGap
			if betterOrEqualP99 && betterOrEqualGap && strictBetter {
				dominated = true
				break
			}
		}
		points[idx].Frontier = !dominated
	}
	return points
}

func zScore(value, mean, std float64) float64 {
	if std <= 1e-9 {
		return 0
	}
	return (value - mean) / std
}

func writeSimulatorRuntimeProfileArtifacts(artifact runtimeProfileArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_runtime_profile.json")
	mdPath := filepath.Join(base, "simulator_runtime_profile.md")
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Runtime Profile\n\n")
	md.WriteString(fmt.Sprintf("Measurement scope: `%s`\n\n", artifact.MeasurementScope))
	md.WriteString(fmt.Sprintf("- scenario cells: `%d`\n", artifact.ScenarioCells))
	md.WriteString(fmt.Sprintf("- runs per cell: `%d`\n", artifact.RunsPerCell))
	md.WriteString(fmt.Sprintf("- steps per run: `%d`\n", artifact.StepsPerRun))
	md.WriteString(fmt.Sprintf("- step duration: `%d ms`\n", artifact.StepDurationMs))
	md.WriteString(fmt.Sprintf("- wall time: `%.4f s`\n", artifact.WallSeconds))
	md.WriteString(fmt.Sprintf("- total steps: `%d`\n", artifact.TotalSteps))
	md.WriteString(fmt.Sprintf("- steps/s: `%.2f`\n", artifact.StepsPerSecond))
	md.WriteString(fmt.Sprintf("- estimated order events/s: `%.2f`\n", artifact.EstimatedOrderEventsPerSecond))
	md.WriteString(fmt.Sprintf("- estimated fills/s: `%.2f`\n\n", artifact.EstimatedFillsPerSecond))
	md.WriteString("Order-event throughput is estimated by summing `mean_orders_per_sec * episode_duration_seconds * runs` across the published hypercube cells and dividing by measured wall-clock time.\n")
	return os.WriteFile(mdPath, []byte(md.String()), 0o644)
}

func writeSimulatorStatisticalReviewArtifacts(rows []pairwiseStatRow) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_statistical_review.json")
	mdPath := filepath.Join(base, "simulator_statistical_review.md")
	csvPath := filepath.Join(base, "simulator_statistical_review.csv")
	raw, err := json.MarshalIndent(map[string]any{"results": rows}, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Statistical Review\n\n")
	md.WriteString("Each row is a paired comparison over shared seeds or shared regime-seed cells. `aligned_effect` is positive when the left side is better under the declared metric direction.\n\n")
	md.WriteString("| Experiment | Left | Right | Metric | Direction | N | Left Mean | Right Mean | Mean Diff | CI95 Diff | Aligned Effect | Cohen's d | Exact p | Left Wins | Right Wins | Ties |\n")
	md.WriteString("|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	var csv strings.Builder
	csv.WriteString("experiment,left,right,metric,direction,paired_samples,left_mean,right_mean,mean_difference,ci95_difference,aligned_effect,cohens_d,exact_sign_flip_p_value,left_wins,right_wins,ties\n")
	for _, row := range rows {
		md.WriteString(fmt.Sprintf("| %s | %s | %s | %s | %s | %d | %.4f | %.4f | %.4f | %.4f | %.4f | %.4f | %.6f | %d | %d | %d |\n",
			row.Experiment, row.Left, row.Right, row.Metric, row.Direction, row.PairedSamples,
			row.LeftMean, row.RightMean, row.MeanDifference, row.CI95Difference, row.AlignedEffect, row.CohensD, row.ExactPValue, row.LeftWins, row.RightWins, row.Ties))
		csv.WriteString(fmt.Sprintf("%s,%s,%s,%s,%s,%d,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.8f,%d,%d,%d\n",
			row.Experiment, row.Left, row.Right, row.Metric, row.Direction, row.PairedSamples,
			row.LeftMean, row.RightMean, row.MeanDifference, row.CI95Difference, row.AlignedEffect, row.CohensD, row.ExactPValue, row.LeftWins, row.RightWins, row.Ties))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorNecessityArtifacts(artifact necessityArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_benchmark_necessity.json")
	mdPath := filepath.Join(base, "simulator_benchmark_necessity.md")
	csvPath := filepath.Join(base, "simulator_benchmark_necessity.csv")
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Benchmark Necessity\n\n")
	md.WriteString("Benchmark score components:\n\n")
	for _, component := range artifact.ScoreComponents {
		md.WriteString(fmt.Sprintf("- `%s`\n", component))
	}
	md.WriteString("\n## Variant Rankings\n\n")
	for _, variant := range artifact.Variants {
		md.WriteString(fmt.Sprintf("### %s\n\n", variant.Variant))
		md.WriteString(fmt.Sprintf("- Base scenario: `%s`\n", variant.BaseScenario))
		md.WriteString(fmt.Sprintf("- Held-out regimes: `%s`\n\n", strings.Join(variant.HeldOutRegimes, ", ")))
		md.WriteString("| Rank | Policy | Score | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |\n")
		md.WriteString("|---:|---|---:|---:|---:|---:|---:|---:|\n")
		for _, row := range variant.Policies {
			md.WriteString(fmt.Sprintf("| %d | %s | %.4f | %.2f | %.2f | %.4f | %.4f | %.4f |\n",
				row.Rank, row.Policy, row.BenchmarkScore, row.MeanFillsPerSec, row.MeanP99LatencyMs, row.MeanRetailSurplusPerUnit, row.MeanRetailAdverseSelection, row.MeanSurplusTransferGap))
		}
		md.WriteString("\n")
	}
	md.WriteString("## Rank Shifts vs Control\n\n")
	md.WriteString("| Variant | Common Policies | Kendall Tau | Frontier Overlap | Policies With Rank Change |\n")
	md.WriteString("|---|---:|---:|---:|---:|\n")
	var csv strings.Builder
	csv.WriteString("section,variant,policy,rank,benchmark_score,mean_fills_per_sec,mean_p99_latency_ms,mean_retail_surplus_per_unit,mean_retail_adverse_selection_rate,mean_surplus_transfer_gap,kendall_tau,frontier_overlap,policies_with_rank_change\n")
	for _, variant := range artifact.Variants {
		for _, row := range variant.Policies {
			csv.WriteString(fmt.Sprintf("variant,%s,%s,%d,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,,,\n",
				variant.Variant, row.Policy, row.Rank, row.BenchmarkScore, row.MeanFillsPerSec, row.MeanP99LatencyMs, row.MeanRetailSurplusPerUnit, row.MeanRetailAdverseSelection, row.MeanSurplusTransferGap))
		}
	}
	for _, shift := range artifact.RankShifts {
		md.WriteString(fmt.Sprintf("| %s | %d | %.4f | %d | %d |\n",
			shift.Variant, len(shift.CommonPolicies), shift.KendallTau, shift.FrontierOverlap, shift.PoliciesWithRankChange))
		csv.WriteString(fmt.Sprintf("rank_shift,%s,,0,0,0,0,0,0,0,%.6f,%d,%d\n", shift.Variant, shift.KendallTau, shift.FrontierOverlap, shift.PoliciesWithRankChange))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorWelfareRobustnessArtifacts(artifact welfareRobustnessArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_welfare_robustness.json")
	mdPath := filepath.Join(base, "simulator_welfare_robustness.md")
	csvPath := filepath.Join(base, "simulator_welfare_robustness.csv")
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Welfare Robustness\n\n")
	md.WriteString("## Suite Summaries\n\n")
	md.WriteString("| Suite | Policy | Runs | p99 (ms) | Impact | Queue Adv. | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	var csv strings.Builder
	csv.WriteString("section,suite,policy,runs,mean_p99_latency_ms,mean_average_price_impact,mean_queue_priority_advantage,mean_latency_arbitrage_profit,mean_retail_surplus_per_unit,mean_retail_adverse_selection_rate,mean_surplus_transfer_gap,target,metric,pearson,spearman,kendall_tau\n")
	for _, summary := range artifact.Summaries {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %.2f | %.2f | %.4f | %.2f | %.4f | %.4f | %.4f |\n",
			summary.Suite, summary.Policy, summary.Runs, summary.MeanP99LatencyMs, summary.MeanAveragePriceImpact, summary.MeanQueuePriorityAdvantage, summary.MeanLatencyArbitrageProfit, summary.MeanRetailSurplusPerUnit, summary.MeanRetailAdverseSelection, summary.MeanSurplusTransferGap))
		csv.WriteString(fmt.Sprintf("summary,%s,%s,%d,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,,,,,\n",
			summary.Suite, summary.Policy, summary.Runs, summary.MeanP99LatencyMs, summary.MeanAveragePriceImpact, summary.MeanQueuePriorityAdvantage, summary.MeanLatencyArbitrageProfit, summary.MeanRetailSurplusPerUnit, summary.MeanRetailAdverseSelection, summary.MeanSurplusTransferGap))
	}
	md.WriteString("\n## Correlations\n\n")
	md.WriteString("| Target | Metric | Samples | Pearson | Spearman |\n")
	md.WriteString("|---|---|---:|---:|---:|\n")
	for _, row := range artifact.Correlations {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %.4f | %.4f |\n", row.Target, row.Metric, row.Samples, row.Pearson, row.Spearman))
		csv.WriteString(fmt.Sprintf("correlation,,,,,,,,,,,%s,%s,%.6f,%.6f,\n", row.Target, row.Metric, row.Pearson, row.Spearman))
	}
	md.WriteString("\n## Rank Stability\n\n")
	md.WriteString("| Left Suite | Right Suite | Metric | Common Policies | Kendall Tau |\n")
	md.WriteString("|---|---|---|---:|---:|\n")
	for _, row := range artifact.RankStability {
		md.WriteString(fmt.Sprintf("| %s | %s | %s | %d | %.4f |\n", row.LeftSuite, row.RightSuite, row.Metric, len(row.CommonPolicy), row.KendallTau))
		csv.WriteString(fmt.Sprintf("rank_stability,%s,%s,0,0,0,0,0,0,0,0,,%s,0,0,%.6f\n", row.LeftSuite, row.RightSuite, row.Metric, row.KendallTau))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorLeaderboardArtifacts(artifact leaderboardArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_policy_leaderboard.json")
	mdPath := filepath.Join(base, "simulator_policy_leaderboard.md")
	csvPath := filepath.Join(base, "simulator_policy_leaderboard.csv")
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Policy Leaderboard\n\n")
	md.WriteString(fmt.Sprintf("- Task: `%s`\n", artifact.TaskName))
	md.WriteString(fmt.Sprintf("- Observation features: `%d`\n", artifact.ObservationFeatureCount))
	md.WriteString(fmt.Sprintf("- Discrete actions: `%d`\n", artifact.ActionCount))
	md.WriteString(fmt.Sprintf("- Train seeds: `%v`\n", artifact.TrainSeeds))
	md.WriteString(fmt.Sprintf("- Validation seeds: `%v`\n", artifact.ValidationSeeds))
	md.WriteString(fmt.Sprintf("- Held-out seeds: `%v`\n", artifact.HeldOutSeeds))
	md.WriteString(fmt.Sprintf("- Validation regimes: `%s`\n", strings.Join(artifact.ValidationRegimes, ", ")))
	md.WriteString(fmt.Sprintf("- Held-out regimes: `%s`\n", strings.Join(artifact.HeldOutRegimes, ", ")))
	md.WriteString(fmt.Sprintf("- Score: `%s`\n\n", artifact.ScoreFormula))
	md.WriteString("| Rank | Policy | Family | Budget | Score | Frontier | Safety | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	var csv strings.Builder
	csv.WriteString("rank,policy,family,training_budget,benchmark_score,frontier,safety_passed,mean_fills_per_sec,mean_p99_latency_ms,mean_average_price_impact,mean_retail_surplus_per_unit,mean_retail_adverse_selection_rate,mean_surplus_transfer_gap\n")
	for _, entry := range artifact.Entries {
		md.WriteString(fmt.Sprintf("| %d | %s | %s | %s | %.4f | %t | %t | %.2f | %.2f | %.2f | %.4f | %.4f | %.4f |\n",
			entry.Rank, entry.Policy, entry.Family, entry.TrainingBudget, entry.BenchmarkScore, entry.Frontier, entry.SafetyPassed, entry.MeanFillsPerSec, entry.MeanP99LatencyMs, entry.MeanAveragePriceImpact, entry.MeanRetailSurplusPerUnit, entry.MeanRetailAdverseSelectionRate, entry.MeanSurplusTransferGap))
		csv.WriteString(fmt.Sprintf("%d,%s,%s,%s,%.6f,%t,%t,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			entry.Rank, entry.Policy, entry.Family, entry.TrainingBudget, entry.BenchmarkScore, entry.Frontier, entry.SafetyPassed, entry.MeanFillsPerSec, entry.MeanP99LatencyMs, entry.MeanAveragePriceImpact, entry.MeanRetailSurplusPerUnit, entry.MeanRetailAdverseSelectionRate, entry.MeanSurplusTransferGap))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}
