use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_account_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
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
        );
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
        );
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
                    let records = engine.export_snapshots().await.map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
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
        );
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
                    let records = engine.export_snapshots().await.map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
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
        );
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
                    let records = engine.export_snapshots().await.map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
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
        );
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
                    let entries = ledger.wal_entries().map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&deposits_from_ledger(
                        &user_id, &entries,
                    )))
                }
            },
        );
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
        );
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
                        .map_err(|error| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                        })?
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
        );
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
                    let trades = trade_journal.entries().map_err(|e| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                    })?;

                    // Build a map: order_id → aggregated info
                    let mut order_map: std::collections::HashMap<String, serde_json::Value> =
                        std::collections::HashMap::new();

                    for trade in &trades {
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
                            let obj = entry.as_object_mut().unwrap();
                            let fills = obj["fills"].as_i64().unwrap_or(0) + 1;
                            obj.insert("fills".into(), serde_json::json!(fills));
                            let total_amount =
                                obj["total_amount"].as_i64().unwrap_or(0) + trade.amount;
                            obj.insert("total_amount".into(), serde_json::json!(total_amount));
                            let total_fee = obj["total_fee"].as_i64().unwrap_or(0) + fee;
                            obj.insert("total_fee".into(), serde_json::json!(total_fee));
                            let total_notional = obj["total_notional"].as_i64().unwrap_or(0)
                                + trade.price.saturating_mul(trade.amount);
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
        );

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
                        .map_err(|e| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                        })?;
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
        );

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
                        .map_err(|e| reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        );

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
        );

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
        );

    balances_route
        .or(positions_route)
        .unify()
        .or(margin_route)
        .unify()
        .or(pnl_route)
        .unify()
        .or(orders_route)
        .unify()
        .or(order_history_route)
        .unify()
        .or(fills_route)
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
