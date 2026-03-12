use super::*;

pub(crate) fn build_account_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    ledger: Arc<LedgerService>,
    index_prices: Arc<PersistentIndexPriceStore>,
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
                        "updated_at": Utc::now(),
                    })))
                }
            },
        );
    let partitioned_engine_for_pnl = partitioned_engine.clone();
    let risk_for_pnl = risk.clone();
    let instruments_for_pnl = instruments.clone();
    let index_prices_for_pnl = index_prices.clone();
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
                            InstrumentKind::Margin | InstrumentKind::Perpetual => position.net_qty,
                        })
                        .unwrap_or(0);
                    let projection = project_pnl(
                        &user_id,
                        &instrument.instrument_id,
                        outcome,
                        position_qty,
                        query.entry_price,
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
    balances_route
        .or(positions_route)
        .unify()
        .or(margin_route)
        .unify()
        .or(pnl_route)
        .unify()
        .or(orders_route)
        .unify()
        .or(deposits_route)
        .unify()
        .boxed()
}
