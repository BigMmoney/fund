use super::*;

pub(crate) fn wal_entries_or_empty<
    T: Clone + serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
>(
    wal: &dyn persistence::WalStore<T>,
) -> anyhow::Result<Vec<T>> {
    match wal.entries() {
        Ok(entries) => Ok(entries),
        Err(error) => {
            let message = error.to_string().to_lowercase();
            if message.contains("os error 2")
                || message.contains("not found")
                || message.contains("no such file")
            {
                Ok(Vec::new())
            } else {
                Err(error)
            }
        }
    }
}

fn market_list_with_registry(
    snapshots: &[MarketRuntimeSnapshot],
    instruments: &PersistentInstrumentRegistry,
) -> Vec<serde_json::Value> {
    let mut items = snapshots_to_market_list(snapshots);
    let mut seen: std::collections::BTreeSet<String> = items
        .iter()
        .filter_map(|item| {
            item.get("market_id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .collect();

    for spec in instruments.list() {
        if seen.insert(spec.instrument_id.clone()) {
            items.push(serde_json::json!({
                "id": spec.instrument_id,
                "market_id": spec.instrument_id,
                "name": spec.instrument_id,
                "kind": spec.kind,
                "state": MarketState::Normal,
                "outcomes": [0],
                "open_orders": 0,
                "markets": [],
                "trading_enabled": true,
            }));
        }
    }

    for extra in supplemental_product_markets() {
        if let Some(market_id) = extra.get("market_id").and_then(|value| value.as_str()) {
            if seen.insert(market_id.to_string()) {
                items.push(extra);
            }
        }
    }

    items.sort_by(|lhs, rhs| lhs["market_id"].as_str().cmp(&rhs["market_id"].as_str()));
    items
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_market_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
    ledger: Arc<LedgerService>,
    index_prices: Arc<PersistentIndexPriceStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let partitioned_engine_for_markets = partitioned_engine.clone();
    let instruments_for_markets = instruments.clone();
    let ip_rate_limiter_for_markets = ip_rate_limiter.clone();
    let markets_route = warp::path("markets")
        .and(warp::path::end())
        .and(warp::get())
        .and(remote_ip())
        .and_then(move |remote: Option<SocketAddr>| {
            let engine = partitioned_engine_for_markets.clone();
            let instruments = instruments_for_markets.clone();
            let ip_rate_limiter = ip_rate_limiter_for_markets.clone();
            async move {
                let ip_key = remote
                    .map(|value| value.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                let records = engine
                    .export_snapshots()
                    .await
                    .map_err(reject_internal_error)?;
                let snapshots = flatten_market_snapshots(&records);
                Ok::<_, warp::Rejection>(warp::reply::json(&market_list_with_registry(
                    &snapshots,
                    instruments.as_ref(),
                )))
            }
        });
    let partitioned_engine_for_market_detail = partitioned_engine.clone();
    let instruments_for_market_detail = instruments.clone();
    let ip_rate_limiter_for_market_detail = ip_rate_limiter.clone();
    let market_detail_route = warp::path!("markets" / String)
        .and(warp::path::end())
        .and(warp::get())
        .and(remote_ip())
        .and_then(move |market_id: String, remote: Option<SocketAddr>| {
            let engine = partitioned_engine_for_market_detail.clone();
            let instruments = instruments_for_market_detail.clone();
            let ip_rate_limiter = ip_rate_limiter_for_market_detail.clone();
            async move {
                let ip_key = remote
                    .map(|value| value.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                let records = engine
                    .export_snapshots()
                    .await
                    .map_err(reject_internal_error)?;
                let snapshots = flatten_market_snapshots(&records);
                let market = market_list_with_registry(&snapshots, instruments.as_ref())
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
                    let records = engine
                        .export_snapshots()
                        .await
                        .map_err(reject_internal_error)?;
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
    let trade_journal_for_market_trades = trade_journal_wal.clone();
    let ip_rate_limiter_for_market_trades = ip_rate_limiter.clone();
    let user_rate_limiter_for_market_trades = user_rate_limiter.clone();
    let market_trades_route = warp::path!("markets" / String / "trades")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_optional_principal())
        .and(optional_query::<TradesQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String,
                  principal: Option<AuthenticatedPrincipal>,
                  query: TradesQuery,
                  remote: Option<SocketAddr>| {
                let trade_journal = trade_journal_for_market_trades.clone();
                let ip_rate_limiter = ip_rate_limiter_for_market_trades.clone();
                let user_rate_limiter = user_rate_limiter_for_market_trades.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    if let Some(user_id) = query.user_id.as_deref() {
                        let principal = principal.ok_or_else(|| {
                            reject_api(StatusCode::UNAUTHORIZED, "missing authentication headers")
                        })?;
                        ensure_subject_or_admin(&principal, user_id)?;
                        user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    }
                    let limit = query.limit.unwrap_or(50).clamp(1, 500);
                    let mut trades: Vec<_> = wal_entries_or_empty(trade_journal.as_ref())
                        .map_err(reject_internal_error)?
                        .into_iter()
                        .filter(|trade| trade.market_id == market_id)
                        .filter(|trade| {
                            query.outcome.is_none_or(|outcome| trade.outcome == outcome)
                        })
                        .filter(|trade| {
                            query.user_id.as_deref().is_none_or(|user_id| {
                                trade.buy_user_id == user_id || trade.sell_user_id == user_id
                            })
                        })
                        .filter(|trade| {
                            query.before.is_none_or(|before| trade.recorded_at < before)
                        })
                        .filter(|trade| query.after.is_none_or(|after| trade.recorded_at > after))
                        .collect();
                    trades.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));
                    trades.truncate(limit);
                    let next_cursor = trades.last().map(|t| t.recorded_at.to_rfc3339());
                    let payload: Vec<_> = trades.iter().map(trade_record_to_json).collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "market_id": market_id,
                        "trades": payload,
                        "count": payload.len(),
                        "next_cursor": next_cursor,
                    })))
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
                    let trades = wal_entries_or_empty(trade_journal.as_ref())
                        .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&trades_to_history(
                        &market_id,
                        query.outcome,
                        &trades,
                        limit,
                        query.after.as_deref(),
                        query.before.as_deref(),
                    )))
                }
            },
        );
    // GET /markets/{market_id}/ticker — 24h rolling ticker
    let trade_journal_for_ticker = trade_journal_wal.clone();
    let ip_rate_limiter_for_ticker = ip_rate_limiter.clone();
    let ticker_route = warp::path!("markets" / String / "ticker")
        .and(warp::get())
        .and(optional_query::<TickerQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String, query: TickerQuery, remote: Option<SocketAddr>| {
                let trade_journal = trade_journal_for_ticker.clone();
                let ip_rate_limiter = ip_rate_limiter_for_ticker.clone();
                async move {
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    let cutoff = Utc::now() - chrono::Duration::hours(24);
                    let trades: Vec<_> = wal_entries_or_empty(trade_journal.as_ref())
                        .map_err(reject_internal_error)?
                        .into_iter()
                        .filter(|t| t.market_id == market_id)
                        .filter(|t| query.outcome.is_none_or(|o| o == t.outcome))
                        .filter(|t| t.recorded_at >= cutoff)
                        .collect();
                    if trades.is_empty() {
                        return Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "market_id": market_id,
                            "outcome": query.outcome.unwrap_or(0),
                            "last_price": null,
                            "open_24h": null,
                            "high_24h": null,
                            "low_24h": null,
                            "volume_24h": 0,
                            "trade_count_24h": 0,
                            "price_change_24h": null,
                            "price_change_pct_24h": null,
                            "timestamp": Utc::now(),
                        })));
                    }
                    let open = trades.first().map(|t| t.price).unwrap_or(0);
                    let last = trades.last().map(|t| t.price).unwrap_or(0);
                    let high = trades.iter().map(|t| t.price).max().unwrap_or(0);
                    let low = trades.iter().map(|t| t.price).min().unwrap_or(0);
                    let volume: i64 = trades.iter().map(|t| t.amount).sum();
                    let notional_volume: i128 = trades
                        .iter()
                        .map(|t| t.price as i128 * t.amount as i128)
                        .sum();
                    let vwap = if volume > 0 {
                        Some((notional_volume / volume as i128) as i64)
                    } else {
                        None
                    };
                    let change = last - open;
                    let change_pct = if open != 0 {
                        (change as f64 / open as f64) * 100.0
                    } else {
                        0.0
                    };
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "market_id": market_id,
                        "outcome": query.outcome.unwrap_or(0),
                        "last_price": last,
                        "open_24h": open,
                        "high_24h": high,
                        "low_24h": low,
                        "volume_24h": volume,
                        "notional_volume_24h": notional_volume.to_string(),
                        "vwap_24h": vwap,
                        "trade_count_24h": trades.len(),
                        "price_change_24h": change,
                        "price_change_pct_24h": format!("{change_pct:.2}"),
                        "timestamp": Utc::now(),
                    })))
                }
            },
        );

    // GET /markets/{market_id}/klines — multi-interval candlesticks
    let trade_journal_for_klines = trade_journal_wal.clone();
    let ip_rate_limiter_for_klines = ip_rate_limiter.clone();
    let klines_route = warp::path!("markets" / String / "klines")
        .and(warp::get())
        .and(optional_query::<KlineQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String, query: KlineQuery, remote: Option<SocketAddr>| {
                let trade_journal = trade_journal_for_klines.clone();
                let ip_rate_limiter = ip_rate_limiter_for_klines.clone();
                async move {
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
                    let interval_str = query.interval.as_deref().unwrap_or("1h");
                    let bucket_secs: i64 = match interval_str {
                        "1m" => 60,
                        "5m" => 300,
                        "15m" => 900,
                        "30m" => 1800,
                        "1h" => 3600,
                        "4h" => 14400,
                        "1d" => 86400,
                        _ => {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "invalid interval; valid: 1m,5m,15m,30m,1h,4h,1d",
                            ));
                        }
                    };
                    let trades = wal_entries_or_empty(trade_journal.as_ref())
                        .map_err(reject_internal_error)?;
                    let mut grouped: std::collections::BTreeMap<i64, Vec<&TradeJournalRecord>> =
                        std::collections::BTreeMap::new();
                    for trade in trades
                        .iter()
                        .filter(|t| t.market_id == market_id)
                        .filter(|t| query.outcome.is_none_or(|o| o == t.outcome))
                    {
                        let ts = trade.recorded_at.timestamp();
                        let bucket = ts - (ts % bucket_secs);
                        grouped.entry(bucket).or_default().push(trade);
                    }
                    let mut candles: Vec<serde_json::Value> = grouped
                        .into_iter()
                        .map(|(bucket_ts, bucket)| {
                            let open = bucket.first().map(|t| t.price).unwrap_or(0);
                            let close = bucket.last().map(|t| t.price).unwrap_or(open);
                            let high = bucket.iter().map(|t| t.price).max().unwrap_or(open);
                            let low = bucket.iter().map(|t| t.price).min().unwrap_or(open);
                            let volume: i64 = bucket.iter().map(|t| t.amount).sum();
                            let ts_str = chrono::DateTime::from_timestamp(bucket_ts, 0)
                                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                                .unwrap_or_default();
                            serde_json::json!({
                                "timestamp": ts_str,
                                "open": open,
                                "high": high,
                                "low": low,
                                "close": close,
                                "volume": volume,
                                "trades": bucket.len(),
                            })
                        })
                        .collect();
                    if candles.len() > limit {
                        candles = candles.split_off(candles.len() - limit);
                    }
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "market_id": market_id,
                        "interval": interval_str,
                        "data": candles,
                    })))
                }
            },
        );

    // GET /markets/{market_id}/open-interest — open interest for the market
    let ledger_for_oi = ledger.clone();
    let ip_rate_limiter_for_oi = ip_rate_limiter.clone();
    let open_interest_route = warp::path!("markets" / String / "open-interest")
        .and(warp::get())
        .and(optional_query::<TickerQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String, query: TickerQuery, remote: Option<SocketAddr>| {
                let ledger = ledger_for_oi.clone();
                let ip_rate_limiter = ip_rate_limiter_for_oi.clone();
                async move {
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    let outcome = query.outcome.unwrap_or(0);
                    let oi = ledger.open_interest(&market_id, outcome);
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "market_id": market_id,
                        "outcome": outcome,
                        "open_interest": oi,
                        "timestamp": Utc::now(),
                    })))
                }
            },
        );

    // GET /markets/{market_id}/funding-rate — public current funding rate
    let funding_engine = partitioned_engine.clone();
    let funding_idx = index_prices.clone();
    let funding_ip_rl = ip_rate_limiter.clone();
    let funding_instruments = instruments.clone();
    let funding_rate_route = warp::path!("markets" / String / "funding-rate")
        .and(warp::get())
        .and(optional_query::<TickerQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String, query: TickerQuery, remote: Option<SocketAddr>| {
                let engine = funding_engine.clone();
                let idx = funding_idx.clone();
                let ip_rl = funding_ip_rl.clone();
                let inst_registry = funding_instruments.clone();
                async move {
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".into());
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    let outcome = query.outcome.unwrap_or(0);
                    let records = engine
                        .export_snapshots()
                        .await
                        .map_err(reject_internal_error)?;
                    let snapshots = flatten_market_snapshots(&records);
                    let snapshot = snapshots
                        .iter()
                        .find(|s| s.market_id == market_id && s.outcome == outcome);
                    let rate = snapshot.and_then(|s| pricing::derive_funding_rate_quote(s, &idx));
                    let inst = inst_registry.resolve(&market_id);
                    match rate {
                        Some(q) => {
                            let interval = if inst.funding_interval_secs > 0 {
                                inst.funding_interval_secs
                            } else {
                                cfg().risk.funding_interval_secs
                            };
                            let now = Utc::now();
                            let next_funding_at = if interval > 0 {
                                let epoch_secs = now.timestamp() as u64;
                                let next = epoch_secs - (epoch_secs % interval) + interval;
                                Some(chrono::DateTime::from_timestamp(next as i64, 0))
                            } else {
                                None
                            };
                            let secs_until = next_funding_at
                                .flatten()
                                .map(|t| (t - now).num_seconds().max(0));
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "market_id": q.market_id,
                                "outcome": q.outcome,
                                "mark_price": q.mark_price,
                                "index_price": q.index_price,
                                "premium_bps": q.premium_bps,
                                "clamped_premium_bps": q.clamped_premium_bps,
                                "interest_bps": q.interest_bps,
                                "funding_rate_ppm": q.funding_rate_ppm,
                                "predicted_funding_rate_ppm": q.funding_rate_ppm,
                                "funding_interval_secs": interval,
                                "next_funding_at": next_funding_at.flatten(),
                                "seconds_until_funding": secs_until,
                                "degraded_mode": q.degraded_mode,
                                "timestamp": now,
                            })))
                        }
                        None => Ok(warp::reply::json(&serde_json::json!({
                            "market_id": market_id,
                            "outcome": outcome,
                            "funding_rate_ppm": null,
                            "message": "insufficient price data",
                            "timestamp": Utc::now(),
                        }))),
                    }
                }
            },
        );

    // GET /markets/{market_id}/mark-price — public mark/fair price
    let mark_engine = partitioned_engine.clone();
    let mark_idx = index_prices.clone();
    let mark_ip_rl = ip_rate_limiter.clone();
    let mark_price_route = warp::path!("markets" / String / "mark-price")
        .and(warp::get())
        .and(optional_query::<TickerQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String, query: TickerQuery, remote: Option<SocketAddr>| {
                let engine = mark_engine.clone();
                let idx = mark_idx.clone();
                let ip_rl = mark_ip_rl.clone();
                async move {
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".into());
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    let outcome = query.outcome.unwrap_or(0);
                    let records = engine
                        .export_snapshots()
                        .await
                        .map_err(reject_internal_error)?;
                    let snapshots = flatten_market_snapshots(&records);
                    let snapshot = snapshots
                        .iter()
                        .find(|s| s.market_id == market_id && s.outcome == outcome);
                    let quote =
                        snapshot.and_then(|s| pricing::fair_price_quote_for_snapshot(s, &idx));
                    match quote {
                        Some(q) => {
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "market_id": market_id,
                                "outcome": outcome,
                                "mark_price": q.fair_price,
                                "timestamp": Utc::now(),
                            })))
                        }
                        None => Ok(warp::reply::json(&serde_json::json!({
                            "market_id": market_id,
                            "outcome": outcome,
                            "mark_price": null,
                            "message": "insufficient price data",
                            "timestamp": Utc::now(),
                        }))),
                    }
                }
            },
        );

    // GET /markets/summary — aggregated 24h stats per market
    let summary_journal = trade_journal_wal.clone();
    let summary_engine = partitioned_engine.clone();
    let summary_instruments = instruments.clone();
    let summary_ip_rl = ip_rate_limiter.clone();
    let market_summary_route = warp::path!("markets" / "summary")
        .and(warp::get())
        .and(remote_ip())
        .and_then(move |remote: Option<SocketAddr>| {
            let journal = summary_journal.clone();
            let engine = summary_engine.clone();
            let instr = summary_instruments.clone();
            let ip_rl = summary_ip_rl.clone();
            async move {
                let ip_key = remote
                    .map(|v| v.ip().to_string())
                    .unwrap_or_else(|| "unknown".into());
                ip_rl.check(&format!("ip:{ip_key}"), 30)?;
                let cutoff = Utc::now() - chrono::Duration::hours(24);
                let trades =
                    wal_entries_or_empty(journal.as_ref()).map_err(reject_internal_error)?;
                let records = engine
                    .export_snapshots()
                    .await
                    .map_err(reject_internal_error)?;
                let snapshots = flatten_market_snapshots(&records);

                // Gather all known market IDs
                let mut market_ids: std::collections::BTreeSet<String> =
                    snapshots.iter().map(|s| s.market_id.clone()).collect();
                for spec in instr.list() {
                    market_ids.insert(spec.instrument_id.clone());
                }

                let summaries: Vec<serde_json::Value> = market_ids
                    .iter()
                    .map(|mid| {
                        let market_trades: Vec<_> = trades
                            .iter()
                            .filter(|t| t.market_id == *mid && t.recorded_at >= cutoff)
                            .collect();
                        let volume: i64 = market_trades.iter().map(|t| t.amount).sum();
                        let notional_volume: i128 = market_trades
                            .iter()
                            .map(|t| t.price as i128 * t.amount as i128)
                            .sum();
                        let last_price = market_trades.last().map(|t| t.price);
                        let high = market_trades.iter().map(|t| t.price).max();
                        let low = market_trades.iter().map(|t| t.price).min();
                        let open = market_trades.first().map(|t| t.price);
                        let change = match (last_price, open) {
                            (Some(l), Some(o)) if o != 0 => {
                                Some(((l - o) as f64 / o as f64 * 10000.0) as i64)
                            }
                            _ => None,
                        };
                        // Best bid/ask from snapshot
                        let snap = snapshots.iter().find(|s| s.market_id == *mid);
                        let (best_bid, best_ask) = snap
                            .map(|s| {
                                let bb = s
                                    .orders
                                    .iter()
                                    .filter(|o| o.side == Side::Buy)
                                    .map(|o| o.price)
                                    .max();
                                let ba = s
                                    .orders
                                    .iter()
                                    .filter(|o| o.side == Side::Sell)
                                    .map(|o| o.price)
                                    .min();
                                (bb, ba)
                            })
                            .unwrap_or((None, None));
                        serde_json::json!({
                            "market_id": mid,
                            "last_price": last_price,
                            "open_24h": open,
                            "high_24h": high,
                            "low_24h": low,
                            "volume_24h": volume,
                            "notional_volume_24h": notional_volume.to_string(),
                            "trade_count_24h": market_trades.len(),
                            "price_change_bps_24h": change,
                            "best_bid": best_bid,
                            "best_ask": best_ask,
                        })
                    })
                    .collect();
                Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                    "markets": summaries,
                    "timestamp": Utc::now(),
                })))
            }
        });

    markets_route
        .or(market_summary_route)
        .unify()
        .or(market_detail_route)
        .unify()
        .or(book_route)
        .unify()
        .or(market_trades_route)
        .unify()
        .or(history_route)
        .unify()
        .or(ticker_route)
        .unify()
        .or(klines_route)
        .unify()
        .or(open_interest_route)
        .unify()
        .or(funding_rate_route)
        .unify()
        .or(mark_price_route)
        .unify()
        .boxed()
}
