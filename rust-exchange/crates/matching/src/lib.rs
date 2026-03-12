use anyhow::Result;
use dashmap::DashMap;
use eventbus::EventBus;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use types::{Event, Fill, Intent, IntentStatus, Order, OrderState, Side};

pub mod high_performance;
pub mod partitioned;
pub use high_performance::{HighPerformanceMatchingEngine, PerformanceMetrics};
pub use partitioned::{
    CancelResult, MarketRuntimeSnapshot, MarketSnapshot, PartitionQueueDepth,
    PartitionSnapshotRecord, PartitionStateSnapshot, PartitionedEngineConfig,
    PartitionedMatchingEngine, RestingOrderSnapshot, SubmissionError, SubmitOrderResult,
};

#[derive(Clone)]
pub struct MatchingEngine {
    intents: Arc<DashMap<String, Intent>>,
    batch_window: Duration,
    event_bus: EventBus,
}

impl MatchingEngine {
    pub fn new(batch_window: Duration, event_bus: EventBus) -> Self {
        Self {
            intents: Arc::new(DashMap::new()),
            batch_window,
            event_bus,
        }
    }

    pub async fn start(self) {
        let mut interval = time::interval(self.batch_window);
        tracing::info!(
            "Matching engine started with batch window: {:?}",
            self.batch_window
        );

        loop {
            interval.tick().await;
            self.process_batch();
        }
    }

    pub fn add_intent(&self, intent: Intent) {
        self.intents.insert(intent.id.clone(), intent.clone());
        self.event_bus.publish(Event::IntentReceived(intent));
    }

    pub fn cancel_intent(&self, intent_id: &str) -> Result<()> {
        if let Some(mut intent) = self.intents.get_mut(intent_id) {
            intent.status = IntentStatus::Cancelled;
            self.event_bus
                .publish(Event::IntentCancelled(intent.clone()));
            Ok(())
        } else {
            anyhow::bail!("intent not found: {intent_id}")
        }
    }

    fn process_batch(&self) {
        if self.intents.is_empty() {
            return;
        }

        tracing::debug!("Processing batch with {} intents", self.intents.len());

        let valid_intents = self.collect_valid_intents();
        if valid_intents.is_empty() {
            return;
        }

        let market_groups = self.group_by_market(&valid_intents);

        for (market_key, intents) in market_groups {
            self.process_market_batch(&market_key, intents);
        }
    }

    fn collect_valid_intents(&self) -> Vec<Intent> {
        let now = chrono::Utc::now();
        let mut valid = Vec::new();

        for entry in self.intents.iter() {
            let intent = entry.value().clone();

            if intent.status == IntentStatus::Cancelled || intent.status == IntentStatus::Filled {
                continue;
            }

            if now > intent.expires_at {
                drop(entry);
                if let Some(mut intent_mut) = self.intents.get_mut(&intent.id) {
                    intent_mut.status = IntentStatus::Expired;
                }
                continue;
            }

            valid.push(intent.clone());
        }

        valid
    }

    fn group_by_market(&self, intents: &[Intent]) -> HashMap<String, Vec<Intent>> {
        let mut groups: HashMap<String, Vec<Intent>> = HashMap::new();

        for intent in intents {
            let key = format!("{}:{}", intent.market_id, intent.outcome);
            groups.entry(key).or_default().push(intent.clone());
        }

        groups
    }

    fn process_market_batch(&self, market_key: &str, intents: Vec<Intent>) {
        let (buy_orders, sell_orders) = self.aggregate_orderbook(&intents);

        if buy_orders.is_empty() || sell_orders.is_empty() {
            return;
        }

        let clearing_price = self.compute_clearing_price(&buy_orders, &sell_orders);
        if clearing_price == 0 {
            return;
        }

        tracing::info!("Market {}: clearing price={}", market_key, clearing_price);

        let fills = self.allocate_fills(&buy_orders, &sell_orders, clearing_price);

        if fills.is_empty() {
            return;
        }

        self.emit_fills(&fills);

        for fill in &fills {
            if let Some(mut intent) = self.intents.get_mut(&fill.intent_id) {
                intent.status = IntentStatus::Filled;
            }
        }
    }

    fn aggregate_orderbook(&self, intents: &[Intent]) -> (Vec<Order>, Vec<Order>) {
        let mut buy_orders = Vec::new();
        let mut sell_orders = Vec::new();

        for intent in intents {
            let order = Order {
                id: intent.id.clone(),
                user_id: intent.user_id.clone(),
                market_id: intent.market_id.clone(),
                side: intent.side,
                price: intent.price,
                amount: intent.amount,
                outcome: intent.outcome,
                status: OrderState::PendingNew,
                created_at: intent.created_at,
            };

            match intent.side {
                Side::Buy => buy_orders.push(order),
                Side::Sell => sell_orders.push(order),
            }
        }

        (buy_orders, sell_orders)
    }

    fn compute_clearing_price(&self, buy_orders: &[Order], sell_orders: &[Order]) -> i64 {
        let demand_curve = self.build_demand_curve(buy_orders);
        let supply_curve = self.build_supply_curve(sell_orders);

        let mut best_price = -1i64;
        let mut max_volume = 0i64;

        let price_points = self.get_all_price_points(buy_orders, sell_orders);

        for price in price_points {
            let demand = demand_curve.get(&price).copied().unwrap_or(0);
            let supply = supply_curve.get(&price).copied().unwrap_or(0);
            let volume = demand.min(supply);

            if volume > max_volume
                || (volume == max_volume && (best_price == -1 || price < best_price))
            {
                max_volume = volume;
                best_price = price;
            }
        }

        if best_price < 0 {
            0
        } else {
            best_price
        }
    }

    fn build_demand_curve(&self, buy_orders: &[Order]) -> HashMap<i64, i64> {
        let mut curve = HashMap::new();
        for order in buy_orders {
            for price in order.price..=100 {
                *curve.entry(price).or_insert(0) += order.amount;
            }
        }
        curve
    }

    fn build_supply_curve(&self, sell_orders: &[Order]) -> HashMap<i64, i64> {
        let mut curve = HashMap::new();
        for order in sell_orders {
            for price in 0..=order.price {
                *curve.entry(price).or_insert(0) += order.amount;
            }
        }
        curve
    }

    fn get_all_price_points(&self, buy_orders: &[Order], sell_orders: &[Order]) -> Vec<i64> {
        let mut prices = std::collections::HashSet::new();
        for order in buy_orders {
            prices.insert(order.price);
        }
        for order in sell_orders {
            prices.insert(order.price);
        }
        let mut price_vec: Vec<i64> = prices.into_iter().collect();
        price_vec.sort_unstable();
        price_vec
    }

    fn allocate_fills(
        &self,
        buy_orders: &[Order],
        sell_orders: &[Order],
        clearing_price: i64,
    ) -> Vec<Fill> {
        let mut fills = Vec::new();

        let eligible_buys: Vec<&Order> = buy_orders
            .iter()
            .filter(|o| o.price >= clearing_price)
            .collect();
        let eligible_sells: Vec<&Order> = sell_orders
            .iter()
            .filter(|o| o.price <= clearing_price)
            .collect();

        let total_buy_amount: i64 = eligible_buys.iter().map(|o| o.amount).sum();
        let total_sell_amount: i64 = eligible_sells.iter().map(|o| o.amount).sum();
        let matched_volume = total_buy_amount.min(total_sell_amount);

        if matched_volume == 0 {
            return fills;
        }

        let buy_alloc = self.allocate_pro_rata(&eligible_buys, matched_volume, total_buy_amount);
        let sell_alloc = self.allocate_pro_rata(&eligible_sells, matched_volume, total_sell_amount);

        for order in eligible_buys {
            if let Some(&fill_amount) = buy_alloc.get(&order.id) {
                if fill_amount > 0 {
                    fills.push(Fill {
                        id: types::generate_id(),
                        intent_id: order.id.clone(),
                        user_id: order.user_id.clone(),
                        market_id: order.market_id.clone(),
                        side: Side::Buy,
                        price: clearing_price,
                        amount: fill_amount,
                        outcome: order.outcome,
                        timestamp: chrono::Utc::now(),
                        op_id: types::generate_op_id("fill"),
                    });
                }
            }
        }

        for order in eligible_sells {
            if let Some(&fill_amount) = sell_alloc.get(&order.id) {
                if fill_amount > 0 {
                    fills.push(Fill {
                        id: types::generate_id(),
                        intent_id: order.id.clone(),
                        user_id: order.user_id.clone(),
                        market_id: order.market_id.clone(),
                        side: Side::Sell,
                        price: clearing_price,
                        amount: fill_amount,
                        outcome: order.outcome,
                        timestamp: chrono::Utc::now(),
                        op_id: types::generate_op_id("fill"),
                    });
                }
            }
        }

        fills
    }

    fn allocate_pro_rata(
        &self,
        orders: &[&Order],
        matched_volume: i64,
        total_amount: i64,
    ) -> HashMap<String, i64> {
        let mut allocation = HashMap::new();
        if orders.is_empty() || matched_volume <= 0 || total_amount <= 0 {
            return allocation;
        }

        let mut allocated = 0i64;
        let mut remainders = Vec::new();

        for order in orders {
            let numerator = order.amount * matched_volume;
            let base = numerator / total_amount;
            let rem = numerator % total_amount;
            let base = base.min(order.amount);

            allocation.insert(order.id.clone(), base);
            allocated += base;
            remainders.push((order.id.clone(), order.amount, rem));
        }

        let mut remaining = matched_volume - allocated;
        if remaining <= 0 {
            return allocation;
        }

        remainders.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.0.cmp(&b.0))
        });

        for (order_id, order_amount, _) in remainders {
            if remaining == 0 {
                break;
            }
            let current = allocation.get(&order_id).copied().unwrap_or(0);
            if current < order_amount {
                allocation.insert(order_id, current + 1);
                remaining -= 1;
            }
        }

        allocation
    }

    fn emit_fills(&self, fills: &[Fill]) {
        for fill in fills {
            self.event_bus.publish(Event::FillCreated(fill.clone()));
            tracing::info!(
                "Fill created: id={}, user={}, side={:?}, price={}, amount={}",
                fill.id,
                fill.user_id,
                fill.side,
                fill.price,
                fill.amount
            );
        }
    }
}
