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

// IndexerService monitors blockchain events and handles reorgs
type IndexerService struct {
	mu                sync.RWMutex
	eventBus          *eventbus.EventBus
	confirmedEvents   map[string]*types.ChainEvent // opID -> event
	pendingEvents     map[string]*types.ChainEvent // opID -> event
	confirmationCount int
	lastBlockNumber   int64
	running           bool
}

func NewIndexerService(eventBus *eventbus.EventBus, confirmationCount int) *IndexerService {
	return &IndexerService{
		eventBus:          eventBus,
		confirmedEvents:   make(map[string]*types.ChainEvent),
		pendingEvents:     make(map[string]*types.ChainEvent),
		confirmationCount: confirmationCount,
		lastBlockNumber:   0,
		running:           false,
	}
}

// Start starts the indexer service
func (is *IndexerService) Start(ctx context.Context) {
	is.running = true
	log.Printf("Indexer service started with %d confirmation blocks", is.confirmationCount)

	// Simulate blockchain polling
	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()

	for is.running {
		select {
		case <-ticker.C:
			is.pollChain()
		case <-ctx.Done():
			is.running = false
			return
		}
	}
}

// pollChain simulates polling blockchain for new events
func (is *IndexerService) pollChain() {
	is.mu.Lock()
	defer is.mu.Unlock()

	// Simulate finding new events
	// In production, this would query the blockchain via RPC

	// Check for confirmations on pending events
	for opID, event := range is.pendingEvents {
		if is.isConfirmed(event) {
			log.Printf("Event confirmed: %s", opID)
			is.commitEventToLedger(event)
			is.confirmedEvents[opID] = event
			delete(is.pendingEvents, opID)
		}
	}

	// Simulate detecting reorgs
	if is.shouldCheckReorg() {
		is.checkForReorg()
	}

	is.lastBlockNumber++
}

// ObserveEvent processes a new blockchain event
func (is *IndexerService) ObserveEvent(event *types.ChainEvent) {
	is.mu.Lock()
	defer is.mu.Unlock()

	opID := utils.GenerateChainOpID(event.ChainID, event.TxHash, event.LogIndex)
	event.Timestamp = time.Now()

	log.Printf("Observed chain event: %s (type: %s, user: %s, amount: %d)",
		opID, event.EventType, event.UserID, event.Amount)

	is.pendingEvents[opID] = event
}

// isConfirmed checks if an event has enough confirmations
func (is *IndexerService) isConfirmed(event *types.ChainEvent) bool {
	// In production, compare event block number with current block number
	// For now, simulate with time-based confirmation
	confirmationAge := time.Since(event.Timestamp)
	requiredAge := time.Duration(is.confirmationCount) * 2 * time.Second // 2s per block
	return confirmationAge >= requiredAge
}

// commitEventToLedger sends confirmed event to ledger service
func (is *IndexerService) commitEventToLedger(event *types.ChainEvent) {
	opID := utils.GenerateChainOpID(event.ChainID, event.TxHash, event.LogIndex)

	switch event.EventType {
	case "deposit":
		// Create ledger delta for deposit
		is.eventBus.Publish(types.EventTypeChainDeposit, map[string]interface{}{
			"op_id":   opID,
			"user_id": event.UserID,
			"amount":  event.Amount,
		})
		log.Printf("Committed deposit: user=%s, amount=%d", event.UserID, event.Amount)

	case "withdrawal":
		// Handle withdrawal confirmation
		is.eventBus.Publish(types.EventTypeChainWithdrawal, map[string]interface{}{
			"op_id":   opID,
			"user_id": event.UserID,
			"amount":  event.Amount,
			"status":  "confirmed",
		})
		log.Printf("Committed withdrawal: user=%s, amount=%d", event.UserID, event.Amount)

	default:
		log.Printf("Unknown event type: %s", event.EventType)
	}

	event.Confirmed = true
}

// shouldCheckReorg determines if we should check for reorgs
func (is *IndexerService) shouldCheckReorg() bool {
	// Check every 10 blocks
	return is.lastBlockNumber%10 == 0
}

// checkForReorg checks for blockchain reorganizations
func (is *IndexerService) checkForReorg() {
	// In production, compare block hashes with what we have stored
	// For now, simulate occasional reorgs (1% chance)

	if !is.simulateReorgDetection() {
		return
	}

	log.Println("WARNING: Blockchain reorg detected!")

	// Find events affected by reorg (last N blocks)
	affectedOpIDs := is.findAffectedEvents()

	for _, opID := range affectedOpIDs {
		event, exists := is.confirmedEvents[opID]
		if !exists {
			continue
		}

		log.Printf("Invalidating event due to reorg: %s", opID)

		// Mark event as invalidated
		event.Invalidated = true

		// Emit reverse ledger delta
		is.emitReverseDelta(event)

		// Remove from confirmed events
		delete(is.confirmedEvents, opID)
	}

	// Trigger reconciliation
	is.eventBus.Publish("reorg.detected", map[string]interface{}{
		"affected_events": len(affectedOpIDs),
		"timestamp":       time.Now(),
	})
}

// simulateReorgDetection simulates reorg detection (1% chance)
func (is *IndexerService) simulateReorgDetection() bool {
	// In production, this would compare actual block hashes
	return false // Disabled for demo stability
}

// findAffectedEvents finds events affected by a reorg
func (is *IndexerService) findAffectedEvents() []string {
	affected := make([]string, 0)

	// Find recently confirmed events (within reorg window)
	reorgWindow := time.Duration(is.confirmationCount) * 2 * time.Second

	for opID, event := range is.confirmedEvents {
		if time.Since(event.Timestamp) < reorgWindow {
			affected = append(affected, opID)
		}
	}

	return affected
}

// emitReverseDelta emits a reverse ledger delta to undo an event
func (is *IndexerService) emitReverseDelta(event *types.ChainEvent) {
	opID := utils.GenerateChainOpID(event.ChainID, event.TxHash, event.LogIndex)
	reverseOpID := fmt.Sprintf("reverse:%s", opID)

	switch event.EventType {
	case "deposit":
		// Reverse deposit: credit vault, debit user
		is.eventBus.Publish("ledger.reverse", map[string]interface{}{
			"op_id":        reverseOpID,
			"original_op":  opID,
			"user_id":      event.UserID,
			"amount":       event.Amount,
			"reverse_type": "deposit",
		})
		log.Printf("Emitted reverse delta for deposit: %s", opID)

	case "withdrawal":
		// Reverse withdrawal: debit vault, credit user
		is.eventBus.Publish("ledger.reverse", map[string]interface{}{
			"op_id":        reverseOpID,
			"original_op":  opID,
			"user_id":      event.UserID,
			"amount":       event.Amount,
			"reverse_type": "withdrawal",
		})
		log.Printf("Emitted reverse delta for withdrawal: %s", opID)
	}
}

// GetStatus returns indexer status
func (is *IndexerService) GetStatus() map[string]interface{} {
	is.mu.RLock()
	defer is.mu.RUnlock()

	return map[string]interface{}{
		"running":          is.running,
		"last_block":       is.lastBlockNumber,
		"pending_events":   len(is.pendingEvents),
		"confirmed_events": len(is.confirmedEvents),
		"confirmations":    is.confirmationCount,
	}
}

// SimulateDeposit simulates a deposit event from the blockchain
func (is *IndexerService) SimulateDeposit(userID string, amount int64) {
	event := &types.ChainEvent{
		ChainID:     "1",
		TxHash:      fmt.Sprintf("0x%s", utils.GenerateID()[:40]),
		LogIndex:    0,
		EventType:   "deposit",
		UserID:      userID,
		Amount:      amount,
		Confirmed:   false,
		Invalidated: false,
		Timestamp:   time.Now(),
	}

	is.ObserveEvent(event)
}

// SimulateWithdrawal simulates a withdrawal event from the blockchain
func (is *IndexerService) SimulateWithdrawal(userID string, amount int64) {
	event := &types.ChainEvent{
		ChainID:     "1",
		TxHash:      fmt.Sprintf("0x%s", utils.GenerateID()[:40]),
		LogIndex:    0,
		EventType:   "withdrawal",
		UserID:      userID,
		Amount:      amount,
		Confirmed:   false,
		Invalidated: false,
		Timestamp:   time.Now(),
	}

	is.ObserveEvent(event)
}

func main() {
	log.Println("Indexer service starting...")

	// Initialize event bus
	eventBus := eventbus.NewEventBus()

	// Create indexer with 6 confirmations
	indexer := NewIndexerService(eventBus, 6)

	// Subscribe to chain events
	depositCh := eventBus.Subscribe(types.EventTypeChainDeposit, 100)
	go func() {
		for event := range depositCh {
			log.Printf("Deposit event: %v", event.Payload)
		}
	}()

	withdrawalCh := eventBus.Subscribe(types.EventTypeChainWithdrawal, 100)
	go func() {
		for event := range withdrawalCh {
			log.Printf("Withdrawal event: %v", event.Payload)
		}
	}()

	reorgCh := eventBus.Subscribe("reorg.detected", 10)
	go func() {
		for event := range reorgCh {
			log.Printf("REORG DETECTED: %v", event.Payload)
		}
	}()

	// Start indexer in background
	ctx := context.Background()
	go indexer.Start(ctx)

	// Simulate some deposits
	time.Sleep(1 * time.Second)
	indexer.SimulateDeposit("user1", 1000)
	indexer.SimulateDeposit("user2", 2000)

	// Status check goroutine
	go func() {
		ticker := time.NewTicker(10 * time.Second)
		defer ticker.Stop()
		for range ticker.C {
			status := indexer.GetStatus()
			log.Printf("Indexer status: %+v", status)
		}
	}()

	// Keep service running
	log.Println("Indexer service started on port :8083")
	select {}
}
