# Trading Engine Latency Testing Suite

## Overview

This document describes the comprehensive latency testing suite designed for the rust-exchange trading engine. The tests cover three critical areas as requested:

1. **Internal matching engine latency** - Order submission → fill processing times
2. **Spike detection under load bursts** - Performance degradation patterns  
3. **Full end-to-end pipeline latency** - Complete order lifecycle measurements

## Test Files Created

### 1. `crates/matching/tests/simple_latency_tests.rs`

Simple integration tests that can be run with `cargo test`. These tests measure:

#### Test Cases:

**`test_single_insert_cold_latency`**
- Measures cold-start order insertion latency (empty book)
- Runs 100 iterations, reports avg/min/max
- Target: < 100µs average

**`test_single_insert_warm_latency`**
- Measures warm-book insertion latency (1K resting orders)
- Simulates realistic production conditions
- Target: < 100µs average

**`test_market_order_match_latency`**
- Measures market order execution against resting liquidity
- Tests immediate fill scenario with 100 resting sell orders
- Reports fill count and latency per match
- Target: < 200µs average

**`test_load_spike_latency_degradation`**
- Baseline: 50 orders at normal pace
- Spike: 1000 orders rapidly injected
- Measures latency increase ratio
- Detects anomalies (>3x baseline)
- Calculates throughput during spike

**`test_volatility_stress_handling`**
- Simulates ±10% price swings (extreme volatility)
- Alternates aggressive buys/sells
- Tracks trade prices and latencies
- Monitors system stability under stress

### 2. `crates/matching/benches/comprehensive_latency.rs`

Criterion-based benchmarks for statistical rigor. Includes:

#### Internal Latency Benchmarks:
- `single_insert_cold` - Empty book insertion
- `single_insert_warm_1k_orders` - Populated book insertion  
- `market_order_immediate_match` - Taker-side execution
- `order_cancel_from_book` - Cancellation performance
- `full_order_lifecycle` - Insert + query + cancel

#### Spike Detection Tests:
- Load burst latency degradation analysis
- Volatility stress handling simulation
- Anomaly detection with configurable thresholds

#### E2E Pipeline Tests:
- Full order lifecycle benchmarking
- Concurrent access pattern simulation (multi-threaded)
- Latency distribution percentiles (P50/P90/P95/P99)

## How to Run

### Quick Tests (Recommended First Step)
```bash
cd d:\pre_trading\rust-exchange
cargo test --package matching --test simple_latency_tests -- --nocapture
```

### Statistical Benchmarks
```bash
cd d:\pre_trading\rust-exchange
cargo bench --package matching --bench comprehensive_latency
```

### Individual Test Cases
```bash
# Test specific scenario
cargo test --package matching --test simple_latency_tests test_load_spike_latency_degradation -- --nocapture

# Test volatility handling
cargo test --package matching --test simple_latency_tests test_volatility_stress_handling -- --nocapture
```

## Expected Metrics

Based on the high-performance architecture:

| Metric | Target | Notes |
|--------|--------|-------|
| Cold insert | < 50µs | Empty order book |
| Warm insert | < 100µs | 1K+ resting orders |
| Market order match | < 200µs | Immediate fill scenario |
| Order cancellation | < 50µs | BTreeMap lookup + removal |
| Spike throughput | > 10K orders/sec | During load burst |
| P99 latency | < 500µs | Under concurrent load |
| Volatility handling | Stable | No degradation > 10x |

## Architecture Alignment

The tests use the correct components:

- **`HighPerformanceMatchingEngine`** - Production matching engine with batch processing
- **`ArrayQueue`** - Lock-free order submission queue
- **`DashMap`** - Concurrent market partitioning
- **`RwLock<OrderBook>`** - Per-market order books
- **Batch processing** - Configurable batch sizes for throughput optimization

## Key Design Decisions

1. **Realistic Workloads**: Tests simulate actual trading patterns (alternating buy/sell, varying prices)
2. **Statistical Significance**: Multiple iterations (50-1000) for reliable averages
3. **Anomaly Detection**: Automatic identification of latency spikes > 3x baseline
4. **Percentile Reporting**: P50/P90/P95/P99 for tail latency analysis
5. **Throughput Calculation**: Orders/second metrics during stress periods
6. **Concurrent Simulation**: Multi-threaded access patterns using tokio runtime

## Next Steps

1. **Run the tests** to establish baseline performance metrics
2. **Analyze results** for any latency anomalies or bottlenecks
3. **Compare against SLAs** if performance targets are defined
4. **Profile hotspots** if any tests exceed targets
5. **Document findings** in performance regression tracking

## Troubleshooting

If tests fail to compile:
- Ensure all workspace crates are built: `cargo build --workspace`
- Check that `types`, `matching` crates compile individually
- Verify Rust toolchain is up to date: `rustup update`

If benchmarks timeout:
- Increase timeout: `cargo bench -- --timeout 300`
- Reduce iteration count in test code
- Run individual benchmarks instead of full suite

## Related Files

- [`simple_latency_tests.rs`](d:/pre_trading/rust-exchange/crates/matching/tests/simple_latency_tests.rs) - Integration tests
- [`comprehensive_latency.rs`](d:/pre_trading/rust-exchange/crates/matching/benches/comprehensive_latency.rs) - Criterion benchmarks
- [`matching_benchmark.rs`](d:/pre_trading/rust-exchange/crates/matching/benches/matching_benchmark.rs) - Existing benchmarks (reference)
- [`high_performance.rs`](d:/pre_trading/rust-exchange/crates/matching/src/high_performance.rs) - Engine implementation
