package simulator

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

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
