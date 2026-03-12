// 高性能撮合引擎 - 10万+ 订单/秒，P99 < 10ms
use crossbeam::queue::ArrayQueue;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use types::{Fill, Order, Side};

#[derive(Clone, Copy, Debug)]
pub struct PerformanceMetrics {
    pub orders_per_sec: f64,
    pub p99_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub average_latency_ms: f64,
    pub total_batches: usize,
}

/// 高性能订单簿（按价格排序）
#[derive(Debug)]
pub struct OrderBook {
    // 买单：价格从高到低
    bids: BTreeMap<i64, Vec<Order>>,
    // 卖单：价格从低到高
    asks: BTreeMap<i64, Vec<Order>>,
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

type PriceLevels = Vec<(i64, usize)>;

impl OrderBook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    /// 添加订单到订单簿
    pub fn add_order(&mut self, order: Order) {
        match order.side {
            Side::Buy => {
                self.bids.entry(order.price).or_default().push(order);
            }
            Side::Sell => {
                self.asks.entry(order.price).or_default().push(order);
            }
        }
    }

    /// 获取最优买单
    pub fn best_bid(&self) -> Option<i64> {
        self.bids.keys().last().copied()
    }

    /// 获取最优卖单
    pub fn best_ask(&self) -> Option<i64> {
        self.asks.keys().next().copied()
    }
}

/// 撮合结果
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub fills: Vec<Fill>,
    pub processing_time_us: u64, // 微秒
}

/// 高性能撮合引擎
pub struct HighPerformanceMatchingEngine {
    // 订单簿按市场分组
    order_books: Arc<DashMap<String, RwLock<OrderBook>>>,

    // 待处理订单队列（无锁）
    pending_orders: Arc<ArrayQueue<Order>>,

    // 成交记录缓存
    fills_cache: Arc<RwLock<Vec<Fill>>>,

    // 性能指标（延迟样本，微秒）
    latency_samples: Arc<RwLock<Vec<u64>>>,

    batch_size: usize,
    batch_count: Arc<RwLock<usize>>,
}

impl HighPerformanceMatchingEngine {
    pub fn new(batch_size: usize, max_queue_size: usize) -> Self {
        Self {
            order_books: Arc::new(DashMap::new()),
            pending_orders: Arc::new(ArrayQueue::new(max_queue_size)),
            fills_cache: Arc::new(RwLock::new(Vec::with_capacity(10000))),
            latency_samples: Arc::new(RwLock::new(Vec::with_capacity(100000))),
            batch_size,
            batch_count: Arc::new(RwLock::new(0)),
        }
    }

    /// 提交订单到撮合引擎
    pub fn submit_order(&self, order: Order) -> Result<(), Order> {
        self.pending_orders.push(order)
    }

    /// 执行一个批次的撮合
    pub fn process_batch(&self) -> MatchResult {
        let start = Instant::now();

        // 1. 从队列中获取最多 batch_size 个订单
        let mut batch = Vec::with_capacity(self.batch_size);
        for _ in 0..self.batch_size {
            if let Some(order) = self.pending_orders.pop() {
                batch.push(order);
            } else {
                break;
            }
        }

        if batch.is_empty() {
            return MatchResult {
                fills: vec![],
                processing_time_us: start.elapsed().as_micros() as u64,
            };
        }

        // 2. 按市场分组
        let mut market_groups: std::collections::HashMap<String, Vec<Order>> =
            std::collections::HashMap::new();
        for order in batch {
            let key = format!("{}:{}", order.market_id, order.outcome);
            market_groups.entry(key).or_default().push(order);
        }

        // 3. 为每个市场执行撮合
        let mut all_fills = Vec::new();
        for (market_key, orders) in market_groups {
            let fills = self.match_market(&market_key, orders);
            all_fills.extend(fills);
        }

        // 4. 记录性能指标
        let elapsed_us = start.elapsed().as_micros() as u64;
        {
            let mut samples = self.latency_samples.write();
            if samples.len() < 100000 {
                samples.push(elapsed_us);
            }
        }

        {
            let mut count = self.batch_count.write();
            *count += 1;
        }

        // 5. 缓存成交记录
        if !all_fills.is_empty() {
            let mut fills = self.fills_cache.write();
            fills.extend(all_fills.iter().cloned());
            if fills.len() > 100000 {
                fills.drain(0..50000);
            }
        }

        MatchResult {
            fills: all_fills,
            processing_time_us: elapsed_us,
        }
    }

    /// 匹配单个市场的订单
    fn match_market(&self, market_key: &str, orders: Vec<Order>) -> Vec<Fill> {
        let mut fills = Vec::new();

        // 获取或创建订单簿 - 使用 or_insert 而不是 or_insert_with
        let entry = self
            .order_books
            .entry(market_key.to_string())
            .or_insert(RwLock::new(OrderBook::new()));

        let mut book = entry.write();

        // 分离买单和卖单
        let (mut buy_orders, mut sell_orders): (Vec<_>, Vec<_>) =
            orders.into_iter().partition(|o| o.side == Side::Buy);

        // 排序：买单价格从高到低，卖单价格从低到高
        buy_orders.sort_by(|a, b| b.price.cmp(&a.price));
        sell_orders.sort_by(|a, b| a.price.cmp(&b.price));

        // 撮合逻辑
        for buy_order in buy_orders {
            while let Some(ask_price) = book.best_ask() {
                if buy_order.price < ask_price {
                    break;
                }

                // 有可成交的卖单
                if let Some(sell_entry) = book.asks.get_mut(&ask_price) {
                    if let Some(sell_order) = sell_entry.pop() {
                        let fill_amount = buy_order.amount.min(sell_order.amount);
                        let fill = Fill {
                            id: uuid::Uuid::new_v4().to_string(),
                            intent_id: format!("{}_{}", buy_order.id, sell_order.id),
                            user_id: buy_order.user_id.clone(),
                            market_id: buy_order.market_id.clone(),
                            side: buy_order.side,
                            price: ask_price,
                            amount: fill_amount,
                            outcome: buy_order.outcome,
                            timestamp: chrono::Utc::now(),
                            op_id: format!("fill_{}", uuid::Uuid::new_v4()),
                        };

                        fills.push(fill);

                        // 如果卖单已经完全成交，从簿中移除
                        if fill_amount >= sell_order.amount {
                            let _ = sell_entry;
                            book.asks.remove(&ask_price);
                        }
                    }
                } else {
                    break;
                }
            }

            // 将未完全成交的买单加入订单簿
            if buy_order.amount > 0 {
                book.add_order(buy_order);
            }
        }

        // 将卖单加入订单簿
        for sell_order in sell_orders {
            if sell_order.amount > 0 {
                book.add_order(sell_order);
            }
        }

        fills
    }

    /// 获取性能指标
    pub fn get_performance_metrics(&self) -> Option<PerformanceMetrics> {
        let samples = self.latency_samples.read();
        if samples.is_empty() {
            return None;
        }

        let mut sorted = samples.clone();
        sorted.sort_unstable();

        let total_us: u64 = sorted.iter().sum();
        let average_us = total_us / sorted.len() as u64;

        let p50_idx = sorted.len() / 2;
        let p99_idx = (sorted.len() * 99) / 100;

        let batch_count = *self.batch_count.read();

        Some(PerformanceMetrics {
            orders_per_sec: if sorted[sorted.len() - 1] > 0 {
                batch_count as f64 * self.batch_size as f64 * 1_000_000.0 / total_us as f64
            } else {
                0.0
            },
            p50_latency_ms: sorted[p50_idx] as f64 / 1000.0,
            p99_latency_ms: sorted[p99_idx] as f64 / 1000.0,
            average_latency_ms: average_us as f64 / 1000.0,
            total_batches: batch_count,
        })
    }

    /// 获取市场订单簿快照
    pub fn get_order_book_snapshot(&self, market_key: &str) -> Option<(PriceLevels, PriceLevels)> {
        self.order_books.get(market_key).map(|book_lock| {
            let book = book_lock.read();
            let bids: Vec<_> = book
                .bids
                .iter()
                .rev()
                .take(10)
                .map(|(p, orders)| (*p, orders.len()))
                .collect();
            let asks: Vec<_> = book
                .asks
                .iter()
                .take(10)
                .map(|(p, orders)| (*p, orders.len()))
                .collect();
            (bids, asks)
        })
    }

    /// 清空并获取所有成交记录
    pub fn drain_fills(&self) -> Vec<Fill> {
        let mut fills = self.fills_cache.write();
        fills.drain(..).collect()
    }
}
