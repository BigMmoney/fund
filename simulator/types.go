package simulator

import "time"

type MatchingMode string

const (
	ModeImmediate MatchingMode = "immediate"
	ModeBatch     MatchingMode = "batch"
	ModeSpeedBump MatchingMode = "speed_bump"
)

type AgentClass string

const (
	AgentMarketMaker AgentClass = "market_maker"
	AgentArbitrageur AgentClass = "latency_arbitrageur"
	AgentRetail      AgentClass = "retail"
	AgentInformed    AgentClass = "informed"
)

type Side string

const (
	Buy  Side = "buy"
	Sell Side = "sell"
)

type AgentConfig struct {
	ID           string
	Class        AgentClass
	LatencyTier  int
	BaseSize     int64
	QuoteWidth   int64
	Intensity    int
	InitialCash  int64
	InitialUnits int64
}

type RiskConfig struct {
	MaxOrderAmount   int64
	MaxOrdersPerStep int
}

type ScenarioConfig struct {
	Name             string
	Mode             MatchingMode
	BatchWindowSteps int
	SpeedBumpSteps   int
	StepDuration     time.Duration
	TotalSteps       int
	Seed             int64
	Agents           []AgentConfig
	Risk             RiskConfig
}

type Order struct {
	ID          string
	AgentID     string
	Class       AgentClass
	Side        Side
	Price       int64
	Amount      int64
	ArrivalStep int
	ArrivalSeq  int64
}

type Fill struct {
	BuyerID         string
	SellerID        string
	BuyerClass      AgentClass
	SellerClass     AgentClass
	Price           int64
	Amount          int64
	FillStep        int
	BuyerArrival    int
	SellerArrival   int
	FundamentalMark int64
}

type AccountState struct {
	Cash  int64
	Units int64
}

type BenchmarkResult struct {
	Name                    string        `json:"name"`
	Mode                    MatchingMode  `json:"mode"`
	BatchWindowMs           int           `json:"batch_window_ms"`
	SpeedBumpMs             int           `json:"speed_bump_ms"`
	Seed                    int64         `json:"seed"`
	OrdersSubmitted         int           `json:"orders_submitted"`
	OrdersAccepted          int           `json:"orders_accepted"`
	Fills                   int           `json:"fills"`
	OrdersPerSec            float64       `json:"orders_per_sec"`
	FillsPerSec             float64       `json:"fills_per_sec"`
	P50LatencyMs            float64       `json:"p50_latency_ms"`
	P95LatencyMs            float64       `json:"p95_latency_ms"`
	P99LatencyMs            float64       `json:"p99_latency_ms"`
	AverageSpread           float64       `json:"average_spread"`
	AveragePriceImpact      float64       `json:"average_price_impact"`
	QueuePriorityAdvantage  float64       `json:"queue_priority_advantage"`
	LatencyArbitrageProfit  float64       `json:"latency_arbitrage_profit"`
	ExecutionDispersion     float64       `json:"execution_dispersion"`
	NegativeBalanceViolations int         `json:"negative_balance_violations"`
	ConservationBreaches    int           `json:"conservation_breaches"`
	RiskRejections          int           `json:"risk_rejections"`
	Elapsed                 time.Duration `json:"-"`
}
