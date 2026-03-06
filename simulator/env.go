package simulator

import (
	"math/rand"
	"time"

	"pre_trading/benchmark"
)

type Environment struct {
	cfg                   ScenarioConfig
	rng                   *rand.Rand
	fundamentals          []int64
	agents                map[string]AgentConfig
	accounts              map[string]AccountState
	initialCash           int64
	initialUnits          int64
	buys                  []Order
	sells                 []Order
	pending               []pendingOrder
	seq                   int64
	metricAcc             *metricAccumulator
	ordersSubmitted       int
	ordersAccepted        int
	fills                 []Fill
	latencies             []time.Duration
	riskRejections        int
	negViolations         int
	conservationHits      int
	maxActiveStep         int
	currentStep           int
	done                  bool
	currentBatchWindow    int
	batchCycleSteps       int
	lastStepAccepted      int
	adaptiveWindowHistory []int
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
	e.fundamentals = generateFundamentals(e.cfg.TotalSteps+1, e.cfg.Seed)
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
		OrdersSubmitted:           e.ordersSubmitted,
		OrdersAccepted:            e.ordersAccepted,
		Fills:                     len(e.fills),
		AverageSpread:             e.metricAcc.averageSpread(),
		AveragePriceImpact:        e.metricAcc.averagePriceImpact(),
		QueuePriorityAdvantage:    e.metricAcc.queuePriorityAdvantage(),
		LatencyArbitrageProfit:    e.metricAcc.ArbProfit,
		ExecutionDispersion:       e.metricAcc.executionDispersion(),
		NegativeBalanceViolations: e.negViolations,
		ConservationBreaches:      e.conservationHits,
		RiskRejections:            e.riskRejections,
	}
}

func (e *Environment) Step() StepResult {
	if e.done {
		return StepResult{Observation: e.Observe(), Metrics: e.Metrics()}
	}

	if e.currentStep < e.cfg.TotalSteps {
		e.runStep(e.currentStep)
		e.currentStep++
		if e.cfg.Mode == ModeBatch && e.cfg.BatchWindowSteps > 0 && e.batchCycleSteps >= e.cfg.BatchWindowSteps {
			e.flushBatch(e.currentStep - 1)
			e.batchCycleSteps = 0
		}
		if e.cfg.Mode == ModeAdaptiveBatch && e.currentBatchWindow > 0 && e.batchCycleSteps >= e.currentBatchWindow {
			windowUsed := e.currentBatchWindow
			e.flushBatch(e.currentStep - 1)
			e.batchCycleSteps = 0
			e.adaptiveWindowHistory = append(e.adaptiveWindowHistory, windowUsed)
			e.currentBatchWindow = e.nextAdaptiveWindow()
		}
	} else {
		e.flushResidualWork()
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

	elapsed := time.Since(start)
	stats := benchmark.ComputeLatencyStats(e.latencies)
	activeSteps := e.maxActiveStep + 1
	if activeSteps <= 0 {
		activeSteps = e.cfg.TotalSteps
	}
	minAdaptiveMs, maxAdaptiveMs, meanAdaptiveMs := e.adaptiveWindowStats()
	return BenchmarkResult{
		Name:                      e.cfg.Name,
		Mode:                      e.cfg.Mode,
		BatchWindowMs:             int(e.cfg.StepDuration.Milliseconds()) * e.cfg.BatchWindowSteps,
		SpeedBumpMs:               int(e.cfg.StepDuration.Milliseconds()) * e.cfg.SpeedBumpSteps,
		AdaptiveWindowMinMs:       minAdaptiveMs,
		AdaptiveWindowMaxMs:       maxAdaptiveMs,
		AdaptiveWindowMeanMs:      meanAdaptiveMs,
		Seed:                      e.cfg.Seed,
		OrdersSubmitted:           e.ordersSubmitted,
		OrdersAccepted:            e.ordersAccepted,
		Fills:                     len(e.fills),
		OrdersPerSec:              benchmark.ComputeThroughput(e.ordersAccepted, time.Duration(activeSteps)*e.cfg.StepDuration),
		FillsPerSec:               benchmark.ComputeThroughput(len(e.fills), time.Duration(activeSteps)*e.cfg.StepDuration),
		P50LatencyMs:              stats.P50Ms,
		P95LatencyMs:              stats.P95Ms,
		P99LatencyMs:              stats.P99Ms,
		AverageSpread:             e.metricAcc.averageSpread(),
		AveragePriceImpact:        e.metricAcc.averagePriceImpact(),
		QueuePriorityAdvantage:    e.metricAcc.queuePriorityAdvantage(),
		LatencyArbitrageProfit:    e.metricAcc.ArbProfit,
		ExecutionDispersion:       e.metricAcc.executionDispersion(),
		NegativeBalanceViolations: e.negViolations,
		ConservationBreaches:      e.conservationHits,
		RiskRejections:            e.riskRejections,
		Elapsed:                   elapsed,
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
		orders := generateOrdersForAgent(agent, step, e.fundamentals, e.rng, &e.seq, e.accounts[agent.ID])
		for _, order := range orders {
			e.ordersSubmitted++
			e.metricAcc.addSubmitted(order.Class, order.Amount)
			if !e.cfg.DisableRiskLimits && (order.Amount > e.cfg.Risk.MaxOrderAmount || perStepCount >= e.cfg.Risk.MaxOrdersPerStep) {
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
	for _, order := range generated {
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
				ReleaseStep: step + maxInt(1, e.cfg.SpeedBumpSteps),
				Order:       order,
			})
		}
	}

	e.batchCycleSteps++
	if spread := currentSpread(e.buys, e.sells); spread > 0 {
		e.metricAcc.addSpread(spread)
	}
}

func (e *Environment) flushBatch(step int) {
	if step > e.maxActiveStep {
		e.maxActiveStep = step
	}
	fills := processBatchBookWithOptions(&e.buys, &e.sells, step, e.fundamentals[minInt(step, len(e.fundamentals)-1)], e.cfg.RandomizeBatchTieBreak, e.rng)
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

func generateFundamentals(steps int, seed int64) []int64 {
	rng := rand.New(rand.NewSource(seed))
	values := make([]int64, steps)
	values[0] = 50
	for i := 1; i < steps; i++ {
		drift := int64(1)
		if (i/20)%2 == 1 {
			drift = -1
		}
		shock := int64(rng.Intn(3) - 1)
		values[i] = clampInt64(values[i-1]+drift+shock, 20, 80)
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
