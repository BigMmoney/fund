use super::*;
use std::hash::{Hash, Hasher};

fn is_missing_storage_error(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_lowercase();
    message.contains("os error 2")
        || message.contains("not found")
        || message.contains("no such file")
}

fn default_instrument_kind() -> InstrumentKind {
    InstrumentKind::Spot
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PositionCostLedgerEvent {
    pub(crate) event_id: String,
    pub(crate) trade_id: String,
    pub(crate) user_id: String,
    pub(crate) market_id: String,
    pub(crate) outcome: i32,
    #[serde(default = "default_instrument_kind")]
    pub(crate) instrument_kind: InstrumentKind,
    pub(crate) delta_qty: i64,
    pub(crate) price: i64,
    pub(crate) recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct PositionCostLedgerEntry {
    pub(crate) user_id: String,
    pub(crate) market_id: String,
    pub(crate) outcome: i32,
    #[serde(default = "default_instrument_kind")]
    pub(crate) instrument_kind: InstrumentKind,
    pub(crate) net_qty: i64,
    #[serde(default)]
    pub(crate) open_notional: i64,
    pub(crate) entry_price: Option<i64>,
    #[serde(default)]
    pub(crate) realized_pnl: i64,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Default, Clone, Copy)]
struct PositionCostState {
    net_qty: i64,
    open_notional: i64,
    entry_price: Option<i64>,
    realized_pnl: i64,
}

const POSITION_COST_LOCK_SHARDS: usize = 64;

pub(crate) struct PositionCostLedgerStore {
    entries: DashMap<String, PositionCostLedgerEntry>,
    persisted_events: DashMap<String, ()>,
    applied_state_events: DashMap<String, ()>,
    state_store: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>>,
    event_store: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>>,
    write_locks: Vec<Mutex<()>>,
    last_synced_trade_cursor: Mutex<Option<(DateTime<Utc>, String)>>,
}

impl PositionCostLedgerStore {
    pub(crate) fn new(
        state_store: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>>,
        event_store: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            entries: DashMap::new(),
            persisted_events: DashMap::new(),
            applied_state_events: DashMap::new(),
            state_store,
            event_store,
            write_locks: (0..POSITION_COST_LOCK_SHARDS)
                .map(|_| Mutex::new(()))
                .collect(),
            last_synced_trade_cursor: Mutex::new(None),
        };
        let event_entries = result.event_store.entries()?;
        if event_entries.is_empty() {
            for entry in result.state_store.entries()? {
                result.entries.insert(
                    position_cost_key(&entry.user_id, &entry.market_id, entry.outcome),
                    entry,
                );
            }
        } else {
            for event in event_entries {
                result.replay_event(event);
            }
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(
        state_path: impl Into<std::path::PathBuf>,
        event_path: impl Into<std::path::PathBuf>,
    ) -> anyhow::Result<Self> {
        let state_store: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>> =
            Arc::new(JsonlFileWal::new(state_path)?);
        let event_store: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>> =
            Arc::new(JsonlFileWal::new(event_path)?);
        Self::new(state_store, event_store)
    }

    pub(crate) fn sync_from_trade_journal(
        &self,
        trade_journal_wal: &dyn persistence::WalStore<TradeJournalRecord>,
    ) -> anyhow::Result<()> {
        let mut trades = match trade_journal_wal.entries() {
            Ok(entries) => entries,
            Err(error) if is_missing_storage_error(&error) => return Ok(()),
            Err(error) => return Err(error),
        };
        trades.sort_by(|lhs, rhs| {
            lhs.recorded_at
                .cmp(&rhs.recorded_at)
                .then_with(|| lhs.trade_id.cmp(&rhs.trade_id))
        });
        let cursor = self.last_synced_trade_cursor.lock().clone();
        for trade in trades.iter() {
            let trade_key = (trade.recorded_at, trade.trade_id.clone());
            let before_or_at_cursor = cursor
                .as_ref()
                .is_some_and(|cursor_key| trade_key <= *cursor_key);
            if before_or_at_cursor && self.has_fully_applied_trade(&trade.trade_id) {
                continue;
            }
            self.apply_trade(trade)?;
        }
        if let Some(last) = trades.last() {
            *self.last_synced_trade_cursor.lock() = Some((last.recorded_at, last.trade_id.clone()));
        }
        Ok(())
    }

    pub(crate) fn get(
        &self,
        user_id: &str,
        market_id: &str,
        outcome: i32,
    ) -> Option<PositionCostLedgerEntry> {
        self.entries
            .get(&position_cost_key(user_id, market_id, outcome))
            .map(|entry| entry.value().clone())
    }

    pub(crate) fn list_for_user(&self, user_id: &str) -> Vec<PositionCostLedgerEntry> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.value().clone())
            .filter(|entry| entry.user_id == user_id)
            .collect();
        items.sort_by(|lhs, rhs| {
            lhs.market_id
                .cmp(&rhs.market_id)
                .then_with(|| lhs.outcome.cmp(&rhs.outcome))
        });
        items
    }

    pub(crate) fn has_persisted_event_id(&self, event_id: &str) -> bool {
        self.persisted_events.contains_key(event_id)
    }

    pub(crate) fn has_applied_state_event_id(&self, event_id: &str) -> bool {
        self.applied_state_events.contains_key(event_id)
    }

    pub(crate) fn has_fully_applied_trade(&self, trade_id: &str) -> bool {
        self.has_applied_state_event_id(&format!("{trade_id}:buy"))
            && self.has_applied_state_event_id(&format!("{trade_id}:sell"))
    }

    pub(crate) fn entry_price_map(&self) -> HashMap<(String, String, i32), i64> {
        self.entries
            .iter()
            .filter_map(|entry| {
                let value = entry.value();
                value.entry_price.map(|price| {
                    (
                        (
                            value.user_id.clone(),
                            value.market_id.clone(),
                            value.outcome,
                        ),
                        price,
                    )
                })
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn apply_fill(&self, fill: &types::Fill) -> anyhow::Result<()> {
        let delta_qty = match fill.side {
            Side::Buy => fill.amount,
            Side::Sell => -fill.amount,
        };
        self.apply_event(PositionCostLedgerEvent {
            event_id: fill.op_id.clone(),
            trade_id: fill.id.clone(),
            user_id: fill.user_id.clone(),
            market_id: fill.market_id.clone(),
            outcome: fill.outcome,
            instrument_kind: InstrumentKind::Spot,
            delta_qty,
            price: fill.price,
            recorded_at: fill.timestamp,
        })
    }

    fn apply_trade(&self, trade: &TradeJournalRecord) -> anyhow::Result<()> {
        self.apply_event(PositionCostLedgerEvent {
            event_id: format!("{}:buy", trade.trade_id),
            trade_id: trade.trade_id.clone(),
            user_id: trade.buy_user_id.clone(),
            market_id: trade.market_id.clone(),
            outcome: trade.outcome,
            instrument_kind: trade.instrument_kind,
            delta_qty: trade.amount,
            price: trade.price,
            recorded_at: trade.recorded_at,
        })?;
        self.apply_event(PositionCostLedgerEvent {
            event_id: format!("{}:sell", trade.trade_id),
            trade_id: trade.trade_id.clone(),
            user_id: trade.sell_user_id.clone(),
            market_id: trade.market_id.clone(),
            outcome: trade.outcome,
            instrument_kind: trade.instrument_kind,
            delta_qty: -trade.amount,
            price: trade.price,
            recorded_at: trade.recorded_at,
        })?;
        Ok(())
    }

    fn apply_event(&self, event: PositionCostLedgerEvent) -> anyhow::Result<()> {
        if self.applied_state_events.contains_key(&event.event_id) {
            return Ok(());
        }
        let key = position_cost_key(&event.user_id, &event.market_id, event.outcome);
        let _guard = self.write_locks[position_cost_lock_shard(&key)].lock();
        if self.applied_state_events.contains_key(&event.event_id) {
            return Ok(());
        }
        let mut state = self
            .entries
            .get(&key)
            .map(|entry| PositionCostState {
                net_qty: entry.net_qty,
                open_notional: if entry.open_notional != 0 {
                    entry.open_notional
                } else {
                    entry
                        .net_qty
                        .saturating_mul(entry.entry_price.unwrap_or_default())
                },
                entry_price: entry.entry_price,
                realized_pnl: entry.realized_pnl,
            })
            .unwrap_or_default();
        apply_fill_to_cost_state(&mut state, event.delta_qty, event.price);
        let next = PositionCostLedgerEntry {
            user_id: event.user_id.clone(),
            market_id: event.market_id.clone(),
            outcome: event.outcome,
            instrument_kind: event.instrument_kind,
            net_qty: state.net_qty,
            open_notional: state.open_notional,
            entry_price: state.entry_price,
            realized_pnl: state.realized_pnl,
            updated_at: event.recorded_at,
        };
        if !self.persisted_events.contains_key(&event.event_id) {
            self.event_store.append(&event)?;
            self.persisted_events.insert(event.event_id.clone(), ());
        }
        self.state_store.append(&next)?;
        self.applied_state_events.insert(event.event_id, ());
        self.entries.insert(key, next);
        Ok(())
    }

    fn replay_event(&self, event: PositionCostLedgerEvent) {
        if self.applied_state_events.contains_key(&event.event_id) {
            return;
        }
        let key = position_cost_key(&event.user_id, &event.market_id, event.outcome);
        let mut state = self
            .entries
            .get(&key)
            .map(|entry| PositionCostState {
                net_qty: entry.net_qty,
                open_notional: if entry.open_notional != 0 {
                    entry.open_notional
                } else {
                    entry
                        .net_qty
                        .saturating_mul(entry.entry_price.unwrap_or_default())
                },
                entry_price: entry.entry_price,
                realized_pnl: entry.realized_pnl,
            })
            .unwrap_or_default();
        apply_fill_to_cost_state(&mut state, event.delta_qty, event.price);
        self.entries.insert(
            key,
            PositionCostLedgerEntry {
                user_id: event.user_id.clone(),
                market_id: event.market_id.clone(),
                outcome: event.outcome,
                instrument_kind: event.instrument_kind,
                net_qty: state.net_qty,
                open_notional: state.open_notional,
                entry_price: state.entry_price,
                realized_pnl: state.realized_pnl,
                updated_at: event.recorded_at,
            },
        );
        self.persisted_events.insert(event.event_id.clone(), ());
        self.applied_state_events.insert(event.event_id, ());
    }
}

impl matching::partitioned::PositionCostStore for PositionCostLedgerStore {
    fn record_trade(&self, record: &TradeJournalRecord) -> anyhow::Result<()> {
        let result = self.apply_trade(record);
        if result.is_ok() {
            *self.last_synced_trade_cursor.lock() =
                Some((record.recorded_at, record.trade_id.clone()));
        }
        result
    }
}

fn position_cost_key(user_id: &str, market_id: &str, outcome: i32) -> String {
    format!("{user_id}:{market_id}:{outcome}")
}

fn position_cost_lock_shard(key: &str) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % POSITION_COST_LOCK_SHARDS
}

fn apply_fill_to_cost_state(state: &mut PositionCostState, delta_qty: i64, price: i64) {
    if delta_qty == 0 || price <= 0 {
        return;
    }
    if state.net_qty == 0 {
        state.net_qty = delta_qty;
        state.open_notional = delta_qty.saturating_mul(price);
        state.entry_price = Some(state.open_notional.abs() / state.net_qty.abs().max(1));
        return;
    }
    let current_sign = state.net_qty.signum();
    let delta_sign = delta_qty.signum();
    if current_sign == delta_sign {
        state.net_qty = state.net_qty.saturating_add(delta_qty);
        state.open_notional = state
            .open_notional
            .saturating_add(delta_qty.saturating_mul(price));
        state.entry_price = Some(state.open_notional.abs() / state.net_qty.abs().max(1));
        return;
    }

    let current_abs = state.net_qty.abs();
    let delta_abs = delta_qty.abs();
    let closed_qty = current_abs.min(delta_abs);
    let entry = state.open_notional.abs() / current_abs.max(1);
    state.realized_pnl = state.realized_pnl.saturating_add(
        (price - entry)
            .saturating_mul(closed_qty)
            .saturating_mul(current_sign),
    );
    if delta_abs < current_abs {
        let released_notional =
            state.open_notional.abs().saturating_mul(closed_qty) / current_abs.max(1);
        state.net_qty = state.net_qty.saturating_add(delta_qty);
        state.open_notional = state
            .open_notional
            .saturating_sub(current_sign.saturating_mul(released_notional));
        state.entry_price = Some(state.open_notional.abs() / state.net_qty.abs().max(1));
        return;
    }
    if delta_abs == current_abs {
        state.net_qty = 0;
        state.open_notional = 0;
        state.entry_price = None;
        return;
    }

    let leftover = delta_abs.saturating_sub(current_abs);
    state.net_qty = delta_sign.saturating_mul(leftover);
    state.open_notional = state.net_qty.saturating_mul(price);
    state.entry_price = Some(price);
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::InMemoryWal;

    #[test]
    fn position_cost_ledger_tracks_flip_and_realized_pnl() {
        let state_store: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>> =
            Arc::new(InMemoryWal::new());
        let event_store: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>> =
            Arc::new(InMemoryWal::new());
        let ledger = PositionCostLedgerStore::new(state_store, event_store).expect("ledger");
        let now = Utc::now();
        let trades = vec![
            TradeJournalRecord {
                partition_id: 0,
                trade_id: "t1".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b1".to_string(),
                buy_user_id: "u1".to_string(),
                sell_order_id: "s1".to_string(),
                sell_user_id: "maker".to_string(),
                price: 100,
                amount: 10,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now,
                aggressor_side: None,
            },
            TradeJournalRecord {
                partition_id: 0,
                trade_id: "t2".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b2".to_string(),
                buy_user_id: "maker".to_string(),
                sell_order_id: "s2".to_string(),
                sell_user_id: "u1".to_string(),
                price: 120,
                amount: 5,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now + chrono::Duration::milliseconds(1),
                aggressor_side: None,
            },
            TradeJournalRecord {
                partition_id: 0,
                trade_id: "t3".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b3".to_string(),
                buy_user_id: "maker".to_string(),
                sell_order_id: "s3".to_string(),
                sell_user_id: "u1".to_string(),
                price: 130,
                amount: 10,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now + chrono::Duration::milliseconds(2),
                aggressor_side: None,
            },
        ];

        for trade in &trades {
            ledger.apply_trade(trade).expect("apply trade");
        }

        let entry = ledger
            .get("u1", "perp:btc-usdt", 0)
            .expect("position cost entry");
        assert_eq!(entry.net_qty, -5);
        assert_eq!(entry.entry_price, Some(130));
        assert_eq!(entry.realized_pnl, 250);
    }

    #[test]
    fn position_cost_ledger_applies_live_fill_events_idempotently() {
        let state_store: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>> =
            Arc::new(InMemoryWal::new());
        let event_store: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>> =
            Arc::new(InMemoryWal::new());
        let ledger = PositionCostLedgerStore::new(state_store, event_store).expect("ledger");
        let now = Utc::now();
        let fill = types::Fill {
            id: "trade-1".to_string(),
            intent_id: "intent-1".to_string(),
            user_id: "u1".to_string(),
            market_id: "perp:btc-usdt".to_string(),
            side: Side::Buy,
            price: 100,
            amount: 3,
            outcome: 0,
            timestamp: now,
            op_id: "trade_buy_trade-1".to_string(),
            fee: 0,
            fee_bps: 0,
            is_maker: false,
            aggressor_side: None,
            fill_index: 0,
            settlement_status: Default::default(),
        };

        ledger.apply_fill(&fill).expect("apply fill");
        ledger.apply_fill(&fill).expect("duplicate fill ignored");

        let entry = ledger
            .get("u1", "perp:btc-usdt", 0)
            .expect("position cost entry");
        assert_eq!(entry.net_qty, 3);
        assert_eq!(entry.entry_price, Some(100));
        assert_eq!(entry.realized_pnl, 0);
    }

    #[test]
    fn position_cost_ledger_recovers_from_event_log_when_state_log_is_stale() {
        let state_store: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>> =
            Arc::new(InMemoryWal::new());
        let event_store: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>> =
            Arc::new(InMemoryWal::new());
        let now = Utc::now();

        state_store
            .append(&PositionCostLedgerEntry {
                user_id: "u1".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                net_qty: 10,
                open_notional: 1_000,
                entry_price: Some(100),
                realized_pnl: 0,
                updated_at: now,
            })
            .expect("stale state snapshot");

        event_store
            .append(&PositionCostLedgerEvent {
                event_id: "t1:buy".to_string(),
                trade_id: "t1".to_string(),
                user_id: "u1".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                delta_qty: 10,
                price: 100,
                recorded_at: now,
            })
            .expect("event 1");
        event_store
            .append(&PositionCostLedgerEvent {
                event_id: "t2:sell".to_string(),
                trade_id: "t2".to_string(),
                user_id: "u1".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                delta_qty: -15,
                price: 130,
                recorded_at: now + chrono::Duration::milliseconds(1),
            })
            .expect("event 2");

        let ledger = PositionCostLedgerStore::new(state_store, event_store).expect("ledger");
        let entry = ledger
            .get("u1", "perp:btc-usdt", 0)
            .expect("replayed entry");
        assert_eq!(entry.net_qty, -5);
        assert_eq!(entry.entry_price, Some(130));
        assert_eq!(entry.realized_pnl, 300);
    }

    #[test]
    fn position_cost_sync_from_trade_journal_is_incremental() {
        let state_store: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>> =
            Arc::new(InMemoryWal::new());
        let event_store: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>> =
            Arc::new(InMemoryWal::new());
        let trade_store: Arc<dyn persistence::WalStore<TradeJournalRecord>> =
            Arc::new(InMemoryWal::new());
        let ledger =
            PositionCostLedgerStore::new(state_store, event_store.clone()).expect("ledger");
        let now = Utc::now();

        trade_store
            .append(&TradeJournalRecord {
                partition_id: 0,
                trade_id: "t1".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b1".to_string(),
                buy_user_id: "u1".to_string(),
                sell_order_id: "s1".to_string(),
                sell_user_id: "maker".to_string(),
                price: 100,
                amount: 2,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now,
                aggressor_side: None,
            })
            .expect("trade 1");
        ledger
            .sync_from_trade_journal(trade_store.as_ref())
            .expect("first sync");
        assert_eq!(event_store.entries().expect("event entries").len(), 2);

        trade_store
            .append(&TradeJournalRecord {
                partition_id: 0,
                trade_id: "t2".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b2".to_string(),
                buy_user_id: "u1".to_string(),
                sell_order_id: "s2".to_string(),
                sell_user_id: "maker".to_string(),
                price: 110,
                amount: 1,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now + chrono::Duration::milliseconds(1),
                aggressor_side: None,
            })
            .expect("trade 2");
        ledger
            .sync_from_trade_journal(trade_store.as_ref())
            .expect("second sync");

        assert_eq!(event_store.entries().expect("event entries").len(), 4);

        trade_store
            .append(&TradeJournalRecord {
                partition_id: 0,
                trade_id: "t0".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b0".to_string(),
                buy_user_id: "u1".to_string(),
                sell_order_id: "s0".to_string(),
                sell_user_id: "maker".to_string(),
                price: 90,
                amount: 1,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now - chrono::Duration::milliseconds(1),
                aggressor_side: None,
            })
            .expect("backfilled historical trade");
        ledger
            .sync_from_trade_journal(trade_store.as_ref())
            .expect("third sync");

        assert_eq!(event_store.entries().expect("event entries").len(), 6);
        let entry = ledger
            .get("u1", "perp:btc-usdt", 0)
            .expect("position cost entry");
        assert_eq!(entry.net_qty, 4);
        assert_eq!(entry.entry_price, Some(100));
    }
}
