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

func StrategicPopulation() []AgentConfig {
	return []AgentConfig{
		{
			ID: "mm-strat-1", Class: AgentMarketMaker, LatencyTier: 2, BaseSize: 3, QuoteWidth: 2, Intensity: 2,
			InventoryTarget: 5000, InventorySkew: 2, TrendSensitivity: 1, SignalScale: 2,
			InitialCash: 500000, InitialUnits: 5000,
		},
		{
			ID: "mm-strat-2", Class: AgentMarketMaker, LatencyTier: 2, BaseSize: 3, QuoteWidth: 3, Intensity: 2,
			InventoryTarget: 5000, InventorySkew: 3, TrendSensitivity: 1, SignalScale: 2,
			InitialCash: 500000, InitialUnits: 5000,
		},
		{
			ID: "arb-strat-1", Class: AgentArbitrageur, LatencyTier: 1, BaseSize: 4, QuoteWidth: 1, Intensity: 1,
			InventoryTarget: 5000, InventorySkew: 0, TrendSensitivity: 2, SignalScale: 3,
			InitialCash: 500000, InitialUnits: 5000,
		},
		{
			ID: "ret-strat-1", Class: AgentRetail, LatencyTier: 3, BaseSize: 2, QuoteWidth: 4, Intensity: 2,
			InventoryTarget: 5000, InventorySkew: 1, TrendSensitivity: 1, SignalScale: 2,
			InitialCash: 500000, InitialUnits: 5000,
		},
		{
			ID: "ret-strat-2", Class: AgentRetail, LatencyTier: 3, BaseSize: 2, QuoteWidth: 5, Intensity: 2,
			InventoryTarget: 5000, InventorySkew: 1, TrendSensitivity: 2, SignalScale: 2,
			InitialCash: 500000, InitialUnits: 5000,
		},
		{
			ID: "inf-strat-1", Class: AgentInformed, LatencyTier: 2, BaseSize: 3, QuoteWidth: 1, Intensity: 1,
			InventoryTarget: 5000, InventorySkew: 0, TrendSensitivity: 3, SignalScale: 3,
			InitialCash: 500000, InitialUnits: 5000,
		},
	}
}

func WithoutAgentClass(pop []AgentConfig, class AgentClass) []AgentConfig {
	filtered := make([]AgentConfig, 0, len(pop))
	for _, agent := range pop {
		if agent.Class == class {
			continue
		}
		filtered = append(filtered, agent)
	}
	return filtered
}

func RetailBurstPopulation() []AgentConfig {
	pop := DefaultPopulation()
	burst := make([]AgentConfig, 0, len(pop)+2)
	for _, agent := range pop {
		if agent.Class == AgentRetail {
			agent.Intensity += 2
			agent.BaseSize += 1
			agent.QuoteWidth += 1
		}
		burst = append(burst, agent)
	}
	burst = append(burst,
		AgentConfig{ID: "ret-burst-1", Class: AgentRetail, LatencyTier: 3, BaseSize: 4, QuoteWidth: 6, Intensity: 4, InitialCash: 500000, InitialUnits: 5000},
		AgentConfig{ID: "ret-burst-2", Class: AgentRetail, LatencyTier: 3, BaseSize: 3, QuoteWidth: 7, Intensity: 4, InitialCash: 500000, InitialUnits: 5000},
	)
	return burst
}

func ScaleClassIntensity(pop []AgentConfig, class AgentClass, numerator, denominator int) []AgentConfig {
	if denominator <= 0 {
		denominator = 1
	}
	scaled := make([]AgentConfig, 0, len(pop))
	for _, agent := range pop {
		if agent.Class == class {
			next := (agent.Intensity * numerator) / denominator
			if next < 1 {
				next = 1
			}
			agent.Intensity = next
		}
		scaled = append(scaled, agent)
	}
	return scaled
}

func AdjustClassQuoteWidth(pop []AgentConfig, class AgentClass, delta int64) []AgentConfig {
	adjusted := make([]AgentConfig, 0, len(pop))
	for _, agent := range pop {
		if agent.Class == class {
			agent.QuoteWidth += delta
			if agent.QuoteWidth < 1 {
				agent.QuoteWidth = 1
			}
		}
		adjusted = append(adjusted, agent)
	}
	return adjusted
}

func ScaleClassQuoteWidth(pop []AgentConfig, class AgentClass, numerator, denominator int) []AgentConfig {
	if denominator <= 0 {
		denominator = 1
	}
	scaled := make([]AgentConfig, 0, len(pop))
	for _, agent := range pop {
		if agent.Class == class {
			next := int64((int(agent.QuoteWidth) * numerator) / denominator)
			if next < 1 {
				next = 1
			}
			agent.QuoteWidth = next
		}
		scaled = append(scaled, agent)
	}
	return scaled
}

func AdjustClassBaseSize(pop []AgentConfig, class AgentClass, delta int64) []AgentConfig {
	adjusted := make([]AgentConfig, 0, len(pop))
	for _, agent := range pop {
		if agent.Class == class {
			agent.BaseSize += delta
			if agent.BaseSize < 1 {
				agent.BaseSize = 1
			}
		}
		adjusted = append(adjusted, agent)
	}
	return adjusted
}

func generateOrdersForAgent(cfg AgentConfig, step int, fundamentals []int64, rng *rand.Rand, seq *int64, acct AccountState, priceAggression int64) []Order {
	orders := make([]Order, 0, cfg.Intensity*2)
	current := fundamentals[step]
	next := current
	if step+1 < len(fundamentals) {
		next = fundamentals[step+1]
	}
	stale := fundamentals[max(0, step-cfg.LatencyTier)]
	trend := current - fundamentals[max(0, step-max(1, cfg.LatencyTier+1))]
	inventoryTarget := cfg.InventoryTarget
	if inventoryTarget == 0 {
		inventoryTarget = cfg.InitialUnits
	}
	inventoryGap := acct.Units - inventoryTarget

	appendOrder := func(side Side, price int64, amount int64) {
		if amount <= 0 {
			return
		}
		if side == Buy {
			price += priceAggression
		} else {
			price -= priceAggression
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
			if cfg.SignalScale > 0 {
				size += minInt64(cfg.SignalScale, absInt64(inventoryGap)/maxInt64(inventoryTarget/2000, 1))
			}
			center := current
			if cfg.InventorySkew > 0 {
				center -= signInt64(inventoryGap) * cfg.InventorySkew
			}
			if cfg.TrendSensitivity > 0 {
				center += signInt64(trend) * minInt64(cfg.TrendSensitivity, maxInt64(absInt64(trend), 1))
			}
			appendOrder(Buy, center-cfg.QuoteWidth, size)
			appendOrder(Sell, center+cfg.QuoteWidth, size)
		case AgentArbitrageur:
			size := cfg.BaseSize
			if cfg.SignalScale > 0 {
				signalSize := absInt64(current-stale) + minInt64(absInt64(trend), cfg.SignalScale)
				size += minInt64(cfg.SignalScale, signalSize)
			}
			signal := current - stale
			if cfg.TrendSensitivity > 0 {
				signal += signInt64(trend) * minInt64(cfg.TrendSensitivity, absInt64(trend))
			}
			switch {
			case signal >= 2:
				appendOrder(Buy, current+1, size)
				if cfg.SignalScale > 1 && signal >= 4 {
					appendOrder(Buy, current+2, maxInt64(1, size-1))
				}
			case signal <= -2:
				appendOrder(Sell, current-1, size)
				if cfg.SignalScale > 1 && signal <= -4 {
					appendOrder(Sell, current-2, maxInt64(1, size-1))
				}
			default:
				if step%2 == 0 {
					appendOrder(Buy, current, size)
				}
			}
		case AgentRetail:
			size := cfg.BaseSize + int64(rng.Intn(2))
			if cfg.SignalScale > 0 {
				size += minInt64(cfg.SignalScale, absInt64(trend)/2)
			}
			offset := int64(rng.Intn(int(cfg.QuoteWidth*2+1))) - cfg.QuoteWidth
			bias := 0
			if cfg.TrendSensitivity > 0 {
				bias += int(signInt64(trend) * minInt64(cfg.TrendSensitivity, maxInt64(absInt64(trend), 1)))
			}
			if cfg.InventorySkew > 0 {
				bias -= int(signInt64(inventoryGap) * minInt64(cfg.InventorySkew, 1))
			}
			buyProb := clampFloat(0.5+0.12*float64(bias), 0.15, 0.85)
			extraSide := Buy
			if rng.Float64() < buyProb {
				appendOrder(Buy, current+offset, size)
			} else {
				extraSide = Sell
				appendOrder(Sell, current+offset, size)
			}
			if cfg.SignalScale > 1 && absInt64(trend) >= 2 {
				appendOrder(extraSide, current+offset/2, maxInt64(1, size-1))
			}
		case AgentInformed:
			size := cfg.BaseSize
			confidence := absInt64(next - current)
			if cfg.SignalScale > 0 {
				size += minInt64(cfg.SignalScale, confidence)
			}
			if next > current {
				price := current + 1 + minInt64(cfg.TrendSensitivity, confidence)
				appendOrder(Buy, price, size)
				if cfg.SignalScale > 1 && confidence >= 2 {
					appendOrder(Buy, current+1, maxInt64(1, size-1))
				}
			} else if next < current {
				price := current - 1 - minInt64(cfg.TrendSensitivity, confidence)
				appendOrder(Sell, price, size)
				if cfg.SignalScale > 1 && confidence >= 2 {
					appendOrder(Sell, current-1, maxInt64(1, size-1))
				}
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

func signInt64(v int64) int64 {
	switch {
	case v > 0:
		return 1
	case v < 0:
		return -1
	default:
		return 0
	}
}
