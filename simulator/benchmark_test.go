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
	Name                           string  `json:"name"`
	Mode                           MatchingMode `json:"mode"`
	BatchWindowMs                  int     `json:"batch_window_ms"`
	Runs                           int     `json:"runs"`
	MeanOrdersPerSec               float64 `json:"mean_orders_per_sec"`
	StdOrdersPerSec                float64 `json:"std_orders_per_sec"`
	MeanFillsPerSec                float64 `json:"mean_fills_per_sec"`
	StdFillsPerSec                 float64 `json:"std_fills_per_sec"`
	MeanP50LatencyMs               float64 `json:"mean_p50_latency_ms"`
	StdP50LatencyMs                float64 `json:"std_p50_latency_ms"`
	MeanP95LatencyMs               float64 `json:"mean_p95_latency_ms"`
	StdP95LatencyMs                float64 `json:"std_p95_latency_ms"`
	MeanP99LatencyMs               float64 `json:"mean_p99_latency_ms"`
	StdP99LatencyMs                float64 `json:"std_p99_latency_ms"`
	MeanAverageSpread              float64 `json:"mean_average_spread"`
	MeanAveragePriceImpact         float64 `json:"mean_average_price_impact"`
	MeanQueuePriorityAdvantage     float64 `json:"mean_queue_priority_advantage"`
	MeanLatencyArbitrageProfit     float64 `json:"mean_latency_arbitrage_profit"`
	MeanExecutionDispersion        float64 `json:"mean_execution_dispersion"`
	NegativeBalanceViolationsTotal int     `json:"negative_balance_violations_total"`
	ConservationBreachesTotal      int     `json:"conservation_breaches_total"`
	RiskRejectionsTotal            int     `json:"risk_rejections_total"`
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

func TestSimulatorDeterminism(t *testing.T) {
	cfg := simulatorScenarios()[1]
	left := NewEnvironment(cfg).Run()
	right := NewEnvironment(cfg).Run()

	if left.Fills != right.Fills ||
		left.OrdersAccepted != right.OrdersAccepted ||
		left.QueuePriorityAdvantage != right.QueuePriorityAdvantage ||
		left.LatencyArbitrageProfit != right.LatencyArbitrageProfit {
		t.Fatalf("expected deterministic benchmark outputs, left=%+v right=%+v", left, right)
	}
}

func TestSimulatorSettlementSafety(t *testing.T) {
	cfg := simulatorScenarios()[4]
	result := NewEnvironment(cfg).Run()
	if result.NegativeBalanceViolations != 0 {
		t.Fatalf("expected no negative balances, got %d", result.NegativeBalanceViolations)
	}
	if result.ConservationBreaches != 0 {
		t.Fatalf("expected no conservation breaches, got %d", result.ConservationBreaches)
	}
}

func TestGenerateSimulatorBenchmarkArtifacts(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SIM_BENCH") != "1" {
		t.Skip("set RUN_SIM_BENCH=1 to generate simulator benchmark artifacts")
	}

	results := make([]BenchmarkResult, 0, len(simulatorScenarios()))
	for _, cfg := range simulatorScenarios() {
		results = append(results, NewEnvironment(cfg).Run())
	}

	immediate := results[0]
	batch100 := results[1]
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
			runs = append(runs, NewEnvironment(cfg).Run())
		}
		aggregates = append(aggregates, summarizeRuns(base, runs))
	}

	immediate := aggregates[0]
	batch500 := aggregates[3]
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
	md.WriteString("| Scenario | Mode | Window (ms) | Orders/s | Fills/s | p50 (ms) | p95 (ms) | Spread | Price Impact | Queue Advantage | Arb Profit | Dispersion | Risk Rejects |\n")
	md.WriteString("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("scenario,mode,batch_window_ms,orders_per_sec,fills_per_sec,p50_latency_ms,p95_latency_ms,p99_latency_ms,average_spread,average_price_impact,queue_priority_advantage,latency_arbitrage_profit,execution_dispersion,risk_rejections,negative_balance_violations,conservation_breaches\n")

	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %s | %s | %d | %.2f | %.2f | %.2f | %.2f | %.2f | %.2f | %.4f | %.2f | %.4f | %d |\n",
			r.Name, r.Mode, r.BatchWindowMs, r.OrdersPerSec, r.FillsPerSec, r.P50LatencyMs, r.P95LatencyMs,
			r.AverageSpread, r.AveragePriceImpact, r.QueuePriorityAdvantage, r.LatencyArbitrageProfit, r.ExecutionDispersion, r.RiskRejections))
		csv.WriteString(fmt.Sprintf("%s,%s,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.4f,%.6f,%d,%d,%d\n",
			r.Name, r.Mode, r.BatchWindowMs, r.OrdersPerSec, r.FillsPerSec, r.P50LatencyMs, r.P95LatencyMs,
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
		agg.NegativeBalanceViolationsTotal += run.NegativeBalanceViolations
		agg.ConservationBreachesTotal += run.ConservationBreaches
		agg.RiskRejectionsTotal += run.RiskRejections
	}
	agg.MeanOrdersPerSec, agg.StdOrdersPerSec = meanStd(orders)
	agg.MeanFillsPerSec, agg.StdFillsPerSec = meanStd(fills)
	agg.MeanP50LatencyMs, agg.StdP50LatencyMs = meanStd(p50)
	agg.MeanP95LatencyMs, agg.StdP95LatencyMs = meanStd(p95)
	agg.MeanP99LatencyMs, agg.StdP99LatencyMs = meanStd(p99)
	agg.MeanAverageSpread, _ = meanStd(spread)
	agg.MeanAveragePriceImpact, _ = meanStd(impact)
	agg.MeanQueuePriorityAdvantage, _ = meanStd(queue)
	agg.MeanLatencyArbitrageProfit, _ = meanStd(arb)
	agg.MeanExecutionDispersion, _ = meanStd(dispersion)
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
	md.WriteString("| Scenario | Runs | Window (ms) | Mean Orders/s | Std Orders/s | Mean Fills/s | Mean p50 (ms) | Mean p95 (ms) | Mean p99 (ms) | Mean Spread | Mean Impact | Mean Queue Adv. | Mean Arb Profit | Mean Dispersion |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n")

	var csv strings.Builder
	csv.WriteString("scenario,mode,batch_window_ms,runs,mean_orders_per_sec,std_orders_per_sec,mean_fills_per_sec,std_fills_per_sec,mean_p50_latency_ms,std_p50_latency_ms,mean_p95_latency_ms,std_p95_latency_ms,mean_p99_latency_ms,std_p99_latency_ms,mean_average_spread,mean_average_price_impact,mean_queue_priority_advantage,mean_latency_arbitrage_profit,mean_execution_dispersion,negative_balance_violations_total,conservation_breaches_total,risk_rejections_total\n")

	for _, r := range results {
		md.WriteString(fmt.Sprintf("| %s | %d | %d | %.2f | %.2f | %.2f | %.2f | %.2f | %.2f | %.2f | %.2f | %.4f | %.2f | %.4f |\n",
			r.Name, r.Runs, r.BatchWindowMs, r.MeanOrdersPerSec, r.StdOrdersPerSec, r.MeanFillsPerSec,
			r.MeanP50LatencyMs, r.MeanP95LatencyMs, r.MeanP99LatencyMs, r.MeanAverageSpread,
			r.MeanAveragePriceImpact, r.MeanQueuePriorityAdvantage, r.MeanLatencyArbitrageProfit, r.MeanExecutionDispersion))
		csv.WriteString(fmt.Sprintf("%s,%s,%d,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.6f,%.4f,%.6f,%d,%d,%d\n",
			r.Name, r.Mode, r.BatchWindowMs, r.Runs, r.MeanOrdersPerSec, r.StdOrdersPerSec, r.MeanFillsPerSec, r.StdFillsPerSec,
			r.MeanP50LatencyMs, r.StdP50LatencyMs, r.MeanP95LatencyMs, r.StdP95LatencyMs, r.MeanP99LatencyMs, r.StdP99LatencyMs,
			r.MeanAverageSpread, r.MeanAveragePriceImpact, r.MeanQueuePriorityAdvantage, r.MeanLatencyArbitrageProfit,
			r.MeanExecutionDispersion, r.NegativeBalanceViolationsTotal, r.ConservationBreachesTotal, r.RiskRejectionsTotal))
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
