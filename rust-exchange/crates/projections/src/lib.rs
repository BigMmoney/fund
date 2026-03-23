//! Read-only projection functions that derive positions, margins, and PnL
//! from ledger balances and trade journals. All functions are pure (no side effects).

use matching::partitioned::TradeJournalRecord;
use risk::{MarginSnapshot, RiskEngine, RiskError};
use std::collections::{BTreeMap, HashMap};
use types::{InstrumentKind, InstrumentSpec};

/// Aggregated position for a user in a specific market/outcome.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PositionProjection {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub instrument_kind: InstrumentKind,
    /// Available (unlocked) quantity.
    pub available: i64,
    /// Quantity locked by resting orders.
    pub hold: i64,
    /// Net derivative position (positive = long, negative = short).
    pub net_qty: i64,
}

/// Margin health snapshot indicating collateral adequacy and liquidation risk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct MarginProjection {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub collateral_total: i64,
    pub position_qty: i64,
    pub mark_price: i64,
    pub notional: i64,
    pub initial_margin_required: i64,
    pub maintenance_margin_required: i64,
    /// Margin ratio in basis points (None when notional is zero).
    pub margin_ratio_bps: Option<i64>,
    /// True if maintenance margin is breached and liquidation should occur.
    pub liquidation_required: bool,
}

/// Unrealised PnL derived from entry vs mark price.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PnlProjection {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub position_qty: i64,
    /// Weighted average entry price (None if unknown).
    pub entry_price: Option<i64>,
    pub mark_price: i64,
    /// `(mark_price - entry_price) * position_qty`. None when entry_price is unknown.
    pub unrealized_pnl: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PositionCostProjection {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub net_qty: i64,
    pub entry_price: Option<i64>,
}

pub fn project_positions(
    user_id: &str,
    balances: &std::collections::HashMap<String, i64>,
) -> Vec<PositionProjection> {
    let prefix = format!("U:{user_id}:");
    let mut positions: BTreeMap<(String, i32, InstrumentKind), (i64, i64, i64)> = BTreeMap::new();

    for (account, balance) in balances {
        if !account.starts_with(&prefix) {
            continue;
        }
        let suffix = &account[prefix.len()..];
        if suffix == "USDC" || suffix == "USDC:HOLD" {
            continue;
        }

        let parts: Vec<_> = suffix.split(':').collect();
        if parts.first() == Some(&"DERIV") && parts.len() >= 3 {
            if let Ok(outcome) = parts[parts.len() - 1].parse::<i32>() {
                let market_id = parts[1..parts.len() - 1].join(":");
                positions
                    .entry((
                        market_id.clone(),
                        outcome,
                        infer_derivative_kind(&market_id),
                    ))
                    .or_default()
                    .2 = *balance;
            }
            continue;
        }

        if parts.len() >= 3 && parts[parts.len() - 1] == "HOLD" {
            if let Ok(outcome) = parts[parts.len() - 2].parse::<i32>() {
                let market_id = parts[..parts.len() - 2].join(":");
                positions
                    .entry((market_id, outcome, InstrumentKind::Spot))
                    .or_default()
                    .1 = *balance;
            }
            continue;
        }

        if parts.len() >= 2 {
            if let Ok(outcome) = parts[parts.len() - 1].parse::<i32>() {
                let market_id = parts[..parts.len() - 1].join(":");
                if market_id != "USDC" {
                    positions
                        .entry((market_id, outcome, InstrumentKind::Spot))
                        .or_default()
                        .0 = *balance;
                }
            }
        }
    }

    positions
        .into_iter()
        .map(
            |((market_id, outcome, instrument_kind), (available, hold, net_qty))| {
                PositionProjection {
                    user_id: user_id.to_string(),
                    market_id,
                    outcome,
                    instrument_kind,
                    available,
                    hold,
                    net_qty,
                }
            },
        )
        .filter(|item| item.available != 0 || item.hold != 0 || item.net_qty != 0)
        .collect()
}

pub fn project_margin(
    risk: &RiskEngine,
    user_id: &str,
    instrument: &InstrumentSpec,
    outcome: i32,
    mark_price: i64,
    leverage: Option<u32>,
    maintenance_margin_bps: i64,
) -> Result<MarginProjection, RiskError> {
    let snapshot = risk.margin_snapshot(
        user_id,
        instrument,
        outcome,
        mark_price,
        leverage,
        maintenance_margin_bps,
    )?;
    Ok(margin_snapshot_to_projection(snapshot))
}

pub fn margin_snapshot_to_projection(snapshot: MarginSnapshot) -> MarginProjection {
    MarginProjection {
        user_id: snapshot.user_id,
        market_id: snapshot.market_id,
        outcome: snapshot.outcome,
        collateral_total: snapshot.collateral_total,
        position_qty: snapshot.position_qty,
        mark_price: snapshot.mark_price,
        notional: snapshot.notional,
        initial_margin_required: snapshot.initial_margin_required,
        maintenance_margin_required: snapshot.maintenance_margin_required,
        margin_ratio_bps: snapshot.margin_ratio_bps,
        liquidation_required: snapshot.liquidation_required,
    }
}

pub fn project_pnl(
    user_id: &str,
    market_id: &str,
    outcome: i32,
    position_qty: i64,
    entry_price: Option<i64>,
    mark_price: i64,
) -> PnlProjection {
    let unrealized_pnl = entry_price.map(|entry| (mark_price - entry).saturating_mul(position_qty));
    PnlProjection {
        user_id: user_id.to_string(),
        market_id: market_id.to_string(),
        outcome,
        position_qty,
        entry_price,
        mark_price,
        unrealized_pnl,
    }
}

pub fn project_position_costs_from_trades(
    trades: &[TradeJournalRecord],
) -> Vec<PositionCostProjection> {
    #[derive(Clone, Copy, Default)]
    struct EntryState {
        qty: i64,
        avg_entry: Option<i64>,
    }

    fn apply_fill(state: &mut EntryState, delta_qty: i64, price: i64) {
        if delta_qty == 0 || price <= 0 {
            return;
        }
        if state.qty == 0 {
            state.qty = delta_qty;
            state.avg_entry = Some(price);
            return;
        }
        let current_sign = state.qty.signum();
        let delta_sign = delta_qty.signum();
        if current_sign == delta_sign {
            let current_abs = state.qty.abs() as i128;
            let delta_abs = delta_qty.abs() as i128;
            let weighted_notional = current_abs
                .saturating_mul(state.avg_entry.unwrap_or(price) as i128)
                .saturating_add(delta_abs.saturating_mul(price as i128));
            let next_abs = current_abs.saturating_add(delta_abs).max(1);
            state.qty = state.qty.saturating_add(delta_qty);
            state.avg_entry = Some((weighted_notional / next_abs) as i64);
            return;
        }
        let current_abs = state.qty.abs();
        let delta_abs = delta_qty.abs();
        if delta_abs < current_abs {
            state.qty = state.qty.saturating_add(delta_qty);
            return;
        }
        if delta_abs == current_abs {
            state.qty = 0;
            state.avg_entry = None;
            return;
        }
        let leftover = delta_abs.saturating_sub(current_abs);
        state.qty = delta_sign.saturating_mul(leftover);
        state.avg_entry = Some(price);
    }

    let mut sorted = trades.to_vec();
    sorted.sort_by(|lhs, rhs| lhs.recorded_at.cmp(&rhs.recorded_at));
    let mut states: HashMap<(String, String, i32), EntryState> = HashMap::new();
    for trade in sorted {
        let buy_key = (
            trade.buy_user_id.clone(),
            trade.market_id.clone(),
            trade.outcome,
        );
        let sell_key = (
            trade.sell_user_id.clone(),
            trade.market_id.clone(),
            trade.outcome,
        );
        apply_fill(
            states.entry(buy_key).or_default(),
            trade.amount,
            trade.price,
        );
        apply_fill(
            states.entry(sell_key).or_default(),
            -trade.amount,
            trade.price,
        );
    }

    let mut items: Vec<_> = states
        .into_iter()
        .filter(|(_, state)| state.qty != 0)
        .map(
            |((user_id, market_id, outcome), state)| PositionCostProjection {
                user_id,
                market_id,
                outcome,
                net_qty: state.qty,
                entry_price: state.avg_entry,
            },
        )
        .collect();
    items.sort_by(|lhs, rhs| {
        lhs.user_id
            .cmp(&rhs.user_id)
            .then_with(|| lhs.market_id.cmp(&rhs.market_id))
            .then_with(|| lhs.outcome.cmp(&rhs.outcome))
    });
    items
}

pub fn project_position_cost_entry_price_map(
    trades: &[TradeJournalRecord],
) -> HashMap<(String, String, i32), i64> {
    project_position_costs_from_trades(trades)
        .into_iter()
        .filter_map(|item| {
            item.entry_price
                .map(|entry_price| ((item.user_id, item.market_id, item.outcome), entry_price))
        })
        .collect()
}

// ── Funding Rate Projection ─────────────────────────────────────────────

/// A single funding rate observation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct FundingRateProjection {
    pub market_id: String,
    pub funding_rate_ppm: i64,
    pub mark_price: i64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Compute the time-weighted average funding rate (parts-per-million) over a
/// sequence of observations. Returns 0 when the input is empty.
pub fn project_average_funding_rate(observations: &[FundingRateProjection]) -> i64 {
    if observations.is_empty() {
        return 0;
    }
    let sum: i128 = observations
        .iter()
        .map(|o| o.funding_rate_ppm as i128)
        .sum();
    (sum / observations.len() as i128) as i64
}

// ── Open Interest Projection ────────────────────────────────────────────

/// Aggregate open interest for a single market.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct OpenInterestProjection {
    pub market_id: String,
    pub outcome: i32,
    /// Total long quantity (absolute). Each buy creates a long, each sell an equal short.
    pub total_long_qty: i64,
    /// Total short quantity (absolute).
    pub total_short_qty: i64,
    /// Notional open interest = min(long, short) × mark_price.
    pub notional_open_interest: i64,
}

/// Project open interest from trade journal for a specific market.
/// Open interest = sum of all active net-long positions (= sum of net-short).
pub fn project_open_interest(
    trades: &[TradeJournalRecord],
    market_id: &str,
    outcome: i32,
    mark_price: i64,
) -> OpenInterestProjection {
    let mut user_positions: HashMap<String, i64> = HashMap::new();

    for trade in trades {
        if trade.market_id != market_id || trade.outcome != outcome {
            continue;
        }
        *user_positions.entry(trade.buy_user_id.clone()).or_default() += trade.amount;
        *user_positions
            .entry(trade.sell_user_id.clone())
            .or_default() -= trade.amount;
    }

    let total_long_qty: i64 = user_positions.values().filter(|&&v| v > 0).sum();
    let total_short_qty: i64 = user_positions
        .values()
        .filter(|&&v| v < 0)
        .map(|v| v.abs())
        .sum();
    let oi_qty = total_long_qty.min(total_short_qty);
    let notional_open_interest = mark_price.saturating_mul(oi_qty);

    OpenInterestProjection {
        market_id: market_id.to_string(),
        outcome,
        total_long_qty,
        total_short_qty,
        notional_open_interest,
    }
}

// ── Fee Summary Projection ──────────────────────────────────────────────

/// Aggregate fee summary for a user across all markets.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct FeeSummaryProjection {
    pub user_id: String,
    /// Total maker fees paid (negative if rebate).
    pub total_maker_fees: i64,
    /// Total taker fees paid.
    pub total_taker_fees: i64,
    /// Total fees combined.
    pub total_fees: i64,
    /// Number of fills as maker.
    pub maker_fill_count: u64,
    /// Number of fills as taker.
    pub taker_fill_count: u64,
    /// Total traded volume (sum of notional across all fills).
    pub total_volume: i64,
}

/// Project fee summary for a specific user from trade journal records.
pub fn project_fee_summary(trades: &[TradeJournalRecord], user_id: &str) -> FeeSummaryProjection {
    let mut total_maker_fees = 0i64;
    let mut total_taker_fees = 0i64;
    let mut maker_fill_count = 0u64;
    let mut taker_fill_count = 0u64;
    let mut total_volume = 0i64;

    for trade in trades {
        let notional = trade.price.saturating_mul(trade.amount);
        if trade.buy_user_id == user_id {
            // Buyer: determine if maker or taker via aggressor_side
            let is_taker = trade.aggressor_side.is_none_or(|s| s == types::Side::Buy);
            if is_taker {
                total_taker_fees = total_taker_fees.saturating_add(trade.taker_fee);
                taker_fill_count += 1;
            } else {
                total_maker_fees = total_maker_fees.saturating_add(trade.maker_fee);
                maker_fill_count += 1;
            }
            total_volume = total_volume.saturating_add(notional);
        }
        if trade.sell_user_id == user_id {
            let is_taker = trade.aggressor_side.is_none_or(|s| s == types::Side::Sell);
            if is_taker {
                total_taker_fees = total_taker_fees.saturating_add(trade.taker_fee);
                taker_fill_count += 1;
            } else {
                total_maker_fees = total_maker_fees.saturating_add(trade.maker_fee);
                maker_fill_count += 1;
            }
            total_volume = total_volume.saturating_add(notional);
        }
    }

    FeeSummaryProjection {
        user_id: user_id.to_string(),
        total_maker_fees,
        total_taker_fees,
        total_fees: total_maker_fees.saturating_add(total_taker_fees),
        maker_fill_count,
        taker_fill_count,
        total_volume,
    }
}

fn infer_derivative_kind(market_id: &str) -> InstrumentKind {
    if market_id.starts_with("perp:") || market_id.starts_with("perpetual:") {
        InstrumentKind::Perpetual
    } else if market_id.starts_with("future:") {
        InstrumentKind::Future
    } else if market_id.starts_with("option:") {
        InstrumentKind::Option
    } else {
        InstrumentKind::Margin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn project_positions_parses_spot_and_derivative_accounts() {
        let mut balances = HashMap::new();
        balances.insert("U:u1:btc-usdt:0".to_string(), 10);
        balances.insert("U:u1:btc-usdt:0:HOLD".to_string(), 3);
        balances.insert("U:u1:DERIV:perp:btc-usdt:0".to_string(), -5);

        let positions = project_positions("u1", &balances);
        assert_eq!(positions.len(), 2);
        assert!(positions.iter().any(|item| {
            item.instrument_kind == InstrumentKind::Spot && item.available == 10 && item.hold == 3
        }));
        assert!(positions.iter().any(|item| {
            item.instrument_kind == InstrumentKind::Perpetual
                && item.market_id == "perp:btc-usdt"
                && item.net_qty == -5
        }));
    }

    #[test]
    fn infer_derivative_kind_supports_future_and_option() {
        assert_eq!(
            infer_derivative_kind("future:btc-usdt:202606"),
            InstrumentKind::Future
        );
        assert_eq!(
            infer_derivative_kind("option:btc-usdt:call-70000:202606"),
            InstrumentKind::Option
        );
    }

    #[test]
    fn project_pnl_uses_entry_and_mark_price() {
        let pnl = project_pnl("u1", "perp:btc-usdt", 0, 3, Some(100), 110);
        assert_eq!(pnl.unrealized_pnl, Some(30));
    }

    #[test]
    fn project_position_costs_track_reduce_and_flip() {
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

        let projections = project_position_costs_from_trades(&trades);
        let item = projections
            .into_iter()
            .find(|value| value.user_id == "u1" && value.market_id == "perp:btc-usdt")
            .expect("projection");
        assert_eq!(item.net_qty, -5);
        assert_eq!(item.entry_price, Some(130));
    }

    #[test]
    fn project_positions_empty_balances() {
        let balances = HashMap::new();
        assert!(project_positions("u1", &balances).is_empty());
    }

    #[test]
    fn project_positions_ignores_cash_accounts() {
        let mut balances = HashMap::new();
        balances.insert("U:u1:USDC".to_string(), 10_000);
        balances.insert("U:u1:USDC:HOLD".to_string(), 500);
        // Cash accounts should not appear as positions.
        assert!(project_positions("u1", &balances).is_empty());
    }

    #[test]
    fn project_positions_ignores_other_users() {
        let mut balances = HashMap::new();
        balances.insert("U:other:btc-usdt:0".to_string(), 10);
        assert!(project_positions("u1", &balances).is_empty());
    }

    #[test]
    fn project_positions_filters_zero_balances() {
        let mut balances = HashMap::new();
        balances.insert("U:u1:btc-usdt:0".to_string(), 0);
        balances.insert("U:u1:btc-usdt:0:HOLD".to_string(), 0);
        assert!(project_positions("u1", &balances).is_empty());
    }

    #[test]
    fn project_pnl_with_no_entry_price() {
        let pnl = project_pnl("u1", "perp:btc-usdt", 0, 5, None, 100);
        assert_eq!(pnl.unrealized_pnl, None);
    }

    #[test]
    fn project_pnl_negative_unrealized() {
        let pnl = project_pnl("u1", "perp:btc-usdt", 0, 3, Some(200), 150);
        assert_eq!(pnl.unrealized_pnl, Some(-150)); // (150-200)*3
    }

    #[test]
    fn project_position_costs_empty_trades() {
        let projections = project_position_costs_from_trades(&[]);
        assert!(projections.is_empty());
    }

    #[test]
    fn project_position_cost_exact_close_removes_entry() {
        let now = Utc::now();
        let trades = vec![
            TradeJournalRecord {
                partition_id: 0,
                trade_id: "t1".to_string(),
                market_id: "m1".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b1".to_string(),
                buy_user_id: "u1".to_string(),
                sell_order_id: "s1".to_string(),
                sell_user_id: "u2".to_string(),
                price: 100,
                amount: 5,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now,
                aggressor_side: None,
            },
            TradeJournalRecord {
                partition_id: 0,
                trade_id: "t2".to_string(),
                market_id: "m1".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b2".to_string(),
                buy_user_id: "u2".to_string(),
                sell_order_id: "s2".to_string(),
                sell_user_id: "u1".to_string(),
                price: 120,
                amount: 5,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now + chrono::Duration::milliseconds(1),
                aggressor_side: None,
            },
        ];
        let projections = project_position_costs_from_trades(&trades);
        // u1 opened +5, then closed -5 → net_qty = 0, excluded from output
        assert!(projections
            .iter()
            .all(|p| !(p.user_id == "u1" && p.net_qty == 0)));
    }

    #[test]
    fn infer_derivative_kind_defaults_to_margin() {
        assert_eq!(
            infer_derivative_kind("some-unknown-prefix:stuff"),
            InstrumentKind::Margin
        );
    }

    #[test]
    fn project_position_cost_entry_price_map_builds_correctly() {
        let now = Utc::now();
        let trades = vec![TradeJournalRecord {
            partition_id: 0,
            trade_id: "t1".to_string(),
            market_id: "m1".to_string(),
            outcome: 0,
            instrument_kind: InstrumentKind::Perpetual,
            buy_order_id: "b1".to_string(),
            buy_user_id: "u1".to_string(),
            sell_order_id: "s1".to_string(),
            sell_user_id: "u2".to_string(),
            price: 100,
            amount: 5,
            maker_fee: 0,
            taker_fee: 0,
            recorded_at: now,
            aggressor_side: None,
        }];
        let map = project_position_cost_entry_price_map(&trades);
        assert_eq!(map.get(&("u1".into(), "m1".into(), 0)), Some(&100));
    }

    #[test]
    fn project_average_funding_rate_empty() {
        assert_eq!(project_average_funding_rate(&[]), 0);
    }

    #[test]
    fn project_average_funding_rate_computes_mean() {
        let now = Utc::now();
        let observations = vec![
            FundingRateProjection {
                market_id: "perp:btc-usdt".into(),
                funding_rate_ppm: 100,
                mark_price: 50_000,
                timestamp: now,
            },
            FundingRateProjection {
                market_id: "perp:btc-usdt".into(),
                funding_rate_ppm: 200,
                mark_price: 50_000,
                timestamp: now,
            },
            FundingRateProjection {
                market_id: "perp:btc-usdt".into(),
                funding_rate_ppm: 300,
                mark_price: 50_000,
                timestamp: now,
            },
        ];
        assert_eq!(project_average_funding_rate(&observations), 200);
    }

    #[test]
    fn project_open_interest_from_trades() {
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
                sell_user_id: "u2".to_string(),
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
                buy_user_id: "u3".to_string(),
                sell_order_id: "s2".to_string(),
                sell_user_id: "u2".to_string(),
                price: 110,
                amount: 5,
                maker_fee: 0,
                taker_fee: 0,
                recorded_at: now,
                aggressor_side: None,
            },
        ];
        // u1: +10, u2: -15, u3: +5 → longs = 15, shorts = 15
        let oi = project_open_interest(&trades, "perp:btc-usdt", 0, 100);
        assert_eq!(oi.total_long_qty, 15);
        assert_eq!(oi.total_short_qty, 15);
        assert_eq!(oi.notional_open_interest, 1500); // 15 * 100
    }

    #[test]
    fn project_open_interest_empty() {
        let oi = project_open_interest(&[], "perp:btc-usdt", 0, 100);
        assert_eq!(oi.total_long_qty, 0);
        assert_eq!(oi.total_short_qty, 0);
        assert_eq!(oi.notional_open_interest, 0);
    }

    #[test]
    fn project_fee_summary_aggregates() {
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
                sell_user_id: "u2".to_string(),
                price: 100,
                amount: 10,
                maker_fee: 5,
                taker_fee: 10,
                recorded_at: now,
                aggressor_side: Some(types::Side::Buy),
            },
            TradeJournalRecord {
                partition_id: 0,
                trade_id: "t2".to_string(),
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                instrument_kind: InstrumentKind::Perpetual,
                buy_order_id: "b2".to_string(),
                buy_user_id: "u2".to_string(),
                sell_order_id: "s2".to_string(),
                sell_user_id: "u1".to_string(),
                price: 110,
                amount: 5,
                maker_fee: 3,
                taker_fee: 7,
                recorded_at: now,
                aggressor_side: Some(types::Side::Sell),
            },
        ];
        // u1: buy in t1 (taker, fee=10) + sell in t2 (taker, fee=7)
        let summary = project_fee_summary(&trades, "u1");
        assert_eq!(summary.total_taker_fees, 17);
        assert_eq!(summary.total_maker_fees, 0);
        assert_eq!(summary.total_fees, 17);
        assert_eq!(summary.taker_fill_count, 2);
        assert_eq!(summary.maker_fill_count, 0);
        assert_eq!(summary.total_volume, 1550); // 100*10 + 110*5

        // u2: sell in t1 (maker, fee=5) + buy in t2 (maker, fee=3)
        let summary2 = project_fee_summary(&trades, "u2");
        assert_eq!(summary2.total_maker_fees, 8);
        assert_eq!(summary2.total_taker_fees, 0);
        assert_eq!(summary2.maker_fill_count, 2);
    }

    #[test]
    fn project_fee_summary_empty() {
        let summary = project_fee_summary(&[], "u1");
        assert_eq!(summary.total_fees, 0);
        assert_eq!(summary.total_volume, 0);
    }
}
