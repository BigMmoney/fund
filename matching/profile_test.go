package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"os"
	"path/filepath"
	"sort"
	"testing"
	"time"

	"pre_trading/services/eventbus"
	"pre_trading/services/types"
)

type systemProfile struct {
	GeneratedAt string            `json:"generated_at"`
	Scenarios   []windowMetricRow `json:"scenarios"`
}

type windowMetricRow struct {
	Label         string  `json:"label,omitempty"`
	Mode          string  `json:"mode,omitempty"`
	BatchWindowMs int     `json:"batch_window_ms"`
	Orders        int     `json:"orders"`
	Fills         int     `json:"fills"`
	DurationMs    float64 `json:"duration_ms"`
	OrdersPerSec  float64 `json:"orders_per_sec"`
	FillsPerSec   float64 `json:"fills_per_sec"`
	P50LatencyMs  float64 `json:"p50_latency_ms"`
	P95LatencyMs  float64 `json:"p95_latency_ms"`
	P99LatencyMs  float64 `json:"p99_latency_ms"`
}

func percentileMs(values []float64, q float64) float64 {
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

func runScenario(window time.Duration, pairs int) (windowMetricRow, error) {
	bus := eventbus.NewEventBus()
	engine := NewMatchingEngine(window, bus)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go engine.Start(ctx)

	fillCh := bus.Subscribe(types.EventTypeFillCreated, pairs*4+1024)
	createdAt := make(map[string]time.Time, pairs*2)
	now := time.Now()

	// Give the engine loop a brief warmup.
	time.Sleep(20 * time.Millisecond)
	start := time.Now()

	for i := 0; i < pairs; i++ {
		buyID := fmt.Sprintf("buy-%d", i)
		sellID := fmt.Sprintf("sell-%d", i)
		ts := time.Now()
		createdAt[buyID] = ts
		createdAt[sellID] = ts

		engine.AddIntent(&types.Intent{
			ID:        buyID,
			UserID:    fmt.Sprintf("ub-%d", i),
			MarketID:  "bench-mkt",
			Side:      "buy",
			Price:     60,
			Amount:    1,
			Outcome:   1,
			CreatedAt: ts,
			ExpiresAt: now.Add(20 * time.Second),
			Status:    "pending",
		})
		engine.AddIntent(&types.Intent{
			ID:        sellID,
			UserID:    fmt.Sprintf("us-%d", i),
			MarketID:  "bench-mkt",
			Side:      "sell",
			Price:     40,
			Amount:    1,
			Outcome:   1,
			CreatedAt: ts,
			ExpiresAt: now.Add(20 * time.Second),
			Status:    "pending",
		})
	}

	expectedFills := pairs * 2
	received := 0
	latencies := make([]float64, 0, expectedFills)
	deadline := time.After(10*window + 5*time.Second)

	for received < expectedFills {
		select {
		case evt := <-fillCh:
			fill, ok := evt.Payload.(types.Fill)
			if !ok {
				continue
			}
			received++
			if ct, exists := createdAt[fill.IntentID]; exists {
				latencies = append(latencies, time.Since(ct).Seconds()*1000)
			}
		case <-deadline:
			return windowMetricRow{}, fmt.Errorf("timeout waiting fills: got %d want %d", received, expectedFills)
		}
	}

	elapsed := time.Since(start)
	elapsedSec := elapsed.Seconds()
	orders := pairs * 2
	fills := received
	row := windowMetricRow{
		BatchWindowMs: int(window / time.Millisecond),
		Orders:        orders,
		Fills:         fills,
		DurationMs:    elapsedSec * 1000,
		OrdersPerSec:  float64(orders) / elapsedSec,
		FillsPerSec:   float64(fills) / elapsedSec,
		P50LatencyMs:  percentileMs(latencies, 0.50),
		P95LatencyMs:  percentileMs(latencies, 0.95),
		P99LatencyMs:  percentileMs(latencies, 0.99),
	}
	return row, nil
}

func runImmediateScenario(pairs int) (windowMetricRow, error) {
	bus := eventbus.NewEventBus()
	engine := NewMatchingEngine(time.Hour, bus)
	fillCh := bus.Subscribe(types.EventTypeFillCreated, pairs*4+1024)
	createdAt := make(map[string]time.Time, pairs*2)
	start := time.Now()

	for i := 0; i < pairs; i++ {
		buyID := fmt.Sprintf("imm-buy-%d", i)
		sellID := fmt.Sprintf("imm-sell-%d", i)
		ts := time.Now()
		createdAt[buyID] = ts
		createdAt[sellID] = ts

		engine.AddIntent(&types.Intent{
			ID:        buyID,
			UserID:    fmt.Sprintf("ib-%d", i),
			MarketID:  "bench-mkt",
			Side:      "buy",
			Price:     60,
			Amount:    1,
			Outcome:   1,
			CreatedAt: ts,
			ExpiresAt: ts.Add(20 * time.Second),
			Status:    "pending",
		})
		engine.AddIntent(&types.Intent{
			ID:        sellID,
			UserID:    fmt.Sprintf("is-%d", i),
			MarketID:  "bench-mkt",
			Side:      "sell",
			Price:     40,
			Amount:    1,
			Outcome:   1,
			CreatedAt: ts,
			ExpiresAt: ts.Add(20 * time.Second),
			Status:    "pending",
		})

		engine.processBatch()
	}

	expectedFills := pairs * 2
	latencies := make([]float64, 0, expectedFills)
	for received := 0; received < expectedFills; received++ {
		evt := <-fillCh
		fill, ok := evt.Payload.(types.Fill)
		if !ok {
			return windowMetricRow{}, fmt.Errorf("unexpected fill payload type")
		}
		if ct, exists := createdAt[fill.IntentID]; exists {
			latencies = append(latencies, time.Since(ct).Seconds()*1000)
		}
	}

	elapsed := time.Since(start)
	elapsedSec := elapsed.Seconds()
	orders := pairs * 2
	row := windowMetricRow{
		Label:         "Immediate Surrogate",
		Mode:          "immediate",
		BatchWindowMs: 0,
		Orders:        orders,
		Fills:         expectedFills,
		DurationMs:    elapsedSec * 1000,
		OrdersPerSec:  float64(orders) / elapsedSec,
		FillsPerSec:   float64(expectedFills) / elapsedSec,
		P50LatencyMs:  percentileMs(latencies, 0.50),
		P95LatencyMs:  percentileMs(latencies, 0.95),
		P99LatencyMs:  percentileMs(latencies, 0.99),
	}
	return row, nil
}

func writeProfileArtifacts(profile systemProfile) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "matching_system_profile.json")
	mdPath := filepath.Join(base, "matching_system_profile.md")

	raw, err := json.MarshalIndent(profile, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	md := "# Matching System Profile\n\n" +
		"| Batch Window (ms) | Orders | Fills | Orders/s | Fills/s | p50 Latency (ms) | p99 Latency (ms) |\n" +
		"|---:|---:|---:|---:|---:|---:|---:|\n"
	for _, r := range profile.Scenarios {
		md += fmt.Sprintf(
			"| %d | %d | %d | %.1f | %.1f | %.2f | %.2f |\n",
			r.BatchWindowMs,
			r.Orders,
			r.Fills,
			r.OrdersPerSec,
			r.FillsPerSec,
			r.P50LatencyMs,
			r.P99LatencyMs,
		)
	}
	return os.WriteFile(mdPath, []byte(md), 0o644)
}

func writePaperEvaluationArtifacts(rows []windowMetricRow) error {
	base := filepath.Join("..", "docs", "benchmarks")
	if err := os.MkdirAll(base, 0o755); err != nil {
		return err
	}

	jsonPath := filepath.Join(base, "paper_evaluation_profile.json")
	mdPath := filepath.Join(base, "paper_evaluation_profile.md")
	csvPath := filepath.Join(base, "paper_evaluation_profile.csv")

	payload := map[string]any{
		"generated_at": time.Now().UTC().Format(time.RFC3339),
		"scenarios":    rows,
	}
	raw, err := json.MarshalIndent(payload, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(jsonPath, append(raw, '\n'), 0o644); err != nil {
		return err
	}

	md := "# Paper Evaluation Profile\n\n" +
		"| Scenario | Mode | Batch Window (ms) | Orders | Fills | Orders/s | Fills/s | p50 (ms) | p95 (ms) | p99 (ms) |\n" +
		"|---|---|---:|---:|---:|---:|---:|---:|---:|---:|\n"
	csv := "scenario,mode,batch_window_ms,orders,fills,orders_per_sec,fills_per_sec,p50_latency_ms,p95_latency_ms,p99_latency_ms\n"
	for _, r := range rows {
		md += fmt.Sprintf(
			"| %s | %s | %d | %d | %d | %.1f | %.1f | %.2f | %.2f | %.2f |\n",
			r.Label,
			r.Mode,
			r.BatchWindowMs,
			r.Orders,
			r.Fills,
			r.OrdersPerSec,
			r.FillsPerSec,
			r.P50LatencyMs,
			r.P95LatencyMs,
			r.P99LatencyMs,
		)
		csv += fmt.Sprintf(
			"%s,%s,%d,%d,%d,%.4f,%.4f,%.4f,%.4f,%.4f\n",
			r.Label,
			r.Mode,
			r.BatchWindowMs,
			r.Orders,
			r.Fills,
			r.OrdersPerSec,
			r.FillsPerSec,
			r.P50LatencyMs,
			r.P95LatencyMs,
			r.P99LatencyMs,
		)
	}

	if err := os.WriteFile(mdPath, []byte(md), 0o644); err != nil {
		return err
	}
	return os.WriteFile(csvPath, []byte(csv), 0o644)
}

func TestGenerateMatchingSystemProfile(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SYSTEM_BENCH") != "1" {
		t.Skip("set RUN_SYSTEM_BENCH=1 to run batch-window system profile")
	}

	prevWriter := log.Writer()
	log.SetOutput(io.Discard)
	defer log.SetOutput(prevWriter)

	rows := make([]windowMetricRow, 0, 3)
	for _, ms := range []int{100, 500, 1000} {
		row, err := runScenario(time.Duration(ms)*time.Millisecond, 200)
		if err != nil {
			t.Fatalf("scenario %dms failed: %v", ms, err)
		}
		if row.Orders != row.Fills {
			t.Fatalf("orders/fills mismatch for %dms: orders=%d fills=%d", ms, row.Orders, row.Fills)
		}
		rows = append(rows, row)
	}

	// Latency should generally increase with wider batch windows.
	if !(rows[0].P50LatencyMs <= rows[1].P50LatencyMs && rows[1].P50LatencyMs <= rows[2].P50LatencyMs) {
		t.Fatalf("unexpected p50 trend across windows: %+v", rows)
	}

	profile := systemProfile{
		GeneratedAt: time.Now().UTC().Format(time.RFC3339),
		Scenarios:   rows,
	}
	if err := writeProfileArtifacts(profile); err != nil {
		t.Fatalf("write profile artifacts failed: %v", err)
	}

	for _, r := range rows {
		t.Logf("window=%dms orders/s=%.1f fills/s=%.1f p50=%.2fms p99=%.2fms",
			r.BatchWindowMs, r.OrdersPerSec, r.FillsPerSec, r.P50LatencyMs, r.P99LatencyMs)
	}
}

func TestGeneratePaperEvaluationProfile(t *testing.T) {
	t.Helper()
	if os.Getenv("RUN_SYSTEM_BENCH") != "1" {
		t.Skip("set RUN_SYSTEM_BENCH=1 to run paper evaluation profile")
	}

	prevWriter := log.Writer()
	log.SetOutput(io.Discard)
	defer log.SetOutput(prevWriter)

	rows := make([]windowMetricRow, 0, 4)

	imm, err := runImmediateScenario(200)
	if err != nil {
		t.Fatalf("immediate scenario failed: %v", err)
	}
	rows = append(rows, imm)

	for _, ms := range []int{100, 250, 500} {
		row, err := runScenario(time.Duration(ms)*time.Millisecond, 200)
		if err != nil {
			t.Fatalf("scenario %dms failed: %v", ms, err)
		}
		row.Label = fmt.Sprintf("FBA-%dms", ms)
		row.Mode = "batch"
		if row.Orders != row.Fills {
			t.Fatalf("orders/fills mismatch for %dms: orders=%d fills=%d", ms, row.Orders, row.Fills)
		}
		rows = append(rows, row)
	}

	if !(rows[0].P50LatencyMs <= rows[1].P50LatencyMs &&
		rows[1].P50LatencyMs <= rows[2].P50LatencyMs &&
		rows[2].P50LatencyMs <= rows[3].P50LatencyMs) {
		t.Fatalf("unexpected p50 trend across paper scenarios: %+v", rows)
	}

	if err := writePaperEvaluationArtifacts(rows); err != nil {
		t.Fatalf("write paper evaluation artifacts failed: %v", err)
	}

	for _, r := range rows {
		t.Logf("%s mode=%s window=%dms orders/s=%.1f p50=%.2fms p95=%.2fms p99=%.2fms",
			r.Label, r.Mode, r.BatchWindowMs, r.OrdersPerSec, r.P50LatencyMs, r.P95LatencyMs, r.P99LatencyMs)
	}
}
