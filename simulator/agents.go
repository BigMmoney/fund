package simulator

import (
	"fmt"
	"math/rand"
	"sort"
)

func DefaultPopulation() []AgentConfig {
	return []AgentConfig{
		{ID: "mm-1", Class: AgentMarketMaker, LatencyTier: 2, BaseSize: 3, QuoteWidth: 2, Intensity: 2, InitialCash: 500000, InitialUnits: 5000},
		{ID: "mm-2", Class: AgentMarketMaker, LatencyTier: 2, BaseSize: 3, QuoteWidth: 3, Intensity: 2, InitialCash: 500000, InitialUnits: 5000},
		{ID: "arb-1", Class: AgentArbitrageur, LatencyTier: 1, BaseSize: 4, QuoteWidth: 1, Intensity: 1, InitialCash: 500000, InitialUnits: 5000},
		{ID: "ret-1", Class: AgentRetail, LatencyTier: 3, BaseSize: 2, QuoteWidth: 4, Intensity: 2, InitialCash: 500000, InitialUnits: 5000},
		{ID: "ret-2", Class: AgentRetail, LatencyTier: 3, BaseSize: 2, QuoteWidth: 5, Intensity: 2, InitialCash: 500000, InitialUnits: 5000},
		{ID: "inf-1", Class: AgentInformed, LatencyTier: 2, BaseSize: 3, QuoteWidth: 1, Intensity: 1, InitialCash: 500000, InitialUnits: 5000},
	}
}

func StressPopulation() []AgentConfig {
	pop := DefaultPopulation()
	pop = append(pop,
		AgentConfig{ID: "ret-3", Class: AgentRetail, LatencyTier: 3, BaseSize: 3, QuoteWidth: 6, Intensity: 3, InitialCash: 500000, InitialUnits: 5000},
		AgentConfig{ID: "arb-2", Class: AgentArbitrageur, LatencyTier: 1, BaseSize: 5, QuoteWidth: 1, Intensity: 2, InitialCash: 500000, InitialUnits: 5000},
	)
	return pop
}

func generateOrdersForAgent(cfg AgentConfig, step int, fundamentals []int64, rng *rand.Rand, seq *int64, acct AccountState) []Order {
	orders := make([]Order, 0, cfg.Intensity*2)
	current := fundamentals[step]
	next := current
	if step+1 < len(fundamentals) {
		next = fundamentals[step+1]
	}
	stale := fundamentals[max(0, step-cfg.LatencyTier)]

	appendOrder := func(side Side, price int64, amount int64) {
		if amount <= 0 {
			return
		}
		*seq = *seq + 1
		orders = append(orders, Order{
			ID:          fmt.Sprintf("%s-%d-%d", cfg.ID, step, *seq),
			AgentID:     cfg.ID,
			Class:       cfg.Class,
			Side:        side,
			Price:       clampInt64(price, 1, 100),
			Amount:      amount,
			ArrivalStep: step,
			ArrivalSeq:  *seq,
		})
	}

	for i := 0; i < cfg.Intensity; i++ {
		switch cfg.Class {
		case AgentMarketMaker:
			size := cfg.BaseSize + int64(i%2)
			appendOrder(Buy, current-cfg.QuoteWidth, size)
			appendOrder(Sell, current+cfg.QuoteWidth, size)
		case AgentArbitrageur:
			size := cfg.BaseSize
			switch {
			case current-stale >= 2:
				appendOrder(Buy, current+1, size)
			case stale-current >= 2:
				appendOrder(Sell, current-1, size)
			default:
				if step%2 == 0 {
					appendOrder(Buy, current, size)
				}
			}
		case AgentRetail:
			size := cfg.BaseSize + int64(rng.Intn(2))
			offset := int64(rng.Intn(int(cfg.QuoteWidth*2+1))) - cfg.QuoteWidth
			if rng.Intn(2) == 0 {
				appendOrder(Buy, current+offset, size)
			} else {
				appendOrder(Sell, current+offset, size)
			}
		case AgentInformed:
			size := cfg.BaseSize
			if next > current {
				appendOrder(Buy, current+1, size)
			} else if next < current {
				appendOrder(Sell, current-1, size)
			} else if rng.Intn(2) == 0 {
				appendOrder(Buy, current, size)
			}
		}
	}

	return filterByResources(orders, acct)
}

func filterByResources(orders []Order, acct AccountState) []Order {
	filtered := make([]Order, 0, len(orders))
	availableCash := acct.Cash
	availableUnits := acct.Units
	for _, order := range orders {
		switch order.Side {
		case Buy:
			cost := order.Price * order.Amount
			if cost <= availableCash {
				availableCash -= cost
				filtered = append(filtered, order)
			}
		case Sell:
			if order.Amount <= availableUnits {
				availableUnits -= order.Amount
				filtered = append(filtered, order)
			}
		}
	}
	return filtered
}

func sortOrdersForArrival(orders []Order, agents map[string]AgentConfig) {
	sort.SliceStable(orders, func(i, j int) bool {
		left := agents[orders[i].AgentID]
		right := agents[orders[j].AgentID]
		if left.LatencyTier == right.LatencyTier {
			return orders[i].ArrivalSeq < orders[j].ArrivalSeq
		}
		return left.LatencyTier < right.LatencyTier
	})
}

func clampInt64(v, minV, maxV int64) int64 {
	if v < minV {
		return minV
	}
	if v > maxV {
		return maxV
	}
	return v
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
