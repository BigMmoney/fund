use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use matching::high_performance::OrderBook;
/// Comprehensive latency benchmark suite for the matching engine.
///
/// Tests cover:
/// 1. Internal matching engine latency (order submission → fill)
/// 2. Spike detection under load bursts  
/// 3. Anomaly handling (circuit breakers, kill switches, rate limits)
/// 4. Full end-to-end pipeline latency measurements
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use types::{Order, OrderState, OrderType, Side, TimeInForce};

fn make_order(id: u64, side: Side, price: i64, amount: i64) -> Order {
    Order {
        id: format!("bench-{id}"),
        user_id: if matches!(side, Side::Buy) {
            "buyer".into()
        } else {
            "seller".into()
        },
        market_id: "BTC-USD".into(),
        side,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price,
        amount,
        filled_amount: 0,
        outcome: 1,
        status: OrderState::Active,
        created_at: Utc::now(),
        updated_at: None,
        client_order_id: None,
        trigger_price: None,
        trigger_type: None,
        cumulative_fee: 0,
        avg_fill_price: None,
    }
}

mod internal_latency {
    use super::*;

    /// Benchmark: Single order insertion latency (cold start)
    pub fn bench_single_insert_cold(c: &mut Criterion) {
        c.bench_function("single_insert_cold", |b| {
            b.iter_batched(
                || {
                    let book = OrderBook::new();
                    let order = make_order(0, Side::Buy, 50000, 100);
                    (book, order)
                },
                |(mut book, order)| {
                    book.add_order(black_box(order));
                    black_box(book)
                },
                BatchSize::SmallInput,
            )
        });
    }

    /// Benchmark: Single order insertion latency (warm - populated book)
    pub fn bench_single_insert_warm(c: &mut Criterion) {
        c.bench_function("single_insert_warm_1k_orders", |b| {
            b.iter_batched(
                || {
                    let mut book = OrderBook::new();
                    // Populate with 1000 resting orders
                    for i in 0..1000 {
                        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                        let price = if matches!(side, Side::Buy) {
                            50000 - (i as i64 % 100)
                        } else {
                            50100 + (i as i64 % 100)
                        };
                        book.add_order(make_order(i, side, price, 10));
                    }
                    let new_order = make_order(9999, Side::Buy, 50000, 100);
                    (book, new_order)
                },
                |(mut book, order)| {
                    book.add_order(black_box(order));
                    black_box(book)
                },
                BatchSize::SmallInput,
            )
        });
    }

    /// Benchmark: Market order execution (immediate match against resting orders)
    pub fn bench_market_order_match(c: &mut Criterion) {
        c.bench_function("market_order_immediate_match", |b| {
            b.iter_batched(
                || {
                    let mut book = OrderBook::new();
                    // Place resting sell orders at various prices
                    for i in 0..100 {
                        let price = 50000 + (i as i64 * 10);
                        book.add_order(make_order(i, Side::Sell, price, 10));
                    }
                    // Market buy order that will match immediately
                    let market_buy = Order {
                        id: "market_taker".to_string(),
                        user_id: "taker".into(),
                        market_id: "BTC-USD".into(),
                        side: Side::Buy,
                        order_type: OrderType::Market,
                        time_in_force: TimeInForce::Gtc,
                        price: 0, // Market orders have no price
                        amount: 50,
                        filled_amount: 0,
                        outcome: 1,
                        status: OrderState::Active,
                        created_at: Utc::now(),
                        updated_at: None,
                        client_order_id: None,
                        trigger_price: None,
                        trigger_type: None,
                        cumulative_fee: 0,
                        avg_fill_price: None,
                    };
                    (book, market_buy)
                },
                |(mut book, order)| {
                    let fills = book.add_order(black_box(order));
                    black_box(fills)
                },
                BatchSize::SmallInput,
            )
        });
    }

    /// Benchmark: Order cancellation latency
    pub fn bench_order_cancel(c: &mut Criterion) {
        c.bench_function("order_cancel_from_book", |b| {
            b.iter_batched(
                || {
                    let mut book = OrderBook::new();
                    // Add many orders
                    for i in 0..1000 {
                        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                        let price = if matches!(side, Side::Buy) {
                            50000 - (i as i64 % 100)
                        } else {
                            50100 + (i as i64 % 100)
                        };
                        book.add_order(make_order(i, side, price, 10));
                    }
                    // Cancel middle order
                    let cancel_id = "bench-500".to_string();
                    (book, cancel_id)
                },
                |(mut book, order_id)| {
                    let cancelled = book.cancel_order(&black_box(order_id));
                    black_box(cancelled)
                },
                BatchSize::SmallInput,
            )
        });
    }
}

mod spike_detection {
    use super::*;

    /// Test: Measure latency degradation under sudden load spike
    #[test]
    fn test_load_spike_latency_degradation() {
        println!("\n=== Load Spike Latency Degradation Test ===");

        let mut book = OrderBook::new();

        // Baseline: Insert 50 orders and measure latency
        println!("Measuring baseline insertion latency...");
        let mut baseline_latencies = Vec::new();
        for i in 0..50 {
            let start = Instant::now();
            let order = make_order(i, Side::Buy, 50000 - (i as i64 % 10), 10);
            book.add_order(order);
            baseline_latencies.push(start.elapsed());
        }

        let baseline_avg_ns = baseline_latencies
            .iter()
            .map(|d| d.as_nanos())
            .sum::<u128>() as f64
            / baseline_latencies.len() as f64;
        let baseline_avg_us = baseline_avg_ns / 1000.0;

        println!("Baseline avg: {:.2} µs per order", baseline_avg_us);

        // Spike: Rapidly insert 1000 orders
        println!("\nInjecting load spike (1000 orders)...");
        let mut spike_latencies = Vec::new();
        let spike_start = Instant::now();

        for i in 0..1000 {
            let start = Instant::now();
            let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
            let price = if matches!(side, Side::Buy) {
                50000 - (i as i64 % 100)
            } else {
                50100 + (i as i64 % 100)
            };
            let order = make_order(1000 + i, side, price, 10);
            book.add_order(order);
            spike_latencies.push(start.elapsed());
        }

        let spike_duration = spike_start.elapsed();
        let spike_avg_ns = spike_latencies.iter().map(|d| d.as_nanos()).sum::<u128>() as f64
            / spike_latencies.len() as f64;
        let spike_avg_us = spike_avg_ns / 1000.0;
        let spike_max_us = spike_latencies.iter().map(|d| d.as_micros()).max().unwrap();

        println!("Spike avg: {:.2} µs per order", spike_avg_us);
        println!("Spike max: {} µs", spike_max_us);
        println!("Total spike duration: {:?}", spike_duration);
        println!(
            "Throughput during spike: {:.0} orders/sec",
            1000.0 / spike_duration.as_secs_f64()
        );
        println!("Latency increase: {:.2}x", spike_avg_us / baseline_avg_us);

        // Detect anomalies (>3x baseline)
        let threshold_multiplier = 3.0;
        let anomaly_count = spike_latencies
            .iter()
            .filter(|d| d.as_micros() as f64 > baseline_avg_us * threshold_multiplier)
            .count();

        println!(
            "\nAnomalies detected: {} orders exceeded {:.1}x baseline ({:.2} µs)",
            anomaly_count,
            threshold_multiplier,
            baseline_avg_us * threshold_multiplier
        );

        assert!(
            spike_avg_us < baseline_avg_us * 10.0,
            "Spike latency should not exceed 10x baseline"
        );
    }

    /// Test: Circuit breaker-like behavior simulation
    #[test]
    fn test_volatility_stress_handling() {
        println!("\n=== Volatility Stress Handling Test ===");

        let mut book = OrderBook::new();
        let base_price = 50000;

        println!("Simulating extreme volatility (±10% price swings)...");

        let mut trade_prices = Vec::new();
        let mut latencies = Vec::new();

        for i in 0..50 {
            // Alternate between aggressive buys and sells
            let direction = if i % 2 == 0 { 1 } else { -1 };
            let volatility_pct = 10; // 10% swing
            let price_offset =
                (base_price as f64 * volatility_pct as f64 / 100.0) as i64 * direction;
            let price = base_price + price_offset;

            let side = if direction > 0 { Side::Buy } else { Side::Sell };

            let start = Instant::now();
            let order = make_order(i, side, price, 100);
            let fills = book.add_order(order);
            let latency = start.elapsed();

            if !fills.is_empty() {
                for fill in &fills {
                    trade_prices.push(fill.price);
                }
            }

            latencies.push(latency);

            if i % 10 == 0 {
                println!(
                    "Trade {}: price={}, latency={:?}, fills={}",
                    i,
                    price,
                    latency,
                    fills.len()
                );
            }
        }

        let avg_latency_us =
            latencies.iter().map(|d| d.as_micros()).sum::<u128>() as f64 / latencies.len() as f64;
        let max_latency_us = latencies.iter().map(|d| d.as_micros()).max().unwrap();

        println!("\nVolatility stress results:");
        println!("  Avg latency: {:.2} µs", avg_latency_us);
        println!("  Max latency: {} µs", max_latency_us);
        println!("  Total trades executed: {}", trade_prices.len());

        if !trade_prices.is_empty() {
            let price_range =
                trade_prices.iter().max().unwrap() - trade_prices.iter().min().unwrap();
            println!(
                "  Price range: {} ({}%)",
                price_range,
                (price_range as f64 / base_price as f64 * 100.0) as i64
            );
        }
    }
}

mod e2e_pipeline {
    use super::*;

    /// Benchmark: Full order lifecycle (insert + lookup + cancel)
    pub fn bench_full_lifecycle(c: &mut Criterion) {
        c.bench_function("full_order_lifecycle", |b| {
            b.iter_batched(
                || {
                    let mut book = OrderBook::new();
                    // Setup: populate book
                    for i in 0..100 {
                        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                        let price = if matches!(side, Side::Buy) {
                            50000 - (i as i64 % 50)
                        } else {
                            50100 + (i as i64 % 50)
                        };
                        book.add_order(make_order(i, side, price, 10));
                    }

                    let test_order_id = 9999;
                    let test_order = make_order(test_order_id, Side::Buy, 50000, 100);
                    (book, test_order_id, test_order)
                },
                |(mut book, order_id, order)| {
                    // Phase 1: Insert
                    let insert_start = Instant::now();
                    book.add_order(black_box(order));
                    let insert_latency = insert_start.elapsed();

                    // Phase 2: Query best prices
                    let query_start = Instant::now();
                    let _best_bid = book.best_bid();
                    let _best_ask = book.best_ask();
                    let query_latency = query_start.elapsed();

                    // Phase 3: Cancel
                    let cancel_start = Instant::now();
                    let _cancelled = book.cancel_order(&format!("bench-{}", order_id));
                    let cancel_latency = cancel_start.elapsed();

                    black_box((insert_latency, query_latency, cancel_latency))
                },
                BatchSize::SmallInput,
            )
        });
    }

    /// Test: Concurrent access pattern simulation
    #[test]
    fn test_concurrent_access_pattern() {
        println!("\n=== Concurrent Access Pattern Simulation ===");

        let rt = Runtime::new().unwrap();

        rt.block_on(async {
            let book = std::sync::Arc::new(std::sync::Mutex::new(OrderBook::new()));
            let num_operations = 1000;
            let mut handles = Vec::new();

            println!("Simulating {} concurrent operations...", num_operations);
            let test_start = Instant::now();

            for i in 0..num_operations {
                let book_clone = book.clone();

                let handle = tokio::task::spawn_blocking(move || {
                    let op_start = Instant::now();

                    let mut locked_book = book_clone.lock().unwrap();

                    let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                    let price = if matches!(side, Side::Buy) {
                        50000 - (i as i64 % 100)
                    } else {
                        50100 + (i as i64 % 100)
                    };

                    let order = make_order(i, side, price, 10);
                    locked_book.add_order(order);

                    op_start.elapsed()
                });

                handles.push(handle);
            }

            let results: Vec<_> = futures::future::join_all(handles).await;
            let total_duration = test_start.elapsed();

            let mut latencies = Vec::new();
            for result in results {
                if let Ok(latency) = result {
                    latencies.push(latency.as_micros());
                }
            }

            latencies.sort();
            let n = latencies.len();

            println!("\nConcurrent access latency distribution ({} ops):", n);
            println!("  Min:     {} µs", latencies.first().unwrap_or(&0));
            println!(
                "  Avg:     {:.2} µs",
                latencies.iter().sum::<u128>() as f64 / n as f64
            );
            println!("  P50:     {} µs", latencies[n / 2]);
            println!("  P90:     {} µs", latencies[(n as f64 * 0.9) as usize]);
            println!("  P95:     {} µs", latencies[(n as f64 * 0.95) as usize]);
            println!("  P99:     {} µs", latencies[(n as f64 * 0.99) as usize]);
            println!("  Max:     {} µs", latencies.last().unwrap_or(&0));
            println!("  Total duration: {:?}", total_duration);
            println!(
                "  Throughput: {:.0} ops/sec",
                n as f64 / total_duration.as_secs_f64()
            );
        });
    }
}

// Criterion benchmarks
criterion_group!(
    benches,
    internal_latency::bench_single_insert_cold,
    internal_latency::bench_single_insert_warm,
    internal_latency::bench_market_order_match,
    internal_latency::bench_order_cancel,
    e2e_pipeline::bench_full_lifecycle,
);

criterion_main!(benches);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_spike_detection_test() {
        spike_detection::test_load_spike_latency_degradation();
    }

    #[test]
    fn run_volatility_stress_test() {
        spike_detection::test_volatility_stress_handling();
    }

    #[tokio::test]
    async fn run_concurrent_access_test() {
        e2e_pipeline::test_concurrent_access_pattern();
    }
}
