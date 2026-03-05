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

// LedgerService implements double-entry accounting with strict validation
type LedgerService struct {
	mu           sync.RWMutex
	accounts     map[string]*types.Account
	seenOpIDs    map[string]bool
	eventBus     *eventbus.EventBus
	walLog       []types.LedgerDelta // Write-Ahead Log for recovery
}

func NewLedgerService(eventBus *eventbus.EventBus) *LedgerService {
	return &LedgerService{
		accounts:  make(map[string]*types.Account),
		seenOpIDs: make(map[string]bool),
		eventBus:  eventBus,
		walLog:    make([]types.LedgerDelta, 0),
	}
}

// CommitDelta commits a ledger delta atomically
func (ls *LedgerService) CommitDelta(delta types.LedgerDelta) error {
	ls.mu.Lock()
	defer ls.mu.Unlock()

	// 1. Check op_id not seen
	if ls.seenOpIDs[delta.OpID] {
		ls.publishRejection(delta.OpID, types.RejectDuplicateOp)
		return fmt.Errorf("duplicate op_id: %s", delta.OpID)
	}

	// 2. Validate entries balanced
	if err := ls.validateBalance(delta.Entries); err != nil {
		ls.publishRejection(delta.OpID, types.RejectInvalidEntry)
		return err
	}

	// 3. Lock all affected accounts (collect them)
	affectedAccounts := ls.getAffectedAccounts(delta.Entries)

	// 4. Verify sufficient balance for all debits
	if err := ls.verifySufficientBalance(delta.Entries, affectedAccounts); err != nil {
		ls.publishRejection(delta.OpID, types.RejectInsufficientFunds)
		return err
	}

	// 5. Apply entries atomically
	ls.applyEntries(delta.Entries, affectedAccounts)

	// 6. Increment account versions
	for _, acc := range affectedAccounts {
		acc.Version++
	}

	// 7. Mark op_id as seen
	ls.seenOpIDs[delta.OpID] = true

	// 8. Append to WAL
	ls.walLog = append(ls.walLog, delta)

	// 9. Emit LedgerCommitted event
	ls.eventBus.Publish(types.EventTypeLedgerCommitted, delta)

	log.Printf("Ledger committed: op_id=%s, entries=%d", delta.OpID, len(delta.Entries))
	return nil
}

// validateBalance checks that debits equal credits
func (ls *LedgerService) validateBalance(entries []types.LedgerEntry) error {
	var sumDebits, sumCredits int64
	for _, e := range entries {
		sumDebits += e.Amount
		sumCredits += e.Amount
	}
	if sumDebits != sumCredits {
		return fmt.Errorf("debits and credits not balanced: debits=%d, credits=%d", sumDebits, sumCredits)
	}
	return nil
}

// getAffectedAccounts collects all accounts involved in the entries
func (ls *LedgerService) getAffectedAccounts(entries []types.LedgerEntry) map[string]*types.Account {
	accounts := make(map[string]*types.Account)
	for _, e := range entries {
		if _, exists := ls.accounts[e.DebitAccount]; !exists {
			ls.accounts[e.DebitAccount] = &types.Account{
				ID:      e.DebitAccount,
				Balance: 0,
				Version: 0,
			}
		}
		if _, exists := ls.accounts[e.CreditAccount]; !exists {
			ls.accounts[e.CreditAccount] = &types.Account{
				ID:      e.CreditAccount,
				Balance: 0,
				Version: 0,
			}
		}
		accounts[e.DebitAccount] = ls.accounts[e.DebitAccount]
		accounts[e.CreditAccount] = ls.accounts[e.CreditAccount]
	}
	return accounts
}

// verifySufficientBalance checks that all debit accounts have sufficient balance
func (ls *LedgerService) verifySufficientBalance(entries []types.LedgerEntry, accounts map[string]*types.Account) error {
	balanceChanges := make(map[string]int64)
	for _, e := range entries {
		balanceChanges[e.DebitAccount] -= e.Amount
		balanceChanges[e.CreditAccount] += e.Amount
	}

	for accountID, change := range balanceChanges {
		newBalance := accounts[accountID].Balance + change
		if newBalance < 0 {
			return fmt.Errorf("insufficient balance: account=%s, balance=%d, change=%d",
				accountID, accounts[accountID].Balance, change)
		}
	}
	return nil
}

// applyEntries applies the entries to accounts
func (ls *LedgerService) applyEntries(entries []types.LedgerEntry, accounts map[string]*types.Account) {
	for _, e := range entries {
		accounts[e.DebitAccount].Balance -= e.Amount
		accounts[e.CreditAccount].Balance += e.Amount
	}
}

// publishRejection publishes a ledger rejection event
func (ls *LedgerService) publishRejection(opID string, reason types.RejectReason) {
	ls.eventBus.Publish(types.EventTypeLedgerRejected, map[string]interface{}{
		"op_id":  opID,
		"reason": reason,
	})
}

// GetBalance returns the balance of an account
func (ls *LedgerService) GetBalance(accountID string) int64 {
	ls.mu.RLock()
	defer ls.mu.RUnlock()

	if acc, exists := ls.accounts[accountID]; exists {
		return acc.Balance
	}
	return 0
}

// CreateHold creates a hold on an account
func (ls *LedgerService) CreateHold(userID string, amount int64, opID string) error {
	delta := types.LedgerDelta{
		OpID: opID,
		Entries: []types.LedgerEntry{
			{
				DebitAccount:  utils.FormatAccount(userID, "USDC"),
				CreditAccount: utils.FormatAccount(userID, "USDC:HOLD"),
				Amount:        amount,
				OpID:          opID,
				Timestamp:     time.Now(),
			},
		},
		Timestamp: time.Now(),
	}
	return ls.CommitDelta(delta)
}

// ReleaseHold releases a hold on an account
func (ls *LedgerService) ReleaseHold(userID string, amount int64, opID string) error {
	delta := types.LedgerDelta{
		OpID: opID,
		Entries: []types.LedgerEntry{
			{
				DebitAccount:  utils.FormatAccount(userID, "USDC:HOLD"),
				CreditAccount: utils.FormatAccount(userID, "USDC"),
				Amount:        amount,
				OpID:          opID,
				Timestamp:     time.Now(),
			},
		},
		Timestamp: time.Now(),
	}
	return ls.CommitDelta(delta)
}

// ProcessDeposit processes a deposit from chain
func (ls *LedgerService) ProcessDeposit(userID string, amount int64, opID string) error {
	delta := types.LedgerDelta{
		OpID: opID,
		Entries: []types.LedgerEntry{
			{
				DebitAccount:  "SYS:ONCHAIN_VAULT:USDC",
				CreditAccount: utils.FormatAccount(userID, "USDC"),
				Amount:        amount,
				OpID:          opID,
				Timestamp:     time.Now(),
			},
		},
		Timestamp: time.Now(),
	}
	return ls.CommitDelta(delta)
}

func main() {
	fmt.Println("Ledger service starting...")

	// Initialize event bus
	eventBus := eventbus.NewEventBus()

	// Create ledger service
	ledger := NewLedgerService(eventBus)

	// Subscribe to ledger events
	commitCh := eventBus.Subscribe(types.EventTypeLedgerCommitted, 100)
	rejectCh := eventBus.Subscribe(types.EventTypeLedgerRejected, 100)

	// Start event listeners
	go func() {
		for event := range commitCh {
			log.Printf("Ledger committed: %v", event.Payload)
		}
	}()

	go func() {
		for event := range rejectCh {
			log.Printf("Ledger rejected: %v", event.Payload)
		}
	}()

	// Example: Process a deposit
	ctx := context.Background()
	_ = ctx

	opID := utils.GenerateOpID("deposit")
	err := ledger.ProcessDeposit("user1", 1000, opID)
	if err != nil {
		log.Printf("Deposit failed: %v", err)
	} else {
		log.Printf("Deposit successful, balance: %d", ledger.GetBalance("U:user1:USDC"))
	}

	// Example: Create a hold
	holdOpID := utils.GenerateOpID("hold")
	err = ledger.CreateHold("user1", 500, holdOpID)
	if err != nil {
		log.Printf("Hold failed: %v", err)
	} else {
		log.Printf("Hold successful, available: %d, hold: %d",
			ledger.GetBalance("U:user1:USDC"),
			ledger.GetBalance("U:user1:USDC:HOLD"))
	}

	// Keep service running
	log.Println("Ledger service started on port :8081")
	select {}
}
