use super::*;

pub(crate) fn build_trading_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let sequencer_for_intent = sequencer.clone();
    let sequencer_for_order = sequencer.clone();
    let ip_rate_limiter_for_intent = ip_rate_limiter.clone();
    let user_rate_limiter_for_intent = user_rate_limiter.clone();
    let partitioned_engine_for_intent = partitioned_engine.clone();
    let intent_route = warp::path("intent")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: IntentRequest| {
                let engine = partitioned_engine_for_intent.clone();
                let sequencer = sequencer_for_intent.clone();
                let user_rate_limiter = user_rate_limiter_for_intent.clone();
                let ip_rate_limiter = ip_rate_limiter_for_intent.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    let request_id = normalize_request_id(req.request_id);
                    let client_order_id = normalize_client_order_id(req.client_order_id);
                    audit("intent", &request_id, &principal);

                    let command = match sequence_new_order(
                        &sequencer,
                        request_id.clone(),
                        client_order_id,
                        principal.subject.clone(),
                        principal.session_id.clone(),
                        req.market_id.clone(),
                        req.side,
                        OrderType::Limit,
                        TimeInForce::Gtc,
                        Some(req.price),
                        req.amount,
                        req.outcome,
                        false,
                        false,
                        None,
                        None,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.submit_new_order(command).await {
                        Ok(result) => {
                            update_lifecycle_after_submit(&sequencer, &request_id, &result);
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "status":"ok",
                                "order_id": result.order_id,
                                "request_id": result.metadata.request_id,
                                "command_seq": result.metadata.command_seq,
                                "lifecycle": result.metadata.lifecycle,
                                "market_state": result.market_state,
                                "order_state": result.state,
                                "remaining_amount": result.remaining_amount,
                                "fills": result.fills.len(),
                            })))
                        }
                        Err(error) => {
                            let _ = sequencer.mark_rejected(&request_id);
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "status":"error",
                                "error":error.to_string()
                            })))
                        }
                    }
                }
            },
        );

    let partitioned_engine_1 = partitioned_engine.clone();
    let ip_rate_limiter_for_submit = ip_rate_limiter.clone();
    let user_rate_limiter_for_submit = user_rate_limiter.clone();
    let submit_order_route = warp::path("submit-order")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: OrderRequest| {
                let engine = partitioned_engine_1.clone();
                let sequencer = sequencer_for_order.clone();
                let user_rate_limiter = user_rate_limiter_for_submit.clone();
                let ip_rate_limiter = ip_rate_limiter_for_submit.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    let request_id = normalize_request_id(req.request_id);
                    let client_order_id = normalize_client_order_id(req.client_order_id);
                    let order_type = req.order_type.unwrap_or(OrderType::Limit);
                    let time_in_force = req.time_in_force.unwrap_or(TimeInForce::Gtc);
                    let post_only = req.post_only.unwrap_or(false);
                    let reduce_only = req.reduce_only.unwrap_or(false);
                    audit("submit_order", &request_id, &principal);

                    let command = match sequence_new_order(
                        &sequencer,
                        request_id.clone(),
                        client_order_id,
                        principal.subject.clone(),
                        principal.session_id.clone().or(req.session_id.clone()),
                        req.market_id.clone(),
                        req.side,
                        order_type,
                        time_in_force,
                        req.price,
                        req.amount,
                        req.outcome,
                        post_only,
                        reduce_only,
                        req.leverage,
                        req.expires_at,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.submit_new_order(command).await {
                        Ok(result) => {
                            update_lifecycle_after_submit(&sequencer, &request_id, &result);
                            let resp = serde_json::json!({
                                "status":"ok",
                                "order_id": result.order_id,
                                "request_id": result.metadata.request_id,
                                "command_seq": result.metadata.command_seq,
                                "lifecycle": result.metadata.lifecycle,
                                "market_state": result.market_state,
                                "order_state": result.state,
                                "remaining_amount": result.remaining_amount,
                                "fills": result.fills.len(),
                            });
                            Ok::<_, warp::Rejection>(warp::reply::json(&resp))
                        }
                        Err(error) => {
                            let _ = sequencer.mark_rejected(&request_id);
                            let resp =
                                serde_json::json!({"status":"error","error":error.to_string()});
                            Ok::<_, warp::Rejection>(warp::reply::json(&resp))
                        }
                    }
                }
            },
        );

    let partitioned_engine_3 = partitioned_engine.clone();
    let ip_rate_limiter_for_cancel = ip_rate_limiter.clone();
    let user_rate_limiter_for_cancel = user_rate_limiter.clone();
    let sequencer_for_cancel_order = sequencer.clone();
    let cancel_order_route = warp::path("cancel-order")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: CancelOrderRequest| {
                let engine = partitioned_engine_3.clone();
                let sequencer = sequencer_for_cancel_order.clone();
                let user_rate_limiter = user_rate_limiter_for_cancel.clone();
                let ip_rate_limiter = ip_rate_limiter_for_cancel.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("cancel_order", &request_id, &principal);
                    let command = match sequence_cancel_order(
                        &sequencer,
                        request_id.clone(),
                        principal.subject,
                        req.market_id,
                        req.outcome,
                        req.order_id,
                        req.client_order_id,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.cancel_order(command).await {
                        Ok(result) => {
                            update_lifecycle_after_cancel(&sequencer, &request_id);
                            let resp = serde_json::json!({
                                "status": "ok",
                                "request_id": result.metadata.request_id,
                                "command_seq": result.metadata.command_seq,
                                "lifecycle": result.metadata.lifecycle,
                                "market_state": result.market_state,
                                "cancelled_order_ids": result.cancelled_order_ids,
                            });
                            Ok::<_, warp::Rejection>(warp::reply::json(&resp))
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

    let partitioned_engine_2b = partitioned_engine.clone();
    let ip_rate_limiter_for_replace = ip_rate_limiter.clone();
    let user_rate_limiter_for_replace = user_rate_limiter.clone();
    let sequencer_for_replace_order = sequencer.clone();
    let replace_order_route = warp::path("replace-order")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: ReplaceOrderRequest| {
                let engine = partitioned_engine_2b.clone();
                let sequencer = sequencer_for_replace_order.clone();
                let user_rate_limiter = user_rate_limiter_for_replace.clone();
                let ip_rate_limiter = ip_rate_limiter_for_replace.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("replace_order", &request_id, &principal);
                    let command = sequence_replace_order(
                        &sequencer,
                        request_id.clone(),
                        principal.subject,
                        req.market_id,
                        req.outcome,
                        req.order_id,
                        req.new_client_order_id,
                        req.new_price,
                        req.new_amount,
                        req.new_time_in_force,
                        req.post_only,
                        req.reduce_only,
                        req.new_leverage,
                        req.new_expires_at,
                    )
                    .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error))?;

                    match engine.replace_order(command).await {
                        Ok(result) => {
                            update_lifecycle_after_submit(&sequencer, &request_id, &result);
                            Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                                "status":"ok",
                                "order_id": result.order_id,
                                "request_id": result.metadata.request_id,
                                "command_seq": result.metadata.command_seq,
                                "lifecycle": result.metadata.lifecycle,
                                "market_state": result.market_state,
                                "order_state": result.state,
                                "remaining_amount": result.remaining_amount,
                                "fills": result.fills.len(),
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

    let partitioned_engine_4 = partitioned_engine.clone();
    let ip_rate_limiter_for_mass_cancel_user = ip_rate_limiter.clone();
    let user_rate_limiter_for_mass_cancel_user = user_rate_limiter.clone();
    let sequencer_for_mass_cancel_user = sequencer.clone();
    let mass_cancel_user_route = warp::path!("mass-cancel" / "user")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: MassCancelByUserRequest| {
                let engine = partitioned_engine_4.clone();
                let sequencer = sequencer_for_mass_cancel_user.clone();
                let user_rate_limiter = user_rate_limiter_for_mass_cancel_user.clone();
                let ip_rate_limiter = ip_rate_limiter_for_mass_cancel_user.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("mass_cancel_user", &request_id, &principal);
                    let command = match sequence_mass_cancel_by_user(
                        &sequencer,
                        request_id.clone(),
                        principal.subject,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.mass_cancel_by_user(command).await {
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

    let partitioned_engine_5 = partitioned_engine.clone();
    let ip_rate_limiter_for_mass_cancel_session = ip_rate_limiter.clone();
    let user_rate_limiter_for_mass_cancel_session = user_rate_limiter.clone();
    let sequencer_for_mass_cancel_session = sequencer.clone();
    let mass_cancel_session_route = warp::path!("mass-cancel" / "session")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: MassCancelBySessionRequest| {
                let engine = partitioned_engine_5.clone();
                let sequencer = sequencer_for_mass_cancel_session.clone();
                let user_rate_limiter = user_rate_limiter_for_mass_cancel_session.clone();
                let ip_rate_limiter = ip_rate_limiter_for_mass_cancel_session.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("mass_cancel_session", &request_id, &principal);
                    let command = match sequence_mass_cancel_by_session(
                        &sequencer,
                        request_id.clone(),
                        principal.subject,
                        req.session_id,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.mass_cancel_by_session(command).await {
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

    intent_route
        .or(submit_order_route)
        .unify()
        .or(cancel_order_route)
        .unify()
        .or(replace_order_route)
        .unify()
        .or(mass_cancel_user_route)
        .unify()
        .or(mass_cancel_session_route)
        .unify()
        .boxed()
}
