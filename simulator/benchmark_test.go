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

type aggregateResult struct {
	Name                           string       `json:"name"`
	Mode                           MatchingMode `json:"mode"`
	BatchWindowMs                  int          `json:"batch_window_ms"`
	SpeedBumpMs                    int          `json:"speed_bump_ms"`
	AdaptiveWindowMinMs            int          `json:"adaptive_window_min_ms"`
	AdaptiveWindowMaxMs            int          `json:"adaptive_window_max_ms"`
	AdaptiveWindowMeanMs           float64      `json:"adaptive_window_mean_ms"`
	Runs                           int          `json:"runs"`
	MeanOrdersPerSec               float64      `json:"mean_orders_per_sec"`
	StdOrdersPerSec                float64      `json:"std_orders_per_sec"`
	CI95OrdersPerSec               float64      `json:"ci95_orders_per_sec"`
	MeanFillsPerSec                float64      `json:"mean_fills_per_sec"`
	StdFillsPerSec                 float64      `json:"std_fills_per_sec"`
	CI95FillsPerSec                float64      `json:"ci95_fills_per_sec"`
	MeanP50LatencyMs               float64      `json:"mean_p50_latency_ms"`
	StdP50LatencyMs                float64      `json:"std_p50_latency_ms"`
	CI95P50LatencyMs               float64      `json:"ci95_p50_latency_ms"`
	MeanP95LatencyMs               float64      `json:"mean_p95_latency_ms"`
	StdP95LatencyMs                float64      `json:"std_p95_latency_ms"`
	CI95P95LatencyMs               float64      `json:"ci95_p95_latency_ms"`
	MeanP99LatencyMs               float64      `json:"mean_p99_latency_ms"`
	StdP99LatencyMs                float64      `json:"std_p99_latency_ms"`
	CI95P99LatencyMs               float64      `json:"ci95_p99_latency_ms"`
	MeanAverageSpread              float64      `json:"mean_average_spread"`
	CI95AverageSpread              float64      `json:"ci95_average_spread"`
	MeanAveragePriceImpact         float64      `json:"mean_average_price_impact"`
	CI95AveragePriceImpact         float64      `json:"ci95_average_price_impact"`
	MeanQueuePriorityAdvantage     float64      `json:"mean_queue_priority_advantage"`
	CI95QueuePriorityAdvantage     float64      `json:"ci95_queue_priority_advantage"`
	MeanLatencyArbitrageProfit     float64      `json:"mean_latency_arbitrage_profit"`
	CI95LatencyArbitrageProfit     float64      `json:"ci95_latency_arbitrage_profit"`
	MeanExecutionDispersion        float64      `json:"mean_execution_dispersion"`
	CI95ExecutionDispersion        float64      `json:"ci95_execution_dispersion"`
	MeanRetailSurplusPerUnit       float64      `json:"mean_retail_surplus_per_unit"`
	CI95RetailSurplusPerUnit       float64      `json:"ci95_retail_surplus_per_unit"`
	MeanArbitrageurSurplusPerUnit  float64      `json:"mean_arbitrageur_surplus_per_unit"`
	CI95ArbitrageurSurplusPerUnit  float64      `json:"ci95_arbitrageur_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64      `json:"mean_retail_adverse_selection_rate"`
	CI95RetailAdverseSelectionRate float64      `json:"ci95_retail_adverse_selection_rate"`
	MeanWelfareDispersion          float64      `json:"mean_welfare_dispersion"`
	CI95WelfareDispersion          float64      `json:"ci95_welfare_dispersion"`
	MeanSurplusTransferGap         float64      `json:"mean_surplus_transfer_gap"`
	CI95SurplusTransferGap         float64      `json:"ci95_surplus_transfer_gap"`
	NegativeBalanceViolationsTotal int          `json:"negative_balance_violations_total"`
	ConservationBreachesTotal      int          `json:"conservation_breaches_total"`
	RiskRejectionsTotal            int          `json:"risk_rejections_total"`
}

type gridSweepResult struct {
	ArbitrageurIntensityMultiplier int     `json:"arbitrageur_intensity_multiplier"`
	MakerQuoteWidthMultiplier      int     `json:"maker_quote_width_multiplier"`
	Runs                           int     `json:"runs"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	CI95OrdersPerSec               float64 `json:"ci95_orders_per_sec"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	CI95FillsPerSec                float64 `json:"ci95_fills_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	CI95P99LatencyMs               float64 `json:"ci95_p99_latency_ms"`
	MeanAveragePriceImpact         float64 `json:"mean_average_price_impact"`
	CI95AveragePriceImpact         float64 `json:"ci95_average_price_impact"`
	MeanQueuePriorityAdvantage     float64 `json:"mean_queue_priority_advantage"`
	CI95QueuePriorityAdvantage     float64 `json:"ci95_queue_priority_advantage"`
	MeanLatencyArbitrageProfit     float64 `json:"mean_latency_arbitrage_profit"`
	CI95LatencyArbitrageProfit     float64 `json:"ci95_latency_arbitrage_profit"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	CI95RetailSurplusPerUnit       float64 `json:"ci95_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	CI95RetailAdverseSelectionRate float64 `json:"ci95_retail_adverse_selection_rate"`
}

type cubeSweepResult struct {
	RetailIntensityMultiplier      int     `json:"retail_intensity_multiplier"`
	InformedIntensityMultiplier    int     `json:"informed_intensity_multiplier"`
	MakerQuoteWidthMultiplier      int     `json:"maker_quote_width_multiplier"`
	Runs                           int     `json:"runs"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	CI95OrdersPerSec               float64 `json:"ci95_orders_per_sec"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	CI95FillsPerSec                float64 `json:"ci95_fills_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	CI95P99LatencyMs               float64 `json:"ci95_p99_latency_ms"`
	MeanAveragePriceImpact         float64 `json:"mean_average_price_impact"`
	CI95AveragePriceImpact         float64 `json:"ci95_average_price_impact"`
	MeanQueuePriorityAdvantage     float64 `json:"mean_queue_priority_advantage"`
	CI95QueuePriorityAdvantage     float64 `json:"ci95_queue_priority_advantage"`
	MeanLatencyArbitrageProfit     float64 `json:"mean_latency_arbitrage_profit"`
	CI95LatencyArbitrageProfit     float64 `json:"ci95_latency_arbitrage_profit"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	CI95RetailSurplusPerUnit       float64 `json:"ci95_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	CI95RetailAdverseSelectionRate float64 `json:"ci95_retail_adverse_selection_rate"`
}

type hypercubeSweepResult struct {
	ArbitrageurIntensityMultiplier int     `json:"arbitrageur_intensity_multiplier"`
	RetailIntensityMultiplier      int     `json:"retail_intensity_multiplier"`
	InformedIntensityMultiplier    int     `json:"informed_intensity_multiplier"`
	MakerQuoteWidthMultiplier      int     `json:"maker_quote_width_multiplier"`
	Runs                           int     `json:"runs"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	CI95OrdersPerSec               float64 `json:"ci95_orders_per_sec"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	CI95FillsPerSec                float64 `json:"ci95_fills_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	CI95P99LatencyMs               float64 `json:"ci95_p99_latency_ms"`
	MeanAveragePriceImpact         float64 `json:"mean_average_price_impact"`
	CI95AveragePriceImpact         float64 `json:"ci95_average_price_impact"`
	MeanQueuePriorityAdvantage     float64 `json:"mean_queue_priority_advantage"`
	CI95QueuePriorityAdvantage     float64 `json:"ci95_queue_priority_advantage"`
	MeanLatencyArbitrageProfit     float64 `json:"mean_latency_arbitrage_profit"`
	CI95LatencyArbitrageProfit     float64 `json:"ci95_latency_arbitrage_profit"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	CI95RetailSurplusPerUnit       float64 `json:"ci95_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	CI95RetailAdverseSelectionRate float64 `json:"ci95_retail_adverse_selection_rate"`
	MeanWelfareDispersion          float64 `json:"mean_welfare_dispersion"`
	CI95WelfareDispersion          float64 `json:"ci95_welfare_dispersion"`
	MeanSurplusTransferGap         float64 `json:"mean_surplus_transfer_gap"`
	CI95SurplusTransferGap         float64 `json:"ci95_surplus_transfer_gap"`
}

type hypercubeFactorLevelSummary struct {
	Factor                         string  `json:"factor"`
	Level                          int     `json:"level"`
	CellCount                      int     `json:"cell_count"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	MeanLatencyArbitrageProfit     float64 `json:"mean_latency_arbitrage_profit"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	MeanSurplusTransferGap         float64 `json:"mean_surplus_transfer_gap"`
}

type hypercubeHighLowContrast struct {
	Factor                      string  `json:"factor"`
	LowLevel                    int     `json:"low_level"`
	HighLevel                   int     `json:"high_level"`
	DeltaOrdersPerSec           float64 `json:"delta_orders_per_sec"`
	DeltaP99LatencyMs           float64 `json:"delta_p99_latency_ms"`
	DeltaLatencyArbitrageProfit float64 `json:"delta_latency_arbitrage_profit"`
	DeltaRetailSurplusPerUnit   float64 `json:"delta_retail_surplus_per_unit"`
	DeltaRetailAdverseSelection float64 `json:"delta_retail_adverse_selection_rate"`
	DeltaSurplusTransferGap     float64 `json:"delta_surplus_transfer_gap"`
}

type retailConditionedArbitrageEffect struct {
	RetailIntensityMultiplier   int     `json:"retail_intensity_multiplier"`
	DeltaOrdersPerSec           float64 `json:"delta_orders_per_sec"`
	DeltaP99LatencyMs           float64 `json:"delta_p99_latency_ms"`
	DeltaLatencyArbitrageProfit float64 `json:"delta_latency_arbitrage_profit"`
	DeltaRetailSurplusPerUnit   float64 `json:"delta_retail_surplus_per_unit"`
	DeltaRetailAdverseSelection float64 `json:"delta_retail_adverse_selection_rate"`
	DeltaSurplusTransferGap     float64 `json:"delta_surplus_transfer_gap"`
}

type hypercubeCompactSummary struct {
	Seeds                      []int64                                  `json:"seeds"`
	PrimaryWelfareMetrics      []string                                 `json:"primary_welfare_metrics"`
	MainEffects                map[string][]hypercubeFactorLevelSummary `json:"main_effects"`
	HighLowContrasts           []hypercubeHighLowContrast               `json:"high_low_contrasts"`
	RetailConditionedArbitrage []retailConditionedArbitrageEffect       `json:"retail_conditioned_arbitrage"`
}

type heldOutPolicyResult struct {
	RegimeName                     string  `json:"regime_name"`
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
}

type heldOutPolicySummary struct {
	Policy                         string  `json:"policy"`
	RegimeCount                    int     `json:"regime_count"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	MeanAveragePriceImpact         float64 `json:"mean_average_price_impact"`
	MeanRetailSurplusPerUnit       float64 `json:"mean_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64 `json:"mean_retail_adverse_selection_rate"`
	MeanSurplusTransferGap         float64 `json:"mean_surplus_transfer_gap"`
}

type fittedQLearningCurvePoint struct {
	Iteration                      int     `json:"iteration"`
	MeanBellmanMSE                 float64 `json:"mean_bellman_mse"`
	Runs                           int     `json:"runs"`
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
}

type onlineDQNLearningCurvePoint struct {
	Episode                        int     `json:"episode"`
	MeanEpisodeReward              float64 `json:"mean_episode_reward"`
	Runs                           int     `json:"runs"`
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
}

type rewardSensitivityProfile struct {
	Name          string        `json:"name"`
	RewardWeights RewardWeights `json:"reward_weights"`
}

type onlineDQNRewardSensitivityResult struct {
	ProfileName                    string        `json:"profile_name"`
	RewardWeights                  RewardWeights `json:"reward_weights"`
	MeanTrainEpisodeReward         float64       `json:"mean_train_episode_reward"`
	Runs                           int           `json:"runs"`
	MeanFillsPerSec                float64       `json:"mean_fills_per_sec"`
	CI95FillsPerSec                float64       `json:"ci95_fills_per_sec"`
	MeanP99LatencyMs               float64       `json:"mean_p99_latency_ms"`
	CI95P99LatencyMs               float64       `json:"ci95_p99_latency_ms"`
	MeanRetailSurplusPerUnit       float64       `json:"mean_retail_surplus_per_unit"`
	CI95RetailSurplusPerUnit       float64       `json:"ci95_retail_surplus_per_unit"`
	MeanRetailAdverseSelectionRate float64       `json:"mean_retail_adverse_selection_rate"`
	CI95RetailAdverseSelectionRate float64       `json:"ci95_retail_adverse_selection_rate"`
	MeanSurplusTransferGap         float64       `json:"mean_surplus_transfer_gap"`
	CI95SurplusTransferGap         float64       `json:"ci95_surplus_transfer_gap"`
}

type doubleDQNLearningCurvePoint struct {
	Episode                        int     `json:"episode"`
	MeanEpisodeReward              float64 `json:"mean_episode_reward"`
	Runs                           int     `json:"runs"`
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
}

type strategicAgentResult struct {
	Name                           string  `json:"name"`
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
}

type paretoPoint struct {
	Name                   string  `json:"name"`
	Category               string  `json:"category"`
	MeanP99LatencyMs       float64 `json:"mean_p99_latency_ms"`
	CI95P99LatencyMs       float64 `json:"ci95_p99_latency_ms"`
	MeanSurplusTransferGap float64 `json:"mean_surplus_transfer_gap"`
	CI95SurplusTransferGap float64 `json:"ci95_surplus_transfer_gap"`
	MeanFillsPerSec        float64 `json:"mean_fills_per_sec"`
	CI95FillsPerSec        float64 `json:"ci95_fills_per_sec"`
	Frontier               bool    `json:"frontier"`
}

type responseSurfaceCoefficient struct {
	Name        string  `json:"name"`
	Coefficient float64 `json:"coefficient"`
}

type responseSurfaceEffect struct {
	Factor    string  `json:"factor"`
	PartialR2 float64 `json:"partial_r2"`
}

type responseSurfaceFit struct {
	Metric       string                       `json:"metric"`
	R2           float64                      `json:"r2"`
	RMSE         float64                      `json:"rmse"`
	Coefficients []responseSurfaceCoefficient `json:"coefficients"`
	Effects      []responseSurfaceEffect      `json:"effects"`
}

type hypercubeResponseSurfaceSummary struct {
	Seeds []int64              `json:"seeds"`
	Fits  []responseSurfaceFit `json:"fits"`
}

func simulatorScenarios() []ScenarioConfig {
	return []ScenarioConfig{
		{
			Name:             "Immediate-Surrogate",
			Mode:             ModeImmediate,
			BatchWindowSteps: 1,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       120,
			Seed:             42,
			Agents:           DefaultPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:           "SpeedBump-50ms",
			Mode:           ModeSpeedBump,
			SpeedBumpSteps: 5,
			StepDuration:   10 * time.Millisecond,
			TotalSteps:     120,
			Seed:           42,
			Agents:         DefaultPopulation(),
			Risk:           RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:             "FBA-100ms",
			Mode:             ModeBatch,
			BatchWindowSteps: 10,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       120,
			Seed:             42,
			Agents:           DefaultPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:             "FBA-250ms",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             42,
			Agents:           DefaultPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:             "FBA-500ms",
			Mode:             ModeBatch,
			BatchWindowSteps: 50,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       150,
			Seed:             42,
			Agents:           DefaultPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Adaptive-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Adaptive-OrderFlow-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveOrderFlow,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 9,
			AdaptiveQueueThreshold: 16,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Adaptive-QueueLoad-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveQueueLoad,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 14,
			AdaptiveQueueThreshold: 10,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Policy-BurstAware-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			PolicyController:       PolicyBurstAware,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Policy-LearnedLinUCB-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			PolicyController:       PolicyLearnedLinUCB,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Policy-LearnedTinyMLP-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			PolicyController:       PolicyLearnedTinyMLP,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Policy-LearnedOfflineContextual-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			PolicyController:       PolicyLearnedOfflineContextual,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Policy-LearnedFittedQ-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			PolicyController:       PolicyLearnedFittedQ,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Policy-LearnedOnlineDQN-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			PolicyController:       PolicyLearnedOnlineDQN,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   42,
			Agents:                 DefaultPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:             "FBA-250ms-Stress",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             99,
			Agents:           StressPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 10, MaxOrdersPerStep: 36},
		},
	}
}

func runScenario(cfg ScenarioConfig) BenchmarkResult {
	if cfg.PolicyController != PolicyNone {
		return NewAdapter(cfg).RunPolicy(cfg.PolicyController)
	}
	return NewEnvironment(cfg).Run()
}

func ablationScenarios() []ScenarioConfig {
	return []ScenarioConfig{
		{
			Name:             "Ablation-Control",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             77,
			Agents:           StressPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 5, MaxOrdersPerStep: 12},
		},
		{
			Name:              "Ablation-RelaxedRisk",
			Mode:              ModeBatch,
			BatchWindowSteps:  25,
			DisableRiskLimits: true,
			StepDuration:      10 * time.Millisecond,
			TotalSteps:        125,
			Seed:              77,
			Agents:            StressPopulation(),
			Risk:              RiskConfig{MaxOrderAmount: 5, MaxOrdersPerStep: 12},
		},
		{
			Name:                   "Ablation-RandomTieBreak",
			Mode:                   ModeBatch,
			BatchWindowSteps:       25,
			RandomizeBatchTieBreak: true,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   77,
			Agents:                 StressPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 5, MaxOrdersPerStep: 12},
		},
		{
			Name:                    "Ablation-NoSettlementChecks",
			Mode:                    ModeBatch,
			BatchWindowSteps:        25,
			DisableSettlementChecks: true,
			StepDuration:            10 * time.Millisecond,
			TotalSteps:              125,
			Seed:                    77,
			Agents:                  StressPopulation(),
			Risk:                    RiskConfig{MaxOrderAmount: 5, MaxOrdersPerStep: 12},
		},
	}
}

func agentWorkloadAblationScenarios() []ScenarioConfig {
	return []ScenarioConfig{
		{
			Name:             "AgentAblation-Control",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             151,
			Agents:           DefaultPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:             "AgentAblation-NoArbitrageurs",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             151,
			Agents:           WithoutAgentClass(DefaultPopulation(), AgentArbitrageur),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:             "AgentAblation-NoInformed",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             151,
			Agents:           WithoutAgentClass(DefaultPopulation(), AgentInformed),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:             "AgentSweep-RetailBurst",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             151,
			Agents:           RetailBurstPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 28},
		},
		{
			Name:             "AgentSweep-ArbIntensityHigh",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             151,
			Agents:           AdjustClassBaseSize(ScaleClassIntensity(DefaultPopulation(), AgentArbitrageur, 2, 1), AgentArbitrageur, 1),
			Risk:             RiskConfig{MaxOrderAmount: 9, MaxOrdersPerStep: 28},
		},
		{
			Name:             "AgentSweep-InformedIntensityHigh",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             151,
			Agents:           AdjustClassBaseSize(ScaleClassIntensity(DefaultPopulation(), AgentInformed, 2, 1), AgentInformed, 1),
			Risk:             RiskConfig{MaxOrderAmount: 9, MaxOrdersPerStep: 26},
		},
		{
			Name:             "AgentSweep-MakersWide",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             151,
			Agents:           AdjustClassQuoteWidth(DefaultPopulation(), AgentMarketMaker, 2),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
	}
}

func scenarioByName(t *testing.T, name string) ScenarioConfig {
	t.Helper()
	for _, cfg := range simulatorScenarios() {
		if cfg.Name == name {
			return cfg
		}
	}
	t.Fatalf("scenario %q not found", name)
	return ScenarioConfig{}
}

func onlineDQNRewardProfiles() []rewardSensitivityProfile {
	return []rewardSensitivityProfile{
		{
			Name:          "default",
			RewardWeights: defaultRewardWeights(),
		},
		{
			Name: "latency_heavy",
			RewardWeights: RewardWeights{
				FillWeight:          2.00,
				SpreadPenalty:       0.10,
				PriceImpactPenalty:  0.20,
				QueuePenalty:        4.0,
				ArbitragePenalty:    0.0005,
				RetailSurplusWeight: 1.0,
				AdversePenalty:      1.0,
				WelfarePenalty:      0.1,
				SurplusGapPenalty:   0.1,
				RiskRejectPenalty:   0.2,
				ConservationPenalty: 10.0,
			},
		},
		{
			Name: "welfare_heavy",
			RewardWeights: RewardWeights{
				FillWeight:          0.10,
				SpreadPenalty:       0.50,
				PriceImpactPenalty:  4.00,
				QueuePenalty:        20.0,
				ArbitragePenalty:    0.0500,
				RetailSurplusWeight: 40.0,
				AdversePenalty:      30.0,
				WelfarePenalty:      8.0,
				SurplusGapPenalty:   12.0,
				RiskRejectPenalty:   1.0,
				ConservationPenalty: 10.0,
			},
		},
	}
}

func strategicAgentScenarios() []ScenarioConfig {
	return []ScenarioConfig{
		{
			Name:                   "Strategic-Control",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   521,
			Agents:                 StrategicPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
		},
		{
			Name:                   "Strategic-HighArb",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   521,
			Agents:                 AdjustClassBaseSize(ScaleClassIntensity(StrategicPopulation(), AgentArbitrageur, 3, 1), AgentArbitrageur, 1),
			Risk:                   RiskConfig{MaxOrderAmount: 9, MaxOrdersPerStep: 28},
		},
		{
			Name:                   "Strategic-RetailBurst",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   521,
			Agents:                 ScaleClassIntensity(StrategicPopulation(), AgentRetail, 3, 1),
			Risk:                   RiskConfig{MaxOrderAmount: 9, MaxOrdersPerStep: 28},
		},
	}
}

func parameterGridScenarios() []ScenarioConfig {
	arbMultipliers := []int{0, 1, 2, 3}
	makerWidths := []int{1, 2, 3}
	scenarios := make([]ScenarioConfig, 0, len(arbMultipliers)*len(makerWidths))
	for _, arb := range arbMultipliers {
		for _, maker := range makerWidths {
			agents := DefaultPopulation()
			switch {
			case arb == 0:
				agents = WithoutAgentClass(agents, AgentArbitrageur)
			case arb > 1:
				agents = AdjustClassBaseSize(ScaleClassIntensity(agents, AgentArbitrageur, arb, 1), AgentArbitrageur, 1)
			}
			if maker > 1 {
				agents = AdjustClassQuoteWidth(agents, AgentMarketMaker, int64(maker))
			}
			scenarios = append(scenarios, ScenarioConfig{
				Name:             fmt.Sprintf("Grid-Arb%d-Maker%d", arb, maker),
				Mode:             ModeBatch,
				BatchWindowSteps: 25,
				StepDuration:     10 * time.Millisecond,
				TotalSteps:       125,
				Seed:             201,
				Agents:           agents,
				Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
			})
		}
	}
	return scenarios
}

func parameterCubeScenarios() []ScenarioConfig {
	retailMultipliers := []int{1, 2, 3}
	informedMultipliers := []int{1, 2, 3}
	makerMultipliers := []int{1, 2, 3}
	scenarios := make([]ScenarioConfig, 0, len(retailMultipliers)*len(informedMultipliers)*len(makerMultipliers))
	for _, retail := range retailMultipliers {
		for _, informed := range informedMultipliers {
			for _, maker := range makerMultipliers {
				agents := DefaultPopulation()
				if retail > 1 {
					agents = ScaleClassIntensity(agents, AgentRetail, retail, 1)
				}
				if informed > 1 {
					agents = ScaleClassIntensity(agents, AgentInformed, informed, 1)
				}
				if maker > 1 {
					agents = ScaleClassQuoteWidth(agents, AgentMarketMaker, maker, 1)
				}
				scenarios = append(scenarios, ScenarioConfig{
					Name:             fmt.Sprintf("Cube-Retail%d-Informed%d-Maker%d", retail, informed, maker),
					Mode:             ModeBatch,
					BatchWindowSteps: 25,
					StepDuration:     10 * time.Millisecond,
					TotalSteps:       125,
					Seed:             241,
					Agents:           agents,
					Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
				})
			}
		}
	}
	return scenarios
}

func parameterHypercubeScenarios() []ScenarioConfig {
	arbMultipliers := []int{0, 1, 2, 3}
	retailMultipliers := []int{1, 2, 3}
	informedMultipliers := []int{1, 2, 3}
	makerMultipliers := []int{1, 2, 3}
	scenarios := make([]ScenarioConfig, 0, len(arbMultipliers)*len(retailMultipliers)*len(informedMultipliers)*len(makerMultipliers))
	for _, arb := range arbMultipliers {
		for _, retail := range retailMultipliers {
			for _, informed := range informedMultipliers {
				for _, maker := range makerMultipliers {
					agents := DefaultPopulation()
					switch {
					case arb == 0:
						agents = WithoutAgentClass(agents, AgentArbitrageur)
					case arb > 1:
						agents = AdjustClassBaseSize(ScaleClassIntensity(agents, AgentArbitrageur, arb, 1), AgentArbitrageur, 1)
					}
					if retail > 1 {
						agents = ScaleClassIntensity(agents, AgentRetail, retail, 1)
					}
					if informed > 1 {
						agents = ScaleClassIntensity(agents, AgentInformed, informed, 1)
					}
					if maker > 1 {
						agents = ScaleClassQuoteWidth(agents, AgentMarketMaker, maker, 1)
					}
					scenarios = append(scenarios, ScenarioConfig{
						Name:             fmt.Sprintf("Hyper-Arb%d-Retail%d-Informed%d-Maker%d", arb, retail, informed, maker),
						Mode:             ModeBatch,
						BatchWindowSteps: 25,
						StepDuration:     10 * time.Millisecond,
						TotalSteps:       125,
						Seed:             281,
						Agents:           agents,
						Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
					})
				}
			}
		}
	}
	return scenarios
}

func heldOutRegimeScenarios() []ScenarioConfig {
	return []ScenarioConfig{
		{
			Name:                   "HeldOut-HighArbWideMaker",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   211,
			Agents:                 ScaleClassQuoteWidth(AdjustClassBaseSize(ScaleClassIntensity(DefaultPopulation(), AgentArbitrageur, 3, 1), AgentArbitrageur, 1), AgentMarketMaker, 3, 1),
			Risk:                   RiskConfig{MaxOrderAmount: 9, MaxOrdersPerStep: 28},
		},
		{
			Name:                   "HeldOut-RetailBurst",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   211,
			Agents:                 RetailBurstPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 9, MaxOrdersPerStep: 30},
		},
		{
			Name:                   "HeldOut-InformedWide",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   211,
			Agents:                 ScaleClassQuoteWidth(ScaleClassIntensity(DefaultPopulation(), AgentInformed, 3, 1), AgentMarketMaker, 2, 1),
			Risk:                   RiskConfig{MaxOrderAmount: 9, MaxOrdersPerStep: 28},
		},
		{
			Name:                   "HeldOut-CompositeStress",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10,
			AdaptiveMaxWindowSteps: 25,
			AdaptiveOrderThreshold: 10,
			AdaptiveQueueThreshold: 12,
			StepDuration:           10 * time.Millisecond,
			TotalSteps:             125,
			Seed:                   211,
			Agents:                 ScaleClassQuoteWidth(ScaleClassIntensity(ScaleClassIntensity(DefaultPopulation(), AgentRetail, 3, 1), AgentInformed, 2, 1), AgentMarketMaker, 3, 1),
			Risk:                   RiskConfig{MaxOrderAmount: 10, MaxOrdersPerStep: 30},
		},
	}
}

func heldOutPolicies() []PolicyController {
	return []PolicyController{
		PolicyBurstAware,
		PolicyLearnedLinUCB,
		PolicyLearnedTinyMLP,
		PolicyLearnedOfflineContextual,
		PolicyLearnedFittedQ,
		PolicyLearnedOnlineDQN,
	}
}

func TestSimulatorDeterminism(t *testing.T) {
	cfg := scenarioByName(t, "SpeedBump-50ms")
	left := runScenario(cfg)
	right := runScenario(cfg)

	if left.Fills != right.Fills ||
		left.OrdersAccepted != right.OrdersAccepted ||
		left.QueuePriorityAdvantage != right.QueuePriorityAdvantage ||
		left.LatencyArbitrageProfit != right.LatencyArbitrageProfit {
		t.Fatalf("expected deterministic benchmark outputs, left=%+v right=%+v", left, right)
	}
}

func TestSimulatorSettlementSafety(t *testing.T) {
	cfg := scenarioByName(t, "FBA-250ms-Stress")
	result := runScenario(cfg)
	if result.NegativeBalanceViolations != 0 {
		t.Fatalf("expected no negative balances, got %d", result.NegativeBalanceViolations)
	}
	if result.ConservationBreaches != 0 {
		t.Fatalf("expected no conservation breaches, got %d", result.ConservationBreaches)
	}
}

func TestEnvironmentStepAPI(t *testing.T) {
	env := NewEnvironment(scenarioByName(t, "Adaptive-100-250ms"))
	initial := env.Reset()
	if initial.Done {
		t.Fatalf("expected fresh environment to be running")
	}
	if initial.CurrentBatchWindowStep != 10 {
		t.Fatalf("expected initial adaptive window to be 10, got %d", initial.CurrentBatchWindowStep)
	}

	var last Observation
	for i := 0; i < 4; i++ {
		step := env.Step()
		last = step.Observation
		if step.Observation.Step <= initial.Step {
			t.Fatalf("expected step counter to advance, got %+v", step.Observation)
		}
	}
	if last.Done {
		t.Fatalf("expected environment not to be done after 4 steps")
	}
	if last.Mode != ModeAdaptiveBatch {
		t.Fatalf("expected adaptive mode observation, got %s", last.Mode)
	}
}

func TestAdaptiveBatchWindowSummary(t *testing.T) {
	result := runScenario(scenarioByName(t, "Adaptive-100-250ms"))
	if result.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected adaptive window mean to be populated, got %+v", result)
	}
	if result.AdaptiveWindowMaxMs < result.AdaptiveWindowMinMs {
		t.Fatalf("expected adaptive max >= min, got %+v", result)
	}
}

func TestAdapterResetAndStep(t *testing.T) {
	adapter := NewAdapter(scenarioByName(t, "Adaptive-100-250ms"))
	initial := adapter.Reset()
	if initial.Done {
		t.Fatalf("expected fresh adapter reset to be running")
	}
	if !initial.Info.ActionSpec.SupportsBatchWindowControl {
		t.Fatalf("expected adaptive scenario to expose batch-window control")
	}
	target := 25
	next := adapter.Step(ControlAction{TargetBatchWindowSteps: &target})
	if next.Info.AppliedAction.TargetBatchWindowSteps == nil {
		t.Fatalf("expected adapter to report applied action")
	}
	if *next.Info.AppliedAction.TargetBatchWindowSteps != 25 {
		t.Fatalf("expected target window 25, got %+v", next.Info.AppliedAction)
	}
	if next.Info.CurrentBatchWindowMs != 250 {
		t.Fatalf("expected current window 250ms, got %+v", next.Info)
	}
	if !next.Info.ActionSpec.SupportsRiskLimitScale || !next.Info.ActionSpec.SupportsTieBreakToggle {
		t.Fatalf("expected expanded action space, got %+v", next.Info.ActionSpec)
	}
	if !next.Info.ActionSpec.SupportsReleaseCadence || !next.Info.ActionSpec.SupportsPriceAggressionBias {
		t.Fatalf("expected release cadence and price aggression controls, got %+v", next.Info.ActionSpec)
	}
}

func TestAdapterAppliesExtendedControls(t *testing.T) {
	adapter := NewAdapter(scenarioByName(t, "Adaptive-100-250ms"))
	adapter.Reset()
	cadence := 18
	bias := int64(1)
	step := adapter.Step(ControlAction{
		ReleaseCadenceSteps: &cadence,
		PriceAggressionBias: &bias,
	})
	if step.Info.AppliedAction.ReleaseCadenceSteps == nil || *step.Info.AppliedAction.ReleaseCadenceSteps != cadence {
		t.Fatalf("expected release cadence to be applied, got %+v", step.Info.AppliedAction)
	}
	if step.Info.AppliedAction.PriceAggressionBias == nil || *step.Info.AppliedAction.PriceAggressionBias != bias {
		t.Fatalf("expected price aggression bias to be applied, got %+v", step.Info.AppliedAction)
	}
	if step.Info.CurrentReleaseCadenceMs != 180 {
		t.Fatalf("expected release cadence 180ms, got %+v", step.Info)
	}
	if step.Info.CurrentPriceAggression != 1 {
		t.Fatalf("expected current price aggression 1, got %+v", step.Info)
	}
}

func TestAdapterIgnoresActionOutsideAdaptiveMode(t *testing.T) {
	adapter := NewAdapter(scenarioByName(t, "SpeedBump-50ms"))
	initial := adapter.Reset()
	if initial.Info.ActionSpec.SupportsBatchWindowControl {
		t.Fatalf("expected speed-bump scenario not to expose adaptive control")
	}
	target := 99
	next := adapter.Step(ControlAction{TargetBatchWindowSteps: &target})
	if next.Info.AppliedAction.TargetBatchWindowSteps != nil {
		t.Fatalf("expected no applied action for non-adaptive mode, got %+v", next.Info.AppliedAction)
	}
	if !next.Info.ActionSpec.SupportsRiskLimitScale {
		t.Fatalf("expected risk-limit scale control to remain available")
	}
}

func TestPolicyControllerProducesNamedResult(t *testing.T) {
	result := runScenario(scenarioByName(t, "Policy-BurstAware-100-250ms"))
	if result.Name != "Policy-BurstAware-100-250ms" {
		t.Fatalf("expected policy-run result name, got %+v", result)
	}
	if result.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected policy baseline to record adaptive window stats, got %+v", result)
	}
}

func TestLearnedPolicyControllerProducesNamedResult(t *testing.T) {
	result := runScenario(scenarioByName(t, "Policy-LearnedLinUCB-100-250ms"))
	if result.Name != "Policy-LearnedLinUCB-100-250ms" {
		t.Fatalf("expected learned-policy result name, got %+v", result)
	}
	if result.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected learned policy baseline to record adaptive window stats, got %+v", result)
	}
}

func TestLearnedPolicyImprovesTailVersusBurstAware(t *testing.T) {
	burst := runScenario(scenarioByName(t, "Policy-BurstAware-100-250ms"))
	learned := runScenario(scenarioByName(t, "Policy-LearnedLinUCB-100-250ms"))
	if learned.P99LatencyMs >= burst.P99LatencyMs {
		t.Fatalf("expected learned controller to improve p99 tail, burst=%+v learned=%+v", burst, learned)
	}
}

func TestTinyMLPPolicyImprovesTailVersusBurstAware(t *testing.T) {
	burst := runScenario(scenarioByName(t, "Policy-BurstAware-100-250ms"))
	learned := runScenario(scenarioByName(t, "Policy-LearnedTinyMLP-100-250ms"))
	if learned.P99LatencyMs >= burst.P99LatencyMs {
		t.Fatalf("expected tiny MLP controller to improve p99 tail, burst=%+v learned=%+v", burst, learned)
	}
}

func TestTinyMLPPolicyImprovesFillsVersusBurstAware(t *testing.T) {
	burst := runScenario(scenarioByName(t, "Policy-BurstAware-100-250ms"))
	learned := runScenario(scenarioByName(t, "Policy-LearnedTinyMLP-100-250ms"))
	if learned.FillsPerSec <= burst.FillsPerSec {
		t.Fatalf("expected gradient-trained tiny MLP to improve fills, burst=%+v learned=%+v", burst, learned)
	}
}

func TestOfflineContextualPolicyProducesNamedResult(t *testing.T) {
	result := runScenario(scenarioByName(t, "Policy-LearnedOfflineContextual-100-250ms"))
	if result.Name != "Policy-LearnedOfflineContextual-100-250ms" {
		t.Fatalf("expected offline contextual policy result name, got %+v", result)
	}
	if result.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected offline contextual controller to record adaptive window stats, got %+v", result)
	}
	if result.RetailAdverseSelectionRate < 0 || result.RetailAdverseSelectionRate > 1 {
		t.Fatalf("expected retail adverse-selection rate to be bounded, got %+v", result)
	}
}

func TestFittedQPolicyProducesNamedResult(t *testing.T) {
	result := runScenario(scenarioByName(t, "Policy-LearnedFittedQ-100-250ms"))
	if result.Name != "Policy-LearnedFittedQ-100-250ms" {
		t.Fatalf("expected fitted-q policy result name, got %+v", result)
	}
	if result.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected fitted-q controller to record adaptive window stats, got %+v", result)
	}
	if result.NegativeBalanceViolations != 0 || result.ConservationBreaches != 0 {
		t.Fatalf("expected fitted-q controller to preserve invariants, got %+v", result)
	}
}

func TestOnlineDQNPolicyProducesNamedResult(t *testing.T) {
	result := runScenario(scenarioByName(t, "Policy-LearnedOnlineDQN-100-250ms"))
	if result.Name != "Policy-LearnedOnlineDQN-100-250ms" {
		t.Fatalf("expected online DQN policy result name, got %+v", result)
	}
	if result.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected online DQN controller to record adaptive window stats, got %+v", result)
	}
	if result.NegativeBalanceViolations != 0 || result.ConservationBreaches != 0 {
		t.Fatalf("expected online DQN controller to preserve invariants, got %+v", result)
	}
}

func TestGenerateSimulatorBenchmarkArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_BENCH") != "1" {
		t.Skip("set RUN_SIM_BENCH=1 to generate simulator benchmark artifacts")
	}

	results := make([]BenchmarkResult, 0, len(simulatorScenarios()))
	for _, cfg := range simulatorScenarios() {
		results = append(results, runScenario(cfg))
	}

	immediate := results[0]
	batch100 := results[2]
	if !(immediate.P50LatencyMs < batch100.P50LatencyMs) {
		t.Fatalf("expected immediate latency to be lower than FBA-100ms")
	}
	for _, result := range results {
		if result.NegativeBalanceViolations != 0 || result.ConservationBreaches != 0 {
			t.Fatalf("expected settlement-safe results, got %+v", result)
		}
	}

	if err := writeSimulatorArtifacts(results); err != nil {
		t.Fatalf("write artifacts: %v", err)
	}
}

func TestGenerateSimulatorMultiSeedArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_BENCH_MULTI") != "1" {
		t.Skip("set RUN_SIM_BENCH_MULTI=1 to generate simulator multi-seed benchmark artifacts")
	}

	seeds := []int64{7, 11, 19, 23, 29, 31, 37, 41}
	aggregates := make([]aggregateResult, 0, len(simulatorScenarios()))
	for _, base := range simulatorScenarios() {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		aggregates = append(aggregates, summarizeRuns(base, runs))
	}

	immediate := aggregates[0]
	speedBump := aggregates[1]
	adaptive := aggregates[5]
	orderFlowAdaptive := aggregates[6]
	queueAdaptive := aggregates[7]
	batch500 := aggregates[4]
	if !(immediate.MeanP50LatencyMs <= speedBump.MeanP50LatencyMs) {
		t.Fatalf("expected immediate p50 latency mean to be lower than speed-bump baseline")
	}
	if adaptive.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected adaptive aggregate to report non-zero adaptive mean window")
	}
	if orderFlowAdaptive.AdaptiveWindowMeanMs <= 0 || queueAdaptive.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected both adaptive comparison baselines to report non-zero adaptive mean window")
	}
	if !(immediate.MeanP50LatencyMs < batch500.MeanP50LatencyMs) {
		t.Fatalf("expected immediate p50 latency mean to be lower than FBA-500ms")
	}
	for _, agg := range aggregates {
		if agg.NegativeBalanceViolationsTotal != 0 || agg.ConservationBreachesTotal != 0 {
			t.Fatalf("expected aggregate settlement-safe results, got %+v", agg)
		}
	}

	if err := writeSimulatorMultiSeedArtifacts(aggregates, seeds); err != nil {
		t.Fatalf("write multi-seed artifacts: %v", err)
	}
	if err := writeSimulatorParetoArtifacts(summarizeParetoFrontier(aggregates), seeds); err != nil {
		t.Fatalf("write pareto artifacts: %v", err)
	}
}

func TestGenerateSimulatorAblationArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_ABLATION") != "1" {
		t.Skip("set RUN_SIM_ABLATION=1 to generate simulator ablation artifacts")
	}

	seeds := []int64{13, 17, 19, 23}
	aggregates := make([]aggregateResult, 0, len(ablationScenarios()))
	for _, base := range ablationScenarios() {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		aggregates = append(aggregates, summarizeRuns(base, runs))
	}

	control := aggregates[0]
	relaxedRisk := aggregates[1]
	if relaxedRisk.RiskRejectionsTotal > control.RiskRejectionsTotal {
		t.Fatalf("expected relaxed risk to reduce rejections, control=%+v relaxed=%+v", control, relaxedRisk)
	}
	if err := writeSimulatorAblationArtifacts(aggregates, seeds); err != nil {
		t.Fatalf("write ablation artifacts: %v", err)
	}
}

func TestGenerateSimulatorAgentAblationArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_AGENT_ABLATION") != "1" {
		t.Skip("set RUN_SIM_AGENT_ABLATION=1 to generate simulator agent/workload ablation artifacts")
	}

	seeds := []int64{43, 47, 53, 59}
	aggregates := make([]aggregateResult, 0, len(agentWorkloadAblationScenarios()))
	for _, base := range agentWorkloadAblationScenarios() {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		aggregates = append(aggregates, summarizeRuns(base, runs))
	}

	control := aggregates[0]
	noArb := aggregates[1]
	retailBurst := aggregates[3]
	arbHigh := aggregates[4]
	if noArb.MeanLatencyArbitrageProfit >= control.MeanLatencyArbitrageProfit {
		t.Fatalf("expected no-arbitrageur ablation to reduce arb profit, control=%+v noArb=%+v", control, noArb)
	}
	if retailBurst.MeanOrdersPerSec <= control.MeanOrdersPerSec {
		t.Fatalf("expected retail burst sweep to raise throughput, control=%+v retail=%+v", control, retailBurst)
	}
	if arbHigh.MeanLatencyArbitrageProfit <= control.MeanLatencyArbitrageProfit {
		t.Fatalf("expected arbitrage-intensity sweep to raise arb profit, control=%+v arbHigh=%+v", control, arbHigh)
	}
	if err := writeSimulatorAgentAblationArtifacts(aggregates, seeds); err != nil {
		t.Fatalf("write agent/workload ablation artifacts: %v", err)
	}
}

func TestGenerateSimulatorParameterGridArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_GRID") != "1" {
		t.Skip("set RUN_SIM_GRID=1 to generate simulator parameter-grid artifacts")
	}

	seeds := []int64{61, 67, 71, 73}
	gridResults := make([]gridSweepResult, 0, len(parameterGridScenarios()))
	for _, base := range parameterGridScenarios() {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		gridResults = append(gridResults, summarizeGridRun(base, runs))
	}

	control := findGridResult(t, gridResults, 1, 1)
	noArb := findGridResult(t, gridResults, 0, 1)
	highArbWide := findGridResult(t, gridResults, 3, 3)
	if noArb.MeanLatencyArbitrageProfit >= control.MeanLatencyArbitrageProfit {
		t.Fatalf("expected zero-arbitrageur grid cell to reduce arb profit, control=%+v noArb=%+v", control, noArb)
	}
	if highArbWide.MeanP99LatencyMs <= control.MeanP99LatencyMs {
		t.Fatalf("expected high-arb/wide-maker cell to worsen p99, control=%+v high=%+v", control, highArbWide)
	}
	if err := writeSimulatorGridArtifacts(gridResults, seeds); err != nil {
		t.Fatalf("write grid artifacts: %v", err)
	}
}

func TestGenerateSimulatorParameterCubeArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_CUBE") != "1" {
		t.Skip("set RUN_SIM_CUBE=1 to generate simulator parameter-cube artifacts")
	}

	seeds := []int64{79, 83, 89, 97}
	cubeResults := make([]cubeSweepResult, 0, len(parameterCubeScenarios()))
	for _, base := range parameterCubeScenarios() {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		cubeResults = append(cubeResults, summarizeCubeRun(base, runs))
	}

	control := findCubeResult(t, cubeResults, 1, 1, 1)
	retailHigh := findCubeResult(t, cubeResults, 3, 1, 1)
	makerWide := findCubeResult(t, cubeResults, 1, 1, 3)
	if retailHigh.MeanOrdersPerSec <= control.MeanOrdersPerSec {
		t.Fatalf("expected high-retail cube cell to raise throughput, control=%+v retailHigh=%+v", control, retailHigh)
	}
	if makerWide.MeanFillsPerSec >= control.MeanFillsPerSec {
		t.Fatalf("expected wide-maker cube cell to reduce fills, control=%+v makerWide=%+v", control, makerWide)
	}
	if err := writeSimulatorCubeArtifacts(cubeResults, seeds); err != nil {
		t.Fatalf("write cube artifacts: %v", err)
	}
}

func TestGenerateSimulatorParameterHypercubeArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_HYPER") != "1" {
		t.Skip("set RUN_SIM_HYPER=1 to generate simulator parameter-hypercube artifacts")
	}

	seeds := []int64{101, 103, 107, 109}
	hyperResults := make([]hypercubeSweepResult, 0, len(parameterHypercubeScenarios()))
	for _, base := range parameterHypercubeScenarios() {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		hyperResults = append(hyperResults, summarizeHypercubeRun(base, runs))
	}

	control := findHypercubeResult(t, hyperResults, 1, 1, 1, 1)
	noArb := findHypercubeResult(t, hyperResults, 0, 1, 1, 1)
	retailHigh := findHypercubeResult(t, hyperResults, 1, 3, 1, 1)
	if noArb.MeanLatencyArbitrageProfit >= control.MeanLatencyArbitrageProfit {
		t.Fatalf("expected hypercube no-arbitrage cell to reduce arbitrage profit, control=%+v noArb=%+v", control, noArb)
	}
	if retailHigh.MeanOrdersPerSec <= control.MeanOrdersPerSec {
		t.Fatalf("expected hypercube high-retail cell to raise throughput, control=%+v retailHigh=%+v", control, retailHigh)
	}

	if err := writeSimulatorHypercubeArtifacts(hyperResults, seeds); err != nil {
		t.Fatalf("write hypercube artifacts: %v", err)
	}
	if err := writeSimulatorHypercubeSummaryArtifacts(summarizeHypercubeCompact(hyperResults, seeds)); err != nil {
		t.Fatalf("write hypercube summary artifacts: %v", err)
	}
	if err := writeSimulatorHypercubeResponseSurfaceArtifacts(summarizeHypercubeResponseSurface(hyperResults, seeds)); err != nil {
		t.Fatalf("write hypercube response-surface artifacts: %v", err)
	}
}

func TestGenerateSimulatorHeldOutPolicyArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_HELDOUT") != "1" {
		t.Skip("set RUN_SIM_HELDOUT=1 to generate simulator held-out policy artifacts")
	}

	seeds := []int64{223, 227, 229, 233}
	results := make([]heldOutPolicyResult, 0, len(heldOutRegimeScenarios())*len(heldOutPolicies()))
	for _, regime := range heldOutRegimeScenarios() {
		for _, policy := range heldOutPolicies() {
			runs := make([]BenchmarkResult, 0, len(seeds))
			for _, seed := range seeds {
				cfg := regime
				cfg.Seed = seed
				cfg.PolicyController = policy
				runs = append(runs, runScenario(cfg))
			}
			results = append(results, summarizeHeldOutPolicyRuns(regime.Name, policy, runs))
		}
	}

	linucb := findHeldOutPolicyResult(t, results, "HeldOut-HighArbWideMaker", PolicyLearnedLinUCB)
	fittedQ := findHeldOutPolicyResult(t, results, "HeldOut-HighArbWideMaker", PolicyLearnedFittedQ)
	if fittedQ.MeanSurplusTransferGap > linucb.MeanSurplusTransferGap && fittedQ.MeanP99LatencyMs >= linucb.MeanP99LatencyMs {
		t.Fatalf("expected fitted-q policy to improve at least one held-out tradeoff axis, linucb=%+v fittedQ=%+v", linucb, fittedQ)
	}

	if err := writeSimulatorHeldOutPolicyArtifacts(results, seeds); err != nil {
		t.Fatalf("write held-out policy artifacts: %v", err)
	}
}

func TestGenerateSimulatorFittedQLearningCurveArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_FITTEDQ_CURVE") != "1" {
		t.Skip("set RUN_SIM_FITTEDQ_CURVE=1 to generate fitted-q learning-curve artifacts")
	}

	base := scenarioByName(t, "Policy-LearnedFittedQ-100-250ms")
	trace := trainLearnedFittedQPolicyTrace(base)
	regimes := heldOutRegimeScenarios()
	points := make([]fittedQLearningCurvePoint, 0, len(trace))
	for _, snapshot := range trace {
		runs := make([]BenchmarkResult, 0, len(regimes)*len(snapshot.Policy.HeldOutSeeds))
		for _, regime := range regimes {
			for _, seed := range snapshot.Policy.HeldOutSeeds {
				cfg := regime
				cfg.Seed = seed
				runs = append(runs, runScenarioWithFittedQPolicy(cfg, snapshot.Policy))
			}
		}
		points = append(points, summarizeFittedQLearningCurvePoint(snapshot, runs))
	}

	if len(points) < 2 {
		t.Fatalf("expected multiple fitted-q learning points, got %+v", points)
	}
	if points[len(points)-1].MeanSurplusTransferGap >= points[0].MeanSurplusTransferGap {
		t.Fatalf("expected fitted-q training to improve held-out welfare gap over the untrained baseline, start=%+v end=%+v", points[0], points[len(points)-1])
	}
	if err := writeSimulatorFittedQLearningCurveArtifacts(points, trace[len(trace)-1].Policy.TrainingSeeds, trace[len(trace)-1].Policy.HeldOutSeeds, trace[len(trace)-1].Policy.HeldOutRegimes); err != nil {
		t.Fatalf("write fitted-q learning-curve artifacts: %v", err)
	}
}

func TestGenerateSimulatorOnlineDQNTrainingArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_ONLINE_DQN") != "1" {
		t.Skip("set RUN_SIM_ONLINE_DQN=1 to generate online-DQN training artifacts")
	}

	base := scenarioByName(t, "Policy-LearnedOnlineDQN-100-250ms")
	trace := trainLearnedOnlineDQNPolicyTrace(base)
	regimes := heldOutRegimeScenarios()
	points := make([]onlineDQNLearningCurvePoint, 0, len(trace))
	for _, snapshot := range trace {
		runs := make([]BenchmarkResult, 0, len(regimes)*len(snapshot.Policy.HeldOutSeeds))
		for _, regime := range regimes {
			for _, seed := range snapshot.Policy.HeldOutSeeds {
				cfg := regime
				cfg.Seed = seed
				runs = append(runs, runScenarioWithOnlineDQNPolicy(cfg, snapshot.Policy))
			}
		}
		points = append(points, summarizeOnlineDQNLearningCurvePoint(snapshot, runs))
	}

	if len(points) < 2 {
		t.Fatalf("expected multiple online-DQN learning points, got %+v", points)
	}
	if points[len(points)-1].MeanP99LatencyMs >= points[0].MeanP99LatencyMs {
		t.Fatalf("expected online DQN training to improve held-out p99 over the untrained baseline, start=%+v end=%+v", points[0], points[len(points)-1])
	}
	if err := writeSimulatorOnlineDQNLearningCurveArtifacts(points, trace[len(trace)-1].Policy.TrainingSeeds, trace[len(trace)-1].Policy.HeldOutSeeds, trace[len(trace)-1].Policy.HeldOutRegimes); err != nil {
		t.Fatalf("write online-DQN learning-curve artifacts: %v", err)
	}
}

func TestGenerateSimulatorOnlineDQNRewardSensitivityArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_REWARD_SENSITIVITY") != "1" {
		t.Skip("set RUN_SIM_REWARD_SENSITIVITY=1 to generate online-DQN reward-sensitivity artifacts")
	}

	base := scenarioByName(t, "Policy-LearnedOnlineDQN-100-250ms")
	profiles := onlineDQNRewardProfiles()
	regimes := heldOutRegimeScenarios()
	results := make([]onlineDQNRewardSensitivityResult, 0, len(profiles))
	var trainingSeeds []int64
	var heldOutSeeds []int64
	var heldOutRegimes []string
	for _, profile := range profiles {
		trace := trainLearnedOnlineDQNPolicyTraceWithRewardWeights(base, profile.RewardWeights)
		final := trace[len(trace)-1]
		trainingSeeds = append([]int64(nil), final.Policy.TrainingSeeds...)
		heldOutSeeds = append([]int64(nil), final.Policy.HeldOutSeeds...)
		heldOutRegimes = append([]string(nil), final.Policy.HeldOutRegimes...)
		runs := make([]BenchmarkResult, 0, len(regimes)*len(final.Policy.HeldOutSeeds))
		for _, regime := range regimes {
			for _, seed := range final.Policy.HeldOutSeeds {
				cfg := regime
				cfg.Seed = seed
				runs = append(runs, runScenarioWithOnlineDQNPolicyAndRewardWeights(cfg, final.Policy, profile.RewardWeights))
			}
		}
		results = append(results, summarizeOnlineDQNRewardSensitivityResult(profile, final.MeanEpisodeReward, runs))
	}

	if len(results) < 3 {
		t.Fatalf("expected multiple reward-sensitivity profiles, got %+v", results)
	}
	if err := writeSimulatorOnlineDQNRewardSensitivityArtifacts(results, profiles, trainingSeeds, heldOutSeeds, heldOutRegimes); err != nil {
		t.Fatalf("write online-DQN reward-sensitivity artifacts: %v", err)
	}
}

func TestGenerateSimulatorDoubleDQNTrainingArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_DOUBLE_DQN") != "1" {
		t.Skip("set RUN_SIM_DOUBLE_DQN=1 to generate double-DQN training artifacts")
	}

	base := scenarioByName(t, "Policy-LearnedOnlineDQN-100-250ms")
	trace := trainLearnedDoubleDQNPolicyTrace(base)
	regimes := heldOutRegimeScenarios()
	points := make([]doubleDQNLearningCurvePoint, 0, len(trace))
	for _, snapshot := range trace {
		runs := make([]BenchmarkResult, 0, len(regimes)*len(snapshot.Policy.HeldOutSeeds))
		for _, regime := range regimes {
			for _, seed := range snapshot.Policy.HeldOutSeeds {
				cfg := regime
				cfg.Seed = seed
				runs = append(runs, runScenarioWithDoubleDQNPolicy(cfg, snapshot.Policy))
			}
		}
		points = append(points, summarizeDoubleDQNLearningCurvePoint(snapshot, runs))
	}

	if len(points) < 2 {
		t.Fatalf("expected multiple double-DQN learning points, got %+v", points)
	}
	if err := writeSimulatorDoubleDQNLearningCurveArtifacts(points, trace[len(trace)-1].Policy.TrainingSeeds, trace[len(trace)-1].Policy.HeldOutSeeds, trace[len(trace)-1].Policy.HeldOutRegimes); err != nil {
		t.Fatalf("write double-DQN learning artifacts: %v", err)
	}
}

func TestGenerateSimulatorStrategicAgentArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_STRATEGIC_AGENTS") != "1" {
		t.Skip("set RUN_SIM_STRATEGIC_AGENTS=1 to generate strategic-agent artifacts")
	}

	results := make([]strategicAgentResult, 0, len(strategicAgentScenarios()))
	seeds := []int64{521, 523, 541, 547}
	for _, base := range strategicAgentScenarios() {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		results = append(results, summarizeStrategicAgentRuns(base.Name, runs))
	}
	if len(results) < 2 {
		t.Fatalf("expected strategic-agent scenarios, got %+v", results)
	}
	if err := writeSimulatorStrategicAgentArtifacts(results, seeds); err != nil {
		t.Fatalf("write strategic-agent artifacts: %v", err)
	}
}

func runScenarioWithFittedQPolicy(cfg ScenarioConfig, policy learnedFittedQPolicy) BenchmarkResult {
	start := time.Now()
	adapter := NewAdapter(cfg)
	timestep := adapter.Reset()
	for !timestep.Done {
		action := chooseFittedQAction(adapter.ActionSpec(), timestep.Observation, policy)
		timestep = adapter.Step(action)
	}
	result := adapter.env.benchmarkResult(time.Since(start))
	result.Name = policyScenarioName(result.Name, PolicyLearnedFittedQ)
	return result
}

func runScenarioWithOnlineDQNPolicy(cfg ScenarioConfig, policy learnedOnlineDQNPolicy) BenchmarkResult {
	return runScenarioWithOnlineDQNPolicyAndRewardWeights(cfg, policy, defaultRewardWeights())
}

func runScenarioWithDoubleDQNPolicy(cfg ScenarioConfig, policy learnedDoubleDQNPolicy) BenchmarkResult {
	start := time.Now()
	adapter := NewAdapter(cfg)
	timestep := adapter.Reset()
	for !timestep.Done {
		action := chooseOnlineDQNAction(adapter.ActionSpec(), timestep.Observation, learnedOnlineDQNPolicy{Model: policy.Model})
		timestep = adapter.Step(action)
	}
	result := adapter.env.benchmarkResult(time.Since(start))
	result.Name = "Policy-LearnedDoubleDQN-100-250ms"
	return result
}

func runScenarioWithOnlineDQNPolicyAndRewardWeights(cfg ScenarioConfig, policy learnedOnlineDQNPolicy, rewardWeights RewardWeights) BenchmarkResult {
	start := time.Now()
	adapter := NewAdapterWithRewardWeights(cfg, rewardWeights)
	timestep := adapter.Reset()
	for !timestep.Done {
		action := chooseOnlineDQNAction(adapter.ActionSpec(), timestep.Observation, policy)
		timestep = adapter.Step(action)
	}
	result := adapter.env.benchmarkResult(time.Since(start))
	result.Name = policyScenarioName(result.Name, PolicyLearnedOnlineDQN)
	return result
}

func summarizeFittedQLearningCurvePoint(snapshot fittedQTrainingSnapshot, runs []BenchmarkResult) fittedQLearningCurvePoint {
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	surplusGap := make([]float64, 0, len(runs))
	for _, run := range runs {
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
		surplusGap = append(surplusGap, run.SurplusTransferGap)
	}
	meanFills, ciFills := meanCI95(fills)
	meanP99, ciP99 := meanCI95(p99)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanGap, ciGap := meanCI95(surplusGap)
	return fittedQLearningCurvePoint{
		Iteration:                      snapshot.Iteration,
		MeanBellmanMSE:                 snapshot.BellmanMSE,
		Runs:                           len(runs),
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
	}
}

func writeSimulatorFittedQLearningCurveArtifacts(points []fittedQLearningCurvePoint, trainingSeeds []int64, heldOutSeeds []int64, heldOutRegimes []string) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_fittedq_learning_curve.json")
	mdPath := filepath.Join(base, "simulator_fittedq_learning_curve.md")
	csvPath := filepath.Join(base, "simulator_fittedq_learning_curve.csv")

	payload := map[string]any{
		"training_seeds":  trainingSeeds,
		"heldout_seeds":   heldOutSeeds,
		"heldout_regimes": heldOutRegimes,
		"results":         points,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Fitted-Q Learning Curve\n\n")
	md.WriteString(fmt.Sprintf("Training seeds: `%v`\n\n", trainingSeeds))
	md.WriteString(fmt.Sprintf("Held-out seeds: `%v`\n\n", heldOutSeeds))
	md.WriteString(fmt.Sprintf("Held-out regimes: `%s`\n\n", strings.Join(heldOutRegimes, ", ")))
	md.WriteString("Each row evaluates the fitted-Q snapshot after a given Bellman-update iteration on the held-out regime set.\n\n")
	md.WriteString("| Iteration | Bellman MSE | Runs | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, point := range points {
		md.WriteString(fmt.Sprintf("| %d | %.4f | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			point.Iteration, point.MeanBellmanMSE, point.Runs,
			point.MeanFillsPerSec, point.CI95FillsPerSec,
			point.MeanP99LatencyMs, point.CI95P99LatencyMs,
			point.MeanRetailSurplusPerUnit, point.CI95RetailSurplusPerUnit,
			point.MeanRetailAdverseSelectionRate, point.CI95RetailAdverseSelectionRate,
			point.MeanSurplusTransferGap, point.CI95SurplusTransferGap))
	}

	var csv strings.Builder
	csv.WriteString("iteration,mean_bellman_mse,runs,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap\n")
	for _, point := range points {
		csv.WriteString(fmt.Sprintf("%d,%.6f,%d,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			point.Iteration, point.MeanBellmanMSE, point.Runs,
			point.MeanFillsPerSec, point.CI95FillsPerSec,
			point.MeanP99LatencyMs, point.CI95P99LatencyMs,
			point.MeanRetailSurplusPerUnit, point.CI95RetailSurplusPerUnit,
			point.MeanRetailAdverseSelectionRate, point.CI95RetailAdverseSelectionRate,
			point.MeanSurplusTransferGap, point.CI95SurplusTransferGap))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func summarizeOnlineDQNLearningCurvePoint(snapshot onlineDQNTrainingSnapshot, runs []BenchmarkResult) onlineDQNLearningCurvePoint {
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	surplusGap := make([]float64, 0, len(runs))
	for _, run := range runs {
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
		surplusGap = append(surplusGap, run.SurplusTransferGap)
	}
	meanFills, ciFills := meanCI95(fills)
	meanP99, ciP99 := meanCI95(p99)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanGap, ciGap := meanCI95(surplusGap)
	return onlineDQNLearningCurvePoint{
		Episode:                        snapshot.Episode,
		MeanEpisodeReward:              snapshot.MeanEpisodeReward,
		Runs:                           len(runs),
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
	}
}

func writeSimulatorOnlineDQNLearningCurveArtifacts(points []onlineDQNLearningCurvePoint, trainingSeeds []int64, heldOutSeeds []int64, heldOutRegimes []string) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_online_dqn_training_curve.json")
	mdPath := filepath.Join(base, "simulator_online_dqn_training_curve.md")
	csvPath := filepath.Join(base, "simulator_online_dqn_training_curve.csv")

	payload := map[string]any{
		"training_seeds":  trainingSeeds,
		"heldout_seeds":   heldOutSeeds,
		"heldout_regimes": heldOutRegimes,
		"results":         points,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Online DQN Training Curve\n\n")
	md.WriteString(fmt.Sprintf("Training seeds: `%v`\n\n", trainingSeeds))
	md.WriteString(fmt.Sprintf("Held-out seeds: `%v`\n\n", heldOutSeeds))
	md.WriteString(fmt.Sprintf("Held-out regimes: `%s`\n\n", strings.Join(heldOutRegimes, ", ")))
	md.WriteString("Each row evaluates an online DQN-style checkpoint on the held-out regime set.\n\n")
	md.WriteString("| Episode | Mean Train Reward | Runs | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, point := range points {
		md.WriteString(fmt.Sprintf("| %d | %.4f | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			point.Episode, point.MeanEpisodeReward, point.Runs,
			point.MeanFillsPerSec, point.CI95FillsPerSec,
			point.MeanP99LatencyMs, point.CI95P99LatencyMs,
			point.MeanRetailSurplusPerUnit, point.CI95RetailSurplusPerUnit,
			point.MeanRetailAdverseSelectionRate, point.CI95RetailAdverseSelectionRate,
			point.MeanSurplusTransferGap, point.CI95SurplusTransferGap))
	}

	var csv strings.Builder
	csv.WriteString("episode,mean_episode_reward,runs,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap\n")
	for _, point := range points {
		csv.WriteString(fmt.Sprintf("%d,%.6f,%d,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			point.Episode, point.MeanEpisodeReward, point.Runs,
			point.MeanFillsPerSec, point.CI95FillsPerSec,
			point.MeanP99LatencyMs, point.CI95P99LatencyMs,
			point.MeanRetailSurplusPerUnit, point.CI95RetailSurplusPerUnit,
			point.MeanRetailAdverseSelectionRate, point.CI95RetailAdverseSelectionRate,
			point.MeanSurplusTransferGap, point.CI95SurplusTransferGap))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func summarizeOnlineDQNRewardSensitivityResult(profile rewardSensitivityProfile, meanTrainReward float64, runs []BenchmarkResult) onlineDQNRewardSensitivityResult {
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	surplusGap := make([]float64, 0, len(runs))
	for _, run := range runs {
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
		surplusGap = append(surplusGap, run.SurplusTransferGap)
	}
	meanFills, ciFills := meanCI95(fills)
	meanP99, ciP99 := meanCI95(p99)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanGap, ciGap := meanCI95(surplusGap)
	return onlineDQNRewardSensitivityResult{
		ProfileName:                    profile.Name,
		RewardWeights:                  profile.RewardWeights,
		MeanTrainEpisodeReward:         meanTrainReward,
		Runs:                           len(runs),
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
	}
}

func writeSimulatorOnlineDQNRewardSensitivityArtifacts(results []onlineDQNRewardSensitivityResult, profiles []rewardSensitivityProfile, trainingSeeds []int64, heldOutSeeds []int64, heldOutRegimes []string) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_online_dqn_reward_sensitivity.json")
	mdPath := filepath.Join(base, "simulator_online_dqn_reward_sensitivity.md")
	csvPath := filepath.Join(base, "simulator_online_dqn_reward_sensitivity.csv")

	payload := map[string]any{
		"profiles":        profiles,
		"training_seeds":  trainingSeeds,
		"heldout_seeds":   heldOutSeeds,
		"heldout_regimes": heldOutRegimes,
		"results":         results,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Online DQN Reward Sensitivity\n\n")
	md.WriteString(fmt.Sprintf("Training seeds: `%v`\n\n", trainingSeeds))
	md.WriteString(fmt.Sprintf("Held-out seeds: `%v`\n\n", heldOutSeeds))
	md.WriteString(fmt.Sprintf("Held-out regimes: `%s`\n\n", strings.Join(heldOutRegimes, ", ")))
	md.WriteString("Each row retrains the online DQN-style controller under a different reward-weight profile and evaluates the final checkpoint on the held-out regime set.\n\n")
	md.WriteString("| Profile | Mean Train Reward | Runs | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, result := range results {
		md.WriteString(fmt.Sprintf("| %s | %.4f | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			result.ProfileName, result.MeanTrainEpisodeReward, result.Runs,
			result.MeanFillsPerSec, result.CI95FillsPerSec,
			result.MeanP99LatencyMs, result.CI95P99LatencyMs,
			result.MeanRetailSurplusPerUnit, result.CI95RetailSurplusPerUnit,
			result.MeanRetailAdverseSelectionRate, result.CI95RetailAdverseSelectionRate,
			result.MeanSurplusTransferGap, result.CI95SurplusTransferGap))
	}

	var csv strings.Builder
	csv.WriteString("profile_name,mean_train_episode_reward,runs,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap,fill_weight,spread_penalty,price_impact_penalty,queue_penalty,arbitrage_penalty,retail_surplus_weight,adverse_penalty,welfare_penalty,surplus_gap_penalty,risk_reject_penalty,conservation_penalty\n")
	for _, result := range results {
		w := result.RewardWeights
		csv.WriteString(fmt.Sprintf("%s,%.6f,%d,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			result.ProfileName, result.MeanTrainEpisodeReward, result.Runs,
			result.MeanFillsPerSec, result.CI95FillsPerSec,
			result.MeanP99LatencyMs, result.CI95P99LatencyMs,
			result.MeanRetailSurplusPerUnit, result.CI95RetailSurplusPerUnit,
			result.MeanRetailAdverseSelectionRate, result.CI95RetailAdverseSelectionRate,
			result.MeanSurplusTransferGap, result.CI95SurplusTransferGap,
			w.FillWeight, w.SpreadPenalty, w.PriceImpactPenalty, w.QueuePenalty, w.ArbitragePenalty,
			w.RetailSurplusWeight, w.AdversePenalty, w.WelfarePenalty, w.SurplusGapPenalty, w.RiskRejectPenalty, w.ConservationPenalty))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func summarizeDoubleDQNLearningCurvePoint(snapshot doubleDQNTrainingSnapshot, runs []BenchmarkResult) doubleDQNLearningCurvePoint {
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	surplusGap := make([]float64, 0, len(runs))
	for _, run := range runs {
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
		surplusGap = append(surplusGap, run.SurplusTransferGap)
	}
	meanFills, ciFills := meanCI95(fills)
	meanP99, ciP99 := meanCI95(p99)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanGap, ciGap := meanCI95(surplusGap)
	return doubleDQNLearningCurvePoint{
		Episode:                        snapshot.Episode,
		MeanEpisodeReward:              snapshot.MeanEpisodeReward,
		Runs:                           len(runs),
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
	}
}

func writeSimulatorDoubleDQNLearningCurveArtifacts(points []doubleDQNLearningCurvePoint, trainingSeeds []int64, heldOutSeeds []int64, heldOutRegimes []string) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_double_dqn_training_curve.json")
	mdPath := filepath.Join(base, "simulator_double_dqn_training_curve.md")
	csvPath := filepath.Join(base, "simulator_double_dqn_training_curve.csv")
	payload := map[string]any{
		"training_seeds":  trainingSeeds,
		"heldout_seeds":   heldOutSeeds,
		"heldout_regimes": heldOutRegimes,
		"results":         points,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Double DQN Training Curve\n\n")
	md.WriteString(fmt.Sprintf("Training seeds: `%v`\n\n", trainingSeeds))
	md.WriteString(fmt.Sprintf("Held-out seeds: `%v`\n\n", heldOutSeeds))
	md.WriteString(fmt.Sprintf("Held-out regimes: `%s`\n\n", strings.Join(heldOutRegimes, ", ")))
	md.WriteString("Each row evaluates a prioritized Double-DQN style checkpoint on the held-out regime set.\n\n")
	md.WriteString("| Episode | Mean Train Reward | Runs | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, point := range points {
		md.WriteString(fmt.Sprintf("| %d | %.4f | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			point.Episode, point.MeanEpisodeReward, point.Runs,
			point.MeanFillsPerSec, point.CI95FillsPerSec,
			point.MeanP99LatencyMs, point.CI95P99LatencyMs,
			point.MeanRetailSurplusPerUnit, point.CI95RetailSurplusPerUnit,
			point.MeanRetailAdverseSelectionRate, point.CI95RetailAdverseSelectionRate,
			point.MeanSurplusTransferGap, point.CI95SurplusTransferGap))
	}
	var csv strings.Builder
	csv.WriteString("episode,mean_episode_reward,runs,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap\n")
	for _, point := range points {
		csv.WriteString(fmt.Sprintf("%d,%.6f,%d,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			point.Episode, point.MeanEpisodeReward, point.Runs,
			point.MeanFillsPerSec, point.CI95FillsPerSec,
			point.MeanP99LatencyMs, point.CI95P99LatencyMs,
			point.MeanRetailSurplusPerUnit, point.CI95RetailSurplusPerUnit,
			point.MeanRetailAdverseSelectionRate, point.CI95RetailAdverseSelectionRate,
			point.MeanSurplusTransferGap, point.CI95SurplusTransferGap))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func summarizeStrategicAgentRuns(name string, runs []BenchmarkResult) strategicAgentResult {
	orders := make([]float64, 0, len(runs))
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	impact := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	surplusGap := make([]float64, 0, len(runs))
	for _, run := range runs {
		orders = append(orders, run.OrdersPerSec)
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		impact = append(impact, run.AveragePriceImpact)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
		surplusGap = append(surplusGap, run.SurplusTransferGap)
	}
	meanOrders, ciOrders := meanCI95(orders)
	meanFills, ciFills := meanCI95(fills)
	meanP99, ciP99 := meanCI95(p99)
	meanImpact, ciImpact := meanCI95(impact)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanGap, ciGap := meanCI95(surplusGap)
	return strategicAgentResult{
		Name:                           name,
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
	}
}

func writeSimulatorStrategicAgentArtifacts(results []strategicAgentResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_strategic_agent_profile.json")
	mdPath := filepath.Join(base, "simulator_strategic_agent_profile.md")
	csvPath := filepath.Join(base, "simulator_strategic_agent_profile.csv")
	payload := map[string]any{"seeds": seeds, "results": results}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Strategic Agent Profile\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("These scenarios use inventory-aware market makers, signal-scaled informed traders, trend-reactive retail flow, and dislocation-sensitive arbitrageurs.\n\n")
	md.WriteString("| Scenario | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, result := range results {
		md.WriteString(fmt.Sprintf("| %s | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			result.Name, result.Runs, result.MeanOrdersPerSec, result.CI95OrdersPerSec, result.MeanFillsPerSec, result.CI95FillsPerSec,
			result.MeanP99LatencyMs, result.CI95P99LatencyMs, result.MeanAveragePriceImpact, result.CI95AveragePriceImpact,
			result.MeanRetailSurplusPerUnit, result.CI95RetailSurplusPerUnit, result.MeanRetailAdverseSelectionRate, result.CI95RetailAdverseSelectionRate,
			result.MeanSurplusTransferGap, result.CI95SurplusTransferGap))
	}
	var csv strings.Builder
	csv.WriteString("name,runs,mean_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_average_price_impact,ci95_average_price_impact,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap\n")
	for _, result := range results {
		csv.WriteString(fmt.Sprintf("%s,%d,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			result.Name, result.Runs, result.MeanOrdersPerSec, result.CI95OrdersPerSec, result.MeanFillsPerSec, result.CI95FillsPerSec,
			result.MeanP99LatencyMs, result.CI95P99LatencyMs, result.MeanAveragePriceImpact, result.CI95AveragePriceImpact,
			result.MeanRetailSurplusPerUnit, result.CI95RetailSurplusPerUnit, result.MeanRetailAdverseSelectionRate, result.CI95RetailAdverseSelectionRate,
			result.MeanSurplusTransferGap, result.CI95SurplusTransferGap))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorArtifacts(results []BenchmarkResult) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_benchmark_profile.json")
	mdPath := filepath.Join(base, "simulator_benchmark_profile.md")
	csvPath := filepath.Join(base, "simulator_benchmark_profile.csv")

	payload := map[string]any{"results": results}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Benchmark Profile\n\n")
	md.WriteString("| Scenario | Mode | Window (ms) | Speed Bump (ms) | Orders/s | Fills/s | p50 (ms) | p95 (ms) | Spread | Price Impact | Queue Advantage | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap | Risk Rejects |\n")
	md.WriteString("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("scenario,mode,batch_window_ms,speed_bump_ms,orders_per_sec,fills_per_sec,p50_latency_ms,p95_latency_ms,p99_latency_ms,average_spread,average_price_impact,queue_priority_advantage,latency_arbitrage_profit,execution_dispersion,retail_surplus_per_unit,arbitrageur_surplus_per_unit,retail_adverse_selection_rate,welfare_dispersion,surplus_transfer_gap,risk_rejections,negative_balance_violations,conservation_breaches\n")

	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %d | %.2f | %.2f | %.2f | %.2f | %.2f | %.2f | %.4f | %.2f | %.4f | %.4f | %.4f | %d |\n",
			r.Name, r.Mode, r.BatchWindowMs, r.SpeedBumpMs, r.OrdersPerSec, r.FillsPerSec, r.P50LatencyMs, r.P95LatencyMs,
			r.AverageSpread, r.AveragePriceImpact, r.QueuePriorityAdvantage, r.LatencyArbitrageProfit, r.RetailSurplusPerUnit, r.RetailAdverseSelectionRate, r.SurplusTransferGap, r.RiskRejections))
		csv.WriteString(fmt.Sprintf("%s,%s,%d,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.4f,%.6f,%.4f,%.4f,%.6f,%.6f,%.6f,%d,%d,%d\n",
			r.Name, r.Mode, r.BatchWindowMs, r.SpeedBumpMs, r.OrdersPerSec, r.FillsPerSec, r.P50LatencyMs, r.P95LatencyMs,
			r.P99LatencyMs, r.AverageSpread, r.AveragePriceImpact, r.QueuePriorityAdvantage, r.LatencyArbitrageProfit,
			r.ExecutionDispersion, r.RetailSurplusPerUnit, r.ArbitrageurSurplusPerUnit, r.RetailAdverseSelectionRate, r.WelfareDispersion, r.SurplusTransferGap, r.RiskRejections, r.NegativeBalanceViolations, r.ConservationBreaches))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func summarizeRuns(base ScenarioConfig, runs []BenchmarkResult) aggregateResult {
	agg := aggregateResult{
		Name:                base.Name,
		Mode:                base.Mode,
		BatchWindowMs:       int(base.StepDuration.Milliseconds()) * base.BatchWindowSteps,
		SpeedBumpMs:         int(base.StepDuration.Milliseconds()) * base.SpeedBumpSteps,
		AdaptiveWindowMinMs: int(base.StepDuration.Milliseconds()) * base.AdaptiveMinWindowSteps,
		AdaptiveWindowMaxMs: int(base.StepDuration.Milliseconds()) * base.AdaptiveMaxWindowSteps,
		Runs:                len(runs),
	}
	orders := make([]float64, 0, len(runs))
	fills := make([]float64, 0, len(runs))
	p50 := make([]float64, 0, len(runs))
	p95 := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	spread := make([]float64, 0, len(runs))
	impact := make([]float64, 0, len(runs))
	queue := make([]float64, 0, len(runs))
	arb := make([]float64, 0, len(runs))
	dispersion := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	arbSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	welfare := make([]float64, 0, len(runs))
	surplusGap := make([]float64, 0, len(runs))
	for _, run := range runs {
		orders = append(orders, run.OrdersPerSec)
		fills = append(fills, run.FillsPerSec)
		p50 = append(p50, run.P50LatencyMs)
		p95 = append(p95, run.P95LatencyMs)
		p99 = append(p99, run.P99LatencyMs)
		spread = append(spread, run.AverageSpread)
		impact = append(impact, run.AveragePriceImpact)
		queue = append(queue, run.QueuePriorityAdvantage)
		arb = append(arb, run.LatencyArbitrageProfit)
		dispersion = append(dispersion, run.ExecutionDispersion)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		arbSurplus = append(arbSurplus, run.ArbitrageurSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
		welfare = append(welfare, run.WelfareDispersion)
		surplusGap = append(surplusGap, run.SurplusTransferGap)
		if run.AdaptiveWindowMinMs > 0 {
			agg.AdaptiveWindowMinMs = run.AdaptiveWindowMinMs
			agg.AdaptiveWindowMaxMs = run.AdaptiveWindowMaxMs
		}
		agg.AdaptiveWindowMeanMs += run.AdaptiveWindowMeanMs
		agg.NegativeBalanceViolationsTotal += run.NegativeBalanceViolations
		agg.ConservationBreachesTotal += run.ConservationBreaches
		agg.RiskRejectionsTotal += run.RiskRejections
	}
	if len(runs) > 0 {
		agg.AdaptiveWindowMeanMs /= float64(len(runs))
	}
	agg.MeanOrdersPerSec, agg.StdOrdersPerSec = meanStd(orders)
	agg.CI95OrdersPerSec = ci95HalfWidth(agg.StdOrdersPerSec, len(orders))
	agg.MeanFillsPerSec, agg.StdFillsPerSec = meanStd(fills)
	agg.CI95FillsPerSec = ci95HalfWidth(agg.StdFillsPerSec, len(fills))
	agg.MeanP50LatencyMs, agg.StdP50LatencyMs = meanStd(p50)
	agg.CI95P50LatencyMs = ci95HalfWidth(agg.StdP50LatencyMs, len(p50))
	agg.MeanP95LatencyMs, agg.StdP95LatencyMs = meanStd(p95)
	agg.CI95P95LatencyMs = ci95HalfWidth(agg.StdP95LatencyMs, len(p95))
	agg.MeanP99LatencyMs, agg.StdP99LatencyMs = meanStd(p99)
	agg.CI95P99LatencyMs = ci95HalfWidth(agg.StdP99LatencyMs, len(p99))
	agg.MeanAverageSpread, agg.CI95AverageSpread = meanCI95(spread)
	agg.MeanAveragePriceImpact, agg.CI95AveragePriceImpact = meanCI95(impact)
	agg.MeanQueuePriorityAdvantage, agg.CI95QueuePriorityAdvantage = meanCI95(queue)
	agg.MeanLatencyArbitrageProfit, agg.CI95LatencyArbitrageProfit = meanCI95(arb)
	agg.MeanExecutionDispersion, agg.CI95ExecutionDispersion = meanCI95(dispersion)
	agg.MeanRetailSurplusPerUnit, agg.CI95RetailSurplusPerUnit = meanCI95(retailSurplus)
	agg.MeanArbitrageurSurplusPerUnit, agg.CI95ArbitrageurSurplusPerUnit = meanCI95(arbSurplus)
	agg.MeanRetailAdverseSelectionRate, agg.CI95RetailAdverseSelectionRate = meanCI95(retailAdverse)
	agg.MeanWelfareDispersion, agg.CI95WelfareDispersion = meanCI95(welfare)
	agg.MeanSurplusTransferGap, agg.CI95SurplusTransferGap = meanCI95(surplusGap)
	return agg
}

func writeSimulatorMultiSeedArtifacts(results []aggregateResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_multiseed_profile.json")
	mdPath := filepath.Join(base, "simulator_multiseed_profile.md")
	csvPath := filepath.Join(base, "simulator_multiseed_profile.csv")

	payload := map[string]any{
		"seeds":   seeds,
		"results": results,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Multi-Seed Benchmark Profile\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("| Scenario | Runs | Window (ms) | Speed Bump (ms) | Adaptive Mean (ms) | Orders/s (mean +/- CI95) | Fills/s (mean +/- CI95) | p50 (mean +/- CI95) | p95 (mean +/- CI95) | p99 (mean +/- CI95) | Impact (mean +/- CI95) | Queue Adv. (mean +/- CI95) | Arb Profit (mean +/- CI95) | Retail Surplus (mean +/- CI95) | Retail Adverse (mean +/- CI95) | Welfare Gap (mean +/- CI95) |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("scenario,mode,batch_window_ms,speed_bump_ms,adaptive_window_min_ms,adaptive_window_max_ms,adaptive_window_mean_ms,runs,mean_orders_per_sec,std_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,std_fills_per_sec,ci95_fills_per_sec,mean_p50_latency_ms,std_p50_latency_ms,ci95_p50_latency_ms,mean_p95_latency_ms,std_p95_latency_ms,ci95_p95_latency_ms,mean_p99_latency_ms,std_p99_latency_ms,ci95_p99_latency_ms,mean_average_spread,ci95_average_spread,mean_average_price_impact,ci95_average_price_impact,mean_queue_priority_advantage,ci95_queue_priority_advantage,mean_latency_arbitrage_profit,ci95_latency_arbitrage_profit,mean_execution_dispersion,ci95_execution_dispersion,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_arbitrageur_surplus_per_unit,ci95_arbitrageur_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_welfare_dispersion,ci95_welfare_dispersion,mean_surplus_transfer_gap,ci95_surplus_transfer_gap,negative_balance_violations_total,conservation_breaches_total,risk_rejections_total\n")

	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %s | %d | %d | %d | %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			r.Name, r.Runs, r.BatchWindowMs, r.SpeedBumpMs, r.AdaptiveWindowMeanMs, r.MeanOrdersPerSec, r.CI95OrdersPerSec, r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP50LatencyMs, r.CI95P50LatencyMs, r.MeanP95LatencyMs, r.CI95P95LatencyMs, r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact, r.MeanQueuePriorityAdvantage, r.CI95QueuePriorityAdvantage,
			r.MeanLatencyArbitrageProfit, r.CI95LatencyArbitrageProfit, r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit,
			r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate, r.MeanSurplusTransferGap, r.CI95SurplusTransferGap))
		fields := []string{
			r.Name,
			string(r.Mode),
			fmt.Sprintf("%d", r.BatchWindowMs),
			fmt.Sprintf("%d", r.SpeedBumpMs),
			fmt.Sprintf("%d", r.AdaptiveWindowMinMs),
			fmt.Sprintf("%d", r.AdaptiveWindowMaxMs),
			fmt.Sprintf("%.4f", r.AdaptiveWindowMeanMs),
			fmt.Sprintf("%d", r.Runs),
			fmt.Sprintf("%.4f", r.MeanOrdersPerSec),
			fmt.Sprintf("%.4f", r.StdOrdersPerSec),
			fmt.Sprintf("%.4f", r.CI95OrdersPerSec),
			fmt.Sprintf("%.4f", r.MeanFillsPerSec),
			fmt.Sprintf("%.4f", r.StdFillsPerSec),
			fmt.Sprintf("%.4f", r.CI95FillsPerSec),
			fmt.Sprintf("%.4f", r.MeanP50LatencyMs),
			fmt.Sprintf("%.4f", r.StdP50LatencyMs),
			fmt.Sprintf("%.4f", r.CI95P50LatencyMs),
			fmt.Sprintf("%.4f", r.MeanP95LatencyMs),
			fmt.Sprintf("%.4f", r.StdP95LatencyMs),
			fmt.Sprintf("%.4f", r.CI95P95LatencyMs),
			fmt.Sprintf("%.4f", r.MeanP99LatencyMs),
			fmt.Sprintf("%.4f", r.StdP99LatencyMs),
			fmt.Sprintf("%.4f", r.CI95P99LatencyMs),
			fmt.Sprintf("%.4f", r.MeanAverageSpread),
			fmt.Sprintf("%.4f", r.CI95AverageSpread),
			fmt.Sprintf("%.4f", r.MeanAveragePriceImpact),
			fmt.Sprintf("%.4f", r.CI95AveragePriceImpact),
			fmt.Sprintf("%.6f", r.MeanQueuePriorityAdvantage),
			fmt.Sprintf("%.6f", r.CI95QueuePriorityAdvantage),
			fmt.Sprintf("%.4f", r.MeanLatencyArbitrageProfit),
			fmt.Sprintf("%.4f", r.CI95LatencyArbitrageProfit),
			fmt.Sprintf("%.6f", r.MeanExecutionDispersion),
			fmt.Sprintf("%.6f", r.CI95ExecutionDispersion),
			fmt.Sprintf("%.6f", r.MeanRetailSurplusPerUnit),
			fmt.Sprintf("%.6f", r.CI95RetailSurplusPerUnit),
			fmt.Sprintf("%.6f", r.MeanArbitrageurSurplusPerUnit),
			fmt.Sprintf("%.6f", r.CI95ArbitrageurSurplusPerUnit),
			fmt.Sprintf("%.6f", r.MeanRetailAdverseSelectionRate),
			fmt.Sprintf("%.6f", r.CI95RetailAdverseSelectionRate),
			fmt.Sprintf("%.6f", r.MeanWelfareDispersion),
			fmt.Sprintf("%.6f", r.CI95WelfareDispersion),
			fmt.Sprintf("%.6f", r.MeanSurplusTransferGap),
			fmt.Sprintf("%.6f", r.CI95SurplusTransferGap),
			fmt.Sprintf("%d", r.NegativeBalanceViolationsTotal),
			fmt.Sprintf("%d", r.ConservationBreachesTotal),
			fmt.Sprintf("%d", r.RiskRejectionsTotal),
		}
		csv.WriteString(strings.Join(fields, ",") + "\n")
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func summarizeParetoFrontier(results []aggregateResult) []paretoPoint {
	points := make([]paretoPoint, 0, len(results))
	for _, result := range results {
		points = append(points, paretoPoint{
			Name:                   result.Name,
			Category:               paretoCategory(result.Name),
			MeanP99LatencyMs:       result.MeanP99LatencyMs,
			CI95P99LatencyMs:       result.CI95P99LatencyMs,
			MeanSurplusTransferGap: result.MeanSurplusTransferGap,
			CI95SurplusTransferGap: result.CI95SurplusTransferGap,
			MeanFillsPerSec:        result.MeanFillsPerSec,
			CI95FillsPerSec:        result.CI95FillsPerSec,
		})
	}
	for idx := range points {
		dominated := false
		for jdx := range points {
			if idx == jdx {
				continue
			}
			other := points[jdx]
			if other.MeanP99LatencyMs <= points[idx].MeanP99LatencyMs &&
				other.MeanSurplusTransferGap <= points[idx].MeanSurplusTransferGap &&
				(other.MeanP99LatencyMs < points[idx].MeanP99LatencyMs || other.MeanSurplusTransferGap < points[idx].MeanSurplusTransferGap) {
				dominated = true
				break
			}
		}
		points[idx].Frontier = !dominated
	}
	sort.Slice(points, func(i, j int) bool {
		if points[i].MeanP99LatencyMs == points[j].MeanP99LatencyMs {
			return points[i].MeanSurplusTransferGap < points[j].MeanSurplusTransferGap
		}
		return points[i].MeanP99LatencyMs < points[j].MeanP99LatencyMs
	})
	return points
}

func paretoCategory(name string) string {
	switch {
	case strings.HasPrefix(name, "Policy-"):
		return "controller"
	case strings.HasPrefix(name, "Adaptive-"):
		return "adaptive"
	case strings.HasPrefix(name, "FBA-"):
		return "batch"
	case strings.HasPrefix(name, "SpeedBump"):
		return "mechanism"
	case strings.HasPrefix(name, "Immediate"):
		return "mechanism"
	default:
		return "other"
	}
}

func writeSimulatorParetoArtifacts(points []paretoPoint, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_controller_pareto.json")
	mdPath := filepath.Join(base, "simulator_controller_pareto.md")
	csvPath := filepath.Join(base, "simulator_controller_pareto.csv")

	payload := map[string]any{
		"seeds":   seeds,
		"results": points,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Controller Pareto Frontier\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("Pareto axes minimize `p99 latency` and `surplus transfer gap`; `fills/s` is reported as a third axis for interpretation.\n\n")
	md.WriteString("| Scenario | Category | Frontier | p99 (ms) | Welfare Gap | Fills/s |\n")
	md.WriteString("|---|---|:---:|---:|---:|---:|\n")
	for _, point := range points {
		flag := ""
		if point.Frontier {
			flag = "yes"
		}
		md.WriteString(fmt.Sprintf("| %s | %s | %s | %.2f +/- %.2f | %.4f +/- %.4f | %.2f +/- %.2f |\n",
			point.Name, point.Category, flag,
			point.MeanP99LatencyMs, point.CI95P99LatencyMs,
			point.MeanSurplusTransferGap, point.CI95SurplusTransferGap,
			point.MeanFillsPerSec, point.CI95FillsPerSec))
	}

	var csv strings.Builder
	csv.WriteString("scenario,category,frontier,mean_p99_latency_ms,ci95_p99_latency_ms,mean_surplus_transfer_gap,ci95_surplus_transfer_gap,mean_fills_per_sec,ci95_fills_per_sec\n")
	for _, point := range points {
		csv.WriteString(fmt.Sprintf("%s,%s,%t,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			point.Name, point.Category, point.Frontier,
			point.MeanP99LatencyMs, point.CI95P99LatencyMs,
			point.MeanSurplusTransferGap, point.CI95SurplusTransferGap,
			point.MeanFillsPerSec, point.CI95FillsPerSec))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorAblationArtifacts(results []aggregateResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_ablation_profile.json")
	mdPath := filepath.Join(base, "simulator_ablation_profile.md")
	csvPath := filepath.Join(base, "simulator_ablation_profile.csv")

	payload := map[string]any{
		"seeds":   seeds,
		"results": results,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Ablation Profile\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("| Scenario | Orders/s | Fills/s | p99 (ms) | Queue Adv. | Arb Profit | Risk Rejects | Safety Violations |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("scenario,mode,mean_orders_per_sec,mean_fills_per_sec,mean_p99_latency_ms,mean_queue_priority_advantage,mean_latency_arbitrage_profit,risk_rejections_total,negative_balance_violations_total,conservation_breaches_total\n")
	for _, r := range results {
		safety := r.NegativeBalanceViolationsTotal + r.ConservationBreachesTotal
		md.WriteString(fmt.Sprintf("| %s | %.2f | %.2f | %.2f | %.4f | %.2f | %d | %d |\n",
			r.Name, r.MeanOrdersPerSec, r.MeanFillsPerSec, r.MeanP99LatencyMs, r.MeanQueuePriorityAdvantage, r.MeanLatencyArbitrageProfit, r.RiskRejectionsTotal, safety))
		csv.WriteString(fmt.Sprintf("%s,%s,%.4f,%.4f,%.4f,%.6f,%.4f,%d,%d,%d\n",
			r.Name, r.Mode, r.MeanOrdersPerSec, r.MeanFillsPerSec, r.MeanP99LatencyMs, r.MeanQueuePriorityAdvantage, r.MeanLatencyArbitrageProfit,
			r.RiskRejectionsTotal, r.NegativeBalanceViolationsTotal, r.ConservationBreachesTotal))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorAgentAblationArtifacts(results []aggregateResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_agent_ablation_profile.json")
	mdPath := filepath.Join(base, "simulator_agent_ablation_profile.md")
	csvPath := filepath.Join(base, "simulator_agent_ablation_profile.csv")

	payload := map[string]any{
		"seeds":   seeds,
		"results": results,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Agent/Workload Ablation Profile\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("| Scenario | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("scenario,mode,mean_orders_per_sec,mean_fills_per_sec,mean_p99_latency_ms,mean_average_price_impact,mean_queue_priority_advantage,mean_latency_arbitrage_profit\n")
	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %s | %.2f | %.2f | %.2f | %.2f | %.4f | %.2f |\n",
			r.Name, r.MeanOrdersPerSec, r.MeanFillsPerSec, r.MeanP99LatencyMs, r.MeanAveragePriceImpact, r.MeanQueuePriorityAdvantage, r.MeanLatencyArbitrageProfit))
		csv.WriteString(fmt.Sprintf("%s,%s,%.4f,%.4f,%.4f,%.4f,%.6f,%.4f\n",
			r.Name, r.Mode, r.MeanOrdersPerSec, r.MeanFillsPerSec, r.MeanP99LatencyMs, r.MeanAveragePriceImpact, r.MeanQueuePriorityAdvantage, r.MeanLatencyArbitrageProfit))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func summarizeGridRun(base ScenarioConfig, runs []BenchmarkResult) gridSweepResult {
	arbMultiplier := 0
	makerWidth := 1
	if _, err := fmt.Sscanf(base.Name, "Grid-Arb%d-Maker%d", &arbMultiplier, &makerWidth); err != nil {
		arbMultiplier = 0
		makerWidth = 1
	}

	orders := make([]float64, 0, len(runs))
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	impact := make([]float64, 0, len(runs))
	queue := make([]float64, 0, len(runs))
	arb := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	for _, run := range runs {
		orders = append(orders, run.OrdersPerSec)
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		impact = append(impact, run.AveragePriceImpact)
		queue = append(queue, run.QueuePriorityAdvantage)
		arb = append(arb, run.LatencyArbitrageProfit)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
	}
	meanOrders, stdOrders := meanStd(orders)
	meanFills, stdFills := meanStd(fills)
	meanP99, stdP99 := meanStd(p99)
	meanImpact, ciImpact := meanCI95(impact)
	meanQueue, ciQueue := meanCI95(queue)
	meanArb, ciArb := meanCI95(arb)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	return gridSweepResult{
		ArbitrageurIntensityMultiplier: arbMultiplier,
		MakerQuoteWidthMultiplier:      makerWidth,
		Runs:                           len(runs),
		MeanOrdersPerSec:               meanOrders,
		CI95OrdersPerSec:               ci95HalfWidth(stdOrders, len(orders)),
		MeanFillsPerSec:                meanFills,
		CI95FillsPerSec:                ci95HalfWidth(stdFills, len(fills)),
		MeanP99LatencyMs:               meanP99,
		CI95P99LatencyMs:               ci95HalfWidth(stdP99, len(p99)),
		MeanAveragePriceImpact:         meanImpact,
		CI95AveragePriceImpact:         ciImpact,
		MeanQueuePriorityAdvantage:     meanQueue,
		CI95QueuePriorityAdvantage:     ciQueue,
		MeanLatencyArbitrageProfit:     meanArb,
		CI95LatencyArbitrageProfit:     ciArb,
		MeanRetailSurplusPerUnit:       meanRetailSurplus,
		CI95RetailSurplusPerUnit:       ciRetailSurplus,
		MeanRetailAdverseSelectionRate: meanRetailAdverse,
		CI95RetailAdverseSelectionRate: ciRetailAdverse,
	}
}

func summarizeCubeRun(base ScenarioConfig, runs []BenchmarkResult) cubeSweepResult {
	retailMultiplier := 1
	informedMultiplier := 1
	makerWidth := 1
	if _, err := fmt.Sscanf(base.Name, "Cube-Retail%d-Informed%d-Maker%d", &retailMultiplier, &informedMultiplier, &makerWidth); err != nil {
		retailMultiplier = 1
		informedMultiplier = 1
		makerWidth = 1
	}

	orders := make([]float64, 0, len(runs))
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	impact := make([]float64, 0, len(runs))
	queue := make([]float64, 0, len(runs))
	arb := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	for _, run := range runs {
		orders = append(orders, run.OrdersPerSec)
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		impact = append(impact, run.AveragePriceImpact)
		queue = append(queue, run.QueuePriorityAdvantage)
		arb = append(arb, run.LatencyArbitrageProfit)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
	}
	meanOrders, stdOrders := meanStd(orders)
	meanFills, stdFills := meanStd(fills)
	meanP99, stdP99 := meanStd(p99)
	meanImpact, ciImpact := meanCI95(impact)
	meanQueue, ciQueue := meanCI95(queue)
	meanArb, ciArb := meanCI95(arb)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	return cubeSweepResult{
		RetailIntensityMultiplier:      retailMultiplier,
		InformedIntensityMultiplier:    informedMultiplier,
		MakerQuoteWidthMultiplier:      makerWidth,
		Runs:                           len(runs),
		MeanOrdersPerSec:               meanOrders,
		CI95OrdersPerSec:               ci95HalfWidth(stdOrders, len(orders)),
		MeanFillsPerSec:                meanFills,
		CI95FillsPerSec:                ci95HalfWidth(stdFills, len(fills)),
		MeanP99LatencyMs:               meanP99,
		CI95P99LatencyMs:               ci95HalfWidth(stdP99, len(p99)),
		MeanAveragePriceImpact:         meanImpact,
		CI95AveragePriceImpact:         ciImpact,
		MeanQueuePriorityAdvantage:     meanQueue,
		CI95QueuePriorityAdvantage:     ciQueue,
		MeanLatencyArbitrageProfit:     meanArb,
		CI95LatencyArbitrageProfit:     ciArb,
		MeanRetailSurplusPerUnit:       meanRetailSurplus,
		CI95RetailSurplusPerUnit:       ciRetailSurplus,
		MeanRetailAdverseSelectionRate: meanRetailAdverse,
		CI95RetailAdverseSelectionRate: ciRetailAdverse,
	}
}

func summarizeHypercubeRun(base ScenarioConfig, runs []BenchmarkResult) hypercubeSweepResult {
	arbMultiplier := 0
	retailMultiplier := 1
	informedMultiplier := 1
	makerWidth := 1
	if _, err := fmt.Sscanf(base.Name, "Hyper-Arb%d-Retail%d-Informed%d-Maker%d", &arbMultiplier, &retailMultiplier, &informedMultiplier, &makerWidth); err != nil {
		arbMultiplier = 0
		retailMultiplier = 1
		informedMultiplier = 1
		makerWidth = 1
	}

	orders := make([]float64, 0, len(runs))
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	impact := make([]float64, 0, len(runs))
	queue := make([]float64, 0, len(runs))
	arb := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	welfare := make([]float64, 0, len(runs))
	surplusGap := make([]float64, 0, len(runs))
	for _, run := range runs {
		orders = append(orders, run.OrdersPerSec)
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		impact = append(impact, run.AveragePriceImpact)
		queue = append(queue, run.QueuePriorityAdvantage)
		arb = append(arb, run.LatencyArbitrageProfit)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
		welfare = append(welfare, run.WelfareDispersion)
		surplusGap = append(surplusGap, run.SurplusTransferGap)
	}
	meanOrders, stdOrders := meanStd(orders)
	meanFills, stdFills := meanStd(fills)
	meanP99, stdP99 := meanStd(p99)
	meanImpact, ciImpact := meanCI95(impact)
	meanQueue, ciQueue := meanCI95(queue)
	meanArb, ciArb := meanCI95(arb)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanWelfare, ciWelfare := meanCI95(welfare)
	meanGap, ciGap := meanCI95(surplusGap)
	return hypercubeSweepResult{
		ArbitrageurIntensityMultiplier: arbMultiplier,
		RetailIntensityMultiplier:      retailMultiplier,
		InformedIntensityMultiplier:    informedMultiplier,
		MakerQuoteWidthMultiplier:      makerWidth,
		Runs:                           len(runs),
		MeanOrdersPerSec:               meanOrders,
		CI95OrdersPerSec:               ci95HalfWidth(stdOrders, len(orders)),
		MeanFillsPerSec:                meanFills,
		CI95FillsPerSec:                ci95HalfWidth(stdFills, len(fills)),
		MeanP99LatencyMs:               meanP99,
		CI95P99LatencyMs:               ci95HalfWidth(stdP99, len(p99)),
		MeanAveragePriceImpact:         meanImpact,
		CI95AveragePriceImpact:         ciImpact,
		MeanQueuePriorityAdvantage:     meanQueue,
		CI95QueuePriorityAdvantage:     ciQueue,
		MeanLatencyArbitrageProfit:     meanArb,
		CI95LatencyArbitrageProfit:     ciArb,
		MeanRetailSurplusPerUnit:       meanRetailSurplus,
		CI95RetailSurplusPerUnit:       ciRetailSurplus,
		MeanRetailAdverseSelectionRate: meanRetailAdverse,
		CI95RetailAdverseSelectionRate: ciRetailAdverse,
		MeanWelfareDispersion:          meanWelfare,
		CI95WelfareDispersion:          ciWelfare,
		MeanSurplusTransferGap:         meanGap,
		CI95SurplusTransferGap:         ciGap,
	}
}

func summarizeHypercubeCompact(results []hypercubeSweepResult, seeds []int64) hypercubeCompactSummary {
	factorLevels := map[string][]int{
		"arbitrageur_intensity": {0, 1, 2, 3},
		"retail_intensity":      {1, 2, 3},
		"informed_intensity":    {1, 2, 3},
		"maker_quote_width":     {1, 2, 3},
	}
	mainEffects := make(map[string][]hypercubeFactorLevelSummary, len(factorLevels))
	contrasts := make([]hypercubeHighLowContrast, 0, len(factorLevels))
	for factor, levels := range factorLevels {
		summaries := make([]hypercubeFactorLevelSummary, 0, len(levels))
		for _, level := range levels {
			filtered := filterHypercubeByLevel(results, factor, level)
			summaries = append(summaries, summarizeHypercubeLevel(factor, level, filtered))
		}
		mainEffects[factor] = summaries
		contrasts = append(contrasts, summarizeHypercubeContrast(factor, levels[0], levels[len(levels)-1], results))
	}

	retailEffects := make([]retailConditionedArbitrageEffect, 0, 3)
	for _, retail := range []int{1, 2, 3} {
		low := filterHypercubeByRetailAndArbitrage(results, retail, 0)
		high := filterHypercubeByRetailAndArbitrage(results, retail, 3)
		retailEffects = append(retailEffects, retailConditionedArbitrageEffect{
			RetailIntensityMultiplier: retail,
			DeltaOrdersPerSec:         meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 { return r.MeanOrdersPerSec }) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanOrdersPerSec }),
			DeltaP99LatencyMs:         meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 { return r.MeanP99LatencyMs }) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanP99LatencyMs }),
			DeltaLatencyArbitrageProfit: meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 {
				return r.MeanLatencyArbitrageProfit
			}) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanLatencyArbitrageProfit }),
			DeltaRetailSurplusPerUnit: meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 {
				return r.MeanRetailSurplusPerUnit
			}) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanRetailSurplusPerUnit }),
			DeltaRetailAdverseSelection: meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 {
				return r.MeanRetailAdverseSelectionRate
			}) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanRetailAdverseSelectionRate }),
			DeltaSurplusTransferGap: meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 {
				return r.MeanSurplusTransferGap
			}) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanSurplusTransferGap }),
		})
	}

	return hypercubeCompactSummary{
		Seeds:                      seeds,
		PrimaryWelfareMetrics:      []string{"retail_surplus_per_unit", "retail_adverse_selection_rate", "surplus_transfer_gap"},
		MainEffects:                mainEffects,
		HighLowContrasts:           contrasts,
		RetailConditionedArbitrage: retailEffects,
	}
}

func summarizeHypercubeResponseSurface(results []hypercubeSweepResult, seeds []int64) hypercubeResponseSurfaceSummary {
	return hypercubeResponseSurfaceSummary{
		Seeds: seeds,
		Fits: []responseSurfaceFit{
			fitHypercubeResponseSurface("surplus_transfer_gap", results, func(r hypercubeSweepResult) float64 { return r.MeanSurplusTransferGap }),
			fitHypercubeResponseSurface("p99_latency_ms", results, func(r hypercubeSweepResult) float64 { return r.MeanP99LatencyMs }),
			fitHypercubeResponseSurface("retail_surplus_per_unit", results, func(r hypercubeSweepResult) float64 { return r.MeanRetailSurplusPerUnit }),
		},
	}
}

func fitHypercubeResponseSurface(metric string, results []hypercubeSweepResult, selector func(hypercubeSweepResult) float64) responseSurfaceFit {
	featureNames := []string{
		"intercept",
		"arb",
		"retail",
		"informed",
		"maker",
		"arb_x_retail",
		"arb_x_informed",
		"arb_x_maker",
		"retail_x_informed",
		"retail_x_maker",
		"informed_x_maker",
	}
	groupColumns := []struct {
		name    string
		columns []int
	}{
		{name: "arbitrageur_intensity", columns: []int{1, 5, 6, 7}},
		{name: "retail_intensity", columns: []int{2, 5, 8, 9}},
		{name: "informed_intensity", columns: []int{3, 6, 8, 10}},
		{name: "maker_quote_width", columns: []int{4, 7, 9, 10}},
		{name: "arb_x_retail", columns: []int{5}},
		{name: "arb_x_informed", columns: []int{6}},
		{name: "arb_x_maker", columns: []int{7}},
		{name: "retail_x_informed", columns: []int{8}},
		{name: "retail_x_maker", columns: []int{9}},
		{name: "informed_x_maker", columns: []int{10}},
	}

	rows := make([][]float64, 0, len(results))
	target := make([]float64, 0, len(results))
	for _, result := range results {
		arb := standardizeArbitrage(result.ArbitrageurIntensityMultiplier)
		retail := standardizeThreeLevel(result.RetailIntensityMultiplier)
		informed := standardizeThreeLevel(result.InformedIntensityMultiplier)
		maker := standardizeThreeLevel(result.MakerQuoteWidthMultiplier)
		rows = append(rows, []float64{
			1.0,
			arb,
			retail,
			informed,
			maker,
			arb * retail,
			arb * informed,
			arb * maker,
			retail * informed,
			retail * maker,
			informed * maker,
		})
		target = append(target, selector(result))
	}

	beta, predictions, sse, sst := fitLinearModel(rows, target, nil)
	r2 := 0.0
	if sst > 0 {
		r2 = 1.0 - sse/sst
	}
	if r2 < 0 {
		r2 = 0
	}
	coefficients := make([]responseSurfaceCoefficient, 0, len(featureNames))
	for idx, name := range featureNames {
		coefficients = append(coefficients, responseSurfaceCoefficient{
			Name:        name,
			Coefficient: beta[idx],
		})
	}
	effects := make([]responseSurfaceEffect, 0, len(groupColumns))
	for _, group := range groupColumns {
		reducedBeta, _, reducedSSE, _ := fitLinearModel(rows, target, group.columns)
		_ = reducedBeta
		partial := 0.0
		if sst > 0 {
			partial = (reducedSSE - sse) / sst
		}
		if partial < 0 {
			partial = 0
		}
		effects = append(effects, responseSurfaceEffect{
			Factor:    group.name,
			PartialR2: partial,
		})
	}
	sort.Slice(effects, func(i, j int) bool {
		return effects[i].PartialR2 > effects[j].PartialR2
	})

	rmse := 0.0
	if len(predictions) > 0 {
		rmse = math.Sqrt(sse / float64(len(predictions)))
	}
	return responseSurfaceFit{
		Metric:       metric,
		R2:           r2,
		RMSE:         rmse,
		Coefficients: coefficients,
		Effects:      effects,
	}
}

func fitLinearModel(rows [][]float64, target []float64, dropColumns []int) ([]float64, []float64, float64, float64) {
	keep := make([]int, 0, len(rows[0]))
	drop := make(map[int]bool, len(dropColumns))
	for _, idx := range dropColumns {
		drop[idx] = true
	}
	for idx := range rows[0] {
		if !drop[idx] {
			keep = append(keep, idx)
		}
	}

	design := make([][]float64, len(rows))
	for i, row := range rows {
		design[i] = make([]float64, 0, len(keep))
		for _, idx := range keep {
			design[i] = append(design[i], row[idx])
		}
	}
	xtx := make([][]float64, len(keep))
	xty := make([]float64, len(keep))
	for i := range xtx {
		xtx[i] = make([]float64, len(keep))
		xtx[i][i] = 1e-6
	}
	for rowIdx, row := range design {
		for i := range row {
			xty[i] += row[i] * target[rowIdx]
			for j := range row {
				xtx[i][j] += row[i] * row[j]
			}
		}
	}
	reducedBeta := solveLinearSystem(xtx, xty)
	fullBeta := make([]float64, len(rows[0]))
	for i, idx := range keep {
		fullBeta[idx] = reducedBeta[i]
	}
	predictions := make([]float64, len(rows))
	targetMean, _ := meanStd(target)
	sse := 0.0
	sst := 0.0
	for i, row := range rows {
		pred := 0.0
		for j, value := range row {
			pred += fullBeta[j] * value
		}
		predictions[i] = pred
		err := target[i] - pred
		sse += err * err
		centered := target[i] - targetMean
		sst += centered * centered
	}
	return fullBeta, predictions, sse, sst
}

func standardizeArbitrage(level int) float64 {
	return (float64(level) - 1.5) / math.Sqrt(1.25)
}

func standardizeThreeLevel(level int) float64 {
	return (float64(level) - 2.0) / math.Sqrt(2.0/3.0)
}

func summarizeHeldOutPolicyRuns(regime string, policy PolicyController, runs []BenchmarkResult) heldOutPolicyResult {
	orders := make([]float64, 0, len(runs))
	fills := make([]float64, 0, len(runs))
	p99 := make([]float64, 0, len(runs))
	impact := make([]float64, 0, len(runs))
	retailSurplus := make([]float64, 0, len(runs))
	retailAdverse := make([]float64, 0, len(runs))
	gap := make([]float64, 0, len(runs))
	negative := 0
	conservation := 0
	for _, run := range runs {
		orders = append(orders, run.OrdersPerSec)
		fills = append(fills, run.FillsPerSec)
		p99 = append(p99, run.P99LatencyMs)
		impact = append(impact, run.AveragePriceImpact)
		retailSurplus = append(retailSurplus, run.RetailSurplusPerUnit)
		retailAdverse = append(retailAdverse, run.RetailAdverseSelectionRate)
		gap = append(gap, run.SurplusTransferGap)
		negative += run.NegativeBalanceViolations
		conservation += run.ConservationBreaches
	}
	meanOrders, ciOrders := meanCI95(orders)
	meanFills, ciFills := meanCI95(fills)
	meanP99, ciP99 := meanCI95(p99)
	meanImpact, ciImpact := meanCI95(impact)
	meanRetailSurplus, ciRetailSurplus := meanCI95(retailSurplus)
	meanRetailAdverse, ciRetailAdverse := meanCI95(retailAdverse)
	meanGap, ciGap := meanCI95(gap)
	return heldOutPolicyResult{
		RegimeName:                     regime,
		Policy:                         string(policy),
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
		NegativeBalanceViolationsTotal: negative,
		ConservationBreachesTotal:      conservation,
	}
}

func summarizeHeldOutPolicyTable(results []heldOutPolicyResult) []heldOutPolicySummary {
	byPolicy := make(map[string][]heldOutPolicyResult)
	for _, result := range results {
		byPolicy[result.Policy] = append(byPolicy[result.Policy], result)
	}
	policies := make([]string, 0, len(byPolicy))
	for policy := range byPolicy {
		policies = append(policies, policy)
	}
	sort.Strings(policies)
	summaries := make([]heldOutPolicySummary, 0, len(policies))
	for _, policy := range policies {
		group := byPolicy[policy]
		summaries = append(summaries, heldOutPolicySummary{
			Policy:                         policy,
			RegimeCount:                    len(group),
			MeanOrdersPerSec:               meanHeldOutMetric(group, func(r heldOutPolicyResult) float64 { return r.MeanOrdersPerSec }),
			MeanFillsPerSec:                meanHeldOutMetric(group, func(r heldOutPolicyResult) float64 { return r.MeanFillsPerSec }),
			MeanP99LatencyMs:               meanHeldOutMetric(group, func(r heldOutPolicyResult) float64 { return r.MeanP99LatencyMs }),
			MeanAveragePriceImpact:         meanHeldOutMetric(group, func(r heldOutPolicyResult) float64 { return r.MeanAveragePriceImpact }),
			MeanRetailSurplusPerUnit:       meanHeldOutMetric(group, func(r heldOutPolicyResult) float64 { return r.MeanRetailSurplusPerUnit }),
			MeanRetailAdverseSelectionRate: meanHeldOutMetric(group, func(r heldOutPolicyResult) float64 { return r.MeanRetailAdverseSelectionRate }),
			MeanSurplusTransferGap:         meanHeldOutMetric(group, func(r heldOutPolicyResult) float64 { return r.MeanSurplusTransferGap }),
		})
	}
	return summaries
}

func meanHeldOutMetric(results []heldOutPolicyResult, selector func(heldOutPolicyResult) float64) float64 {
	if len(results) == 0 {
		return 0
	}
	values := make([]float64, 0, len(results))
	for _, result := range results {
		values = append(values, selector(result))
	}
	mean, _ := meanStd(values)
	return mean
}

func filterHypercubeByLevel(results []hypercubeSweepResult, factor string, level int) []hypercubeSweepResult {
	filtered := make([]hypercubeSweepResult, 0, len(results))
	for _, result := range results {
		switch factor {
		case "arbitrageur_intensity":
			if result.ArbitrageurIntensityMultiplier == level {
				filtered = append(filtered, result)
			}
		case "retail_intensity":
			if result.RetailIntensityMultiplier == level {
				filtered = append(filtered, result)
			}
		case "informed_intensity":
			if result.InformedIntensityMultiplier == level {
				filtered = append(filtered, result)
			}
		case "maker_quote_width":
			if result.MakerQuoteWidthMultiplier == level {
				filtered = append(filtered, result)
			}
		}
	}
	return filtered
}

func filterHypercubeByRetailAndArbitrage(results []hypercubeSweepResult, retail, arbitrage int) []hypercubeSweepResult {
	filtered := make([]hypercubeSweepResult, 0, len(results))
	for _, result := range results {
		if result.RetailIntensityMultiplier == retail && result.ArbitrageurIntensityMultiplier == arbitrage {
			filtered = append(filtered, result)
		}
	}
	return filtered
}

func summarizeHypercubeLevel(factor string, level int, results []hypercubeSweepResult) hypercubeFactorLevelSummary {
	return hypercubeFactorLevelSummary{
		Factor:                         factor,
		Level:                          level,
		CellCount:                      len(results),
		MeanOrdersPerSec:               meanHypercubeMetric(results, func(r hypercubeSweepResult) float64 { return r.MeanOrdersPerSec }),
		MeanP99LatencyMs:               meanHypercubeMetric(results, func(r hypercubeSweepResult) float64 { return r.MeanP99LatencyMs }),
		MeanLatencyArbitrageProfit:     meanHypercubeMetric(results, func(r hypercubeSweepResult) float64 { return r.MeanLatencyArbitrageProfit }),
		MeanRetailSurplusPerUnit:       meanHypercubeMetric(results, func(r hypercubeSweepResult) float64 { return r.MeanRetailSurplusPerUnit }),
		MeanRetailAdverseSelectionRate: meanHypercubeMetric(results, func(r hypercubeSweepResult) float64 { return r.MeanRetailAdverseSelectionRate }),
		MeanSurplusTransferGap:         meanHypercubeMetric(results, func(r hypercubeSweepResult) float64 { return r.MeanSurplusTransferGap }),
	}
}

func summarizeHypercubeContrast(factor string, lowLevel, highLevel int, results []hypercubeSweepResult) hypercubeHighLowContrast {
	low := filterHypercubeByLevel(results, factor, lowLevel)
	high := filterHypercubeByLevel(results, factor, highLevel)
	return hypercubeHighLowContrast{
		Factor:                      factor,
		LowLevel:                    lowLevel,
		HighLevel:                   highLevel,
		DeltaOrdersPerSec:           meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 { return r.MeanOrdersPerSec }) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanOrdersPerSec }),
		DeltaP99LatencyMs:           meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 { return r.MeanP99LatencyMs }) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanP99LatencyMs }),
		DeltaLatencyArbitrageProfit: meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 { return r.MeanLatencyArbitrageProfit }) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanLatencyArbitrageProfit }),
		DeltaRetailSurplusPerUnit:   meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 { return r.MeanRetailSurplusPerUnit }) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanRetailSurplusPerUnit }),
		DeltaRetailAdverseSelection: meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 { return r.MeanRetailAdverseSelectionRate }) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanRetailAdverseSelectionRate }),
		DeltaSurplusTransferGap:     meanHypercubeMetric(high, func(r hypercubeSweepResult) float64 { return r.MeanSurplusTransferGap }) - meanHypercubeMetric(low, func(r hypercubeSweepResult) float64 { return r.MeanSurplusTransferGap }),
	}
}

func meanHypercubeMetric(results []hypercubeSweepResult, selector func(hypercubeSweepResult) float64) float64 {
	if len(results) == 0 {
		return 0
	}
	values := make([]float64, 0, len(results))
	for _, result := range results {
		values = append(values, selector(result))
	}
	mean, _ := meanStd(values)
	return mean
}

func findGridResult(t *testing.T, results []gridSweepResult, arb, maker int) gridSweepResult {
	t.Helper()
	for _, result := range results {
		if result.ArbitrageurIntensityMultiplier == arb && result.MakerQuoteWidthMultiplier == maker {
			return result
		}
	}
	t.Fatalf("grid cell arb=%d maker=%d not found", arb, maker)
	return gridSweepResult{}
}

func findCubeResult(t *testing.T, results []cubeSweepResult, retail, informed, maker int) cubeSweepResult {
	t.Helper()
	for _, result := range results {
		if result.RetailIntensityMultiplier == retail &&
			result.InformedIntensityMultiplier == informed &&
			result.MakerQuoteWidthMultiplier == maker {
			return result
		}
	}
	t.Fatalf("cube cell retail=%d informed=%d maker=%d not found", retail, informed, maker)
	return cubeSweepResult{}
}

func findHypercubeResult(t *testing.T, results []hypercubeSweepResult, arb, retail, informed, maker int) hypercubeSweepResult {
	t.Helper()
	for _, result := range results {
		if result.ArbitrageurIntensityMultiplier == arb &&
			result.RetailIntensityMultiplier == retail &&
			result.InformedIntensityMultiplier == informed &&
			result.MakerQuoteWidthMultiplier == maker {
			return result
		}
	}
	t.Fatalf("hypercube cell arb=%d retail=%d informed=%d maker=%d not found", arb, retail, informed, maker)
	return hypercubeSweepResult{}
}

func findHeldOutPolicyResult(t *testing.T, results []heldOutPolicyResult, regime string, policy PolicyController) heldOutPolicyResult {
	t.Helper()
	for _, result := range results {
		if result.RegimeName == regime && result.Policy == string(policy) {
			return result
		}
	}
	t.Fatalf("held-out result regime=%s policy=%s not found", regime, policy)
	return heldOutPolicyResult{}
}

func writeSimulatorGridArtifacts(results []gridSweepResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_parameter_grid_profile.json")
	mdPath := filepath.Join(base, "simulator_parameter_grid_profile.md")
	csvPath := filepath.Join(base, "simulator_parameter_grid_profile.csv")

	payload := map[string]any{
		"seeds":   seeds,
		"results": results,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Parameter Grid Profile\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("| Arb Multiplier | Maker Width Multiplier | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit | Retail Surplus | Retail Adverse |\n")
	md.WriteString("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("arbitrageur_intensity_multiplier,maker_quote_width_multiplier,runs,mean_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_average_price_impact,ci95_average_price_impact,mean_queue_priority_advantage,ci95_queue_priority_advantage,mean_latency_arbitrage_profit,ci95_latency_arbitrage_profit,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate\n")
	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %d | %d | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			r.ArbitrageurIntensityMultiplier, r.MakerQuoteWidthMultiplier, r.Runs,
			r.MeanOrdersPerSec, r.CI95OrdersPerSec,
			r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanQueuePriorityAdvantage, r.CI95QueuePriorityAdvantage,
			r.MeanLatencyArbitrageProfit, r.CI95LatencyArbitrageProfit, r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit, r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate))
		csv.WriteString(fmt.Sprintf("%d,%d,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.6f,%.4f,%.4f,%.6f,%.6f,%.6f,%.6f\n",
			r.ArbitrageurIntensityMultiplier, r.MakerQuoteWidthMultiplier, r.Runs,
			r.MeanOrdersPerSec, r.CI95OrdersPerSec,
			r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanQueuePriorityAdvantage, r.CI95QueuePriorityAdvantage,
			r.MeanLatencyArbitrageProfit, r.CI95LatencyArbitrageProfit,
			r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit,
			r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorCubeArtifacts(results []cubeSweepResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_parameter_cube_profile.json")
	mdPath := filepath.Join(base, "simulator_parameter_cube_profile.md")
	csvPath := filepath.Join(base, "simulator_parameter_cube_profile.csv")

	payload := map[string]any{
		"seeds":   seeds,
		"results": results,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Parameter Cube Profile\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("| Retail Multiplier | Informed Multiplier | Maker Width Multiplier | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit | Retail Surplus | Retail Adverse |\n")
	md.WriteString("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("retail_intensity_multiplier,informed_intensity_multiplier,maker_quote_width_multiplier,runs,mean_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_average_price_impact,ci95_average_price_impact,mean_queue_priority_advantage,ci95_queue_priority_advantage,mean_latency_arbitrage_profit,ci95_latency_arbitrage_profit,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate\n")
	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %d | %d | %d | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			r.RetailIntensityMultiplier, r.InformedIntensityMultiplier, r.MakerQuoteWidthMultiplier, r.Runs,
			r.MeanOrdersPerSec, r.CI95OrdersPerSec,
			r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanQueuePriorityAdvantage, r.CI95QueuePriorityAdvantage,
			r.MeanLatencyArbitrageProfit, r.CI95LatencyArbitrageProfit, r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit, r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate))
		csv.WriteString(fmt.Sprintf("%d,%d,%d,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.6f,%.4f,%.4f,%.6f,%.6f,%.6f,%.6f\n",
			r.RetailIntensityMultiplier, r.InformedIntensityMultiplier, r.MakerQuoteWidthMultiplier, r.Runs,
			r.MeanOrdersPerSec, r.CI95OrdersPerSec,
			r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanQueuePriorityAdvantage, r.CI95QueuePriorityAdvantage,
			r.MeanLatencyArbitrageProfit, r.CI95LatencyArbitrageProfit,
			r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit,
			r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorHypercubeArtifacts(results []hypercubeSweepResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_parameter_hypercube_profile.json")
	mdPath := filepath.Join(base, "simulator_parameter_hypercube_profile.md")
	csvPath := filepath.Join(base, "simulator_parameter_hypercube_profile.csv")

	payload := map[string]any{
		"seeds":   seeds,
		"results": results,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Parameter Hypercube Profile\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("| Arb | Retail | Informed | Maker Width | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Queue Adv. | Arb Profit | Retail Surplus | Retail Adverse | Welfare Dispersion | Welfare Gap |\n")
	md.WriteString("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("arbitrageur_intensity_multiplier,retail_intensity_multiplier,informed_intensity_multiplier,maker_quote_width_multiplier,runs,mean_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_average_price_impact,ci95_average_price_impact,mean_queue_priority_advantage,ci95_queue_priority_advantage,mean_latency_arbitrage_profit,ci95_latency_arbitrage_profit,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_welfare_dispersion,ci95_welfare_dispersion,mean_surplus_transfer_gap,ci95_surplus_transfer_gap\n")
	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %d | %d | %d | %d | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f |\n",
			r.ArbitrageurIntensityMultiplier, r.RetailIntensityMultiplier, r.InformedIntensityMultiplier, r.MakerQuoteWidthMultiplier, r.Runs,
			r.MeanOrdersPerSec, r.CI95OrdersPerSec,
			r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanQueuePriorityAdvantage, r.CI95QueuePriorityAdvantage,
			r.MeanLatencyArbitrageProfit, r.CI95LatencyArbitrageProfit,
			r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit,
			r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate,
			r.MeanWelfareDispersion, r.CI95WelfareDispersion,
			r.MeanSurplusTransferGap, r.CI95SurplusTransferGap))
		csv.WriteString(fmt.Sprintf("%d,%d,%d,%d,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.6f,%.4f,%.4f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			r.ArbitrageurIntensityMultiplier, r.RetailIntensityMultiplier, r.InformedIntensityMultiplier, r.MakerQuoteWidthMultiplier, r.Runs,
			r.MeanOrdersPerSec, r.CI95OrdersPerSec,
			r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanQueuePriorityAdvantage, r.CI95QueuePriorityAdvantage,
			r.MeanLatencyArbitrageProfit, r.CI95LatencyArbitrageProfit,
			r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit,
			r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate,
			r.MeanWelfareDispersion, r.CI95WelfareDispersion,
			r.MeanSurplusTransferGap, r.CI95SurplusTransferGap))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorHypercubeSummaryArtifacts(summary hypercubeCompactSummary) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_parameter_hypercube_summary.json")
	mdPath := filepath.Join(base, "simulator_parameter_hypercube_summary.md")
	csvPath := filepath.Join(base, "simulator_parameter_hypercube_summary.csv")

	raw, err := json.MarshalIndent(summary, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Parameter Hypercube Summary\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", summary.Seeds))
	md.WriteString("Primary welfare metrics emphasized in the paper line:\n\n")
	for _, metric := range summary.PrimaryWelfareMetrics {
		md.WriteString(fmt.Sprintf("- `%s`\n", metric))
	}
	md.WriteString("\n## Main Effects\n\n")
	md.WriteString("| Factor | Level | Cells | Orders/s | p99 (ms) | Arb Profit | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, factor := range []string{"arbitrageur_intensity", "retail_intensity", "informed_intensity", "maker_quote_width"} {
		for _, level := range summary.MainEffects[factor] {
			md.WriteString(fmt.Sprintf("| %s | %d | %d | %.2f | %.2f | %.2f | %.4f | %.4f | %.4f |\n",
				level.Factor, level.Level, level.CellCount,
				level.MeanOrdersPerSec, level.MeanP99LatencyMs, level.MeanLatencyArbitrageProfit,
				level.MeanRetailSurplusPerUnit, level.MeanRetailAdverseSelectionRate, level.MeanSurplusTransferGap))
		}
	}
	md.WriteString("\n## High-Low Contrasts\n\n")
	md.WriteString("| Factor | Low | High | Delta Orders/s | Delta p99 (ms) | Delta Arb Profit | Delta Retail Surplus | Delta Retail Adverse | Delta Welfare Gap |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, contrast := range summary.HighLowContrasts {
		md.WriteString(fmt.Sprintf("| %s | %d | %d | %.2f | %.2f | %.2f | %.4f | %.4f | %.4f |\n",
			contrast.Factor, contrast.LowLevel, contrast.HighLevel,
			contrast.DeltaOrdersPerSec, contrast.DeltaP99LatencyMs, contrast.DeltaLatencyArbitrageProfit,
			contrast.DeltaRetailSurplusPerUnit, contrast.DeltaRetailAdverseSelection, contrast.DeltaSurplusTransferGap))
	}
	md.WriteString("\n## Retail-Conditioned Arbitrage Effect\n\n")
	md.WriteString("Each row reports the average `(arb=3) - (arb=0)` delta at a fixed retail-intensity level, averaged over informed intensity and maker width.\n\n")
	md.WriteString("| Retail Level | Delta Orders/s | Delta p99 (ms) | Delta Arb Profit | Delta Retail Surplus | Delta Retail Adverse | Delta Welfare Gap |\n")
	md.WriteString("|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, effect := range summary.RetailConditionedArbitrage {
		md.WriteString(fmt.Sprintf("| %d | %.2f | %.2f | %.2f | %.4f | %.4f | %.4f |\n",
			effect.RetailIntensityMultiplier,
			effect.DeltaOrdersPerSec, effect.DeltaP99LatencyMs, effect.DeltaLatencyArbitrageProfit,
			effect.DeltaRetailSurplusPerUnit, effect.DeltaRetailAdverseSelection, effect.DeltaSurplusTransferGap))
	}

	var csv strings.Builder
	csv.WriteString("section,factor,level,level_high,cell_count,mean_orders_per_sec,mean_p99_latency_ms,mean_latency_arbitrage_profit,mean_retail_surplus_per_unit,mean_retail_adverse_selection_rate,mean_surplus_transfer_gap\n")
	for _, factor := range []string{"arbitrageur_intensity", "retail_intensity", "informed_intensity", "maker_quote_width"} {
		for _, level := range summary.MainEffects[factor] {
			csv.WriteString(fmt.Sprintf("main_effect,%s,%d,,%d,%.4f,%.4f,%.4f,%.6f,%.6f,%.6f\n",
				level.Factor, level.Level, level.CellCount,
				level.MeanOrdersPerSec, level.MeanP99LatencyMs, level.MeanLatencyArbitrageProfit,
				level.MeanRetailSurplusPerUnit, level.MeanRetailAdverseSelectionRate, level.MeanSurplusTransferGap))
		}
	}
	for _, contrast := range summary.HighLowContrasts {
		csv.WriteString(fmt.Sprintf("high_low_contrast,%s,%d,%d,,%.4f,%.4f,%.4f,%.6f,%.6f,%.6f\n",
			contrast.Factor, contrast.LowLevel, contrast.HighLevel,
			contrast.DeltaOrdersPerSec, contrast.DeltaP99LatencyMs, contrast.DeltaLatencyArbitrageProfit,
			contrast.DeltaRetailSurplusPerUnit, contrast.DeltaRetailAdverseSelection, contrast.DeltaSurplusTransferGap))
	}
	for _, effect := range summary.RetailConditionedArbitrage {
		csv.WriteString(fmt.Sprintf("retail_conditioned_arbitrage,retail_intensity,%d,3-0,,%.4f,%.4f,%.4f,%.6f,%.6f,%.6f\n",
			effect.RetailIntensityMultiplier,
			effect.DeltaOrdersPerSec, effect.DeltaP99LatencyMs, effect.DeltaLatencyArbitrageProfit,
			effect.DeltaRetailSurplusPerUnit, effect.DeltaRetailAdverseSelection, effect.DeltaSurplusTransferGap))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorHypercubeResponseSurfaceArtifacts(summary hypercubeResponseSurfaceSummary) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_parameter_hypercube_response_surface.json")
	mdPath := filepath.Join(base, "simulator_parameter_hypercube_response_surface.md")
	csvPath := filepath.Join(base, "simulator_parameter_hypercube_response_surface.csv")

	raw, err := json.MarshalIndent(summary, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Parameter Hypercube Response Surface\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", summary.Seeds))
	md.WriteString("Each fit uses a standardized response surface with main effects and pairwise interactions over `arb`, `retail`, `informed`, and `maker` factors.\n\n")
	for _, fit := range summary.Fits {
		md.WriteString(fmt.Sprintf("## %s\n\n", fit.Metric))
		md.WriteString(fmt.Sprintf("- `R^2`: %.4f\n", fit.R2))
		md.WriteString(fmt.Sprintf("- `RMSE`: %.4f\n\n", fit.RMSE))
		md.WriteString("| Coefficient | Value |\n|---|---:|\n")
		for _, coefficient := range fit.Coefficients {
			md.WriteString(fmt.Sprintf("| %s | %.4f |\n", coefficient.Name, coefficient.Coefficient))
		}
		md.WriteString("\n| Effect Group | Partial R^2 |\n|---|---:|\n")
		for _, effect := range fit.Effects {
			md.WriteString(fmt.Sprintf("| %s | %.4f |\n", effect.Factor, effect.PartialR2))
		}
		md.WriteString("\n")
	}

	var csv strings.Builder
	csv.WriteString("metric,section,name,value\n")
	for _, fit := range summary.Fits {
		csv.WriteString(fmt.Sprintf("%s,fit,r2,%.6f\n", fit.Metric, fit.R2))
		csv.WriteString(fmt.Sprintf("%s,fit,rmse,%.6f\n", fit.Metric, fit.RMSE))
		for _, coefficient := range fit.Coefficients {
			csv.WriteString(fmt.Sprintf("%s,coefficient,%s,%.6f\n", fit.Metric, coefficient.Name, coefficient.Coefficient))
		}
		for _, effect := range fit.Effects {
			csv.WriteString(fmt.Sprintf("%s,effect,%s,%.6f\n", fit.Metric, effect.Factor, effect.PartialR2))
		}
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func writeSimulatorHeldOutPolicyArtifacts(results []heldOutPolicyResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "simulator_heldout_policy_profile.json")
	mdPath := filepath.Join(base, "simulator_heldout_policy_profile.md")
	csvPath := filepath.Join(base, "simulator_heldout_policy_profile.csv")

	summary := summarizeHeldOutPolicyTable(results)
	payload := map[string]any{
		"seeds":   seeds,
		"results": results,
		"summary": summary,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	var md strings.Builder
	md.WriteString("# Simulator Held-Out Policy Profile\n\n")
	md.WriteString(fmt.Sprintf("Held-out seeds: `%v`\n\n", seeds))
	md.WriteString("The held-out regimes are excluded from the fitted-Q training seeds and evaluate generalization under unseen stress combinations.\n\n")
	md.WriteString("## Regime x Policy Results\n\n")
	md.WriteString("| Regime | Policy | Runs | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap | Neg. Bal. | Conservation |\n")
	md.WriteString("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f | %d | %d |\n",
			r.RegimeName, r.Policy, r.Runs,
			r.MeanOrdersPerSec, r.CI95OrdersPerSec,
			r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit,
			r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate,
			r.MeanSurplusTransferGap, r.CI95SurplusTransferGap,
			r.NegativeBalanceViolationsTotal, r.ConservationBreachesTotal))
	}
	md.WriteString("\n## Policy Summary\n\n")
	md.WriteString("| Policy | Regimes | Orders/s | Fills/s | p99 (ms) | Impact | Retail Surplus | Retail Adverse | Welfare Gap |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|---:|\n")
	for _, s := range summary {
		md.WriteString(fmt.Sprintf("| %s | %d | %.2f | %.2f | %.2f | %.2f | %.4f | %.4f | %.4f |\n",
			s.Policy, s.RegimeCount, s.MeanOrdersPerSec, s.MeanFillsPerSec, s.MeanP99LatencyMs,
			s.MeanAveragePriceImpact, s.MeanRetailSurplusPerUnit, s.MeanRetailAdverseSelectionRate, s.MeanSurplusTransferGap))
	}

	var csv strings.Builder
	csv.WriteString("section,regime,policy,runs,mean_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_average_price_impact,ci95_average_price_impact,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap,negative_balance_violations_total,conservation_breaches_total\n")
	for _, r := range results {
		csv.WriteString(fmt.Sprintf("regime_policy,%s,%s,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%d,%d\n",
			r.RegimeName, r.Policy, r.Runs,
			r.MeanOrdersPerSec, r.CI95OrdersPerSec,
			r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit,
			r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate,
			r.MeanSurplusTransferGap, r.CI95SurplusTransferGap,
			r.NegativeBalanceViolationsTotal, r.ConservationBreachesTotal))
	}
	for _, s := range summary {
		csv.WriteString(fmt.Sprintf("policy_summary,,%s,%d,%.4f,,%.4f,,%.4f,,%.4f,,%.6f,,%.6f,,%.6f,,,\n",
			s.Policy, s.RegimeCount,
			s.MeanOrdersPerSec, s.MeanFillsPerSec, s.MeanP99LatencyMs,
			s.MeanAveragePriceImpact, s.MeanRetailSurplusPerUnit,
			s.MeanRetailAdverseSelectionRate, s.MeanSurplusTransferGap))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func meanStd(values []float64) (float64, float64) {
	if len(values) == 0 {
		return 0, 0
	}
	var mean float64
	for _, value := range values {
		mean += value
	}
	mean /= float64(len(values))
	if len(values) == 1 {
		return mean, 0
	}
	var variance float64
	for _, value := range values {
		delta := value - mean
		variance += delta * delta
	}
	variance /= float64(len(values))
	return mean, math.Sqrt(variance)
}

func meanCI95(values []float64) (float64, float64) {
	mean, std := meanStd(values)
	return mean, ci95HalfWidth(std, len(values))
}

func ci95HalfWidth(std float64, n int) float64 {
	if n <= 1 {
		return 0
	}
	return 1.96 * std / math.Sqrt(float64(n))
}
