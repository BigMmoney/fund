package benchmark

import (
	"testing"
	"time"
)

func TestGenerateBalancedOrders(t *testing.T) {
	orders := GenerateBalancedOrders(3, "mkt", 1, 60, 40, 2)
	if len(orders) != 6 {
		t.Fatalf("expected 6 orders, got %d", len(orders))
	}
	if orders[0].Side != Buy || orders[1].Side != Sell {
		t.Fatalf("expected buy/sell pair ordering")
	}
}

func TestComputeLatencyStats(t *testing.T) {
	stats := ComputeLatencyStats([]time.Duration{
		10 * time.Millisecond,
		20 * time.Millisecond,
		30 * time.Millisecond,
		40 * time.Millisecond,
		50 * time.Millisecond,
	})
	if stats.Count != 5 {
		t.Fatalf("expected count=5, got %d", stats.Count)
	}
	if stats.P50Ms < 29 || stats.P50Ms > 31 {
		t.Fatalf("unexpected p50: %f", stats.P50Ms)
	}
	if stats.P99Ms < stats.P95Ms {
		t.Fatalf("expected p99 >= p95")
	}
}

func TestComputeThroughput(t *testing.T) {
	got := ComputeThroughput(100, 2*time.Second)
	if got != 50 {
		t.Fatalf("expected throughput=50, got %f", got)
	}
}
