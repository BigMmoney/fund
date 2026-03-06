package main

import (
	"encoding/json"
	"io"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/gorilla/mux"
	"github.com/gorilla/websocket"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
	"pre_trading/services/utils"
)

// APIGateway handles HTTP and WebSocket requests
type APIGateway struct {
	router      *mux.Router
	eventBus    *eventbus.EventBus
	upgrader    websocket.Upgrader
	wsClients   map[*websocket.Conn]bool
	wsClientsMu sync.RWMutex
	// References to other services (in production, use gRPC/HTTP clients)
	markets    map[string]*types.Market
	marketsMu  sync.RWMutex
	orderBooks map[string]*OrderBook
	booksMu    sync.RWMutex
	users      map[string]*User
	usersMu    sync.RWMutex
	trades     []Trade
	tradesMu   sync.RWMutex
	stats      *PlatformStats
	statsMu    sync.RWMutex
}

// OrderBook with real orders
type OrderBook struct {
	MarketID string
	Bids     []OrderLevel
	Asks     []OrderLevel
}

type OrderLevel struct {
	Price  int64 `json:"price"`
	Amount int64 `json:"amount"`
	Count  int   `json:"count"`
}

type User struct {
	ID        string
	Username  string
	Balance   int64
	Hold      int64
	Positions map[string]int64
	CreatedAt time.Time
}

type Trade struct {
	ID        string    `json:"id"`
	MarketID  string    `json:"market_id"`
	Price     int64     `json:"price"`
	Amount    int64     `json:"amount"`
	Side      string    `json:"side"`
	Buyer     string    `json:"buyer"`
	Seller    string    `json:"seller"`
	Timestamp time.Time `json:"timestamp"`
}

type PlatformStats struct {
	TotalVolume24h int64
	TotalTrades24h int
	ActiveMarkets  int
	TotalUsers     int
	TotalLiquidity int64
	LastUpdated    time.Time
}

func NewAPIGateway(eventBus *eventbus.EventBus) *APIGateway {
	gw := &APIGateway{
		router:   mux.NewRouter(),
		eventBus: eventBus,
		upgrader: websocket.Upgrader{
			CheckOrigin: func(r *http.Request) bool { return true },
		},
		wsClients:  make(map[*websocket.Conn]bool),
		markets:    make(map[string]*types.Market),
		orderBooks: make(map[string]*OrderBook),
		users:      make(map[string]*User),
		trades:     make([]Trade, 0),
		stats:      &PlatformStats{},
	}

	gw.setupRoutes()
	gw.setupWebSocketBroadcaster()
	gw.initializeHotMarkets()
	gw.initializeUsers()
	gw.startStatsUpdater()

	return gw
}

func (gw *APIGateway) setupRoutes() {
	// Health check (must be before PathPrefix)
	gw.router.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(map[string]string{"status": "healthy"})
	}).Methods("GET")

	// WebSocket route
	gw.router.HandleFunc("/ws", gw.handleWebSocket)

	// API routes
	gw.router.HandleFunc("/v1/intents", gw.handleCreateIntent).Methods("POST")
	gw.router.HandleFunc("/v1/orders/{id}/cancel", gw.handleCancelOrder).Methods("POST")
	gw.router.HandleFunc("/v1/markets", gw.handleGetMarkets).Methods("GET")
	gw.router.HandleFunc("/v1/markets/{id}", gw.handleGetMarket).Methods("GET")
	gw.router.HandleFunc("/v1/markets/{id}/book", gw.handleGetOrderBook).Methods("GET")
	gw.router.HandleFunc("/v1/markets/{id}/history", gw.handleGetPriceHistory).Methods("GET")
	gw.router.HandleFunc("/v1/positions", gw.handleGetPositions).Methods("GET")
	gw.router.HandleFunc("/v1/balances", gw.handleGetBalances).Methods("GET")
	gw.router.HandleFunc("/v1/withdrawals", gw.handleCreateWithdrawal).Methods("POST")
	gw.router.HandleFunc("/v1/deposits", gw.handleGetDeposits).Methods("GET")
	gw.router.HandleFunc("/v1/trades", gw.handleGetTrades).Methods("GET")
	gw.router.HandleFunc("/v1/orders", gw.handleGetOrders).Methods("GET")
	gw.router.HandleFunc("/v1/stats", gw.handleGetStats).Methods("GET")

	// HFT Trading routes
	gw.router.HandleFunc("/hft/stream", gw.handleHFTStream).Methods("GET")
	gw.router.HandleFunc("/hft/execute", gw.handleHFTExecute).Methods("POST")
	gw.router.HandleFunc("/hft/strategies", gw.handleHFTStrategies).Methods("GET")
	gw.router.HandleFunc("/hft/signals", gw.handleHFTSignals).Methods("GET")
	gw.router.HandleFunc("/hft/risk", gw.handleHFTRisk).Methods("GET")

	// Admin routes
	gw.router.HandleFunc("/admin/markets", gw.handleAdminGetMarkets).Methods("GET")
	gw.router.HandleFunc("/admin/markets", gw.handleAdminCreateMarket).Methods("POST")
	gw.router.HandleFunc("/admin/markets/{id}", gw.handleAdminUpdateMarket).Methods("PUT")
	gw.router.HandleFunc("/admin/users", gw.handleAdminGetUsers).Methods("GET")
	gw.router.HandleFunc("/admin/stats", gw.handleAdminGetStats).Methods("GET")
	gw.router.HandleFunc("/admin/trades", gw.handleAdminGetTrades).Methods("GET")

	// Proxy to price service
	gw.router.HandleFunc("/api/prices/crypto", gw.proxyPriceService).Methods("GET")
	gw.router.HandleFunc("/api/prices/stocks", gw.proxyPriceService).Methods("GET")

	// Serve React frontend (must be last)
	// Try multiple paths for frontend build
	frontendPaths := []string{
		"./frontend-modern/dist",
		"../frontend-modern/dist",
		"./dist",
	}

	var frontendPath string
	for _, path := range frontendPaths {
		if _, err := http.Dir(path).Open("/"); err == nil {
			frontendPath = path
			break
		}
	}

	if frontendPath == "" {
		log.Println("⚠️  Warning: No frontend build found")
		return
	}

	log.Printf("✅ Serving frontend from: %s\n", frontendPath)
	fs := http.FileServer(http.Dir(frontendPath))
	gw.router.PathPrefix("/").Handler(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// For SPA routing, serve index.html for non-asset requests
		if r.URL.Path != "/" && !contains(r.URL.Path, ".") {
			http.ServeFile(w, r, frontendPath+"/index.html")
			return
		}
		fs.ServeHTTP(w, r)
	}))
}

// Helper function
func contains(s, substr string) bool {
	for _, c := range substr {
		found := false
		for _, sc := range s {
			if c == sc {
				found = true
				break
			}
		}
		if !found {
			return false
		}
	}
	return true
}

// Proxy to price service
func (gw *APIGateway) proxyPriceService(w http.ResponseWriter, r *http.Request) {
	targetURL := "http://localhost:8081" + r.URL.Path
	resp, err := http.Get(targetURL)
	if err != nil {
		http.Error(w, "Price service unavailable", http.StatusServiceUnavailable)
		return
	}
	defer resp.Body.Close()

	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.WriteHeader(resp.StatusCode)

	body, _ := io.ReadAll(resp.Body)
	w.Write(body)
}

// Intent creation
type CreateIntentRequest struct {
	UserID    string `json:"user_id"`
	MarketID  string `json:"market_id"`
	Side      string `json:"side"`
	Price     int64  `json:"price"`
	Amount    int64  `json:"amount"`
	Outcome   int    `json:"outcome"`
	ExpiresIn int64  `json:"expires_in"` // seconds
}

type IntentResponse struct {
	IntentID  string `json:"intent_id"`
	Status    string `json:"status"`
	CreatedAt string `json:"created_at"`
}

func (gw *APIGateway) handleCreateIntent(w http.ResponseWriter, r *http.Request) {
	var req CreateIntentRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	// Validate request
	if req.UserID == "" || req.MarketID == "" || req.Side == "" {
		http.Error(w, "missing required fields", http.StatusBadRequest)
		return
	}

	if req.Side != "buy" && req.Side != "sell" {
		http.Error(w, "side must be 'buy' or 'sell'", http.StatusBadRequest)
		return
	}

	// Create intent
	intent := &types.Intent{
		ID:        utils.GenerateID(),
		UserID:    req.UserID,
		MarketID:  req.MarketID,
		Side:      req.Side,
		Price:     req.Price,
		Amount:    req.Amount,
		Outcome:   req.Outcome,
		CreatedAt: time.Now(),
		ExpiresAt: time.Now().Add(time.Duration(req.ExpiresIn) * time.Second),
		Status:    "pending",
	}

	// Publish to event bus
	gw.eventBus.Publish(types.EventTypeIntentReceived, intent)

	// Return response
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(IntentResponse{
		IntentID:  intent.ID,
		Status:    intent.Status,
		CreatedAt: intent.CreatedAt.Format(time.RFC3339),
	})

	log.Printf("Intent created: %s", intent.ID)
}

// Cancel order
func (gw *APIGateway) handleCancelOrder(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	orderID := vars["id"]

	// Publish cancel event
	gw.eventBus.Publish(types.EventTypeIntentCancelled, map[string]string{
		"intent_id": orderID,
	})

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{
		"status": "cancelled",
	})

	log.Printf("Order cancelled: %s", orderID)
}

// Get markets
func (gw *APIGateway) handleGetMarkets(w http.ResponseWriter, r *http.Request) {
	gw.marketsMu.RLock()
	defer gw.marketsMu.RUnlock()

	markets := make([]*types.Market, 0, len(gw.markets))
	for _, market := range gw.markets {
		markets = append(markets, market)
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(markets)
}

// Get single market
func (gw *APIGateway) handleGetMarket(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	marketID := vars["id"]

	gw.marketsMu.RLock()
	market, exists := gw.markets[marketID]
	gw.marketsMu.RUnlock()

	if !exists {
		http.Error(w, "market not found", http.StatusNotFound)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(market)
}

// Get order book
func (gw *APIGateway) handleGetOrderBook(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	marketID := vars["id"]

	gw.booksMu.RLock()
	book, exists := gw.orderBooks[marketID]
	gw.booksMu.RUnlock()

	var response map[string]interface{}
	if exists {
		response = map[string]interface{}{
			"market_id": marketID,
			"bids":      book.Bids,
			"asks":      book.Asks,
			"timestamp": time.Now().Format(time.RFC3339),
		}
	} else {
		response = map[string]interface{}{
			"market_id": marketID,
			"bids":      []interface{}{},
			"asks":      []interface{}{},
			"timestamp": time.Now().Format(time.RFC3339),
		}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(response)
}

// Get positions
func (gw *APIGateway) handleGetPositions(w http.ResponseWriter, r *http.Request) {
	userID := r.URL.Query().Get("user_id")
	if userID == "" {
		http.Error(w, "user_id required", http.StatusBadRequest)
		return
	}

	gw.usersMu.RLock()
	user, exists := gw.users[userID]
	gw.usersMu.RUnlock()

	positions := []map[string]interface{}{}

	if exists && user.Positions != nil {
		for marketID, amount := range user.Positions {
			if amount != 0 {
				gw.marketsMu.RLock()
				market := gw.markets[marketID]
				gw.marketsMu.RUnlock()

				marketName := marketID
				if market != nil {
					marketName = market.Name
				}

				// Calculate current value based on order book
				gw.booksMu.RLock()
				book := gw.orderBooks[marketID]
				gw.booksMu.RUnlock()

				currentPrice := int64(50) // Default mid price
				if book != nil && len(book.Bids) > 0 {
					currentPrice = book.Bids[0].Price
				}

				positions = append(positions, map[string]interface{}{
					"market_id":     marketID,
					"market_name":   marketName,
					"outcome":       "YES",
					"amount":        amount,
					"avg_price":     utils.RandomInt(30, 70),
					"current_price": currentPrice,
					"pnl":           (currentPrice - int64(utils.RandomInt(30, 70))) * amount / 100,
					"created_at":    time.Now().Add(-time.Duration(utils.RandomInt(1, 72)) * time.Hour),
				})
			}
		}
	}

	// Add sample positions if none exist
	if len(positions) == 0 {
		sampleMarkets := []string{"btc-150k-2026", "trump-2028", "eth-10k-2026"}
		for _, marketID := range sampleMarkets {
			gw.booksMu.RLock()
			book := gw.orderBooks[marketID]
			gw.booksMu.RUnlock()

			currentPrice := int64(50)
			if book != nil && len(book.Bids) > 0 {
				currentPrice = book.Bids[0].Price
			}

			avgPrice := int64(utils.RandomInt(30, 60))
			amount := int64(utils.RandomInt(100, 1000)) * 100

			positions = append(positions, map[string]interface{}{
				"market_id":     marketID,
				"market_name":   gw.getMarketName(marketID),
				"outcome":       "YES",
				"amount":        amount,
				"avg_price":     avgPrice,
				"current_price": currentPrice,
				"pnl":           (currentPrice - avgPrice) * amount / 100,
				"created_at":    time.Now().Add(-time.Duration(utils.RandomInt(1, 72)) * time.Hour),
			})
		}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(positions)
}

func (gw *APIGateway) getMarketName(marketID string) string {
	gw.marketsMu.RLock()
	defer gw.marketsMu.RUnlock()
	if market, exists := gw.markets[marketID]; exists {
		return market.Name
	}
	return marketID
}

// Get balances
func (gw *APIGateway) handleGetBalances(w http.ResponseWriter, r *http.Request) {
	userID := r.URL.Query().Get("user_id")
	if userID == "" {
		http.Error(w, "user_id required", http.StatusBadRequest)
		return
	}

	gw.usersMu.RLock()
	user, exists := gw.users[userID]
	gw.usersMu.RUnlock()

	var balances []types.Balance
	if exists {
		balances = []types.Balance{
			{
				UserID:    userID,
				Asset:     "USDC",
				Available: user.Balance,
				Hold:      user.Hold,
				UpdatedAt: time.Now(),
			},
		}
	} else {
		balances = []types.Balance{
			{
				UserID:    userID,
				Asset:     "USDC",
				Available: 10000 * 100, // $10,000 in cents
				Hold:      0,
				UpdatedAt: time.Now(),
			},
		}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(balances)
}

// Create withdrawal
type WithdrawalRequest struct {
	UserID  string `json:"user_id"`
	Amount  int64  `json:"amount"`
	Address string `json:"address"`
}

func (gw *APIGateway) handleCreateWithdrawal(w http.ResponseWriter, r *http.Request) {
	var req WithdrawalRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	if req.UserID == "" || req.Amount <= 0 || req.Address == "" {
		http.Error(w, "missing required fields", http.StatusBadRequest)
		return
	}

	// Publish withdrawal request
	gw.eventBus.Publish(types.EventTypeChainWithdrawal, map[string]interface{}{
		"user_id": req.UserID,
		"amount":  req.Amount,
		"address": req.Address,
	})

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{
		"status":        "pending",
		"withdrawal_id": utils.GenerateID(),
	})

	log.Printf("Withdrawal requested: user=%s, amount=%d", req.UserID, req.Amount)
}

// WebSocket handling
func (gw *APIGateway) handleWebSocket(w http.ResponseWriter, r *http.Request) {
	conn, err := gw.upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("WebSocket upgrade failed: %v", err)
		return
	}

	gw.wsClientsMu.Lock()
	gw.wsClients[conn] = true
	gw.wsClientsMu.Unlock()

	log.Printf("WebSocket client connected: %s", conn.RemoteAddr())

	// Clean up on disconnect
	defer func() {
		gw.wsClientsMu.Lock()
		delete(gw.wsClients, conn)
		gw.wsClientsMu.Unlock()
		conn.Close()
		log.Printf("WebSocket client disconnected: %s", conn.RemoteAddr())
	}()

	// Read messages from client (ping/pong, subscriptions, etc.)
	for {
		messageType, message, err := conn.ReadMessage()
		if err != nil {
			break
		}
		log.Printf("Received WebSocket message: %s", string(message))

		// Echo back for now
		if err := conn.WriteMessage(messageType, message); err != nil {
			break
		}
	}
}

// setupWebSocketBroadcaster sets up event broadcasting to WebSocket clients
func (gw *APIGateway) setupWebSocketBroadcaster() {
	// Subscribe to all relevant events
	eventTypes := []string{
		types.EventTypeFillCreated,
		types.EventTypeIntentReceived,
		types.EventTypeLedgerCommitted,
		types.EventTypeMarketStateChange,
	}

	eventCh := gw.eventBus.SubscribeMultiple(eventTypes, 100)

	go func() {
		for event := range eventCh {
			gw.broadcastToWebSockets(event)
		}
	}()
}

// broadcastToWebSockets broadcasts an event to all connected WebSocket clients
func (gw *APIGateway) broadcastToWebSockets(event types.Event) {
	message, err := json.Marshal(event)
	if err != nil {
		log.Printf("Failed to marshal event: %v", err)
		return
	}

	gw.wsClientsMu.RLock()
	defer gw.wsClientsMu.RUnlock()

	for conn := range gw.wsClients {
		if err := conn.WriteMessage(websocket.TextMessage, message); err != nil {
			log.Printf("Failed to send to WebSocket client: %v", err)
		}
	}
}

// initializeHotMarkets creates trending prediction markets
func (gw *APIGateway) initializeHotMarkets() {
	gw.marketsMu.Lock()
	defer gw.marketsMu.Unlock()

	// 2026 Hot Prediction Markets
	markets := []struct {
		id, name, desc string
		volume         int64
	}{
		{"trump-2028", "Will Trump run for President in 2028?", "Resolves YES if Donald Trump officially announces candidacy for 2028 presidential election", 2500000},
		{"btc-150k-2026", "Will Bitcoin reach $150K by Dec 2026?", "Resolves YES if BTC/USD reaches $150,000 on any major exchange by December 31, 2026", 8500000},
		{"eth-10k-2026", "Will Ethereum reach $10K in 2026?", "Resolves YES if ETH/USD reaches $10,000 by December 31, 2026", 4200000},
		{"fed-rate-cut", "Will Fed cut rates before July 2026?", "Resolves YES if Federal Reserve announces rate cut before July 1, 2026", 3100000},
		{"ai-agi-2026", "Will AGI be announced in 2026?", "Resolves YES if a major AI lab claims to have achieved AGI by end of 2026", 1800000},
		{"spacex-mars", "Will SpaceX land on Mars by 2028?", "Resolves YES if SpaceX successfully lands a spacecraft on Mars before Jan 1, 2028", 950000},
		{"china-taiwan", "Will China invade Taiwan by 2027?", "Resolves YES if Chinese military forces enter Taiwan before Jan 1, 2027", 750000},
		{"apple-ai-device", "Will Apple release AI device in 2026?", "Resolves YES if Apple announces a new AI-focused hardware device in 2026", 620000},
		{"solana-flip-eth", "Will Solana flip Ethereum market cap?", "Resolves YES if SOL market cap exceeds ETH market cap at any point in 2026", 1100000},
		{"world-cup-2026", "Will USA win World Cup 2026?", "Resolves YES if United States wins FIFA World Cup 2026", 2800000},
	}

	for _, m := range markets {
		gw.markets[m.id] = &types.Market{
			ID:          m.id,
			Name:        m.name,
			Description: m.desc,
			Outcomes:    []string{"YES", "NO"},
			State:       types.MarketStateOpen,
			CreatedAt:   time.Now().Add(-time.Duration(utils.RandomInt(1, 30)) * 24 * time.Hour),
		}
		gw.initializeOrderBook(m.id, m.volume)
	}
}

// initializeOrderBook creates realistic order book data
func (gw *APIGateway) initializeOrderBook(marketID string, volume int64) {
	gw.booksMu.Lock()
	defer gw.booksMu.Unlock()

	// Generate realistic order book based on market
	midPrice := 45 + utils.RandomInt(0, 20) // 45-65 cents

	bids := make([]OrderLevel, 0)
	asks := make([]OrderLevel, 0)

	// Generate bid levels (below mid price)
	for i := 0; i < 8; i++ {
		price := int64(midPrice - i - 1)
		if price < 1 {
			break
		}
		amount := int64(utils.RandomInt(1000, 50000)) * 100 // In cents
		bids = append(bids, OrderLevel{
			Price:  price,
			Amount: amount,
			Count:  utils.RandomInt(5, 50),
		})
	}

	// Generate ask levels (above mid price)
	for i := 0; i < 8; i++ {
		price := int64(midPrice + i + 1)
		if price > 99 {
			break
		}
		amount := int64(utils.RandomInt(1000, 50000)) * 100
		asks = append(asks, OrderLevel{
			Price:  price,
			Amount: amount,
			Count:  utils.RandomInt(5, 50),
		})
	}

	gw.orderBooks[marketID] = &OrderBook{
		MarketID: marketID,
		Bids:     bids,
		Asks:     asks,
	}
}

// initializeUsers creates sample users
func (gw *APIGateway) initializeUsers() {
	gw.usersMu.Lock()
	defer gw.usersMu.Unlock()

	users := []struct {
		id, name string
		balance  int64
	}{
		{"user1", "Demo User", 1000000},      // $10,000
		{"whale1", "Crypto Whale", 50000000}, // $500,000
		{"trader1", "Pro Trader", 10000000},  // $100,000
		{"mm1", "Market Maker", 100000000},   // $1,000,000
	}

	for _, u := range users {
		gw.users[u.id] = &User{
			ID:        u.id,
			Username:  u.name,
			Balance:   u.balance,
			Hold:      0,
			Positions: make(map[string]int64),
			CreatedAt: time.Now().Add(-time.Duration(utils.RandomInt(1, 90)) * 24 * time.Hour),
		}
	}

	// Generate sample trades
	gw.generateSampleTrades()
}

// generateSampleTrades creates recent trade history
func (gw *APIGateway) generateSampleTrades() {
	gw.tradesMu.Lock()
	defer gw.tradesMu.Unlock()

	marketIDs := []string{"trump-2028", "btc-150k-2026", "eth-10k-2026", "fed-rate-cut", "ai-agi-2026"}
	userIDs := []string{"user1", "whale1", "trader1", "mm1"}

	for i := 0; i < 50; i++ {
		marketID := marketIDs[utils.RandomInt(0, len(marketIDs))]
		side := "buy"
		if utils.RandomInt(0, 2) == 1 {
			side = "sell"
		}

		trade := Trade{
			ID:        utils.GenerateUUID(),
			MarketID:  marketID,
			Price:     int64(utils.RandomInt(30, 70)),
			Amount:    int64(utils.RandomInt(100, 10000)) * 100,
			Side:      side,
			Buyer:     userIDs[utils.RandomInt(0, len(userIDs))],
			Seller:    userIDs[utils.RandomInt(0, len(userIDs))],
			Timestamp: time.Now().Add(-time.Duration(utils.RandomInt(1, 1440)) * time.Minute),
		}
		gw.trades = append(gw.trades, trade)
	}

	// Sort by timestamp (newest first)
	for i := 0; i < len(gw.trades)-1; i++ {
		for j := i + 1; j < len(gw.trades); j++ {
			if gw.trades[j].Timestamp.After(gw.trades[i].Timestamp) {
				gw.trades[i], gw.trades[j] = gw.trades[j], gw.trades[i]
			}
		}
	}
}

// startStatsUpdater periodically updates platform stats
func (gw *APIGateway) startStatsUpdater() {
	go func() {
		for {
			gw.updateStats()
			time.Sleep(30 * time.Second)
		}
	}()
}

func (gw *APIGateway) updateStats() {
	gw.statsMu.Lock()
	defer gw.statsMu.Unlock()

	gw.marketsMu.RLock()
	activeMarkets := len(gw.markets)
	gw.marketsMu.RUnlock()

	gw.usersMu.RLock()
	totalUsers := len(gw.users)
	gw.usersMu.RUnlock()

	gw.tradesMu.RLock()
	trades24h := 0
	volume24h := int64(0)
	cutoff := time.Now().Add(-24 * time.Hour)
	for _, t := range gw.trades {
		if t.Timestamp.After(cutoff) {
			trades24h++
			volume24h += t.Price * t.Amount / 100
		}
	}
	gw.tradesMu.RUnlock()

	// Calculate total liquidity
	gw.booksMu.RLock()
	liquidity := int64(0)
	for _, book := range gw.orderBooks {
		for _, bid := range book.Bids {
			liquidity += bid.Amount
		}
		for _, ask := range book.Asks {
			liquidity += ask.Amount
		}
	}
	gw.booksMu.RUnlock()

	gw.stats = &PlatformStats{
		TotalVolume24h: volume24h,
		TotalTrades24h: trades24h,
		ActiveMarkets:  activeMarkets,
		TotalUsers:     totalUsers,
		TotalLiquidity: liquidity,
		LastUpdated:    time.Now(),
	}
}

// handleGetTrades returns recent trades
func (gw *APIGateway) handleGetTrades(w http.ResponseWriter, r *http.Request) {
	marketID := r.URL.Query().Get("market_id")
	userID := r.URL.Query().Get("user_id")
	limit := 50

	gw.tradesMu.RLock()
	defer gw.tradesMu.RUnlock()

	var result []Trade
	for _, t := range gw.trades {
		// Filter by market if specified
		if marketID != "" && t.MarketID != marketID {
			continue
		}
		// Filter by user if specified
		if userID != "" && t.Buyer != userID && t.Seller != userID {
			continue
		}
		result = append(result, t)
		if len(result) >= limit {
			break
		}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(result)
}

// handleGetPriceHistory returns price history for a market
func (gw *APIGateway) handleGetPriceHistory(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	marketID := vars["id"]

	// Generate sample price history (24 hours, hourly)
	history := make([]map[string]interface{}, 0)
	now := time.Now()
	basePrice := 45 + utils.RandomInt(0, 20)

	for i := 23; i >= 0; i-- {
		t := now.Add(-time.Duration(i) * time.Hour)
		// Add some random walk to the price
		change := utils.RandomInt(-5, 5)
		price := basePrice + change
		if price < 5 {
			price = 5
		}
		if price > 95 {
			price = 95
		}

		history = append(history, map[string]interface{}{
			"timestamp": t.Format(time.RFC3339),
			"price":     price,
			"volume":    utils.RandomInt(10000, 500000) * 100,
			"high":      price + utils.RandomInt(1, 5),
			"low":       price - utils.RandomInt(1, 5),
		})
		basePrice = price
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]interface{}{
		"market_id": marketID,
		"interval":  "1h",
		"data":      history,
	})
}

// handleGetDeposits returns deposit history
func (gw *APIGateway) handleGetDeposits(w http.ResponseWriter, r *http.Request) {
	userID := r.URL.Query().Get("user_id")
	if userID == "" {
		http.Error(w, "user_id required", http.StatusBadRequest)
		return
	}

	// Sample deposits
	deposits := []map[string]interface{}{
		{
			"id":        utils.GenerateUUID(),
			"amount":    1000000, // $10,000
			"asset":     "USDC",
			"tx_hash":   "0x" + utils.GenerateUUID()[:32],
			"status":    "confirmed",
			"timestamp": time.Now().Add(-72 * time.Hour),
		},
		{
			"id":        utils.GenerateUUID(),
			"amount":    500000, // $5,000
			"asset":     "USDC",
			"tx_hash":   "0x" + utils.GenerateUUID()[:32],
			"status":    "confirmed",
			"timestamp": time.Now().Add(-24 * time.Hour),
		},
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(deposits)
}

// handleGetOrders returns user's open orders
func (gw *APIGateway) handleGetOrders(w http.ResponseWriter, r *http.Request) {
	userID := r.URL.Query().Get("user_id")
	if userID == "" {
		http.Error(w, "user_id required", http.StatusBadRequest)
		return
	}

	// Sample open orders
	orders := []map[string]interface{}{
		{
			"id":         utils.GenerateUUID(),
			"market_id":  "btc-150k-2026",
			"side":       "buy",
			"price":      45,
			"amount":     50000,
			"filled":     0,
			"status":     "open",
			"created_at": time.Now().Add(-30 * time.Minute),
		},
		{
			"id":         utils.GenerateUUID(),
			"market_id":  "trump-2028",
			"side":       "sell",
			"price":      55,
			"amount":     25000,
			"filled":     10000,
			"status":     "partial",
			"created_at": time.Now().Add(-2 * time.Hour),
		},
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(orders)
}

// handleGetStats returns platform statistics
func (gw *APIGateway) handleGetStats(w http.ResponseWriter, r *http.Request) {
	gw.statsMu.RLock()
	defer gw.statsMu.RUnlock()

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(gw.stats)
}

// ==================== HFT Handlers ====================

// HFT WebSocket stream for real-time market data
func (gw *APIGateway) handleHFTStream(w http.ResponseWriter, r *http.Request) {
	conn, err := gw.upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("HFT WebSocket upgrade failed: %v", err)
		return
	}
	defer conn.Close()

	log.Printf("HFT client connected: %s", conn.RemoteAddr())

	ticker := time.NewTicker(100 * time.Millisecond) // 10 updates per second
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			// Generate real-time market data
			data := map[string]interface{}{
				"timestamp": time.Now().UnixMilli(),
				"markets":   gw.getHFTMarketData(),
			}
			if err := conn.WriteJSON(data); err != nil {
				return
			}
		}
	}
}

func (gw *APIGateway) getHFTMarketData() []map[string]interface{} {
	gw.marketsMu.RLock()
	defer gw.marketsMu.RUnlock()

	result := make([]map[string]interface{}, 0)
	for _, m := range gw.markets {
		gw.booksMu.RLock()
		book := gw.orderBooks[m.ID]
		gw.booksMu.RUnlock()

		midPrice := int64(50)
		spread := int64(1)
		bidDepth := int64(0)
		askDepth := int64(0)

		if book != nil {
			if len(book.Bids) > 0 && len(book.Asks) > 0 {
				midPrice = (book.Bids[0].Price + book.Asks[0].Price) / 2
				spread = book.Asks[0].Price - book.Bids[0].Price
			}
			for _, b := range book.Bids {
				bidDepth += b.Amount
			}
			for _, a := range book.Asks {
				askDepth += a.Amount
			}
		}

		result = append(result, map[string]interface{}{
			"symbol":    m.ID,
			"mid_price": midPrice,
			"spread":    spread,
			"bid_depth": bidDepth,
			"ask_depth": askDepth,
			"imbalance": float64(bidDepth-askDepth) / float64(bidDepth+askDepth+1) * 100,
		})
	}
	return result
}

// HFT Execute - Low latency order execution
type HFTExecuteRequest struct {
	Symbol   string `json:"symbol"`
	Side     string `json:"side"`
	Price    int64  `json:"price"`
	Size     int64  `json:"size"`
	Strategy string `json:"strategy"`
}

func (gw *APIGateway) handleHFTExecute(w http.ResponseWriter, r *http.Request) {
	startTime := time.Now()

	var req HFTExecuteRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	// Simulate order execution
	fillPrice := req.Price
	if req.Side == "buy" {
		fillPrice += int64(utils.RandomInt(0, 1))
	} else {
		fillPrice -= int64(utils.RandomInt(0, 1))
	}

	execID := utils.GenerateUUID()
	latency := time.Since(startTime)

	// Record trade
	trade := Trade{
		ID:        execID,
		MarketID:  req.Symbol,
		Price:     fillPrice,
		Amount:    req.Size,
		Side:      req.Side,
		Buyer:     "hft-algo",
		Seller:    "market",
		Timestamp: time.Now(),
	}

	gw.tradesMu.Lock()
	gw.trades = append([]Trade{trade}, gw.trades...)
	if len(gw.trades) > 1000 {
		gw.trades = gw.trades[:1000]
	}
	gw.tradesMu.Unlock()

	response := map[string]interface{}{
		"exec_id":    execID,
		"symbol":     req.Symbol,
		"side":       req.Side,
		"req_price":  req.Price,
		"fill_price": fillPrice,
		"size":       req.Size,
		"status":     "filled",
		"latency_us": latency.Microseconds(),
		"timestamp":  time.Now().UnixMilli(),
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(response)
}

// HFT Strategies
func (gw *APIGateway) handleHFTStrategies(w http.ResponseWriter, r *http.Request) {
	strategies := []map[string]interface{}{
		{
			"id":           "mm-1",
			"name":         "Market Making",
			"status":       "running",
			"pnl":          4231.50,
			"trades":       1247,
			"win_rate":     68.2,
			"spread":       0.5,
			"inventory":    15000,
			"max_position": 100000,
		},
		{
			"id":       "mom-1",
			"name":     "Momentum Scalper",
			"status":   "running",
			"pnl":      6892.30,
			"trades":   892,
			"win_rate": 71.4,
			"avg_hold": "2.3s",
			"signals":  156,
		},
		{
			"id":            "arb-1",
			"name":          "Arbitrage Bot",
			"status":        "paused",
			"pnl":           1724.80,
			"trades":        708,
			"opportunities": 12,
			"spread_min":    0.3,
		},
		{
			"id":       "mean-1",
			"name":     "Mean Reversion",
			"status":   "running",
			"pnl":      2156.20,
			"trades":   423,
			"win_rate": 65.8,
			"lookback": "30s",
		},
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(strategies)
}

// HFT Signals
func (gw *APIGateway) handleHFTSignals(w http.ResponseWriter, r *http.Request) {
	signals := []map[string]interface{}{
		{
			"market":    "btc-150k-2026",
			"type":      "buy",
			"strength":  "strong",
			"reason":    "Momentum breakout detected",
			"price":     56,
			"target":    62,
			"stop":      52,
			"timestamp": time.Now().Add(-30 * time.Second),
		},
		{
			"market":    "eth-10k-2026",
			"type":      "sell",
			"strength":  "medium",
			"reason":    "RSI overbought (78)",
			"price":     48,
			"target":    42,
			"stop":      52,
			"timestamp": time.Now().Add(-45 * time.Second),
		},
		{
			"market":    "trump-2028",
			"type":      "neutral",
			"strength":  "weak",
			"reason":    "Consolidating in range",
			"price":     45,
			"timestamp": time.Now().Add(-60 * time.Second),
		},
		{
			"market":    "solana-flip-eth",
			"type":      "buy",
			"strength":  "strong",
			"reason":    "Volume spike +340%",
			"price":     28,
			"target":    35,
			"stop":      24,
			"timestamp": time.Now().Add(-15 * time.Second),
		},
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(signals)
}

// HFT Risk Monitor
func (gw *APIGateway) handleHFTRisk(w http.ResponseWriter, r *http.Request) {
	risk := map[string]interface{}{
		"position_utilization": 32.5,
		"max_drawdown":         -2100,
		"daily_pnl":            12847.32,
		"beta":                 0.12,
		"leverage":             4.2,
		"var_95":               -5200,
		"sharpe":               2.41,
		"sortino":              3.18,
		"exposure": map[string]interface{}{
			"long":  850000,
			"short": 320000,
			"net":   530000,
		},
		"risk_limits": map[string]interface{}{
			"max_position":     100000,
			"max_loss_daily":   -10000,
			"max_leverage":     10.0,
			"position_timeout": 3600,
		},
		"alerts": []map[string]interface{}{
			{"level": "info", "message": "Position utilization normal", "timestamp": time.Now()},
		},
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(risk)
}

// ==================== Admin Handlers ====================

func (gw *APIGateway) handleAdminGetMarkets(w http.ResponseWriter, r *http.Request) {
	gw.marketsMu.RLock()
	defer gw.marketsMu.RUnlock()

	type MarketWithStats struct {
		*types.Market
		Volume   int64 `json:"volume"`
		BidDepth int64 `json:"bid_depth"`
		AskDepth int64 `json:"ask_depth"`
		MidPrice int64 `json:"mid_price"`
	}

	var markets []MarketWithStats
	for _, m := range gw.markets {
		ms := MarketWithStats{Market: m}

		gw.booksMu.RLock()
		if book, ok := gw.orderBooks[m.ID]; ok {
			for _, b := range book.Bids {
				ms.BidDepth += b.Amount
			}
			for _, a := range book.Asks {
				ms.AskDepth += a.Amount
			}
			if len(book.Bids) > 0 && len(book.Asks) > 0 {
				ms.MidPrice = (book.Bids[0].Price + book.Asks[0].Price) / 2
			}
		}
		gw.booksMu.RUnlock()

		// Calculate volume from trades
		gw.tradesMu.RLock()
		for _, t := range gw.trades {
			if t.MarketID == m.ID {
				ms.Volume += t.Amount
			}
		}
		gw.tradesMu.RUnlock()

		markets = append(markets, ms)
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(markets)
}

type CreateMarketRequest struct {
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Outcomes    []string `json:"outcomes"`
}

func (gw *APIGateway) handleAdminCreateMarket(w http.ResponseWriter, r *http.Request) {
	var req CreateMarketRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	marketID := utils.GenerateUUID()[:8]

	gw.marketsMu.Lock()
	gw.markets[marketID] = &types.Market{
		ID:          marketID,
		Name:        req.Name,
		Description: req.Description,
		Outcomes:    req.Outcomes,
		State:       types.MarketStateOpen,
		CreatedAt:   time.Now(),
	}
	gw.marketsMu.Unlock()

	gw.initializeOrderBook(marketID, 100000)

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{
		"id":      marketID,
		"status":  "created",
		"message": "Market created successfully",
	})
}

func (gw *APIGateway) handleAdminUpdateMarket(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	marketID := vars["id"]

	var update struct {
		State string `json:"state"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	gw.marketsMu.Lock()
	if market, ok := gw.markets[marketID]; ok {
		switch update.State {
		case "open":
			market.State = types.MarketStateOpen
		case "halted":
			market.State = types.MarketStateHalted
		case "resolved":
			market.State = types.MarketStateResolved
		}
	}
	gw.marketsMu.Unlock()

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "updated"})
}

func (gw *APIGateway) handleAdminGetUsers(w http.ResponseWriter, r *http.Request) {
	gw.usersMu.RLock()
	defer gw.usersMu.RUnlock()

	type UserInfo struct {
		ID        string    `json:"id"`
		Username  string    `json:"username"`
		Balance   int64     `json:"balance"`
		Hold      int64     `json:"hold"`
		Positions int       `json:"positions"`
		CreatedAt time.Time `json:"created_at"`
	}

	var users []UserInfo
	for _, u := range gw.users {
		users = append(users, UserInfo{
			ID:        u.ID,
			Username:  u.Username,
			Balance:   u.Balance,
			Hold:      u.Hold,
			Positions: len(u.Positions),
			CreatedAt: u.CreatedAt,
		})
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(users)
}

func (gw *APIGateway) handleAdminGetStats(w http.ResponseWriter, r *http.Request) {
	gw.statsMu.RLock()
	stats := *gw.stats
	gw.statsMu.RUnlock()

	// Add more detailed stats
	response := map[string]interface{}{
		"volume_24h":      stats.TotalVolume24h,
		"trades_24h":      stats.TotalTrades24h,
		"active_markets":  stats.ActiveMarkets,
		"total_users":     stats.TotalUsers,
		"total_liquidity": stats.TotalLiquidity,
		"last_updated":    stats.LastUpdated,
		"system_status":   "healthy",
		"uptime_hours":    time.Since(time.Now().Add(-24 * time.Hour)).Hours(),
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(response)
}

func (gw *APIGateway) handleAdminGetTrades(w http.ResponseWriter, r *http.Request) {
	limit := 100

	gw.tradesMu.RLock()
	defer gw.tradesMu.RUnlock()

	result := gw.trades
	if len(result) > limit {
		result = result[:limit]
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(result)
}

func main() {
	log.Println("API Gateway starting...")

	// Initialize event bus
	log.Println("Creating event bus...")
	eventBus := eventbus.NewEventBus()

	// Create API gateway
	log.Println("Creating API gateway...")
	gateway := NewAPIGateway(eventBus)

	// Start HTTP server
	addr := ":8080"
	log.Printf("API Gateway listening on %s", addr)
	if err := http.ListenAndServe(addr, gateway.router); err != nil {
		log.Fatalf("Server failed: %v", err)
	}
}
