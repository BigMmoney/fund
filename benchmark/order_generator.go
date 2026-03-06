package benchmark

import "fmt"

type Side string

const (
	Buy  Side = "buy"
	Sell Side = "sell"
)

type SyntheticOrder struct {
	ID       string
	UserID   string
	MarketID string
	Outcome  int
	Side     Side
	Price    int64
	Amount   int64
}

// GenerateBalancedOrders creates N buy/sell order pairs to guarantee matchability.
func GenerateBalancedOrders(
	pairs int,
	marketID string,
	outcome int,
	buyPrice int64,
	sellPrice int64,
	amount int64,
) []SyntheticOrder {
	if pairs <= 0 || amount <= 0 {
		return nil
	}

	orders := make([]SyntheticOrder, 0, pairs*2)
	for i := 0; i < pairs; i++ {
		orders = append(orders, SyntheticOrder{
			ID:       fmt.Sprintf("buy-%d", i),
			UserID:   fmt.Sprintf("ub-%d", i),
			MarketID: marketID,
			Outcome:  outcome,
			Side:     Buy,
			Price:    buyPrice,
			Amount:   amount,
		})
		orders = append(orders, SyntheticOrder{
			ID:       fmt.Sprintf("sell-%d", i),
			UserID:   fmt.Sprintf("us-%d", i),
			MarketID: marketID,
			Outcome:  outcome,
			Side:     Sell,
			Price:    sellPrice,
			Amount:   amount,
		})
	}
	return orders
}
