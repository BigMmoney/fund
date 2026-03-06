package main

import (
	"testing"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
)

func newTestRisk() *RiskService {
	return NewRiskService(eventbus.NewEventBus())
}

func TestRiskStateTransitionsAffectTradingPermission(t *testing.T) {
	rs := newTestRisk()
	marketID := "m-state-1"

	// Default for unknown market behaves as OPEN for state lookup.
	if !rs.CanTrade(marketID) {
		t.Fatalf("expected unknown market to be tradable under normal mode")
	}

	rs.SetMarketState(marketID, types.MarketStateCloseOnly)
	if rs.CanTrade(marketID) {
		t.Fatalf("close-only market must not allow new trading")
	}

	rs.SetMarketState(marketID, types.MarketStateClosed)
	if rs.CanTrade(marketID) {
		t.Fatalf("closed market must not allow trading")
	}

	rs.SetMarketState(marketID, types.MarketStateOpen)
	if !rs.CanTrade(marketID) {
		t.Fatalf("open market should allow trading when kill switch is inactive")
	}
}

func TestKillSwitchLevelsAreMonotonic(t *testing.T) {
	rs := newTestRisk()
	marketID := "m-kill-1"
	rs.SetMarketState(marketID, types.MarketStateOpen)

	rs.ActivateKillSwitch(types.KillSwitchL1, "test-l1")
	if rs.CanTrade(marketID) {
		t.Fatalf("L1 must block trading")
	}
	if !rs.CanWithdraw() {
		t.Fatalf("L1 should still allow withdrawals")
	}
	if !rs.CanSignChainTx() {
		t.Fatalf("L1 should still allow chain signing")
	}

	rs.ActivateKillSwitch(types.KillSwitchL2, "test-l2")
	if rs.CanWithdraw() {
		t.Fatalf("L2 must block withdrawals")
	}
	if !rs.CanSignChainTx() {
		t.Fatalf("L2 should still allow chain signing")
	}

	rs.ActivateKillSwitch(types.KillSwitchL3, "test-l3")
	if rs.CanSignChainTx() {
		t.Fatalf("L3 must block chain signing")
	}

	rs.ActivateKillSwitch(types.KillSwitchL4, "test-l4")
	if !rs.IsReadOnly() {
		t.Fatalf("L4 must set read-only mode")
	}

	rs.DeactivateKillSwitch()
	if rs.IsReadOnly() {
		t.Fatalf("kill switch deactivation should exit read-only mode")
	}
}

func TestRiskParameterTightenAndRelaxBounded(t *testing.T) {
	rs := newTestRisk()
	marketID := "m-risk-1"
	rs.InitializeRiskParams(marketID)

	for i := 0; i < 20; i++ {
		rs.TightenRiskParams(marketID, "stress")
	}
	params := rs.GetRiskParams(marketID)
	if params == nil {
		t.Fatalf("expected risk params for market")
	}

	if params.CurrentFee > 0.05 {
		t.Fatalf("fee cap violated: got %.4f", params.CurrentFee)
	}
	if params.BatchWindowMs > 2000 {
		t.Fatalf("batch window cap violated: got %d", params.BatchWindowMs)
	}
	if params.MaxPositionSize <= 0 {
		t.Fatalf("max position size must remain positive, got %d", params.MaxPositionSize)
	}

	rs.RelaxRiskParams(marketID)
	params = rs.GetRiskParams(marketID)
	if params.CurrentFee != params.BaseFee {
		t.Fatalf("relax must restore base fee")
	}
	if params.BatchWindowMs != 500 {
		t.Fatalf("relax must restore batch window to 500ms")
	}
	if params.MaxPositionSize != 100000 {
		t.Fatalf("relax must restore max position size to 100000")
	}
}
