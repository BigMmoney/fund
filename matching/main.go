package main

import (
	"context"
	"fmt"
	"log"
	"sync"
	"time"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
	"pre_trading/services/utils"
)

// MatchingEngine implements FBA (Frequent Batch Auction) matching
type MatchingEngine struct {
	mu          sync.RWMutex
	intents     map[string]*types.Intent // intentID -> Intent
	batchWindow time.Duration
	eventBus    *eventbus.EventBus
	running     bool
}

func NewMatchingEngine(batchWindow time.Duration, eventBus *eventbus.EventBus) *MatchingEngine {
	return &MatchingEngine{
		intents:     make(map[string]*types.Intent),
		batchWindow: batchWindow,
		eventBus:    eventBus,
		running:     false,
	}
}

// Start starts the matching engine batch processing loop
func (me *MatchingEngine) Start(ctx context.Context) {
	me.running = true
	ticker := time.NewTicker(me.batchWindow)
	defer ticker.Stop()

	log.Printf("Matching engine started with batch window: %v", me.batchWindow)

	for me.running {
		select {
		case <-ticker.C:
			me.processBatch()
		case <-ctx.Done():
			me.running = false
			return
		}
	}
}

// AddIntent adds a new intent to the matching engine
func (me *MatchingEngine) AddIntent(intent *types.Intent) {
	me.mu.Lock()
	defer me.mu.Unlock()

	me.intents[intent.ID] = intent
	me.eventBus.Publish(types.EventTypeIntentReceived, intent)
	log.Printf("Intent received: id=%s, side=%s, price=%d, amount=%d", intent.ID, intent.Side, intent.Price, intent.Amount)
}

// CancelIntent cancels an intent
func (me *MatchingEngine) CancelIntent(intentID string) error {
	me.mu.Lock()
	defer me.mu.Unlock()

	intent, exists := me.intents[intentID]
	if !exists {
		return fmt.Errorf("intent not found: %s", intentID)
	}

	intent.Status = "cancelled"
	me.eventBus.Publish(types.EventTypeIntentCancelled, intent)
	log.Printf("Intent cancelled: id=%s", intentID)
	return nil
}

// processBatch processes a single batch of intents
func (me *MatchingEngine) processBatch() {
	me.mu.Lock()
	defer me.mu.Unlock()

	if len(me.intents) == 0 {
		return
	}

	log.Printf("Processing batch with %d intents", len(me.intents))

	// 1. Collect valid intents
	validIntents := me.collectValidIntents()
	if len(validIntents) == 0 {
		return
	}

	// 2. Group by market and outcome
	marketGroups := me.groupByMarket(validIntents)

	// 3. Process each market separately
	for marketKey, intents := range marketGroups {
		me.processMarketBatch(marketKey, intents)
	}
}

// collectValidIntents filters out expired and invalid intents
func (me *MatchingEngine) collectValidIntents() []*types.Intent {
	valid := make([]*types.Intent, 0)
	now := time.Now()

	for _, intent := range me.intents {
		// Skip cancelled or filled intents
		if intent.Status == "cancelled" || intent.Status == "filled" {
			continue
		}

		// Check expiration
		if now.After(intent.ExpiresAt) {
			intent.Status = "expired"
			continue
		}

		valid = append(valid, intent)
	}

	return valid
}

// groupByMarket groups intents by market and outcome
func (me *MatchingEngine) groupByMarket(intents []*types.Intent) map[string][]*types.Intent {
	groups := make(map[string][]*types.Intent)

	for _, intent := range intents {
		key := fmt.Sprintf("%s:%d", intent.MarketID, intent.Outcome)
		groups[key] = append(groups[key], intent)
	}

	return groups
}

// processMarketBatch processes intents for a single market/outcome
func (me *MatchingEngine) processMarketBatch(marketKey string, intents []*types.Intent) {
	// 3. Aggregate L2 orderbook
	buyOrders, sellOrders := me.aggregateOrderbook(intents)

	if len(buyOrders) == 0 || len(sellOrders) == 0 {
		log.Printf("Market %s: no matching orders (buys=%d, sells=%d)", marketKey, len(buyOrders), len(sellOrders))
		return
	}

	// 4. Compute clearing price
	clearingPrice := me.computeClearingPrice(buyOrders, sellOrders)
	if clearingPrice == 0 {
		log.Printf("Market %s: no clearing price found", marketKey)
		return
	}

	log.Printf("Market %s: clearing price=%d", marketKey, clearingPrice)

	// 5. Allocate fills proportionally
	fills := me.allocateFills(buyOrders, sellOrders, clearingPrice)

	if len(fills) == 0 {
		return
	}

	// 6. Emit fills and create ledger deltas
	batchOpID := utils.GenerateOpID("batch")
	me.emitFills(fills, batchOpID)

	// 7. Update intent statuses
	for _, fill := range fills {
		if intent, exists := me.intents[fill.IntentID]; exists {
			intent.Status = "filled"
		}
	}
}

// aggregateOrderbook aggregates intents into buy and sell orders
func (me *MatchingEngine) aggregateOrderbook(intents []*types.Intent) ([]types.Order, []types.Order) {
	var buyOrders, sellOrders []types.Order

	for _, intent := range intents {
		order := types.Order{
			ID:        intent.ID,
			UserID:    intent.UserID,
			MarketID:  intent.MarketID,
			Side:      intent.Side,
			Price:     intent.Price,
			Amount:    intent.Amount,
			Outcome:   intent.Outcome,
			Status:    "pending",
			CreatedAt: intent.CreatedAt,
		}

		if intent.Side == "buy" {
			buyOrders = append(buyOrders, order)
		} else {
			sellOrders = append(sellOrders, order)
		}
	}

	return buyOrders, sellOrders
}

// computeClearingPrice computes the price that maximizes matched volume
func (me *MatchingEngine) computeClearingPrice(buyOrders, sellOrders []types.Order) int64 {
	// Build demand and supply curves
	demandCurve := me.buildDemandCurve(buyOrders)
	supplyCurve := me.buildSupplyCurve(sellOrders)

	var bestPrice int64
	var maxVolume int64

	// Find price that maximizes min(demand, supply)
	pricePoints := me.getAllPricePoints(buyOrders, sellOrders)
	for _, price := range pricePoints {
		demand := me.getDemandAtPrice(demandCurve, price)
		supply := me.getSupplyAtPrice(supplyCurve, price)
		volume := utils.MinInt64(demand, supply)

		if volume > maxVolume {
			maxVolume = volume
			bestPrice = price
		}
	}

	return bestPrice
}

// buildDemandCurve builds cumulative demand at each price level
func (me *MatchingEngine) buildDemandCurve(buyOrders []types.Order) map[int64]int64 {
	curve := make(map[int64]int64)
	for _, order := range buyOrders {
		for price := order.Price; price <= 100; price++ {
			curve[price] += order.Amount
		}
	}
	return curve
}

// buildSupplyCurve builds cumulative supply at each price level
func (me *MatchingEngine) buildSupplyCurve(sellOrders []types.Order) map[int64]int64 {
	curve := make(map[int64]int64)
	for _, order := range sellOrders {
		for price := int64(0); price <= order.Price; price++ {
			curve[price] += order.Amount
		}
	}
	return curve
}

// getAllPricePoints gets all unique price points from orders
func (me *MatchingEngine) getAllPricePoints(buyOrders, sellOrders []types.Order) []int64 {
	priceSet := make(map[int64]bool)
	for _, order := range buyOrders {
		priceSet[order.Price] = true
	}
	for _, order := range sellOrders {
		priceSet[order.Price] = true
	}

	prices := make([]int64, 0, len(priceSet))
	for price := range priceSet {
		prices = append(prices, price)
	}
	return prices
}

// getDemandAtPrice gets demand at a specific price
func (me *MatchingEngine) getDemandAtPrice(curve map[int64]int64, price int64) int64 {
	return curve[price]
}

// getSupplyAtPrice gets supply at a specific price
func (me *MatchingEngine) getSupplyAtPrice(curve map[int64]int64, price int64) int64 {
	return curve[price]
}

// allocateFills allocates fills proportionally at clearing price
func (me *MatchingEngine) allocateFills(buyOrders, sellOrders []types.Order, clearingPrice int64) []types.Fill {
	fills := make([]types.Fill, 0)

	// Filter orders willing to trade at clearing price
	eligibleBuys := me.filterEligibleBuys(buyOrders, clearingPrice)
	eligibleSells := me.filterEligibleSells(sellOrders, clearingPrice)

	totalBuyAmount := me.sumOrderAmounts(eligibleBuys)
	totalSellAmount := me.sumOrderAmounts(eligibleSells)
	matchedVolume := utils.MinInt64(totalBuyAmount, totalSellAmount)

	if matchedVolume == 0 {
		return fills
	}

	// Allocate proportionally
	fills = append(fills, me.allocateBuyFills(eligibleBuys, clearingPrice, matchedVolume, totalBuyAmount)...)
	fills = append(fills, me.allocateSellFills(eligibleSells, clearingPrice, matchedVolume, totalSellAmount)...)

	return fills
}

// filterEligibleBuys filters buy orders willing to pay clearing price or higher
func (me *MatchingEngine) filterEligibleBuys(buyOrders []types.Order, clearingPrice int64) []types.Order {
	eligible := make([]types.Order, 0)
	for _, order := range buyOrders {
		if order.Price >= clearingPrice {
			eligible = append(eligible, order)
		}
	}
	return eligible
}

// filterEligibleSells filters sell orders willing to accept clearing price or lower
func (me *MatchingEngine) filterEligibleSells(sellOrders []types.Order, clearingPrice int64) []types.Order {
	eligible := make([]types.Order, 0)
	for _, order := range sellOrders {
		if order.Price <= clearingPrice {
			eligible = append(eligible, order)
		}
	}
	return eligible
}

// sumOrderAmounts sums the amounts of orders
func (me *MatchingEngine) sumOrderAmounts(orders []types.Order) int64 {
	var total int64
	for _, order := range orders {
		total += order.Amount
	}
	return total
}

// allocateBuyFills allocates fills for buy orders
func (me *MatchingEngine) allocateBuyFills(orders []types.Order, price int64, matchedVolume, totalAmount int64) []types.Fill {
	fills := make([]types.Fill, 0)

	for _, order := range orders {
		fillAmount := (order.Amount * matchedVolume) / totalAmount
		if fillAmount > 0 {
			fill := types.Fill{
				ID:        utils.GenerateID(),
				IntentID:  order.ID,
				UserID:    order.UserID,
				MarketID:  order.MarketID,
				Side:      "buy",
				Price:     price,
				Amount:    fillAmount,
				Outcome:   order.Outcome,
				Timestamp: time.Now(),
				OpID:      utils.GenerateOpID("fill"),
			}
			fills = append(fills, fill)
		}
	}

	return fills
}

// allocateSellFills allocates fills for sell orders
func (me *MatchingEngine) allocateSellFills(orders []types.Order, price int64, matchedVolume, totalAmount int64) []types.Fill {
	fills := make([]types.Fill, 0)

	for _, order := range orders {
		fillAmount := (order.Amount * matchedVolume) / totalAmount
		if fillAmount > 0 {
			fill := types.Fill{
				ID:        utils.GenerateID(),
				IntentID:  order.ID,
				UserID:    order.UserID,
				MarketID:  order.MarketID,
				Side:      "sell",
				Price:     price,
				Amount:    fillAmount,
				Outcome:   order.Outcome,
				Timestamp: time.Now(),
				OpID:      utils.GenerateOpID("fill"),
			}
			fills = append(fills, fill)
		}
	}

	return fills
}

// emitFills publishes fills to event bus
func (me *MatchingEngine) emitFills(fills []types.Fill, batchOpID string) {
	for _, fill := range fills {
		me.eventBus.Publish(types.EventTypeFillCreated, fill)
		log.Printf("Fill created: id=%s, user=%s, side=%s, price=%d, amount=%d",
			fill.ID, fill.UserID, fill.Side, fill.Price, fill.Amount)
	}
}

func main() {
	fmt.Println("Matching engine starting...")

	// Initialize event bus
	eventBus := eventbus.NewEventBus()

	// Create matching engine with 500ms batch window
	engine := NewMatchingEngine(500*time.Millisecond, eventBus)

	// Subscribe to fill events
	fillCh := eventBus.Subscribe(types.EventTypeFillCreated, 100)
	go func() {
		for event := range fillCh {
			fill := event.Payload.(types.Fill)
			log.Printf("Fill event: %s %s %.2f @ %d", fill.Side, fill.UserID, float64(fill.Amount)/100, fill.Price)
		}
	}()

	// Start matching engine in background
	ctx := context.Background()
	go engine.Start(ctx)

	// Example: Add some test intents
	intent1 := &types.Intent{
		ID:        utils.GenerateID(),
		UserID:    "user1",
		MarketID:  "market1",
		Side:      "buy",
		Price:     55,
		Amount:    1000,
		Outcome:   1,
		CreatedAt: time.Now(),
		ExpiresAt: time.Now().Add(10 * time.Second),
		Status:    "pending",
	}
	engine.AddIntent(intent1)

	intent2 := &types.Intent{
		ID:        utils.GenerateID(),
		UserID:    "user2",
		MarketID:  "market1",
		Side:      "sell",
		Price:     54,
		Amount:    800,
		Outcome:   1,
		CreatedAt: time.Now(),
		ExpiresAt: time.Now().Add(10 * time.Second),
		Status:    "pending",
	}
	engine.AddIntent(intent2)

	// Keep service running
	log.Println("Matching engine started on port :8082")
	select {}
}
