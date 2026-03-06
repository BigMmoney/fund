package benchmark

import (
	"sort"
	"time"
)

type LatencyStats struct {
	Count int
	MinMs float64
	P50Ms float64
	P95Ms float64
	P99Ms float64
	MaxMs float64
}

func durationToMs(d time.Duration) float64 {
	return d.Seconds() * 1000
}

func percentile(values []float64, q float64) float64 {
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

func ComputeLatencyStats(samples []time.Duration) LatencyStats {
	if len(samples) == 0 {
		return LatencyStats{}
	}

	ms := make([]float64, 0, len(samples))
	for _, s := range samples {
		ms = append(ms, durationToMs(s))
	}
	sort.Float64s(ms)

	return LatencyStats{
		Count: len(ms),
		MinMs: ms[0],
		P50Ms: percentile(ms, 0.50),
		P95Ms: percentile(ms, 0.95),
		P99Ms: percentile(ms, 0.99),
		MaxMs: ms[len(ms)-1],
	}
}
