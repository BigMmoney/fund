package simulator

import "math"

type metricAccumulator struct {
	SpreadSum         float64
	SpreadSamples     int
	PriceImpactSum    float64
	PriceImpactCount  int
	ArbProfit         float64
	SubmittedByClass  map[AgentClass]int64
	FilledByClass     map[AgentClass]int64
}

func newMetricAccumulator() *metricAccumulator {
	return &metricAccumulator{
		SubmittedByClass: make(map[AgentClass]int64),
		FilledByClass:    make(map[AgentClass]int64),
	}
}

func (m *metricAccumulator) addSpread(spread int64) {
	if spread <= 0 {
		return
	}
	m.SpreadSum += float64(spread)
	m.SpreadSamples++
}

func (m *metricAccumulator) addFill(fill Fill) {
	m.PriceImpactSum += math.Abs(float64(fill.Price - fill.FundamentalMark))
	m.PriceImpactCount++
	m.FilledByClass[fill.BuyerClass] += fill.Amount
	m.FilledByClass[fill.SellerClass] += fill.Amount

	if fill.BuyerClass == AgentArbitrageur && fill.FundamentalMark > fill.Price {
		m.ArbProfit += float64(fill.FundamentalMark-fill.Price) * float64(fill.Amount)
	}
	if fill.SellerClass == AgentArbitrageur && fill.Price > fill.FundamentalMark {
		m.ArbProfit += float64(fill.Price-fill.FundamentalMark) * float64(fill.Amount)
	}
}

func (m *metricAccumulator) addSubmitted(class AgentClass, amount int64) {
	m.SubmittedByClass[class] += amount
}

func (m *metricAccumulator) averageSpread() float64 {
	if m.SpreadSamples == 0 {
		return 0
	}
	return m.SpreadSum / float64(m.SpreadSamples)
}

func (m *metricAccumulator) averagePriceImpact() float64 {
	if m.PriceImpactCount == 0 {
		return 0
	}
	return m.PriceImpactSum / float64(m.PriceImpactCount)
}

func (m *metricAccumulator) queuePriorityAdvantage() float64 {
	retailRate := m.fillRate(AgentRetail)
	arbRate := m.fillRate(AgentArbitrageur)
	return arbRate - retailRate
}

func (m *metricAccumulator) executionDispersion() float64 {
	classes := []AgentClass{AgentMarketMaker, AgentArbitrageur, AgentRetail, AgentInformed}
	rates := make([]float64, 0, len(classes))
	for _, class := range classes {
		rates = append(rates, m.fillRate(class))
	}
	var mean float64
	for _, v := range rates {
		mean += v
	}
	mean /= float64(len(rates))

	var variance float64
	for _, v := range rates {
		delta := v - mean
		variance += delta * delta
	}
	variance /= float64(len(rates))
	return math.Sqrt(variance)
}

func (m *metricAccumulator) fillRate(class AgentClass) float64 {
	submitted := m.SubmittedByClass[class]
	if submitted == 0 {
		return 0
	}
	return float64(m.FilledByClass[class]) / float64(submitted)
}
