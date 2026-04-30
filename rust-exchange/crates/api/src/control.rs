use super::*;

const MAX_ID_LEN: usize = 256;

fn all_market_states() -> [MarketState; 8] {
    [
        MarketState::PreOpen,
        MarketState::Normal,
        MarketState::Stress,
        MarketState::AuctionCall,
        MarketState::CancelOnly,
        MarketState::Halted,
        MarketState::Maintenance,
        MarketState::Closed,
    ]
}

fn validate_deposit_fields(user_id: &str, op_id: &str) -> Result<(), Rejection> {
    if user_id.is_empty() || user_id.len() > MAX_ID_LEN {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "user_id must be between 1 and 256 characters",
        ));
    }
    if op_id.is_empty() || op_id.len() > MAX_ID_LEN {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "op_id must be between 1 and 256 characters",
        ));
    }
    Ok(())
}

pub(crate) fn build_control_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    ledger: Arc<LedgerService>,
    sequencer: Arc<Sequencer>,
    governance_actions: Arc<PendingGovernanceActionStore>,
    beta_controls: Arc<BetaControlStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let ledger_clone = ledger.clone();
    let beta_controls_for_deposit = beta_controls.clone();
    let ip_rate_limiter_for_deposit = ip_rate_limiter.clone();
    let admin_rate_limiter_for_deposit = admin_rate_limiter.clone();
    let deposit_route = warp::path("deposit")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: DepositRequest| {
                let ledger = ledger_clone.clone();
                let beta_controls = beta_controls_for_deposit.clone();
                let admin_rate_limiter = admin_rate_limiter_for_deposit.clone();
                let ip_rate_limiter = ip_rate_limiter_for_deposit.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    validate_deposit_fields(&req.user_id, &req.op_id)?;
                    if req.amount <= 0 {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "deposit amount must be positive",
                        ));
                    }
                    const MAX_SINGLE_DEPOSIT: i64 = 10_000_000_000; // 10B subunits
                    if req.amount > MAX_SINGLE_DEPOSIT {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "deposit amount exceeds single-operation cap",
                        ));
                    }
                    if let Some(max_cash_balance) = beta_controls
                        .user(&req.user_id)
                        .and_then(|control| control.max_cash_balance)
                    {
                        let current_cash = ledger
                            .cash_available_balance(&req.user_id)
                            .saturating_add(ledger.cash_hold_balance(&req.user_id));
                        let next_cash = current_cash.saturating_add(req.amount);
                        if next_cash > max_cash_balance {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                format!(
                                    "beta cash cap exceeded: next_cash_balance={} > max_cash_balance={}",
                                    next_cash, max_cash_balance
                                ),
                            ));
                        }
                    }
                    audit("deposit", &req.op_id, &principal);
                    match ledger.process_deposit(&req.user_id, req.amount, req.op_id.clone()) {
                        Ok(_) => Ok::<_, warp::Rejection>(warp::reply::json(
                            &serde_json::json!({"status":"ok"}),
                        )),
                        Err(e) => Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            sanitize_internal_error(&e.to_string()),
                        )),
                    }
                }
            },
        );

    let ledger_for_position_deposit = ledger.clone();
    let ip_rate_limiter_for_position_deposit = ip_rate_limiter.clone();
    let admin_rate_limiter_for_position_deposit = admin_rate_limiter.clone();
    let position_deposit_route = warp::path("position-deposit")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: PositionDepositRequest| {
                let ledger = ledger_for_position_deposit.clone();
                let admin_rate_limiter = admin_rate_limiter_for_position_deposit.clone();
                let ip_rate_limiter = ip_rate_limiter_for_position_deposit.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    validate_deposit_fields(&req.user_id, &req.op_id)?;
                    if req.amount <= 0 {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "deposit amount must be positive",
                        ));
                    }
                    const MAX_SINGLE_DEPOSIT: i64 = 10_000_000_000; // 10B subunits
                    if req.amount > MAX_SINGLE_DEPOSIT {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "deposit amount exceeds single-operation cap",
                        ));
                    }
                    audit("position_deposit", &req.op_id, &principal);
                    match ledger.process_position_deposit(
                        &req.user_id,
                        &req.market_id,
                        req.outcome,
                        req.amount,
                        req.op_id.clone(),
                    ) {
                        Ok(_) => Ok::<_, warp::Rejection>(warp::reply::json(
                            &serde_json::json!({"status":"ok"}),
                        )),
                        Err(e) => Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            sanitize_internal_error(&e.to_string()),
                        )),
                    }
                }
            },
        );

    let partitioned_engine_6 = partitioned_engine.clone();
    let ip_rate_limiter_for_mass_cancel_market = ip_rate_limiter.clone();
    let admin_rate_limiter_for_mass_cancel_market = admin_rate_limiter.clone();
    let sequencer_for_mass_cancel_market = sequencer.clone();
    let mass_cancel_market_route = warp::path!("mass-cancel" / "market")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: MassCancelByMarketRequest| {
                let engine = partitioned_engine_6.clone();
                let sequencer = sequencer_for_mass_cancel_market.clone();
                let admin_rate_limiter = admin_rate_limiter_for_mass_cancel_market.clone();
                let ip_rate_limiter = ip_rate_limiter_for_mass_cancel_market.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("mass_cancel_market", &request_id, &principal);
                    let command = match sequence_mass_cancel_by_market(
                        &sequencer,
                        request_id.clone(),
                        req.market_id,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.mass_cancel_by_market(command).await {
                        Ok(result) => {
                            update_lifecycle_after_cancel(&sequencer, &request_id);
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "status": "ok",
                                "request_id": result.metadata.request_id,
                                "command_seq": result.metadata.command_seq,
                                "lifecycle": result.metadata.lifecycle,
                                "market_state": result.market_state,
                                "cancelled_count": result.cancelled_order_ids.len(),
                                "cancelled_order_ids": result.cancelled_order_ids,
                            })))
                        }
                        Err(error) => {
                            let _ = sequencer.mark_rejected(&request_id);
                            Err(reject_submission_error(&error))
                        }
                    }
                }
            },
        );

    let partitioned_engine_7 = partitioned_engine.clone();
    let governance_actions_for_kill_switch = governance_actions.clone();
    let ip_rate_limiter_for_kill_switch = ip_rate_limiter.clone();
    let admin_rate_limiter_for_kill_switch = admin_rate_limiter.clone();
    let sequencer_for_kill_switch = sequencer.clone();
    let kill_switch_route = warp::path!("admin" / "kill-switch")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: KillSwitchRequest| {
                let engine = partitioned_engine_7.clone();
                let _sequencer = sequencer_for_kill_switch.clone();
                let governance_actions = governance_actions_for_kill_switch.clone();
                let admin_rate_limiter = admin_rate_limiter_for_kill_switch.clone();
                let ip_rate_limiter = ip_rate_limiter_for_kill_switch.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = normalize_request_id(req.request_id.clone());
                    audit("kill_switch", &request_id, &principal);
                    let pending = create_pending_governance_action(
                        governance_actions.as_ref(),
                        "kill_switch",
                        serde_json::to_value(&req).map_err(|error| {
                            reject_api(StatusCode::BAD_REQUEST, error.to_string())
                        })?,
                        &principal.subject,
                        Some(request_id),
                    )
                    .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "pending",
                        "approval": pending,
                        "kill_switch_enabled": engine.kill_switch_enabled(),
                    })))
                }
            },
        );

    let partitioned_engine_8 = partitioned_engine.clone();
    let governance_actions_for_market_state = governance_actions.clone();
    let ip_rate_limiter_for_market_state = ip_rate_limiter.clone();
    let admin_rate_limiter_for_market_state = admin_rate_limiter.clone();
    let sequencer_for_market_state = sequencer.clone();
    let set_market_state_route = warp::path!("admin" / "market-state")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: SetMarketStateRequest| {
                let _engine = partitioned_engine_8.clone();
                let _sequencer = sequencer_for_market_state.clone();
                let governance_actions = governance_actions_for_market_state.clone();
                let admin_rate_limiter = admin_rate_limiter_for_market_state.clone();
                let ip_rate_limiter = ip_rate_limiter_for_market_state.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = normalize_request_id(req.request_id.clone());
                    audit("market_state", &request_id, &principal);
                    let pending = create_pending_governance_action(
                        governance_actions.as_ref(),
                        "set_market_state",
                        serde_json::to_value(&req).map_err(|error| {
                            reject_api(StatusCode::BAD_REQUEST, error.to_string())
                        })?,
                        &principal.subject,
                        Some(request_id),
                    )
                    .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "pending",
                        "approval": pending,
                    })))
                }
            },
        );
    let partitioned_engine_for_market_state_get = partitioned_engine.clone();
    let governance_actions_for_market_state_get = governance_actions.clone();
    let ip_rate_limiter_for_market_state_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_market_state_get = admin_rate_limiter.clone();
    let market_state_route = warp::path!("admin" / "market-state" / String)
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<MarketStateQuery>())
        .and(remote_ip())
        .and_then(
            move |market_id: String,
                  principal: AuthenticatedPrincipal,
                  query: MarketStateQuery,
                  remote: Option<SocketAddr>| {
                let engine = partitioned_engine_for_market_state_get.clone();
                let governance_actions = governance_actions_for_market_state_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_market_state_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_market_state_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let outcome = query.outcome.unwrap_or(0);
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
                        .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "market not found"))?;
                    let allowed_transitions: Vec<_> = all_market_states()
                        .into_iter()
                        .filter(|state| snapshot.state.can_transition_to(*state))
                        .collect();
                    let pending: Vec<_> = governance_actions
                        .list_recent(100, Some("pending"))
                        .into_iter()
                        .filter(|item| item.action_type == "set_market_state")
                        .filter(|item| {
                            item.payload["market_id"].as_str() == Some(snapshot.market_id.as_str())
                                && item.payload["outcome"]
                                    .as_i64()
                                    .map(|value| value as i32)
                                    .unwrap_or(0)
                                    == snapshot.outcome
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "market_id": snapshot.market_id,
                        "outcome": snapshot.outcome,
                        "state": snapshot.state,
                        "reference_price": snapshot.reference_price,
                        "last_trade_price": snapshot.last_trade_price,
                        "allowed_transitions": allowed_transitions,
                        "pending_actions": pending,
                        "updated_at": Utc::now(),
                    })))
                }
            },
        );

    let partitioned_engine_9 = partitioned_engine.clone();
    let governance_actions_for_reference = governance_actions.clone();
    let ip_rate_limiter_for_reference = ip_rate_limiter.clone();
    let admin_rate_limiter_for_reference = admin_rate_limiter.clone();
    let reference_price_route = warp::path!("admin" / "reference-price")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: ReferencePriceRequest| {
                let _engine = partitioned_engine_9.clone();
                let governance_actions = governance_actions_for_reference.clone();
                let admin_rate_limiter = admin_rate_limiter_for_reference.clone();
                let ip_rate_limiter = ip_rate_limiter_for_reference.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let request_id = normalize_request_id(req.request_id.clone());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    audit("reference_price", &request_id, &principal);
                    let pending = create_pending_governance_action(
                        governance_actions.as_ref(),
                        "reference_price",
                        serde_json::to_value(&req).map_err(|error| {
                            reject_api(StatusCode::BAD_REQUEST, error.to_string())
                        })?,
                        &principal.subject,
                        Some(request_id),
                    )
                    .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "pending",
                        "approval": pending,
                    })))
                }
            },
        );
    deposit_route
        .or(mass_cancel_market_route)
        .unify()
        .or(position_deposit_route)
        .unify()
        .or(kill_switch_route)
        .unify()
        .or(set_market_state_route)
        .unify()
        .or(market_state_route)
        .unify()
        .or(reference_price_route)
        .unify()
        .boxed()
}
