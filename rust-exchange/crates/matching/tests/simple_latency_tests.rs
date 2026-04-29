use chrono::Utc;
use matching::high_performance::HighPerformanceMatchingEngine;
/// Simple latency tests for the matching engine.
/// Run with: cargo test --package matching --test simple_latency_tests
use std::time::Instant;
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

#[test]
fn test_single_insert_cold_latency() {
    println!("\n=== Cold Start Insert Latency ===");

    let engine = HighPerformanceMatchingEngine::new(100, 10000);
    let mut latencies = Vec::new();

    for i in 0..100 {
        let order = make_order(i, Side::Buy, 50000, 100);

        engine.submit_order(order).unwrap();
        let result = engine.process_batch();

        latencies.push(result.processing_time_us as u128);
    }

    let avg_us = latencies.iter().sum::<u128>() as f64 / latencies.len() as f64;
    let min_us = latencies.iter().min().unwrap();
    let max_us = latencies.iter().max().unwrap();

    println!("Avg: {:.2} µs", avg_us);
    println!("Min: {} µs", min_us);
    println!("Max: {} µs", max_us);

    assert!(avg_us < 100.0, "Average latency should be under 100µs");
}

#[test]
fn test_single_insert_warm_latency() {
    println!("\n=== Warm Book Insert Latency (1K orders) ===");

    let engine = HighPerformanceMatchingEngine::new(100, 10000);

    // Populate with 1000 resting orders
    for i in 0..1000 {
        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
        let price = if matches!(side, Side::Buy) {
            50000 - (i as i64 % 100)
        } else {
            50100 + (i as i64 % 100)
        };
        engine.submit_order(make_order(i, side, price, 10)).unwrap();
    }
    engine.process_batch(); // Process initial orders

    let mut latencies = Vec::new();

    for i in 0..100 {
        let order = make_order(1000 + i, Side::Buy, 50000, 100);

        engine.submit_order(order).unwrap();
        let result = engine.process_batch();

        latencies.push(result.processing_time_us as u128);
    }

    let avg_us = latencies.iter().sum::<u128>() as f64 / latencies.len() as f64;
    let min_us = latencies.iter().min().unwrap();
    let max_us = latencies.iter().max().unwrap();

    println!("Avg: {:.2} µs", avg_us);
    println!("Min: {} µs", min_us);
    println!("Max: {} µs", max_us);

    assert!(avg_us < 100.0, "Average latency should be under 100µs");
}

#[test]
fn test_market_order_match_latency() {
    println!("\n=== Market Order Match Latency ===");

    let engine = HighPerformanceMatchingEngine::new(100, 10000);

    // Place resting sell orders
    for i in 0..100 {
        let price = 50000 + (i as i64 * 10);
        engine
            .submit_order(make_order(i, Side::Sell, price, 10))
            .unwrap();
    }
    engine.process_batch(); // Process resting orders

    let mut latencies = Vec::new();

    for i in 0..10 {
        let market_buy = Order {
            id: format!("market_taker_{i}"),
            user_id: "taker".into(),
            market_id: "BTC-USD".into(),
            side: Side::Buy,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Gtc,
            price: 0,
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

        engine.submit_order(market_buy).unwrap();
        let result = engine.process_batch();

        latencies.push(result.processing_time_us as u128);
        println!(
            "Match {}: {} fills, latency={} µs",
            i,
            result.fills.len(),
            result.processing_time_us
        );
    }

    let avg_us = latencies.iter().sum::<u128>() as f64 / latencies.len() as f64;
    println!("\nAvg match latency: {:.2} µs", avg_us);

    assert!(
        avg_us < 200.0,
        "Market order match should complete under 200µs"
    );
}

#[test]
fn test_load_spike_latency_degradation() {
    println!("\n=== Load Spike Latency Degradation Test ===");

    let engine = HighPerformanceMatchingEngine::new(100, 10000);

    // Baseline: Insert 50 orders and measure latency
    println!("Measuring baseline insertion latency...");
    let mut baseline_latencies = Vec::new();
    for i in 0..50 {
        let order = make_order(i, Side::Buy, 50000 - (i as i64 % 10), 10);
        engine.submit_order(order).unwrap();
        let result = engine.process_batch();
        baseline_latencies.push(result.processing_time_us as u128);
    }

    let baseline_avg_us =
        baseline_latencies.iter().sum::<u128>() as f64 / baseline_latencies.len() as f64;
    println!("Baseline avg: {:.2} µs per order", baseline_avg_us);

    // Spike: Rapidly insert 1000 orders
    println!("\nInjecting load spike (1000 orders)...");
    let mut spike_latencies = Vec::new();
    let spike_start = Instant::now();

    for i in 0..1000 {
        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
        let price = if matches!(side, Side::Buy) {
            50000 - (i as i64 % 100)
        } else {
            50100 + (i as i64 % 100)
        };
        let order = make_order(1000 + i, side, price, 10);
        engine.submit_order(order).unwrap();
        let result = engine.process_batch();
        spike_latencies.push(result.processing_time_us as u128);
    }

    let spike_duration = spike_start.elapsed();
    let spike_avg_us = spike_latencies.iter().sum::<u128>() as f64 / spike_latencies.len() as f64;
    let spike_max_us = spike_latencies.iter().max().unwrap();

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
        .filter(|&&d| d as f64 > baseline_avg_us * threshold_multiplier)
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

#[test]
fn test_volatility_stress_handling() {
    println!("\n=== Volatility Stress Handling Test ===");

    let engine = HighPerformanceMatchingEngine::new(100, 10000);
    let base_price = 50000;

    println!("Simulating extreme volatility (±10% price swings)...");

    let mut trade_prices = Vec::new();
    let mut latencies = Vec::new();

    for i in 0..50 {
        // Alternate between aggressive buys and sells
        let direction = if i % 2 == 0 { 1 } else { -1 };
        let volatility_pct = 10; // 10% swing
        let price_offset = (base_price as f64 * volatility_pct as f64 / 100.0) as i64 * direction;
        let price = base_price + price_offset;

        let side = if direction > 0 { Side::Buy } else { Side::Sell };

        let order = make_order(i, side, price, 100);
        engine.submit_order(order).unwrap();
        let result = engine.process_batch();

        if !result.fills.is_empty() {
            for fill in &result.fills {
                trade_prices.push(fill.price);
            }
        }

        latencies.push(result.processing_time_us as u128);

        if i % 10 == 0 {
            println!(
                "Trade {}: price={}, latency={} µs, fills={}",
                i,
                price,
                result.processing_time_us,
                result.fills.len()
            );
        }
    }

    let avg_latency_us = latencies.iter().sum::<u128>() as f64 / latencies.len() as f64;
    let max_latency_us = latencies.iter().max().unwrap();

    println!("\nVolatility stress results:");
    println!("  Avg latency: {:.2} µs", avg_latency_us);
    println!("  Max latency: {} µs", max_latency_us);
    println!("  Total trades executed: {}", trade_prices.len());

    if !trade_prices.is_empty() {
        let price_range = trade_prices.iter().max().unwrap() - trade_prices.iter().min().unwrap();
        println!(
            "  Price range: {} ({}%)",
            price_range,
            (price_range as f64 / base_price as f64 * 100.0) as i64
        );
    }
}
