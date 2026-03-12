use super::*;

pub(crate) fn build_market_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
    ledger: Arc<LedgerService>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let partitioned_engine_for_markets = partitioned_engine.clone();
    let ip_rate_limiter_for_markets = ip_rate_limiter.clone();
    let markets_route = warp::path("markets")
        .and(warp::path::end())
        .and(warp::get())
        .and(remote_ip())
        .and_then(move |remote: Option<SocketAddr>| {
            let engine = partitioned_engine_for_markets.clone();
            let ip_rate_limiter = ip_rate_limiter_for_markets.clone();
            async move {
                let ip_key = remote
                    .map(|value| value.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                let records = engine.export_snapshots().await.map_err(|error| {
                    reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
                let snapshots = flatten_market_snapshots(&records);
                Ok::<_, warp::Rejection>(warp::reply::json(&snapshots_to_market_list(&snapshots)))
            }
        });
    let partitioned_engine_for_market_detail = partitioned_engine.clone();
    let ip_rate_limiter_for_market_detail = ip_rate_limiter.clone();
    let market_detail_route = warp::path!("markets" / String)
        .and(warp::path::end())
        .and(warp::get())
        .and(remote_ip())
        .and_then(move |market_id: String, remote: Option<SocketAddr>| {
            let engine = partitioned_engine_for_market_detail.clone();
            let ip_rate_limiter = ip_rate_limiter_for_market_detail.clone();
            async move {
                let ip_key = remote
                    .map(|value| value.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                let records = engine.export_snapshots().await.map_err(|error| {
                    reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
                let snapshots = flatten_market_snapshots(&records);
                let market = snapshots_to_market_list(&snapshots)
                    .into_iter()
                    .find(|entry| entry["market_id"] == market_id)
                    .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "market not found"))?;
                Ok::<_, warp::Rejection>(warp::reply::json(&market))
            }
        });
    let partitioned_engine_for_book = partitioned_engine.clone();
    let ip_rate_limiter_for_book = ip_rate_limiter.clone();
    let book_route = warp::path!("markets" / String / "book")
        .and(warp::get())
        .and(optional_query::<BookQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String, query: BookQuery, remote: Option<SocketAddr>| {
                let engine = partitioned_engine_for_book.clone();
                let ip_rate_limiter = ip_rate_limiter_for_book.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    let outcome = query.outcome.unwrap_or(0);
                    let depth = query.depth.unwrap_or(20).clamp(1, 200);
                    let records = engine.export_snapshots().await.map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    let snapshots = flatten_market_snapshots(&records);
                    let snapshot = snapshots
                        .into_iter()
                        .find(|snapshot| {
                            snapshot.market_id == market_id && snapshot.outcome == outcome
                        })
                        .ok_or_else(|| {
                            reject_api(StatusCode::NOT_FOUND, "market book not found")
                        })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&snapshot_to_order_book(
                        &snapshot, depth,
                    )))
                }
            },
        );
    let trade_journal_for_trades = trade_journal_wal.clone();
    let ip_rate_limiter_for_trades = ip_rate_limiter.clone();
    let user_rate_limiter_for_trades = user_rate_limiter.clone();
    let trades_route = warp::path("trades")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_optional_principal())
        .and(optional_query::<TradesQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: Option<AuthenticatedPrincipal>,
                  query: TradesQuery,
                  remote: Option<SocketAddr>| {
                let trade_journal = trade_journal_for_trades.clone();
                let ip_rate_limiter = ip_rate_limiter_for_trades.clone();
                let user_rate_limiter = user_rate_limiter_for_trades.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    if let Some(user_id) = query.user_id.as_deref() {
                        let principal = principal.ok_or_else(|| {
                            reject_api(StatusCode::UNAUTHORIZED, "missing internal auth headers")
                        })?;
                        ensure_subject_or_admin(&principal, user_id)?;
                        user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    }
                    let limit = query.limit.unwrap_or(50).clamp(1, 500);
                    let mut trades: Vec<_> = trade_journal
                        .entries()
                        .map_err(|error| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                        })?
                        .into_iter()
                        .filter(|trade| {
                            query
                                .market_id
                                .as_deref()
                                .is_none_or(|market_id| trade.market_id == market_id)
                        })
                        .filter(|trade| {
                            query.outcome.is_none_or(|outcome| trade.outcome == outcome)
                        })
                        .filter(|trade| {
                            query.user_id.as_deref().is_none_or(|user_id| {
                                trade.buy_user_id == user_id || trade.sell_user_id == user_id
                            })
                        })
                        .collect();
                    trades.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));
                    trades.truncate(limit);
                    let payload: Vec<_> = trades.iter().map(trade_record_to_json).collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&payload))
                }
            },
        );
    let trade_journal_for_history = trade_journal_wal.clone();
    let ip_rate_limiter_for_history = ip_rate_limiter.clone();
    let history_route = warp::path!("markets" / String / "history")
        .and(warp::get())
        .and(optional_query::<HistoryQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String, query: HistoryQuery, remote: Option<SocketAddr>| {
                let trade_journal = trade_journal_for_history.clone();
                let ip_rate_limiter = ip_rate_limiter_for_history.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    let limit = query.limit.unwrap_or(24).clamp(1, 500);
                    let trades = trade_journal.entries().map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&trades_to_history(
                        &market_id,
                        query.outcome,
                        &trades,
                        limit,
                    )))
                }
            },
        );
    let partitioned_engine_for_stats = partitioned_engine.clone();
    let trade_journal_for_stats = trade_journal_wal.clone();
    let ledger_for_stats = ledger.clone();
    let ip_rate_limiter_for_stats = ip_rate_limiter.clone();
    let stats_route = warp::path("stats")
        .and(warp::path::end())
        .and(warp::get())
        .and(remote_ip())
        .and_then(move |remote: Option<SocketAddr>| {
            let engine = partitioned_engine_for_stats.clone();
            let trade_journal = trade_journal_for_stats.clone();
            let ledger = ledger_for_stats.clone();
            let ip_rate_limiter = ip_rate_limiter_for_stats.clone();
            async move {
                let ip_key = remote
                    .map(|value| value.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                let records = engine.export_snapshots().await.map_err(|error| {
                    reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
                let snapshots = flatten_market_snapshots(&records);
                let trades = trade_journal.entries().map_err(|error| {
                    reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
                let entries = ledger.wal_entries().map_err(|error| {
                    reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
                Ok::<_, warp::Rejection>(warp::reply::json(&stats_from_snapshots_and_trades(
                    &snapshots, &trades, &entries,
                )))
            }
        });
    let partitioned_engine_2 = partitioned_engine.clone();
    let ip_rate_limiter_for_matching_status = ip_rate_limiter.clone();
    let admin_rate_limiter_for_matching_status = admin_rate_limiter.clone();
    let matching_status_route = warp::path("matching-status")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let engine = partitioned_engine_2.clone();
                let ip_rate_limiter = ip_rate_limiter_for_matching_status.clone();
                let admin_rate_limiter = admin_rate_limiter_for_matching_status.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let queues: Vec<_> = engine
                        .queue_depths()
                        .into_iter()
                        .map(|depth| {
                            serde_json::json!({
                                "partition_id": depth.partition_id,
                                "inflight": depth.inflight,
                                "capacity": depth.capacity,
                            })
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "kill_switch_enabled": engine.kill_switch_enabled(),
                        "queues": queues,
                    })))
                }
            },
        );
    markets_route
        .or(market_detail_route)
        .unify()
        .or(book_route)
        .unify()
        .or(trades_route)
        .unify()
        .or(history_route)
        .unify()
        .or(stats_route)
        .unify()
        .or(matching_status_route)
        .unify()
        .boxed()
}
