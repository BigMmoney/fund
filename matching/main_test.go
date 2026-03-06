package main

import (
	"testing"
	"time"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
)

func newTestEngine() *MatchingEngine {
	return NewMatchingEngine(100*time.Millisecond, eventbus.NewEventBus())
}

func sumFills(fills []types.Fill, side string) int64 {
	var total int64
	for _, f := range fills {
		if f.Side == side {
			total += f.Amount
		}
	}
	return total
}

func TestComputeClearingPrice_MaxVolumeAndDeterministic(t *testing.T) {
	engine := newTestEngine()

	buys := []types.Order{
		{ID: "b1", Side: "buy", Price: 60, Amount: 100},
		{ID: "b2", Side: "buy", Price: 55, Amount: 100},
	}
	sells := []types.Order{
		{ID: "s1", Side: "sell", Price: 50, Amount: 80},
		{ID: "s2", Side: "sell", Price: 55, Amount: 120},
	}

	for i := 0; i < 20; i++ {
		price := engine.computeClearingPrice(buys, sells)
		if price != 55 {
			t.Fatalf("expected deterministic clearing price 55, got %d", price)
		}
	}
}

func TestAllocateFills_ConservesMatchedVolume(t *testing.T) {
	engine := newTestEngine()
	clearingPrice := int64(55)

	buys := []types.Order{
		{ID: "b1", UserID: "u1", MarketID: "m1", Side: "buy", Price: 60, Amount: 1, Outcome: 1},
		{ID: "b2", UserID: "u2", MarketID: "m1", Side: "buy", Price: 60, Amount: 1, Outcome: 1},
		{ID: "b3", UserID: "u3", MarketID: "m1", Side: "buy", Price: 60, Amount: 1, Outcome: 1},
	}
	sells := []types.Order{
		{ID: "s1", UserID: "u4", MarketID: "m1", Side: "sell", Price: 50, Amount: 2, Outcome: 1},
	}

	fills := engine.allocateFills(buys, sells, clearingPrice)
	buyFilled := sumFills(fills, "buy")
	sellFilled := sumFills(fills, "sell")

	if buyFilled != 2 || sellFilled != 2 {
		t.Fatalf("expected balanced fills=2/2, got buy=%d sell=%d", buyFilled, sellFilled)
	}

	maxByIntent := map[string]int64{"b1": 1, "b2": 1, "b3": 1, "s1": 2}
	for _, f := range fills {
		if f.Price != clearingPrice {
			t.Fatalf("fill %s has wrong price: %d", f.IntentID, f.Price)
		}
		if f.Amount <= 0 {
			t.Fatalf("fill %s has non-positive amount: %d", f.IntentID, f.Amount)
		}
		if f.Amount > maxByIntent[f.IntentID] {
			t.Fatalf("fill %s exceeds order amount: fill=%d max=%d", f.IntentID, f.Amount, maxByIntent[f.IntentID])
		}
	}
}

func TestProcessBatch_RespectsMarketOutcomeIsolation(t *testing.T) {
	engine := newTestEngine()
	now := time.Now()

	engine.AddIntent(&types.Intent{
		ID: "buy-outcome-1", UserID: "u1", MarketID: "m1", Side: "buy", Price: 60, Amount: 100,
		Outcome: 1, CreatedAt: now, ExpiresAt: now.Add(time.Minute), Status: "pending",
	})
	engine.AddIntent(&types.Intent{
		ID: "sell-outcome-2", UserID: "u2", MarketID: "m1", Side: "sell", Price: 40, Amount: 100,
		Outcome: 2, CreatedAt: now, ExpiresAt: now.Add(time.Minute), Status: "pending",
	})

	engine.processBatch()

	if engine.intents["buy-outcome-1"].Status == "filled" || engine.intents["sell-outcome-2"].Status == "filled" {
		t.Fatalf("intents across different outcomes must not match")
	}
}
