use risk::{MarginSnapshot, RiskEngine, RiskError};
use std::collections::BTreeMap;
use types::{InstrumentKind, InstrumentSpec};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PositionProjection {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub instrument_kind: InstrumentKind,
    pub available: i64,
    pub hold: i64,
    pub net_qty: i64,
}

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
    pub margin_ratio_bps: Option<i64>,
    pub liquidation_required: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PnlProjection {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub position_qty: i64,
    pub entry_price: Option<i64>,
    pub mark_price: i64,
    pub unrealized_pnl: Option<i64>,
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

fn infer_derivative_kind(market_id: &str) -> InstrumentKind {
    if market_id.starts_with("perp:") || market_id.starts_with("perpetual:") {
        InstrumentKind::Perpetual
    } else {
        InstrumentKind::Margin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn project_pnl_uses_entry_and_mark_price() {
        let pnl = project_pnl("u1", "perp:btc-usdt", 0, 3, Some(100), 110);
        assert_eq!(pnl.unrealized_pnl, Some(30));
    }
}
