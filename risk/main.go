package main

import (
	"context"
	"log"
	"sync"
	"time"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
)

// RiskService manages market states and risk controls
type RiskService struct {
	mu           sync.RWMutex
	eventBus     *eventbus.EventBus
	marketStates map[string]types.MarketState
	killSwitch   types.KillSwitchLevel
	riskParams   map[string]*RiskParams
	running      bool
}

// RiskParams holds dynamic risk parameters for a market
type RiskParams struct {
	MaxPositionSize int64
	BaseFee         float64
	CurrentFee      float64
	BatchWindowMs   int
	MaxSlippage     float64
}

func NewRiskService(eventBus *eventbus.EventBus) *RiskService {
	return &RiskService{
		eventBus:     eventBus,
		marketStates: make(map[string]types.MarketState),
		killSwitch:   0, // No kill switch active
		riskParams:   make(map[string]*RiskParams),
		running:      false,
	}
}

// Start starts the risk service monitoring loop
func (rs *RiskService) Start(ctx context.Context) {
	rs.running = true
	log.Println("Risk service started")

	// Monitor risk metrics periodically
	ticker := time.NewTicker(5 * time.Second)
	defer ticker.Stop()

	for rs.running {
		select {
		case <-ticker.C:
			rs.monitorRiskMetrics()
		case <-ctx.Done():
			rs.running = false
			return
		}
	}
}

// SetMarketState sets the state of a market
func (rs *RiskService) SetMarketState(marketID string, state types.MarketState) {
	rs.mu.Lock()
	defer rs.mu.Unlock()

	oldState := rs.marketStates[marketID]
	rs.marketStates[marketID] = state

	log.Printf("Market %s state changed: %s -> %s", marketID, oldState.String(), state.String())

	// Publish state change event
	rs.eventBus.Publish(types.EventTypeMarketStateChange, map[string]interface{}{
		"market_id": marketID,
		"old_state": oldState.String(),
		"new_state": state.String(),
		"timestamp": time.Now(),
	})

	// Apply state-specific rules
	rs.applyStateRules(marketID, state)
}

// GetMarketState returns the current state of a market
func (rs *RiskService) GetMarketState(marketID string) types.MarketState {
	rs.mu.RLock()
	defer rs.mu.RUnlock()

	state, exists := rs.marketStates[marketID]
	if !exists {
		return types.MarketStateOpen // Default to open
	}
	return state
}

// applyStateRules applies rules based on market state
func (rs *RiskService) applyStateRules(marketID string, state types.MarketState) {
	switch state {
	case types.MarketStateCloseOnly:
		log.Printf("Market %s: No new positions allowed", marketID)
		// In production, signal to matching engine to reject new orders

	case types.MarketStateClosed:
		log.Printf("Market %s: Trading halted", marketID)
		// Reject all orders for this market

	case types.MarketStateFinalized:
		log.Printf("Market %s: Market resolved, initiating settlements", marketID)
		// Trigger settlement process
		rs.eventBus.Publish("market.settlement", map[string]interface{}{
			"market_id": marketID,
		})

	case types.MarketStateOpen:
		log.Printf("Market %s: Normal trading", marketID)
	}
}

// CanTrade checks if trading is allowed for a market
func (rs *RiskService) CanTrade(marketID string) bool {
	rs.mu.RLock()
	defer rs.mu.RUnlock()

	// Check kill switch
	if rs.killSwitch >= types.KillSwitchL1 {
		return false
	}

	// Check market state
	state := rs.marketStates[marketID]
	return state == types.MarketStateOpen
}

// CanWithdraw checks if withdrawals are allowed
func (rs *RiskService) CanWithdraw() bool {
	rs.mu.RLock()
	defer rs.mu.RUnlock()

	return rs.killSwitch < types.KillSwitchL2
}

// CanSignChainTx checks if chain transactions can be signed
func (rs *RiskService) CanSignChainTx() bool {
	rs.mu.RLock()
	defer rs.mu.RUnlock()

	return rs.killSwitch < types.KillSwitchL3
}

// IsReadOnly checks if system is in read-only mode
func (rs *RiskService) IsReadOnly() bool {
	rs.mu.RLock()
	defer rs.mu.RUnlock()

	return rs.killSwitch >= types.KillSwitchL4
}

// ActivateKillSwitch activates a kill switch level
func (rs *RiskService) ActivateKillSwitch(level types.KillSwitchLevel, reason string) {
	rs.mu.Lock()
	defer rs.mu.Unlock()

	rs.killSwitch = level
	log.Printf("KILL SWITCH ACTIVATED: %s - Reason: %s", level.String(), reason)

	// Publish kill switch event
	rs.eventBus.Publish(types.EventTypeKillSwitch, map[string]interface{}{
		"level":     level.String(),
		"reason":    reason,
		"timestamp": time.Now(),
	})

	// Execute level-specific actions
	rs.executeKillSwitchActions(level)
}

// DeactivateKillSwitch deactivates the kill switch
func (rs *RiskService) DeactivateKillSwitch() {
	rs.mu.Lock()
	defer rs.mu.Unlock()

	oldLevel := rs.killSwitch
	rs.killSwitch = 0
	log.Printf("Kill switch deactivated (was: %s)", oldLevel.String())
}

// executeKillSwitchActions executes actions for a kill switch level
func (rs *RiskService) executeKillSwitchActions(level types.KillSwitchLevel) {
	switch level {
	case types.KillSwitchL1:
		log.Println("L1: Stopping new positions")
		// Signal to matching engine to stop processing new intents

	case types.KillSwitchL2:
		log.Println("L2: Stopping withdrawals")
		// Signal to API gateway to reject withdrawal requests

	case types.KillSwitchL3:
		log.Println("L3: Stopping chain transactions")
		// Signal to indexer and tx-orchestrator to stop signing

	case types.KillSwitchL4:
		log.Println("L4: Entering read-only mode")
		// All write operations disabled
	}
}

// InitializeRiskParams initializes risk parameters for a market
func (rs *RiskService) InitializeRiskParams(marketID string) {
	rs.mu.Lock()
	defer rs.mu.Unlock()

	rs.riskParams[marketID] = &RiskParams{
		MaxPositionSize: 100000, // $1000 in cents
		BaseFee:         0.01,   // 1%
		CurrentFee:      0.01,
		BatchWindowMs:   500,
		MaxSlippage:     0.05, // 5%
	}

	log.Printf("Initialized risk params for market %s", marketID)
}

// GetRiskParams returns risk parameters for a market
func (rs *RiskService) GetRiskParams(marketID string) *RiskParams {
	rs.mu.RLock()
	defer rs.mu.RUnlock()

	params, exists := rs.riskParams[marketID]
	if !exists {
		return nil
	}
	return params
}

// TightenRiskParams dynamically tightens risk parameters
func (rs *RiskService) TightenRiskParams(marketID string, reason string) {
	rs.mu.Lock()
	defer rs.mu.Unlock()

	params, exists := rs.riskParams[marketID]
	if !exists {
		return
	}

	log.Printf("Tightening risk params for market %s: %s", marketID, reason)

	// Increase fees
	params.CurrentFee = params.CurrentFee * 1.5
	if params.CurrentFee > 0.05 {
		params.CurrentFee = 0.05 // Cap at 5%
	}

	// Reduce max position
	params.MaxPositionSize = int64(float64(params.MaxPositionSize) * 0.8)

	// Extend batch window (slow down trading)
	params.BatchWindowMs = int(float64(params.BatchWindowMs) * 1.2)
	if params.BatchWindowMs > 2000 {
		params.BatchWindowMs = 2000 // Cap at 2s
	}

	log.Printf("New risk params: fee=%.2f%%, max_pos=%d, batch=%dms",
		params.CurrentFee*100, params.MaxPositionSize, params.BatchWindowMs)
}

// RelaxRiskParams dynamically relaxes risk parameters
func (rs *RiskService) RelaxRiskParams(marketID string) {
	rs.mu.Lock()
	defer rs.mu.Unlock()

	params, exists := rs.riskParams[marketID]
	if !exists {
		return
	}

	log.Printf("Relaxing risk params for market %s", marketID)

	// Reset to base values
	params.CurrentFee = params.BaseFee
	params.MaxPositionSize = 100000
	params.BatchWindowMs = 500
}

// monitorRiskMetrics monitors risk metrics and takes action if needed
func (rs *RiskService) monitorRiskMetrics() {
	rs.mu.RLock()
	defer rs.mu.RUnlock()

	// In production, this would check:
	// - Trading volume anomalies
	// - Price volatility
	// - Liquidity imbalances
	// - System latency
	// - External oracle discrepancies

	// For now, just log status
	log.Printf("Risk monitoring: markets=%d, kill_switch=%s",
		len(rs.marketStates), rs.killSwitch.String())
}

// GetStatus returns risk service status
func (rs *RiskService) GetStatus() map[string]interface{} {
	rs.mu.RLock()
	defer rs.mu.RUnlock()

	marketStates := make(map[string]string)
	for marketID, state := range rs.marketStates {
		marketStates[marketID] = state.String()
	}

	return map[string]interface{}{
		"running":       rs.running,
		"kill_switch":   rs.killSwitch.String(),
		"market_states": marketStates,
		"num_markets":   len(rs.marketStates),
	}
}

func main() {
	log.Println("Risk service starting...")

	// Initialize event bus
	eventBus := eventbus.NewEventBus()

	// Create risk service
	riskService := NewRiskService(eventBus)

	// Subscribe to events
	stateChangeCh := eventBus.Subscribe(types.EventTypeMarketStateChange, 100)
	go func() {
		for event := range stateChangeCh {
			log.Printf("Market state change: %v", event.Payload)
		}
	}()

	killSwitchCh := eventBus.Subscribe(types.EventTypeKillSwitch, 10)
	go func() {
		for event := range killSwitchCh {
			log.Printf("KILL SWITCH EVENT: %v", event.Payload)
		}
	}()

	// Start risk service
	ctx := context.Background()
	go riskService.Start(ctx)

	// Initialize sample markets
	riskService.InitializeRiskParams("market1")
	riskService.InitializeRiskParams("market2")
	riskService.SetMarketState("market1", types.MarketStateOpen)
	riskService.SetMarketState("market2", types.MarketStateOpen)

	// Simulate state transitions
	time.Sleep(5 * time.Second)
	log.Println("\n=== Simulating state transitions ===")

	riskService.SetMarketState("market1", types.MarketStateCloseOnly)
	time.Sleep(2 * time.Second)

	riskService.TightenRiskParams("market2", "High volatility detected")
	time.Sleep(2 * time.Second)

	// Simulate kill switch
	log.Println("\n=== Simulating kill switch ===")
	riskService.ActivateKillSwitch(types.KillSwitchL1, "Suspicious activity detected")
	time.Sleep(3 * time.Second)

	log.Printf("Can trade: %v", riskService.CanTrade("market1"))
	log.Printf("Can withdraw: %v", riskService.CanWithdraw())
	log.Printf("Can sign chain tx: %v", riskService.CanSignChainTx())

	// Deactivate kill switch
	time.Sleep(2 * time.Second)
	riskService.DeactivateKillSwitch()

	// Status check
	status := riskService.GetStatus()
	log.Printf("\nRisk service status: %+v", status)

	// Keep service running
	log.Println("\nRisk service started on port :8084")
	select {}
}
