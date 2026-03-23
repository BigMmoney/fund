package main

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
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
	router          *mux.Router
	eventBus        *eventbus.EventBus
	upgrader        websocket.Upgrader
	wsClients       map[*websocket.Conn]*wsClient
	wsClientsMu     sync.RWMutex
	httpClient      *http.Client
	rustCoreURL     string
	allowOrigins    map[string]struct{}
	globalLimiter   *fixedWindowRateLimiter
	userLimiter     *fixedWindowRateLimiter
	adminLimiter    *fixedWindowRateLimiter
	authTokens      map[string]AuthenticatedPrincipal
	internalSecret  []byte
	wsConnLimit     int
	demoWithdrawals bool
	demoAdminWrites bool
	demoHFTExecute  bool
	demoWebSocket   bool
	demoHFTStream   bool
	demoAdminUsers  bool
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

type wsClient struct {
	conn    *websocket.Conn
	writeMu sync.Mutex
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

type contextKey string

const principalContextKey contextKey = "principal"

type AuthenticatedPrincipal struct {
	Subject   string
	Role      string
	SessionID string
}

const internalAuthMaxSkew = 30 * time.Second
const maxJSONBodyBytes int64 = 16 * 1024

type fixedWindowRateLimiter struct {
	mu      sync.Mutex
	window  time.Duration
	entries map[string][]time.Time
}

func newFixedWindowRateLimiter(window time.Duration) *fixedWindowRateLimiter {
	return &fixedWindowRateLimiter{
		window:  window,
		entries: make(map[string][]time.Time),
	}
}

func (l *fixedWindowRateLimiter) Allow(key string, limit int) bool {
	if key == "" {
		key = "unknown"
	}
	now := time.Now()
	l.mu.Lock()
	defer l.mu.Unlock()
	windowStart := now.Add(-l.window)
	kept := l.entries[key][:0]
	for _, ts := range l.entries[key] {
		if ts.After(windowStart) {
			kept = append(kept, ts)
		}
	}
	if len(kept) >= limit {
		l.entries[key] = kept
		return false
	}
	kept = append(kept, now)
	l.entries[key] = kept
	return true
}

func defaultAllowedOrigins() map[string]struct{} {
	values := os.Getenv("GO_API_ALLOWED_ORIGINS")
	if strings.TrimSpace(values) == "" {
		values = "http://127.0.0.1:5173,http://localhost:5173"
	}
	allowed := make(map[string]struct{})
	for _, value := range strings.Split(values, ",") {
		value = strings.TrimSpace(value)
		if value != "" {
			allowed[value] = struct{}{}
		}
	}
	return allowed
}

func getEnvDefault(key, fallback string) string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	return value
}

func getEnvInt(key string, fallback int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil || parsed <= 0 {
		return fallback
	}
	return parsed
}

func parseAuthTokens(value string) map[string]AuthenticatedPrincipal {
	tokens := make(map[string]AuthenticatedPrincipal)
	for _, record := range strings.Split(value, ";") {
		record = strings.TrimSpace(record)
		if record == "" {
			continue
		}
		parts := strings.SplitN(record, "|", 3)
		if len(parts) != 3 {
			log.Printf("Skipping invalid GO_API_AUTH_TOKENS entry: %q", record)
			continue
		}
		token := strings.TrimSpace(parts[0])
		subject := strings.TrimSpace(parts[1])
		role := strings.ToLower(strings.TrimSpace(parts[2]))
		if token == "" || subject == "" || (role != "user" && role != "admin") {
			log.Printf("Skipping malformed GO_API_AUTH_TOKENS entry for subject=%q", subject)
			continue
		}
		tokens[token] = AuthenticatedPrincipal{Subject: subject, Role: role}
	}
	return tokens
}

func bearerToken(r *http.Request) string {
	value := strings.TrimSpace(r.Header.Get("Authorization"))
	if value == "" {
		return ""
	}
	parts := strings.SplitN(value, " ", 2)
	if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
		return ""
	}
	return strings.TrimSpace(parts[1])
}

func internalAuthPayload(method, path, query, subject, role, sessionID string, timestamp int64, requestID string) string {
	return strings.Join([]string{
		strings.ToUpper(method),
		path,
		query,
		subject,
		role,
		sessionID,
		strconv.FormatInt(timestamp, 10),
		requestID,
	}, "\n")
}

func splitPathAndQuery(path string) (string, string) {
	parts := strings.SplitN(path, "?", 2)
	if len(parts) == 2 {
		return parts[0], parts[1]
	}
	return path, ""
}

func signInternalAuth(secret []byte, method, path string, principal *AuthenticatedPrincipal, requestID string, timestamp int64) string {
	sessionID := ""
	if principal != nil {
		sessionID = principal.SessionID
	}
	cleanPath, query := splitPathAndQuery(path)
	payload := internalAuthPayload(method, cleanPath, query, principal.Subject, principal.Role, sessionID, timestamp, requestID)
	mac := hmac.New(sha256.New, secret)
	_, _ = mac.Write([]byte(payload))
	return hex.EncodeToString(mac.Sum(nil))
}

func bodySHA256Hex(body []byte) string {
	sum := sha256.Sum256(body)
	return hex.EncodeToString(sum[:])
}

func (gw *APIGateway) isAllowedOrigin(origin string) bool {
	if strings.TrimSpace(origin) == "" {
		return true
	}
	_, allowed := gw.allowOrigins[origin]
	return allowed
}

func (gw *APIGateway) corsMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		origin := r.Header.Get("Origin")
		if origin != "" && gw.isAllowedOrigin(origin) {
			w.Header().Set("Access-Control-Allow-Origin", origin)
			w.Header().Set("Vary", "Origin")
			w.Header().Set("Access-Control-Allow-Headers", "Authorization, Content-Type, X-Session-Id, X-Request-Id")
			w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, OPTIONS")
		}
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func clientIP(r *http.Request) string {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err == nil {
		return host
	}
	return r.RemoteAddr
}

func (gw *APIGateway) globalRateLimitMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !gw.globalLimiter.Allow("ip:"+clientIP(r), 60) {
			writeJSONError(w, http.StatusTooManyRequests, "rate limit exceeded")
			return
		}
		next.ServeHTTP(w, r)
	})
}

func principalFromContext(r *http.Request) *AuthenticatedPrincipal {
	principal, _ := r.Context().Value(principalContextKey).(*AuthenticatedPrincipal)
	return principal
}

func (gw *APIGateway) authenticate(r *http.Request) (*AuthenticatedPrincipal, error) {
	token := bearerToken(r)
	if token == "" {
		return nil, http.ErrNoCookie
	}
	principal, ok := gw.authTokens[token]
	if !ok {
		return nil, http.ErrNoCookie
	}
	principal.SessionID = strings.TrimSpace(r.Header.Get("X-Session-Id"))
	return &principal, nil
}

func (gw *APIGateway) withUserAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		principal, err := gw.authenticate(r)
		if err != nil {
			writeJSONError(w, http.StatusUnauthorized, "missing or invalid auth headers")
			return
		}
		if !gw.userLimiter.Allow("user:"+principal.Subject, 30) {
			writeJSONError(w, http.StatusTooManyRequests, "user rate limit exceeded")
			return
		}
		ctx := context.WithValue(r.Context(), principalContextKey, principal)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func (gw *APIGateway) withAdminAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		principal, err := gw.authenticate(r)
		if err != nil {
			writeJSONError(w, http.StatusUnauthorized, "missing or invalid auth headers")
			return
		}
		if principal.Role != "admin" {
			writeJSONError(w, http.StatusForbidden, "admin role required")
			return
		}
		if !gw.adminLimiter.Allow("admin:"+principal.Subject, 10) {
			writeJSONError(w, http.StatusTooManyRequests, "admin rate limit exceeded")
			return
		}
		ctx := context.WithValue(r.Context(), principalContextKey, principal)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func ensurePrincipalSubject(w http.ResponseWriter, principal *AuthenticatedPrincipal, claimedUserID string) bool {
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return false
	}
	if strings.TrimSpace(claimedUserID) == "" || claimedUserID != principal.Subject {
		writeJSONError(w, http.StatusForbidden, "user_id does not match authenticated subject")
		return false
	}
	return true
}

func normalizeAuthenticatedUserID(w http.ResponseWriter, principal *AuthenticatedPrincipal, claimedUserID string) (string, bool) {
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return "", false
	}
	claimedUserID = strings.TrimSpace(claimedUserID)
	if claimedUserID != "" && claimedUserID != principal.Subject {
		writeJSONError(w, http.StatusForbidden, "user_id does not match authenticated subject")
		return "", false
	}
	return principal.Subject, true
}

func writeJSONError(w http.ResponseWriter, status int, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]string{"status": "error", "error": message})
}

func decodeJSONBody(w http.ResponseWriter, r *http.Request, target interface{}) bool {
	r.Body = http.MaxBytesReader(w, r.Body, maxJSONBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		writeJSONError(w, http.StatusBadRequest, "invalid request body")
		return false
	}
	var trailing struct{}
	if err := decoder.Decode(&trailing); err != io.EOF {
		writeJSONError(w, http.StatusBadRequest, "unexpected trailing data")
		return false
	}
	return true
}

func requestID(r *http.Request) string {
	value := strings.TrimSpace(r.Header.Get("X-Request-Id"))
	if value == "" {
		return utils.GenerateID()
	}
	return value
}

func proxyPathWithQuery(path string, values url.Values) string {
	encoded := values.Encode()
	if encoded == "" {
		return path
	}
	return path + "?" + encoded
}

func (gw *APIGateway) attachInternalAuth(req *http.Request, method, path, requestID string, principal *AuthenticatedPrincipal) bool {
	if principal == nil {
		return true
	}
	if len(gw.internalSecret) == 0 {
		return false
	}
	timestamp := time.Now().Unix()
	signature := signInternalAuth(gw.internalSecret, method, path, principal, requestID, timestamp)
	req.Header.Set("X-Internal-Auth-Subject", principal.Subject)
	req.Header.Set("X-Internal-Auth-Role", principal.Role)
	if principal.SessionID != "" {
		req.Header.Set("X-Internal-Auth-Session-Id", principal.SessionID)
	}
	req.Header.Set("X-Internal-Auth-Timestamp", strconv.FormatInt(timestamp, 10))
	req.Header.Set("X-Internal-Auth-Signature", signature)
	return true
}

func (gw *APIGateway) proxyJSON(w http.ResponseWriter, r *http.Request, path string, payload interface{}, principal *AuthenticatedPrincipal) bool {
	body, err := json.Marshal(payload)
	if err != nil {
		writeJSONError(w, http.StatusInternalServerError, "failed to encode proxy payload")
		return false
	}
	req, err := http.NewRequestWithContext(r.Context(), http.MethodPost, gw.rustCoreURL+path, bytes.NewReader(body))
	if err != nil {
		writeJSONError(w, http.StatusBadGateway, "failed to construct proxy request")
		return false
	}
	req.Header.Set("Content-Type", "application/json")
	requestID := requestID(r)
	req.Header.Set("X-Request-Id", requestID)
	req.Header.Set("X-Internal-Auth-Body-Sha256", bodySHA256Hex(body))
	if !gw.attachInternalAuth(req, http.MethodPost, path, requestID, principal) {
		writeJSONError(w, http.StatusInternalServerError, "internal auth secret is not configured")
		return false
	}
	resp, err := gw.httpClient.Do(req)
	if err != nil {
		writeJSONError(w, http.StatusBadGateway, "rust core unavailable")
		return false
	}
	defer resp.Body.Close()
	for key, values := range resp.Header {
		for _, value := range values {
			w.Header().Add(key, value)
		}
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(resp.StatusCode)
	_, _ = io.Copy(w, resp.Body)
	return true
}

func (gw *APIGateway) proxyGET(w http.ResponseWriter, r *http.Request, path string, principal *AuthenticatedPrincipal) bool {
	req, err := http.NewRequestWithContext(r.Context(), http.MethodGet, gw.rustCoreURL+path, nil)
	if err != nil {
		writeJSONError(w, http.StatusBadGateway, "failed to construct proxy request")
		return false
	}
	requestID := requestID(r)
	req.Header.Set("X-Request-Id", requestID)
	if !gw.attachInternalAuth(req, http.MethodGet, path, requestID, principal) {
		writeJSONError(w, http.StatusInternalServerError, "internal auth secret is not configured")
		return false
	}
	resp, err := gw.httpClient.Do(req)
	if err != nil {
		writeJSONError(w, http.StatusBadGateway, "rust core unavailable")
		return false
	}
	defer resp.Body.Close()
	for key, values := range resp.Header {
		for _, value := range values {
			w.Header().Add(key, value)
		}
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(resp.StatusCode)
	_, _ = io.Copy(w, resp.Body)
	return true
}

func NewAPIGateway(eventBus *eventbus.EventBus) *APIGateway {
	authTokens := parseAuthTokens(getEnvDefault("GO_API_AUTH_TOKENS", ""))
	internalSecret := []byte(strings.TrimSpace(getEnvDefault("INTERNAL_AUTH_SHARED_SECRET", "")))
	gw := &APIGateway{
		router:          mux.NewRouter(),
		eventBus:        eventBus,
		wsClients:       make(map[*websocket.Conn]*wsClient),
		httpClient:      &http.Client{Timeout: 5 * time.Second},
		rustCoreURL:     strings.TrimRight(getEnvDefault("RUST_CORE_BASE_URL", "http://127.0.0.1:3030"), "/"),
		allowOrigins:    defaultAllowedOrigins(),
		globalLimiter:   newFixedWindowRateLimiter(time.Second),
		userLimiter:     newFixedWindowRateLimiter(time.Second),
		adminLimiter:    newFixedWindowRateLimiter(time.Second),
		authTokens:      authTokens,
		internalSecret:  internalSecret,
		wsConnLimit:     getEnvInt("GO_API_WS_CONN_LIMIT", 100),
		demoWithdrawals: strings.EqualFold(getEnvDefault("ENABLE_GO_DEMO_WITHDRAWALS", "false"), "true"),
		demoAdminWrites: strings.EqualFold(getEnvDefault("ENABLE_GO_DEMO_ADMIN_WRITES", "false"), "true"),
		demoHFTExecute:  strings.EqualFold(getEnvDefault("ENABLE_GO_DEMO_HFT_EXECUTE", "false"), "true"),
		demoWebSocket:   strings.EqualFold(getEnvDefault("ENABLE_GO_DEMO_WS", "false"), "true"),
		demoHFTStream:   strings.EqualFold(getEnvDefault("ENABLE_GO_DEMO_HFT_STREAM", "false"), "true"),
		demoAdminUsers:  strings.EqualFold(getEnvDefault("ENABLE_GO_DEMO_ADMIN_USERS", "false"), "true"),
		markets:         make(map[string]*types.Market),
		orderBooks:      make(map[string]*OrderBook),
		users:           make(map[string]*User),
		trades:          make([]Trade, 0),
		stats:           &PlatformStats{},
	}
	if len(gw.authTokens) == 0 {
		log.Printf("WARNING: GO_API_AUTH_TOKENS is empty; authenticated Go routes will reject all callers")
	}
	if len(gw.internalSecret) == 0 {
		log.Printf("WARNING: INTERNAL_AUTH_SHARED_SECRET is empty; authenticated proxying to Rust will fail until configured")
	}
	gw.upgrader = websocket.Upgrader{
		CheckOrigin: func(r *http.Request) bool { return gw.isAllowedOrigin(r.Header.Get("Origin")) },
	}
	gw.router.Use(gw.corsMiddleware)
	gw.router.Use(gw.globalRateLimitMiddleware)

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
	gw.router.Handle("/ws", gw.withUserAuth(http.HandlerFunc(gw.handleWebSocket)))

	// API routes
	gw.router.Handle("/v1/intents", gw.withUserAuth(http.HandlerFunc(gw.handleCreateIntent))).Methods("POST")
	gw.router.Handle("/v1/orders/{id}/cancel", gw.withUserAuth(http.HandlerFunc(gw.handleCancelOrder))).Methods("POST")
	gw.router.HandleFunc("/v1/markets", gw.handleGetMarkets).Methods("GET")
	gw.router.HandleFunc("/v1/markets/{id}", gw.handleGetMarket).Methods("GET")
	gw.router.HandleFunc("/v1/markets/{id}/book", gw.handleGetOrderBook).Methods("GET")
	gw.router.HandleFunc("/v1/markets/{id}/history", gw.handleGetPriceHistory).Methods("GET")
	gw.router.Handle("/v1/positions", gw.withUserAuth(http.HandlerFunc(gw.handleGetPositions))).Methods("GET")
	gw.router.Handle("/v1/balances", gw.withUserAuth(http.HandlerFunc(gw.handleGetBalances))).Methods("GET")
	gw.router.Handle("/v1/margin", gw.withUserAuth(http.HandlerFunc(gw.handleGetMargin))).Methods("GET")
	gw.router.Handle("/v1/pnl", gw.withUserAuth(http.HandlerFunc(gw.handleGetPnl))).Methods("GET")
	gw.router.Handle("/v1/otc/quotes", gw.withUserAuth(http.HandlerFunc(gw.handleCreateOtcQuote))).Methods("POST")
	gw.router.Handle("/v1/otc/quotes", gw.withUserAuth(http.HandlerFunc(gw.handleListOtcQuotes))).Methods("GET")
	gw.router.Handle("/v1/otc/quotes/{id}/accept", gw.withUserAuth(http.HandlerFunc(gw.handleAcceptOtcQuote))).Methods("POST")
	gw.router.Handle("/v1/earn/positions", gw.withUserAuth(http.HandlerFunc(gw.handleGetEarnPositions))).Methods("GET")
	gw.router.Handle("/v1/earn/subscribe", gw.withUserAuth(http.HandlerFunc(gw.handleEarnSubscribe))).Methods("POST")
	gw.router.Handle("/v1/earn/redeem", gw.withUserAuth(http.HandlerFunc(gw.handleEarnRedeem))).Methods("POST")
	gw.router.Handle("/v1/withdrawals", gw.withUserAuth(http.HandlerFunc(gw.handleCreateWithdrawal))).Methods("POST")
	gw.router.Handle("/v1/deposits", gw.withUserAuth(http.HandlerFunc(gw.handleGetDeposits))).Methods("GET")
	gw.router.HandleFunc("/v1/trades", gw.handleGetTrades).Methods("GET")
	gw.router.Handle("/v1/orders", gw.withUserAuth(http.HandlerFunc(gw.handleGetOrders))).Methods("GET")
	gw.router.HandleFunc("/v1/stats", gw.handleGetStats).Methods("GET")

	// HFT Trading routes
	gw.router.Handle("/hft/stream", gw.withUserAuth(http.HandlerFunc(gw.handleHFTStream))).Methods("GET")
	gw.router.Handle("/hft/execute", gw.withUserAuth(http.HandlerFunc(gw.handleHFTExecute))).Methods("POST")
	gw.router.HandleFunc("/hft/strategies", gw.handleHFTStrategies).Methods("GET")
	gw.router.HandleFunc("/hft/signals", gw.handleHFTSignals).Methods("GET")
	gw.router.HandleFunc("/hft/risk", gw.handleHFTRisk).Methods("GET")

	// Admin routes
	gw.router.Handle("/admin/markets", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminGetMarkets))).Methods("GET")
	gw.router.Handle("/admin/markets", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminCreateMarket))).Methods("POST")
	gw.router.Handle("/admin/markets/{id}", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminUpdateMarket))).Methods("PUT")
	gw.router.Handle("/admin/instruments", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminListInstruments))).Methods("GET")
	gw.router.Handle("/admin/kill-switch", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminKillSwitch))).Methods("POST")
	gw.router.Handle("/admin/market-state", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminSetMarketState))).Methods("POST")
	gw.router.Handle("/admin/risk/funding-rates", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminGetFundingRates))).Methods("GET")
	gw.router.Handle("/admin/risk/funding-rates", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminUpsertFundingRate))).Methods("POST")
	gw.router.Handle("/admin/risk/events", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminGetRiskEvents))).Methods("GET")
	gw.router.Handle("/admin/risk/governance/actions", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminGetGovernanceActions))).Methods("GET")
	gw.router.Handle("/admin/risk/governance/actions/{id}/approve", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminApproveGovernanceAction))).Methods("POST")
	gw.router.Handle("/admin/risk/governance/actions/{id}/reject", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminRejectGovernanceAction))).Methods("POST")
	gw.router.Handle("/admin/users", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminGetUsers))).Methods("GET")
	gw.router.Handle("/admin/stats", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminGetStats))).Methods("GET")
	gw.router.Handle("/admin/trades", gw.withAdminAuth(http.HandlerFunc(gw.handleAdminGetTrades))).Methods("GET")

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
	targetURL := "http://127.0.0.1:8081" + proxyPathWithQuery(r.URL.Path, r.URL.Query())
	req, err := http.NewRequestWithContext(r.Context(), http.MethodGet, targetURL, nil)
	if err != nil {
		writeJSONError(w, http.StatusBadGateway, "failed to construct price service request")
		return
	}
	resp, err := gw.httpClient.Do(req)
	if err != nil {
		writeJSONError(w, http.StatusServiceUnavailable, "price service unavailable")
		return
	}
	defer resp.Body.Close()

	for key, values := range resp.Header {
		for _, value := range values {
			w.Header().Add(key, value)
		}
	}
	w.WriteHeader(resp.StatusCode)
	_, _ = io.Copy(w, resp.Body)
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


type OtcQuoteCreateRequest struct {
	UserID   string `json:"user_id"`
	MarketID string `json:"market_id"`
	Side     string `json:"side"`
	Price    int64  `json:"price"`
	Amount   int64  `json:"amount"`
	Outcome  int    `json:"outcome"`
}

type EarnFlowRequest struct {
	UserID    string `json:"user_id"`
	ProductID string `json:"product_id"`
	Amount    int64  `json:"amount"`
}

func (gw *APIGateway) handleCreateIntent(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	var req CreateIntentRequest
	if !decodeJSONBody(w, r, &req) {
		return
	}
	userID, ok := normalizeAuthenticatedUserID(w, principal, req.UserID)
	if !ok {
		return
	}

	// Validate request
	if req.MarketID == "" || req.Side == "" {
		http.Error(w, "missing required fields", http.StatusBadRequest)
		return
	}

	if req.Side != "buy" && req.Side != "sell" {
		http.Error(w, "side must be 'buy' or 'sell'", http.StatusBadRequest)
		return
	}

	payload := map[string]interface{}{
		"request_id":      requestID(r),
		"client_order_id": utils.GenerateID(),
		"user_id":         userID,
		"session_id":      principal.SessionID,
		"market_id":       req.MarketID,
		"side":            req.Side,
		"price":           req.Price,
		"amount":          req.Amount,
		"outcome":         req.Outcome,
	}
	if req.ExpiresIn > 0 {
		payload["expires_at"] = time.Now().Add(time.Duration(req.ExpiresIn) * time.Second).UTC().Format(time.RFC3339)
	}
	gw.proxyJSON(w, r, "/submit-order", payload, principal)
	log.Printf("Intent proxied to Rust core: user=%s request_id=%s", userID, payload["request_id"])
}

// Cancel order
func (gw *APIGateway) handleCancelOrder(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	vars := mux.Vars(r)
	orderID := vars["id"]
	marketID := strings.TrimSpace(r.URL.Query().Get("market_id"))
	if marketID == "" {
		writeJSONError(w, http.StatusBadRequest, "market_id query parameter required")
		return
	}
	payload := map[string]interface{}{
		"request_id":      requestID(r),
		"user_id":         principal.Subject,
		"market_id":       marketID,
		"order_id":        orderID,
		"client_order_id": nil,
	}
	if outcome := strings.TrimSpace(r.URL.Query().Get("outcome")); outcome != "" {
		parsed, err := strconv.Atoi(outcome)
		if err != nil {
			writeJSONError(w, http.StatusBadRequest, "invalid outcome query parameter")
			return
		}
		payload["outcome"] = parsed
	}
	gw.proxyJSON(w, r, "/cancel-order", payload, principal)
	log.Printf("Cancel proxied to Rust core: user=%s order=%s", principal.Subject, orderID)
}

// Get markets
func (gw *APIGateway) handleGetMarkets(w http.ResponseWriter, r *http.Request) {
	gw.proxyGET(w, r, "/markets", nil)
}

// Get single market
func (gw *APIGateway) handleGetMarket(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	marketID := vars["id"]
	gw.proxyGET(w, r, "/markets/"+url.PathEscape(marketID), nil)
}

// Get order book
func (gw *APIGateway) handleGetOrderBook(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	marketID := vars["id"]
	gw.proxyGET(w, r, proxyPathWithQuery("/markets/"+url.PathEscape(marketID)+"/book", r.URL.Query()), nil)
}

// Get positions
func (gw *APIGateway) handleGetPositions(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	userID := r.URL.Query().Get("user_id")
	resolvedUserID, ok := normalizeAuthenticatedUserID(w, principal, userID)
	if !ok {
		return
	}
	gw.proxyGET(w, r, "/positions/"+resolvedUserID, principal)
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
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	userID := r.URL.Query().Get("user_id")
	resolvedUserID, ok := normalizeAuthenticatedUserID(w, principal, userID)
	if !ok {
		return
	}
	gw.proxyGET(w, r, "/balances/"+resolvedUserID, principal)
}

func (gw *APIGateway) handleGetMargin(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	userID := r.URL.Query().Get("user_id")
	resolvedUserID, ok := normalizeAuthenticatedUserID(w, principal, userID)
	if !ok {
		return
	}
	gw.proxyGET(w, r, proxyPathWithQuery("/margin/"+resolvedUserID, r.URL.Query()), principal)
}

func (gw *APIGateway) handleGetPnl(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	userID := r.URL.Query().Get("user_id")
	resolvedUserID, ok := normalizeAuthenticatedUserID(w, principal, userID)
	if !ok {
		return
	}
	gw.proxyGET(w, r, proxyPathWithQuery("/pnl/"+resolvedUserID, r.URL.Query()), principal)
}

func (gw *APIGateway) handleCreateOtcQuote(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	var req OtcQuoteCreateRequest
	if !decodeJSONBody(w, r, &req) {
		return
	}
	userID, ok := normalizeAuthenticatedUserID(w, principal, req.UserID)
	if !ok {
		return
	}
	payload := map[string]interface{}{
		"user_id":   userID,
		"market_id": req.MarketID,
		"side":      req.Side,
		"price":     req.Price,
		"amount":    req.Amount,
		"outcome":   req.Outcome,
	}
	gw.proxyJSON(w, r, "/otc/quotes", payload, principal)
}

func (gw *APIGateway) handleListOtcQuotes(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.proxyGET(w, r, "/otc/quotes", principal)
}

func (gw *APIGateway) handleAcceptOtcQuote(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	vars := mux.Vars(r)
	quoteID := vars["id"]
	gw.proxyJSON(w, r, "/otc/quotes/"+url.PathEscape(quoteID)+"/accept", map[string]interface{}{}, principal)
}

func (gw *APIGateway) handleGetEarnPositions(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	userID := r.URL.Query().Get("user_id")
	resolvedUserID, ok := normalizeAuthenticatedUserID(w, principal, userID)
	if !ok {
		return
	}
	gw.proxyGET(w, r, "/earn/positions/"+url.PathEscape(resolvedUserID), principal)
}

func (gw *APIGateway) handleEarnSubscribe(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	var req EarnFlowRequest
	if !decodeJSONBody(w, r, &req) {
		return
	}
	userID, ok := normalizeAuthenticatedUserID(w, principal, req.UserID)
	if !ok {
		return
	}
	payload := map[string]interface{}{
		"user_id":    userID,
		"product_id": req.ProductID,
		"amount":     req.Amount,
	}
	gw.proxyJSON(w, r, "/earn/subscribe", payload, principal)
}

func (gw *APIGateway) handleEarnRedeem(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	var req EarnFlowRequest
	if !decodeJSONBody(w, r, &req) {
		return
	}
	userID, ok := normalizeAuthenticatedUserID(w, principal, req.UserID)
	if !ok {
		return
	}
	payload := map[string]interface{}{
		"user_id":    userID,
		"product_id": req.ProductID,
		"amount":     req.Amount,
	}
	gw.proxyJSON(w, r, "/earn/redeem", payload, principal)
}

// Create withdrawal
type WithdrawalRequest struct {
	UserID  string `json:"user_id"`
	Amount  int64  `json:"amount"`
	Address string `json:"address"`
}

func (gw *APIGateway) handleCreateWithdrawal(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	var req WithdrawalRequest
	if !decodeJSONBody(w, r, &req) {
		return
	}
	userID, ok := normalizeAuthenticatedUserID(w, principal, req.UserID)
	if !ok {
		return
	}
	req.UserID = userID

	if req.UserID == "" || req.Amount <= 0 || req.Address == "" {
		http.Error(w, "missing required fields", http.StatusBadRequest)
		return
	}
	if !gw.demoWithdrawals {
		writeJSONError(w, http.StatusNotImplemented, "withdrawals are disabled in compatibility mode; use Rust core workflow")
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

	log.Printf("Withdrawal requested in demo mode: user=%s, amount=%d, request_id=%s", req.UserID, req.Amount, requestID(r))
}

// WebSocket handling
func (gw *APIGateway) handleWebSocket(w http.ResponseWriter, r *http.Request) {
	if !gw.demoWebSocket {
		writeJSONError(w, http.StatusNotImplemented, "compatibility websocket is disabled; use Rust canonical HTTP reads")
		return
	}
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.wsClientsMu.RLock()
	currentClients := len(gw.wsClients)
	gw.wsClientsMu.RUnlock()
	if currentClients >= gw.wsConnLimit {
		writeJSONError(w, http.StatusTooManyRequests, "websocket connection limit exceeded")
		return
	}
	conn, err := gw.upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("WebSocket upgrade failed: %v", err)
		return
	}
	conn.SetReadLimit(64 * 1024)
	client := &wsClient{conn: conn}

	gw.wsClientsMu.Lock()
	gw.wsClients[conn] = client
	gw.wsClientsMu.Unlock()

	log.Printf("WebSocket client connected: subject=%s addr=%s", principal.Subject, conn.RemoteAddr())

	// Clean up on disconnect
	defer func() {
		gw.removeWSClient(conn)
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
		if err := gw.writeWSMessage(client, messageType, message); err != nil {
			break
		}
	}
}

func (gw *APIGateway) removeWSClient(conn *websocket.Conn) {
	gw.wsClientsMu.Lock()
	client, ok := gw.wsClients[conn]
	if ok {
		delete(gw.wsClients, conn)
	}
	gw.wsClientsMu.Unlock()
	if ok {
		_ = client.conn.Close()
	}
}

func (gw *APIGateway) writeWSMessage(client *wsClient, messageType int, payload []byte) error {
	client.writeMu.Lock()
	defer client.writeMu.Unlock()
	_ = client.conn.SetWriteDeadline(time.Now().Add(2 * time.Second))
	err := client.conn.WriteMessage(messageType, payload)
	_ = client.conn.SetWriteDeadline(time.Time{})
	return err
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
	clients := make([]*wsClient, 0, len(gw.wsClients))
	for _, client := range gw.wsClients {
		clients = append(clients, client)
	}
	gw.wsClientsMu.RUnlock()

	for _, client := range clients {
		if err := gw.writeWSMessage(client, websocket.TextMessage, message); err != nil {
			log.Printf("Failed to send to WebSocket client: %v", err)
			gw.removeWSClient(client.conn)
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
	values := r.URL.Query()
	userID := strings.TrimSpace(values.Get("user_id"))
	if userID != "" {
		principal, err := gw.authenticate(r)
		if err != nil {
			writeJSONError(w, http.StatusUnauthorized, "missing or invalid auth headers")
			return
		}
		resolvedUserID, ok := normalizeAuthenticatedUserID(w, principal, userID)
		if !ok {
			return
		}
		values.Set("user_id", resolvedUserID)
		gw.proxyGET(w, r, proxyPathWithQuery("/trades", values), principal)
		return
	}
	gw.proxyGET(w, r, proxyPathWithQuery("/trades", values), nil)
}

// handleGetPriceHistory returns price history for a market
func (gw *APIGateway) handleGetPriceHistory(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	marketID := vars["id"]
	gw.proxyGET(w, r, proxyPathWithQuery("/markets/"+url.PathEscape(marketID)+"/history", r.URL.Query()), nil)
}

// handleGetDeposits returns deposit history
func (gw *APIGateway) handleGetDeposits(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	values := r.URL.Query()
	resolvedUserID, ok := normalizeAuthenticatedUserID(w, principal, values.Get("user_id"))
	if !ok {
		return
	}
	values.Del("user_id")
	gw.proxyGET(w, r, proxyPathWithQuery("/deposits/"+url.PathEscape(resolvedUserID), values), principal)
}

// handleGetOrders returns user's open orders
func (gw *APIGateway) handleGetOrders(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	values := r.URL.Query()
	resolvedUserID, ok := normalizeAuthenticatedUserID(w, principal, values.Get("user_id"))
	if !ok {
		return
	}
	values.Del("user_id")
	gw.proxyGET(w, r, proxyPathWithQuery("/orders/"+url.PathEscape(resolvedUserID), values), principal)
}

// handleGetStats returns platform statistics
func (gw *APIGateway) handleGetStats(w http.ResponseWriter, r *http.Request) {
	gw.proxyGET(w, r, "/stats", nil)
}

// ==================== HFT Handlers ====================

// HFT WebSocket stream for real-time market data
func (gw *APIGateway) handleHFTStream(w http.ResponseWriter, r *http.Request) {
	if !gw.demoHFTStream {
		writeJSONError(w, http.StatusNotImplemented, "compatibility HFT stream is disabled; use Rust canonical market reads")
		return
	}
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.wsClientsMu.RLock()
	currentClients := len(gw.wsClients)
	gw.wsClientsMu.RUnlock()
	if currentClients >= gw.wsConnLimit {
		writeJSONError(w, http.StatusTooManyRequests, "websocket connection limit exceeded")
		return
	}
	conn, err := gw.upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("HFT WebSocket upgrade failed: %v", err)
		return
	}
	defer conn.Close()

	log.Printf("HFT client connected: subject=%s addr=%s", principal.Subject, conn.RemoteAddr())

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
			_ = conn.SetWriteDeadline(time.Now().Add(2 * time.Second))
			if err := conn.WriteJSON(data); err != nil {
				return
			}
			_ = conn.SetWriteDeadline(time.Time{})
		case <-r.Context().Done():
			return
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
	if !gw.demoHFTExecute {
		writeJSONError(w, http.StatusNotImplemented, "HFT execute is disabled in compatibility mode; route through Rust core order entry")
		return
	}
	startTime := time.Now()

	var req HFTExecuteRequest
	if !decodeJSONBody(w, r, &req) {
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
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.proxyGET(w, r, proxyPathWithQuery("/markets", r.URL.Query()), principal)
}

func (gw *APIGateway) handleAdminListInstruments(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.proxyGET(w, r, "/admin/instruments", principal)
}

func (gw *APIGateway) handleAdminKillSwitch(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	var req map[string]interface{}
	if !decodeJSONBody(w, r, &req) {
		return
	}
	gw.proxyJSON(w, r, "/admin/kill-switch", req, principal)
}

func (gw *APIGateway) handleAdminSetMarketState(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	var req map[string]interface{}
	if !decodeJSONBody(w, r, &req) {
		return
	}
	gw.proxyJSON(w, r, "/admin/market-state", req, principal)
}

func (gw *APIGateway) handleAdminGetFundingRates(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.proxyGET(w, r, proxyPathWithQuery("/admin/risk/funding-rates", r.URL.Query()), principal)
}

func (gw *APIGateway) handleAdminUpsertFundingRate(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	var req map[string]interface{}
	if !decodeJSONBody(w, r, &req) {
		return
	}
	gw.proxyJSON(w, r, "/admin/risk/funding-rates", req, principal)
}

func (gw *APIGateway) handleAdminGetRiskEvents(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.proxyGET(w, r, proxyPathWithQuery("/admin/risk/events", r.URL.Query()), principal)
}

func (gw *APIGateway) handleAdminGetGovernanceActions(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.proxyGET(w, r, proxyPathWithQuery("/admin/risk/governance/actions", r.URL.Query()), principal)
}

func (gw *APIGateway) handleAdminApproveGovernanceAction(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	vars := mux.Vars(r)
	actionID := vars["id"]
	gw.proxyJSON(w, r, "/admin/risk/governance/actions/"+url.PathEscape(actionID)+"/approve", map[string]interface{}{}, principal)
}

func (gw *APIGateway) handleAdminRejectGovernanceAction(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	vars := mux.Vars(r)
	actionID := vars["id"]
	gw.proxyJSON(w, r, "/admin/risk/governance/actions/"+url.PathEscape(actionID)+"/reject", map[string]interface{}{}, principal)
}

type CreateMarketRequest struct {
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Outcomes    []string `json:"outcomes"`
}

func (gw *APIGateway) handleAdminCreateMarket(w http.ResponseWriter, r *http.Request) {
	if !gw.demoAdminWrites {
		writeJSONError(w, http.StatusNotImplemented, "admin market writes are disabled in compatibility mode; use Rust control plane")
		return
	}
	var req CreateMarketRequest
	if !decodeJSONBody(w, r, &req) {
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
	if !gw.demoAdminWrites {
		writeJSONError(w, http.StatusNotImplemented, "admin market writes are disabled in compatibility mode; use Rust control plane")
		return
	}
	vars := mux.Vars(r)
	marketID := vars["id"]

	var update struct {
		State string `json:"state"`
	}
	if !decodeJSONBody(w, r, &update) {
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
	if !gw.demoAdminUsers {
		writeJSONError(w, http.StatusNotImplemented, "compatibility admin users view is disabled; no canonical Rust user directory endpoint exists yet")
		return
	}
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
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	gw.proxyGET(w, r, "/stats", principal)
}

func (gw *APIGateway) handleAdminGetTrades(w http.ResponseWriter, r *http.Request) {
	principal := principalFromContext(r)
	if principal == nil {
		writeJSONError(w, http.StatusUnauthorized, "missing principal")
		return
	}
	values := r.URL.Query()
	if strings.TrimSpace(values.Get("limit")) == "" {
		values.Set("limit", "100")
	}
	gw.proxyGET(w, r, proxyPathWithQuery("/trades", values), principal)
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
	addr := getEnvDefault("GO_API_BIND_ADDR", "127.0.0.1:8080")
	log.Printf("API Gateway listening on %s", addr)
	server := &http.Server{
		Addr:              addr,
		Handler:           gateway.router,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      15 * time.Second,
		IdleTimeout:       60 * time.Second,
		MaxHeaderBytes:    1 << 20,
	}
	if err := server.ListenAndServe(); err != nil {
		log.Fatalf("Server failed: %v", err)
	}
}
