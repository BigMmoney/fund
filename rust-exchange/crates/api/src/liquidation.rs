use super::*;

pub(crate) fn apply_liquidation_queue_override(
    queue_store: &LiquidationQueueStore,
    queue_id: &str,
    req: &LiquidationQueueOverrideRequest,
) -> Result<LiquidationQueueRecord, Rejection> {
    let current = queue_store
        .get(queue_id)
        .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "liquidation queue item not found"))?;
    let now = Utc::now();
    let next = match req.action.as_str() {
        "cancel" => LiquidationQueueRecord {
            status: "cancelled".to_string(),
            error: Some("cancelled by operator".to_string()),
            last_attempt_at: Some(now),
            recorded_at: now,
            ..current.clone()
        },
        "requeue" => LiquidationQueueRecord {
            status: "queued".to_string(),
            retry_tier: req.retry_tier.unwrap_or(current.retry_tier),
            strategy: liquidation_strategy_for_tier(req.retry_tier.unwrap_or(current.retry_tier))
                .to_string(),
            liquidator_user_id: req
                .liquidator_user_id
                .clone()
                .unwrap_or_else(|| current.liquidator_user_id.clone()),
            next_attempt_at: Some(
                now + chrono::Duration::seconds(req.next_attempt_secs.unwrap_or(0).max(0)),
            ),
            last_attempt_at: Some(now),
            recorded_at: now,
            ..current.clone()
        },
        "assign" => LiquidationQueueRecord {
            liquidator_user_id: req.liquidator_user_id.clone().ok_or_else(|| {
                reject_api(StatusCode::BAD_REQUEST, "liquidator_user_id required")
            })?,
            last_attempt_at: Some(now),
            recorded_at: now,
            ..current.clone()
        },
        "advance_tier" => {
            let tier = req
                .retry_tier
                .unwrap_or(current.retry_tier.saturating_add(1));
            LiquidationQueueRecord {
                status: "queued".to_string(),
                retry_tier: tier,
                strategy: liquidation_strategy_for_tier(tier).to_string(),
                next_attempt_at: Some(
                    now + chrono::Duration::seconds(req.next_attempt_secs.unwrap_or(0).max(0)),
                ),
                last_attempt_at: Some(now),
                recorded_at: now,
                ..current.clone()
            }
        }
        _ => {
            return Err(reject_api(
                StatusCode::BAD_REQUEST,
                "unsupported override action",
            ))
        }
    };
    queue_store
        .append(next.clone())
        .map_err(|error| reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(next)
}

pub(crate) async fn run_liquidation_queue_worker(
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    audit_store: Arc<RiskAutomationAuditStore>,
    queue_store: Arc<LiquidationQueueStore>,
    auction_store: Arc<LiquidationAuctionStore>,
    adl_governance_store: Arc<PersistentAdlGovernanceStore>,
    liquidation_policy_store: Arc<PersistentLiquidationPolicyStore>,
    default_liquidator_user_id: &str,
    maintenance_margin_bps: i64,
    penalty_bps: i64,
) {
    let now = Utc::now();
    let governance = adl_governance_store.current().governance;
    let policy = liquidation_policy_store.current();
    let pending = queue_store.list_by_statuses_oldest(1000, &["queued", "auction_open"]);
    for item in pending {
        if item
            .next_attempt_at
            .map_or(false, |next_attempt_at| next_attempt_at > now)
        {
            continue;
        }

        let instrument = instruments.resolve(&item.market_id);
        let refreshed_adl = risk.adl_ranking_with_governance(
            &instrument,
            item.outcome,
            item.mark_price,
            item.position_qty,
            &governance,
        );
        let current_item = if refreshed_adl != item.adl_candidates {
            let updated = LiquidationQueueRecord {
                adl_candidates: refreshed_adl.clone(),
                recorded_at: now,
                ..item.clone()
            };
            let _ = queue_store.append(updated.clone());
            updated
        } else {
            item.clone()
        };

        let schedule_retry = |chosen_liquidator: String,
                              error_text: String,
                              queue_store: &Arc<LiquidationQueueStore>,
                              audit_store: &Arc<RiskAutomationAuditStore>,
                              current_item: &LiquidationQueueRecord,
                              policy: &LiquidationPolicyRecord| {
            let next_tier = current_item.retry_tier.saturating_add(1);
            if next_tier < policy.max_retry_tiers {
                let next_delay = liquidation_retry_delay_secs(policy, next_tier);
                let retry_record = LiquidationQueueRecord {
                    liquidator_user_id: chosen_liquidator.clone(),
                    status: "queued".to_string(),
                    retry_tier: next_tier,
                    retry_count: current_item.retry_count.saturating_add(1),
                    strategy: liquidation_strategy_for_tier(next_tier).to_string(),
                    next_attempt_at: Some(now + chrono::Duration::seconds(next_delay)),
                    last_attempt_at: Some(now),
                    error: Some(error_text.clone()),
                    recorded_at: now,
                    ..current_item.clone()
                };
                let _ = queue_store.append(retry_record);
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "liquidation_retry_scheduled",
                    "queued",
                    &current_item.market_id,
                    current_item.outcome,
                    Some(current_item.user_id.clone()),
                    Some(chosen_liquidator),
                    &current_item.queue_id,
                    serde_json::json!({
                        "retry_tier": next_tier,
                        "retry_count": current_item.retry_count.saturating_add(1),
                        "strategy": liquidation_strategy_for_tier(next_tier),
                        "next_attempt_at": now + chrono::Duration::seconds(next_delay),
                        "error": error_text,
                    }),
                );
            } else {
                let _ = queue_store.append(LiquidationQueueRecord {
                    liquidator_user_id: chosen_liquidator.clone(),
                    status: "failed".to_string(),
                    last_attempt_at: Some(now),
                    error: Some(error_text.clone()),
                    recorded_at: now,
                    ..current_item.clone()
                });
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "liquidation_executed",
                    "error",
                    &current_item.market_id,
                    current_item.outcome,
                    Some(current_item.user_id.clone()),
                    Some(chosen_liquidator),
                    &current_item.queue_id,
                    serde_json::json!({"terminal": true, "error": error_text}),
                );
            }
        };

        if current_item.retry_tier == 0 {
            let auction = auction_store.get(&current_item.queue_id);
            match auction {
                None if current_item.status == "queued" => {
                    let reserve_price = risk
                        .bankruptcy_reference_price(
                            &current_item.user_id,
                            &instrument,
                            current_item.outcome,
                            current_item.mark_price,
                        )
                        .unwrap_or(current_item.mark_price);
                    let record = LiquidationAuctionRecord {
                        auction_id: format!(
                            "{}:round:{}",
                            current_item.queue_id, current_item.auction_round
                        ),
                        queue_id: current_item.queue_id.clone(),
                        status: "open".to_string(),
                        market_id: current_item.market_id.clone(),
                        outcome: current_item.outcome,
                        liquidated_user_id: current_item.user_id.clone(),
                        reserve_price,
                        mark_price: current_item.mark_price,
                        round: current_item.auction_round,
                        target_position_qty: current_item.remaining_position_qty,
                        filled_position_qty: current_item.filled_position_qty,
                        opened_at: now,
                        expires_at: now
                            + chrono::Duration::seconds(policy.auction_window_secs.max(1)),
                        best_bid_price: None,
                        best_bidder_user_id: None,
                        bids: Vec::new(),
                        winner_user_id: None,
                        error: None,
                        recorded_at: now,
                    };
                    let _ = auction_store.append(record);
                    let _ = queue_store.append(LiquidationQueueRecord {
                        status: "auction_open".to_string(),
                        recorded_at: now,
                        ..current_item.clone()
                    });
                    append_risk_audit_event(
                        audit_store.as_ref(),
                        "liquidation_auction_opened",
                        "ok",
                        &current_item.market_id,
                        current_item.outcome,
                        Some(current_item.user_id.clone()),
                        Some(current_item.liquidator_user_id.clone()),
                        &current_item.queue_id,
                        serde_json::json!({
                            "reserve_price": reserve_price,
                            "round": current_item.auction_round,
                            "target_position_qty": current_item.remaining_position_qty,
                        }),
                    );
                    continue;
                }
                Some(auction) if auction.status == "open" && auction.expires_at > now => continue,
                Some(auction) if auction.status == "open" => {
                    let valid_bids: Vec<_> = auction
                        .bids
                        .iter()
                        .filter(|bid| {
                            bid.bid_price >= auction.reserve_price && bid.bid_quantity > 0
                        })
                        .cloned()
                        .collect();
                    if valid_bids.is_empty() {
                        schedule_retry(
                            default_liquidator_user_id.to_string(),
                            "auction ended without valid ladder bids".to_string(),
                            &queue_store,
                            &audit_store,
                            &current_item,
                            &policy,
                        );
                        continue;
                    }
                    let _ = queue_store.append(LiquidationQueueRecord {
                        status: "running".to_string(),
                        last_attempt_at: Some(now),
                        recorded_at: now,
                        ..current_item.clone()
                    });
                    let mut remaining_position_qty = current_item.remaining_position_qty.max(0);
                    let mut filled_position_qty = current_item.filled_position_qty;
                    let mut matched_fills = Vec::new();
                    let mut last_winner: Option<String> = None;
                    for (fill_index, bid) in valid_bids.into_iter().enumerate() {
                        if remaining_position_qty <= 0 {
                            break;
                        }
                        let executable_qty = remaining_position_qty.min(bid.bid_quantity).max(0);
                        if executable_qty <= 0 {
                            continue;
                        }
                        let fill_request_id = format!(
                            "{}:round:{}:fill:{}",
                            current_item.queue_id, current_item.auction_round, fill_index
                        );
                        match risk.execute_partial_liquidation_with_governance(
                            &current_item.user_id,
                            &bid.bidder_user_id,
                            &instrument,
                            current_item.outcome,
                            current_item.mark_price,
                            instrument.max_leverage,
                            maintenance_margin_bps,
                            penalty_bps,
                            Some(executable_qty),
                            &fill_request_id,
                            &governance,
                        ) {
                            Ok(execution) => {
                                remaining_position_qty = remaining_position_qty
                                    .saturating_sub(execution.transferred_position_qty)
                                    .max(0);
                                filled_position_qty = filled_position_qty
                                    .saturating_add(execution.transferred_position_qty);
                                last_winner = Some(execution.liquidator_user_id.clone());
                                matched_fills.push(serde_json::json!({
                                    "bidder_user_id": bid.bidder_user_id,
                                    "bid_price": bid.bid_price,
                                    "bid_quantity": bid.bid_quantity,
                                    "execution": execution,
                                }));
                            }
                            Err(error) => {
                                matched_fills.push(serde_json::json!({
                                    "bidder_user_id": bid.bidder_user_id,
                                    "bid_price": bid.bid_price,
                                    "bid_quantity": bid.bid_quantity,
                                    "error": error.to_string(),
                                }));
                            }
                        }
                    }
                    if filled_position_qty == current_item.filled_position_qty {
                        let _ = auction_store.append(LiquidationAuctionRecord {
                            status: "failed".to_string(),
                            error: Some("no executable ladder bids succeeded".to_string()),
                            recorded_at: now,
                            ..auction.clone()
                        });
                        schedule_retry(
                            default_liquidator_user_id.to_string(),
                            "no executable ladder bids succeeded".to_string(),
                            &queue_store,
                            &audit_store,
                            &current_item,
                            &policy,
                        );
                        continue;
                    }
                    let _ = auction_store.append(LiquidationAuctionRecord {
                        status: if remaining_position_qty == 0 {
                            "settled".to_string()
                        } else {
                            "partial".to_string()
                        },
                        winner_user_id: last_winner.clone(),
                        filled_position_qty,
                        recorded_at: now,
                        ..auction.clone()
                    });
                    if remaining_position_qty > 0
                        && current_item.auction_round.saturating_add(1) < policy.max_auction_rounds
                    {
                        let next_queue = LiquidationQueueRecord {
                            liquidator_user_id: last_winner
                                .clone()
                                .unwrap_or_else(|| current_item.liquidator_user_id.clone()),
                            status: "queued".to_string(),
                            remaining_position_qty,
                            filled_position_qty,
                            auction_round: current_item.auction_round.saturating_add(1),
                            retry_count: current_item.retry_count.saturating_add(1),
                            next_attempt_at: Some(now),
                            last_attempt_at: Some(now),
                            recorded_at: now,
                            ..current_item.clone()
                        };
                        let _ = queue_store.append(next_queue);
                        append_risk_audit_event(
                            audit_store.as_ref(),
                            "liquidation_ladder_round_completed",
                            "ok",
                            &current_item.market_id,
                            current_item.outcome,
                            Some(current_item.user_id.clone()),
                            last_winner,
                            &current_item.queue_id,
                            serde_json::json!({
                                "matched_fills": matched_fills,
                                "remaining_position_qty": remaining_position_qty,
                                "next_round": current_item.auction_round.saturating_add(1),
                            }),
                        );
                    } else {
                        let final_snapshot = LiquidationQueueRecord {
                            liquidator_user_id: last_winner
                                .clone()
                                .unwrap_or_else(|| current_item.liquidator_user_id.clone()),
                            status: if remaining_position_qty == 0 {
                                "completed".to_string()
                            } else {
                                "queued".to_string()
                            },
                            remaining_position_qty,
                            filled_position_qty,
                            last_attempt_at: Some(now),
                            recorded_at: now,
                            ..current_item.clone()
                        };
                        let _ = queue_store.append(final_snapshot.clone());
                        if remaining_position_qty > 0 {
                            schedule_retry(
                                last_winner
                                    .unwrap_or_else(|| default_liquidator_user_id.to_string()),
                                "auction ladder exhausted; escalating tier".to_string(),
                                &queue_store,
                                &audit_store,
                                &final_snapshot,
                                &policy,
                            );
                        } else {
                            append_risk_audit_event(
                                audit_store.as_ref(),
                                "liquidation_executed",
                                "ok",
                                &current_item.market_id,
                                current_item.outcome,
                                Some(current_item.user_id.clone()),
                                Some(final_snapshot.liquidator_user_id.clone()),
                                &current_item.queue_id,
                                serde_json::json!({"matched_fills": matched_fills}),
                            );
                        }
                    }
                    continue;
                }
                _ => continue,
            }
        }

        let chosen_liquidator = if current_item.retry_tier == 1 {
            default_liquidator_user_id.to_string()
        } else {
            current_item
                .adl_candidates
                .first()
                .map(|candidate| candidate.user_id.clone())
                .filter(|candidate| candidate != &current_item.user_id)
                .unwrap_or_else(|| default_liquidator_user_id.to_string())
        };
        let _ = queue_store.append(LiquidationQueueRecord {
            liquidator_user_id: chosen_liquidator.clone(),
            status: "running".to_string(),
            last_attempt_at: Some(now),
            recorded_at: now,
            ..current_item.clone()
        });
        match risk.execute_partial_liquidation_with_governance(
            &current_item.user_id,
            &chosen_liquidator,
            &instrument,
            current_item.outcome,
            current_item.mark_price,
            instrument.max_leverage,
            maintenance_margin_bps,
            penalty_bps,
            Some(current_item.remaining_position_qty.max(0)),
            &current_item.queue_id,
            &governance,
        ) {
            Ok(execution) => {
                let remaining_position_qty = current_item
                    .remaining_position_qty
                    .saturating_sub(execution.transferred_position_qty)
                    .max(0);
                let filled_position_qty = current_item
                    .filled_position_qty
                    .saturating_add(execution.transferred_position_qty);
                let _ = queue_store.append(LiquidationQueueRecord {
                    liquidator_user_id: chosen_liquidator.clone(),
                    status: if remaining_position_qty == 0 {
                        "completed".to_string()
                    } else {
                        "failed".to_string()
                    },
                    remaining_position_qty,
                    filled_position_qty,
                    last_attempt_at: Some(now),
                    recorded_at: now,
                    ..current_item.clone()
                });
                append_risk_audit_event(
                    audit_store.as_ref(),
                    "liquidation_executed",
                    "ok",
                    &current_item.market_id,
                    current_item.outcome,
                    Some(execution.user_id.clone()),
                    Some(execution.liquidator_user_id.clone()),
                    &current_item.queue_id,
                    serde_json::json!(execution),
                );
            }
            Err(error) => {
                schedule_retry(
                    chosen_liquidator,
                    error.to_string(),
                    &queue_store,
                    &audit_store,
                    &current_item,
                    &policy,
                );
            }
        }
    }
}

pub(crate) fn build_liquidation_routes(
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    adl_governance: Arc<PersistentAdlGovernanceStore>,
    liquidation_queue: Arc<LiquidationQueueStore>,
    liquidation_auction: Arc<LiquidationAuctionStore>,
    ledger: Arc<LedgerService>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let risk_for_liquidation = risk.clone();
    let instruments_for_liquidation = instruments.clone();
    let adl_governance_for_liquidation = adl_governance.clone();
    let ip_rate_limiter_for_liquidation = ip_rate_limiter.clone();
    let admin_rate_limiter_for_liquidation = admin_rate_limiter.clone();
    let liquidation_execute_route = warp::path!("admin" / "risk" / "liquidations" / "execute")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(warp::body::json())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: LiquidationExecuteRequest| {
                let risk = risk_for_liquidation.clone();
                let instruments = instruments_for_liquidation.clone();
                let adl_governance = adl_governance_for_liquidation.clone();
                let ip_rate_limiter = ip_rate_limiter_for_liquidation.clone();
                let admin_rate_limiter = admin_rate_limiter_for_liquidation.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("liquidation_execute", &request_id, &principal);
                    let instrument = instruments.resolve(&req.market_id);
                    let governance = adl_governance.current();
                    let execution = risk
                        .execute_liquidation_with_governance(
                            &req.user_id,
                            &req.liquidator_user_id,
                            &instrument,
                            req.outcome.unwrap_or(0),
                            req.mark_price,
                            req.leverage.or(instrument.max_leverage),
                            req.maintenance_margin_bps.unwrap_or(1_000),
                            req.penalty_bps.unwrap_or(500),
                            &request_id,
                            &governance.governance,
                        )
                        .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "execution": execution,
                    })))
                }
            },
        );
    let liquidation_auction_for_get = liquidation_auction.clone();
    let ip_rate_limiter_for_liquidation_auction_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_liquidation_auction_get = admin_rate_limiter.clone();
    let liquidation_auction_route = warp::path!("admin" / "risk" / "liquidations" / "auctions")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<LiquidationAuctionsQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: LiquidationAuctionsQuery,
                  remote: Option<SocketAddr>| {
                let auction_store = liquidation_auction_for_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_liquidation_auction_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_liquidation_auction_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let items = auction_store.list_recent(
                        query.limit.unwrap_or(100).clamp(1, 1000),
                        query.status.as_deref(),
                    );
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": items,
                    })))
                }
            },
        );
    let liquidation_auction_for_bid = liquidation_auction.clone();
    let liquidation_queue_for_bid = liquidation_queue.clone();
    let ip_rate_limiter_for_liquidation_bid = ip_rate_limiter.clone();
    let user_rate_limiter_for_liquidation_bid = user_rate_limiter.clone();
    let liquidation_auction_bid_route =
        warp::path!("admin" / "risk" / "liquidations" / "auctions" / String / "bids")
            .and(warp::post())
            .and(with_principal())
            .and(remote_ip())
            .and(body_limit())
            .and(warp::body::json())
            .and_then(
                move |queue_id: String,
                      principal: AuthenticatedPrincipal,
                      remote: Option<SocketAddr>,
                      req: LiquidationAuctionBidRequest| {
                    let auction_store = liquidation_auction_for_bid.clone();
                    let queue_store = liquidation_queue_for_bid.clone();
                    let ip_rate_limiter = ip_rate_limiter_for_liquidation_bid.clone();
                    let user_rate_limiter = user_rate_limiter_for_liquidation_bid.clone();
                    async move {
                        require_user(&principal)?;
                        let ip_key = remote
                            .map(|value| value.ip().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                        user_rate_limiter
                            .check(&format!("user-write:{}", principal.subject), 20)?;
                        if req.bid_price <= 0 {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "bid_price must be positive",
                            ));
                        }
                        if req.bid_quantity <= 0 {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "bid_quantity must be positive",
                            ));
                        }
                        let queue = queue_store.get(&queue_id).ok_or_else(|| {
                            reject_api(StatusCode::NOT_FOUND, "liquidation queue item not found")
                        })?;
                        if auction_store.get(&queue_id).is_none() {
                            return Err(reject_api(
                                StatusCode::NOT_FOUND,
                                "liquidation auction not found",
                            ));
                        }
                        if principal.subject == queue.user_id {
                            return Err(reject_api(
                                StatusCode::FORBIDDEN,
                                "liquidated user cannot bid",
                            ));
                        }
                        let next = auction_store
                            .submit_bid(
                                &queue_id,
                                &principal.subject,
                                req.bid_price,
                                req.bid_quantity,
                                Utc::now(),
                            )
                            .map_err(|error| {
                                let message = error.to_string();
                                let status = if message.contains("not found") {
                                    StatusCode::NOT_FOUND
                                } else if message.contains("not open")
                                    || message.contains("expired")
                                {
                                    StatusCode::BAD_REQUEST
                                } else {
                                    StatusCode::INTERNAL_SERVER_ERROR
                                };
                                reject_api(status, message)
                            })?;
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "status": "ok",
                            "queue_id": queue_id,
                            "auction": next,
                        })))
                    }
                },
            );
    let liquidation_queue_for_get = liquidation_queue.clone();
    let ip_rate_limiter_for_liquidation_queue = ip_rate_limiter.clone();
    let admin_rate_limiter_for_liquidation_queue = admin_rate_limiter.clone();
    let liquidation_queue_route = warp::path!("admin" / "risk" / "liquidations" / "queue")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<LiquidationQueueQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: LiquidationQueueQuery,
                  remote: Option<SocketAddr>| {
                let queue_store = liquidation_queue_for_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_liquidation_queue.clone();
                let admin_rate_limiter = admin_rate_limiter_for_liquidation_queue.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let items = queue_store.list_recent(
                        query.limit.unwrap_or(100).clamp(1, 1000),
                        query.status.as_deref(),
                    );
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": items,
                    })))
                }
            },
        );
    let ledger_for_insurance_get = ledger.clone();
    let ip_rate_limiter_for_insurance_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_insurance_get = admin_rate_limiter.clone();
    let insurance_fund_route = warp::path!("admin" / "risk" / "insurance-fund")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ledger = ledger_for_insurance_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_insurance_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_insurance_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "balance": ledger.insurance_fund_balance(),
                        "account_id": ledger::LedgerService::insurance_fund_account(),
                    })))
                }
            },
        );
    let ledger_for_insurance_post = ledger.clone();
    let ip_rate_limiter_for_insurance_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_insurance_post = admin_rate_limiter.clone();
    let insurance_fund_deposit_route = warp::path!("admin" / "risk" / "insurance-fund" / "deposit")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(warp::body::json())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: InsuranceFundDepositRequest| {
                let ledger = ledger_for_insurance_post.clone();
                let ip_rate_limiter = ip_rate_limiter_for_insurance_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_insurance_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("insurance_fund_deposit", &request_id, &principal);
                    ledger
                        .deposit_insurance_fund(req.amount, request_id.clone())
                        .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "balance": ledger.insurance_fund_balance(),
                    })))
                }
            },
        );
    liquidation_execute_route
        .or(liquidation_auction_route)
        .unify()
        .or(liquidation_auction_bid_route)
        .unify()
        .or(liquidation_queue_route)
        .unify()
        .or(insurance_fund_route)
        .unify()
        .or(insurance_fund_deposit_route)
        .unify()
        .boxed()
}
