# Benchmark Utilities

This package contains reusable experiment helpers for system evaluation.

## Components

- `order_generator.go`
  - synthetic order-flow generation for deterministic workloads.

- `latency_measurement.go`
  - p50/p95/p99 and summary stats for latency samples.

- `throughput_measurement.go`
  - throughput helpers (`orders/sec`, `fills/sec`).

## Typical Usage

```go
orders := benchmark.GenerateBalancedOrders(1000, "mkt-1", 1, 60, 40, 1)
stats := benchmark.ComputeLatencyStats(samples)
ops := benchmark.ComputeThroughput(len(orders), elapsed)
```
