package main

import (
	"encoding/json"
	"fmt"
	"log"
	"math"
	"math/rand"
	"net/http"
	"sync"
	"time"
)

// HFT 高频交易价格服务 - 支持毫秒级更新 (SSE 版本，无需外部依赖)

// TickData 代表一个价格tick
type TickData struct {
	Symbol       string  `json:"symbol"`
	Name         string  `json:"name"`
	Price        float64 `json:"price"`
	Bid          float64 `json:"bid"`
	Ask          float64 `json:"ask"`
	BidSize      float64 `json:"bidSize"`
	AskSize      float64 `json:"askSize"`
	Change       float64 `json:"change"`
	ChangePerc   float64 `json:"changePercent"`
	Volume       float64 `json:"volume"`
	High24h      float64 `json:"high24h"`
	Low24h       float64 `json:"low24h"`
	Timestamp    int64   `json:"timestamp"`
	TimestampStr string  `json:"timestampStr"`
}

// 加密货币基准价格
var cryptoBasePrices = map[string]struct {
	Name  string
	Price float64
}{
	"BTC":   {"Bitcoin", 95500.0},
	"ETH":   {"Ethereum", 3285.0},
	"SOL":   {"Solana", 198.0},
	"BNB":   {"BNB", 685.0},
	"XRP":   {"XRP", 2.35},
	"ADA":   {"Cardano", 0.98},
	"DOGE":  {"Dogecoin", 0.385},
	"DOT":   {"Polkadot", 7.85},
	"MATIC": {"Polygon", 0.52},
	"AVAX":  {"Avalanche", 38.50},
	"LINK":  {"Chainlink", 22.45},
	"UNI":   {"Uniswap", 12.85},
}

// 股票基准价格
var stockBasePrices = map[string]struct {
	Name  string
	Price float64
}{
	"AAPL":  {"Apple Inc.", 178.50},
	"MSFT":  {"Microsoft Corp.", 385.20},
	"GOOGL": {"Alphabet Inc.", 142.80},
	"AMZN":  {"Amazon.com Inc.", 165.30},
	"TSLA":  {"Tesla Inc.", 245.75},
	"NVDA":  {"NVIDIA Corp.", 485.60},
	"META":  {"Meta Platforms", 395.20},
	"JPM":   {"JPMorgan Chase", 158.90},
	"V":     {"Visa Inc.", 268.40},
	"WMT":   {"Walmart Inc.", 165.50},
}

// PriceEngine 价格引擎
type PriceEngine struct {
	mu           sync.RWMutex
	cryptoPrices map[string]*TickData
	stockPrices  map[string]*TickData
	updateCount  int64
}

var engine = &PriceEngine{
	cryptoPrices: make(map[string]*TickData),
	stockPrices:  make(map[string]*TickData),
}

// 初始化价格
func (e *PriceEngine) initPrices() {
	e.mu.Lock()
	defer e.mu.Unlock()

	now := time.Now()
	for symbol, info := range cryptoBasePrices {
		e.cryptoPrices[symbol] = &TickData{
			Symbol:       symbol,
			Name:         info.Name,
			Price:        info.Price,
			Bid:          info.Price * 0.9999,
			Ask:          info.Price * 1.0001,
			BidSize:      rand.Float64() * 100,
			AskSize:      rand.Float64() * 100,
			Change:       0,
			ChangePerc:   0,
			Volume:       rand.Float64() * 1000000000,
			High24h:      info.Price * 1.02,
			Low24h:       info.Price * 0.98,
			Timestamp:    now.UnixMilli(),
			TimestampStr: now.Format("15:04:05.000"),
		}
	}

	for symbol, info := range stockBasePrices {
		e.stockPrices[symbol] = &TickData{
			Symbol:       symbol,
			Name:         info.Name,
			Price:        info.Price,
			Bid:          info.Price - 0.01,
			Ask:          info.Price + 0.01,
			BidSize:      rand.Float64() * 10000,
			AskSize:      rand.Float64() * 10000,
			Change:       0,
			ChangePerc:   0,
			Volume:       rand.Float64() * 100000000,
			High24h:      info.Price * 1.015,
			Low24h:       info.Price * 0.985,
			Timestamp:    now.UnixMilli(),
			TimestampStr: now.Format("15:04:05.000"),
		}
	}
}

// 模拟价格变动 (高频)
func (e *PriceEngine) simulateTick() {
	e.mu.Lock()
	defer e.mu.Unlock()

	now := time.Now()
	e.updateCount++

	// 更新加密货币价格 - 波动较大
	for symbol, tick := range e.cryptoPrices {
		base := cryptoBasePrices[symbol].Price
		// 随机价格变动 (-0.1% to +0.1%)
		change := (rand.Float64() - 0.5) * 0.002 * base
		tick.Price += change

		// 保持价格在合理范围内
		if tick.Price < base*0.9 {
			tick.Price = base * 0.9
		} else if tick.Price > base*1.1 {
			tick.Price = base * 1.1
		}

		// 更新买卖价
		spread := tick.Price * 0.0001 // 0.01% spread
		tick.Bid = tick.Price - spread
		tick.Ask = tick.Price + spread
		tick.BidSize = math.Max(1, tick.BidSize+(rand.Float64()-0.5)*10)
		tick.AskSize = math.Max(1, tick.AskSize+(rand.Float64()-0.5)*10)

		// 计算变化
		tick.Change = tick.Price - base
		tick.ChangePerc = (tick.Change / base) * 100

		// 更新高低点
		if tick.Price > tick.High24h {
			tick.High24h = tick.Price
		}
		if tick.Price < tick.Low24h {
			tick.Low24h = tick.Price
		}

		// 更新时间戳
		tick.Timestamp = now.UnixMilli()
		tick.TimestampStr = now.Format("15:04:05.000")

		// 累积成交量
		tick.Volume += rand.Float64() * tick.Price * 10
	}

	// 更新股票价格 - 波动较小
	for symbol, tick := range e.stockPrices {
		base := stockBasePrices[symbol].Price
		// 随机价格变动 (-0.05% to +0.05%)
		change := (rand.Float64() - 0.5) * 0.001 * base
		tick.Price += change

		// 保持价格在合理范围内
		if tick.Price < base*0.95 {
			tick.Price = base * 0.95
		} else if tick.Price > base*1.05 {
			tick.Price = base * 1.05
		}

		// 更新买卖价
		tick.Bid = tick.Price - 0.01
		tick.Ask = tick.Price + 0.01
		tick.BidSize = math.Max(100, tick.BidSize+(rand.Float64()-0.5)*100)
		tick.AskSize = math.Max(100, tick.AskSize+(rand.Float64()-0.5)*100)

		// 计算变化
		tick.Change = tick.Price - base
		tick.ChangePerc = (tick.Change / base) * 100

		// 更新高低点
		if tick.Price > tick.High24h {
			tick.High24h = tick.Price
		}
		if tick.Price < tick.Low24h {
			tick.Low24h = tick.Price
		}

		// 更新时间戳
		tick.Timestamp = now.UnixMilli()
		tick.TimestampStr = now.Format("15:04:05.000")

		// 累积成交量
		tick.Volume += rand.Float64() * 1000
	}
}

// 启动高频价格更新
func (e *PriceEngine) startHFTUpdater() {
	// 100ms 更新一次 (每秒10次)
	ticker := time.NewTicker(100 * time.Millisecond)
	go func() {
		for range ticker.C {
			e.simulateTick()
		}
	}()

	fmt.Println("✅ HFT Price Engine started - 100ms update interval (10 updates/sec)")
}

// REST API 处理器
func handleCryptoPrices(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Cache-Control", "no-cache")

	engine.mu.RLock()
	prices := make([]TickData, 0, len(engine.cryptoPrices))
	for _, tick := range engine.cryptoPrices {
		prices = append(prices, *tick)
	}
	engine.mu.RUnlock()

	json.NewEncoder(w).Encode(prices)
}

func handleStockPrices(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Cache-Control", "no-cache")

	engine.mu.RLock()
	prices := make([]TickData, 0, len(engine.stockPrices))
	for _, tick := range engine.stockPrices {
		prices = append(prices, *tick)
	}
	engine.mu.RUnlock()

	json.NewEncoder(w).Encode(prices)
}

func handleAllPrices(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Cache-Control", "no-cache")

	engine.mu.RLock()
	crypto := make([]TickData, 0, len(engine.cryptoPrices))
	for _, tick := range engine.cryptoPrices {
		crypto = append(crypto, *tick)
	}
	stocks := make([]TickData, 0, len(engine.stockPrices))
	for _, tick := range engine.stockPrices {
		stocks = append(stocks, *tick)
	}
	updateCount := engine.updateCount
	engine.mu.RUnlock()

	response := map[string]interface{}{
		"crypto":      crypto,
		"stocks":      stocks,
		"timestamp":   time.Now().UnixMilli(),
		"updateCount": updateCount,
	}
	json.NewEncoder(w).Encode(response)
}

// SSE (Server-Sent Events) 实时数据流
func handleStream(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.Header().Set("Access-Control-Allow-Origin", "*")

	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "SSE not supported", http.StatusInternalServerError)
		return
	}

	log.Println("📡 New SSE client connected")

	ticker := time.NewTicker(100 * time.Millisecond) // 100ms updates
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			engine.mu.RLock()
			crypto := make([]TickData, 0, len(engine.cryptoPrices))
			for _, tick := range engine.cryptoPrices {
				crypto = append(crypto, *tick)
			}
			stocks := make([]TickData, 0, len(engine.stockPrices))
			for _, tick := range engine.stockPrices {
				stocks = append(stocks, *tick)
			}
			engine.mu.RUnlock()

			data := map[string]interface{}{
				"crypto":    crypto,
				"stocks":    stocks,
				"timestamp": time.Now().UnixMilli(),
			}

			jsonData, _ := json.Marshal(data)
			fmt.Fprintf(w, "data: %s\n\n", jsonData)
			flusher.Flush()

		case <-r.Context().Done():
			log.Println("📡 SSE client disconnected")
			return
		}
	}
}

func handleStatus(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")

	engine.mu.RLock()
	updateCount := engine.updateCount
	engine.mu.RUnlock()

	status := map[string]interface{}{
		"service":       "HFT Price Stream",
		"version":       "2.0",
		"updateRate":    "100ms (10/sec)",
		"updateCount":   updateCount,
		"cryptoSymbols": len(cryptoBasePrices),
		"stockSymbols":  len(stockBasePrices),
		"timestamp":     time.Now().UnixMilli(),
	}
	json.NewEncoder(w).Encode(status)
}

func main() {
	rand.Seed(time.Now().UnixNano())

	fmt.Println("🚀 Starting HFT Real-Time Price Stream Service...")
	fmt.Println("📊 Supported Assets:")
	fmt.Printf("   - Crypto: %d symbols (BTC, ETH, SOL, BNB, XRP, ADA, DOGE, DOT, MATIC, AVAX, LINK, UNI)\n", len(cryptoBasePrices))
	fmt.Printf("   - Stocks: %d symbols (AAPL, MSFT, GOOGL, AMZN, TSLA, NVDA, META, JPM, V, WMT)\n", len(stockBasePrices))

	// 初始化价格
	engine.initPrices()

	// 启动高频更新
	engine.startHFTUpdater()

	// HTTP 路由
	http.HandleFunc("/api/prices/crypto", handleCryptoPrices)
	http.HandleFunc("/api/prices/stocks", handleStockPrices)
	http.HandleFunc("/api/prices/all", handleAllPrices)
	http.HandleFunc("/api/prices/stream", handleStream)
	http.HandleFunc("/api/prices/status", handleStatus)

	port := ":8081"
	fmt.Printf("✅ HFT Price Service running on http://localhost%s\n", port)
	fmt.Println("📡 Endpoints:")
	fmt.Println("   SSE Stream: /api/prices/stream (100ms updates)")
	fmt.Println("   REST API:   /api/prices/crypto, /api/prices/stocks, /api/prices/all")
	fmt.Println("   Status:     /api/prices/status")

	if err := http.ListenAndServe(port, nil); err != nil {
		log.Fatalf("❌ Server error: %v", err)
	}
}
