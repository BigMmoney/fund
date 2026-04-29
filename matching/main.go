package main

import (
	"context"
	"fmt"
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
	"pre_trading/services/utils"
)

// MatchingEngine implements FBA (Frequent Batch Auction) matching
type MatchingEngine struct {
	mu              sync.Mutex
	intents         map[string]*types.Intent // intentID -> Intent
	batchWindow     time.Duration
	minBatchSize    int           // trigger batch when queue reaches this size
	eventBus        *eventbus.EventBus
	running         bool
	publishMu       sync.Mutex    // protects batch event publishing
	triggerCh       chan struct{}  // signals early batch trigger
	cancelCh        chan string    // fast-path cancel processing (P0/P1)
	lastBatchTime   time.Time
	lastBatchMu     sync.Mutex
	pendingCount    atomic.Int64  // tracks pending intents without lock
	cancelTracker   *CancelTracker // optional cancel lifecycle tracker (nil in production)
	backlogMode     atomic.Bool    // P2: true during recovery backlog drain
	drainBatchSize  int            // P2: target batch size during backlog drain
}

// CancelTracker tracks cancel lifecycle metrics for stress testing
type CancelTracker struct {
	mu               sync.Mutex
	submittedAt      map[string]time.Time
	batchStarted     map[string]time.Time
	completedAt      map[string]time.Time
	wasFilled        map[string]bool
	batchDelayed     map[string]bool
	batchStartTime   time.Time
}

func NewCancelTracker() *CancelTracker {
	return &CancelTracker{
		submittedAt:  make(map[string]time.Time),
		batchStarted: make(map[string]time.Time),
		completedAt:  make(map[string]time.Time),
		wasFilled:    make(map[string]bool),
		batchDelayed: make(map[string]bool),
	}
}

func (ct *CancelTracker) RecordCancelSubmit(intentID string, t time.Time) {
	ct.mu.Lock()
	ct.submittedAt[intentID] = t
	ct.mu.Unlock()
}

func (ct *CancelTracker) SetBatchStart(t time.Time) {
	ct.mu.Lock()
	ct.batchStartTime = t
	ct.mu.Unlock()
}

func (ct *CancelTracker) GetBatchStart() time.Time {
	ct.mu.Lock()
	defer ct.mu.Unlock()
	return ct.batchStartTime
}

func (ct *CancelTracker) RecordCancelInBatch(intentID string, wasFilled bool, submittedBeforeBatch bool) {
	ct.mu.Lock()
	ct.batchStarted[intentID] = ct.batchStartTime
	ct.wasFilled[intentID] = wasFilled
	ct.batchDelayed[intentID] = !submittedBeforeBatch
	ct.mu.Unlock()
}

func (ct *CancelTracker) RecordCancelComplete(intentID string, t time.Time) {
	ct.mu.Lock()
	ct.completedAt[intentID] = t
	ct.mu.Unlock()
}

// RecordFastCancelComplete records a fast-path cancel completion (bypasses batch)
func (ct *CancelTracker) RecordFastCancelComplete(intentID string, t time.Time) {
	ct.mu.Lock()
	ct.batchStarted[intentID] = t // fast-path: batch start = completion time (no queue wait)
	ct.completedAt[intentID] = t
	ct.mu.Unlock()
}

func (ct *CancelTracker) ExtractMetrics() (total int, beforeFill int, afterFill int, delayed int, queueWaitMs, matchExecMs, totalLatencyMs []float64) {
	ct.mu.Lock()
	defer ct.mu.Unlock()

	type record struct {
		queueWait    float64
		matchExec    float64
		totalLatency float64
		wasFilled    bool
		batchDelayed bool
	}
	records := make([]record, 0, len(ct.completedAt))

	for id, completed := range ct.completedAt {
		submitted, hasSubmit := ct.submittedAt[id]
		batchStart, hasBatch := ct.batchStarted[id]
		if !hasSubmit || !hasBatch {
			continue
		}
		records = append(records, record{
			queueWait:    batchStart.Sub(submitted).Seconds() * 1000,
			matchExec:    completed.Sub(batchStart).Seconds() * 1000,
			totalLatency: completed.Sub(submitted).Seconds() * 1000,
			wasFilled:    ct.wasFilled[id],
			batchDelayed: ct.batchDelayed[id],
		})
	}

	queueWaitMs = make([]float64, len(records))
	matchExecMs = make([]float64, len(records))
	totalLatencyMs = make([]float64, len(records))
	for i, r := range records {
		queueWaitMs[i] = r.queueWait
		matchExecMs[i] = r.matchExec
		totalLatencyMs[i] = r.totalLatency
		if r.wasFilled {
			afterFill++
		} else {
			beforeFill++
		}
		if r.batchDelayed {
			delayed++
		}
	}
	return len(records), beforeFill, afterFill, delayed, queueWaitMs, matchExecMs, totalLatencyMs
}

func NewMatchingEngine(batchWindow time.Duration, eventBus *eventbus.EventBus) *MatchingEngine {
	return &MatchingEngine{
		intents:      make(map[string]*types.Intent, 1024),
		batchWindow:  batchWindow,
		minBatchSize: 100, // trigger early batch at 100 pending orders
		eventBus:     eventBus,
		running:      false,
		triggerCh:    make(chan struct{}, 1),
		cancelCh:     make(chan string, 2048), // buffered fast-path cancel queue
		drainBatchSize: 500, // P2: process 500 orders per batch during backlog drain
	}
}

func NewMatchingEngineWithTracker(batchWindow time.Duration, eventBus *eventbus.EventBus, tracker *CancelTracker) *MatchingEngine {
	return &MatchingEngine{
		intents:       make(map[string]*types.Intent, 1024),
		batchWindow:   batchWindow,
		minBatchSize:  100,
		eventBus:      eventBus,
		running:       false,
		triggerCh:     make(chan struct{}, 1),
		cancelCh:      make(chan string, 2048),
		drainBatchSize: 500,
		cancelTracker: tracker,
	}
}

// Start starts the matching engine batch processing loop
func (me *MatchingEngine) Start(ctx context.Context) {
	me.running = true
	me.lastBatchTime = time.Now()

	// P0+P2: Ultra-low-latency batch loop with cancel fast path and backlog drain
	currentInterval := me.batchWindow
	ticker := time.NewTicker(currentInterval)
	defer ticker.Stop()

	// Dedicated goroutine for cancel fast path (P1)
	cancelDone := make(chan struct{})
	go me.cancelProcessor(cancelDone)

	for me.running {
		select {
		case <-ticker.C:
			me.processBatch()
			
			// Adjust next interval based on queue depth
			pending := me.pendingCount.Load()

			// P2: Backlog drain mode uses larger batches to clear recovery queue fast
			if me.backlogMode.Load() && pending > 0 {
				newInterval := time.Millisecond // P0: 1ms floor
				if newInterval != currentInterval {
					currentInterval = newInterval
					ticker.Reset(currentInterval)
				}
				continue
			}

			if pending > 0 {
				// P0: Reduced floor from 5ms → 1ms
				newInterval := time.Duration(1+pending/50) * time.Millisecond
				if newInterval > me.batchWindow {
					newInterval = me.batchWindow
				}
				if newInterval != currentInterval {
					currentInterval = newInterval
					ticker.Reset(currentInterval)
				}
			} else {
				// Queue empty: use full batch window
				me.backlogMode.Store(false)
				if currentInterval != me.batchWindow {
					currentInterval = me.batchWindow
					ticker.Reset(currentInterval)
				}
			}
		case <-me.triggerCh:
			me.processBatch()
			pending := me.pendingCount.Load()

			// P2: Backlog mode stays aggressive
			if me.backlogMode.Load() && pending > 0 {
				currentInterval = time.Millisecond
				ticker.Reset(currentInterval)
				continue
			}

			// P0: 1ms floor instead of 5ms
			newInterval := time.Duration(1+pending/50) * time.Millisecond
			if newInterval > me.batchWindow {
				newInterval = me.batchWindow
			}
			if newInterval < time.Millisecond {
				newInterval = time.Millisecond
			}
			currentInterval = newInterval
			ticker.Reset(currentInterval)
		case <-ctx.Done():
			me.running = false
			close(me.cancelCh)
			<-cancelDone
			return
		}
	}
}

// cancelProcessor is a dedicated goroutine that processes cancels immediately (P1 fast path)
func (me *MatchingEngine) cancelProcessor(done chan<- struct{}) {
	defer close(done)
	for intentID := range me.cancelCh {
		// Fast-path: cancel without waiting for next batch cycle
		me.mu.Lock()
		if intent, exists := me.intents[intentID]; exists && intent.Status == "active" {
			intent.Status = "cancelled"
			me.pendingCount.Add(-1)
		}
		me.mu.Unlock()

		if me.cancelTracker != nil {
			now := time.Now()
			me.cancelTracker.RecordFastCancelComplete(intentID, now)
		}
	}
}

// AddIntent adds a new intent to the matching engine
func (me *MatchingEngine) AddIntent(intent *types.Intent) {
	me.mu.Lock()
	me.intents[intent.ID] = intent
	me.pendingCount.Add(1)
	me.mu.Unlock()

	// Always signal for immediate processing under adaptive micro-batching
	me.signalTrigger()
}

// signalTrigger sends non-blocking trigger signal
func (me *MatchingEngine) signalTrigger() {
	select {
	case me.triggerCh <- struct{}{}:
	default:
		// Already pending, skip
	}
}

// CancelIntent cancels an intent
func (me *MatchingEngine) CancelIntent(intentID string) error {
	me.mu.Lock()
	_, exists := me.intents[intentID]
	if !exists {
		me.mu.Unlock()
		return fmt.Errorf("intent not found: %s", intentID)
	}

	if me.cancelTracker != nil {
		me.cancelTracker.RecordCancelSubmit(intentID, time.Now())
	}
	me.mu.Unlock()

	// P1: Send to fast-path cancel processor instead of waiting for next batch
	select {
	case me.cancelCh <- intentID:
	default:
		// Channel full: fall back to batch processing
		me.mu.Lock()
		if me.intents[intentID].Status == "active" {
			me.intents[intentID].Status = "cancelled"
			me.pendingCount.Add(-1)
		}
		me.mu.Unlock()
		if me.cancelTracker != nil {
			me.cancelTracker.RecordFastCancelComplete(intentID, time.Now())
		}
	}
	return nil
}

// EnableBacklogMode activates aggressive batch draining for recovery (P2)
func (me *MatchingEngine) EnableBacklogMode() {
	me.backlogMode.Store(true)
	me.signalTrigger()
}

// DisableBacklogMode returns to normal adaptive batching
func (me *MatchingEngine) DisableBacklogMode() {
	me.backlogMode.Store(false)
}

// processBatch processes a single batch of intents
func (me *MatchingEngine) processBatch() {
	if me.cancelTracker != nil {
		me.cancelTracker.SetBatchStart(time.Now())
	}

	// Snapshot intents under lock, then release immediately
	me.mu.Lock()
	if len(me.intents) == 0 {
		me.pendingCount.Store(0)
		me.mu.Unlock()
		return
	}
	
	// Copy to local slice for processing
	localIntents := make([]*types.Intent, 0, len(me.intents))
	for _, intent := range me.intents {
		localIntents = append(localIntents, intent)
	}
	me.pendingCount.Add(-int64(len(localIntents)))
	me.mu.Unlock()

	// 1. Collect valid intents and prune inactive in single pass
	validIntents := me.collectAndPrune(localIntents)
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

// collectAndPrune merges collection and pruning into single pass
func (me *MatchingEngine) collectAndPrune(allIntents []*types.Intent) []*types.Intent {
	valid := make([]*types.Intent, 0, len(allIntents)/2)
	now := time.Now()

	for _, intent := range allIntents {
		switch intent.Status {
		case "cancelled", "filled":
			if me.cancelTracker != nil && intent.Status == "cancelled" {
				submittedBefore := false
				if t, ok := func() (time.Time, bool) {
					me.cancelTracker.mu.Lock()
					t, ok := me.cancelTracker.submittedAt[intent.ID]
					me.cancelTracker.mu.Unlock()
					return t, ok
				}(); ok {
					submittedBefore = t.Before(me.cancelTracker.GetBatchStart())
				}
				me.cancelTracker.RecordCancelInBatch(intent.ID, false, submittedBefore)
				me.cancelTracker.RecordCancelComplete(intent.ID, now)
			}
			me.removeIntent(intent.ID)
		case "expired":
			me.removeIntent(intent.ID)
		default:
			if now.After(intent.ExpiresAt) {
				intent.Status = "expired"
				me.removeIntent(intent.ID)
				continue
			}
			valid = append(valid, intent)
		}
	}

	return valid
}

// removeIntent deletes an intent from the engine (must be called outside lock)
func (me *MatchingEngine) removeIntent(id string) {
	me.mu.Lock()
	delete(me.intents, id)
	me.mu.Unlock()
}

// groupByMarket groups intents by market and outcome
func (me *MatchingEngine) groupByMarket(intents []*types.Intent) map[string][]*types.Intent {
	// Pre-count markets to size map correctly
	marketCount := 0
	seen := make(map[string]bool, len(intents)/4)
	for _, intent := range intents {
		key := fmt.Sprintf("%s:%d", intent.MarketID, intent.Outcome)
		if !seen[key] {
			seen[key] = true
			marketCount++
		}
	}
	
	groups := make(map[string][]*types.Intent, marketCount)
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
		return
	}

	// 4. Compute clearing price
	clearingPrice := me.computeClearingPrice(buyOrders, sellOrders)
	if clearingPrice == 0 {
		return
	}

	// 5. Allocate fills proportionally
	fills := me.allocateFills(buyOrders, sellOrders, clearingPrice)

	if len(fills) == 0 {
		return
	}

	// 6. Emit fills and create ledger deltas
	batchOpID := utils.GenerateOpID("batch")
	me.emitFills(fills, batchOpID)

	// 7. Update intent statuses
	fillTotals := make(map[string]int64, len(fills))
	for _, fill := range fills {
		fillTotals[fill.IntentID] += fill.Amount
	}
	
	// Update statuses under lock
	me.mu.Lock()
	for intentID, filledAmount := range fillTotals {
		if intent, ok := me.intents[intentID]; ok {
			if filledAmount >= intent.Amount {
				intent.Amount = 0
				intent.Status = "filled"
			} else {
				intent.Amount -= filledAmount
			}
		}
	}
	me.mu.Unlock()
}

// aggregateOrderbook aggregates intents into buy and sell orders
func (me *MatchingEngine) aggregateOrderbook(intents []*types.Intent) ([]types.Order, []types.Order) {
	// Pre-count sides for capacity hints
	var buyCount, sellCount int
	for _, intent := range intents {
		if intent.Side == "buy" {
			buyCount++
		} else {
			sellCount++
		}
	}
	
	buyOrders := make([]types.Order, 0, buyCount)
	sellOrders := make([]types.Order, 0, sellCount)

	for _, intent := range intents {
		if intent.Side == "buy" {
			buyOrders = append(buyOrders, types.Order{
				ID: intent.ID, UserID: intent.UserID, MarketID: intent.MarketID,
				Side: intent.Side, Price: intent.Price, Amount: intent.Amount,
				Outcome: intent.Outcome, Status: "pending", CreatedAt: intent.CreatedAt,
			})
		} else {
			sellOrders = append(sellOrders, types.Order{
				ID: intent.ID, UserID: intent.UserID, MarketID: intent.MarketID,
				Side: intent.Side, Price: intent.Price, Amount: intent.Amount,
				Outcome: intent.Outcome, Status: "pending", CreatedAt: intent.CreatedAt,
			})
		}

	}

	return buyOrders, sellOrders
}

// computeClearingPrice computes the price that maximizes matched volume
func (me *MatchingEngine) computeClearingPrice(buyOrders, sellOrders []types.Order) int64 {
	pricePoints := me.getAllPricePoints(buyOrders, sellOrders)
	if len(pricePoints) == 0 {
		return 0
	}

	sort.Slice(pricePoints, func(i, j int) bool { return pricePoints[i] < pricePoints[j] })

	buyLevels := me.aggregateAmountsByPrice(buyOrders)
	sellLevels := me.aggregateAmountsByPrice(sellOrders)

	supplyAtPrice := make(map[int64]int64, len(pricePoints))
	var cumulativeSupply int64
	for _, price := range pricePoints {
		cumulativeSupply += sellLevels[price]
		supplyAtPrice[price] = cumulativeSupply
	}

	var (
		bestPrice int64 = -1
		maxVolume int64
	)
	cumulativeDemand := int64(0)
	for i := len(pricePoints) - 1; i >= 0; i-- {
		price := pricePoints[i]
		cumulativeDemand += buyLevels[price]
		volume := utils.MinInt64(cumulativeDemand, supplyAtPrice[price])
		if volume > maxVolume || (volume == maxVolume && (bestPrice == -1 || price < bestPrice)) {
			maxVolume = volume
			bestPrice = price
		}
	}

	if bestPrice < 0 {
		return 0
	}
	return bestPrice
}

func (me *MatchingEngine) aggregateAmountsByPrice(orders []types.Order) map[int64]int64 {
	levels := make(map[int64]int64, len(orders))
	for _, order := range orders {
		levels[order.Price] += order.Amount
	}
	return levels
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

	buyAlloc := me.allocateProRataAmounts(eligibleBuys, matchedVolume, totalBuyAmount)
	sellAlloc := me.allocateProRataAmounts(eligibleSells, matchedVolume, totalSellAmount)

	fills = append(fills, me.createFills(eligibleBuys, buyAlloc, clearingPrice, "buy")...)
	fills = append(fills, me.createFills(eligibleSells, sellAlloc, clearingPrice, "sell")...)

	return fills
}

// filterEligibleBuys filters buy orders willing to pay clearing price or higher
func (me *MatchingEngine) filterEligibleBuys(buyOrders []types.Order, clearingPrice int64) []types.Order {
	eligible := make([]types.Order, 0, len(buyOrders)/2)
	for _, order := range buyOrders {
		if order.Price >= clearingPrice {
			eligible = append(eligible, order)
		}
	}
	return eligible
}

// filterEligibleSells filters sell orders willing to accept clearing price or lower
func (me *MatchingEngine) filterEligibleSells(sellOrders []types.Order, clearingPrice int64) []types.Order {
	eligible := make([]types.Order, 0, len(sellOrders)/2)
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

type allocationRemainder struct {
	OrderID   string
	Amount    int64
	Remainder int64
}

// allocateProRataAmounts allocates an exact matchedVolume using largest remainder.
func (me *MatchingEngine) allocateProRataAmounts(orders []types.Order, matchedVolume, totalAmount int64) map[string]int64 {
	allocation := make(map[string]int64, len(orders))
	if len(orders) == 0 || matchedVolume <= 0 || totalAmount <= 0 {
		return allocation
	}

	var allocated int64
	remainders := make([]allocationRemainder, 0, len(orders))
	for _, order := range orders {
		numerator := order.Amount * matchedVolume
		base := numerator / totalAmount
		rem := numerator % totalAmount
		if base > order.Amount {
			base = order.Amount
		}
		allocation[order.ID] = base
		allocated += base
		remainders = append(remainders, allocationRemainder{
			OrderID:   order.ID,
			Amount:    order.Amount,
			Remainder: rem,
		})
	}

	remaining := matchedVolume - allocated
	if remaining <= 0 {
		return allocation
	}

	sort.Slice(remainders, func(i, j int) bool {
		if remainders[i].Remainder == remainders[j].Remainder {
			if remainders[i].Amount == remainders[j].Amount {
				return remainders[i].OrderID < remainders[j].OrderID
			}
			return remainders[i].Amount > remainders[j].Amount
		}
		return remainders[i].Remainder > remainders[j].Remainder
	})

	for _, item := range remainders {
		if remaining == 0 {
			break
		}
		if allocation[item.OrderID] < item.Amount {
			allocation[item.OrderID]++
			remaining--
		}
	}

	return allocation
}

func (me *MatchingEngine) createFills(orders []types.Order, allocation map[string]int64, price int64, side string) []types.Fill {
	fills := make([]types.Fill, 0, len(orders)/2)
	for _, order := range orders {
		fillAmount := allocation[order.ID]
		if fillAmount <= 0 {
			continue
		}
		fills = append(fills, types.Fill{
			ID: utils.GenerateID(), IntentID: order.ID, UserID: order.UserID,
			MarketID: order.MarketID, Side: side, Price: price, Amount: fillAmount,
			Outcome: order.Outcome, Timestamp: time.Now(), OpID: utils.GenerateOpID("fill"),
		})
	}
	return fills
}

// emitFills publishes fills to event bus
func (me *MatchingEngine) emitFills(fills []types.Fill, batchOpID string) {
	// Batch publish all fills at once
	me.publishMu.Lock()
	for i := range fills {
		me.eventBus.Publish(types.EventTypeFillCreated, fills[i])
	}
	me.publishMu.Unlock()
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
			_ = fill // fill processed silently in production
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
	fmt.Println("Matching engine running (press Ctrl+C to stop)")
	select {}
}
