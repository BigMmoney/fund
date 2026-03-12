use super::*;

pub(crate) fn build_control_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    ledger: Arc<LedgerService>,
    sequencer: Arc<Sequencer>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let ledger_clone = ledger.clone();
    let ip_rate_limiter_for_deposit = ip_rate_limiter.clone();
    let admin_rate_limiter_for_deposit = admin_rate_limiter.clone();
    let deposit_route = warp::path("deposit")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(warp::body::json())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: DepositRequest| {
                let ledger = ledger_clone.clone();
                let admin_rate_limiter = admin_rate_limiter_for_deposit.clone();
                let ip_rate_limiter = ip_rate_limiter_for_deposit.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    audit("deposit", &req.op_id, &principal);
                    let resp =
                        match ledger.process_deposit(&req.user_id, req.amount, req.op_id.clone()) {
                            Ok(_) => serde_json::json!({"status":"ok"}),
                            Err(e) => serde_json::json!({"status":"error","error":e.to_string()}),
                        };
                    Ok::<_, warp::Rejection>(warp::reply::json(&resp))
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
        .and(warp::body::json())
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
                            Ok::<_, warp::Rejection>(warp::reply::json(
                                &serde_json::json!({"status":"error","error":error.to_string()}),
                            ))
                        }
                    }
                }
            },
        );

    let partitioned_engine_7 = partitioned_engine.clone();
    let ip_rate_limiter_for_kill_switch = ip_rate_limiter.clone();
    let admin_rate_limiter_for_kill_switch = admin_rate_limiter.clone();
    let sequencer_for_kill_switch = sequencer.clone();
    let kill_switch_route = warp::path!("admin" / "kill-switch")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(warp::body::json())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: KillSwitchRequest| {
                let engine = partitioned_engine_7.clone();
                let sequencer = sequencer_for_kill_switch.clone();
                let admin_rate_limiter = admin_rate_limiter_for_kill_switch.clone();
                let ip_rate_limiter = ip_rate_limiter_for_kill_switch.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("kill_switch", &request_id, &principal);
                    let command = match sequence_admin(
                        &sequencer,
                        request_id.clone(),
                        principal.subject,
                        AdminAction::KillSwitch {
                            enabled: req.enabled,
                        },
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.submit_admin(command).await {
                        Ok(()) => {
                            update_lifecycle_after_admin(&sequencer, &request_id);
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "status": "ok",
                                "kill_switch_enabled": engine.kill_switch_enabled(),
                            })))
                        }
                        Err(error) => {
                            let _ = sequencer.mark_rejected(&request_id);
                            Ok::<_, warp::Rejection>(warp::reply::json(
                                &serde_json::json!({"status":"error","error":error.to_string()}),
                            ))
                        }
                    }
                }
            },
        );

    let partitioned_engine_8 = partitioned_engine.clone();
    let ip_rate_limiter_for_market_state = ip_rate_limiter.clone();
    let admin_rate_limiter_for_market_state = admin_rate_limiter.clone();
    let sequencer_for_market_state = sequencer.clone();
    let set_market_state_route = warp::path!("admin" / "market-state")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(warp::body::json())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: SetMarketStateRequest| {
                let engine = partitioned_engine_8.clone();
                let sequencer = sequencer_for_market_state.clone();
                let admin_rate_limiter = admin_rate_limiter_for_market_state.clone();
                let ip_rate_limiter = ip_rate_limiter_for_market_state.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("market_state", &request_id, &principal);
                    let command = match sequence_admin(
                        &sequencer,
                        request_id.clone(),
                        principal.subject,
                        AdminAction::SetMarketState {
                            market_id: req.market_id,
                            outcome: req.outcome,
                            state: req.state,
                        },
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.submit_admin(command).await {
                        Ok(()) => {
                            update_lifecycle_after_admin(&sequencer, &request_id);
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "status": "ok",
                            })))
                        }
                        Err(error) => {
                            let _ = sequencer.mark_rejected(&request_id);
                            Ok::<_, warp::Rejection>(warp::reply::json(
                                &serde_json::json!({"status":"error","error":error.to_string()}),
                            ))
                        }
                    }
                }
            },
        );

    let partitioned_engine_9 = partitioned_engine.clone();
    let ip_rate_limiter_for_reference = ip_rate_limiter.clone();
    let admin_rate_limiter_for_reference = admin_rate_limiter.clone();
    let reference_price_route = warp::path!("market" / "reference-price")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(warp::body::json())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: ReferencePriceRequest| {
                let engine = partitioned_engine_9.clone();
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
                    match engine
                        .update_reference_price(
                            req.market_id,
                            req.outcome,
                            req.source.unwrap_or_else(|| "manual".to_string()),
                            req.reference_price,
                        )
                        .await
                    {
                        Ok(snapshot) => {
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "status": "ok",
                                "request_id": request_id,
                                "market_id": snapshot.market_id,
                                "outcome": snapshot.outcome,
                                "market_state": snapshot.state,
                                "reference_price": snapshot.reference_price,
                                "best_bid": snapshot.best_bid,
                                "best_ask": snapshot.best_ask,
                            })))
                        }
                        Err(error) => Ok::<_, warp::Rejection>(warp::reply::json(
                            &serde_json::json!({"status":"error","error":error.to_string()}),
                        )),
                    }
                }
            },
        );
    deposit_route
        .or(mass_cancel_market_route)
        .unify()
        .or(kill_switch_route)
        .unify()
        .or(set_market_state_route)
        .unify()
        .or(reference_price_route)
        .unify()
        .boxed()
}
