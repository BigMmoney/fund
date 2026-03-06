package simulator

import (
	"math/rand"
	"time"

	"pre_trading/benchmark"
)

type Environment struct {
	cfg              ScenarioConfig
	rng              *rand.Rand
	fundamentals     []int64
	agents           map[string]AgentConfig
	accounts         map[string]AccountState
	initialCash      int64
	initialUnits     int64
	buys             []Order
	sells            []Order
	pending          []pendingOrder
	seq              int64
	metricAcc        *metricAccumulator
	ordersSubmitted  int
	ordersAccepted   int
	fills            []Fill
	latencies        []time.Duration
	riskRejections   int
	negViolations    int
	conservationHits int
	maxActiveStep    int
}

type pendingOrder struct {
	ReleaseStep int
	Order       Order
}

func NewEnvironment(cfg ScenarioConfig) *Environment {
	agents := make(map[string]AgentConfig, len(cfg.Agents))
	accounts := make(map[string]AccountState, len(cfg.Agents))
	var totalCash, totalUnits int64
	for _, agent := range cfg.Agents {
		agents[agent.ID] = agent
		accounts[agent.ID] = AccountState{Cash: agent.InitialCash, Units: agent.InitialUnits}
		totalCash += agent.InitialCash
		totalUnits += agent.InitialUnits
	}
	return &Environment{
		cfg:          cfg,
		rng:          rand.New(rand.NewSource(cfg.Seed)),
		fundamentals: generateFundamentals(cfg.TotalSteps+1, cfg.Seed),
		agents:       agents,
		accounts:     accounts,
		initialCash:  totalCash,
		initialUnits: totalUnits,
		metricAcc:    newMetricAccumulator(),
		latencies:    make([]time.Duration, 0, cfg.TotalSteps*4),
		maxActiveStep: -1,
	}
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

func (e *Environment) Run() BenchmarkResult {
	start := time.Now()

	for step := 0; step < e.cfg.TotalSteps; step++ {
		e.step(step)
		if e.cfg.Mode == ModeBatch && e.cfg.BatchWindowSteps > 0 && (step+1)%e.cfg.BatchWindowSteps == 0 {
			e.flushBatch(step)
		}
	}
	if e.cfg.Mode == ModeBatch {
		e.flushBatch(e.cfg.TotalSteps)
	}
	if e.cfg.Mode == ModeSpeedBump {
		flushStep := e.cfg.TotalSteps
		for len(e.pending) > 0 {
			e.releasePending(flushStep)
			flushStep++
		}
	}

	elapsed := time.Since(start)
	stats := benchmark.ComputeLatencyStats(e.latencies)
	activeSteps := e.maxActiveStep + 1
	if activeSteps <= 0 {
		activeSteps = e.cfg.TotalSteps
	}
	return BenchmarkResult{
		Name:                      e.cfg.Name,
		Mode:                      e.cfg.Mode,
		BatchWindowMs:             int(e.cfg.StepDuration.Milliseconds()) * e.cfg.BatchWindowSteps,
		SpeedBumpMs:               int(e.cfg.StepDuration.Milliseconds()) * e.cfg.SpeedBumpSteps,
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

func (e *Environment) step(step int) {
	if step > e.maxActiveStep {
		e.maxActiveStep = step
	}
	if e.cfg.Mode == ModeSpeedBump {
		e.releasePending(step)
	}
	fundamental := e.fundamentals[step]
	perStepCount := 0
	generated := make([]Order, 0, len(e.cfg.Agents)*2)
	for _, agent := range e.cfg.Agents {
		orders := generateOrdersForAgent(agent, step, e.fundamentals, e.rng, &e.seq, e.accounts[agent.ID])
		for _, order := range orders {
			e.ordersSubmitted++
			e.metricAcc.addSubmitted(order.Class, order.Amount)
			if order.Amount > e.cfg.Risk.MaxOrderAmount || perStepCount >= e.cfg.Risk.MaxOrdersPerStep {
				e.riskRejections++
				continue
			}
			generated = append(generated, order)
			e.ordersAccepted++
			perStepCount++
		}
	}

	sortOrdersForArrival(generated, e.agents)
	for _, order := range generated {
		switch e.cfg.Mode {
		case ModeImmediate:
			fills := processImmediateBook(&e.buys, &e.sells, order, step, fundamental)
			e.applyFills(fills)
		case ModeBatch:
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

	if spread := currentSpread(e.buys, e.sells); spread > 0 {
		e.metricAcc.addSpread(spread)
	}
}

func (e *Environment) flushBatch(step int) {
	if step > e.maxActiveStep {
		e.maxActiveStep = step
	}
	fills := processBatchBook(&e.buys, &e.sells, step, e.fundamentals[minInt(step, len(e.fundamentals)-1)])
	e.applyFills(fills)
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

		if buyer.Cash < 0 || buyer.Units < 0 || seller.Cash < 0 || seller.Units < 0 {
			e.negViolations++
		}
		if !e.checkConservation() {
			e.conservationHits++
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
