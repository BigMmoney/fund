package main

import (
	"testing"
	"time"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
	"pre_trading/services/utils"
)

func newTestLedger() *LedgerService {
	return NewLedgerService(eventbus.NewEventBus())
}

func totalBalance(ls *LedgerService) int64 {
	ls.mu.RLock()
	defer ls.mu.RUnlock()
	var total int64
	for _, acc := range ls.accounts {
		total += acc.Balance
	}
	return total
}

func TestLedgerConservationAndVersionIncrements(t *testing.T) {
	ls := newTestLedger()

	if err := ls.ProcessDeposit("alice", 1000, "deposit:alice:1"); err != nil {
		t.Fatalf("deposit failed: %v", err)
	}

	beforeTotal := totalBalance(ls)

	delta := types.LedgerDelta{
		OpID: "transfer:alice:bob:1",
		Entries: []types.LedgerEntry{
			{
				DebitAccount:  utils.FormatAccount("alice", "USDC"),
				CreditAccount: utils.FormatAccount("bob", "USDC"),
				Amount:        400,
				OpID:          "transfer:alice:bob:1",
				Timestamp:     time.Now(),
			},
		},
		Timestamp: time.Now(),
	}
	if err := ls.CommitDelta(delta); err != nil {
		t.Fatalf("commit failed: %v", err)
	}

	if got := ls.GetBalance(utils.FormatAccount("alice", "USDC")); got != 600 {
		t.Fatalf("alice balance mismatch, want 600, got %d", got)
	}
	if got := ls.GetBalance(utils.FormatAccount("bob", "USDC")); got != 400 {
		t.Fatalf("bob balance mismatch, want 400, got %d", got)
	}
	if afterTotal := totalBalance(ls); afterTotal != beforeTotal {
		t.Fatalf("ledger conservation violated, before=%d after=%d", beforeTotal, afterTotal)
	}

	ls.mu.RLock()
	defer ls.mu.RUnlock()
	if ls.accounts[utils.FormatAccount("alice", "USDC")].Version != 2 {
		t.Fatalf("alice version should be 2 after deposit+transfer")
	}
	if ls.accounts[utils.FormatAccount("bob", "USDC")].Version != 1 {
		t.Fatalf("bob version should be 1 after first transfer")
	}
}

func TestLedgerReplayIdempotency_DuplicateOpRejectedWithoutMutation(t *testing.T) {
	ls := newTestLedger()
	if err := ls.ProcessDeposit("alice", 500, "deposit:alice:2"); err != nil {
		t.Fatalf("seed deposit failed: %v", err)
	}

	delta := types.LedgerDelta{
		OpID: "transfer:alice:bob:dup",
		Entries: []types.LedgerEntry{
			{
				DebitAccount:  utils.FormatAccount("alice", "USDC"),
				CreditAccount: utils.FormatAccount("bob", "USDC"),
				Amount:        300,
				OpID:          "transfer:alice:bob:dup",
				Timestamp:     time.Now(),
			},
		},
		Timestamp: time.Now(),
	}

	if err := ls.CommitDelta(delta); err != nil {
		t.Fatalf("first commit failed: %v", err)
	}
	aliceBefore := ls.GetBalance(utils.FormatAccount("alice", "USDC"))
	bobBefore := ls.GetBalance(utils.FormatAccount("bob", "USDC"))
	totalBefore := totalBalance(ls)

	if err := ls.CommitDelta(delta); err == nil {
		t.Fatalf("expected duplicate op rejection")
	}

	if got := ls.GetBalance(utils.FormatAccount("alice", "USDC")); got != aliceBefore {
		t.Fatalf("alice balance mutated after duplicate replay, want %d got %d", aliceBefore, got)
	}
	if got := ls.GetBalance(utils.FormatAccount("bob", "USDC")); got != bobBefore {
		t.Fatalf("bob balance mutated after duplicate replay, want %d got %d", bobBefore, got)
	}
	if got := totalBalance(ls); got != totalBefore {
		t.Fatalf("total balance mutated after duplicate replay, want %d got %d", totalBefore, got)
	}
}

func TestLedgerRejectsInvalidEntries(t *testing.T) {
	ls := newTestLedger()

	invalids := []types.LedgerDelta{
		{
			OpID:      "invalid:empty",
			Entries:   nil,
			Timestamp: time.Now(),
		},
		{
			OpID: "invalid:negative",
			Entries: []types.LedgerEntry{
				{
					DebitAccount:  "U:a:USDC",
					CreditAccount: "U:b:USDC",
					Amount:        -1,
					OpID:          "invalid:negative",
					Timestamp:     time.Now(),
				},
			},
			Timestamp: time.Now(),
		},
		{
			OpID: "invalid:self-transfer",
			Entries: []types.LedgerEntry{
				{
					DebitAccount:  "U:a:USDC",
					CreditAccount: "U:a:USDC",
					Amount:        1,
					OpID:          "invalid:self-transfer",
					Timestamp:     time.Now(),
				},
			},
			Timestamp: time.Now(),
		},
	}

	for _, delta := range invalids {
		if err := ls.CommitDelta(delta); err == nil {
			t.Fatalf("expected rejection for delta %s", delta.OpID)
		}
	}
}
