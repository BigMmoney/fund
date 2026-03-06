package simulator

import (
	"sort"
)

func processImmediateBook(buys *[]Order, sells *[]Order, incoming Order, step int, fundamental int64) []Fill {
	fills := make([]Fill, 0)
	switch incoming.Side {
	case Buy:
		sortSellBook(sells)
		for incoming.Amount > 0 {
			idx := findEligibleSell(*sells, incoming)
			if idx < 0 {
				break
			}
			resting := (*sells)[idx]
			fillAmount := minInt64(incoming.Amount, resting.Amount)
			fills = append(fills, Fill{
				BuyerID:         incoming.AgentID,
				SellerID:        resting.AgentID,
				BuyerClass:      incoming.Class,
				SellerClass:     resting.Class,
				Price:           resting.Price,
				Amount:          fillAmount,
				FillStep:        step,
				BuyerArrival:    incoming.ArrivalStep,
				SellerArrival:   resting.ArrivalStep,
				FundamentalMark: fundamental,
			})
			incoming.Amount -= fillAmount
			(*sells)[idx].Amount -= fillAmount
			if (*sells)[idx].Amount == 0 {
				*sells = append((*sells)[:idx], (*sells)[idx+1:]...)
			}
		}
		if incoming.Amount > 0 {
			*buys = append(*buys, incoming)
			sortBuyBook(buys)
		}
	case Sell:
		sortBuyBook(buys)
		for incoming.Amount > 0 {
			idx := findEligibleBuy(*buys, incoming)
			if idx < 0 {
				break
			}
			resting := (*buys)[idx]
			fillAmount := minInt64(incoming.Amount, resting.Amount)
			fills = append(fills, Fill{
				BuyerID:         resting.AgentID,
				SellerID:        incoming.AgentID,
				BuyerClass:      resting.Class,
				SellerClass:     incoming.Class,
				Price:           resting.Price,
				Amount:          fillAmount,
				FillStep:        step,
				BuyerArrival:    resting.ArrivalStep,
				SellerArrival:   incoming.ArrivalStep,
				FundamentalMark: fundamental,
			})
			incoming.Amount -= fillAmount
			(*buys)[idx].Amount -= fillAmount
			if (*buys)[idx].Amount == 0 {
				*buys = append((*buys)[:idx], (*buys)[idx+1:]...)
			}
		}
		if incoming.Amount > 0 {
			*sells = append(*sells, incoming)
			sortSellBook(sells)
		}
	}
	return fills
}

func processBatchBook(buys *[]Order, sells *[]Order, step int, fundamental int64) []Fill {
	if len(*buys) == 0 || len(*sells) == 0 {
		return nil
	}

	clearingPrice, matchedVolume := computeBatchClearing(*buys, *sells)
	if matchedVolume == 0 {
		return nil
	}

	eligibleBuys := make([]Order, 0)
	for _, order := range *buys {
		if order.Price >= clearingPrice {
			eligibleBuys = append(eligibleBuys, order)
		}
	}
	eligibleSells := make([]Order, 0)
	for _, order := range *sells {
		if order.Price <= clearingPrice {
			eligibleSells = append(eligibleSells, order)
		}
	}

	buyAlloc := allocateProRata(eligibleBuys, matchedVolume)
	sellAlloc := allocateProRata(eligibleSells, matchedVolume)

	fills := make([]Fill, 0, len(eligibleBuys)+len(eligibleSells))
	for _, order := range eligibleBuys {
		if amount := buyAlloc[order.ID]; amount > 0 {
			fills = append(fills, Fill{
				BuyerID:         order.AgentID,
				BuyerClass:      order.Class,
				Price:           clearingPrice,
				Amount:          amount,
				FillStep:        step,
				BuyerArrival:    order.ArrivalStep,
				FundamentalMark: fundamental,
			})
		}
	}
	for _, order := range eligibleSells {
		if amount := sellAlloc[order.ID]; amount > 0 {
			fills = append(fills, Fill{
				SellerID:        order.AgentID,
				SellerClass:     order.Class,
				Price:           clearingPrice,
				Amount:          amount,
				FillStep:        step,
				SellerArrival:   order.ArrivalStep,
				FundamentalMark: fundamental,
			})
		}
	}

	paired := pairBatchFills(fills)
	updateResiduals(buys, buyAlloc)
	updateResiduals(sells, sellAlloc)
	return paired
}

func computeBatchClearing(buys, sells []Order) (int64, int64) {
	prices := make(map[int64]struct{})
	for _, buy := range buys {
		prices[buy.Price] = struct{}{}
	}
	for _, sell := range sells {
		prices[sell.Price] = struct{}{}
	}

	points := make([]int64, 0, len(prices))
	for price := range prices {
		points = append(points, price)
	}
	sort.Slice(points, func(i, j int) bool { return points[i] < points[j] })

	var bestPrice int64 = -1
	var bestVolume int64
	for _, price := range points {
		var demand, supply int64
		for _, buy := range buys {
			if buy.Price >= price {
				demand += buy.Amount
			}
		}
		for _, sell := range sells {
			if sell.Price <= price {
				supply += sell.Amount
			}
		}
		volume := minInt64(demand, supply)
		if volume > bestVolume || (volume == bestVolume && (bestPrice == -1 || price < bestPrice)) {
			bestPrice = price
			bestVolume = volume
		}
	}
	if bestPrice < 0 {
		return 0, 0
	}
	return bestPrice, bestVolume
}

type remainderEntry struct {
	ID        string
	Amount    int64
	Remainder int64
}

func allocateProRata(orders []Order, matchedVolume int64) map[string]int64 {
	allocation := make(map[string]int64, len(orders))
	if len(orders) == 0 || matchedVolume <= 0 {
		return allocation
	}
	var total int64
	for _, order := range orders {
		total += order.Amount
	}
	if total == 0 {
		return allocation
	}
	var allocated int64
	remainders := make([]remainderEntry, 0, len(orders))
	for _, order := range orders {
		numerator := order.Amount * matchedVolume
		base := numerator / total
		rem := numerator % total
		allocation[order.ID] = base
		allocated += base
		remainders = append(remainders, remainderEntry{ID: order.ID, Amount: order.Amount, Remainder: rem})
	}
	sort.Slice(remainders, func(i, j int) bool {
		if remainders[i].Remainder == remainders[j].Remainder {
			if remainders[i].Amount == remainders[j].Amount {
				return remainders[i].ID < remainders[j].ID
			}
			return remainders[i].Amount > remainders[j].Amount
		}
		return remainders[i].Remainder > remainders[j].Remainder
	})
	for remaining := matchedVolume - allocated; remaining > 0; remaining-- {
		item := remainders[(matchedVolume-allocated)-remaining]
		if allocation[item.ID] < item.Amount {
			allocation[item.ID]++
		}
	}
	return allocation
}

func pairBatchFills(sideFills []Fill) []Fill {
	buys := make([]Fill, 0)
	sells := make([]Fill, 0)
	for _, fill := range sideFills {
		switch {
		case fill.BuyerID != "":
			buys = append(buys, fill)
		case fill.SellerID != "":
			sells = append(sells, fill)
		}
	}
	sort.Slice(buys, func(i, j int) bool { return buys[i].BuyerArrival < buys[j].BuyerArrival })
	sort.Slice(sells, func(i, j int) bool { return sells[i].SellerArrival < sells[j].SellerArrival })

	paired := make([]Fill, 0)
	i, j := 0, 0
	for i < len(buys) && j < len(sells) {
		buy := &buys[i]
		sellIdx := j
		for sellIdx < len(sells) && sells[sellIdx].SellerID == buy.BuyerID {
			sellIdx++
		}
		if sellIdx >= len(sells) {
			i++
			continue
		}
		if sellIdx != j {
			sells[j], sells[sellIdx] = sells[sellIdx], sells[j]
		}
		sell := &sells[j]
		amount := minInt64(buy.Amount, sell.Amount)
		paired = append(paired, Fill{
			BuyerID:         buy.BuyerID,
			SellerID:        sell.SellerID,
			BuyerClass:      buy.BuyerClass,
			SellerClass:     sell.SellerClass,
			Price:           buy.Price,
			Amount:          amount,
			FillStep:        buy.FillStep,
			BuyerArrival:    buy.BuyerArrival,
			SellerArrival:   sell.SellerArrival,
			FundamentalMark: buy.FundamentalMark,
		})
		buy.Amount -= amount
		sell.Amount -= amount
		if buy.Amount == 0 {
			i++
		}
		if sell.Amount == 0 {
			j++
		}
	}
	return paired
}

func updateResiduals(book *[]Order, allocation map[string]int64) {
	residuals := make([]Order, 0, len(*book))
	for _, order := range *book {
		order.Amount -= allocation[order.ID]
		if order.Amount > 0 {
			residuals = append(residuals, order)
		}
	}
	*book = residuals
}

func sortBuyBook(book *[]Order) {
	sort.SliceStable(*book, func(i, j int) bool {
		if (*book)[i].Price == (*book)[j].Price {
			if (*book)[i].ArrivalStep == (*book)[j].ArrivalStep {
				return (*book)[i].ArrivalSeq < (*book)[j].ArrivalSeq
			}
			return (*book)[i].ArrivalStep < (*book)[j].ArrivalStep
		}
		return (*book)[i].Price > (*book)[j].Price
	})
}

func sortSellBook(book *[]Order) {
	sort.SliceStable(*book, func(i, j int) bool {
		if (*book)[i].Price == (*book)[j].Price {
			if (*book)[i].ArrivalStep == (*book)[j].ArrivalStep {
				return (*book)[i].ArrivalSeq < (*book)[j].ArrivalSeq
			}
			return (*book)[i].ArrivalStep < (*book)[j].ArrivalStep
		}
		return (*book)[i].Price < (*book)[j].Price
	})
}

func minInt64(a, b int64) int64 {
	if a < b {
		return a
	}
	return b
}

func findEligibleSell(book []Order, incoming Order) int {
	for idx, order := range book {
		if order.Price > incoming.Price {
			break
		}
		if order.AgentID != incoming.AgentID {
			return idx
		}
	}
	return -1
}

func findEligibleBuy(book []Order, incoming Order) int {
	for idx, order := range book {
		if order.Price < incoming.Price {
			break
		}
		if order.AgentID != incoming.AgentID {
			return idx
		}
	}
	return -1
}
