package simulator

import (
	"testing"
	"time"
)

// TestHotSpotScenarios tests tail latency, queuing, and stability under high hot spots
func TestHotSpotScenarios(t *testing.T) {
	t.Run("SingleHotSpotExtreme", func(t *testing.T) {
		testSingleHotSpotExtreme(t)
	})
	t.Run("HighCancellationMarketMaking", func(t *testing.T) {
		testHighCancellationMarketMaking(t)
	})
	t.Run("HotSpotSoakTest", func(t *testing.T) {
		testHotSpotSoakTest(t)
	})
	t.Run("RecoveryBackpressure", func(t *testing.T) {
		testRecoveryBackpressure(t)
	})
}

func testSingleHotSpotExtreme(t *testing.T) {
	cfg := ScenarioConfig{
		Name:             "HotSpot-SingleExtreme",
		Mode:             ModeBatch,
		BatchWindowSteps: 15, // 150ms
		StepDuration:     10 * time.Millisecond,
		TotalSteps:       400,
		Seed:             1001,
		Agents: []AgentConfig{
			{ID: "mm-hot-1", Class: AgentMarketMaker, LatencyTier: 2,
			 BaseSize: 12, QuoteWidth: 2, Intensity: 6,
			 InitialCash: 2000000, InitialUnits: 50000},
			{ID: "mm-hot-2", Class: AgentMarketMaker, LatencyTier: 2,
			 BaseSize: 8, QuoteWidth: 3, Intensity: 4,
			 InitialCash: 2000000, InitialUnits: 50000},
			{ID: "arb-hot-1", Class: AgentArbitrageur, LatencyTier: 1,
			 BaseSize: 6, QuoteWidth: 1, Intensity: 4,
			 InitialCash: 1500000, InitialUnits: 20000},
			{ID: "ret-1", Class: AgentRetail, LatencyTier: 3,
			 BaseSize: 2, QuoteWidth: 4, Intensity: 1,
			 InitialCash: 300000, InitialUnits: 3000},
		},
		Risk: RiskConfig{MaxOrderAmount: 20, MaxOrdersPerStep: 80},
	}

	result := runScenario(cfg)

	// Validate results
	if result.P99LatencyMs < 10 {
		t.Errorf("Expected high P99 latency, got %.2f ms", result.P99LatencyMs)
	}
	if result.OrdersPerSec < 100 {
		t.Errorf("Expected high throughput, got %.2f orders/sec", result.OrdersPerSec)
	}
	t.Logf("Single Hot Spot Extreme - P99: %.2fms, Orders/sec: %.2f, Queue Priority: %.4f",
		result.P99LatencyMs, result.OrdersPerSec, result.QueuePriorityAdvantage)
}

func testHighCancellationMarketMaking(t *testing.T) {
	cfg := ScenarioConfig{
		Name:             "HotSpot-HighCancellation",
		Mode:             ModeBatch,
		BatchWindowSteps: 5, // 50ms
		StepDuration:     10 * time.Millisecond,
		TotalSteps:       250,
		Seed:             1002,
		Agents: []AgentConfig{
			{ID: "mm-cancel-1", Class: AgentMarketMaker, LatencyTier: 2,
			 BaseSize: 6, QuoteWidth: 6, Intensity: 5,
			 InitialCash: 1500000, InitialUnits: 30000},
			{ID: "mm-cancel-2", Class: AgentMarketMaker, LatencyTier: 2,
			 BaseSize: 6, QuoteWidth: 7, Intensity: 5,
			 InitialCash: 1500000, InitialUnits: 30000},
			{ID: "arb-scale-1", Class: AgentArbitrageur, LatencyTier: 1,
			 BaseSize: 6, QuoteWidth: 1, Intensity: 4,
			 InitialCash: 1200000, InitialUnits: 15000},
			{ID: "arb-scale-2", Class: AgentArbitrageur, LatencyTier: 1,
			 BaseSize: 6, QuoteWidth: 1, Intensity: 3,
			 InitialCash: 1200000, InitialUnits: 15000},
			{ID: "ret-2x-1", Class: AgentRetail, LatencyTier: 3,
			 BaseSize: 2, QuoteWidth: 4, Intensity: 4,
			 InitialCash: 400000, InitialUnits: 5000},
			{ID: "ret-2x-2", Class: AgentRetail, LatencyTier: 3,
			 BaseSize: 2, QuoteWidth: 5, Intensity: 4,
			 InitialCash: 400000, InitialUnits: 5000},
		},
		Risk: RiskConfig{MaxOrderAmount: 15, MaxOrdersPerStep: 90},
	}

	result := runScenario(cfg)

	// Validate results
	if result.RetailAdverseSelectionRate < 0.01 {
		t.Errorf("Expected higher adverse selection, got %.4f", result.RetailAdverseSelectionRate)
	}
	t.Logf("High Cancellation MM - Adverse Selection: %.4f, Queue Priority: %.4f, Execution Dispersion: %.4f",
		result.RetailAdverseSelectionRate, result.QueuePriorityAdvantage, result.ExecutionDispersion)
}

func testHotSpotSoakTest(t *testing.T) {
	seeds := []int64{1003, 1004, 1005}
	results := make([]BenchmarkResult, len(seeds))

	for i, seed := range seeds {
		cfg := ScenarioConfig{
			Name:             "HotSpot-SoakTest",
			Mode:             ModeAdaptiveBatch,
			AdaptivePolicy:   AdaptiveBalanced,
			AdaptiveMinWindowSteps: 10, // 100ms
			AdaptiveMaxWindowSteps: 50, // 500ms
			AdaptiveOrderThreshold: 15,
			AdaptiveQueueThreshold: 20,
			StepDuration:     10 * time.Millisecond,
			TotalSteps:       600, // 6000ms
			Seed:             seed,
			Agents: []AgentConfig{
				{ID: "mm-soak-1", Class: AgentMarketMaker, LatencyTier: 2,
				 BaseSize: 6, QuoteWidth: 2, Intensity: 5,
				 InitialCash: 2000000, InitialUnits: 100000},
				{ID: "mm-soak-2", Class: AgentMarketMaker, LatencyTier: 2,
				 BaseSize: 5, QuoteWidth: 3, Intensity: 4,
				 InitialCash: 2000000, InitialUnits: 80000},
				{ID: "arb-soak-1", Class: AgentArbitrageur, LatencyTier: 1,
				 BaseSize: 4, QuoteWidth: 1, Intensity: 3,
				 InitialCash: 1500000, InitialUnits: 20000},
				{ID: "ret-soak-1", Class: AgentRetail, LatencyTier: 3,
				 BaseSize: 2, QuoteWidth: 4, Intensity: 2,
				 InitialCash: 500000, InitialUnits: 10000},
			},
			Risk: RiskConfig{MaxOrderAmount: 12, MaxOrdersPerStep: 60},
		}
		results[i] = runScenario(cfg)
	}

	// Validate stability
	for i, result := range results {
		if result.ConservationBreaches > 0 {
			t.Errorf("Conservation breach in seed %d", seeds[i])
		}
		if result.NegativeBalanceViolations > 0 {
			t.Errorf("Negative balance violation in seed %d", seeds[i])
		}
		t.Logf("Soak Test Seed %d - P99: %.2fms, Orders/sec: %.2f, Adaptive Mean Window: %.2fms",
			seeds[i], result.P99LatencyMs, result.OrdersPerSec, result.AdaptiveWindowMeanMs)
	}
}

func testRecoveryBackpressure(t *testing.T) {
	// First run normal scenario to establish baseline
	cfg := ScenarioConfig{
		Name:             "Recovery-Backpressure",
		Mode:             ModeBatch,
		BatchWindowSteps: 10, // 100ms
		StepDuration:     10 * time.Millisecond,
		TotalSteps:       300,
		Seed:             1006,
		Agents: []AgentConfig{
			{ID: "mm-recovery-1", Class: AgentMarketMaker, LatencyTier: 2,
			 BaseSize: 8, QuoteWidth: 3, Intensity: 4,
			 InitialCash: 2000000, InitialUnits: 50000},
			{ID: "arb-recovery-1", Class: AgentArbitrageur, LatencyTier: 1,
			 BaseSize: 5, QuoteWidth: 1, Intensity: 3,
			 InitialCash: 1500000, InitialUnits: 20000},
			{ID: "ret-recovery-1", Class: AgentRetail, LatencyTier: 3,
			 BaseSize: 3, QuoteWidth: 4, Intensity: 2,
			 InitialCash: 400000, InitialUnits: 5000},
		},
		Risk: RiskConfig{MaxOrderAmount: 15, MaxOrdersPerStep: 70},
	}

	result := runScenario(cfg)

	// Validate recovery under backpressure
	if result.P99LatencyMs < 5 {
		t.Errorf("Expected some latency under backpressure, got %.2f ms", result.P99LatencyMs)
	}
	t.Logf("Recovery Backpressure - P99: %.2fms, Orders/sec: %.2f, Risk Rejections: %d",
		result.P99LatencyMs, result.OrdersPerSec, result.RiskRejections)
}