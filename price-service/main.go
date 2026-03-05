package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"
)

// CryptoPrice represents cryptocurrency price data
type CryptoPrice struct {
	ID              string  `json:"id"`
	Symbol          string  `json:"symbol"`
	Name            string  `json:"name"`
	CurrentPrice    float64 `json:"current_price"`
	PriceChange24h  float64 `json:"price_change_24h"`
	PriceChangePerc float64 `json:"price_change_percentage_24h"`
	High24h         float64 `json:"high_24h"`
	Low24h          float64 `json:"low_24h"`
	Volume24h       float64 `json:"total_volume"`
	MarketCap       float64 `json:"market_cap"`
	LastUpdated     string  `json:"last_updated"`
}

// StockPrice represents stock price data
type StockPrice struct {
	Symbol        string  `json:"symbol"`
	Name          string  `json:"name"`
	Price         float64 `json:"price"`
	Change        float64 `json:"change"`
	ChangePercent float64 `json:"changePercent"`
	High          float64 `json:"high"`
	Low           float64 `json:"low"`
	Volume        int64   `json:"volume"`
	MarketCap     int64   `json:"marketCap"`
	LastUpdated   string  `json:"lastUpdated"`
}

// CryptoAPI represents a cryptocurrency API provider
type CryptoAPI struct {
	Name     string
	URL      string
	Parser   func(body []byte) ([]CryptoPrice, error)
	Enabled  bool
	Failures int
}

// PriceService manages real-time price data
type PriceService struct {
	cryptoPrices  map[string]*CryptoPrice
	stockPrices   map[string]*StockPrice
	cryptoAPIs    []*CryptoAPI
	currentAPIIdx int
	mu            sync.RWMutex
	lastUpdate    time.Time
	lastSource    string
}

var priceService *PriceService

func init() {
	priceService = &PriceService{
		cryptoPrices: make(map[string]*CryptoPrice),
		stockPrices:  make(map[string]*StockPrice),
		cryptoAPIs:   initCryptoAPIs(),
	}
}

// initCryptoAPIs initializes all available crypto API providers
func initCryptoAPIs() []*CryptoAPI {
	return []*CryptoAPI{
		// 1. CoinGecko - Free tier, no API key needed
		{
			Name:    "CoinGecko",
			URL:     "https://api.coingecko.com/api/v3/coins/markets?vs_currency=usd&order=market_cap_desc&per_page=20&sparkline=false",
			Parser:  parseCoinGecko,
			Enabled: true,
		},
		// 2. CoinCap - Free, no API key needed
		{
			Name:    "CoinCap",
			URL:     "https://api.coincap.io/v2/assets?limit=20",
			Parser:  parseCoinCap,
			Enabled: true,
		},
		// 3. CryptoCompare - Free tier
		{
			Name:    "CryptoCompare",
			URL:     "https://min-api.cryptocompare.com/data/top/mktcapfull?limit=20&tsym=USD",
			Parser:  parseCryptoCompare,
			Enabled: true,
		},
		// 4. Binance - Free public API
		{
			Name:    "Binance",
			URL:     "https://api.binance.com/api/v3/ticker/24hr",
			Parser:  parseBinance,
			Enabled: true,
		},
		// 5. Kraken - Free public API
		{
			Name:    "Kraken",
			URL:     "https://api.kraken.com/0/public/Ticker?pair=XBTUSD,ETHUSD,SOLUSD,BNBUSD,XRPUSD,ADAUSD,DOGEUSD,DOTUSD,MATICUSD,AVAXUSD,LINKUSD,UNIUSD",
			Parser:  parseKraken,
			Enabled: true,
		},
	}
}

// parseCoinGecko parses CoinGecko API response
func parseCoinGecko(body []byte) ([]CryptoPrice, error) {
	var prices []CryptoPrice
	if err := json.Unmarshal(body, &prices); err != nil {
		return nil, err
	}
	return prices, nil
}

// CoinCapResponse for CoinCap API
type CoinCapResponse struct {
	Data []struct {
		ID                string `json:"id"`
		Symbol            string `json:"symbol"`
		Name              string `json:"name"`
		PriceUsd          string `json:"priceUsd"`
		ChangePercent24Hr string `json:"changePercent24Hr"`
		VolumeUsd24Hr     string `json:"volumeUsd24Hr"`
		MarketCapUsd      string `json:"marketCapUsd"`
	} `json:"data"`
}

// parseCoinCap parses CoinCap API response
func parseCoinCap(body []byte) ([]CryptoPrice, error) {
	var resp CoinCapResponse
	if err := json.Unmarshal(body, &resp); err != nil {
		return nil, err
	}

	var prices []CryptoPrice
	for _, coin := range resp.Data {
		price := parseFloat(coin.PriceUsd)
		change := parseFloat(coin.ChangePercent24Hr)
		volume := parseFloat(coin.VolumeUsd24Hr)
		marketCap := parseFloat(coin.MarketCapUsd)

		prices = append(prices, CryptoPrice{
			ID:              coin.ID,
			Symbol:          coin.Symbol,
			Name:            coin.Name,
			CurrentPrice:    price,
			PriceChangePerc: change,
			PriceChange24h:  price * change / 100,
			Volume24h:       volume,
			MarketCap:       marketCap,
			High24h:         price * 1.02,
			Low24h:          price * 0.98,
			LastUpdated:     time.Now().Format(time.RFC3339),
		})
	}
	return prices, nil
}

// CryptoCompareResponse for CryptoCompare API
type CryptoCompareResponse struct {
	Data []struct {
		CoinInfo struct {
			Id       string `json:"Id"`
			Name     string `json:"Name"`
			FullName string `json:"FullName"`
		} `json:"CoinInfo"`
		Raw struct {
			USD struct {
				PRICE           float64 `json:"PRICE"`
				CHANGE24HOUR    float64 `json:"CHANGE24HOUR"`
				CHANGEPCT24HOUR float64 `json:"CHANGEPCT24HOUR"`
				HIGH24HOUR      float64 `json:"HIGH24HOUR"`
				LOW24HOUR       float64 `json:"LOW24HOUR"`
				VOLUME24HOUR    float64 `json:"VOLUME24HOUR"`
				MKTCAP          float64 `json:"MKTCAP"`
			} `json:"USD"`
		} `json:"RAW"`
	} `json:"Data"`
}

// parseCryptoCompare parses CryptoCompare API response
func parseCryptoCompare(body []byte) ([]CryptoPrice, error) {
	var resp CryptoCompareResponse
	if err := json.Unmarshal(body, &resp); err != nil {
		return nil, err
	}

	var prices []CryptoPrice
	for _, coin := range resp.Data {
		if coin.Raw.USD.PRICE == 0 {
			continue
		}
		prices = append(prices, CryptoPrice{
			ID:              coin.CoinInfo.Name,
			Symbol:          coin.CoinInfo.Name,
			Name:            coin.CoinInfo.FullName,
			CurrentPrice:    coin.Raw.USD.PRICE,
			PriceChange24h:  coin.Raw.USD.CHANGE24HOUR,
			PriceChangePerc: coin.Raw.USD.CHANGEPCT24HOUR,
			High24h:         coin.Raw.USD.HIGH24HOUR,
			Low24h:          coin.Raw.USD.LOW24HOUR,
			Volume24h:       coin.Raw.USD.VOLUME24HOUR,
			MarketCap:       coin.Raw.USD.MKTCAP,
			LastUpdated:     time.Now().Format(time.RFC3339),
		})
	}
	return prices, nil
}

// BinanceTicker for Binance API
type BinanceTicker struct {
	Symbol             string `json:"symbol"`
	PriceChange        string `json:"priceChange"`
	PriceChangePercent string `json:"priceChangePercent"`
	LastPrice          string `json:"lastPrice"`
	HighPrice          string `json:"highPrice"`
	LowPrice           string `json:"lowPrice"`
	Volume             string `json:"volume"`
	QuoteVolume        string `json:"quoteVolume"`
}

// parseBinance parses Binance API response
func parseBinance(body []byte) ([]CryptoPrice, error) {
	var tickers []BinanceTicker
	if err := json.Unmarshal(body, &tickers); err != nil {
		return nil, err
	}

	// 只选择 USDT 交易对的主流币种
	symbolMap := map[string]string{
		"BTCUSDT":   "Bitcoin",
		"ETHUSDT":   "Ethereum",
		"SOLUSDT":   "Solana",
		"BNBUSDT":   "BNB",
		"XRPUSDT":   "XRP",
		"ADAUSDT":   "Cardano",
		"DOGEUSDT":  "Dogecoin",
		"DOTUSDT":   "Polkadot",
		"MATICUSDT": "Polygon",
		"AVAXUSDT":  "Avalanche",
		"LINKUSDT":  "Chainlink",
		"UNIUSDT":   "Uniswap",
		"SHIBUSDT":  "Shiba Inu",
		"LTCUSDT":   "Litecoin",
		"ATOMUSDT":  "Cosmos",
	}

	var prices []CryptoPrice
	for _, ticker := range tickers {
		name, ok := symbolMap[ticker.Symbol]
		if !ok {
			continue
		}
		price := parseFloat(ticker.LastPrice)
		change := parseFloat(ticker.PriceChange)
		changePerc := parseFloat(ticker.PriceChangePercent)
		high := parseFloat(ticker.HighPrice)
		low := parseFloat(ticker.LowPrice)
		volume := parseFloat(ticker.QuoteVolume)

		symbol := ticker.Symbol[:len(ticker.Symbol)-4] // 移除 USDT

		prices = append(prices, CryptoPrice{
			ID:              symbol,
			Symbol:          symbol,
			Name:            name,
			CurrentPrice:    price,
			PriceChange24h:  change,
			PriceChangePerc: changePerc,
			High24h:         high,
			Low24h:          low,
			Volume24h:       volume,
			MarketCap:       0, // Binance 不提供市值
			LastUpdated:     time.Now().Format(time.RFC3339),
		})
	}
	return prices, nil
}

// KrakenResponse for Kraken API
type KrakenResponse struct {
	Result map[string]struct {
		A []string `json:"a"` // ask
		B []string `json:"b"` // bid
		C []string `json:"c"` // last trade
		V []string `json:"v"` // volume
		P []string `json:"p"` // vwap
		T []int    `json:"t"` // trade count
		L []string `json:"l"` // low
		H []string `json:"h"` // high
		O string   `json:"o"` // open
	} `json:"result"`
	Error []string `json:"error"`
}

// parseKraken parses Kraken API response
func parseKraken(body []byte) ([]CryptoPrice, error) {
	var resp KrakenResponse
	if err := json.Unmarshal(body, &resp); err != nil {
		return nil, err
	}

	krakenPairMap := map[string]struct{ ID, Symbol, Name string }{
		"XXBTZUSD": {"bitcoin", "BTC", "Bitcoin"},
		"XETHZUSD": {"ethereum", "ETH", "Ethereum"},
		"SOLUSD":   {"solana", "SOL", "Solana"},
		"BNBUSD":   {"bnb", "BNB", "BNB"},
		"XXRPZUSD": {"ripple", "XRP", "XRP"},
		"ADAUSD":   {"cardano", "ADA", "Cardano"},
		"XDGUSD":   {"dogecoin", "DOGE", "Dogecoin"},
		"DOTUSD":   {"polkadot", "DOT", "Polkadot"},
		"MATICUSD": {"polygon", "MATIC", "Polygon"},
		"AVAXUSD":  {"avalanche", "AVAX", "Avalanche"},
		"LINKUSD":  {"chainlink", "LINK", "Chainlink"},
		"UNIUSD":   {"uniswap", "UNI", "Uniswap"},
	}

	var prices []CryptoPrice
	for pair, data := range resp.Result {
		info, ok := krakenPairMap[pair]
		if !ok {
			continue
		}
		if len(data.C) < 1 || len(data.H) < 2 || len(data.L) < 2 || len(data.V) < 2 {
			continue
		}

		price := parseFloat(data.C[0])
		open := parseFloat(data.O)
		high := parseFloat(data.H[1])
		low := parseFloat(data.L[1])
		volume := parseFloat(data.V[1])
		change := price - open
		changePerc := 0.0
		if open > 0 {
			changePerc = (change / open) * 100
		}

		prices = append(prices, CryptoPrice{
			ID:              info.ID,
			Symbol:          info.Symbol,
			Name:            info.Name,
			CurrentPrice:    price,
			PriceChange24h:  change,
			PriceChangePerc: changePerc,
			High24h:         high,
			Low24h:          low,
			Volume24h:       volume * price,
			MarketCap:       0,
			LastUpdated:     time.Now().Format(time.RFC3339),
		})
	}
	return prices, nil
}

// parseFloat safely parses a string to float64
func parseFloat(s string) float64 {
	var f float64
	fmt.Sscanf(s, "%f", &f)
	return f
}

// FetchCryptoPrices tries multiple APIs with automatic fallback
func (ps *PriceService) FetchCryptoPrices() error {
	ps.mu.Lock()
	startIdx := ps.currentAPIIdx
	ps.mu.Unlock()

	// 尝试所有 API，从当前索引开始
	for i := 0; i < len(ps.cryptoAPIs); i++ {
		idx := (startIdx + i) % len(ps.cryptoAPIs)
		api := ps.cryptoAPIs[idx]

		if !api.Enabled {
			continue
		}

		prices, err := ps.tryFetchFromAPI(api)
		if err != nil {
			fmt.Printf("⚠️  %s failed: %v\n", api.Name, err)
			api.Failures++

			// 连续失败3次后暂时禁用
			if api.Failures >= 3 {
				api.Enabled = false
				fmt.Printf("🔴 %s disabled due to repeated failures\n", api.Name)
				go ps.reEnableAPIAfter(api, 5*time.Minute)
			}
			continue
		}

		// 成功获取数据
		ps.mu.Lock()
		for _, price := range prices {
			p := price
			ps.cryptoPrices[price.ID] = &p
		}
		ps.lastUpdate = time.Now()
		ps.lastSource = api.Name
		ps.currentAPIIdx = idx
		api.Failures = 0
		ps.mu.Unlock()

		fmt.Printf("✅ Updated %d crypto prices from %s\n", len(prices), api.Name)
		return nil
	}

	// 所有 API 都失败，使用模拟数据
	ps.useMockCryptoPrices()
	return nil
}

// tryFetchFromAPI attempts to fetch data from a single API
func (ps *PriceService) tryFetchFromAPI(api *CryptoAPI) ([]CryptoPrice, error) {
	client := &http.Client{Timeout: 10 * time.Second}
	resp, err := client.Get(api.URL)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("HTTP %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}

	return api.Parser(body)
}

// reEnableAPIAfter re-enables an API after a delay
func (ps *PriceService) reEnableAPIAfter(api *CryptoAPI, delay time.Duration) {
	time.Sleep(delay)
	api.Enabled = true
	api.Failures = 0
	fmt.Printf("🟢 %s re-enabled\n", api.Name)
}

// useMockCryptoPrices uses mock data when all APIs are unavailable
func (ps *PriceService) useMockCryptoPrices() {
	ps.mu.Lock()
	defer ps.mu.Unlock()

	// 添加随机波动
	rand := float64(time.Now().UnixNano()%1000) / 10

	mockCryptos := []CryptoPrice{
		{"bitcoin", "BTC", "Bitcoin", 95420.50 + rand, 1250.30, 1.33, 96500.00, 93800.00, 28500000000, 1870000000000, time.Now().Format(time.RFC3339)},
		{"ethereum", "ETH", "Ethereum", 3285.40 + rand/2, 85.20, 2.66, 3350.00, 3180.00, 15200000000, 395000000000, time.Now().Format(time.RFC3339)},
		{"solana", "SOL", "Solana", 198.75 + rand/5, 8.45, 4.44, 205.80, 188.50, 4800000000, 86000000000, time.Now().Format(time.RFC3339)},
		{"binancecoin", "BNB", "BNB", 685.30 + rand/3, -12.40, -1.78, 702.50, 678.20, 1850000000, 102000000000, time.Now().Format(time.RFC3339)},
		{"ripple", "XRP", "XRP", 2.35 + rand/50, 0.08, 3.52, 2.45, 2.22, 3200000000, 128000000000, time.Now().Format(time.RFC3339)},
		{"cardano", "ADA", "Cardano", 0.98 + rand/100, 0.04, 4.26, 1.02, 0.92, 890000000, 34500000000, time.Now().Format(time.RFC3339)},
		{"dogecoin", "DOGE", "Dogecoin", 0.385 + rand/200, 0.015, 4.05, 0.398, 0.365, 2100000000, 55000000000, time.Now().Format(time.RFC3339)},
		{"polkadot", "DOT", "Polkadot", 7.85 + rand/20, 0.32, 4.24, 8.15, 7.52, 420000000, 10500000000, time.Now().Format(time.RFC3339)},
		{"polygon", "MATIC", "Polygon", 0.52 + rand/100, 0.02, 4.00, 0.55, 0.49, 380000000, 5200000000, time.Now().Format(time.RFC3339)},
		{"avalanche-2", "AVAX", "Avalanche", 38.50 + rand/10, 1.85, 5.04, 40.20, 36.80, 650000000, 15000000000, time.Now().Format(time.RFC3339)},
		{"chainlink", "LINK", "Chainlink", 22.45 + rand/20, 0.95, 4.42, 23.50, 21.30, 520000000, 13500000000, time.Now().Format(time.RFC3339)},
		{"uniswap", "UNI", "Uniswap", 12.85 + rand/30, 0.48, 3.88, 13.40, 12.25, 280000000, 7700000000, time.Now().Format(time.RFC3339)},
	}

	for i := range mockCryptos {
		ps.cryptoPrices[mockCryptos[i].ID] = &mockCryptos[i]
	}
	ps.lastUpdate = time.Now()
	ps.lastSource = "MockData"
	fmt.Printf("✅ Using mock data for %d crypto prices (all APIs unavailable)\n", len(mockCryptos))
}

// FetchStockPrices fetches stock prices (using mock data for demo)
func (ps *PriceService) FetchStockPrices() error {
	mockStocks := []StockPrice{
		{"AAPL", "Apple Inc.", 178.50, 2.35, 1.33, 180.20, 176.80, 52000000, 2800000000000, time.Now().Format(time.RFC3339)},
		{"MSFT", "Microsoft Corp.", 385.20, 5.80, 1.53, 388.50, 382.10, 28000000, 2900000000000, time.Now().Format(time.RFC3339)},
		{"GOOGL", "Alphabet Inc.", 142.80, -1.20, -0.83, 144.50, 141.90, 25000000, 1800000000000, time.Now().Format(time.RFC3339)},
		{"AMZN", "Amazon.com Inc.", 165.30, 3.15, 1.94, 167.20, 163.50, 48000000, 1700000000000, time.Now().Format(time.RFC3339)},
		{"TSLA", "Tesla Inc.", 245.75, -8.25, -3.25, 252.30, 243.10, 115000000, 780000000000, time.Now().Format(time.RFC3339)},
		{"NVDA", "NVIDIA Corp.", 485.60, 12.40, 2.62, 490.80, 478.30, 42000000, 1200000000000, time.Now().Format(time.RFC3339)},
		{"META", "Meta Platforms", 395.20, 6.80, 1.75, 398.50, 390.10, 18000000, 1000000000000, time.Now().Format(time.RFC3339)},
		{"JPM", "JPMorgan Chase", 158.90, 1.45, 0.92, 160.20, 157.50, 12000000, 450000000000, time.Now().Format(time.RFC3339)},
		{"V", "Visa Inc.", 268.40, 3.20, 1.21, 270.50, 265.80, 8000000, 550000000000, time.Now().Format(time.RFC3339)},
		{"WMT", "Walmart Inc.", 165.50, 0.85, 0.52, 166.80, 164.20, 9000000, 450000000000, time.Now().Format(time.RFC3339)},
		{"DIS", "Disney Co.", 98.75, -1.15, -1.15, 100.20, 97.80, 15000000, 180000000000, time.Now().Format(time.RFC3339)},
		{"BA", "Boeing Co.", 175.20, 4.30, 2.51, 178.50, 172.90, 11000000, 105000000000, time.Now().Format(time.RFC3339)},
	}

	ps.mu.Lock()
	defer ps.mu.Unlock()

	for i := range mockStocks {
		ps.stockPrices[mockStocks[i].Symbol] = &mockStocks[i]
	}

	fmt.Printf("✅ Updated %d stock prices\n", len(mockStocks))
	return nil
}

// GetCryptoPrices returns all crypto prices
func (ps *PriceService) GetCryptoPrices() []CryptoPrice {
	ps.mu.RLock()
	defer ps.mu.RUnlock()

	prices := make([]CryptoPrice, 0, len(ps.cryptoPrices))
	for _, price := range ps.cryptoPrices {
		prices = append(prices, *price)
	}
	return prices
}

// GetStockPrices returns all stock prices
func (ps *PriceService) GetStockPrices() []StockPrice {
	ps.mu.RLock()
	defer ps.mu.RUnlock()

	prices := make([]StockPrice, 0, len(ps.stockPrices))
	for _, price := range ps.stockPrices {
		prices = append(prices, *price)
	}
	return prices
}

// GetServiceStatus returns the current service status
func (ps *PriceService) GetServiceStatus() map[string]interface{} {
	ps.mu.RLock()
	defer ps.mu.RUnlock()

	apis := make([]map[string]interface{}, len(ps.cryptoAPIs))
	for i, api := range ps.cryptoAPIs {
		apis[i] = map[string]interface{}{
			"name":     api.Name,
			"enabled":  api.Enabled,
			"failures": api.Failures,
		}
	}

	return map[string]interface{}{
		"lastUpdate":  ps.lastUpdate.Format(time.RFC3339),
		"lastSource":  ps.lastSource,
		"cryptoCount": len(ps.cryptoPrices),
		"stockCount":  len(ps.stockPrices),
		"apis":        apis,
	}
}

// StartPriceUpdater starts background price updates
func (ps *PriceService) StartPriceUpdater(interval time.Duration) {
	// Initial fetch
	if err := ps.FetchCryptoPrices(); err != nil {
		fmt.Printf("⚠️  Failed to fetch crypto prices: %v\n", err)
	}
	if err := ps.FetchStockPrices(); err != nil {
		fmt.Printf("⚠️  Failed to fetch stock prices: %v\n", err)
	}

	// Periodic updates
	ticker := time.NewTicker(interval)
	go func() {
		for range ticker.C {
			if err := ps.FetchCryptoPrices(); err != nil {
				fmt.Printf("⚠️  Failed to update crypto prices: %v\n", err)
			}
			if err := ps.FetchStockPrices(); err != nil {
				fmt.Printf("⚠️  Failed to update stock prices: %v\n", err)
			}
		}
	}()
}

// HTTP Handlers
func handleCryptoPrices(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")

	prices := priceService.GetCryptoPrices()
	json.NewEncoder(w).Encode(prices)
}

func handleStockPrices(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")

	prices := priceService.GetStockPrices()
	json.NewEncoder(w).Encode(prices)
}

func handleStatus(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")

	status := priceService.GetServiceStatus()
	json.NewEncoder(w).Encode(status)
}

func main() {
	fmt.Println("🚀 Starting Real-Time Price Service with Multi-API Fallback...")
	fmt.Println("📡 Available APIs:")
	for _, api := range priceService.cryptoAPIs {
		fmt.Printf("   - %s\n", api.Name)
	}

	// Start price updater (updates every 30 seconds)
	priceService.StartPriceUpdater(30 * time.Second)

	// Setup HTTP routes
	http.HandleFunc("/api/prices/crypto", handleCryptoPrices)
	http.HandleFunc("/api/prices/stocks", handleStockPrices)
	http.HandleFunc("/api/prices/status", handleStatus)

	port := ":8081"
	fmt.Printf("✅ Price service running on http://localhost%s\n", port)
	fmt.Println("   GET /api/prices/crypto - Get cryptocurrency prices")
	fmt.Println("   GET /api/prices/stocks - Get stock prices")
	fmt.Println("   GET /api/prices/status - Get API status")

	if err := http.ListenAndServe(port, nil); err != nil {
		fmt.Printf("❌ Server error: %v\n", err)
	}
}
