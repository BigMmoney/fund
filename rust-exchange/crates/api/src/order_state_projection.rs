use super::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OrderProjectionStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Replaced,
    Rejected,
    ClosedNoFill,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct OrderStateProjectionEntry {
    pub(crate) user_id: String,
    pub(crate) order_id: String,
    pub(crate) request_id: String,
    #[serde(default)]
    pub(crate) command_seq: Option<u64>,
    pub(crate) market_id: String,
    pub(crate) outcome: i32,
    pub(crate) side: Side,
    pub(crate) order_type: OrderType,
    pub(crate) time_in_force: TimeInForce,
    pub(crate) price: Option<i64>,
    pub(crate) original_amount: i64,
    #[serde(default)]
    pub(crate) remaining_amount: i64,
    #[serde(default)]
    pub(crate) filled_amount: i64,
    #[serde(default)]
    pub(crate) leverage: Option<u32>,
    #[serde(default)]
    pub(crate) post_only: bool,
    #[serde(default)]
    pub(crate) reduce_only: bool,
    pub(crate) status: OrderProjectionStatus,
    #[serde(default)]
    pub(crate) replaces_order_id: Option<String>,
    #[serde(default)]
    pub(crate) replaced_by_order_id: Option<String>,
    #[serde(default)]
    pub(crate) close_reason: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

pub(crate) struct OrderStateProjectionStore {
    entries: DashMap<String, OrderStateProjectionEntry>,
    store: Arc<dyn persistence::WalStore<OrderStateProjectionEntry>>,
    write_locks: Vec<Mutex<()>>,
    /// Observer-only sink. When `Some`, emits `projection_updated` after
    /// every successful upsert (i.e. after the WAL append commits). See
    /// `docs/MONITOR_DESIGN.md` §3.1.
    trace_emitter: Option<Arc<dyn types::TraceEmitter>>,
}

const ORDER_PROJECTION_LOCK_SHARDS: usize = 64;

impl OrderStateProjectionStore {
    pub(crate) fn new(
        store: Arc<dyn persistence::WalStore<OrderStateProjectionEntry>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            entries: DashMap::new(),
            store,
            write_locks: (0..ORDER_PROJECTION_LOCK_SHARDS)
                .map(|_| Mutex::new(()))
                .collect(),
            trace_emitter: None,
        };
        for entry in result.store.entries()? {
            result
                .entries
                .insert(order_projection_key(&entry.user_id, &entry.order_id), entry);
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<OrderStateProjectionEntry>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    /// Builder-style attachment of a `TraceEmitter`. Observer-only —
    /// failures emit nothing, no behavioural impact when None.
    pub(crate) fn with_trace_emitter(
        mut self,
        emitter: Arc<dyn types::TraceEmitter>,
    ) -> Self {
        self.trace_emitter = Some(emitter);
        self
    }

    pub(crate) fn get(&self, user_id: &str, order_id: &str) -> Option<OrderStateProjectionEntry> {
        self.entries
            .get(&order_projection_key(user_id, order_id))
            .map(|entry| entry.value().clone())
    }

    pub(crate) fn latest_command_seq(&self) -> Option<u64> {
        self.entries
            .iter()
            .filter_map(|entry| entry.value().command_seq)
            .max()
    }

    pub(crate) fn list_all(&self) -> Vec<OrderStateProjectionEntry> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|lhs, rhs| {
            lhs.command_seq
                .cmp(&rhs.command_seq)
                .then_with(|| lhs.order_id.cmp(&rhs.order_id))
        });
        items
    }

    pub(crate) fn active_order_count_for_user(
        &self,
        user_id: &str,
        exclude_order_id: Option<&str>,
    ) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                let value = entry.value();
                value.user_id == user_id
                    && exclude_order_id.is_none_or(|order_id| value.order_id != order_id)
                    && matches!(
                        value.status,
                        OrderProjectionStatus::Open | OrderProjectionStatus::PartiallyFilled
                    )
            })
            .count()
    }

    pub(crate) fn sync_from_sources(
        &self,
        records: &[SequencedCommandRecord],
        trades: &[TradeJournalRecord],
        snapshots: &[MarketRuntimeSnapshot],
    ) -> anyhow::Result<()> {
        let mut materialized: HashMap<String, OrderStateProjectionEntry> = self
            .list_all()
            .into_iter()
            .map(|entry| (order_projection_key(&entry.user_id, &entry.order_id), entry))
            .collect();

        let mut ordered_records = records.to_vec();
        ordered_records.sort_by_key(|record| record.command_seq);
        for record in ordered_records {
            match record.command {
                Command::NewOrder(command) => {
                    let entry = materialized
                        .entry(order_projection_key(
                            &command.user_id,
                            &command.client_order_id,
                        ))
                        .or_insert_with(|| projection_from_new_order(&command));
                    entry.request_id = command.metadata.request_id.clone();
                    entry.command_seq = command.metadata.command_seq;
                    entry.updated_at = command.metadata.updated_at;
                    if command.metadata.lifecycle == types::CommandLifecycle::Rejected {
                        entry.status = OrderProjectionStatus::Rejected;
                        entry.close_reason = Some("sequencer_rejected".to_string());
                    }
                }
                Command::CancelOrder(command) => {
                    if let Some(entry) = materialized
                        .get_mut(&order_projection_key(&command.user_id, &command.order_id))
                    {
                        entry.status = OrderProjectionStatus::Cancelled;
                        entry.close_reason = Some("cancel_order".to_string());
                        entry.updated_at = command.metadata.updated_at;
                    }
                }
                Command::ReplaceOrder(command) => {
                    let old_key = order_projection_key(&command.user_id, &command.order_id);
                    let mut previous_for_replace = None;
                    if let Some(entry) = materialized.get_mut(&old_key) {
                        entry.status = OrderProjectionStatus::Replaced;
                        entry.updated_at = command.metadata.updated_at;
                        entry.close_reason = Some("replace_order".to_string());
                        previous_for_replace = Some(entry.clone());
                    }
                    if let (Some(new_order_id), Some(previous)) =
                        (&command.new_client_order_id, previous_for_replace)
                    {
                        if let Some(entry) = materialized.get_mut(&old_key) {
                            entry.replaced_by_order_id = Some(new_order_id.clone());
                        }
                        materialized
                            .entry(order_projection_key(&command.user_id, new_order_id))
                            .or_insert_with(|| {
                                projection_from_replace(&command, &previous, new_order_id.clone())
                            });
                    }
                }
                _ => {}
            }
        }

        for snapshot in snapshots {
            for order in &snapshot.orders {
                let key = order_projection_key(&order.user_id, &order.order_id);
                let entry = materialized
                    .entry(key)
                    .or_insert_with(|| projection_from_snapshot(order));
                entry.market_id = order.market_id.clone();
                entry.outcome = order.outcome;
                entry.side = order.side;
                entry.order_type = order.order_type;
                entry.time_in_force = order.time_in_force;
                entry.price = Some(order.price);
                entry.original_amount = order.original_amount;
                entry.remaining_amount = order.remaining_amount;
                entry.filled_amount = order.original_amount.saturating_sub(order.remaining_amount);
                entry.leverage = order.leverage;
                entry.post_only = order.post_only;
                entry.reduce_only = order.reduce_only;
                entry.status = if entry.filled_amount > 0 {
                    OrderProjectionStatus::PartiallyFilled
                } else {
                    OrderProjectionStatus::Open
                };
                entry.updated_at = Utc::now();
            }
        }

        let mut fills_by_order: HashMap<(String, String), (i64, DateTime<Utc>)> = HashMap::new();
        for trade in trades {
            let buy_key = (trade.buy_user_id.clone(), trade.buy_order_id.clone());
            let sell_key = (trade.sell_user_id.clone(), trade.sell_order_id.clone());
            accumulate_fill(
                &mut fills_by_order,
                buy_key,
                trade.amount,
                trade.recorded_at,
            );
            accumulate_fill(
                &mut fills_by_order,
                sell_key,
                trade.amount,
                trade.recorded_at,
            );
        }

        for ((user_id, order_id), (filled_amount, updated_at)) in fills_by_order {
            let key = order_projection_key(&user_id, &order_id);
            let entry = materialized
                .entry(key)
                .or_insert_with(|| OrderStateProjectionEntry {
                    user_id: user_id.clone(),
                    order_id: order_id.clone(),
                    request_id: String::new(),
                    command_seq: parse_command_seq_from_order_like_id(&order_id),
                    market_id: String::new(),
                    outcome: 0,
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price: None,
                    original_amount: filled_amount,
                    remaining_amount: 0,
                    filled_amount,
                    leverage: None,
                    post_only: false,
                    reduce_only: false,
                    status: OrderProjectionStatus::Filled,
                    replaces_order_id: None,
                    replaced_by_order_id: None,
                    close_reason: Some("trade_log_only".to_string()),
                    created_at: updated_at,
                    updated_at,
                });
            entry.filled_amount = entry.filled_amount.max(filled_amount);
            if entry.original_amount < entry.filled_amount {
                entry.original_amount = entry.filled_amount;
            }
            entry.remaining_amount = entry.original_amount.saturating_sub(entry.filled_amount);
            if entry.remaining_amount == 0 {
                entry.status = OrderProjectionStatus::Filled;
            } else {
                entry.status = OrderProjectionStatus::PartiallyFilled;
            }
            entry.updated_at = updated_at;
        }

        for entry in materialized.values() {
            self.upsert(entry.clone())?;
        }
        Ok(())
    }

    pub(crate) fn record_submit_success(
        &self,
        command: &NewOrderCommand,
        result: &matching::SubmitOrderResult,
        replaces_order_id: Option<String>,
    ) -> anyhow::Result<()> {
        let filled_amount = command.amount.saturating_sub(result.remaining_amount);
        self.upsert(OrderStateProjectionEntry {
            user_id: command.user_id.clone(),
            order_id: result.order_id.clone(),
            request_id: command.metadata.request_id.clone(),
            command_seq: command.metadata.command_seq,
            market_id: command.market_id.clone(),
            outcome: command.outcome,
            side: command.side,
            order_type: command.order_type,
            time_in_force: command.time_in_force,
            price: command.price,
            original_amount: command.amount,
            remaining_amount: result.remaining_amount,
            filled_amount,
            leverage: command.leverage,
            post_only: command.post_only,
            reduce_only: command.reduce_only,
            status: projection_status_from_submit(result, filled_amount),
            replaces_order_id,
            replaced_by_order_id: None,
            close_reason: projection_close_reason_from_submit(result, filled_amount),
            created_at: command.metadata.received_at,
            updated_at: result.metadata.updated_at,
        })
    }

    pub(crate) fn record_new_order_rejection(
        &self,
        command: &NewOrderCommand,
    ) -> anyhow::Result<()> {
        self.upsert(OrderStateProjectionEntry {
            user_id: command.user_id.clone(),
            order_id: command.client_order_id.clone(),
            request_id: command.metadata.request_id.clone(),
            command_seq: command.metadata.command_seq,
            market_id: command.market_id.clone(),
            outcome: command.outcome,
            side: command.side,
            order_type: command.order_type,
            time_in_force: command.time_in_force,
            price: command.price,
            original_amount: command.amount,
            remaining_amount: command.amount,
            filled_amount: 0,
            leverage: command.leverage,
            post_only: command.post_only,
            reduce_only: command.reduce_only,
            status: OrderProjectionStatus::Rejected,
            replaces_order_id: None,
            replaced_by_order_id: None,
            close_reason: Some("submission_rejected".to_string()),
            created_at: command.metadata.received_at,
            updated_at: Utc::now(),
        })
    }

    pub(crate) fn record_replace_success(
        &self,
        command: &ReplaceOrderCommand,
        result: &matching::SubmitOrderResult,
    ) -> anyhow::Result<()> {
        let old_key = order_projection_key(&command.user_id, &command.order_id);
        let existing = self
            .entries
            .get(&old_key)
            .map(|entry| entry.value().clone());
        if let Some(mut previous) = existing.clone() {
            previous.status = OrderProjectionStatus::Replaced;
            previous.replaced_by_order_id = Some(result.order_id.clone());
            previous.close_reason = Some("replace_order".to_string());
            previous.updated_at = result.metadata.updated_at;
            self.upsert(previous)?;
        }
        let base = existing.unwrap_or_else(|| OrderStateProjectionEntry {
            user_id: command.user_id.clone(),
            order_id: command.order_id.clone(),
            request_id: command.metadata.request_id.clone(),
            command_seq: command.metadata.command_seq,
            market_id: command.market_id.clone(),
            outcome: command.outcome.unwrap_or_default(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: None,
            original_amount: command.new_amount.unwrap_or_default(),
            remaining_amount: command.new_amount.unwrap_or_default(),
            filled_amount: 0,
            leverage: command.new_leverage,
            post_only: command.post_only.unwrap_or(false),
            reduce_only: command.reduce_only.unwrap_or(false),
            status: OrderProjectionStatus::Replaced,
            replaces_order_id: None,
            replaced_by_order_id: None,
            close_reason: None,
            created_at: command.metadata.received_at,
            updated_at: command.metadata.updated_at,
        });
        let replacement = NewOrderCommand {
            metadata: command.metadata.clone(),
            client_order_id: result.order_id.clone(),
            user_id: command.user_id.clone(),
            session_id: None,
            market_id: base.market_id.clone(),
            side: base.side,
            order_type: base.order_type,
            time_in_force: command.new_time_in_force.unwrap_or(base.time_in_force),
            price: command.new_price.or(base.price),
            amount: command
                .new_amount
                .unwrap_or(base.remaining_amount.max(base.original_amount)),
            outcome: base.outcome,
            post_only: command.post_only.unwrap_or(base.post_only),
            reduce_only: command.reduce_only.unwrap_or(base.reduce_only),
            leverage: command.new_leverage.or(base.leverage),
            expires_at: command.new_expires_at,
            stp_mode: types::StpMode::default(),
            trigger_price: None,
            trigger_type: None,
            display_qty: command.new_display_qty,
            min_fill_qty: command.new_min_fill_qty,
            stp_group_id: None,
            is_market_maker: false,
        };
        self.record_submit_success(&replacement, result, Some(command.order_id.clone()))
    }

    pub(crate) fn record_replace_rejection(
        &self,
        command: &ReplaceOrderCommand,
    ) -> anyhow::Result<()> {
        if let Some(mut entry) = self.get(&command.user_id, &command.order_id) {
            entry.updated_at = Utc::now();
            entry.close_reason = Some("replace_rejected_kept_original".to_string());
            self.upsert(entry)?;
        }
        Ok(())
    }

    pub(crate) fn record_cancelled_orders(
        &self,
        user_id: &str,
        order_ids: &[String],
        command_seq: Option<u64>,
        reason: &str,
    ) -> anyhow::Result<()> {
        for order_id in order_ids {
            if let Some(mut entry) = self.get(user_id, order_id) {
                entry.status = OrderProjectionStatus::Cancelled;
                entry.command_seq = command_seq.or(entry.command_seq);
                entry.close_reason = Some(reason.to_string());
                entry.updated_at = Utc::now();
                self.upsert(entry)?;
            }
        }
        Ok(())
    }

    fn upsert(&self, entry: OrderStateProjectionEntry) -> anyhow::Result<()> {
        let key = order_projection_key(&entry.user_id, &entry.order_id);
        let _guard = self.write_locks[order_projection_lock_shard(&key)].lock();
        self.store.append(&entry)?;
        // Observer: emit projection_updated after the WAL append commits.
        // Done before inserting into the in-memory map so a panic mid-emit
        // (the trait says implementations MUST NOT panic) cannot leave the
        // map in a half-inserted state — but since emit is fire-and-forget
        // and never panics, this ordering is purely defensive.
        if let Some(emitter) = &self.trace_emitter {
            let mut ev = types::OrderTraceEvent::new(
                types::OrderTraceStage::ProjectionUpdated,
                entry.order_id.clone(),
            );
            ev.user_id = Some(entry.user_id.clone());
            ev.request_id = Some(entry.request_id.clone());
            ev.command_seq = entry.command_seq;
            ev.market_id = Some(entry.market_id.clone());
            ev.outcome = Some(entry.outcome);
            ev.side = Some(entry.side);
            ev.price = entry.price;
            ev.amount = Some(entry.original_amount);
            ev.remaining_amount = Some(entry.remaining_amount);
            ev.filled_amount = Some(entry.filled_amount);
            ev.detail = serde_json::json!({ "status": entry.status });
            emitter.emit(ev);
        }
        self.entries.insert(key, entry);
        Ok(())
    }
}

fn accumulate_fill(
    target: &mut HashMap<(String, String), (i64, DateTime<Utc>)>,
    key: (String, String),
    amount: i64,
    recorded_at: DateTime<Utc>,
) {
    let entry = target.entry(key).or_insert((0, recorded_at));
    entry.0 = entry.0.saturating_add(amount);
    entry.1 = entry.1.max(recorded_at);
}

fn order_projection_key(user_id: &str, order_id: &str) -> String {
    format!("{user_id}::{order_id}")
}

fn order_projection_lock_shard(key: &str) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % ORDER_PROJECTION_LOCK_SHARDS
}

fn projection_from_new_order(command: &NewOrderCommand) -> OrderStateProjectionEntry {
    OrderStateProjectionEntry {
        user_id: command.user_id.clone(),
        order_id: command.client_order_id.clone(),
        request_id: command.metadata.request_id.clone(),
        command_seq: command.metadata.command_seq,
        market_id: command.market_id.clone(),
        outcome: command.outcome,
        side: command.side,
        order_type: command.order_type,
        time_in_force: command.time_in_force,
        price: command.price,
        original_amount: command.amount,
        remaining_amount: command.amount,
        filled_amount: 0,
        leverage: command.leverage,
        post_only: command.post_only,
        reduce_only: command.reduce_only,
        status: default_projection_status(command),
        replaces_order_id: None,
        replaced_by_order_id: None,
        close_reason: default_projection_close_reason(command),
        created_at: command.metadata.received_at,
        updated_at: command.metadata.updated_at,
    }
}

fn projection_from_replace(
    command: &ReplaceOrderCommand,
    previous: &OrderStateProjectionEntry,
    new_order_id: String,
) -> OrderStateProjectionEntry {
    OrderStateProjectionEntry {
        user_id: command.user_id.clone(),
        order_id: new_order_id,
        request_id: command.metadata.request_id.clone(),
        command_seq: command.metadata.command_seq,
        market_id: previous.market_id.clone(),
        outcome: command.outcome.unwrap_or(previous.outcome),
        side: previous.side,
        order_type: previous.order_type,
        time_in_force: command.new_time_in_force.unwrap_or(previous.time_in_force),
        price: command.new_price.or(previous.price),
        original_amount: command.new_amount.unwrap_or(previous.remaining_amount),
        remaining_amount: command.new_amount.unwrap_or(previous.remaining_amount),
        filled_amount: 0,
        leverage: command.new_leverage.or(previous.leverage),
        post_only: command.post_only.unwrap_or(previous.post_only),
        reduce_only: command.reduce_only.unwrap_or(previous.reduce_only),
        status: OrderProjectionStatus::Open,
        replaces_order_id: Some(previous.order_id.clone()),
        replaced_by_order_id: None,
        close_reason: None,
        created_at: command.metadata.received_at,
        updated_at: command.metadata.updated_at,
    }
}

fn projection_from_snapshot(order: &RestingOrderSnapshot) -> OrderStateProjectionEntry {
    OrderStateProjectionEntry {
        user_id: order.user_id.clone(),
        order_id: order.order_id.clone(),
        request_id: order.request_id.clone(),
        command_seq: order.command_seq,
        market_id: order.market_id.clone(),
        outcome: order.outcome,
        side: order.side,
        order_type: order.order_type,
        time_in_force: order.time_in_force,
        price: Some(order.price),
        original_amount: order.original_amount,
        remaining_amount: order.remaining_amount,
        filled_amount: order.original_amount.saturating_sub(order.remaining_amount),
        leverage: order.leverage,
        post_only: order.post_only,
        reduce_only: order.reduce_only,
        status: if order.remaining_amount < order.original_amount {
            OrderProjectionStatus::PartiallyFilled
        } else {
            OrderProjectionStatus::Open
        },
        replaces_order_id: None,
        replaced_by_order_id: None,
        close_reason: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn default_projection_status(command: &NewOrderCommand) -> OrderProjectionStatus {
    if command.metadata.lifecycle == types::CommandLifecycle::Rejected {
        OrderProjectionStatus::Rejected
    } else if matches!(command.order_type, OrderType::Market)
        || matches!(
            command.time_in_force,
            TimeInForce::Ioc | TimeInForce::Fok | TimeInForce::Gtd
        )
    {
        OrderProjectionStatus::ClosedNoFill
    } else {
        OrderProjectionStatus::Open
    }
}

fn default_projection_close_reason(command: &NewOrderCommand) -> Option<String> {
    match default_projection_status(command) {
        OrderProjectionStatus::Rejected => Some("sequencer_rejected".to_string()),
        OrderProjectionStatus::ClosedNoFill => Some("not_resting_after_completion".to_string()),
        _ => None,
    }
}

fn projection_status_from_submit(
    result: &matching::SubmitOrderResult,
    filled_amount: i64,
) -> OrderProjectionStatus {
    if result.remaining_amount == 0 && filled_amount > 0 {
        OrderProjectionStatus::Filled
    } else if filled_amount > 0 {
        OrderProjectionStatus::PartiallyFilled
    } else if result.state == types::OrderState::Active {
        OrderProjectionStatus::Open
    } else {
        OrderProjectionStatus::ClosedNoFill
    }
}

fn projection_close_reason_from_submit(
    result: &matching::SubmitOrderResult,
    filled_amount: i64,
) -> Option<String> {
    if result.remaining_amount == 0 && filled_amount == 0 {
        Some("closed_without_fill".to_string())
    } else {
        None
    }
}

pub(crate) fn parse_command_seq_from_order_like_id(value: &str) -> Option<u64> {
    let marker = "seq-";
    let start = value.find(marker)? + marker.len();
    let digits = value[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matching::partitioned::TimingBreakdown;
    use persistence::InMemoryWal;
    use types::{CommandMetadata, NewOrderCommand, StpMode};

    #[test]
    fn records_generated_replacement_order_id() {
        let store = OrderStateProjectionStore::new(Arc::new(InMemoryWal::new())).unwrap();
        let base = NewOrderCommand {
            metadata: CommandMetadata::new("req-1"),
            client_order_id: "old-1".to_string(),
            user_id: "u1".to_string(),
            session_id: None,
            market_id: "btc-usdt".to_string(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: Some(100),
            amount: 10,
            outcome: 0,
            post_only: false,
            reduce_only: false,
            leverage: None,
            expires_at: None,
            stp_mode: StpMode::default(),
            trigger_price: None,
            trigger_type: None,
            display_qty: None,
            min_fill_qty: None,
            stp_group_id: None,
            is_market_maker: false,
        };
        let result = matching::SubmitOrderResult {
            metadata: CommandMetadata::new("req-1"),
            order_id: "old-1".to_string(),
            market_state: MarketState::Normal,
            fills: Vec::new(),
            state: types::OrderState::Active,
            remaining_amount: 10,
            partition: 0,
            queue_wait_us: 0,
            match_execution_us: 0,
            persist_us: 0,
            timing: TimingBreakdown::default(),
        };
        store.record_submit_success(&base, &result, None).unwrap();

        let replace = ReplaceOrderCommand {
            metadata: CommandMetadata::new("req-2"),
            user_id: "u1".to_string(),
            market_id: "btc-usdt".to_string(),
            outcome: Some(0),
            order_id: "old-1".to_string(),
            new_client_order_id: None,
            new_price: Some(101),
            new_amount: Some(8),
            new_time_in_force: None,
            post_only: None,
            reduce_only: None,
            new_leverage: None,
            new_expires_at: None,
            new_display_qty: None,
            new_min_fill_qty: None,
            new_trigger_price: None,
            new_trigger_type: None,
        };
        let replace_result = matching::SubmitOrderResult {
            metadata: CommandMetadata::new("req-2"),
            order_id: "generated-2".to_string(),
            market_state: MarketState::Normal,
            fills: Vec::new(),
            state: types::OrderState::Active,
            remaining_amount: 8,
            partition: 0,
            queue_wait_us: 0,
            match_execution_us: 0,
            persist_us: 0,
            timing: TimingBreakdown::default(),
        };

        store
            .record_replace_success(&replace, &replace_result)
            .unwrap();

        let old_entry = store.get("u1", "old-1").unwrap();
        assert_eq!(old_entry.status, OrderProjectionStatus::Replaced);
        assert_eq!(
            old_entry.replaced_by_order_id.as_deref(),
            Some("generated-2")
        );

        let new_entry = store.get("u1", "generated-2").unwrap();
        assert_eq!(new_entry.replaces_order_id.as_deref(), Some("old-1"));
        assert_eq!(new_entry.price, Some(101));
    }
}
