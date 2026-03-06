package benchmark

import "time"

func ComputeThroughput(events int, elapsed time.Duration) float64 {
	if events <= 0 || elapsed <= 0 {
		return 0
	}
	return float64(events) / elapsed.Seconds()
}
