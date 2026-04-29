package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"math/rand"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
)

func init() {
	log.SetOutput(io.Discard)
}

func pctFloat(values []float64, q float64) float64 {
	if len(values) == 0 {
		return 0
	}
	cp := append([]float64(nil), values...)
	sort.Float64s(cp)
	idx := int(float64(len(cp)-1) * q)
	if idx < 0 {
		idx = 0
	}
	if idx >= len(cp) {
		idx = len(cp) - 1
	}
	return cp[idx]
}

func newQuietEngine(window time.Duration) (*MatchingEngine, *eventbus.EventBus) {
	bus := eventbus.NewEventBus()
	engine := NewMatchingEngine(window, bus)
	return engine, bus
}

func submitIntent(engine *MatchingEngine, intent *types.Intent, createdAt map[string]time.Time, mu *sync.Mutex) {
	ts := time.Now()
	mu.Lock()
	createdAt[intent.ID] = ts
	mu.Unlock()
	engine.AddIntent(intent)
}

type stressResult struct {
	Name         string  `json:"name"`
	Orders       int     `json:"orders"`
	Fills        int     `json:"fills"`
	Cancels      int     `json:"cancels,omitempty"`
	DurationMs   float64 `json:"duration_ms"`
	OrdersPerSec float64 `json:"orders_per_sec"`
	FillsPerSec  float64 `json:"fills_per_sec"`
	P50LatencyMs float64 `json:"p50_latency_ms"`
	P95LatencyMs float64 `json:"p95_latency_ms"`
	P99LatencyMs float64 `json:"p99_latency_ms"`
	MaxLatencyMs float64 `json:"max_latency_ms"`
	MinLatencyMs float64 `json:"min_latency_ms"`
	Notes        string  `json:"notes,omitempty"`
}

// cancelLifecycle tracks per-cancel timing breakdown
type cancelLifecycle struct {
	IntentID       string
	SubmittedAt    time.Time // When cancel was requested
	BatchStartedAt time.Time // When batch processing began
	CompletedAt    time.Time // When cancel was finalized
	WasFilled      bool      // True if intent was filled before cancel
	BatchDelayed   bool      // True if cancel waited for next batch window
}

// cancelMetrics aggregates cancel lifecycle statistics
type cancelMetrics struct {
	TotalCancels        int
	CancelledBeforeFill int
	CancelledAfterFill  int
	BatchDelayed        int
	QueueWaitMs         []float64 // SubmittedAt -> BatchStartedAt
	MatchExecMs         []float64 // BatchStartedAt -> CompletedAt
	TotalLatencyMs      []float64 // SubmittedAt -> CompletedAt
}

// recoveryCohort separates historical vs new order latencies
type recoveryCohort struct {
	Name        string
	OrderCount  int
	FillCount   int
	LatenciesMs []float64
}

// enhancedResult extends stressResult with detailed analytics
type enhancedResult struct {
	stressResult
	CancelMetrics   *cancelMetrics     `json:"cancel_metrics,omitempty"`
	RecoveryCohorts []recoveryCohort `json:"recovery_cohorts,omitempty"`
}

func (r stressResult) mdRow() string {
	return fmt.Sprintf("| %s | %d | %d | %d | %.1f | %.1f | %.2f | %.2f | %.2f | %.2f | %.2f | %.2f |\n",
		r.Name, r.Orders, r.Fills, r.Cancels, r.DurationMs, r.OrdersPerSec, r.FillsPerSec,
		r.P50LatencyMs, r.P95LatencyMs, r.P99LatencyMs, r.MaxLatencyMs, r.MinLatencyMs)
}

func collectFills(fillCh <-chan types.Event, expected int, createdAt map[string]time.Time, mu *sync.Mutex, timeout time.Duration) ([]float64, error) {
	return drainFills(fillCh, createdAt, mu, timeout, 500*time.Millisecond)
}

func drainFills(fillCh <-chan types.Event, createdAt map[string]time.Time, mu *sync.Mutex, totalTimeout time.Duration, idleTimeout time.Duration) ([]float64, error) {
	latencies := make([]float64, 0, 1024)
	deadline := time.After(totalTimeout)
	idleTimer := time.NewTimer(idleTimeout)
	defer idleTimer.Stop()

	for {
		select {
		case evt := <-fillCh:
			fill, ok := evt.Payload.(types.Fill)
			if !ok {
				continue
			}
			mu.Lock()
			ct, exists := createdAt[fill.IntentID]
			mu.Unlock()
			if exists {
				latencies = append(latencies, time.Since(ct).Seconds()*1000)
			}
			if !idleTimer.Stop() {
				select {
				case <-idleTimer.C:
				default:
				}
			}
			idleTimer.Reset(idleTimeout)
		case <-idleTimer.C:
			return latencies, nil
		case <-deadline:
			return latencies, fmt.Errorf("total timeout collecting fills: got %d", len(latencies))
		}
	}
}

func buildResult(name string, orders, fills, cancels int, start time.Time, latencies []float64, notes string) stressResult {
	elapsed := time.Since(start)
	elapsedSec := elapsed.Seconds()
	r := stressResult{
		Name:         name,
		Orders:       orders,
		Fills:        fills,
		Cancels:      cancels,
		DurationMs:   elapsedSec * 1000,
		OrdersPerSec: float64(orders) / elapsedSec,
		FillsPerSec:  float64(fills) / elapsedSec,
		Notes:        notes,
	}
	if len(latencies) > 0 {
		r.P50LatencyMs = pctFloat(latencies, 0.50)
		r.P95LatencyMs = pctFloat(latencies, 0.95)
		r.P99LatencyMs = pctFloat(latencies, 0.99)
		r.MaxLatencyMs = pctFloat(latencies, 1.0)
		r.MinLatencyMs = pctFloat(latencies, 0.0)
	}
	return r
}

func printResult(r stressResult) {
	fmt.Println(strings.Repeat("=", 80))
	fmt.Printf("  STRESS TEST: %s\n", r.Name)
	fmt.Println(strings.Repeat("-", 80))
	fmt.Printf("  Orders:        %d\n", r.Orders)
	fmt.Printf("  Fills:         %d\n", r.Fills)
	if r.Cancels > 0 {
		fmt.Printf("  Cancels:       %d\n", r.Cancels)
	}
	fmt.Printf("  Duration:      %.1f ms (%.1f s)\n", r.DurationMs, r.DurationMs/1000)
	fmt.Printf("  Throughput:    %.1f orders/s | %.1f fills/s\n", r.OrdersPerSec, r.FillsPerSec)
	fmt.Printf("  Latency p50:   %.2f ms\n", r.P50LatencyMs)
	fmt.Printf("  Latency p95:   %.2f ms\n", r.P95LatencyMs)
	fmt.Printf("  Latency p99:   %.2f ms\n", r.P99LatencyMs)
	fmt.Printf("  Latency min:   %.2f ms\n", r.MinLatencyMs)
	fmt.Printf("  Latency max:   %.2f ms\n", r.MaxLatencyMs)
	if r.Notes != "" {
		fmt.Printf("  Notes:         %s\n", r.Notes)
	}
	fmt.Println(strings.Repeat("=", 80))
}

func writeStressJSON(filename string, r stressResult) {
	dir := filepath.Join("..", "docs", "benchmarks")
	_ = os.MkdirAll(dir, 0o755)
	path := filepath.Join(dir, filename)

	content := fmt.Sprintf(`{
  "name": "%s",
  "orders": %d,
  "fills": %d,
  "cancels": %d,
  "duration_ms": %.2f,
  "orders_per_sec": %.2f,
  "fills_per_sec": %.2f,
  "latency": {
    "p50_ms": %.2f,
    "p95_ms": %.2f,
    "p99_ms": %.2f,
    "min_ms": %.2f,
    "max_ms": %.2f
  },
  "notes": "%s"
}
`, r.Name, r.Orders, r.Fills, r.Cancels, r.DurationMs, r.OrdersPerSec, r.FillsPerSec,
		r.P50LatencyMs, r.P95LatencyMs, r.P99LatencyMs, r.MinLatencyMs, r.MaxLatencyMs, r.Notes)

	_ = os.WriteFile(path, []byte(content), 0o644)
}

func printCancelMetrics(m cancelMetrics) {
	fmt.Println(strings.Repeat("-", 80))
	fmt.Println("  CANCEL LIFECYCLE BREAKDOWN:")
	fmt.Printf("  Total cancels tracked:    %d\n", m.TotalCancels)
	fmt.Printf("  Cancelled before fill:    %d (%.1f%%)\n", m.CancelledBeforeFill, pct(m.CancelledBeforeFill, m.TotalCancels))
	fmt.Printf("  Cancelled after fill:     %d (%.1f%%)\n", m.CancelledAfterFill, pct(m.CancelledAfterFill, m.TotalCancels))
	fmt.Printf("  Batch-delayed cancels:    %d (%.1f%%)\n", m.BatchDelayed, pct(m.BatchDelayed, m.TotalCancels))
	fmt.Println()
	fmt.Printf("  queue_wait (submit->batch):\n")
	fmt.Printf("    p50: %.2f ms | p95: %.2f ms | p99: %.2f ms | max: %.2f ms\n",
		pctFloat(m.QueueWaitMs, 0.50), pctFloat(m.QueueWaitMs, 0.95),
		pctFloat(m.QueueWaitMs, 0.99), pctFloat(m.QueueWaitMs, 1.0))
	fmt.Printf("  match_exec (batch->done):\n")
	fmt.Printf("    p50: %.2f ms | p95: %.2f ms | p99: %.2f ms | max: %.2f ms\n",
		pctFloat(m.MatchExecMs, 0.50), pctFloat(m.MatchExecMs, 0.95),
		pctFloat(m.MatchExecMs, 0.99), pctFloat(m.MatchExecMs, 1.0))
	fmt.Printf("  total_latency (submit->done):\n")
	fmt.Printf("    p50: %.2f ms | p95: %.2f ms | p99: %.2f ms | max: %.2f ms\n",
		pctFloat(m.TotalLatencyMs, 0.50), pctFloat(m.TotalLatencyMs, 0.95),
		pctFloat(m.TotalLatencyMs, 0.99), pctFloat(m.TotalLatencyMs, 1.0))
}

func pct(part, total int) float64 {
	if total == 0 {
		return 0
	}
	return float64(part) / float64(total) * 100
}

func writeEnhancedJSON(filename string, r enhancedResult) {
	dir := filepath.Join("..", "docs", "benchmarks")
	_ = os.MkdirAll(dir, 0o755)
	path := filepath.Join(dir, filename)

	var cmJSON string
	if r.CancelMetrics != nil {
		cm := r.CancelMetrics
		cmJSON = fmt.Sprintf(`,
  "cancel_metrics": {
    "total": %d,
    "before_fill": %d,
    "after_fill": %d,
    "batch_delayed": %d,
    "queue_wait": {"p50": %.2f, "p95": %.2f, "p99": %.2f, "max": %.2f},
    "match_exec": {"p50": %.2f, "p95": %.2f, "p99": %.2f, "max": %.2f},
    "total_latency": {"p50": %.2f, "p95": %.2f, "p99": %.2f, "max": %.2f}
  }`,
			cm.TotalCancels, cm.CancelledBeforeFill, cm.CancelledAfterFill, cm.BatchDelayed,
			pctFloat(cm.QueueWaitMs, 0.50), pctFloat(cm.QueueWaitMs, 0.95), pctFloat(cm.QueueWaitMs, 0.99), pctFloat(cm.QueueWaitMs, 1.0),
			pctFloat(cm.MatchExecMs, 0.50), pctFloat(cm.MatchExecMs, 0.95), pctFloat(cm.MatchExecMs, 0.99), pctFloat(cm.MatchExecMs, 1.0),
			pctFloat(cm.TotalLatencyMs, 0.50), pctFloat(cm.TotalLatencyMs, 0.95), pctFloat(cm.TotalLatencyMs, 0.99), pctFloat(cm.TotalLatencyMs, 1.0))
	}

	var cohortsJSON string
	if len(r.RecoveryCohorts) > 0 {
		var b strings.Builder
		b.WriteString(",\n  \"recovery_cohorts\": [\n")
		for i, c := range r.RecoveryCohorts {
			if i > 0 {
				b.WriteString(",\n")
			}
			b.WriteString(fmt.Sprintf("    {\"name\": \"%s\", \"orders\": %d, \"fills\": %d, \"latency\": {\"p50\": %.2f, \"p95\": %.2f, \"p99\": %.2f, \"max\": %.2f}}",
				c.Name, c.OrderCount, c.FillCount,
				pctFloat(c.LatenciesMs, 0.50), pctFloat(c.LatenciesMs, 0.95),
				pctFloat(c.LatenciesMs, 0.99), pctFloat(c.LatenciesMs, 1.0)))
		}
		b.WriteString("\n  ]")
		cohortsJSON = b.String()
	}

	content := fmt.Sprintf(`{
  "name": "%s",
  "orders": %d,
  "fills": %d,
  "cancels": %d,
  "duration_ms": %.2f,
  "orders_per_sec": %.2f,
  "fills_per_sec": %.2f,
  "latency": {
    "p50_ms": %.2f,
    "p95_ms": %.2f,
    "p99_ms": %.2f,
    "min_ms": %.2f,
    "max_ms": %.2f
  },
  "notes": "%s"%s%s
}`, r.Name, r.Orders, r.Fills, r.Cancels, r.DurationMs, r.OrdersPerSec, r.FillsPerSec,
		r.P50LatencyMs, r.P95LatencyMs, r.P99LatencyMs, r.MinLatencyMs, r.MaxLatencyMs, r.Notes, cmJSON, cohortsJSON)

	_ = os.WriteFile(path, []byte(content), 0o644)
}

// --- 1. Single-Hotspot Extreme Test ---
// All orders target one market, one outcome, extreme contention.
func TestStress_SingleHotspot(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		pairs     = 5000
		window    = 50 * time.Millisecond
		fillLimit = pairs*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, pairs*2)
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	start := time.Now()
	for i := 0; i < pairs; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("hs-buy-%d", i), UserID: fmt.Sprintf("u%d", i),
			MarketID: "hotspot", Side: "buy", Price: 60, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("hs-sell-%d", i), UserID: fmt.Sprintf("u%d", i+pairs),
			MarketID: "hotspot", Side: "sell", Price: 40, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}

	latencies, err := collectFills(fillCh, pairs*2, createdAt, &mu, 15*time.Second)
	if err != nil {
		t.Logf("warning: %v", err)
	}

	result := buildResult("Single-Hotspot (packed)", pairs*2, len(latencies), 0, start, latencies,
		fmt.Sprintf("5000 pairs on single market/outcome"))
	printResult(result)
	writeStressJSON("stress_single_hotspot.json", result)
}

// --- 2. Multi-Market Uniform Test ---
// Orders spread evenly across many markets.
func TestStress_MultiMarketUniform(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		totalPairs = 4000
		numMarkets = 20
		pairsPerMk = totalPairs / numMarkets
		window     = 50 * time.Millisecond
		fillLimit  = totalPairs*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, totalPairs*2)
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	start := time.Now()
	for m := 0; m < numMarkets; m++ {
		marketID := fmt.Sprintf("mkt-%d", m)
		for i := 0; i < pairsPerMk; i++ {
			submitIntent(engine, &types.Intent{
				ID: fmt.Sprintf("mm-buy-%d-%d", m, i), UserID: fmt.Sprintf("u%d", m*1000+i),
				MarketID: marketID, Side: "buy", Price: 60, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, createdAt, &mu)
			submitIntent(engine, &types.Intent{
				ID: fmt.Sprintf("mm-sell-%d-%d", m, i), UserID: fmt.Sprintf("u%d", m*1000+i+50000),
				MarketID: marketID, Side: "sell", Price: 40, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, createdAt, &mu)
		}
	}

	latencies, err := collectFills(fillCh, totalPairs*2, createdAt, &mu, 20*time.Second)
	if err != nil {
		t.Logf("warning: %v", err)
	}

	result := buildResult("Multi-Market Uniform", totalPairs*2, len(latencies), 0, start, latencies,
		fmt.Sprintf("%d markets x %d pairs each", numMarkets, pairsPerMk))
	printResult(result)
	writeStressJSON("stress_multi_market_uniform.json", result)
}

// --- 3. Hotspot Skew Test (packed vs spread) ---
// 80% orders on 2 markets (packed), 20% spread across 18 markets.
func TestStress_HotspotSkew(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		totalPairs = 4000
		window     = 50 * time.Millisecond
		fillLimit  = totalPairs*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, totalPairs*2)
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	hotPairs := int(float64(totalPairs) * 0.8)
	coldPairs := totalPairs - hotPairs
	coldMarkets := 18
	pairsPerCold := coldPairs / coldMarkets

	start := time.Now()

	// Hot markets (2 markets, 80% traffic)
	for m := 0; m < 2; m++ {
		perHot := hotPairs / 2
		marketID := fmt.Sprintf("hot-%d", m)
		for i := 0; i < perHot; i++ {
			submitIntent(engine, &types.Intent{
				ID: fmt.Sprintf("hot-buy-%d-%d", m, i), UserID: fmt.Sprintf("u%d", m*5000+i),
				MarketID: marketID, Side: "buy", Price: 60, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, createdAt, &mu)
			submitIntent(engine, &types.Intent{
				ID: fmt.Sprintf("hot-sell-%d-%d", m, i), UserID: fmt.Sprintf("u%d", m*5000+i+60000),
				MarketID: marketID, Side: "sell", Price: 40, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, createdAt, &mu)
		}
	}

	// Cold markets (18 markets, 20% traffic)
	for m := 0; m < coldMarkets; m++ {
		marketID := fmt.Sprintf("cold-%d", m)
		for i := 0; i < pairsPerCold; i++ {
			submitIntent(engine, &types.Intent{
				ID: fmt.Sprintf("cold-buy-%d-%d", m, i), UserID: fmt.Sprintf("u%d", m*2000+i+70000),
				MarketID: marketID, Side: "buy", Price: 60, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, createdAt, &mu)
			submitIntent(engine, &types.Intent{
				ID: fmt.Sprintf("cold-sell-%d-%d", m, i), UserID: fmt.Sprintf("u%d", m*2000+i+80000),
				MarketID: marketID, Side: "sell", Price: 40, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, createdAt, &mu)
		}
	}

	latencies, err := collectFills(fillCh, totalPairs*2, createdAt, &mu, 20*time.Second)
	if err != nil {
		t.Logf("warning: %v", err)
	}

	result := buildResult("Hotspot Skew (80/20)", totalPairs*2, len(latencies), 0, start, latencies,
		fmt.Sprintf("80%% on 2 markets, 20%% on %d markets", coldMarkets))
	printResult(result)
	writeStressJSON("stress_hotspot_skew.json", result)
}

// --- 4. High-Cancel Market Making Flow Test ---
// Heavy cancel-to-order ratio simulating market-making behavior.
func TestStress_HighCancelMarketMaking(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		basePairs   = 2000
		cancelRatio = 3
		window      = 50 * time.Millisecond
		fillLimit   = basePairs*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, basePairs*2*(cancelRatio+1))
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	start := time.Now()
	totalCancels := int64(0)

	for i := 0; i < basePairs; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("mm-buy-%d", i), UserID: fmt.Sprintf("mm%d", i%50),
			MarketID: "mm-market", Side: "buy", Price: 55 + int64(i%10), Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("mm-sell-%d", i), UserID: fmt.Sprintf("mm%d", i%50+100),
			MarketID: "mm-market", Side: "sell", Price: 45 - int64(i%10), Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}

	for c := 0; c < cancelRatio; c++ {
		time.Sleep(window)
		for i := 0; i < basePairs; i++ {
			cancelID := fmt.Sprintf("mm-buy-%d", i)
			if i%2 == c%2 {
				cancelID = fmt.Sprintf("mm-sell-%d", i)
			}
			_ = engine.CancelIntent(cancelID)
			atomic.AddInt64(&totalCancels, 1)

			newID := fmt.Sprintf("mm-replace-%d-%d", c, i)
			side := "buy"
			price := 55 + int64(rand.Intn(10))
			if i%2 == c%2 {
				side = "sell"
				price = 45 - int64(rand.Intn(10))
			}
			submitIntent(engine, &types.Intent{
				ID: newID, UserID: fmt.Sprintf("mm%d", i%50),
				MarketID: "mm-market", Side: side, Price: price, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, createdAt, &mu)
		}
	}

	time.Sleep(3 * window)
	engine.processBatch()

	expectedFills := basePairs * 2 * (cancelRatio + 1)
	latencies, _ := collectFills(fillCh, expectedFills, createdAt, &mu, 15*time.Second)

	result := buildResult("High-Cancel Market Making", basePairs*2*(cancelRatio+1), len(latencies), int(totalCancels), start, latencies,
		fmt.Sprintf("cancel:order ratio approx %d:1", cancelRatio))
	printResult(result)
	writeStressJSON("stress_high_cancel_mm.json", result)
}

// --- 4b. High-Cancel with Lifecycle Breakdown ---
// Same workload as TestStress_HighCancelMarketMaking but tracks cancel queue_wait, match_exec, batch_delay.
func TestStress_HighCancelLifecycleBreakdown(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		basePairs   = 2000
		cancelRatio = 3
		window      = 50 * time.Millisecond
		fillLimit   = basePairs*4 + 65536
	)

	bus := eventbus.NewEventBus()
	tracker := NewCancelTracker()
	engine := NewMatchingEngineWithTracker(window, bus, tracker)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, basePairs*2*(cancelRatio+1))
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	start := time.Now()
	totalCancels := int64(0)
	successfulCancels := int64(0)

	for i := 0; i < basePairs; i++ {
		// Non-crossing orders: buy at 45, sell at 55 — no immediate fills
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("mm-buy-%d", i), UserID: fmt.Sprintf("mm%d", i%50),
			MarketID: "mm-cancel-test", Side: "buy", Price: 45, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("mm-sell-%d", i), UserID: fmt.Sprintf("mm%d", i%50+100),
			MarketID: "mm-cancel-test", Side: "sell", Price: 55, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}

	for c := 0; c < cancelRatio; c++ {
		time.Sleep(window)
		for i := 0; i < basePairs; i++ {
			cancelID := fmt.Sprintf("mm-buy-%d", i)
			if i%2 == c%2 {
				cancelID = fmt.Sprintf("mm-sell-%d", i)
			}
			if err := engine.CancelIntent(cancelID); err == nil {
				atomic.AddInt64(&successfulCancels, 1)
			}
			atomic.AddInt64(&totalCancels, 1)

			newID := fmt.Sprintf("mm-replace-%d-%d", c, i)
			side := "buy"
			price := 45 + int64(rand.Intn(5))
			if i%2 == c%2 {
				side = "sell"
				price = 55 - int64(rand.Intn(5))
			}
			submitIntent(engine, &types.Intent{
				ID: newID, UserID: fmt.Sprintf("mm%d", i%50),
				MarketID: "mm-cancel-test", Side: side, Price: price, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, createdAt, &mu)
		}
	}

	time.Sleep(3 * window)
	engine.processBatch()

	expectedFills := basePairs * 2 * (cancelRatio + 1)
	latencies, _ := collectFills(fillCh, expectedFills, createdAt, &mu, 15*time.Second)

	cancelTotal, cancelBeforeFill, cancelAfterFill, cancelDelayed, queueWaitMs, matchExecMs, totalLatencyMs := tracker.ExtractMetrics()

	result := enhancedResult{
		stressResult: buildResult("High-Cancel Lifecycle Breakdown", basePairs*2*(cancelRatio+1), len(latencies), int(successfulCancels), start, latencies,
			fmt.Sprintf("cancel:order ratio %d:1 | %d attempted / %d successful cancels | %d tracked", cancelRatio, totalCancels, successfulCancels, cancelTotal)),
		CancelMetrics: &cancelMetrics{
			TotalCancels:        cancelTotal,
			CancelledBeforeFill: cancelBeforeFill,
			CancelledAfterFill:  cancelAfterFill,
			BatchDelayed:        cancelDelayed,
			QueueWaitMs:         queueWaitMs,
			MatchExecMs:         matchExecMs,
			TotalLatencyMs:      totalLatencyMs,
		},
	}

	fmt.Println(strings.Repeat("=", 80))
	fmt.Printf("  STRESS TEST: %s\n", result.Name)
	fmt.Println(strings.Repeat("-", 80))
	fmt.Printf("  Orders:        %d\n", result.Orders)
	fmt.Printf("  Fills:         %d\n", result.Fills)
	fmt.Printf("  Cancels:       %d\n", result.Cancels)
	fmt.Printf("  Duration:      %.1f ms\n", result.DurationMs)
	fmt.Printf("  Throughput:    %.1f orders/s | %.1f fills/s\n", result.OrdersPerSec, result.FillsPerSec)
	fmt.Printf("  Latency p50:   %.2f ms\n", result.P50LatencyMs)
	fmt.Printf("  Latency p95:   %.2f ms\n", result.P95LatencyMs)
	fmt.Printf("  Latency p99:   %.2f ms\n", result.P99LatencyMs)
	fmt.Printf("  Latency min:   %.2f ms\n", result.MinLatencyMs)
	fmt.Printf("  Latency max:   %.2f ms\n", result.MaxLatencyMs)

	printCancelMetrics(*result.CancelMetrics)

	fmt.Println(strings.Repeat("=", 80))
	writeEnhancedJSON("stress_high_cancel_lifecycle.json", result)
}

// --- 5. Burst Traffic Test ---
// Sudden spike: 10x normal rate injected within a single batch window.
func TestStress_BurstTraffic(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		burstPairs = 10000
		window     = 50 * time.Millisecond
		fillLimit  = burstPairs*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, (burstPairs+100)*2)
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	for i := 0; i < 100; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("warm-buy-%d", i), UserID: fmt.Sprintf("w%d", i),
			MarketID: "burst-mkt", Side: "buy", Price: 60, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("warm-sell-%d", i), UserID: fmt.Sprintf("w%d", i+10000),
			MarketID: "burst-mkt", Side: "sell", Price: 40, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}
	time.Sleep(window)

	start := time.Now()
	for i := 0; i < burstPairs; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("burst-buy-%d", i), UserID: fmt.Sprintf("b%d", i),
			MarketID: "burst-mkt", Side: "buy", Price: 60, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("burst-sell-%d", i), UserID: fmt.Sprintf("b%d", i+20000),
			MarketID: "burst-mkt", Side: "sell", Price: 40, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}

	latencies, err := collectFills(fillCh, (burstPairs+100)*2, createdAt, &mu, 20*time.Second)
	if err != nil {
		t.Logf("warning: %v", err)
	}

	result := buildResult("Burst Traffic", (burstPairs+100)*2, len(latencies), 0, start, latencies,
		fmt.Sprintf("%d pairs injected within single window", burstPairs))
	printResult(result)
	writeStressJSON("stress_burst_traffic.json", result)
}

// --- 6. Queue Saturation Test ---
// Feed orders faster than the engine can process, saturating the internal queue.
func TestStress_QueueSaturation(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		totalPairs = 20000
		window     = 100 * time.Millisecond
		fillLimit  = totalPairs*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, totalPairs*2)
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	start := time.Now()

	for i := 0; i < totalPairs; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("qs-buy-%d", i), UserID: fmt.Sprintf("q%d", i),
			MarketID: "saturate-mkt", Side: "buy", Price: 60, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("qs-sell-%d", i), UserID: fmt.Sprintf("q%d", i+30000),
			MarketID: "saturate-mkt", Side: "sell", Price: 40, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}

	submitElapsed := time.Since(start)
	t.Logf("Queue saturated: %d orders submitted in %.1fms", totalPairs*2, submitElapsed.Seconds()*1000)

	latencies, err := collectFills(fillCh, totalPairs*2, createdAt, &mu, 30*time.Second)
	if err != nil {
		t.Logf("warning: %v", err)
	}

	result := buildResult("Queue Saturation", totalPairs*2, len(latencies), 0, start, latencies,
		fmt.Sprintf("submit phase %.1fms, 100ms batch window", submitElapsed.Seconds()*1000))
	printResult(result)
	writeStressJSON("stress_queue_saturation.json", result)
}

// --- 7. Long Soak Test ---
// Sustained load over extended period to detect memory leaks / degradation.
func TestStress_LongSoak(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		pairsPerBatch = 200
		numBatches    = 20
		window        = 50 * time.Millisecond
		fillLimit     = pairsPerBatch*numBatches*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, pairsPerBatch*numBatches*2)
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	start := time.Now()
	totalOrders := 0
	totalFills := 0
	batchLatencies := make([][]float64, 0, numBatches)

	for b := 0; b < numBatches; b++ {
		batchStart := time.Now()
		batchCreatedAt := make(map[string]time.Time, pairsPerBatch*2)
		// Use unique market per batch to prevent cross-batch order accumulation
		marketID := fmt.Sprintf("soak-mkt-%d", b)

		for i := 0; i < pairsPerBatch; i++ {
			submitIntent(engine, &types.Intent{
				ID: fmt.Sprintf("soak-buy-%d-%d", b, i), UserID: fmt.Sprintf("s%d", i%100),
				MarketID: marketID, Side: "buy", Price: 60, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, batchCreatedAt, &mu)
			submitIntent(engine, &types.Intent{
				ID: fmt.Sprintf("soak-sell-%d-%d", b, i), UserID: fmt.Sprintf("s%d", i%100+200),
				MarketID: marketID, Side: "sell", Price: 40, Amount: 1, Outcome: 1,
				CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
			}, batchCreatedAt, &mu)
		}

		mu.Lock()
		for k, v := range batchCreatedAt {
			createdAt[k] = v
		}
		mu.Unlock()
		totalOrders += pairsPerBatch * 2

		batchLat, err := drainFills(fillCh, createdAt, &mu, 3*time.Second, 300*time.Millisecond)
		if err != nil {
			t.Logf("batch %d warning: %v", b, err)
		}
		totalFills += len(batchLat)
		batchLatencies = append(batchLatencies, batchLat)

		batchDur := time.Since(batchStart)
		if b%10 == 0 || b == numBatches-1 {
			var avgLat float64
			if len(batchLat) > 0 {
				avgLat = pctFloat(batchLat, 0.50)
			}
			t.Logf("Soak batch %d/%d: %d fills, p50=%.2fms, elapsed=%.1fms",
				b+1, numBatches, len(batchLat), avgLat, batchDur.Seconds()*1000)
		}
	}

	elapsed := time.Since(start)

	allLatencies := make([]float64, 0, totalFills)
	for _, bl := range batchLatencies {
		allLatencies = append(allLatencies, bl...)
	}

	var first5, last5 []float64
	for i := 0; i < 5 && i < len(batchLatencies); i++ {
		first5 = append(first5, batchLatencies[i]...)
	}
	for i := len(batchLatencies) - 5; i < len(batchLatencies); i++ {
		if i >= 0 {
			last5 = append(last5, batchLatencies[i]...)
		}
	}

	notes := fmt.Sprintf("%d batches x %d pairs over %.1fs", numBatches, pairsPerBatch, elapsed.Seconds())
	if len(first5) > 0 && len(last5) > 0 {
		firstP99 := pctFloat(first5, 0.99)
		lastP99 := pctFloat(last5, 0.99)
		degradation := (lastP99 - firstP99) / firstP99 * 100
		notes += fmt.Sprintf(" | p99 degradation: %.1f%% (%.2fms -> %.2fms)", degradation, firstP99, lastP99)
	}

	result := buildResult("Long Soak", totalOrders, totalFills, 0, start, allLatencies, notes)
	printResult(result)
	writeStressJSON("stress_long_soak.json", result)
}

// --- 8. Recovery Backpressure Test ---
// After saturation, verify the engine recovers and processes backlog correctly.
func TestStress_RecoveryBackpressure(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		saturationPairs = 8000
		recoveryPairs   = 2000
		window          = 100 * time.Millisecond
		fillLimit       = (saturationPairs + recoveryPairs)*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, (saturationPairs+recoveryPairs)*2)
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	t.Log("Phase 1: Saturating queue...")
	satStart := time.Now()
	for i := 0; i < saturationPairs; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bp-sat-buy-%d", i), UserID: fmt.Sprintf("bp%d", i),
			MarketID: "backpressure-mkt", Side: "buy", Price: 60, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bp-sat-sell-%d", i), UserID: fmt.Sprintf("bp%d", i+40000),
			MarketID: "backpressure-mkt", Side: "sell", Price: 40, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}
	satElapsed := time.Since(satStart)
	t.Logf("Saturation: %d orders in %.1fms", saturationPairs*2, satElapsed.Seconds()*1000)

	time.Sleep(2 * window)

	t.Log("Phase 2: Injecting recovery orders...")
	recStart := time.Now()
	for i := 0; i < recoveryPairs; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bp-rec-buy-%d", i), UserID: fmt.Sprintf("br%d", i),
			MarketID: "backpressure-mkt", Side: "buy", Price: 60, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bp-rec-sell-%d", i), UserID: fmt.Sprintf("br%d", i+50000),
			MarketID: "backpressure-mkt", Side: "sell", Price: 40, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}

	totalExpected := (saturationPairs + recoveryPairs) * 2
	latencies, err := collectFills(fillCh, totalExpected, createdAt, &mu, 30*time.Second)
	if err != nil {
		t.Logf("warning: %v", err)
	}

	recElapsed := time.Since(recStart)

	notes := fmt.Sprintf("saturation %.1fms + recovery %.1fms", satElapsed.Seconds()*1000, recElapsed.Seconds()*1000)
	result := buildResult("Recovery Backpressure", totalExpected, len(latencies), 0, satStart, latencies, notes)
	printResult(result)
	writeStressJSON("stress_recovery_backpressure.json", result)
}

// --- 8b. Recovery Cohort Separation Test ---
// Separates 'historical backlog' (bp-sat-*) from 'new recovery orders' (bp-rec-*)
// and reports distinct p50/p95/p99 for each cohort.
func TestStress_RecoveryCohortSeparation(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	const (
		saturationPairs = 8000
		recoveryPairs   = 2000
		window          = 100 * time.Millisecond
		fillLimit       = (saturationPairs + recoveryPairs)*4 + 65536
	)

	engine, bus := newQuietEngine(window)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, fillLimit)
	createdAt := make(map[string]time.Time, (saturationPairs+recoveryPairs)*2)
	var mu sync.Mutex

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	t.Log("Phase 1: Saturating queue (historical backlog)...")
	satStart := time.Now()
	for i := 0; i < saturationPairs; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bp-sat-buy-%d", i), UserID: fmt.Sprintf("bp%d", i),
			MarketID: "backpressure-mkt", Side: "buy", Price: 60, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bp-sat-sell-%d", i), UserID: fmt.Sprintf("bp%d", i+40000),
			MarketID: "backpressure-mkt", Side: "sell", Price: 40, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}
	satElapsed := time.Since(satStart)
	t.Logf("Saturation: %d orders in %.1fms", saturationPairs*2, satElapsed.Seconds()*1000)

	time.Sleep(2 * window)
	// P2: Enable backlog drain mode to clear historical orders aggressively
	engine.EnableBacklogMode()
	time.Sleep(window / 2) // Brief settling period

	t.Log("Phase 2: Injecting recovery orders (new cohort)...")
	recStart := time.Now()
	for i := 0; i < recoveryPairs; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bp-rec-buy-%d", i), UserID: fmt.Sprintf("br%d", i),
			MarketID: "backpressure-mkt", Side: "buy", Price: 60, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bp-rec-sell-%d", i), UserID: fmt.Sprintf("br%d", i+50000),
			MarketID: "backpressure-mkt", Side: "sell", Price: 40, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}

	// Collect ALL fills with cohort tagging
	type taggedFill struct {
		intentID  string
		latencyMs float64
		isHistorical bool
	}
	taggedFills := make([]taggedFill, 0, fillLimit)
	deadline := time.After(30 * time.Second)
	idleTimer := time.NewTimer(500 * time.Millisecond)
	defer idleTimer.Stop()

	for {
		select {
		case evt := <-fillCh:
			fill, ok := evt.Payload.(types.Fill)
			if !ok {
				continue
			}
			mu.Lock()
			ct, exists := createdAt[fill.IntentID]
			mu.Unlock()
			if exists {
				lat := time.Since(ct).Seconds() * 1000
				isHist := strings.HasPrefix(fill.IntentID, "bp-sat-")
				taggedFills = append(taggedFills, taggedFill{fill.IntentID, lat, isHist})
			}
			if !idleTimer.Stop() {
				select {
				case <-idleTimer.C:
				default:
				}
			}
			idleTimer.Reset(500 * time.Millisecond)
		case <-idleTimer.C:
			goto doneCollecting
		case <-deadline:
			t.Logf("timeout collecting recovery fills: got %d", len(taggedFills))
			goto doneCollecting
		}
	}
doneCollecting:

	recElapsed := time.Since(recStart)

	// Separate cohorts
	var histLatencies, recLatencies []float64
	for _, f := range taggedFills {
		if f.isHistorical {
			histLatencies = append(histLatencies, f.latencyMs)
		} else {
			recLatencies = append(recLatencies, f.latencyMs)
		}
	}

	allLatencies := make([]float64, len(taggedFills))
	for i, f := range taggedFills {
		allLatencies[i] = f.latencyMs
	}

	totalExpected := (saturationPairs + recoveryPairs) * 2
	notes := fmt.Sprintf("saturation %.1fms + recovery %.1fms | hist=%d fills, rec=%d fills",
		satElapsed.Seconds()*1000, recElapsed.Seconds()*1000, len(histLatencies), len(recLatencies))

	result := enhancedResult{
		stressResult: buildResult("Recovery Cohort Separation", totalExpected, len(taggedFills), 0, satStart, allLatencies, notes),
		RecoveryCohorts: []recoveryCohort{
			{
				Name:        "Historical Backlog (bp-sat-*)",
				OrderCount:  saturationPairs * 2,
				FillCount:   len(histLatencies),
				LatenciesMs: histLatencies,
			},
			{
				Name:        "New Recovery (bp-rec-*)",
				OrderCount:  recoveryPairs * 2,
				FillCount:   len(recLatencies),
				LatenciesMs: recLatencies,
			},
		},
	}

	fmt.Println(strings.Repeat("=", 80))
	fmt.Printf("  STRESS TEST: %s\n", result.Name)
	fmt.Println(strings.Repeat("-", 80))
	fmt.Printf("  Orders:        %d\n", result.Orders)
	fmt.Printf("  Fills:         %d\n", result.Fills)
	fmt.Printf("  Duration:      %.1f ms\n", result.DurationMs)
	fmt.Printf("  Throughput:    %.1f orders/s | %.1f fills/s\n", result.OrdersPerSec, result.FillsPerSec)
	fmt.Printf("  Latency p50:   %.2f ms\n", result.P50LatencyMs)
	fmt.Printf("  Latency p95:   %.2f ms\n", result.P95LatencyMs)
	fmt.Printf("  Latency p99:   %.2f ms\n", result.P99LatencyMs)
	fmt.Println()

	for _, c := range result.RecoveryCohorts {
		fmt.Printf("  COHORT: %s\n", c.Name)
		fmt.Printf("    Orders: %d | Fills: %d (%.1f%%)\n", c.OrderCount, c.FillCount, pct(c.FillCount, c.OrderCount))
		if len(c.LatenciesMs) > 0 {
			fmt.Printf("    p50: %.2f ms | p95: %.2f ms | p99: %.2f ms | max: %.2f ms\n",
				pctFloat(c.LatenciesMs, 0.50), pctFloat(c.LatenciesMs, 0.95),
				pctFloat(c.LatenciesMs, 0.99), pctFloat(c.LatenciesMs, 1.0))
		}
		fmt.Println()
	}

	fmt.Println(strings.Repeat("=", 80))
	writeEnhancedJSON("stress_recovery_cohort_separation.json", result)
	writeEnhancedJSON("stress_recovery_cohort_separation.json", result)
}

// --- 7. Backlog Drain Strategy Test (P2) ---
func TestStress_BacklogDrainStrategy(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	bus := eventbus.NewEventBus()
	engine := NewMatchingEngine(50*time.Millisecond, bus)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)
	time.Sleep(30 * time.Millisecond)

	const batchSize = 10000
	fillCh := bus.Subscribe(types.EventTypeFillCreated, batchSize*2)
	createdAt := make(map[string]time.Time, batchSize*2)
	var mu sync.Mutex
	
	// Phase 1: Create saturated backlog (crossing orders to force fills)
	start := time.Now()
	for i := 0; i < batchSize; i++ {
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bl-buy-%d", i), UserID: fmt.Sprintf("bl%d", i%50),
			MarketID: "bl-test", Side: "buy", Price: 55, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
		submitIntent(engine, &types.Intent{
			ID: fmt.Sprintf("bl-sell-%d", i), UserID: fmt.Sprintf("bl%d", i%50+100),
			MarketID: "bl-test", Side: "sell", Price: 45, Amount: 1, Outcome: 1,
			CreatedAt: time.Now(), ExpiresAt: time.Now().Add(60 * time.Second), Status: "pending",
		}, createdAt, &mu)
	}
	satElapsed := time.Since(start)
	t.Logf("Saturation: %d orders in %.1fms", batchSize*2, satElapsed.Seconds()*1000)
	
	// Phase 2: Enable backlog drain mode
	engine.EnableBacklogMode()
	recStart := time.Now()
	
	// Wait for backlog to drain using collectFills helper
	latencies, _ := collectFills(fillCh, batchSize*2, createdAt, &mu, 15*time.Second)
	recElapsed := time.Since(recStart)
	
	result := enhancedResult{
		stressResult: buildResult("Backlog Drain Strategy (P2)", batchSize*2, len(latencies), 0, start, latencies,
		fmt.Sprintf("saturation %.1fms | drain %.1fms | %.1f fills/s",
			satElapsed.Seconds()*1000, recElapsed.Seconds()*1000,
			float64(len(latencies))/recElapsed.Seconds())),
	}

	fmt.Println(strings.Repeat("=", 80))
	fmt.Printf("  STRESS TEST: %s\n", result.Name)
	fmt.Println(strings.Repeat("-", 80))
	fmt.Printf("  Orders:        %d\n", result.Orders)
	fmt.Printf("  Fills:         %d\n", result.Fills)
	fmt.Printf("  Duration:      %.1f ms\n", result.DurationMs)
	fmt.Printf("  Throughput:    %.1f orders/s | %.1f fills/s\n", result.OrdersPerSec, result.FillsPerSec)
	fmt.Printf("  Latency p50:   %.2f ms\n", result.P50LatencyMs)
	fmt.Printf("  Latency p95:   %.2f ms\n", result.P95LatencyMs)
	fmt.Printf("  Latency p99:   %.2f ms\n", result.P99LatencyMs)
	fmt.Printf("  Latency min:   %.2f ms\n", result.MinLatencyMs)
	fmt.Printf("  Latency max:   %.2f ms\n", result.MaxLatencyMs)
	fmt.Printf("  Notes:         %s\n", result.Notes)
	fmt.Println(strings.Repeat("=", 80))

	writeEnhancedJSON("stress_backlog_drain.json", result)
}

// --- Combined Summary Test ---
func TestStress_AllScenarios(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	scenarios := []struct {
		name string
		fn   func(*testing.T)
	}{
		{"SingleHotspot", TestStress_SingleHotspot},
		{"MultiMarketUniform", TestStress_MultiMarketUniform},
		{"HotspotSkew", TestStress_HotspotSkew},
		{"HighCancelMarketMaking", TestStress_HighCancelMarketMaking},
		{"BurstTraffic", TestStress_BurstTraffic},
		{"QueueSaturation", TestStress_QueueSaturation},
		{"LongSoak", TestStress_LongSoak},
		{"RecoveryBackpressure", TestStress_RecoveryBackpressure},
	}

	fmt.Println("\n" + strings.Repeat("=", 80))
	fmt.Println("  COMPREHENSIVE STRESS TEST SUITE - ALL SCENARIOS")
	fmt.Println(strings.Repeat("=", 80) + "\n")

	for _, s := range scenarios {
		t.Run(s.name, s.fn)
	}
}

// --- 9. Full-Chain Concurrency Test ---
// Tests end-to-end latency: API -> Sequencer -> Matching -> WAL -> Response
// Runs with 1, 4, 8, 16 concurrent workers hitting the rust-exchange HTTP API.
func TestStress_FullChainConcurrency(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping stress test in short mode")
	}

	apiBaseURL := os.Getenv("EXCHANGE_API_URL")
	if apiBaseURL == "" {
		apiBaseURL = "http://localhost:8080"
	}

	concurrencyLevels := []int{1, 4, 8, 16}
	ordersPerWorker := 50
	marketID := os.Getenv("EXCHANGE_TEST_MARKET")
	if marketID == "" {
		marketID = "BTC-USD"
	}

	type chainResult struct {
		Concurrency  int     `json:"concurrency"`
		TotalOrders  int     `json:"total_orders"`
		SuccessCount int     `json:"success_count"`
		ErrorCount   int     `json:"error_count"`
		DurationMs   float64 `json:"duration_ms"`
		OrdersPerSec float64 `json:"orders_per_sec"`
		P50LatencyMs float64 `json:"p50_latency_ms"`
		P95LatencyMs float64 `json:"p95_latency_ms"`
		P99LatencyMs float64 `json:"p99_latency_ms"`
		MaxLatencyMs float64 `json:"max_latency_ms"`
		MinLatencyMs float64 `json:"min_latency_ms"`
	}

	fmt.Println(strings.Repeat("=", 80))
	fmt.Println("  FULL-CHAIN CONCURRENCY TEST (API -> Sequencer -> Matching -> WAL)")
	fmt.Println(strings.Repeat("=", 80))
	fmt.Printf("  API Endpoint: %s\n", apiBaseURL)
	fmt.Printf("  Market:       %s\n", marketID)
	fmt.Printf("  Orders/Worker: %d\n\n", ordersPerWorker)

	// Verify API is reachable
	resp, err := http.Get(apiBaseURL + "/health")
	if err != nil {
		t.Skipf("Exchange API not reachable at %s (err=%v). Set EXCHANGE_API_URL to test.", apiBaseURL, err)
		return
	}
	if resp.StatusCode != 200 {
		statusCode := resp.StatusCode
		if resp.Body != nil {
			resp.Body.Close()
		}
		t.Skipf("Exchange API returned status %d at %s. Set EXCHANGE_API_URL to test.", statusCode, apiBaseURL)
		return
	}
	if resp.Body != nil {
		resp.Body.Close()
	}

	allResults := make([]chainResult, 0, len(concurrencyLevels))

	for _, concurrency := range concurrencyLevels {
		t.Run(fmt.Sprintf("concurrency_%d", concurrency), func(t *testing.T) {
			var wg sync.WaitGroup
			latencies := make([]float64, 0, concurrency*ordersPerWorker)
			var latMu sync.Mutex
			var successCount, errorCount int64

			start := time.Now()

			for w := 0; w < concurrency; w++ {
				wg.Add(1)
				go func(workerID int) {
					defer wg.Done()
					client := &http.Client{Timeout: 10 * time.Second}

					for i := 0; i < ordersPerWorker; i++ {
						orderID := fmt.Sprintf("fc-w%d-o%d-%d", workerID, i, time.Now().UnixNano())
						side := "buy"
						if i%2 == 0 {
							side = "sell"
						}

						payload := map[string]interface{}{
							"order_id":  orderID,
							"user_id":   fmt.Sprintf("fc-user-%d", workerID),
							"market_id": marketID,
							"side":      side,
							"price":     50000 + float64(i),
							"quantity":  0.01,
							"type":      "limit",
						}

						body, _ := json.Marshal(payload)
						reqStart := time.Now()

						req, err := http.NewRequest("POST", apiBaseURL+"/api/v1/orders", bytes.NewReader(body))
						if err != nil {
							atomic.AddInt64(&errorCount, 1)
							continue
						}
						req.Header.Set("Content-Type", "application/json")
						req.Header.Set("X-User-ID", fmt.Sprintf("fc-user-%d", workerID))

						resp, err := client.Do(req)
						latency := time.Since(reqStart).Seconds() * 1000

						if err != nil {
							atomic.AddInt64(&errorCount, 1)
							continue
						}
						if resp.Body != nil {
							resp.Body.Close()
						}

						if resp.StatusCode >= 200 && resp.StatusCode < 300 {
							atomic.AddInt64(&successCount, 1)
							latMu.Lock()
							latencies = append(latencies, latency)
							latMu.Unlock()
						} else {
							atomic.AddInt64(&errorCount, 1)
						}
					}
				}(w)
			}

			wg.Wait()
			elapsed := time.Since(start)

			totalOrders := concurrency * ordersPerWorker
			elapsedSec := elapsed.Seconds()

			result := chainResult{
				Concurrency:  concurrency,
				TotalOrders:  totalOrders,
				SuccessCount: int(successCount),
				ErrorCount:   int(errorCount),
				DurationMs:   elapsedSec * 1000,
				OrdersPerSec: float64(totalOrders) / elapsedSec,
			}
			if len(latencies) > 0 {
				result.P50LatencyMs = pctFloat(latencies, 0.50)
				result.P95LatencyMs = pctFloat(latencies, 0.95)
				result.P99LatencyMs = pctFloat(latencies, 0.99)
				result.MaxLatencyMs = pctFloat(latencies, 1.0)
				result.MinLatencyMs = pctFloat(latencies, 0.0)
			}
			allResults = append(allResults, result)

			fmt.Printf("  Concurrency %2d: %d orders | %d ok / %d err | %.1fms duration | %.0f ops/s\n",
				concurrency, totalOrders, result.SuccessCount, result.ErrorCount,
				result.DurationMs, result.OrdersPerSec)
			if len(latencies) > 0 {
				fmt.Printf("               Latency: p50=%.2fms p95=%.2fms p99=%.2fms max=%.2fms\n",
					result.P50LatencyMs, result.P95LatencyMs, result.P99LatencyMs, result.MaxLatencyMs)
			}
		})
	}

	// Summary table
	fmt.Println()
	fmt.Println(strings.Repeat("=", 80))
	fmt.Printf("  %-14s | %-8s | %-8s | %-8s | %-10s | %-10s | %-10s | %-10s\n",
		"Concurrency", "Orders", "Success", "Errors", "Duration(ms)", "Ops/sec", "p50(ms)", "p99(ms)")
	fmt.Println(strings.Repeat("-", 80))
	for _, r := range allResults {
		fmt.Printf("  %-14d | %-8d | %-8d | %-8d | %-10.1f | %-10.0f | %-10.2f | %-10.2f\n",
			r.Concurrency, r.TotalOrders, r.SuccessCount, r.ErrorCount,
			r.DurationMs, r.OrdersPerSec, r.P50LatencyMs, r.P99LatencyMs)
	}
	fmt.Println(strings.Repeat("=", 80))

	// Write JSON
	dir := filepath.Join("..", "docs", "benchmarks")
	_ = os.MkdirAll(dir, 0o755)
	path := filepath.Join(dir, "stress_full_chain_concurrency.json")

	jsonBytes, _ := json.MarshalIndent(allResults, "", "  ")
	_ = os.WriteFile(path, jsonBytes, 0o644)
	t.Logf("Results written to %s", path)
}
