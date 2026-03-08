package simulator

import (
	"math"
	"math/rand"
	"time"

	"pre_trading/benchmark"
)

type Environment struct {
	cfg                    ScenarioConfig
	rng                    *rand.Rand
	fundamentals           []int64
	agents                 map[string]AgentConfig
	accounts               map[string]AccountState
	initialCash            int64
	initialUnits           int64
	buys                   []Order
	sells                  []Order
	pending                []pendingOrder
	seq                    int64
	metricAcc              *metricAccumulator
	ordersSubmitted        int
	ordersAccepted         int
	fills                  []Fill
	latencies              []time.Duration
	riskRejections         int
	negViolations          int
	conservationHits       int
	maxActiveStep          int
	currentStep            int
	done                   bool
	currentBatchWindow     int
	batchCycleSteps        int
	lastStepAccepted       int
	adaptiveWindowHistory  []int
	runtimeRiskScale       float64
	runtimeRandomTieBreak  bool
	runtimeReleaseCadence  int
	runtimePriceAggression int64
	traceAcceptedOrders    []acceptedOrderEvent
	traceSnapshots         []bookSnapshot
}

type acceptedOrderEvent struct {
	Step           int
	EventTimeMs    float64
	Side           Side
	Amount         float64
	ReferencePrice float64
}

type bookSnapshot struct {
	Step     int
	MidPrice float64
	Spread   float64
	BidQty   [5]float64
	AskQty   [5]float64
}

type pendingOrder struct {
	ReleaseStep int
	Order       Order
}

func NewEnvironment(cfg ScenarioConfig) *Environment {
	e := &Environment{cfg: cfg}
	e.Reset()
	return e
}

func (e *Environment) Reset() Observation {
	agents := make(map[string]AgentConfig, len(e.cfg.Agents))
	accounts := make(map[string]AccountState, len(e.cfg.Agents))
	var totalCash, totalUnits int64
	for _, agent := range e.cfg.Agents {
		agents[agent.ID] = agent
		accounts[agent.ID] = AccountState{Cash: agent.InitialCash, Units: agent.InitialUnits}
		totalCash += agent.InitialCash
		totalUnits += agent.InitialUnits
	}

	e.rng = rand.New(rand.NewSource(e.cfg.Seed))
	e.fundamentals = generateFundamentals(e.cfg.TotalSteps+1, e.cfg.Seed, e.cfg.Fundamentals)
	e.agents = agents
	e.accounts = accounts
	e.initialCash = totalCash
	e.initialUnits = totalUnits
	e.buys = nil
	e.sells = nil
	e.pending = nil
	e.seq = 0
	e.metricAcc = newMetricAccumulator()
	e.ordersSubmitted = 0
	e.ordersAccepted = 0
	e.fills = nil
	e.latencies = make([]time.Duration, 0, maxInt(1, e.cfg.TotalSteps)*4)
	e.riskRejections = 0
	e.negViolations = 0
	e.conservationHits = 0
	e.maxActiveStep = -1
	e.currentStep = 0
	e.done = false
	e.batchCycleSteps = 0
	e.lastStepAccepted = 0
	e.adaptiveWindowHistory = nil
	e.currentBatchWindow = e.initialBatchWindow()
	e.runtimeRiskScale = 1.0
	e.runtimeRandomTieBreak = e.cfg.RandomizeBatchTieBreak
	e.runtimeReleaseCadence = 0
	e.runtimePriceAggression = 0
	e.traceAcceptedOrders = nil
	e.traceSnapshots = nil
	return e.Observe()
}

func (e *Environment) Observe() Observation {
	step := e.currentStep
	if step >= len(e.fundamentals) {
		step = len(e.fundamentals) - 1
	}
	if step < 0 {
		step = 0
	}
	return Observation{
		Step:                   e.currentStep,
		Done:                   e.done,
		Mode:                   e.cfg.Mode,
		CurrentBatchWindowStep: e.currentBatchWindow,
		SpeedBumpSteps:         e.cfg.SpeedBumpSteps,
		CurrentReleaseCadence:  e.runtimeReleaseCadence,
		CurrentPriceAggression: e.runtimePriceAggression,
		PendingOrders:          len(e.pending),
		BuyDepth:               len(e.buys),
		SellDepth:              len(e.sells),
		Spread:                 currentSpread(e.buys, e.sells),
		Fundamental:            e.fundamentals[step],
		OrdersSubmitted:        e.ordersSubmitted,
		OrdersAccepted:         e.ordersAccepted,
		Fills:                  len(e.fills),
		RiskRejections:         e.riskRejections,
	}
}

func (e *Environment) Metrics() MetricsSnapshot {
	return MetricsSnapshot{
		OrdersSubmitted:            e.ordersSubmitted,
		OrdersAccepted:             e.ordersAccepted,
		Fills:                      len(e.fills),
		AverageSpread:              e.metricAcc.averageSpread(),
		AveragePriceImpact:         e.metricAcc.averagePriceImpact(),
		QueuePriorityAdvantage:     e.metricAcc.queuePriorityAdvantage(),
		LatencyArbitrageProfit:     e.metricAcc.ArbProfit,
		ExecutionDispersion:        e.metricAcc.executionDispersion(),
		RetailSurplusPerUnit:       e.metricAcc.retailSurplusPerUnit(),
		ArbitrageurSurplusPerUnit:  e.metricAcc.arbitrageurSurplusPerUnit(),
		RetailAdverseSelectionRate: e.metricAcc.retailAdverseSelectionRate(),
		WelfareDispersion:          e.metricAcc.welfareDispersion(),
		SurplusTransferGap:         e.metricAcc.surplusTransferGap(),
		NegativeBalanceViolations:  e.negViolations,
		ConservationBreaches:       e.conservationHits,
		RiskRejections:             e.riskRejections,
	}
}

func (e *Environment) Step() StepResult {
	if e.done {
		return StepResult{Observation: e.Observe(), Metrics: e.Metrics()}
	}

	if e.currentStep < e.cfg.TotalSteps {
		e.runStep(e.currentStep)
		e.currentStep++
		if e.cfg.Mode == ModeBatch && e.cfg.BatchWindowSteps > 0 && e.batchCycleSteps >= e.effectiveFlushWindow(e.cfg.BatchWindowSteps) {
			e.flushBatch(e.currentStep - 1)
			e.batchCycleSteps = 0
		}
		if e.cfg.Mode == ModeAdaptiveBatch && e.currentBatchWindow > 0 && e.batchCycleSteps >= e.effectiveFlushWindow(e.currentBatchWindow) {
			windowUsed := e.currentBatchWindow
			e.flushBatch(e.currentStep - 1)
			e.batchCycleSteps = 0
			e.adaptiveWindowHistory = append(e.adaptiveWindowHistory, windowUsed)
			e.currentBatchWindow = e.nextAdaptiveWindow()
		}
	} else {
		e.flushResidualWork()
	}

	if e.currentStep > 0 {
		e.recordSnapshot(e.currentStep - 1)
	}

	e.done = e.currentStep >= e.cfg.TotalSteps && !e.hasResidualWork()
	return StepResult{Observation: e.Observe(), Metrics: e.Metrics()}
}

func (e *Environment) Run() BenchmarkResult {
	start := time.Now()
	e.Reset()
	for !e.done {
		e.Step()
	}

	return e.benchmarkResult(time.Since(start))
}

func (e *Environment) benchmarkResult(elapsed time.Duration) BenchmarkResult {
	stats := benchmark.ComputeLatencyStats(e.latencies)
	activeSteps := e.maxActiveStep + 1
	if activeSteps <= 0 {
		activeSteps = e.cfg.TotalSteps
	}
	minAdaptiveMs, maxAdaptiveMs, meanAdaptiveMs := e.adaptiveWindowStats()
	return BenchmarkResult{
		Name:                       e.cfg.Name,
		Mode:                       e.cfg.Mode,
		BatchWindowMs:              int(e.cfg.StepDuration.Milliseconds()) * e.cfg.BatchWindowSteps,
		SpeedBumpMs:                int(e.cfg.StepDuration.Milliseconds()) * e.cfg.SpeedBumpSteps,
		AdaptiveWindowMinMs:        minAdaptiveMs,
		AdaptiveWindowMaxMs:        maxAdaptiveMs,
		AdaptiveWindowMeanMs:       meanAdaptiveMs,
		Seed:                       e.cfg.Seed,
		OrdersSubmitted:            e.ordersSubmitted,
		OrdersAccepted:             e.ordersAccepted,
		Fills:                      len(e.fills),
		OrdersPerSec:               benchmark.ComputeThroughput(e.ordersAccepted, time.Duration(activeSteps)*e.cfg.StepDuration),
		FillsPerSec:                benchmark.ComputeThroughput(len(e.fills), time.Duration(activeSteps)*e.cfg.StepDuration),
		P50LatencyMs:               stats.P50Ms,
		P95LatencyMs:               stats.P95Ms,
		P99LatencyMs:               stats.P99Ms,
		AverageSpread:              e.metricAcc.averageSpread(),
		AveragePriceImpact:         e.metricAcc.averagePriceImpact(),
		QueuePriorityAdvantage:     e.metricAcc.queuePriorityAdvantage(),
		LatencyArbitrageProfit:     e.metricAcc.ArbProfit,
		ExecutionDispersion:        e.metricAcc.executionDispersion(),
		RetailSurplusPerUnit:       e.metricAcc.retailSurplusPerUnit(),
		ArbitrageurSurplusPerUnit:  e.metricAcc.arbitrageurSurplusPerUnit(),
		RetailAdverseSelectionRate: e.metricAcc.retailAdverseSelectionRate(),
		WelfareDispersion:          e.metricAcc.welfareDispersion(),
		SurplusTransferGap:         e.metricAcc.surplusTransferGap(),
		NegativeBalanceViolations:  e.negViolations,
		ConservationBreaches:       e.conservationHits,
		RiskRejections:             e.riskRejections,
		Elapsed:                    elapsed,
	}
}

func (e *Environment) runStep(step int) {
	if step > e.maxActiveStep {
		e.maxActiveStep = step
	}
	if e.cfg.Mode == ModeSpeedBump {
		e.releasePending(step)
	}

	fundamental := e.fundamentals[step]
	perStepCount := 0
	acceptedThisStep := 0
	generated := make([]Order, 0, len(e.cfg.Agents)*2)
	for _, agent := range e.cfg.Agents {
		orders := generateOrdersForAgent(agent, step, e.fundamentals, e.rng, &e.seq, e.availableAccount(agent.ID), e.runtimePriceAggression)
		for _, order := range orders {
			e.ordersSubmitted++
			e.metricAcc.addSubmitted(order.Class, order.Amount)
			maxOrderAmount, maxOrdersPerStep := e.effectiveRiskLimits()
			if !e.cfg.DisableRiskLimits && (order.Amount > maxOrderAmount || perStepCount >= maxOrdersPerStep) {
				e.riskRejections++
				continue
			}
			generated = append(generated, order)
			e.ordersAccepted++
			perStepCount++
			acceptedThisStep++
		}
	}
	e.lastStepAccepted = acceptedThisStep

	sortOrdersForArrival(generated, e.agents)
	for idx, order := range generated {
		e.recordAcceptedOrder(order, step, idx, len(generated), fundamental)
		switch e.cfg.Mode {
		case ModeImmediate:
			fills := processImmediateBook(&e.buys, &e.sells, order, step, fundamental)
			e.applyFills(fills)
		case ModeBatch, ModeAdaptiveBatch:
			if order.Side == Buy {
				e.buys = append(e.buys, order)
				sortBuyBook(&e.buys)
			} else {
				e.sells = append(e.sells, order)
				sortSellBook(&e.sells)
			}
		case ModeSpeedBump:
			e.pending = append(e.pending, pendingOrder{
				ReleaseStep: step + e.effectiveReleaseCadence(maxInt(1, e.cfg.SpeedBumpSteps)),
				Order:       order,
			})
		}
	}

	e.batchCycleSteps++
	if spread := currentSpread(e.buys, e.sells); spread > 0 {
		e.metricAcc.addSpread(spread)
	}
}

func (e *Environment) availableAccount(agentID string) AccountState {
	acct := e.accounts[agentID]
	var reservedCash int64
	var reservedUnits int64
	for _, order := range e.buys {
		if order.AgentID == agentID {
			reservedCash += order.Price * order.Amount
		}
	}
	for _, order := range e.sells {
		if order.AgentID == agentID {
			reservedUnits += order.Amount
		}
	}
	for _, pending := range e.pending {
		if pending.Order.AgentID != agentID {
			continue
		}
		if pending.Order.Side == Buy {
			reservedCash += pending.Order.Price * pending.Order.Amount
		} else {
			reservedUnits += pending.Order.Amount
		}
	}
	acct.Cash -= reservedCash
	acct.Units -= reservedUnits
	if acct.Cash < 0 {
		acct.Cash = 0
	}
	if acct.Units < 0 {
		acct.Units = 0
	}
	return acct
}

func (e *Environment) flushBatch(step int) {
	if step > e.maxActiveStep {
		e.maxActiveStep = step
	}
	fills := processBatchBookWithOptions(&e.buys, &e.sells, step, e.fundamentals[minInt(step, len(e.fundamentals)-1)], e.runtimeRandomTieBreak, e.rng)
	e.applyFills(fills)
}

func (e *Environment) flushResidualWork() {
	step := e.currentStep
	if e.cfg.Mode == ModeSpeedBump {
		e.releasePending(step)
		if len(e.pending) > 0 {
			e.currentStep++
			return
		}
	}
	if (e.cfg.Mode == ModeBatch || e.cfg.Mode == ModeAdaptiveBatch) && (len(e.buys) > 0 || len(e.sells) > 0) {
		if e.cfg.Mode == ModeAdaptiveBatch && e.batchCycleSteps > 0 {
			e.adaptiveWindowHistory = append(e.adaptiveWindowHistory, e.currentBatchWindow)
		}
		e.flushBatch(step)
		if len(e.buys) > 0 || len(e.sells) > 0 {
			e.buys = nil
			e.sells = nil
		}
		e.batchCycleSteps = 0
		e.currentStep++
	}
}

func (e *Environment) hasResidualWork() bool {
	switch e.cfg.Mode {
	case ModeSpeedBump:
		return len(e.pending) > 0
	case ModeBatch, ModeAdaptiveBatch:
		return len(e.buys) > 0 || len(e.sells) > 0
	default:
		return false
	}
}

func (e *Environment) releasePending(step int) {
	if step > e.maxActiveStep {
		e.maxActiveStep = step
	}
	if len(e.pending) == 0 {
		return
	}

	ready := make([]Order, 0, len(e.pending))
	remaining := e.pending[:0]
	for _, candidate := range e.pending {
		if candidate.ReleaseStep <= step {
			ready = append(ready, candidate.Order)
			continue
		}
		remaining = append(remaining, candidate)
	}
	e.pending = remaining
	if len(ready) == 0 {
		return
	}

	sortOrdersForArrival(ready, e.agents)
	fundamental := e.fundamentals[minInt(step, len(e.fundamentals)-1)]
	for _, order := range ready {
		fills := processImmediateBook(&e.buys, &e.sells, order, step, fundamental)
		e.applyFills(fills)
	}
	if spread := currentSpread(e.buys, e.sells); spread > 0 {
		e.metricAcc.addSpread(spread)
	}
}

func (e *Environment) applyFills(fills []Fill) {
	for _, fill := range fills {
		if fill.BuyerID == "" || fill.SellerID == "" || fill.BuyerID == fill.SellerID {
			continue
		}
		if !e.cfg.DisableSettlementApplication {
			buyer := e.accounts[fill.BuyerID]
			seller := e.accounts[fill.SellerID]
			notional := fill.Price * fill.Amount

			buyer.Cash -= notional
			buyer.Units += fill.Amount
			seller.Cash += notional
			seller.Units -= fill.Amount
			e.accounts[fill.BuyerID] = buyer
			e.accounts[fill.SellerID] = seller

			if !e.cfg.DisableSettlementChecks {
				if buyer.Cash < 0 || buyer.Units < 0 || seller.Cash < 0 || seller.Units < 0 {
					e.negViolations++
				}
				if !e.checkConservation() {
					e.conservationHits++
				}
			}
		}

		e.metricAcc.addFill(fill)
		e.fills = append(e.fills, fill)

		maxArrival := fill.BuyerArrival
		if fill.SellerArrival > maxArrival {
			maxArrival = fill.SellerArrival
		}
		if fill.FillStep >= maxArrival {
			steps := fill.FillStep - maxArrival + 1
			e.latencies = append(e.latencies, time.Duration(steps)*e.cfg.StepDuration)
		}
	}
}

func (e *Environment) checkConservation() bool {
	var totalCash, totalUnits int64
	for _, acct := range e.accounts {
		totalCash += acct.Cash
		totalUnits += acct.Units
	}
	return totalCash == e.initialCash && totalUnits == e.initialUnits
}

func (e *Environment) initialBatchWindow() int {
	switch e.cfg.Mode {
	case ModeBatch:
		return maxInt(1, e.cfg.BatchWindowSteps)
	case ModeAdaptiveBatch:
		minWindow := maxInt(1, e.cfg.AdaptiveMinWindowSteps)
		maxWindow := maxInt(minWindow, e.cfg.AdaptiveMaxWindowSteps)
		if minWindow > maxWindow {
			minWindow = maxWindow
		}
		return minWindow
	default:
		return 1
	}
}

func (e *Environment) nextAdaptiveWindow() int {
	if e.cfg.Mode != ModeAdaptiveBatch {
		return e.currentBatchWindow
	}

	minWindow := maxInt(1, e.cfg.AdaptiveMinWindowSteps)
	maxWindow := maxInt(minWindow, e.cfg.AdaptiveMaxWindowSteps)
	orderThreshold := e.cfg.AdaptiveOrderThreshold
	if orderThreshold <= 0 {
		orderThreshold = maxInt(4, e.cfg.Risk.MaxOrdersPerStep/2)
	}
	queueThreshold := e.cfg.AdaptiveQueueThreshold
	if queueThreshold <= 0 {
		queueThreshold = maxInt(8, orderThreshold)
	}

	next := e.currentBatchWindow
	queueDepth := len(e.buys) + len(e.sells)
	imbalance := absInt(len(e.buys) - len(e.sells))
	crossDepth := minInt(len(e.buys), len(e.sells))
	stepUp := 5
	stepDown := 5
	policy := e.cfg.AdaptivePolicy
	if policy == "" {
		policy = AdaptiveBalanced
	}

	switch policy {
	case AdaptiveOrderFlow:
		stepUp = 10
		stepDown = 5
		switch {
		case e.lastStepAccepted >= orderThreshold:
			next = minInt(maxWindow, e.currentBatchWindow+stepUp)
		case e.lastStepAccepted <= maxInt(1, orderThreshold/2):
			next = maxInt(minWindow, e.currentBatchWindow-stepDown)
		}
	case AdaptiveQueueLoad:
		stepUp = 8
		stepDown = 10
		switch {
		case queueDepth >= queueThreshold && crossDepth <= maxInt(2, queueThreshold/3):
			next = minInt(maxWindow, e.currentBatchWindow+stepUp)
		case imbalance >= maxInt(3, queueThreshold/2):
			next = minInt(maxWindow, e.currentBatchWindow+stepUp)
		case queueDepth <= maxInt(2, queueThreshold/2) && imbalance <= 1:
			next = maxInt(minWindow, e.currentBatchWindow-stepDown)
		}
	default:
		switch {
		case e.lastStepAccepted >= orderThreshold || queueDepth >= queueThreshold:
			next = minInt(maxWindow, e.currentBatchWindow+stepUp)
		case e.lastStepAccepted <= maxInt(1, orderThreshold/2) && queueDepth <= maxInt(2, queueThreshold/2):
			next = maxInt(minWindow, e.currentBatchWindow-stepDown)
		}
	}
	if next < minWindow {
		next = minWindow
	}
	if next > maxWindow {
		next = maxWindow
	}
	return next
}

func (e *Environment) adaptiveWindowStats() (int, int, float64) {
	if len(e.adaptiveWindowHistory) == 0 {
		return 0, 0, 0
	}
	minSteps := e.adaptiveWindowHistory[0]
	maxSteps := e.adaptiveWindowHistory[0]
	var sum int
	for _, steps := range e.adaptiveWindowHistory {
		if steps < minSteps {
			minSteps = steps
		}
		if steps > maxSteps {
			maxSteps = steps
		}
		sum += steps
	}
	stepMs := float64(e.cfg.StepDuration.Milliseconds())
	return int(float64(minSteps) * stepMs), int(float64(maxSteps) * stepMs), (float64(sum) / float64(len(e.adaptiveWindowHistory))) * stepMs
}

func (e *Environment) effectiveReleaseCadence(base int) int {
	cadence := base
	if e.runtimeReleaseCadence > cadence {
		cadence = e.runtimeReleaseCadence
	}
	if cadence < 1 {
		cadence = 1
	}
	return cadence
}

func (e *Environment) effectiveFlushWindow(base int) int {
	return e.effectiveReleaseCadence(base)
}

func (e *Environment) effectiveRiskLimits() (int64, int) {
	if e.cfg.DisableRiskLimits {
		return e.cfg.Risk.MaxOrderAmount, e.cfg.Risk.MaxOrdersPerStep
	}
	scale := e.runtimeRiskScale
	if scale <= 0 {
		scale = 1.0
	}
	maxOrderAmount := int64(math.Round(float64(e.cfg.Risk.MaxOrderAmount) * scale))
	if maxOrderAmount < 1 {
		maxOrderAmount = 1
	}
	maxOrdersPerStep := int(math.Round(float64(e.cfg.Risk.MaxOrdersPerStep) * scale))
	if maxOrdersPerStep < 1 {
		maxOrdersPerStep = 1
	}
	return maxOrderAmount, maxOrdersPerStep
}

func generateFundamentals(steps int, seed int64, cfg FundamentalConfig) []int64 {
	rng := rand.New(rand.NewSource(seed))
	values := make([]int64, steps)
	if cfg.Base == 0 && cfg.Floor == 0 && cfg.Ceiling == 0 && cfg.RegimeLength == 0 && cfg.DriftMagnitude == 0 && cfg.ShockMin == 0 && cfg.ShockMax == 0 && cfg.ShockPersistence == 0 {
		cfg = FundamentalConfig{
			Base:           50,
			Floor:          20,
			Ceiling:        80,
			RegimeLength:   20,
			DriftMagnitude: 1,
			ShockMin:       -1,
			ShockMax:       1,
		}
	}
	base := cfg.Base
	if base == 0 {
		base = 50
	}
	floor := cfg.Floor
	if floor == 0 {
		floor = 20
	}
	ceiling := cfg.Ceiling
	if ceiling == 0 {
		ceiling = 80
	}
	regimeLength := cfg.RegimeLength
	if regimeLength <= 0 {
		regimeLength = 20
	}
	driftMagnitude := cfg.DriftMagnitude
	if driftMagnitude <= 0 {
		driftMagnitude = 1
	}
	shockMin := cfg.ShockMin
	shockMax := cfg.ShockMax
	if shockMin == 0 && shockMax == 0 {
		shockMin = -1
		shockMax = 1
	}
	if shockMin > shockMax {
		shockMin, shockMax = shockMax, shockMin
	}
	persistence := cfg.ShockPersistence
	if persistence < 0 {
		persistence = 0
	}
	if persistence > 0.95 {
		persistence = 0.95
	}
	values[0] = base
	prevShock := int64(0)
	for i := 1; i < steps; i++ {
		drift := driftMagnitude
		if (i/regimeLength)%2 == 1 {
			drift = -driftMagnitude
		}
		span := shockMax - shockMin + 1
		shock := int64(0)
		if span > 0 {
			shock = shockMin + int64(rng.Intn(int(span)))
		}
		if i > 1 && persistence > 0 && rng.Float64() < persistence {
			shock = prevShock + signInt64(prevShock)*int64(rng.Intn(2))
			if shock < shockMin {
				shock = shockMin
			}
			if shock > shockMax {
				shock = shockMax
			}
		}
		prevShock = shock
		values[i] = clampInt64(values[i-1]+drift+shock, floor, ceiling)
	}
	return values
}

func currentSpread(buys, sells []Order) int64 {
	if len(buys) == 0 || len(sells) == 0 {
		return 0
	}
	bestBid := buys[0].Price
	for _, buy := range buys {
		if buy.Price > bestBid {
			bestBid = buy.Price
		}
	}
	bestAsk := sells[0].Price
	for _, sell := range sells {
		if sell.Price < bestAsk {
			bestAsk = sell.Price
		}
	}
	if bestAsk <= bestBid {
		return 1
	}
	return bestAsk - bestBid
}

func currentMidPrice(buys, sells []Order, fundamental int64) float64 {
	if len(buys) == 0 || len(sells) == 0 {
		return float64(fundamental)
	}
	bestBid := buys[0].Price
	for _, buy := range buys {
		if buy.Price > bestBid {
			bestBid = buy.Price
		}
	}
	bestAsk := sells[0].Price
	for _, sell := range sells {
		if sell.Price < bestAsk {
			bestAsk = sell.Price
		}
	}
	return float64(bestBid+bestAsk) / 2.0
}

func topLevelQty(orders []Order, levels int) [5]float64 {
	var out [5]float64
	if levels <= 0 || len(orders) == 0 {
		return out
	}
	copyOrders := append([]Order(nil), orders...)
	if len(copyOrders) > 1 {
		if copyOrders[0].Side == Buy {
			sortBuyBook(&copyOrders)
		} else {
			sortSellBook(&copyOrders)
		}
	}
	levelIdx := -1
	var lastPrice int64
	for idx, order := range copyOrders {
		if idx == 0 || order.Price != lastPrice {
			levelIdx++
			if levelIdx >= levels || levelIdx >= len(out) {
				break
			}
			lastPrice = order.Price
		}
		out[levelIdx] += float64(order.Amount)
	}
	return out
}

func (e *Environment) recordAcceptedOrder(order Order, step, index, count int, fundamental int64) {
	if count <= 0 {
		count = 1
	}
	stepMs := float64(e.cfg.StepDuration.Milliseconds())
	if stepMs <= 0 {
		stepMs = 1
	}
	slotMs := stepMs / float64(count+1)
	e.traceAcceptedOrders = append(e.traceAcceptedOrders, acceptedOrderEvent{
		Step:           step,
		EventTimeMs:    float64(step)*stepMs + slotMs*float64(index+1),
		Side:           order.Side,
		Amount:         float64(order.Amount),
		ReferencePrice: currentMidPrice(e.buys, e.sells, fundamental),
	})
}

func (e *Environment) recordSnapshot(step int) {
	if step < 0 || len(e.fundamentals) == 0 {
		return
	}
	fundamental := e.fundamentals[minInt(step, len(e.fundamentals)-1)]
	e.traceSnapshots = append(e.traceSnapshots, bookSnapshot{
		Step:     step,
		MidPrice: currentMidPrice(e.buys, e.sells, fundamental),
		Spread:   float64(currentSpread(e.buys, e.sells)),
		BidQty:   topLevelQty(e.buys, 5),
		AskQty:   topLevelQty(e.sells, 5),
	})
}

func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
}

func absInt(v int) int {
	if v < 0 {
		return -v
	}
	return v
}
