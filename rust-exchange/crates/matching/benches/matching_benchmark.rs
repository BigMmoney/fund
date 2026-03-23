//! Criterion benchmarks for matching engine throughput and latency.
//!
//! Run with: `cargo bench --package matching`

use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use matching::high_performance::OrderBook;
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

fn bench_order_insert(c: &mut Criterion) {
    c.bench_function("orderbook_insert_1000", |b| {
        b.iter_batched(
            || {
                let book = OrderBook::new();
                let orders: Vec<Order> = (0..1000)
                    .map(|i| {
                        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                        let price = if matches!(side, Side::Buy) {
                            50000 - (i as i64 % 100)
                        } else {
                            50100 + (i as i64 % 100)
                        };
                        make_order(i, side, price, 100)
                    })
                    .collect();
                (book, orders)
            },
            |(mut book, orders)| {
                for order in orders {
                    book.add_order(order);
                    black_box(());
                }
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_best_price_lookup(c: &mut Criterion) {
    c.bench_function("orderbook_best_bid_ask_10k_orders", |b| {
        b.iter_batched(
            || {
                let mut book = OrderBook::new();
                for i in 0..10_000u64 {
                    let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                    let price = if matches!(side, Side::Buy) {
                        50000 - (i as i64 % 500)
                    } else {
                        50100 + (i as i64 % 500)
                    };
                    book.add_order(make_order(i, side, price, 10));
                }
                book
            },
            |book| {
                for _ in 0..1000 {
                    black_box(book.best_bid());
                    black_box(book.best_ask());
                }
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_order_insert, bench_best_price_lookup);
criterion_main!(benches);
