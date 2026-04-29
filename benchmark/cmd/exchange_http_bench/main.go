package main

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/google/uuid"
)

type config struct {
	BaseURL            string
	Secret             string
	APIKeyTemplate     string
	APISecretTemplate  string
	Market             string
	Buyers             []string
	Sellers            []string
	PairCount          int
	PairConcurrency    int
	BasePrice          int64
	Amount             int64
	RateLimitPerSecond int
	Prefix             string
	DebugProgress      bool
	DisableKeepAlives  bool
	AdminSubject       string
	CashThresholdBps   int64
	CashTargetBps      int64
	PosThresholdUnits  int64
	PosTargetUnits     int64
	InitialCash        int64
	InitialPosition    int64
	RateLimitRetryMax  int
	RateLimitBackoffMs int
	RequestStaggerMs   int
	SeedUsers          bool
	SeedCashAmount     int64
	SeedPositionAmount int64
	SeedRetryMax       int
	SeedRetryBackoffMs int
	SeedRequestDelayMs int
}

type orderBody struct {
	MarketID      string `json:"market_id"`
	Side          string `json:"side"`
	OrderType     string `json:"order_type"`
	Price         int64  `json:"price"`
	Amount        int64  `json:"amount"`
	Outcome       int    `json:"outcome"`
	TimeInForce   string `json:"time_in_force"`
	ClientOrderID string `json:"client_order_id"`
}

type granularTiming struct {
	ValidationUs        float64 `json:"validation_us"`
	RiskUs              float64 `json:"risk_us"`
	MatchingCoreUs      float64 `json:"matching_core_us"`
	SettlementPersistUs float64 `json:"settlement_persist_us"`
	PostMatchUs         float64 `json:"post_match_us"`
}

type orderResponse struct {
	Fills            int            `json:"fills"`
	QueueWaitUs      float64        `json:"queue_wait_us"`
	MatchExecutionUs float64        `json:"match_execution_us"`
	PersistUs        float64        `json:"persist_us"`
	GranularTiming   granularTiming `json:"granular_timing"`
}

type errorResponse struct {
	Status  string                 `json:"status"`
	Code    string                 `json:"code"`
	Message string                 `json:"message"`
	Error   string                 `json:"error"`
	Details map[string]interface{} `json:"details"`
}

type requestResult struct {
	StatusCode          int     `json:"status_code"`
	LatencyUs           float64 `json:"latency_us"`
	Fills               int     `json:"fills"`
	RetryCount          int     `json:"retry_count"`
	PrepareOrderUs      float64 `json:"prepare_order_us"`
	EncodeRequestUs     float64 `json:"encode_request_us"`
	BuildAndSignUs      float64 `json:"build_and_sign_request_us"`
	HttpRoundTripUs     float64 `json:"http_roundtrip_us"`
	ResponseReadUs      float64 `json:"response_read_us"`
	ResponseParseUs     float64 `json:"response_parse_us"`
	RetryBackoffUs      float64 `json:"retry_backoff_us"`
	RecoveryActionUs    float64 `json:"recovery_action_us"`
	QueueWaitUs         float64 `json:"queue_wait_us"`
	RiskUs              float64 `json:"risk_us"`
	MatchingCoreUs      float64 `json:"matching_core_us"`
	SettlementPersistUs float64 `json:"settlement_persist_us"`
	PostMatchUs         float64 `json:"post_match_us"`
	PreSubmitAvailable  float64 `json:"pre_submit_available_balance"`
	SuccessType         string  `json:"success_type,omitempty"`
	ErrorCode           string  `json:"error_code,omitempty"`
	ErrorMessage        string  `json:"error_message,omitempty"`
	ErrorCategory       string  `json:"error_category,omitempty"`
	TriggerHint         string  `json:"trigger_hint,omitempty"`
}

type numericSummary struct {
	Count int     `json:"count"`
	Min   float64 `json:"min"`
	P50   float64 `json:"p50"`
	P95   float64 `json:"p95"`
	P99   float64 `json:"p99"`
	P999  float64 `json:"p999"`
	Max   float64 `json:"max"`
	Avg   float64 `json:"avg"`
}

type pathSummary struct {
	TotalRequests       int                       `json:"total_requests"`
	SuccessCount        int                       `json:"success_count"`
	ErrorCount          int                       `json:"error_count"`
	StatusBreakdown     map[string]int            `json:"status_breakdown"`
	ClientLatencyUs     numericSummary            `json:"client_latency_us"`
	PrepareOrderUs      numericSummary            `json:"prepare_order_us"`
	EncodeRequestUs     numericSummary            `json:"encode_request_us"`
	BuildAndSignUs      numericSummary            `json:"build_and_sign_request_us"`
	HttpRoundTripUs     numericSummary            `json:"http_roundtrip_us"`
	ResponseReadUs      numericSummary            `json:"response_read_us"`
	ResponseParseUs     numericSummary            `json:"response_parse_us"`
	RetryBackoffUs      numericSummary            `json:"retry_backoff_us"`
	RecoveryActionUs    numericSummary            `json:"recovery_action_us"`
	PreSubmitAvailable  numericSummary            `json:"pre_submit_available_balance"`
	QueueWaitUs         numericSummary            `json:"queue_wait_us"`
	RiskUs              numericSummary            `json:"risk_us"`
	MatchingCoreUs      numericSummary            `json:"matching_core_us"`
	SettlementPersistUs numericSummary            `json:"settlement_persist_us"`
	PostMatchUs         numericSummary            `json:"post_match_us"`
	PerMarket           map[string]numericSummary `json:"per_market,omitempty"`
}

type benchmarkSummary struct {
	ClientImpl          string                    `json:"client_impl"`
	ClientMode          string                    `json:"client_mode"`
	ClientModeDesc      string                    `json:"client_mode_description"`
	PrimaryMetricBasis  string                    `json:"primary_metric_basis"`
	Market              string                    `json:"market"`
	TotalRequests       int                       `json:"total_requests"`
	SuccessCount        int                       `json:"success_count"`
	ErrorCount          int                       `json:"error_count"`
	SuccessRate         float64                   `json:"success_rate"`
	Http4xxCount        int                       `json:"http_4xx_count"`
	Http429Count        int                       `json:"http_429_count"`
	FillsReported       int                       `json:"fills_reported"`
	PreSubmitAvailable  numericSummary            `json:"pre_submit_available_balance"`
	TopupCount          int64                     `json:"topup_count"`
	TopupAmount         int64                     `json:"topup_amount"`
	RetryCount          int64                     `json:"retry_count"`
	StatusBreakdown     map[string]int            `json:"status_breakdown"`
	ClientLatencyUs     numericSummary            `json:"client_latency_us"`
	PrepareOrderUs      numericSummary            `json:"prepare_order_us"`
	EncodeRequestUs     numericSummary            `json:"encode_request_us"`
	BuildAndSignUs      numericSummary            `json:"build_and_sign_request_us"`
	HttpRoundTripUs     numericSummary            `json:"http_roundtrip_us"`
	ResponseReadUs      numericSummary            `json:"response_read_us"`
	ResponseParseUs     numericSummary            `json:"response_parse_us"`
	RetryBackoffUs      numericSummary            `json:"retry_backoff_us"`
	RecoveryActionUs    numericSummary            `json:"recovery_action_us"`
	QueueWaitUs         numericSummary            `json:"queue_wait_us"`
	RiskUs              numericSummary            `json:"risk_us"`
	MatchingCoreUs      numericSummary            `json:"matching_core_us"`
	SettlementPersistUs numericSummary            `json:"settlement_persist_us"`
	PostMatchUs         numericSummary            `json:"post_match_us"`
	PerMarket           map[string]numericSummary `json:"per_market"`
	SuccessPath         pathSummary               `json:"success_path"`
	ErrorPath           pathSummary               `json:"error_path"`
	DirectSuccessPath   pathSummary               `json:"direct_success_path"`
	RescuedSuccessPath  pathSummary               `json:"rescued_success_path"`
	FlowControlPath     pathSummary               `json:"flow_controlled_success_path"`
	SuccessCategories   []successCategorySummary  `json:"success_categories,omitempty"`
	ErrorCategories     []errorCategorySummary    `json:"error_categories,omitempty"`
}

type errorCategorySummary struct {
	Category        string         `json:"category"`
	Count           int            `json:"count"`
	SharePct        float64        `json:"share_pct"`
	ClientLatencyUs numericSummary `json:"client_latency_us"`
	TriggerHint     string         `json:"trigger_hint"`
}

type successCategorySummary struct {
	SuccessType     string         `json:"success_type"`
	Count           int            `json:"count"`
	ClientLatencyUs numericSummary `json:"client_latency_us"`
}

type benchmarkCounters struct {
	TopupCount  atomic.Int64
	TopupAmount atomic.Int64
	RetryCount  atomic.Int64
}

type benchmarkState struct {
	mu        sync.Mutex
	cash      map[string]int64
	positions map[string]int64
	cfg       config
}

type preparedOrder struct {
	Body               orderBody
	PreSubmitAvailable float64
	ReservedAmount     int64
}

func main() {
	cfg := parseFlags()
	transport := &http.Transport{
		DisableKeepAlives:   cfg.DisableKeepAlives,
		MaxIdleConns:        512,
		MaxIdleConnsPerHost: 256,
		MaxConnsPerHost:     256,
		IdleConnTimeout:     90 * time.Second,
	}
	client := &http.Client{
		Timeout:   30 * time.Second,
		Transport: transport,
	}
	defer transport.CloseIdleConnections()

	if cfg.SeedUsers {
		if err := seedUsers(client, cfg); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}

	results, counters := runBenchmark(client, cfg)
	if cfg.DebugProgress {
		fmt.Fprintf(os.Stderr, "summary_start results=%d\n", len(results))
	}
	summary := buildSummary(results, cfg, counters)
	if cfg.DebugProgress {
		fmt.Fprintln(os.Stderr, "summary_done")
	}

	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(summary); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func parseFlags() config {
	var cfg config
	var buyersCSV string
	var sellersCSV string

	flag.StringVar(&cfg.BaseURL, "base-url", "http://127.0.0.1:3131", "API base URL")
	flag.StringVar(&cfg.Secret, "secret", "", "internal auth secret")
	flag.StringVar(&cfg.APIKeyTemplate, "api-key-template", "", "optional API key or fmt template (for example bench-%s) used for trade requests")
	flag.StringVar(&cfg.APISecretTemplate, "api-secret-template", "", "optional API key secret or fmt template (for example secret-%s) used for trade requests")
	flag.StringVar(&cfg.Market, "market", "btc-usdt", "market id")
	flag.StringVar(&buyersCSV, "buyers", "", "comma-separated buyer subjects")
	flag.StringVar(&sellersCSV, "sellers", "", "comma-separated seller subjects")
	flag.IntVar(&cfg.PairCount, "pair-count", 40, "number of maker/taker pairs")
	flag.IntVar(&cfg.PairConcurrency, "pair-concurrency", 3, "pairs submitted per wave")
	flag.Int64Var(&cfg.BasePrice, "base-price", 50000, "base order price")
	flag.Int64Var(&cfg.Amount, "amount", 1, "order amount")
	flag.IntVar(&cfg.RateLimitPerSecond, "rate-limit-per-second", 48, "wave pacing rate")
	flag.StringVar(&cfg.Prefix, "prefix", "go-http", "client order id prefix")
	flag.BoolVar(&cfg.DebugProgress, "debug-progress", false, "emit progress logs to stderr")
	flag.BoolVar(&cfg.DisableKeepAlives, "disable-keep-alives", true, "disable HTTP keep-alives for a conservative client profile")
	flag.StringVar(&cfg.AdminSubject, "admin-subject", "bench-admin", "admin subject used for dynamic benchmark top-ups")
	flag.Int64Var(&cfg.CashThresholdBps, "cash-threshold-bps", 30000, "top up buyer cash when available falls below required_notional * bps / 10000")
	flag.Int64Var(&cfg.CashTargetBps, "cash-target-bps", 240000, "top up buyer cash to required_notional * bps / 10000")
	flag.Int64Var(&cfg.PosThresholdUnits, "position-threshold-units", 4, "top up seller position when available units fall below this threshold")
	flag.Int64Var(&cfg.PosTargetUnits, "position-target-units", 32, "top up seller position to this target amount")
	flag.Int64Var(&cfg.InitialCash, "initial-cash", 100000000, "initial modeled buyer/seller cash per account before dynamic top-ups")
	flag.Int64Var(&cfg.InitialPosition, "initial-position", 20000, "initial modeled seller position per account before dynamic top-ups")
	flag.IntVar(&cfg.RateLimitRetryMax, "rate-limit-retry-max", 2, "retry HTTP 429 responses this many times before classifying as an error")
	flag.IntVar(&cfg.RateLimitBackoffMs, "rate-limit-backoff-ms", 150, "base backoff in milliseconds for HTTP 429 retries")
	flag.IntVar(&cfg.RequestStaggerMs, "request-stagger-ms", 5, "small per-request stagger to avoid rate-limit burst pollution without changing measured request latency")
	flag.BoolVar(&cfg.SeedUsers, "seed-users", false, "seed buyer and seller accounts through admin deposit APIs before running the benchmark")
	flag.Int64Var(&cfg.SeedCashAmount, "seed-cash-amount", 500000000, "cash amount to seed into each buyer and seller before the benchmark")
	flag.Int64Var(&cfg.SeedPositionAmount, "seed-position-amount", 200000, "position amount to seed into each seller before the benchmark")
	flag.IntVar(&cfg.SeedRetryMax, "seed-retry-max", 6, "retry admin seed requests this many times on HTTP 429/5xx or transient transport errors")
	flag.IntVar(&cfg.SeedRetryBackoffMs, "seed-retry-backoff-ms", 200, "base backoff in milliseconds for admin seed retries")
	flag.IntVar(&cfg.SeedRequestDelayMs, "seed-request-delay-ms", 25, "small delay between sequential admin seed requests to avoid limiter spikes")
	flag.Parse()

	cfg.Buyers = splitCSV(buyersCSV)
	cfg.Sellers = splitCSV(sellersCSV)

	if cfg.Secret == "" || len(cfg.Buyers) == 0 || len(cfg.Sellers) == 0 {
		fmt.Fprintln(os.Stderr, "secret, buyers, and sellers are required")
		os.Exit(2)
	}
	if (cfg.APIKeyTemplate == "") != (cfg.APISecretTemplate == "") {
		fmt.Fprintln(os.Stderr, "api-key-template and api-secret-template must be provided together")
		os.Exit(2)
	}
	if cfg.PairCount <= 0 {
		fmt.Fprintln(os.Stderr, "pair-count must be > 0")
		os.Exit(2)
	}
	if cfg.PairConcurrency <= 0 {
		cfg.PairConcurrency = 1
	}
	if cfg.RateLimitPerSecond <= 0 {
		cfg.RateLimitPerSecond = 1
	}
	if cfg.CashThresholdBps <= 0 {
		cfg.CashThresholdBps = 30000
	}
	if cfg.CashTargetBps <= cfg.CashThresholdBps {
		cfg.CashTargetBps = cfg.CashThresholdBps * 2
	}
	if cfg.PosThresholdUnits <= 0 {
		cfg.PosThresholdUnits = 4
	}
	if cfg.PosTargetUnits <= cfg.PosThresholdUnits {
		cfg.PosTargetUnits = cfg.PosThresholdUnits * 4
	}
	if cfg.InitialCash <= 0 {
		cfg.InitialCash = 100000000
	}
	if cfg.InitialPosition <= 0 {
		cfg.InitialPosition = 20000
	}
	if cfg.RateLimitRetryMax < 0 {
		cfg.RateLimitRetryMax = 0
	}
	if cfg.RateLimitBackoffMs <= 0 {
		cfg.RateLimitBackoffMs = 150
	}
	if cfg.RequestStaggerMs < 0 {
		cfg.RequestStaggerMs = 0
	}
	if cfg.SeedCashAmount <= 0 {
		cfg.SeedCashAmount = 500000000
	}
	if cfg.SeedPositionAmount <= 0 {
		cfg.SeedPositionAmount = 200000
	}
	if cfg.SeedRetryMax < 0 {
		cfg.SeedRetryMax = 0
	}
	if cfg.SeedRetryBackoffMs <= 0 {
		cfg.SeedRetryBackoffMs = 200
	}
	if cfg.SeedRequestDelayMs < 0 {
		cfg.SeedRequestDelayMs = 0
	}

	return cfg
}

func splitCSV(value string) []string {
	parts := strings.Split(value, ",")
	out := make([]string, 0, len(parts))
	for _, part := range parts {
		trimmed := strings.TrimSpace(part)
		if trimmed != "" {
			out = append(out, trimmed)
		}
	}
	return out
}

func runBenchmark(client *http.Client, cfg config) ([]requestResult, *benchmarkCounters) {
	results := make([]requestResult, 0, cfg.PairCount*2)
	counters := &benchmarkCounters{}
	state := newBenchmarkState(cfg)
	for offset := 0; offset < cfg.PairCount; offset += cfg.PairConcurrency {
		wavePairs := minInt(cfg.PairConcurrency, cfg.PairCount-offset)
		waveRequestStaggerMs := effectiveWaveRequestStaggerMs(cfg, wavePairs)
		waveStart := time.Now()
		if cfg.DebugProgress {
			fmt.Fprintf(os.Stderr, "wave_start offset=%d pairs=%d stagger_ms=%d\n", offset, wavePairs, waveRequestStaggerMs)
		}
		sellSpecs := make([]requestSpec, 0, wavePairs)
		buySpecs := make([]requestSpec, 0, wavePairs)

		for i := 0; i < wavePairs; i++ {
			pairIndex := offset + i
			price := cfg.BasePrice + int64(pairIndex%25)
			seller := cfg.Sellers[pairIndex%len(cfg.Sellers)]
			buyer := cfg.Buyers[pairIndex%len(cfg.Buyers)]
			sellSpecs = append(sellSpecs, newRequestSpec(cfg, seller, "sell", price, pairIndex, "maker"))
			buySpecs = append(buySpecs, newRequestSpec(cfg, buyer, "buy", price, pairIndex, "taker"))
		}

		results = append(results, runWave(client, cfg, state, counters, sellSpecs, waveRequestStaggerMs)...)
		results = append(results, runWave(client, cfg, state, counters, buySpecs, waveRequestStaggerMs)...)
		if cfg.DebugProgress {
			fmt.Fprintf(os.Stderr, "wave_done offset=%d total_results=%d\n", offset, len(results))
		}

		requestsThisWave := 2 * wavePairs
		waveBudgetMs := ceilDiv(1000*requestsThisWave, cfg.RateLimitPerSecond)
		elapsedMs := int(time.Since(waveStart).Milliseconds())
		sleepMs := waveBudgetMs - elapsedMs
		if offset+wavePairs < cfg.PairCount && sleepMs > 0 {
			time.Sleep(time.Duration(sleepMs) * time.Millisecond)
		}
	}
	return results, counters
}

func seedUsers(client *http.Client, cfg config) error {
	for _, userID := range cfg.Buyers {
		if err := topUpCash(client, cfg, userID, cfg.SeedCashAmount); err != nil {
			return fmt.Errorf("seed buyer cash for %s: %w", userID, err)
		}
		seedRequestPause(cfg)
	}
	for _, userID := range cfg.Sellers {
		if err := topUpCash(client, cfg, userID, cfg.SeedCashAmount); err != nil {
			return fmt.Errorf("seed seller cash for %s: %w", userID, err)
		}
		seedRequestPause(cfg)
		if err := topUpPosition(client, cfg, userID, cfg.Market, 0, cfg.SeedPositionAmount); err != nil {
			return fmt.Errorf("seed seller position for %s: %w", userID, err)
		}
		seedRequestPause(cfg)
	}
	return nil
}

func seedRequestPause(cfg config) {
	if cfg.SeedRequestDelayMs <= 0 {
		return
	}
	time.Sleep(time.Duration(cfg.SeedRequestDelayMs) * time.Millisecond)
}

type requestSpec struct {
	Subject string
	Body    orderBody
}

func newRequestSpec(cfg config, subject string, side string, price int64, pairIndex int, phase string) requestSpec {
	return requestSpec{
		Subject: subject,
		Body: orderBody{
			MarketID:      cfg.Market,
			Side:          side,
			OrderType:     "limit",
			Price:         price,
			Amount:        cfg.Amount,
			Outcome:       0,
			TimeInForce:   "gtc",
			ClientOrderID: fmt.Sprintf("%s-%s-%d-%s", cfg.Prefix, phase, pairIndex, uuid.NewString()[:8]),
		},
	}
}

func effectiveWaveRequestStaggerMs(cfg config, requestsInWave int) int {
	if requestsInWave <= 1 {
		return 0
	}
	if cfg.RateLimitPerSecond <= 0 {
		return cfg.RequestStaggerMs
	}
	perWaveBudgetMs := ceilDiv(1000*requestsInWave, cfg.RateLimitPerSecond)
	targetStaggerMs := ceilDiv(perWaveBudgetMs, requestsInWave-1)
	if targetStaggerMs < cfg.RequestStaggerMs {
		return cfg.RequestStaggerMs
	}
	return targetStaggerMs
}

func runWave(client *http.Client, cfg config, state *benchmarkState, counters *benchmarkCounters, specs []requestSpec, requestStaggerMs int) []requestResult {
	results := make([]requestResult, len(specs))
	var wg sync.WaitGroup
	for i := range specs {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			if requestStaggerMs > 0 && idx > 0 {
				time.Sleep(time.Duration(idx*requestStaggerMs) * time.Millisecond)
			}
			results[idx] = submitOrder(client, cfg, state, counters, specs[idx])
		}(i)
	}
	wg.Wait()
	return results
}

func submitOrder(client *http.Client, cfg config, state *benchmarkState, counters *benchmarkCounters, spec requestSpec) requestResult {
	requestStart := time.Now()
	firstPreSubmit := 0.0
	usedBusinessRescue := false
	usedFlowControlRetry := false
	businessRetryCount := 0
	flowRetryCount := 0
	prepareOrderUs := 0.0
	encodeRequestUs := 0.0
	buildAndSignUs := 0.0
	httpRoundTripUs := 0.0
	responseReadUs := 0.0
	responseParseUs := 0.0
	retryBackoffUs := 0.0
	recoveryActionUs := 0.0
	for {
		prepareStart := time.Now()
		prepared, err := prepareOrderBody(client, cfg, state, counters, spec)
		prepareOrderUs += elapsedUs(prepareStart)
		if businessRetryCount == 0 && flowRetryCount == 0 {
			firstPreSubmit = prepared.PreSubmitAvailable
		}
		if err != nil {
			return requestResult{
				StatusCode:         0,
				LatencyUs:          elapsedUs(requestStart),
				PrepareOrderUs:     prepareOrderUs,
				EncodeRequestUs:    encodeRequestUs,
				BuildAndSignUs:     buildAndSignUs,
				HttpRoundTripUs:    httpRoundTripUs,
				ResponseReadUs:     responseReadUs,
				ResponseParseUs:    responseParseUs,
				RetryBackoffUs:     retryBackoffUs,
				RecoveryActionUs:   recoveryActionUs,
				PreSubmitAvailable: firstPreSubmit,
			}
		}

		encodeStart := time.Now()
		bodyBytes, err := json.Marshal(prepared.Body)
		encodeRequestUs += elapsedUs(encodeStart)
		if err != nil {
			return requestResult{
				StatusCode:         0,
				LatencyUs:          elapsedUs(requestStart),
				PrepareOrderUs:     prepareOrderUs,
				EncodeRequestUs:    encodeRequestUs,
				BuildAndSignUs:     buildAndSignUs,
				HttpRoundTripUs:    httpRoundTripUs,
				ResponseReadUs:     responseReadUs,
				ResponseParseUs:    responseParseUs,
				RetryBackoffUs:     retryBackoffUs,
				RecoveryActionUs:   recoveryActionUs,
				PreSubmitAvailable: firstPreSubmit,
			}
		}

		buildStart := time.Now()
		req, err := http.NewRequest(http.MethodPost, strings.TrimRight(cfg.BaseURL, "/")+"/submit-order", bytes.NewReader(bodyBytes))
		if err != nil {
			return requestResult{
				StatusCode:         0,
				LatencyUs:          elapsedUs(requestStart),
				PrepareOrderUs:     prepareOrderUs,
				EncodeRequestUs:    encodeRequestUs,
				BuildAndSignUs:     buildAndSignUs,
				HttpRoundTripUs:    httpRoundTripUs,
				ResponseReadUs:     responseReadUs,
				ResponseParseUs:    responseParseUs,
				RetryBackoffUs:     retryBackoffUs,
				RecoveryActionUs:   recoveryActionUs,
				PreSubmitAvailable: firstPreSubmit,
			}
		}
		req.Header.Set("Accept", "application/json")
		req.Header.Set("Content-Type", "application/json")

		requestID := strings.ReplaceAll(uuid.NewString(), "-", "")
		for key, value := range tradeAuthHeaders(cfg, http.MethodPost, "/submit-order", spec.Subject, "user", bodyBytes, requestID) {
			req.Header.Set(key, value)
		}
		buildAndSignUs += elapsedUs(buildStart)

		transportStart := time.Now()
		resp, err := client.Do(req)
		httpRoundTripUs += elapsedUs(transportStart)
		if err != nil {
			state.releaseReservation(spec, prepared.ReservedAmount)
			return requestResult{
				StatusCode:         0,
				LatencyUs:          elapsedUs(requestStart),
				PrepareOrderUs:     prepareOrderUs,
				EncodeRequestUs:    encodeRequestUs,
				BuildAndSignUs:     buildAndSignUs,
				HttpRoundTripUs:    httpRoundTripUs,
				ResponseReadUs:     responseReadUs,
				ResponseParseUs:    responseParseUs,
				RetryBackoffUs:     retryBackoffUs,
				RecoveryActionUs:   recoveryActionUs,
				PreSubmitAvailable: firstPreSubmit,
			}
		}

		readStart := time.Now()
		bodyText, _ := io.ReadAll(resp.Body)
		resp.Body.Close()
		responseReadUs += elapsedUs(readStart)

		parseStart := time.Now()
		var parsed orderResponse
		_ = json.Unmarshal(bodyText, &parsed)
		var apiErr errorResponse
		_ = json.Unmarshal(bodyText, &apiErr)

		errorMessage := strings.TrimSpace(apiErr.Message)
		if errorMessage == "" {
			errorMessage = strings.TrimSpace(apiErr.Error)
		}
		errorCode := strings.TrimSpace(apiErr.Code)
		errorCategory := classifyError(resp.StatusCode, errorCode, errorMessage, apiErr.Details)
		triggerHint := deriveTriggerHint(errorCode, errorMessage, apiErr.Details)
		responseParseUs += elapsedUs(parseStart)

		if resp.StatusCode >= 200 && resp.StatusCode < 300 {
			successType := "direct success"
			if usedBusinessRescue {
				successType = "rescued success"
			} else if usedFlowControlRetry {
				successType = "flow-controlled success"
			}
			return requestResult{
				StatusCode:          resp.StatusCode,
				LatencyUs:           elapsedUs(requestStart),
				Fills:               parsed.Fills,
				RetryCount:          businessRetryCount + flowRetryCount,
				PrepareOrderUs:      prepareOrderUs,
				EncodeRequestUs:     encodeRequestUs,
				BuildAndSignUs:      buildAndSignUs,
				HttpRoundTripUs:     httpRoundTripUs,
				ResponseReadUs:      responseReadUs,
				ResponseParseUs:     responseParseUs,
				RetryBackoffUs:      retryBackoffUs,
				RecoveryActionUs:    recoveryActionUs,
				PreSubmitAvailable:  firstPreSubmit,
				SuccessType:         successType,
				QueueWaitUs:         parsed.QueueWaitUs,
				RiskUs:              parsed.GranularTiming.RiskUs,
				MatchingCoreUs:      parsed.GranularTiming.MatchingCoreUs,
				SettlementPersistUs: parsed.GranularTiming.SettlementPersistUs,
				PostMatchUs:         parsed.GranularTiming.PostMatchUs,
				ErrorCode:           errorCode,
				ErrorMessage:        errorMessage,
				ErrorCategory:       errorCategory,
				TriggerHint:         triggerHint,
			}
		}

		state.releaseReservation(spec, prepared.ReservedAmount)
		if resp.StatusCode == http.StatusTooManyRequests && shouldRetryRateLimit(cfg, errorCategory, triggerHint, flowRetryCount) {
			usedFlowControlRetry = true
			flowRetryCount++
			backoff := rateLimitBackoff(cfg, flowRetryCount, errorCategory, triggerHint)
			retryBackoffUs += float64(backoff.Microseconds())
			time.Sleep(backoff)
			continue
		}

		if !usedBusinessRescue && errorCategory == "risk reject" && strings.Contains(strings.ToUpper(triggerHint), "INSUFFICIENT_FUNDS") {
			recoveryStart := time.Now()
			if emergencyTopup(client, cfg, state, counters, spec, prepared.Body.Price, prepared.Body.Amount) == nil {
				recoveryActionUs += elapsedUs(recoveryStart)
				if counters != nil {
					counters.RetryCount.Add(1)
				}
				usedBusinessRescue = true
				businessRetryCount++
				continue
			}
			recoveryActionUs += elapsedUs(recoveryStart)
		}

		return requestResult{
			StatusCode:          resp.StatusCode,
			LatencyUs:           elapsedUs(requestStart),
			Fills:               parsed.Fills,
			RetryCount:          businessRetryCount + flowRetryCount,
			PrepareOrderUs:      prepareOrderUs,
			EncodeRequestUs:     encodeRequestUs,
			BuildAndSignUs:      buildAndSignUs,
			HttpRoundTripUs:     httpRoundTripUs,
			ResponseReadUs:      responseReadUs,
			ResponseParseUs:     responseParseUs,
			RetryBackoffUs:      retryBackoffUs,
			RecoveryActionUs:    recoveryActionUs,
			PreSubmitAvailable:  firstPreSubmit,
			QueueWaitUs:         parsed.QueueWaitUs,
			RiskUs:              parsed.GranularTiming.RiskUs,
			MatchingCoreUs:      parsed.GranularTiming.MatchingCoreUs,
			SettlementPersistUs: parsed.GranularTiming.SettlementPersistUs,
			PostMatchUs:         parsed.GranularTiming.PostMatchUs,
			ErrorCode:           errorCode,
			ErrorMessage:        errorMessage,
			ErrorCategory:       errorCategory,
			TriggerHint:         triggerHint,
		}
	}
}

func elapsedUs(start time.Time) float64 {
	return float64(time.Since(start).Microseconds())
}

func shouldRetryRateLimit(cfg config, errorCategory, triggerHint string, flowRetryCount int) bool {
	if flowRetryCount < 0 {
		return false
	}
	if errorCategory == "api rate limit" {
		// Cross the 1s fixed window once; repeated short retries only amplify IP limiter noise.
		return flowRetryCount < 1
	}
	return flowRetryCount < cfg.RateLimitRetryMax
}

func rateLimitBackoff(cfg config, retry int, errorCategory, triggerHint string) time.Duration {
	if retry <= 0 {
		retry = 1
	}
	if errorCategory == "api rate limit" {
		backoffMs := 1100 * retry
		if strings.Contains(strings.ToLower(triggerHint), "limiter=user_write") {
			backoffMs = 1050 * retry
		}
		if backoffMs > 2500 {
			backoffMs = 2500
		}
		return time.Duration(backoffMs) * time.Millisecond
	}
	backoffMs := cfg.RateLimitBackoffMs * retry
	if backoffMs > 1000 {
		backoffMs = 1000
	}
	return time.Duration(backoffMs) * time.Millisecond
}

func effectiveSeedBackoffDuration(attempt int, cfg config) time.Duration {
	backoffMs := cfg.SeedRetryBackoffMs * (attempt + 1)
	if backoffMs > 2000 {
		backoffMs = 2000
	}
	return time.Duration(backoffMs) * time.Millisecond
}

func resolveTemplateValue(template, subject string) string {
	if strings.Contains(template, "%s") {
		return fmt.Sprintf(template, subject)
	}
	return template
}

func tradeAuthHeaders(cfg config, method, path, subject, role string, body []byte, requestID string) map[string]string {
	if cfg.APIKeyTemplate != "" && cfg.APISecretTemplate != "" {
		apiKey := resolveTemplateValue(cfg.APIKeyTemplate, subject)
		apiSecret := resolveTemplateValue(cfg.APISecretTemplate, subject)
		return apiKeyHeaders(method, path, subject, role, apiKey, apiSecret, body, requestID)
	}
	return internalAuthHeaders(method, path, subject, role, cfg.Secret, body, requestID)
}

func internalAuthHeaders(method, path, subject, role, secret string, body []byte, requestID string) map[string]string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	sessionID := ""
	payload := fmt.Sprintf("%s\n%s\n\n%s\n%s\n%s\n%s\n%s",
		strings.ToUpper(method), path, subject, role, sessionID, timestamp, requestID,
	)
	return map[string]string{
		"x-request-id":                requestID,
		"x-internal-auth-subject":     subject,
		"x-internal-auth-role":        role,
		"x-internal-auth-session-id":  sessionID,
		"x-internal-auth-timestamp":   timestamp,
		"x-internal-auth-signature":   hmacHex(payload, secret),
		"x-internal-auth-body-sha256": sha256Hex(body),
	}
}

func apiKeyHeaders(method, path, subject, role, apiKey, apiSecret string, body []byte, requestID string) map[string]string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	bodyHash := sha256Hex(body)
	payload := fmt.Sprintf("%s\n%s\n\n%s\n%s\n%s\n%s\n%s\n%s",
		strings.ToUpper(method), path, apiKey, subject, role, timestamp, requestID, bodyHash,
	)
	return map[string]string{
		"x-request-id":      requestID,
		"x-api-key":         apiKey,
		"x-api-timestamp":   timestamp,
		"x-api-signature":   hmacHex(payload, apiSecret),
		"x-api-body-sha256": bodyHash,
	}
}

func prepareOrderBody(client *http.Client, cfg config, state *benchmarkState, counters *benchmarkCounters, spec requestSpec) (preparedOrder, error) {
	body := spec.Body
	if strings.EqualFold(body.Side, "buy") {
		requiredNotional := maxInt64(1, body.Price*body.Amount)
		availableCash := state.cashAvailable(spec.Subject)
		preSubmitAvailable := float64(availableCash)
		cashThreshold := scaleByBps(requiredNotional, cfg.CashThresholdBps)
		cashTarget := scaleByBps(requiredNotional, cfg.CashTargetBps)
		if availableCash < cashThreshold {
			topup := maxInt64(0, cashTarget-availableCash)
			if topup > 0 {
				if err := topUpCash(client, cfg, spec.Subject, topup); err != nil {
					return preparedOrder{Body: body, PreSubmitAvailable: preSubmitAvailable}, err
				}
				recordTopup(counters, topup)
				availableCash = state.addCash(spec.Subject, topup)
			}
		}
		maxAffordable := maxInt64(1, availableCash/maxInt64(1, body.Price))
		if body.Amount > maxAffordable {
			body.Amount = maxAffordable
		}
		reserved := body.Price * body.Amount
		state.reserveCash(spec.Subject, reserved)
		return preparedOrder{Body: body, PreSubmitAvailable: preSubmitAvailable, ReservedAmount: reserved}, nil
	}

	availablePos := state.positionAvailable(spec.Subject, body.MarketID, body.Outcome)
	preSubmitAvailable := float64(availablePos)
	if availablePos < cfg.PosThresholdUnits {
		topup := maxInt64(0, cfg.PosTargetUnits-availablePos)
		if topup > 0 {
			if err := topUpPosition(client, cfg, spec.Subject, body.MarketID, body.Outcome, topup); err != nil {
				return preparedOrder{Body: body, PreSubmitAvailable: preSubmitAvailable}, err
			}
			recordTopup(counters, topup)
			availablePos = state.addPosition(spec.Subject, body.MarketID, body.Outcome, topup)
		}
	}
	if body.Amount > maxInt64(1, availablePos) {
		body.Amount = maxInt64(1, availablePos)
	}
	state.reservePosition(spec.Subject, body.MarketID, body.Outcome, body.Amount)
	return preparedOrder{Body: body, PreSubmitAvailable: preSubmitAvailable, ReservedAmount: body.Amount}, nil
}

func recordTopup(counters *benchmarkCounters, amount int64) {
	if counters == nil || amount <= 0 {
		return
	}
	counters.TopupCount.Add(1)
	counters.TopupAmount.Add(amount)
}

func emergencyTopup(client *http.Client, cfg config, state *benchmarkState, counters *benchmarkCounters, spec requestSpec, price, amount int64) error {
	if strings.EqualFold(spec.Body.Side, "buy") {
		topup := maxInt64(scaleByBps(maxInt64(1, price*amount), cfg.CashTargetBps), cfg.InitialCash/4)
		if err := topUpCash(client, cfg, spec.Subject, topup); err != nil {
			return err
		}
		state.addCash(spec.Subject, topup)
		recordTopup(counters, topup)
		return nil
	}

	topup := maxInt64(cfg.PosTargetUnits, cfg.InitialPosition/2)
	if err := topUpPosition(client, cfg, spec.Subject, spec.Body.MarketID, spec.Body.Outcome, topup); err != nil {
		return err
	}
	state.addPosition(spec.Subject, spec.Body.MarketID, spec.Body.Outcome, topup)
	recordTopup(counters, topup)
	return nil
}

func topUpCash(client *http.Client, cfg config, userID string, amount int64) error {
	payload := map[string]interface{}{
		"user_id": userID,
		"amount":  amount,
		"op_id":   fmt.Sprintf("%s-topup-cash-%s-%s", cfg.Prefix, userID, uuid.NewString()[:8]),
	}
	body, statusCode, err := doAdminRequestWithRetry(client, cfg, http.MethodPost, "/deposit", payload)
	if err != nil {
		return fmt.Errorf("deposit topup transport failed: %w", err)
	}
	if statusCode != http.StatusOK {
		return fmt.Errorf("deposit topup failed with status %d body=%s", statusCode, compactResponseBody(body))
	}
	return nil
}

func topUpPosition(client *http.Client, cfg config, userID, marketID string, outcome int, amount int64) error {
	payload := map[string]interface{}{
		"user_id":   userID,
		"market_id": marketID,
		"outcome":   outcome,
		"amount":    amount,
		"op_id":     fmt.Sprintf("%s-topup-pos-%s-%s", cfg.Prefix, userID, uuid.NewString()[:8]),
	}
	body, statusCode, err := doAdminRequestWithRetry(client, cfg, http.MethodPost, "/position-deposit", payload)
	if err != nil {
		return fmt.Errorf("position topup transport failed: %w", err)
	}
	if statusCode != http.StatusOK {
		return fmt.Errorf("position topup failed with status %d body=%s", statusCode, compactResponseBody(body))
	}
	return nil
}

func doAdminRequestWithRetry(client *http.Client, cfg config, method, path string, payload interface{}) ([]byte, int, error) {
	var lastBody []byte
	var lastStatus int
	var lastErr error
	for attempt := 0; attempt <= cfg.SeedRetryMax; attempt++ {
		body, statusCode, err := doAuthedRequest(client, cfg, method, path, cfg.AdminSubject, "admin", payload)
		lastBody = body
		lastStatus = statusCode
		lastErr = err
		if err == nil && statusCode == http.StatusOK {
			return body, statusCode, nil
		}

		retryableStatus := statusCode == http.StatusTooManyRequests || statusCode >= http.StatusInternalServerError
		retryableError := err != nil
		if attempt >= cfg.SeedRetryMax || (!retryableStatus && !retryableError) {
			if err != nil {
				return body, statusCode, err
			}
			return body, statusCode, nil
		}
		time.Sleep(effectiveSeedBackoffDuration(attempt, cfg))
	}

	if lastErr != nil {
		return lastBody, lastStatus, lastErr
	}
	return lastBody, lastStatus, nil
}

func compactResponseBody(body []byte) string {
	text := strings.TrimSpace(string(body))
	if text == "" {
		return "<empty>"
	}
	if len(text) > 512 {
		return text[:512] + "...[truncated]"
	}
	return text
}

func doAuthedRequest(client *http.Client, cfg config, method, path, subject, role string, payload interface{}) ([]byte, int, error) {
	var bodyBytes []byte
	if payload != nil {
		encoded, err := json.Marshal(payload)
		if err != nil {
			return nil, 0, err
		}
		bodyBytes = encoded
	}

	req, err := http.NewRequest(method, strings.TrimRight(cfg.BaseURL, "/")+path, bytes.NewReader(bodyBytes))
	if err != nil {
		return nil, 0, err
	}
	req.Header.Set("Accept", "application/json")
	if method != http.MethodGet {
		req.Header.Set("Content-Type", "application/json")
	}
	requestID := strings.ReplaceAll(uuid.NewString(), "-", "")
	for key, value := range internalAuthHeaders(method, path, subject, role, cfg.Secret, bodyBytes, requestID) {
		req.Header.Set(key, value)
	}

	resp, err := client.Do(req)
	if err != nil {
		return nil, 0, err
	}
	defer resp.Body.Close()

	bodyText, readErr := io.ReadAll(resp.Body)
	if readErr != nil {
		return nil, resp.StatusCode, readErr
	}
	return bodyText, resp.StatusCode, nil
}

func newBenchmarkState(cfg config) *benchmarkState {
	return &benchmarkState{
		cash:      map[string]int64{},
		positions: map[string]int64{},
		cfg:       cfg,
	}
}

func (s *benchmarkState) positionKey(userID, marketID string, outcome int) string {
	return fmt.Sprintf("%s|%s|%d", userID, marketID, outcome)
}

func (s *benchmarkState) cashAvailable(userID string) int64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.cash[userID]; !ok {
		s.cash[userID] = s.cfg.InitialCash
	}
	return s.cash[userID]
}

func (s *benchmarkState) addCash(userID string, amount int64) int64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.cash[userID]; !ok {
		s.cash[userID] = s.cfg.InitialCash
	}
	s.cash[userID] += amount
	return s.cash[userID]
}

func (s *benchmarkState) reserveCash(userID string, amount int64) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.cash[userID]; !ok {
		s.cash[userID] = s.cfg.InitialCash
	}
	s.cash[userID] = maxInt64(0, s.cash[userID]-amount)
}

func (s *benchmarkState) positionAvailable(userID, marketID string, outcome int) int64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	key := s.positionKey(userID, marketID, outcome)
	if _, ok := s.positions[key]; !ok {
		s.positions[key] = s.cfg.InitialPosition
	}
	return s.positions[key]
}

func (s *benchmarkState) addPosition(userID, marketID string, outcome int, amount int64) int64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	key := s.positionKey(userID, marketID, outcome)
	if _, ok := s.positions[key]; !ok {
		s.positions[key] = s.cfg.InitialPosition
	}
	s.positions[key] += amount
	return s.positions[key]
}

func (s *benchmarkState) reservePosition(userID, marketID string, outcome int, amount int64) {
	s.mu.Lock()
	defer s.mu.Unlock()
	key := s.positionKey(userID, marketID, outcome)
	if _, ok := s.positions[key]; !ok {
		s.positions[key] = s.cfg.InitialPosition
	}
	s.positions[key] = maxInt64(0, s.positions[key]-amount)
}

func (s *benchmarkState) releaseReservation(spec requestSpec, amount int64) {
	if amount <= 0 {
		return
	}
	if strings.EqualFold(spec.Body.Side, "buy") {
		s.addCash(spec.Subject, amount)
		return
	}
	s.addPosition(spec.Subject, spec.Body.MarketID, spec.Body.Outcome, amount)
}

func sha256Hex(body []byte) string {
	sum := sha256.Sum256(body)
	return hex.EncodeToString(sum[:])
}

func hmacHex(message, secret string) string {
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(message))
	return hex.EncodeToString(mac.Sum(nil))
}

func classifyError(statusCode int, code, message string, details map[string]interface{}) string {
	lowerCode := strings.ToLower(strings.TrimSpace(code))
	lowerMessage := strings.ToLower(strings.TrimSpace(message))
	detailText := strings.ToLower(flattenDetails(details))
	combined := strings.Join([]string{lowerCode, lowerMessage, detailText}, " ")

	if statusCode == http.StatusTooManyRequests || strings.Contains(lowerCode, "rate_limited") {
		if strings.Contains(detailText, "limiter=") || strings.Contains(lowerMessage, "submit-order") {
			return "api rate limit"
		}
		if strings.Contains(detailText, "user_id=") && strings.Contains(detailText, "limit=") {
			return "engine rate limit"
		}
		return "business rule reject"
	}

	if statusCode == http.StatusUnauthorized || statusCode == http.StatusForbidden ||
		strings.Contains(combined, "auth") ||
		strings.Contains(combined, "signature") ||
		strings.Contains(combined, "timestamp") ||
		strings.Contains(combined, "x-request-id") ||
		strings.Contains(combined, "admin role required") {
		return "auth / signature"
	}

	if strings.Contains(combined, "duplicate") ||
		strings.Contains(combined, "replay") ||
		strings.Contains(combined, "already exists") {
		return "bad state / duplicate / replay"
	}

	if strings.Contains(combined, "insufficient") ||
		strings.Contains(combined, "margin") ||
		strings.Contains(combined, "position") ||
		strings.Contains(lowerCode, "insufficient_funds") ||
		strings.Contains(lowerCode, "account_frozen") {
		return "risk reject"
	}

	if strings.Contains(lowerCode, "invalid") ||
		strings.Contains(lowerCode, "tick_size") ||
		strings.Contains(lowerCode, "lot_size") ||
		strings.Contains(lowerCode, "below_min") ||
		strings.Contains(lowerCode, "fat_finger") ||
		strings.Contains(combined, "invalid") ||
		strings.Contains(combined, "unsupported") ||
		strings.Contains(combined, "too long") ||
		strings.Contains(combined, "json body") ||
		strings.Contains(combined, "orders array is empty") ||
		strings.Contains(combined, "leverage must") {
		return "validation"
	}

	if strings.Contains(lowerCode, "market_closed") ||
		strings.Contains(lowerCode, "killswitch") ||
		strings.Contains(lowerCode, "circuit") ||
		strings.Contains(lowerCode, "self_trade") ||
		strings.Contains(lowerCode, "insufficient_liquidity") ||
		strings.Contains(lowerCode, "price_band") ||
		strings.Contains(lowerCode, "instrument_delisted") {
		return "business rule reject"
	}

	if statusCode >= 400 && statusCode < 500 {
		return "business rule reject"
	}

	return "other"
}

func deriveTriggerHint(code, message string, details map[string]interface{}) string {
	if strings.EqualFold(strings.TrimSpace(code), "RATE_LIMITED") {
		if detail := strings.TrimSpace(flattenDetails(details)); detail != "" {
			return detail
		}
	}
	if strings.TrimSpace(code) != "" {
		return code
	}
	if detail := strings.TrimSpace(flattenDetails(details)); detail != "" {
		return detail
	}
	if strings.TrimSpace(message) != "" {
		return message
	}
	return "unknown"
}

func flattenDetails(details map[string]interface{}) string {
	if len(details) == 0 {
		return ""
	}
	parts := make([]string, 0, len(details))
	for key, value := range details {
		parts = append(parts, fmt.Sprintf("%s=%v", key, value))
	}
	sort.Strings(parts)
	return strings.Join(parts, ", ")
}

func buildSummary(results []requestResult, cfg config, counters *benchmarkCounters) benchmarkSummary {
	statusBreakdown := map[string]int{}
	fills := 0
	http4xxCount := 0
	http429Count := 0
	successResults := make([]requestResult, 0, len(results))
	errorResults := make([]requestResult, 0, len(results))
	for _, result := range results {
		statusBreakdown[strconv.Itoa(result.StatusCode)]++
		fills += result.Fills
		if result.StatusCode >= 400 && result.StatusCode < 500 {
			http4xxCount++
		}
		if result.StatusCode == http.StatusTooManyRequests {
			http429Count++
		}
		if result.StatusCode >= 200 && result.StatusCode < 300 {
			successResults = append(successResults, result)
		} else {
			errorResults = append(errorResults, result)
		}
	}

	clientMode := "keepalive_off"
	clientModeDesc := "DisableKeepAlives=true; conservative, stable, slightly pessimistic"
	if !cfg.DisableKeepAlives {
		clientMode = "keepalive_on"
		clientModeDesc = "DisableKeepAlives=false; closer to a production HTTP client"
	}

	return benchmarkSummary{
		ClientImpl:          "go",
		ClientMode:          clientMode,
		ClientModeDesc:      clientModeDesc,
		PrimaryMetricBasis:  "keepalive_on + direct success",
		Market:              cfg.Market,
		TotalRequests:       len(results),
		SuccessCount:        len(successResults),
		ErrorCount:          len(errorResults),
		SuccessRate:         percentage(len(successResults), len(results)),
		Http4xxCount:        http4xxCount,
		Http429Count:        http429Count,
		FillsReported:       fills,
		StatusBreakdown:     statusBreakdown,
		ClientLatencyUs:     summarize(extract(results, func(r requestResult) float64 { return r.LatencyUs })),
		PrepareOrderUs:      summarize(extract(results, func(r requestResult) float64 { return r.PrepareOrderUs })),
		EncodeRequestUs:     summarize(extract(results, func(r requestResult) float64 { return r.EncodeRequestUs })),
		BuildAndSignUs:      summarize(extract(results, func(r requestResult) float64 { return r.BuildAndSignUs })),
		HttpRoundTripUs:     summarize(extract(results, func(r requestResult) float64 { return r.HttpRoundTripUs })),
		ResponseReadUs:      summarize(extract(results, func(r requestResult) float64 { return r.ResponseReadUs })),
		ResponseParseUs:     summarize(extract(results, func(r requestResult) float64 { return r.ResponseParseUs })),
		RetryBackoffUs:      summarize(extract(results, func(r requestResult) float64 { return r.RetryBackoffUs })),
		RecoveryActionUs:    summarize(extract(results, func(r requestResult) float64 { return r.RecoveryActionUs })),
		PreSubmitAvailable:  summarize(extract(results, func(r requestResult) float64 { return r.PreSubmitAvailable })),
		TopupCount:          loadCounter(counters, true),
		TopupAmount:         loadCounter(counters, false),
		RetryCount:          loadRetryCounter(counters),
		QueueWaitUs:         summarize(extract(results, func(r requestResult) float64 { return r.QueueWaitUs })),
		RiskUs:              summarize(extract(results, func(r requestResult) float64 { return r.RiskUs })),
		MatchingCoreUs:      summarize(extract(results, func(r requestResult) float64 { return r.MatchingCoreUs })),
		SettlementPersistUs: summarize(extract(results, func(r requestResult) float64 { return r.SettlementPersistUs })),
		PostMatchUs:         summarize(extract(results, func(r requestResult) float64 { return r.PostMatchUs })),
		PerMarket: map[string]numericSummary{
			cfg.Market: summarize(extract(results, func(r requestResult) float64 { return r.LatencyUs })),
		},
		SuccessPath:        buildPathSummary(successResults, cfg.Market),
		ErrorPath:          buildPathSummary(errorResults, cfg.Market),
		DirectSuccessPath:  buildPathSummary(filterBySuccessType(successResults, "direct success"), cfg.Market),
		RescuedSuccessPath: buildPathSummary(filterBySuccessType(successResults, "rescued success"), cfg.Market),
		FlowControlPath:    buildPathSummary(filterBySuccessType(successResults, "flow-controlled success"), cfg.Market),
		SuccessCategories:  buildSuccessCategorySummaries(successResults),
		ErrorCategories:    buildErrorCategorySummaries(errorResults),
	}
}

func buildSuccessCategorySummaries(results []requestResult) []successCategorySummary {
	if len(results) == 0 {
		return nil
	}

	order := []string{"direct success", "rescued success", "flow-controlled success"}
	grouped := map[string][]requestResult{}
	for _, result := range results {
		key := result.SuccessType
		if strings.TrimSpace(key) == "" {
			key = "direct success"
		}
		grouped[key] = append(grouped[key], result)
	}

	summaries := make([]successCategorySummary, 0, len(order))
	for _, key := range order {
		items := grouped[key]
		if len(items) == 0 {
			continue
		}
		summaries = append(summaries, successCategorySummary{
			SuccessType:     key,
			Count:           len(items),
			ClientLatencyUs: summarize(extract(items, func(r requestResult) float64 { return r.LatencyUs })),
		})
	}
	return summaries
}

func buildErrorCategorySummaries(results []requestResult) []errorCategorySummary {
	if len(results) == 0 {
		return nil
	}

	grouped := make(map[string][]requestResult)
	triggerCounts := make(map[string]map[string]int)
	for _, result := range results {
		category := result.ErrorCategory
		if strings.TrimSpace(category) == "" {
			category = "other"
		}
		grouped[category] = append(grouped[category], result)
		if _, ok := triggerCounts[category]; !ok {
			triggerCounts[category] = map[string]int{}
		}
		trigger := result.TriggerHint
		if strings.TrimSpace(trigger) == "" {
			trigger = "unknown"
		}
		triggerCounts[category][trigger]++
	}

	categories := make([]string, 0, len(grouped))
	for category := range grouped {
		categories = append(categories, category)
	}
	sort.Strings(categories)

	summaries := make([]errorCategorySummary, 0, len(categories))
	for _, category := range categories {
		items := grouped[category]
		values := extract(items, func(r requestResult) float64 { return r.LatencyUs })
		summaries = append(summaries, errorCategorySummary{
			Category:        category,
			Count:           len(items),
			SharePct:        percentage(len(items), len(results)),
			ClientLatencyUs: summarize(values),
			TriggerHint:     mostCommonTrigger(triggerCounts[category]),
		})
	}
	return summaries
}

func mostCommonTrigger(counts map[string]int) string {
	bestTrigger := "unknown"
	bestCount := -1
	for trigger, count := range counts {
		if count > bestCount || (count == bestCount && trigger < bestTrigger) {
			bestTrigger = trigger
			bestCount = count
		}
	}
	return bestTrigger
}

func loadCounter(counters *benchmarkCounters, count bool) int64 {
	if counters == nil {
		return 0
	}
	if count {
		return counters.TopupCount.Load()
	}
	return counters.TopupAmount.Load()
}

func loadRetryCounter(counters *benchmarkCounters) int64 {
	if counters == nil {
		return 0
	}
	return counters.RetryCount.Load()
}

func buildPathSummary(results []requestResult, market string) pathSummary {
	statusBreakdown := map[string]int{}
	successCount := 0
	errorCount := 0
	for _, result := range results {
		statusBreakdown[strconv.Itoa(result.StatusCode)]++
		if result.StatusCode >= 200 && result.StatusCode < 300 {
			successCount++
		} else {
			errorCount++
		}
	}

	summary := pathSummary{
		TotalRequests:       len(results),
		SuccessCount:        successCount,
		ErrorCount:          errorCount,
		StatusBreakdown:     statusBreakdown,
		ClientLatencyUs:     summarize(extract(results, func(r requestResult) float64 { return r.LatencyUs })),
		PrepareOrderUs:      summarize(extract(results, func(r requestResult) float64 { return r.PrepareOrderUs })),
		EncodeRequestUs:     summarize(extract(results, func(r requestResult) float64 { return r.EncodeRequestUs })),
		BuildAndSignUs:      summarize(extract(results, func(r requestResult) float64 { return r.BuildAndSignUs })),
		HttpRoundTripUs:     summarize(extract(results, func(r requestResult) float64 { return r.HttpRoundTripUs })),
		ResponseReadUs:      summarize(extract(results, func(r requestResult) float64 { return r.ResponseReadUs })),
		ResponseParseUs:     summarize(extract(results, func(r requestResult) float64 { return r.ResponseParseUs })),
		RetryBackoffUs:      summarize(extract(results, func(r requestResult) float64 { return r.RetryBackoffUs })),
		RecoveryActionUs:    summarize(extract(results, func(r requestResult) float64 { return r.RecoveryActionUs })),
		PreSubmitAvailable:  summarize(extract(results, func(r requestResult) float64 { return r.PreSubmitAvailable })),
		QueueWaitUs:         summarize(extract(results, func(r requestResult) float64 { return r.QueueWaitUs })),
		RiskUs:              summarize(extract(results, func(r requestResult) float64 { return r.RiskUs })),
		MatchingCoreUs:      summarize(extract(results, func(r requestResult) float64 { return r.MatchingCoreUs })),
		SettlementPersistUs: summarize(extract(results, func(r requestResult) float64 { return r.SettlementPersistUs })),
		PostMatchUs:         summarize(extract(results, func(r requestResult) float64 { return r.PostMatchUs })),
	}
	if len(results) > 0 {
		summary.PerMarket = map[string]numericSummary{
			market: summarize(extract(results, func(r requestResult) float64 { return r.LatencyUs })),
		}
	}
	return summary
}

func filterBySuccessType(results []requestResult, successType string) []requestResult {
	filtered := make([]requestResult, 0, len(results))
	for _, result := range results {
		if strings.EqualFold(strings.TrimSpace(result.SuccessType), strings.TrimSpace(successType)) {
			filtered = append(filtered, result)
		}
	}
	return filtered
}

func extract(results []requestResult, getter func(requestResult) float64) []float64 {
	values := make([]float64, 0, len(results))
	for _, result := range results {
		values = append(values, getter(result))
	}
	return values
}

func summarize(values []float64) numericSummary {
	if len(values) == 0 {
		return numericSummary{}
	}
	cp := append([]float64(nil), values...)
	sort.Float64s(cp)

	sum := 0.0
	for _, value := range cp {
		sum += value
	}
	return numericSummary{
		Count: len(cp),
		Min:   round2(cp[0]),
		P50:   round2(percentile(cp, 0.50)),
		P95:   round2(percentile(cp, 0.95)),
		P99:   round2(percentile(cp, 0.99)),
		P999:  round2(percentile(cp, 0.999)),
		Max:   round2(cp[len(cp)-1]),
		Avg:   round2(sum / float64(len(cp))),
	}
}

func percentile(sorted []float64, q float64) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := int(float64(len(sorted)-1) * q)
	if idx < 0 {
		idx = 0
	}
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

func round2(value float64) float64 {
	return float64(int(value*100+0.5)) / 100
}

func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func ceilDiv(numerator, denominator int) int {
	if numerator <= 0 || denominator <= 0 {
		return 0
	}
	return (numerator + denominator - 1) / denominator
}

func percentage(numerator, denominator int) float64 {
	if denominator <= 0 {
		return 0
	}
	return round2((float64(numerator) * 100.0) / float64(denominator))
}

func maxInt64(a, b int64) int64 {
	if a > b {
		return a
	}
	return b
}

func scaleByBps(value, bps int64) int64 {
	scaled := (value * bps) / 10000
	return maxInt64(1, scaled)
}
