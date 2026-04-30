use super::*;

fn snapshot_order_to_json(order: &RestingOrderSnapshot) -> serde_json::Value {
    serde_json::json!({
        "id": order.order_id,
        "request_id": order.request_id,
        "command_seq": order.command_seq,
        "market_id": order.market_id,
        "outcome": order.outcome,
        "side": order.side,
        "order_type": order.order_type,
        "time_in_force": order.time_in_force,
        "price": order.price,
        "amount": order.original_amount,
        "filled": order.original_amount - order.remaining_amount,
        "remaining": order.remaining_amount,
        "leverage": order.leverage,
        "post_only": order.post_only,
        "reduce_only": order.reduce_only,
        "status": if order.remaining_amount < order.original_amount {
            "partial"
        } else {
            "open"
        },
        "source": "open_orders",
        "created_at": Utc::now(),
    })
}

fn build_order_lookup_from_trades(
    trades: &[TradeJournalRecord],
    user_id: &str,
    order_id: &str,
) -> Option<serde_json::Value> {
    let mut matching: Vec<_> = trades
        .iter()
        .filter(|trade| {
            (trade.buy_user_id == user_id && trade.buy_order_id == order_id)
                || (trade.sell_user_id == user_id && trade.sell_order_id == order_id)
        })
        .collect();
    if matching.is_empty() {
        return None;
    }
    matching.sort_by(|lhs, rhs| lhs.recorded_at.cmp(&rhs.recorded_at));
    let filled_amount: i64 = matching.iter().map(|trade| trade.amount).sum();
    let turnover: i128 = matching
        .iter()
        .map(|trade| trade.price as i128 * trade.amount as i128)
        .sum();
    let avg_fill_price = if filled_amount > 0 {
        Some((turnover / filled_amount as i128) as i64)
    } else {
        None
    };
    let first = matching.first()?;
    let last = matching.last()?;
    let is_buy = first.buy_user_id == user_id && first.buy_order_id == order_id;
    let cumulative_fee: i64 = matching
        .iter()
        .map(|trade| {
            if is_buy {
                trade.taker_fee
            } else {
                trade.maker_fee
            }
        })
        .sum();
    Some(serde_json::json!({
        "id": order_id,
        "market_id": first.market_id,
        "outcome": first.outcome,
        "side": if is_buy { Side::Buy } else { Side::Sell },
        "amount": filled_amount,
        "filled": filled_amount,
        "remaining": 0,
        "status": "filled",
        "source": "trade_log",
        "fills": matching.len(),
        "avg_fill_price": avg_fill_price,
        "cumulative_fee": cumulative_fee,
        "first_fill_at": first.recorded_at,
        "last_fill_at": last.recorded_at,
    }))
}

fn build_order_lookup_from_commands(
    records: &[SequencedCommandRecord],
    user_id: &str,
    order_id: &str,
) -> Option<serde_json::Value> {
    let base = records
        .iter()
        .rev()
        .find_map(|record| match &record.command {
            Command::NewOrder(command)
                if command.user_id == user_id && command.client_order_id == order_id =>
            {
                Some(command)
            }
            _ => None,
        })?;

    let base_lifecycle = base.metadata.lifecycle;
    let cancel_record = records
        .iter()
        .rev()
        .find_map(|record| match &record.command {
            Command::CancelOrder(command)
                if command.user_id == user_id && command.order_id == order_id =>
            {
                Some(command)
            }
            _ => None,
        });
    let replace_record = records
        .iter()
        .rev()
        .find_map(|record| match &record.command {
            Command::ReplaceOrder(command)
                if command.user_id == user_id && command.order_id == order_id =>
            {
                Some(command)
            }
            _ => None,
        });

    let (status, close_reason) = if base_lifecycle == types::CommandLifecycle::Rejected {
        ("rejected", Some("sequencer_rejected"))
    } else if let Some(replace) = replace_record {
        (
            "replaced",
            replace
                .new_client_order_id
                .as_deref()
                .or(Some("replacement_submitted")),
        )
    } else if cancel_record.is_some() || base_lifecycle == types::CommandLifecycle::Cancelled {
        ("cancelled", Some("cancel_order"))
    } else if matches!(base.order_type, OrderType::Market)
        || matches!(
            base.time_in_force,
            TimeInForce::Ioc | TimeInForce::Fok | TimeInForce::Gtd
        )
    {
        ("closed_no_fill", Some("not_resting_after_completion"))
    } else {
        return None;
    };

    Some(serde_json::json!({
        "id": order_id,
        "request_id": base.metadata.request_id,
        "command_seq": base.metadata.command_seq,
        "market_id": base.market_id,
        "outcome": base.outcome,
        "side": base.side,
        "order_type": base.order_type,
        "time_in_force": base.time_in_force,
        "price": base.price,
        "amount": base.amount,
        "filled": 0,
        "remaining": base.amount,
        "post_only": base.post_only,
        "reduce_only": base.reduce_only,
        "leverage": base.leverage,
        "status": status,
        "source": "sequencer_log",
        "close_reason": close_reason,
        "created_at": base.metadata.received_at,
        "updated_at": base.metadata.updated_at,
    }))
}

fn projected_order_to_json(
    entry: &OrderStateProjectionEntry,
    open_order: Option<&RestingOrderSnapshot>,
    trades: &[TradeJournalRecord],
) -> serde_json::Value {
    let mut matching: Vec<_> = trades
        .iter()
        .filter(|trade| {
            (trade.buy_user_id == entry.user_id && trade.buy_order_id == entry.order_id)
                || (trade.sell_user_id == entry.user_id && trade.sell_order_id == entry.order_id)
        })
        .collect();
    matching.sort_by(|lhs, rhs| lhs.recorded_at.cmp(&rhs.recorded_at));
    let filled_amount: i64 = matching.iter().map(|trade| trade.amount).sum();
    let turnover: i128 = matching
        .iter()
        .map(|trade| trade.price as i128 * trade.amount as i128)
        .sum();
    let avg_fill_price = if filled_amount > 0 {
        Some((turnover / filled_amount as i128) as i64)
    } else {
        None
    };
    let cumulative_fee: i64 = matching
        .iter()
        .map(|trade| {
            let is_buy = trade.buy_user_id == entry.user_id && trade.buy_order_id == entry.order_id;
            if is_buy {
                trade.taker_fee
            } else {
                trade.maker_fee
            }
        })
        .sum();
    let remaining_amount = open_order
        .map(|order| order.remaining_amount)
        .unwrap_or_else(|| entry.remaining_amount);
    let status = if let Some(order) = open_order {
        if order.remaining_amount < order.original_amount {
            "partial"
        } else {
            "open"
        }
    } else {
        match entry.status {
            OrderProjectionStatus::Open => "open",
            OrderProjectionStatus::PartiallyFilled => "partial",
            OrderProjectionStatus::Filled => "filled",
            OrderProjectionStatus::Cancelled => "cancelled",
            OrderProjectionStatus::Replaced => "replaced",
            OrderProjectionStatus::Rejected => "rejected",
            OrderProjectionStatus::ClosedNoFill => "closed_no_fill",
        }
    };

    serde_json::json!({
        "id": entry.order_id,
        "request_id": entry.request_id,
        "command_seq": entry.command_seq,
        "market_id": if entry.market_id.is_empty() { open_order.map(|value| value.market_id.clone()).unwrap_or_default() } else { entry.market_id.clone() },
        "outcome": entry.outcome,
        "side": entry.side,
        "order_type": entry.order_type,
        "time_in_force": entry.time_in_force,
        "price": entry.price.or_else(|| open_order.map(|value| value.price)),
        "amount": open_order.map(|value| value.original_amount).unwrap_or(entry.original_amount),
        "filled": open_order.map(|value| value.original_amount.saturating_sub(value.remaining_amount)).unwrap_or(filled_amount.max(entry.filled_amount)),
        "remaining": remaining_amount,
        "leverage": entry.leverage.or_else(|| open_order.and_then(|value| value.leverage)),
        "post_only": open_order.map(|value| value.post_only).unwrap_or(entry.post_only),
        "reduce_only": open_order.map(|value| value.reduce_only).unwrap_or(entry.reduce_only),
        "status": status,
        "source": if open_order.is_some() { "order_projection+open_orders" } else { "order_projection" },
        "fills": matching.len(),
        "avg_fill_price": avg_fill_price,
        "cumulative_fee": cumulative_fee,
        "replaces_order_id": entry.replaces_order_id,
        "replaced_by_order_id": entry.replaced_by_order_id,
        "close_reason": entry.close_reason,
        "created_at": entry.created_at,
        "updated_at": entry.updated_at,
    })
}

fn classify_user_account(
    user_id: &str,
    account_id: &str,
    balance: i64,
) -> Option<serde_json::Value> {
    let prefix = format!("U:{user_id}:");
    let suffix = account_id.strip_prefix(&prefix)?;
    let parts: Vec<&str> = suffix.split(':').collect();
    let kind = if parts.len() == 1 && parts[0] == "USDC" {
        "cash_available"
    } else if parts.len() == 2 && parts[0] == "USDC" && parts[1] == "HOLD" {
        "cash_locked"
    } else if parts.len() == 2 {
        "spot_position_available"
    } else if parts.len() == 3 && parts[2] == "HOLD" {
        "spot_position_locked"
    } else if parts.len() == 4 && parts[0] == "DERIV" {
        "derivative_position"
    } else if parts.len() == 5 && parts[0] == "ISO" && parts[4] == "USDC" {
        "isolated_margin"
    } else {
        "user_account"
    };
    Some(serde_json::json!({
        "account_id": account_id,
        "kind": kind,
        "balance": balance,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_account_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    order_projection: Arc<OrderStateProjectionStore>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    ledger: Arc<LedgerService>,
    index_prices: Arc<PersistentIndexPriceStore>,
    position_costs: Arc<PositionCostLedgerStore>,
    trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
    audit_store: Arc<RiskAutomationAuditStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let ledger_for_balances = ledger.clone();
    let ip_rate_limiter_for_balances = ip_rate_limiter.clone();
    let user_rate_limiter_for_balances = user_rate_limiter.clone();
    let balances_route = warp::path!("balances" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let ledger = ledger_for_balances.clone();
                let ip_rate_limiter = ip_rate_limiter_for_balances.clone();
                let user_rate_limiter = user_rate_limiter_for_balances.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    let resp = serde_json::json!([
                        {
                            "user_id": user_id,
                            "asset": "USDC",
                            "available": ledger.cash_available_balance(&user_id),
                            "hold": ledger.cash_hold_balance(&user_id),
                            "updated_at": Utc::now(),
                        }
                    ]);
                    Ok::<_, warp::Rejection>(warp::reply::json(&resp))
                }
            },
        )
        .boxed();
    let ledger_for_positions = ledger.clone();
    let ip_rate_limiter_for_positions = ip_rate_limiter.clone();
    let user_rate_limiter_for_positions = user_rate_limiter.clone();
    let positions_route = warp::path!("positions" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let ledger = ledger_for_positions.clone();
                let ip_rate_limiter = ip_rate_limiter_for_positions.clone();
                let user_rate_limiter = user_rate_limiter_for_positions.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    let balances = ledger.balances_for_user(&user_id);
                    let resp: Vec<_> = project_positions(&user_id, &balances)
                        .into_iter()
                        .map(|position| {
                            serde_json::json!({
                                "user_id": position.user_id,
                                "market_id": position.market_id,
                                "outcome": position.outcome,
                                "instrument_kind": position.instrument_kind,
                                "available": position.available,
                                "amount": position.available,
                                "hold": position.hold,
                                "net_qty": position.net_qty,
                                "updated_at": Utc::now(),
                            })
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&resp))
                }
            },
        )
        .boxed();
    let partitioned_engine_for_margin = partitioned_engine.clone();
    let risk_for_margin = risk.clone();
    let instruments_for_margin = instruments.clone();
    let index_prices_for_margin = index_prices.clone();
    let ip_rate_limiter_for_margin = ip_rate_limiter.clone();
    let user_rate_limiter_for_margin = user_rate_limiter.clone();
    let margin_route = warp::path!("margin" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<MarginQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: MarginQuery,
                  remote: Option<SocketAddr>| {
                let engine = partitioned_engine_for_margin.clone();
                let risk = risk_for_margin.clone();
                let instruments = instruments_for_margin.clone();
                let index_prices = index_prices_for_margin.clone();
                let ip_rate_limiter = ip_rate_limiter_for_margin.clone();
                let user_rate_limiter = user_rate_limiter_for_margin.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;

                    let instrument = instruments.resolve(&query.market_id);
                    if instrument.kind == InstrumentKind::Spot {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "margin projection requires margin or perpetual instrument",
                        ));
                    }

                    let outcome = query.outcome.unwrap_or(0);
                    let records = engine
                        .export_snapshots()
                        .await
                        .map_err(reject_internal_error)?;
                    let snapshots = flatten_market_snapshots(&records);
                    let mark_price = query
                        .mark_price
                        .or_else(|| {
                            resolve_mark_price_for_market(
                                &snapshots,
                                index_prices.as_ref(),
                                &instrument.instrument_id,
                                outcome,
                            )
                        })
                        .ok_or_else(|| {
                            reject_api(StatusCode::BAD_REQUEST, "mark price unavailable for market")
                        })?;
                    let projection = project_margin(
                        risk.as_ref(),
                        &user_id,
                        &instrument,
                        outcome,
                        mark_price,
                        query.leverage.or(instrument.max_leverage),
                        query.maintenance_margin_bps.unwrap_or(1_000),
                    )
                    .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
                    let estimated_liquidation_price = risk
                        .bankruptcy_reference_price_details(
                            &user_id,
                            &instrument,
                            outcome,
                            mark_price,
                        )
                        .map(|d| d.maintenance_reference_price);
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "user_id": projection.user_id,
                        "market_id": projection.market_id,
                        "outcome": projection.outcome,
                        "instrument_kind": instrument.kind,
                        "collateral_total": projection.collateral_total,
                        "position_qty": projection.position_qty,
                        "mark_price": projection.mark_price,
                        "notional": projection.notional,
                        "initial_margin_required": projection.initial_margin_required,
                        "maintenance_margin_required": projection.maintenance_margin_required,
                        "margin_ratio_bps": projection.margin_ratio_bps,
                        "liquidation_required": projection.liquidation_required,
                        "estimated_liquidation_price": estimated_liquidation_price,
                        "updated_at": Utc::now(),
                    })))
                }
            },
        )
        .boxed();
    let partitioned_engine_for_pnl = partitioned_engine.clone();
    let risk_for_pnl = risk.clone();
    let instruments_for_pnl = instruments.clone();
    let index_prices_for_pnl = index_prices.clone();
    let position_costs_for_pnl = position_costs.clone();
    let trade_journal_wal_for_pnl = trade_journal_wal.clone();
    let ip_rate_limiter_for_pnl = ip_rate_limiter.clone();
    let user_rate_limiter_for_pnl = user_rate_limiter.clone();
    let pnl_route = warp::path!("pnl" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<PnlQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: PnlQuery,
                  remote: Option<SocketAddr>| {
                let engine = partitioned_engine_for_pnl.clone();
                let risk = risk_for_pnl.clone();
                let instruments = instruments_for_pnl.clone();
                let index_prices = index_prices_for_pnl.clone();
                let position_costs = position_costs_for_pnl.clone();
                let _trade_journal_wal = trade_journal_wal_for_pnl.clone();
                let ip_rate_limiter = ip_rate_limiter_for_pnl.clone();
                let user_rate_limiter = user_rate_limiter_for_pnl.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;

                    let instrument = instruments.resolve(&query.market_id);
                    let outcome = query.outcome.unwrap_or(0);
                    let records = engine
                        .export_snapshots()
                        .await
                        .map_err(reject_internal_error)?;
                    let snapshots = flatten_market_snapshots(&records);
                    let mark_price = query
                        .mark_price
                        .or_else(|| {
                            resolve_mark_price_for_market(
                                &snapshots,
                                index_prices.as_ref(),
                                &instrument.instrument_id,
                                outcome,
                            )
                        })
                        .ok_or_else(|| {
                            reject_api(StatusCode::BAD_REQUEST, "mark price unavailable for market")
                        })?;
                    let derived_entry_price = if query.entry_price.is_none() {
                        position_costs
                            .get(&user_id, &instrument.instrument_id, outcome)
                            .and_then(|entry| entry.entry_price)
                    } else {
                        None
                    };
                    let balances = risk.ledger().balances_for_user(&user_id);
                    let position_qty = project_positions(&user_id, &balances)
                        .into_iter()
                        .find(|position| {
                            position.market_id == instrument.instrument_id
                                && position.outcome == outcome
                        })
                        .map(|position| match position.instrument_kind {
                            InstrumentKind::Spot => {
                                position.available.saturating_add(position.hold)
                            }
                            _ => position.net_qty,
                        })
                        .unwrap_or(0);
                    let projection = project_pnl(
                        &user_id,
                        &instrument.instrument_id,
                        outcome,
                        position_qty,
                        derived_entry_price,
                        mark_price,
                    );
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "user_id": projection.user_id,
                        "market_id": projection.market_id,
                        "outcome": projection.outcome,
                        "instrument_kind": instrument.kind,
                        "position_qty": projection.position_qty,
                        "entry_price": projection.entry_price,
                        "mark_price": projection.mark_price,
                        "unrealized_pnl": projection.unrealized_pnl,
                        "updated_at": Utc::now(),
                    })))
                }
            },
        )
        .boxed();
    let partitioned_engine_for_orders = partitioned_engine.clone();
    let ip_rate_limiter_for_orders = ip_rate_limiter.clone();
    let user_rate_limiter_for_orders = user_rate_limiter.clone();
    let orders_route = warp::path!("orders" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<OrdersQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: OrdersQuery,
                  remote: Option<SocketAddr>| {
                let engine = partitioned_engine_for_orders.clone();
                let ip_rate_limiter = ip_rate_limiter_for_orders.clone();
                let user_rate_limiter = user_rate_limiter_for_orders.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    let records = engine
                        .export_snapshots()
                        .await
                        .map_err(reject_internal_error)?;
                    let snapshots = flatten_market_snapshots(&records);
                    let orders = snapshots_to_orders(
                        &snapshots,
                        &user_id,
                        query.market_id.as_deref(),
                        query.outcome,
                    );
                    Ok::<_, warp::Rejection>(warp::reply::json(&orders))
                }
            },
        )
        .boxed();
    let partitioned_engine_for_order_lookup = partitioned_engine.clone();
    let sequencer_for_order_lookup = sequencer.clone();
    let order_projection_for_order_lookup = order_projection.clone();
    let trade_journal_for_order_lookup = trade_journal_wal.clone();
    let ip_rate_limiter_for_order_lookup = ip_rate_limiter.clone();
    let user_rate_limiter_for_order_lookup = user_rate_limiter.clone();
    let order_lookup_route = warp::path!("orders" / String / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<OrderLookupQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  order_id: String,
                  principal: AuthenticatedPrincipal,
                  query: OrderLookupQuery,
                  remote: Option<SocketAddr>| {
                let engine = partitioned_engine_for_order_lookup.clone();
                let sequencer = sequencer_for_order_lookup.clone();
                let order_projection = order_projection_for_order_lookup.clone();
                let trade_journal = trade_journal_for_order_lookup.clone();
                let ip_rate_limiter = ip_rate_limiter_for_order_lookup.clone();
                let user_rate_limiter = user_rate_limiter_for_order_lookup.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    let records = engine
                        .export_snapshots()
                        .await
                        .map_err(reject_internal_error)?;
                    let snapshots = flatten_market_snapshots(&records);
                    let open_order = snapshots
                        .iter()
                        .filter(|snapshot| {
                            query
                                .market_id
                                .as_deref()
                                .is_none_or(|market_id| snapshot.market_id == market_id)
                                && query
                                    .outcome
                                    .is_none_or(|outcome| snapshot.outcome == outcome)
                        })
                        .flat_map(|snapshot| snapshot.orders.iter())
                        .find(|order| order.user_id == user_id && order.order_id == order_id);
                    let trades = trade_journal.entries().map_err(reject_internal_error)?;
                    if let Some(entry) = order_projection.get(&user_id, &order_id) {
                        return Ok::<_, warp::Rejection>(warp::reply::json(
                            &projected_order_to_json(&entry, open_order, &trades),
                        ));
                    }
                    if let Some(open_order) = open_order {
                        return Ok::<_, warp::Rejection>(warp::reply::json(
                            &snapshot_order_to_json(open_order),
                        ));
                    }
                    if let Some(historical) =
                        build_order_lookup_from_trades(&trades, &user_id, &order_id)
                    {
                        return Ok::<_, warp::Rejection>(warp::reply::json(&historical));
                    }
                    let reconstructed = build_order_lookup_from_commands(
                        &sequencer.latest_records(),
                        &user_id,
                        &order_id,
                    )
                    .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "order not found"))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&reconstructed))
                }
            },
        )
        .boxed();
    let ledger_for_ledger_view = ledger.clone();
    let ip_rate_limiter_for_ledger_view = ip_rate_limiter.clone();
    let user_rate_limiter_for_ledger_view = user_rate_limiter.clone();
    let ledger_view_route = warp::path!("ledger" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<LedgerViewQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: LedgerViewQuery,
                  remote: Option<SocketAddr>| {
                let ledger = ledger_for_ledger_view.clone();
                let ip_rate_limiter = ip_rate_limiter_for_ledger_view.clone();
                let user_rate_limiter = user_rate_limiter_for_ledger_view.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;

                    let include_zero = query.include_zero.unwrap_or(false);
                    let raw_balances = ledger.balances_for_user(&user_id);
                    let mut raw_accounts: Vec<_> = raw_balances
                        .iter()
                        .filter(|(_, balance)| include_zero || **balance != 0)
                        .filter_map(|(account_id, balance)| {
                            classify_user_account(&user_id, account_id, *balance)
                        })
                        .collect();
                    raw_accounts.sort_by(|lhs, rhs| {
                        lhs["account_id"].as_str().cmp(&rhs["account_id"].as_str())
                    });

                    let balances = ledger.balances_for_user(&user_id);
                    let positions: Vec<_> = project_positions(&user_id, &balances)
                        .into_iter()
                        .filter(|position| {
                            include_zero
                                || position.available != 0
                                || position.hold != 0
                                || position.net_qty != 0
                        })
                        .map(|position| {
                            serde_json::json!({
                                "market_id": position.market_id,
                                "outcome": position.outcome,
                                "instrument_kind": position.instrument_kind,
                                "available": position.available,
                                "locked": position.hold,
                                "net_qty": position.net_qty,
                            })
                        })
                        .collect();

                    let isolated_margin: Vec<_> = raw_balances
                        .iter()
                        .filter_map(|(account_id, balance)| {
                            if !account_id.starts_with(&format!("U:{user_id}:ISO:")) {
                                return None;
                            }
                            if !include_zero && *balance == 0 {
                                return None;
                            }
                            let parts: Vec<&str> = account_id.split(':').collect();
                            if parts.len() != 6 {
                                return None;
                            }
                            Some(serde_json::json!({
                                "account_id": account_id,
                                "market_id": parts[3],
                                "outcome": parts[4].parse::<i32>().ok(),
                                "balance": balance,
                            }))
                        })
                        .collect();

                    let fee_account = if principal.role == PrincipalRole::Admin {
                        serde_json::json!({
                            "account_id": "SYS:FEE_COLLECTOR:USDC",
                            "balance": ledger.fee_collector_balance(),
                        })
                    } else {
                        serde_json::json!({
                            "account_id": "SYS:FEE_COLLECTOR:USDC",
                        })
                    };

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "user_id": user_id,
                        "cash": {
                            "asset": "USDC",
                            "available": ledger.cash_available_balance(&user_id),
                            "locked": ledger.cash_hold_balance(&user_id),
                            "total": ledger.cash_available_balance(&user_id)
                                .saturating_add(ledger.cash_hold_balance(&user_id)),
                        },
                        "positions": positions,
                        "isolated_margin": isolated_margin,
                        "fee_account": fee_account,
                        "raw_accounts": raw_accounts,
                        "updated_at": Utc::now(),
                    })))
                }
            },
        )
        .boxed();
    let ledger_for_deposits = ledger.clone();
    let ip_rate_limiter_for_deposits = ip_rate_limiter.clone();
    let user_rate_limiter_for_deposits = user_rate_limiter.clone();
    let deposits_route = warp::path!("deposits" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let ledger = ledger_for_deposits.clone();
                let ip_rate_limiter = ip_rate_limiter_for_deposits.clone();
                let user_rate_limiter = user_rate_limiter_for_deposits.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    let entries = ledger.wal_entries().map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&deposits_from_ledger(
                        &user_id, &entries,
                    )))
                }
            },
        )
        .boxed();
    let position_costs_for_get = position_costs.clone();
    let ip_rate_limiter_for_costs = ip_rate_limiter.clone();
    let user_rate_limiter_for_costs = user_rate_limiter.clone();
    let position_costs_route = warp::path!("position-costs" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let position_costs = position_costs_for_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_costs.clone();
                let user_rate_limiter = user_rate_limiter_for_costs.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(
                        &position_costs.list_for_user(&user_id),
                    ))
                }
            },
        )
        .boxed();
    let trade_journal_for_fills = trade_journal_wal.clone();
    let ip_rate_limiter_for_fills = ip_rate_limiter.clone();
    let user_rate_limiter_for_fills = user_rate_limiter.clone();
    let fills_route = warp::path!("fills" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<FillsQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: FillsQuery,
                  remote: Option<SocketAddr>| {
                let trade_journal = trade_journal_for_fills.clone();
                let ip_rate_limiter = ip_rate_limiter_for_fills.clone();
                let user_rate_limiter = user_rate_limiter_for_fills.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    let limit = query.limit.unwrap_or(50).clamp(1, 500);
                    let mut fills: Vec<serde_json::Value> = trade_journal
                        .entries()
                        .map_err(reject_internal_error)?
                        .into_iter()
                        .filter(|trade| {
                            trade.buy_user_id == user_id || trade.sell_user_id == user_id
                        })
                        .filter(|trade| {
                            query.market_id.as_deref().is_none_or(|m| trade.market_id == m)
                        })
                        .filter(|trade| {
                            query.outcome.is_none_or(|o| trade.outcome == o)
                        })
                        .map(|trade| {
                            let is_buyer = trade.buy_user_id == user_id;
                            let side = if is_buyer { "buy" } else { "sell" };
                            let fee = if is_buyer { trade.taker_fee } else { trade.maker_fee };
                            let order_id = if is_buyer {
                                &trade.buy_order_id
                            } else {
                                &trade.sell_order_id
                            };
                            serde_json::json!({
                                "trade_id": trade.trade_id,
                                "market_id": trade.market_id,
                                "outcome": trade.outcome,
                                "side": side,
                                "price": trade.price,
                                "amount": trade.amount,
                                "fee": fee,
                                "order_id": order_id,
                                "counterparty": if is_buyer { &trade.sell_user_id } else { &trade.buy_user_id },
                                "timestamp": trade.recorded_at,
                            })
                        })
                        .collect();
                    fills.sort_by(|lhs, rhs| {
                        rhs["timestamp"].to_string().cmp(&lhs["timestamp"].to_string())
                    });
                    fills.truncate(limit);
                    Ok::<_, warp::Rejection>(warp::reply::json(&fills))
                }
            },
        )
        .boxed();
    // GET /order-history/{user_id} — closed/filled order history from trade journal
    let trade_journal_for_history = trade_journal_wal.clone();
    let ip_rate_limiter_for_history = ip_rate_limiter.clone();
    let user_rate_limiter_for_history = user_rate_limiter.clone();
    let order_history_route = warp::path!("order-history" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<OrderHistoryQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: OrderHistoryQuery,
                  remote: Option<SocketAddr>| {
                let trade_journal = trade_journal_for_history.clone();
                let ip_rate_limiter = ip_rate_limiter_for_history.clone();
                let user_rate_limiter = user_rate_limiter_for_history.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;

                    let limit = query.limit.unwrap_or(50).clamp(1, 500);
                    let trades = trade_journal.entries().map_err(reject_internal_error)?;

                    // Cap processing to avoid OOM on large WALs — process newest entries first
                    // and stop after we've filled the limit for the requested user.
                    const MAX_TRADES_TO_SCAN: usize = 10_000;
                    let scan_window = trades.iter().rev().take(MAX_TRADES_TO_SCAN);

                    // Build a map: order_id → aggregated info
                    let mut order_map: std::collections::HashMap<String, serde_json::Value> =
                        std::collections::HashMap::new();

                    for trade in scan_window {
                        let is_buyer = trade.buy_user_id == user_id;
                        let is_seller = trade.sell_user_id == user_id;
                        if !is_buyer && !is_seller {
                            continue;
                        }
                        if let Some(ref m) = query.market_id {
                            if trade.market_id != *m {
                                continue;
                            }
                        }
                        if let Some(o) = query.outcome {
                            if trade.outcome != o {
                                continue;
                            }
                        }

                        let sides: Vec<(&str, &str, i64)> = {
                            let mut v = Vec::new();
                            if is_buyer {
                                v.push(("buy", trade.buy_order_id.as_str(), trade.taker_fee));
                            }
                            if is_seller {
                                v.push(("sell", trade.sell_order_id.as_str(), trade.maker_fee));
                            }
                            v
                        };

                        for (side, order_id, fee) in sides {
                            if let Some(ref s) = query.side {
                                if *s != side {
                                    continue;
                                }
                            }
                            let entry =
                                order_map.entry(order_id.to_string()).or_insert_with(|| {
                                    serde_json::json!({
                                        "order_id": order_id,
                                        "market_id": trade.market_id,
                                        "outcome": trade.outcome,
                                        "side": side,
                                        "status": "filled",
                                        "fills": 0_i64,
                                        "total_amount": 0_i64,
                                        "total_fee": 0_i64,
                                        "avg_price": 0_i64,
                                        "total_notional": 0_i64,
                                        "first_fill_at": trade.recorded_at,
                                        "last_fill_at": trade.recorded_at,
                                    })
                                });
                            let obj = entry.as_object_mut().ok_or_else(|| {
                                reject_api(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    "unexpected non-object entry in order map",
                                )
                            })?;
                            let fills = obj["fills"].as_i64().unwrap_or(0) + 1;
                            obj.insert("fills".into(), serde_json::json!(fills));
                            let total_amount = obj["total_amount"]
                                .as_i64()
                                .unwrap_or(0)
                                .saturating_add(trade.amount);
                            obj.insert("total_amount".into(), serde_json::json!(total_amount));
                            let total_fee =
                                obj["total_fee"].as_i64().unwrap_or(0).saturating_add(fee);
                            obj.insert("total_fee".into(), serde_json::json!(total_fee));
                            let trade_notional = trade.price.saturating_mul(trade.amount);
                            let total_notional = obj["total_notional"]
                                .as_i64()
                                .unwrap_or(0)
                                .saturating_add(trade_notional);
                            obj.insert("total_notional".into(), serde_json::json!(total_notional));
                            if total_amount > 0 {
                                obj.insert(
                                    "avg_price".into(),
                                    serde_json::json!(total_notional / total_amount),
                                );
                            }
                            obj.insert("last_fill_at".into(), serde_json::json!(trade.recorded_at));
                        }
                    }

                    let mut items: Vec<_> = order_map.into_values().collect();
                    items.sort_by(|a, b| {
                        b["last_fill_at"]
                            .to_string()
                            .cmp(&a["last_fill_at"].to_string())
                    });
                    items.truncate(limit);
                    Ok::<_, warp::Rejection>(warp::reply::json(&items))
                }
            },
        )
        .boxed();

    let audit_store_for_funding = audit_store.clone();
    let ip_rate_limiter_for_funding = ip_rate_limiter.clone();
    let user_rate_limiter_for_funding = user_rate_limiter.clone();
    let funding_history_route = warp::path!("funding-history" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<FundingHistoryQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: FundingHistoryQuery,
                  remote: Option<SocketAddr>| {
                let audit_store = audit_store_for_funding.clone();
                let ip_rate_limiter = ip_rate_limiter_for_funding.clone();
                let user_rate_limiter = user_rate_limiter_for_funding.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    let limit = query.limit.unwrap_or(100).min(1000);
                    let records = audit_store
                        .list_funding_for_user(
                            &user_id,
                            query.market_id.as_deref(),
                            query.outcome,
                            limit,
                        )
                        .map_err(reject_internal_error)?;
                    let items: Vec<serde_json::Value> = records
                        .into_iter()
                        .map(|record| {
                            let role = if record.user_id.as_deref() == Some(user_id.as_str()) {
                                "payer"
                            } else {
                                "receiver"
                            };
                            serde_json::json!({
                                "event_id": record.event_id,
                                "market_id": record.market_id,
                                "outcome": record.outcome,
                                "role": role,
                                "payer_user_id": record.user_id,
                                "receiver_user_id": record.counterparty_user_id,
                                "settlement": record.detail.get("settlement"),
                                "recorded_at": record.recorded_at,
                            })
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&items))
                }
            },
        )
        .boxed();

    // ── Trade export (CSV / JSON) ────────────────────────────
    let export_journal = trade_journal_wal.clone();
    let export_ip_rl = ip_rate_limiter.clone();
    let export_user_rl = user_rate_limiter.clone();
    let trade_export_route = warp::path!("export" / "trades" / String)
        .and(warp::get())
        .and(with_principal())
        .and(warp::query::<TradeExportQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: TradeExportQuery,
                  remote: Option<SocketAddr>| {
                let journal = export_journal.clone();
                let ip_rl = export_ip_rl.clone();
                let user_rl = export_user_rl.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".into());
                    ip_rl.check(&format!("ip:{ip_key}"), 10)?;
                    user_rl.check(&format!("user-export:{}", principal.subject), 5)?;
                    let trades: Vec<TradeJournalRecord> = journal
                        .entries()
                        .map_err(reject_internal_error)?
                        .into_iter()
                        .filter(|t| t.buy_user_id == user_id || t.sell_user_id == user_id)
                        .filter(|t| query.market_id.as_deref().is_none_or(|m| t.market_id == m))
                        .filter(|t| query.from.is_none_or(|from| t.recorded_at >= from))
                        .filter(|t| query.to.is_none_or(|to| t.recorded_at <= to))
                        .collect();

                    let format = query.format.as_deref().unwrap_or("json");
                    if format == "csv" {
                        let mut csv = String::from(
                            "trade_id,market_id,outcome,side,price,amount,fee,order_id,timestamp\n",
                        );
                        for t in &trades {
                            let is_buyer = t.buy_user_id == user_id;
                            let side = if is_buyer { "buy" } else { "sell" };
                            let fee = if is_buyer { t.taker_fee } else { t.maker_fee };
                            let order_id = if is_buyer {
                                &t.buy_order_id
                            } else {
                                &t.sell_order_id
                            };
                            use std::fmt::Write;
                            let _ = writeln!(
                                csv,
                                "{},{},{},{},{},{},{},{},{}",
                                t.trade_id,
                                t.market_id,
                                t.outcome,
                                side,
                                t.price,
                                t.amount,
                                fee,
                                order_id,
                                t.recorded_at.to_rfc3339(),
                            );
                        }
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "format": "csv",
                            "count": trades.len(),
                            "csv": csv,
                        })))
                    } else {
                        let items: Vec<serde_json::Value> = trades
                            .iter()
                            .map(|t| {
                                let is_buyer = t.buy_user_id == user_id;
                                let side = if is_buyer { "buy" } else { "sell" };
                                let fee = if is_buyer { t.taker_fee } else { t.maker_fee };
                                let order_id = if is_buyer {
                                    &t.buy_order_id
                                } else {
                                    &t.sell_order_id
                                };
                                serde_json::json!({
                                    "trade_id": t.trade_id,
                                    "market_id": t.market_id,
                                    "outcome": t.outcome,
                                    "side": side,
                                    "price": t.price,
                                    "amount": t.amount,
                                    "fee": fee,
                                    "order_id": order_id,
                                    "timestamp": t.recorded_at,
                                })
                            })
                            .collect();
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "format": "json",
                            "count": items.len(),
                            "trades": items,
                        })))
                    }
                }
            },
        )
        .boxed();

    // ── Leverage adjustment ────────────────────────────────
    let leverage_instruments = instruments.clone();
    let leverage_ip_rl = ip_rate_limiter.clone();
    let leverage_user_rl = user_rate_limiter.clone();
    let leverage_store: Arc<DashMap<String, serde_json::Value>> = Arc::new(DashMap::new());
    let leverage_store_get = leverage_store.clone();

    let set_leverage_route = warp::path!("leverage" / String)
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: LeverageAdjustRequest| {
                let instruments = leverage_instruments.clone();
                let ip_rl = leverage_ip_rl.clone();
                let user_rl = leverage_user_rl.clone();
                let store = leverage_store.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".into());
                    ip_rl.check(&format!("ip:{ip_key}"), 30)?;
                    user_rl.check(&format!("user-write:{}", principal.subject), 10)?;
                    if req.leverage == 0 {
                        return Err(reject_api(StatusCode::BAD_REQUEST, "leverage must be >= 1"));
                    }
                    let spec = instruments
                        .get(&req.market_id)
                        .ok_or_else(|| reject_api(StatusCode::BAD_REQUEST, "unknown market_id"))?;
                    if let Some(max_lev) = spec.max_leverage {
                        if req.leverage > max_lev {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                format!("leverage exceeds max {max_lev} for this market"),
                            ));
                        }
                    }
                    let key = format!("{user_id}:{}", req.market_id);
                    let entry = serde_json::json!({
                        "user_id": user_id,
                        "market_id": req.market_id,
                        "leverage": req.leverage,
                        "updated_at": Utc::now(),
                    });
                    store.insert(key, entry.clone());
                    Ok::<_, warp::Rejection>(warp::reply::json(&entry))
                }
            },
        )
        .boxed();

    let leverage_ip_rl2 = ip_rate_limiter.clone();
    let leverage_user_rl2 = user_rate_limiter.clone();
    let get_leverage_route = warp::path!("leverage" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let ip_rl = leverage_ip_rl2.clone();
                let user_rl = leverage_user_rl2.clone();
                let store = leverage_store_get.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".into());
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-read:{}", principal.subject), 30)?;
                    let items: Vec<serde_json::Value> = store
                        .iter()
                        .filter(|entry| entry.key().starts_with(&format!("{user_id}:")))
                        .map(|entry| entry.value().clone())
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&items))
                }
            },
        )
        .boxed();

    balances_route
        .or(positions_route)
        .unify()
        .or(margin_route)
        .unify()
        .or(pnl_route)
        .unify()
        .or(orders_route)
        .unify()
        .or(order_lookup_route)
        .unify()
        .or(order_history_route)
        .unify()
        .or(fills_route)
        .unify()
        .or(ledger_view_route)
        .unify()
        .or(deposits_route)
        .unify()
        .or(position_costs_route)
        .unify()
        .or(funding_history_route)
        .unify()
        .or(trade_export_route)
        .unify()
        .or(set_leverage_route)
        .unify()
        .or(get_leverage_route)
        .unify()
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequenced_new_order(
        request_id: &str,
        order_id: &str,
        lifecycle: types::CommandLifecycle,
    ) -> SequencedCommandRecord {
        let mut metadata = CommandMetadata::new(request_id);
        metadata.command_seq = Some(7);
        metadata.advance(lifecycle);
        SequencedCommandRecord {
            request_id: request_id.to_string(),
            command_seq: 7,
            command: Command::NewOrder(NewOrderCommand {
                metadata,
                client_order_id: order_id.to_string(),
                user_id: "user-1".to_string(),
                session_id: Some("sess-1".to_string()),
                market_id: "btc-usdt".to_string(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                time_in_force: TimeInForce::Gtc,
                price: Some(50_000),
                amount: 2,
                outcome: 0,
                post_only: false,
                reduce_only: false,
                leverage: None,
                expires_at: None,
                stp_mode: types::StpMode::CancelTaker,
                trigger_price: None,
                trigger_type: None,
                display_qty: None,
                min_fill_qty: None,
                stp_group_id: None,
                is_market_maker: false,
            }),
            recorded_at: Utc::now(),
        }
    }

    fn sequenced_cancel(request_id: &str, order_id: &str) -> SequencedCommandRecord {
        let mut metadata = CommandMetadata::new(request_id);
        metadata.command_seq = Some(8);
        metadata.advance(types::CommandLifecycle::Completed);
        SequencedCommandRecord {
            request_id: request_id.to_string(),
            command_seq: 8,
            command: Command::CancelOrder(CancelOrderCommand {
                metadata,
                user_id: "user-1".to_string(),
                market_id: "btc-usdt".to_string(),
                outcome: Some(0),
                order_id: order_id.to_string(),
                client_order_id: Some(order_id.to_string()),
            }),
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn order_lookup_reconstructs_cancelled_unfilled_order_from_sequencer() {
        let records = vec![
            sequenced_new_order("req-order", "ord-1", types::CommandLifecycle::Completed),
            sequenced_cancel("req-cancel", "ord-1"),
        ];
        let payload = build_order_lookup_from_commands(&records, "user-1", "ord-1")
            .expect("reconstructed order");
        assert_eq!(payload["status"], "cancelled");
        assert_eq!(payload["source"], "sequencer_log");
        assert_eq!(payload["remaining"], 2);
        assert_eq!(payload["filled"], 0);
    }

    #[test]
    fn order_lookup_reconstructs_rejected_order_from_sequencer() {
        let records = vec![sequenced_new_order(
            "req-order",
            "ord-reject",
            types::CommandLifecycle::Rejected,
        )];
        let payload = build_order_lookup_from_commands(&records, "user-1", "ord-reject")
            .expect("reconstructed order");
        assert_eq!(payload["status"], "rejected");
        assert_eq!(payload["close_reason"], "sequencer_rejected");
    }
}
