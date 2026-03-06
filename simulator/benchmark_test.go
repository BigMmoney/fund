package simulator

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"path/filepath"
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
	NegativeBalanceViolationsTotal int          `json:"negative_balance_violations_total"`
	ConservationBreachesTotal      int          `json:"conservation_breaches_total"`
	RiskRejectionsTotal            int          `json:"risk_rejections_total"`
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
			Name:            "SpeedBump-50ms",
			Mode:            ModeSpeedBump,
			SpeedBumpSteps:  5,
			StepDuration:    10 * time.Millisecond,
			TotalSteps:      120,
			Seed:            42,
			Agents:          DefaultPopulation(),
			Risk:            RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 24},
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
			Name:                   "Policy-LearnedLinear-100-250ms",
			Mode:                   ModeAdaptiveBatch,
			PolicyController:       PolicyLearnedLinear,
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
			Name:                    "Ablation-Control",
			Mode:                    ModeBatch,
			BatchWindowSteps:        25,
			StepDuration:            10 * time.Millisecond,
			TotalSteps:              125,
			Seed:                    77,
			Agents:                  StressPopulation(),
			Risk:                    RiskConfig{MaxOrderAmount: 5, MaxOrdersPerStep: 12},
		},
		{
			Name:                    "Ablation-RelaxedRisk",
			Mode:                    ModeBatch,
			BatchWindowSteps:        25,
			DisableRiskLimits:       true,
			StepDuration:            10 * time.Millisecond,
			TotalSteps:              125,
			Seed:                    77,
			Agents:                  StressPopulation(),
			Risk:                    RiskConfig{MaxOrderAmount: 5, MaxOrdersPerStep: 12},
		},
		{
			Name:                    "Ablation-RandomTieBreak",
			Mode:                    ModeBatch,
			BatchWindowSteps:        25,
			RandomizeBatchTieBreak:  true,
			StepDuration:            10 * time.Millisecond,
			TotalSteps:              125,
			Seed:                    77,
			Agents:                  StressPopulation(),
			Risk:                    RiskConfig{MaxOrderAmount: 5, MaxOrdersPerStep: 12},
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
			Name:             "AgentAblation-RetailBurst",
			Mode:             ModeBatch,
			BatchWindowSteps: 25,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       125,
			Seed:             151,
			Agents:           RetailBurstPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 8, MaxOrdersPerStep: 28},
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
	result := runScenario(scenarioByName(t, "Policy-LearnedLinear-100-250ms"))
	if result.Name != "Policy-LearnedLinear-100-250ms" {
		t.Fatalf("expected learned-policy result name, got %+v", result)
	}
	if result.AdaptiveWindowMeanMs <= 0 {
		t.Fatalf("expected learned policy baseline to record adaptive window stats, got %+v", result)
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
	if noArb.MeanLatencyArbitrageProfit >= control.MeanLatencyArbitrageProfit {
		t.Fatalf("expected no-arbitrageur ablation to reduce arb profit, control=%+v noArb=%+v", control, noArb)
	}
	if err := writeSimulatorAgentAblationArtifacts(aggregates, seeds); err != nil {
		t.Fatalf("write agent/workload ablation artifacts: %v", err)
	}
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
	md.WriteString("| Scenario | Mode | Window (ms) | Speed Bump (ms) | Orders/s | Fills/s | p50 (ms) | p95 (ms) | Spread | Price Impact | Queue Advantage | Arb Profit | Dispersion | Risk Rejects |\n")
	md.WriteString("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("scenario,mode,batch_window_ms,speed_bump_ms,orders_per_sec,fills_per_sec,p50_latency_ms,p95_latency_ms,p99_latency_ms,average_spread,average_price_impact,queue_priority_advantage,latency_arbitrage_profit,execution_dispersion,risk_rejections,negative_balance_violations,conservation_breaches\n")

	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %d | %.2f | %.2f | %.2f | %.2f | %.2f | %.2f | %.4f | %.2f | %.4f | %d |\n",
			r.Name, r.Mode, r.BatchWindowMs, r.SpeedBumpMs, r.OrdersPerSec, r.FillsPerSec, r.P50LatencyMs, r.P95LatencyMs,
			r.AverageSpread, r.AveragePriceImpact, r.QueuePriorityAdvantage, r.LatencyArbitrageProfit, r.ExecutionDispersion, r.RiskRejections))
		csv.WriteString(fmt.Sprintf("%s,%s,%d,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.4f,%.6f,%d,%d,%d\n",
			r.Name, r.Mode, r.BatchWindowMs, r.SpeedBumpMs, r.OrdersPerSec, r.FillsPerSec, r.P50LatencyMs, r.P95LatencyMs,
			r.P99LatencyMs, r.AverageSpread, r.AveragePriceImpact, r.QueuePriorityAdvantage, r.LatencyArbitrageProfit,
			r.ExecutionDispersion, r.RiskRejections, r.NegativeBalanceViolations, r.ConservationBreaches))
	}

	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func summarizeRuns(base ScenarioConfig, runs []BenchmarkResult) aggregateResult {
	agg := aggregateResult{
		Name:          base.Name,
		Mode:          base.Mode,
		BatchWindowMs: int(base.StepDuration.Milliseconds()) * base.BatchWindowSteps,
		SpeedBumpMs:   int(base.StepDuration.Milliseconds()) * base.SpeedBumpSteps,
		AdaptiveWindowMinMs: int(base.StepDuration.Milliseconds()) * base.AdaptiveMinWindowSteps,
		AdaptiveWindowMaxMs: int(base.StepDuration.Milliseconds()) * base.AdaptiveMaxWindowSteps,
		Runs:          len(runs),
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
	md.WriteString("| Scenario | Runs | Window (ms) | Speed Bump (ms) | Adaptive Mean (ms) | Orders/s (mean +/- CI95) | Fills/s (mean +/- CI95) | p50 (mean +/- CI95) | p95 (mean +/- CI95) | p99 (mean +/- CI95) | Spread (mean +/- CI95) | Impact (mean +/- CI95) | Queue Adv. (mean +/- CI95) | Arb Profit (mean +/- CI95) |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("scenario,mode,batch_window_ms,speed_bump_ms,adaptive_window_min_ms,adaptive_window_max_ms,adaptive_window_mean_ms,runs,mean_orders_per_sec,std_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,std_fills_per_sec,ci95_fills_per_sec,mean_p50_latency_ms,std_p50_latency_ms,ci95_p50_latency_ms,mean_p95_latency_ms,std_p95_latency_ms,ci95_p95_latency_ms,mean_p99_latency_ms,std_p99_latency_ms,ci95_p99_latency_ms,mean_average_spread,ci95_average_spread,mean_average_price_impact,ci95_average_price_impact,mean_queue_priority_advantage,ci95_queue_priority_advantage,mean_latency_arbitrage_profit,ci95_latency_arbitrage_profit,mean_execution_dispersion,ci95_execution_dispersion,negative_balance_violations_total,conservation_breaches_total,risk_rejections_total\n")

	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %s | %d | %d | %d | %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.2f +/- %.2f |\n",
			r.Name, r.Runs, r.BatchWindowMs, r.SpeedBumpMs, r.AdaptiveWindowMeanMs, r.MeanOrdersPerSec, r.CI95OrdersPerSec, r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP50LatencyMs, r.CI95P50LatencyMs, r.MeanP95LatencyMs, r.CI95P95LatencyMs, r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanAverageSpread, r.CI95AverageSpread, r.MeanAveragePriceImpact, r.CI95AveragePriceImpact,
			r.MeanQueuePriorityAdvantage, r.CI95QueuePriorityAdvantage, r.MeanLatencyArbitrageProfit, r.CI95LatencyArbitrageProfit))
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
