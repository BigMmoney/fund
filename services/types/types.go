package types

import "time"

// Account types
type Account struct {
	ID      string
	Balance int64
	Version int64
	Type    string // USDC, USDC_HOLD, OUTCOME, etc.
}

// Ledger entry
type LedgerEntry struct {
	DebitAccount  string
	CreditAccount string
	Amount        int64
	OpID          string
	Timestamp     time.Time
}

// Order and Intent types
type Intent struct {
	ID          string
	UserID      string
	MarketID    string
	Side        string // "buy" or "sell"
	Price       int64
	Amount      int64
	Outcome     int
	CreatedAt   time.Time
	ExpiresAt   time.Time
	Status      string // "pending", "filled", "cancelled", "expired"
}

type Order struct {
	ID        string
	UserID    string
	MarketID  string
	Side      string
	Price     int64
	Amount    int64
	Outcome   int
	Status    string
	CreatedAt time.Time
}

// Fill represents a matched trade
type Fill struct {
	ID         string
	IntentID   string
	UserID     string
	MarketID   string
	Side       string
	Price      int64
	Amount     int64
	Outcome    int
	Timestamp  time.Time
	OpID       string
}

// Market types
type Market struct {
	ID          string
	Name        string
	Description string
	Outcomes    []string
	State       MarketState
	CreatedAt   time.Time
	ResolvedAt  *time.Time
	WinningOutcome *int
}

type MarketState int

const (
	MarketStateOpen MarketState = iota
	MarketStateCloseOnly
	MarketStateClosed
	MarketStateProposed
	MarketStateFinalized
	MarketStateHalted
	MarketStateResolved
)

func (s MarketState) String() string {
	return [...]string{"OPEN", "CLOSE_ONLY", "CLOSED", "PROPOSED", "FINALIZED", "HALTED", "RESOLVED"}[s]
}

// Position represents user's position in a market
type Position struct {
	UserID      string
	MarketID    string
	Outcome     int
	Amount      int64
	AvgPrice    int64
	UpdatedAt   time.Time
}

// Balance represents user's available balance
type Balance struct {
	UserID    string
	Asset     string
	Available int64
	Hold      int64
	UpdatedAt time.Time
}

// Chain event types
type ChainEvent struct {
	ChainID     string
	TxHash      string
	LogIndex    int
	EventType   string // "deposit", "withdrawal_request", etc.
	UserID      string
	Amount      int64
	Confirmed   bool
	Invalidated bool
	Timestamp   time.Time
}

// Ledger delta for batch operations
type LedgerDelta struct {
	OpID      string
	Entries   []LedgerEntry
	Timestamp time.Time
}

// Event bus message types
type Event struct {
	Type      string
	Payload   interface{}
	Timestamp time.Time
}

const (
	EventTypeIntentReceived   = "intent.received"
	EventTypeIntentCancelled  = "intent.cancelled"
	EventTypeFillCreated      = "fill.created"
	EventTypeLedgerCommitted  = "ledger.committed"
	EventTypeLedgerRejected   = "ledger.rejected"
	EventTypeChainDeposit     = "chain.deposit"
	EventTypeChainWithdrawal  = "chain.withdrawal"
	EventTypeMarketStateChange = "market.state_change"
	EventTypeKillSwitch       = "kill_switch"
)

// Kill switch levels
type KillSwitchLevel int

const (
	KillSwitchL1 KillSwitchLevel = iota + 1 // Stop new positions
	KillSwitchL2                             // Stop withdrawals
	KillSwitchL3                             // Stop signing / chain tx
	KillSwitchL4                             // Read-only mode
)

func (k KillSwitchLevel) String() string {
	return [...]string{"", "L1_STOP_POSITIONS", "L2_STOP_WITHDRAWALS", "L3_STOP_CHAIN_TX", "L4_READ_ONLY"}[k]
}

// Error types
type RejectReason string

const (
	RejectInsufficientFunds RejectReason = "INSUFFICIENT_FUNDS"
	RejectVersionConflict   RejectReason = "VERSION_CONFLICT"
	RejectDuplicateOp       RejectReason = "DUPLICATE_OP"
	RejectInvalidEntry      RejectReason = "INVALID_ENTRY"
	RejectMarketClosed      RejectReason = "MARKET_CLOSED"
	RejectKillSwitch        RejectReason = "KILL_SWITCH_ACTIVE"
)
