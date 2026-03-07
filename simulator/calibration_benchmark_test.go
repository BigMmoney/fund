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

type marketFactsProfile struct {
	ProfileName string              `json:"profile_name"`
	Summary     marketFactsSummary  `json:"summary"`
	Symbols     []marketSymbolFacts `json:"symbols"`
}

type marketFactsSummary struct {
	SymbolCount                 int        `json:"symbol_count"`
	TradeCountTotal             int        `json:"trade_count_total"`
	SpreadMeanRange             [2]float64 `json:"spread_mean_range"`
	SpreadMeanBpsRange          [2]float64 `json:"spread_mean_bps_range"`
	OrderSignLag1Range          [2]float64 `json:"order_sign_lag1_range"`
	InterArrivalMeanMsRange     [2]float64 `json:"inter_arrival_mean_ms_range"`
	VolatilityAbsLag1Range      [2]float64 `json:"volatility_abs_lag1_range"`
	TopBucketMeanImpactRange    [2]float64 `json:"top_bucket_mean_impact_range"`
	TopBucketMeanImpactBpsRange [2]float64 `json:"top_bucket_mean_impact_bps_range"`
}

type marketSymbolFacts struct {
	Symbol       string             `json:"symbol"`
	DepthProfile []marketDepthFact  `json:"depth_profile"`
	ImpactCurve  []marketImpactFact `json:"impact_curve"`
}

type marketDepthFact struct {
	Level         int     `json:"level"`
	MeanBidQty    float64 `json:"mean_bid_qty"`
	MeanAskQty    float64 `json:"mean_ask_qty"`
	BidShapeRatio float64 `json:"bid_shape_ratio"`
	AskShapeRatio float64 `json:"ask_shape_ratio"`
}

type marketImpactFact struct {
	Bucket              string  `json:"bucket"`
	MeanSignedImpact    float64 `json:"mean_signed_impact"`
	P90SignedImpact     float64 `json:"p90_signed_impact"`
	MeanSignedImpactBps float64 `json:"mean_signed_impact_bps"`
	P90SignedImpactBps  float64 `json:"p90_signed_impact_bps"`
}

type syntheticStylizedFacts struct {
	SpreadMeanBps          float64    `json:"spread_mean_bps"`
	OrderSignLag1          float64    `json:"order_sign_lag1"`
	InterArrivalMeanMs     float64    `json:"inter_arrival_mean_ms"`
	VolatilityAbsLag1      float64    `json:"volatility_abs_lag1"`
	TopBucketImpactMeanBps float64    `json:"top_bucket_impact_mean_bps"`
	BidShapeRatios         [5]float64 `json:"bid_shape_ratios"`
	AskShapeRatios         [5]float64 `json:"ask_shape_ratios"`
}

type calibrationMetricRow struct {
	Metric             string  `json:"metric"`
	MarketLow          float64 `json:"market_low"`
	MarketHigh         float64 `json:"market_high"`
	BaselineValue      float64 `json:"baseline_value"`
	CalibratedValue    float64 `json:"calibrated_value"`
	BaselineWithin     bool    `json:"baseline_within"`
	CalibratedWithin   bool    `json:"calibrated_within"`
	BaselineDistance   float64 `json:"baseline_distance_to_range"`
	CalibratedDistance float64 `json:"calibrated_distance_to_range"`
}

type calibrationComparisonArtifact struct {
	MarketProfile          string                 `json:"market_profile"`
	Seeds                  []int64                `json:"seeds"`
	BaselineScenarioName   string                 `json:"baseline_scenario_name"`
	CalibratedScenarioName string                 `json:"calibrated_scenario_name"`
	BaselineFacts          syntheticStylizedFacts `json:"baseline_facts"`
	CalibratedFacts        syntheticStylizedFacts `json:"calibrated_facts"`
	Rows                   []calibrationMetricRow `json:"rows"`
}

func TestGenerateSimulatorCalibrationArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_CALIBRATION_COMPARE") != "1" {
		t.Skip("set RUN_SIM_CALIBRATION_COMPARE=1 to generate calibration comparison artifacts")
	}

	market, err := loadMarketFactsProfile(filepath.Join("..", "docs", "benchmarks", "binance_spot_multimarket_facts.json"))
	if err != nil {
		t.Fatalf("load market facts: %v", err)
	}
	seeds := []int64{601, 607, 613, 617}
	baselineFacts := runSyntheticFactsAggregate(calibrationBaselineScenario(), seeds)
	calibratedFacts := runSyntheticFactsAggregate(calibratedMarketScenario(), seeds)
	rows := buildCalibrationRows(market, baselineFacts, calibratedFacts)
	if err := writeCalibrationTargetArtifacts(market, rows); err != nil {
		t.Fatalf("write target artifacts: %v", err)
	}
	if err := writeCalibrationComparisonArtifacts(calibrationComparisonArtifact{
		MarketProfile:          market.ProfileName,
		Seeds:                  seeds,
		BaselineScenarioName:   calibrationBaselineScenario().Name,
		CalibratedScenarioName: calibratedMarketScenario().Name,
		BaselineFacts:          baselineFacts,
		CalibratedFacts:        calibratedFacts,
		Rows:                   rows,
	}); err != nil {
		t.Fatalf("write comparison artifacts: %v", err)
	}
}

func TestGenerateSimulatorCalibratedBenchmarkArtifacts(t *testing.T) {
	if os.Getenv("RUN_SIM_CALIBRATED_BENCH") != "1" {
		t.Skip("set RUN_SIM_CALIBRATED_BENCH=1 to generate calibrated benchmark artifacts")
	}
	seeds := []int64{701, 709, 719, 727}
	scenarios := calibratedBenchmarkScenarios()
	results := make([]aggregateResult, 0, len(scenarios))
	for _, base := range scenarios {
		runs := make([]BenchmarkResult, 0, len(seeds))
		for _, seed := range seeds {
			cfg := base
			cfg.Seed = seed
			runs = append(runs, runScenario(cfg))
		}
		results = append(results, summarizeRuns(base, runs))
	}
	if err := writeCalibratedBenchmarkArtifacts(results, seeds); err != nil {
		t.Fatalf("write calibrated benchmark artifacts: %v", err)
	}
}

func calibrationBaselineScenario() ScenarioConfig {
	return ScenarioConfig{
		Name:         "Calibration-Baseline",
		Mode:         ModeImmediate,
		StepDuration: 1000 * time.Millisecond,
		TotalSteps:   720,
		Agents:       StrategicPopulation(),
		Risk:         RiskConfig{MaxOrderAmount: 12, MaxOrdersPerStep: 64},
		Fundamentals: FundamentalConfig{Base: 50, Floor: 20, Ceiling: 80, RegimeLength: 20, DriftMagnitude: 1, ShockMin: -1, ShockMax: 1},
	}
}

func calibratedMarketScenario() ScenarioConfig {
	return ScenarioConfig{
		Name:         "Calibration-Calibrated",
		Mode:         ModeImmediate,
		StepDuration: 1000 * time.Millisecond,
		TotalSteps:   720,
		Agents:       CalibratedPopulation(),
		Risk:         RiskConfig{MaxOrderAmount: 20, MaxOrdersPerStep: 96},
		Fundamentals: FundamentalConfig{Base: 10000, Floor: 9800, Ceiling: 10200, RegimeLength: 40, DriftMagnitude: 2, ShockMin: -1, ShockMax: 1, ShockPersistence: 0.30},
	}
}

func calibratedBenchmarkScenarios() []ScenarioConfig {
	base := calibratedMarketScenario()
	return []ScenarioConfig{
		{
			Name:         "Calibrated-Immediate-Surrogate",
			Mode:         ModeImmediate,
			StepDuration: base.StepDuration,
			TotalSteps:   480,
			Agents:       CalibratedPopulation(),
			Risk:         RiskConfig{MaxOrderAmount: 20, MaxOrdersPerStep: 96},
			Fundamentals: base.Fundamentals,
		},
		{
			Name:             "Calibrated-FBA-2s",
			Mode:             ModeBatch,
			BatchWindowSteps: 2,
			StepDuration:     base.StepDuration,
			TotalSteps:       480,
			Agents:           CalibratedPopulation(),
			Risk:             RiskConfig{MaxOrderAmount: 20, MaxOrdersPerStep: 96},
			Fundamentals:     base.Fundamentals,
		},
		{
			Name:                   "Calibrated-Adaptive-1-3s",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			AdaptiveMinWindowSteps: 1,
			AdaptiveMaxWindowSteps: 3,
			AdaptiveOrderThreshold: 8,
			AdaptiveQueueThreshold: 10,
			StepDuration:           base.StepDuration,
			TotalSteps:             480,
			Agents:                 CalibratedPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 20, MaxOrdersPerStep: 96},
			Fundamentals:           base.Fundamentals,
		},
		{
			Name:                   "Calibrated-Policy-LearnedOfflineContextual-1-3s",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			PolicyController:       PolicyLearnedOfflineContextual,
			AdaptiveMinWindowSteps: 1,
			AdaptiveMaxWindowSteps: 3,
			AdaptiveOrderThreshold: 8,
			AdaptiveQueueThreshold: 10,
			StepDuration:           base.StepDuration,
			TotalSteps:             480,
			Agents:                 CalibratedPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 20, MaxOrdersPerStep: 96},
			Fundamentals:           base.Fundamentals,
		},
		{
			Name:                   "Calibrated-Policy-LearnedFittedQ-1-3s",
			Mode:                   ModeAdaptiveBatch,
			AdaptivePolicy:         AdaptiveBalanced,
			PolicyController:       PolicyLearnedFittedQ,
			AdaptiveMinWindowSteps: 1,
			AdaptiveMaxWindowSteps: 3,
			AdaptiveOrderThreshold: 8,
			AdaptiveQueueThreshold: 10,
			StepDuration:           base.StepDuration,
			TotalSteps:             480,
			Agents:                 CalibratedPopulation(),
			Risk:                   RiskConfig{MaxOrderAmount: 20, MaxOrdersPerStep: 96},
			Fundamentals:           base.Fundamentals,
		},
	}
}

func loadMarketFactsProfile(path string) (marketFactsProfile, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return marketFactsProfile{}, err
	}
	var profile marketFactsProfile
	if err := json.Unmarshal(raw, &profile); err != nil {
		return marketFactsProfile{}, err
	}
	return profile, nil
}

func runSyntheticFactsAggregate(base ScenarioConfig, seeds []int64) syntheticStylizedFacts {
	runs := make([]syntheticStylizedFacts, 0, len(seeds))
	for _, seed := range seeds {
		cfg := base
		cfg.Seed = seed
		env := NewEnvironment(cfg)
		env.Run()
		runs = append(runs, summarizeSyntheticFacts(env))
	}
	return aggregateSyntheticFacts(runs)
}

func summarizeSyntheticFacts(env *Environment) syntheticStylizedFacts {
	spreadBps := make([]float64, 0, len(env.traceSnapshots))
	midPrices := make([]float64, 0, len(env.traceSnapshots))
	var bidShape [5][]float64
	var askShape [5][]float64
	for _, snap := range env.traceSnapshots {
		if snap.MidPrice > 0 && snap.Spread > 0 {
			spreadBps = append(spreadBps, (snap.Spread/snap.MidPrice)*10000.0)
		}
		if snap.MidPrice > 0 {
			midPrices = append(midPrices, snap.MidPrice)
		}
		bidBase := snap.BidQty[0]
		askBase := snap.AskQty[0]
		for idx := 0; idx < 5; idx++ {
			if bidBase > 0 {
				bidShape[idx] = append(bidShape[idx], snap.BidQty[idx]/bidBase)
			}
			if askBase > 0 {
				askShape[idx] = append(askShape[idx], snap.AskQty[idx]/askBase)
			}
		}
	}
	signs := make([]float64, 0, len(env.fills))
	interArrival := make([]float64, 0, maxInt(0, len(env.fills)-1))
	fillCountByStep := make(map[int]int)
	for _, fill := range env.fills {
		fillCountByStep[fill.FillStep]++
	}
	stepOffsets := make(map[int]int)
	fillEvents := make([]acceptedOrderEvent, 0, len(env.fills))
	stepMs := float64(env.cfg.StepDuration.Milliseconds())
	if stepMs <= 0 {
		stepMs = 1
	}
	for _, fill := range env.fills {
		sign := -1.0
		if fill.BuyerArrival >= fill.SellerArrival {
			sign = 1.0
		}
		slotCount := fillCountByStep[fill.FillStep]
		stepOffsets[fill.FillStep]++
		slot := float64(stepOffsets[fill.FillStep]) * (stepMs / float64(slotCount+1))
		fillEvents = append(fillEvents, acceptedOrderEvent{
			Step:           fill.FillStep,
			EventTimeMs:    float64(fill.FillStep)*stepMs + slot,
			Amount:         float64(fill.Amount),
			ReferencePrice: float64(fill.Price),
			Side:           map[bool]Side{true: Buy, false: Sell}[sign > 0],
		})
		signs = append(signs, sign)
	}
	lastTime := 0.0
	hasLast := false
	for _, event := range fillEvents {
		if hasLast {
			interArrival = append(interArrival, event.EventTimeMs-lastTime)
		}
		lastTime = event.EventTimeMs
		hasLast = true
	}
	returns := make([]float64, 0, maxInt(0, len(midPrices)-1))
	for idx := 1; idx < len(midPrices); idx++ {
		prev := midPrices[idx-1]
		if prev <= 0 {
			continue
		}
		returns = append(returns, (midPrices[idx]-prev)/prev)
	}
	absReturns := make([]float64, 0, len(returns))
	for _, value := range returns {
		absReturns = append(absReturns, math.Abs(value))
	}
	amounts := make([]float64, 0, len(fillEvents))
	for _, event := range fillEvents {
		amounts = append(amounts, event.Amount)
	}
	q25 := percentileGo(amounts, 0.25)
	q50 := percentileGo(amounts, 0.50)
	q75 := percentileGo(amounts, 0.75)
	impactQ4 := make([]float64, 0)
	horizon := 10
	for _, event := range fillEvents {
		targetStep := event.Step + horizon
		if targetStep >= len(env.traceSnapshots) || event.ReferencePrice <= 0 {
			continue
		}
		futureMid := env.traceSnapshots[targetStep].MidPrice
		if futureMid <= 0 {
			continue
		}
		sign := 1.0
		if event.Side == Sell {
			sign = -1.0
		}
		impactBps := sign * ((futureMid - event.ReferencePrice) / event.ReferencePrice) * 10000.0
		switch {
		case event.Amount <= q25:
		case event.Amount <= q50:
		case event.Amount <= q75:
		default:
			impactQ4 = append(impactQ4, impactBps)
		}
	}
	summary := syntheticStylizedFacts{
		SpreadMeanBps:          meanSafe(spreadBps),
		OrderSignLag1:          autocorrGo(signs, 1),
		InterArrivalMeanMs:     meanSafe(interArrival),
		VolatilityAbsLag1:      autocorrGo(absReturns, 1),
		TopBucketImpactMeanBps: meanSafe(impactQ4),
	}
	for idx := 0; idx < 5; idx++ {
		summary.BidShapeRatios[idx] = meanSafe(bidShape[idx])
		summary.AskShapeRatios[idx] = meanSafe(askShape[idx])
	}
	return summary
}

func aggregateSyntheticFacts(runs []syntheticStylizedFacts) syntheticStylizedFacts {
	out := syntheticStylizedFacts{}
	if len(runs) == 0 {
		return out
	}
	spread := make([]float64, 0, len(runs))
	sign := make([]float64, 0, len(runs))
	arrival := make([]float64, 0, len(runs))
	vol := make([]float64, 0, len(runs))
	impact := make([]float64, 0, len(runs))
	var bidLevels [5][]float64
	var askLevels [5][]float64
	for _, run := range runs {
		spread = append(spread, run.SpreadMeanBps)
		sign = append(sign, run.OrderSignLag1)
		arrival = append(arrival, run.InterArrivalMeanMs)
		vol = append(vol, run.VolatilityAbsLag1)
		impact = append(impact, run.TopBucketImpactMeanBps)
		for idx := 0; idx < 5; idx++ {
			bidLevels[idx] = append(bidLevels[idx], run.BidShapeRatios[idx])
			askLevels[idx] = append(askLevels[idx], run.AskShapeRatios[idx])
		}
	}
	out.SpreadMeanBps = meanSafe(spread)
	out.OrderSignLag1 = meanSafe(sign)
	out.InterArrivalMeanMs = meanSafe(arrival)
	out.VolatilityAbsLag1 = meanSafe(vol)
	out.TopBucketImpactMeanBps = meanSafe(impact)
	for idx := 0; idx < 5; idx++ {
		out.BidShapeRatios[idx] = meanSafe(bidLevels[idx])
		out.AskShapeRatios[idx] = meanSafe(askLevels[idx])
	}
	return out
}

func buildCalibrationRows(market marketFactsProfile, baseline, calibrated syntheticStylizedFacts) []calibrationMetricRow {
	rows := []calibrationMetricRow{
		makeCalibrationRow("spread_mean_bps", market.Summary.SpreadMeanBpsRange[0], market.Summary.SpreadMeanBpsRange[1], baseline.SpreadMeanBps, calibrated.SpreadMeanBps),
		makeCalibrationRow("order_sign_lag1", market.Summary.OrderSignLag1Range[0], market.Summary.OrderSignLag1Range[1], baseline.OrderSignLag1, calibrated.OrderSignLag1),
		makeCalibrationRow("inter_arrival_mean_ms", market.Summary.InterArrivalMeanMsRange[0], market.Summary.InterArrivalMeanMsRange[1], baseline.InterArrivalMeanMs, calibrated.InterArrivalMeanMs),
		makeCalibrationRow("volatility_abs_lag1", market.Summary.VolatilityAbsLag1Range[0], market.Summary.VolatilityAbsLag1Range[1], baseline.VolatilityAbsLag1, calibrated.VolatilityAbsLag1),
		makeCalibrationRow("impact_q4_mean_bps", market.Summary.TopBucketMeanImpactBpsRange[0], market.Summary.TopBucketMeanImpactBpsRange[1], baseline.TopBucketImpactMeanBps, calibrated.TopBucketImpactMeanBps),
	}
	for level := 2; level <= 5; level++ {
		bidLow, bidHigh := marketDepthShapeRange(market.Symbols, level, true)
		askLow, askHigh := marketDepthShapeRange(market.Symbols, level, false)
		rows = append(rows,
			makeCalibrationRow(fmt.Sprintf("bid_shape_L%d", level), bidLow, bidHigh, baseline.BidShapeRatios[level-1], calibrated.BidShapeRatios[level-1]),
			makeCalibrationRow(fmt.Sprintf("ask_shape_L%d", level), askLow, askHigh, baseline.AskShapeRatios[level-1], calibrated.AskShapeRatios[level-1]),
		)
	}
	return rows
}

func marketDepthShapeRange(symbols []marketSymbolFacts, level int, bid bool) (float64, float64) {
	values := make([]float64, 0, len(symbols))
	for _, symbol := range symbols {
		for _, depth := range symbol.DepthProfile {
			if depth.Level != level {
				continue
			}
			if bid {
				values = append(values, depth.BidShapeRatio)
			} else {
				values = append(values, depth.AskShapeRatio)
			}
		}
	}
	if len(values) == 0 {
		return 0, 0
	}
	sort.Float64s(values)
	return values[0], values[len(values)-1]
}

func makeCalibrationRow(metric string, low, high, baseline, calibrated float64) calibrationMetricRow {
	return calibrationMetricRow{
		Metric:             metric,
		MarketLow:          low,
		MarketHigh:         high,
		BaselineValue:      baseline,
		CalibratedValue:    calibrated,
		BaselineWithin:     withinRange(baseline, low, high),
		CalibratedWithin:   withinRange(calibrated, low, high),
		BaselineDistance:   distanceToRange(baseline, low, high),
		CalibratedDistance: distanceToRange(calibrated, low, high),
	}
}

func withinRange(value, low, high float64) bool {
	return value >= low && value <= high
}

func distanceToRange(value, low, high float64) float64 {
	if withinRange(value, low, high) {
		return 0
	}
	if value < low {
		return low - value
	}
	return value - high
}

func writeCalibrationTargetArtifacts(market marketFactsProfile, rows []calibrationMetricRow) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	payload := map[string]any{
		"market_profile": market.ProfileName,
		"market_summary": market.Summary,
		"rows":           rows,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(base, "simulator_calibration_target_table.json"), append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Calibration Target Table\n\n")
	md.WriteString(fmt.Sprintf("Market profile: `%s`\n\n", market.ProfileName))
	md.WriteString("| Metric | Market Low | Market High | Baseline | Calibrated | Baseline In Range | Calibrated In Range |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|\n")
	for _, row := range rows {
		md.WriteString(fmt.Sprintf("| %s | %.6f | %.6f | %.6f | %.6f | %t | %t |\n",
			row.Metric, row.MarketLow, row.MarketHigh, row.BaselineValue, row.CalibratedValue, row.BaselineWithin, row.CalibratedWithin))
	}
	return os.WriteFile(filepath.Join(base, "simulator_calibration_target_table.md"), []byte(md.String()), 0o644)
}

func writeCalibrationComparisonArtifacts(artifact calibrationComparisonArtifact) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	raw, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(base, "simulator_calibrated_vs_market.json"), append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Calibrated vs Market Comparison\n\n")
	md.WriteString(fmt.Sprintf("Market profile: `%s`\n\n", artifact.MarketProfile))
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", artifact.Seeds))
	md.WriteString(fmt.Sprintf("Baseline scenario: `%s`\n\n", artifact.BaselineScenarioName))
	md.WriteString(fmt.Sprintf("Calibrated scenario: `%s`\n\n", artifact.CalibratedScenarioName))
	md.WriteString("| Metric | Market Range | Baseline | Calibrated | Baseline Dist. | Calibrated Dist. |\n")
	md.WriteString("|---|---|---:|---:|---:|---:|\n")
	for _, row := range artifact.Rows {
		md.WriteString(fmt.Sprintf("| %s | %.6f -> %.6f | %.6f | %.6f | %.6f | %.6f |\n",
			row.Metric, row.MarketLow, row.MarketHigh, row.BaselineValue, row.CalibratedValue, row.BaselineDistance, row.CalibratedDistance))
	}
	return os.WriteFile(filepath.Join(base, "simulator_calibrated_vs_market.md"), []byte(md.String()), 0o644)
}

func writeCalibratedBenchmarkArtifacts(results []aggregateResult, seeds []int64) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}
	payload := map[string]any{
		"seeds":   seeds,
		"results": results,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	jsonPath := filepath.Join(base, "simulator_calibrated_benchmark_profile.json")
	mdPath := filepath.Join(base, "simulator_calibrated_benchmark_profile.md")
	csvPath := filepath.Join(base, "simulator_calibrated_benchmark_profile.csv")
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}
	var md strings.Builder
	md.WriteString("# Simulator Calibrated Benchmark Profile\n\n")
	md.WriteString(fmt.Sprintf("Seeds: `%v`\n\n", seeds))
	md.WriteString("| Scenario | Orders/s | Fills/s | p99 (ms) | Retail Surplus | Retail Adverse | Welfare Gap | Safety |\n")
	md.WriteString("|---|---:|---:|---:|---:|---:|---:|---|\n")
	var csv strings.Builder
	csv.WriteString("name,mean_orders_per_sec,ci95_orders_per_sec,mean_fills_per_sec,ci95_fills_per_sec,mean_p99_latency_ms,ci95_p99_latency_ms,mean_retail_surplus_per_unit,ci95_retail_surplus_per_unit,mean_retail_adverse_selection_rate,ci95_retail_adverse_selection_rate,mean_surplus_transfer_gap,ci95_surplus_transfer_gap\n")
	for _, r := range results {
		safety := "zero-breach"
		if r.NegativeBalanceViolationsTotal > 0 || r.ConservationBreachesTotal > 0 {
			safety = "breach"
		}
		md.WriteString(fmt.Sprintf("| %s | %.2f +/- %.2f | %.2f +/- %.2f | %.2f +/- %.2f | %.4f +/- %.4f | %.4f +/- %.4f | %.4f +/- %.4f | %s |\n",
			r.Name, r.MeanOrdersPerSec, r.CI95OrdersPerSec, r.MeanFillsPerSec, r.CI95FillsPerSec,
			r.MeanP99LatencyMs, r.CI95P99LatencyMs, r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit,
			r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate, r.MeanSurplusTransferGap, r.CI95SurplusTransferGap, safety))
		csv.WriteString(fmt.Sprintf("%s,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f,%.6f\n",
			r.Name, r.MeanOrdersPerSec, r.CI95OrdersPerSec, r.MeanFillsPerSec, r.CI95FillsPerSec, r.MeanP99LatencyMs, r.CI95P99LatencyMs,
			r.MeanRetailSurplusPerUnit, r.CI95RetailSurplusPerUnit, r.MeanRetailAdverseSelectionRate, r.CI95RetailAdverseSelectionRate, r.MeanSurplusTransferGap, r.CI95SurplusTransferGap))
	}
	if err := os.WriteFile(mdPath, []byte(md.String()), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv.String()), 0o644)
}

func percentileGo(values []float64, q float64) float64 {
	if len(values) == 0 {
		return 0
	}
	ordered := append([]float64(nil), values...)
	sort.Float64s(ordered)
	idx := int(math.Round(float64(len(ordered)-1) * q))
	if idx < 0 {
		idx = 0
	}
	if idx >= len(ordered) {
		idx = len(ordered) - 1
	}
	return ordered[idx]
}

func autocorrGo(values []float64, lag int) float64 {
	if len(values) <= lag || lag <= 0 {
		return 0
	}
	left := values[:len(values)-lag]
	right := values[lag:]
	meanLeft := meanSafe(left)
	meanRight := meanSafe(right)
	num := 0.0
	denLeft := 0.0
	denRight := 0.0
	for idx := range left {
		lv := left[idx] - meanLeft
		rv := right[idx] - meanRight
		num += lv * rv
		denLeft += lv * lv
		denRight += rv * rv
	}
	if denLeft <= 0 || denRight <= 0 {
		return 0
	}
	return num / math.Sqrt(denLeft*denRight)
}

func meanSafe(values []float64) float64 {
	if len(values) == 0 {
		return 0
	}
	sum := 0.0
	for _, value := range values {
		sum += value
	}
	return sum / float64(len(values))
}
