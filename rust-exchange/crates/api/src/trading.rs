use super::*;

/// Maximum allowed string length for user-supplied identifiers (market_id, order_id, etc.).
const MAX_ID_LEN: usize = 256;

/// Validate common order input fields that are present in both intent and submit-order.
fn validate_order_fields(
    market_id: &str,
    amount: i64,
    price: Option<i64>,
    leverage: Option<u32>,
    order_type: Option<OrderType>,
) -> Result<(), Rejection> {
    if market_id.len() > MAX_ID_LEN {
        return Err(reject_api(StatusCode::BAD_REQUEST, "market_id too long"));
    }
    if amount <= 0 {
        return Err(reject_api(
            StatusCode::BAD_REQUEST,
            "amount must be positive",
        ));
    }
    let is_market = matches!(
        order_type,
        Some(OrderType::Market | OrderType::StopMarket | OrderType::TakeProfitMarket)
    );
    if let Some(p) = price {
        if is_market {
            return Err(reject_api(
                StatusCode::BAD_REQUEST,
                "market orders must not specify a price",
            ));
        }
        if p <= 0 {
            return Err(reject_api(
                StatusCode::BAD_REQUEST,
                "price must be positive",
            ));
        }
    }
    if leverage == Some(0) {
        return Err(reject_api(StatusCode::BAD_REQUEST, "leverage must be >= 1"));
    }
    Ok(())
}

fn enforce_submit_order_rate_limits(
    ip_rate_limiter: &FixedWindowRateLimiter,
    user_rate_limiter: &FixedWindowRateLimiter,
    ip_key: &str,
    user_id: &str,
) -> Result<(), Rejection> {
    if ip_rate_limiter.check(&format!("ip:{ip_key}"), 60).is_err() {
        observability::METRICS.record_submit_order_ip_rate_limited();
        return Err(warp::reject::custom(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "submit-order ip rate limit exceeded".to_string(),
            code: Some("RATE_LIMITED".to_string()),
            details: Some(serde_json::json!({
                "limiter": "ip",
                "route": "submit-order",
            })),
        }));
    }
    if user_rate_limiter
        .check(&format!("user:{}", user_id), 30)
        .is_err()
    {
        observability::METRICS.record_submit_order_user_rate_limited();
        return Err(warp::reject::custom(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "submit-order user write rate limit exceeded".to_string(),
            code: Some("RATE_LIMITED".to_string()),
            details: Some(serde_json::json!({
                "limiter": "user_write",
                "route": "submit-order",
            })),
        }));
    }
    Ok(())
}

fn effective_open_order_cap(
    risk: &RiskEngine,
    beta_controls: &BetaControlStore,
    user_id: &str,
) -> Option<u32> {
    let risk_cap = risk
        .user_risk_limits(user_id)
        .and_then(|limits| (limits.max_open_orders > 0).then_some(limits.max_open_orders));
    let beta_cap = beta_controls
        .user(user_id)
        .and_then(|control| control.max_open_orders);
    match (risk_cap, beta_cap) {
        (Some(lhs), Some(rhs)) => Some(lhs.min(rhs)),
        (Some(lhs), None) => Some(lhs),
        (None, Some(rhs)) => Some(rhs),
        (None, None) => None,
    }
}

fn estimated_order_notional(
    side: Side,
    amount: i64,
    submitted_price: Option<i64>,
    snapshot: Option<&MarketRuntimeSnapshot>,
) -> Option<i64> {
    let _ = side;
    let reference_price = submitted_price
        .or_else(|| snapshot.and_then(|v| v.last_trade_price.or(v.reference_price)))?;
    Some(reference_price.saturating_mul(amount.saturating_abs()))
}

async fn enforce_beta_order_controls(
    engine: &PartitionedMatchingEngine,
    risk: &RiskEngine,
    order_projection: &OrderStateProjectionStore,
    instruments: &dyn InstrumentRegistry,
    beta_controls: &BetaControlStore,
    principal: &AuthenticatedPrincipal,
    market_id: &str,
    outcome: i32,
    side: Side,
    amount: i64,
    price: Option<i64>,
    leverage: Option<u32>,
    exclude_order_id: Option<&str>,
) -> Result<(), Rejection> {
    let control_plane = beta_controls.control_plane();
    if !control_plane.enabled {
        return Ok(());
    }

    if !beta_controls.allows_user(&principal.subject) {
        return Err(reject_api(
            StatusCode::FORBIDDEN,
            "beta whitelist required for this user",
        ));
    }

    if let Some(limit) = effective_open_order_cap(risk, beta_controls, &principal.subject) {
        let current_open_orders =
            order_projection.active_order_count_for_user(&principal.subject, exclude_order_id);
        if current_open_orders >= limit as usize {
            return Err(reject_api(
                StatusCode::BAD_REQUEST,
                format!(
                    "open order cap exceeded: current_open_orders={} >= max_open_orders={}",
                    current_open_orders, limit
                ),
            ));
        }
    }

    let instrument = instruments
        .get(market_id)
        .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "unknown market_id"))?;
    let market_control = beta_controls.market(market_id);

    if let Some(requested_leverage) = leverage {
        let mut effective_max_leverage = instrument.max_leverage;
        if let Some(beta_max) = market_control.as_ref().and_then(|value| value.max_leverage) {
            effective_max_leverage =
                Some(effective_max_leverage.map_or(beta_max, |v| v.min(beta_max)));
        }
        if let Some(max_leverage) = effective_max_leverage {
            if requested_leverage > max_leverage {
                return Err(reject_api(
                    StatusCode::BAD_REQUEST,
                    format!(
                        "leverage exceeds beta/instrument cap: requested={} > allowed={}",
                        requested_leverage, max_leverage
                    ),
                ));
            }
        }
    }

    if let Some(max_order_notional) = market_control.and_then(|value| value.max_order_notional) {
        let snapshot = if price.is_some() {
            None
        } else {
            let snapshots = engine.export_snapshots().await.map_err(|error| {
                reject_api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    sanitize_internal_error(&error.to_string()),
                )
            })?;
            flatten_market_snapshots(&snapshots)
                .into_iter()
                .find(|entry| entry.market_id == market_id && entry.outcome == outcome)
        };
        let order_notional = estimated_order_notional(side, amount, price, snapshot.as_ref())
            .ok_or_else(|| {
                reject_api(
                    StatusCode::BAD_REQUEST,
                    "unable to estimate order notional for beta market cap",
                )
            })?;
        if order_notional > max_order_notional {
            return Err(reject_api(
                StatusCode::BAD_REQUEST,
                format!(
                    "beta market notional cap exceeded: order_notional={} > max_order_notional={}",
                    order_notional, max_order_notional
                ),
            ));
        }
    }

    Ok(())
}

pub(crate) fn build_trading_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    order_projection: Arc<OrderStateProjectionStore>,
    risk: Arc<RiskEngine>,
    instruments: Arc<dyn InstrumentRegistry>,
    stop_order_store: Arc<StopOrderStore>,
    beta_controls: Arc<BetaControlStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
    system_sentinel: Arc<sentinel::SystemSentinel>,
    event_bus: eventbus::EventBus,
) -> JsonRoute {
    let sequencer_for_intent = sequencer.clone();
    let order_projection_for_intent = order_projection.clone();
    let risk_for_intent = risk.clone();
    let ip_rate_limiter_for_intent = ip_rate_limiter.clone();
    let user_rate_limiter_for_intent = user_rate_limiter.clone();
    let partitioned_engine_for_intent = partitioned_engine.clone();
    let instruments_for_intent = instruments.clone();
    let beta_controls_for_intent = beta_controls.clone();
    let sentinel_for_intent = system_sentinel.clone();
    let event_bus_for_intent = event_bus.clone();
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
                let order_projection = order_projection_for_intent.clone();
                let risk = risk_for_intent.clone();
                let user_rate_limiter = user_rate_limiter_for_intent.clone();
                let ip_rate_limiter = ip_rate_limiter_for_intent.clone();
                let instruments = instruments_for_intent.clone();
                let beta_controls = beta_controls_for_intent.clone();
                let sentinel = sentinel_for_intent.clone();
                let event_bus = event_bus_for_intent.clone();
                async move {
                    require_user(&principal)?;
                    // Normalize request_id / client_order_id up front so the
                    // api_received trace carries the canonical identifiers
                    // and the projector's trace_key bucket is keyed on the
                    // same value the sequencer will see (design §3.3.1).
                    let request_id = normalize_request_id(req.request_id);
                    let client_order_id = normalize_client_order_id(req.client_order_id);
                    api_trace::emit_new_order_received(
                        &event_bus,
                        &request_id,
                        Some(&client_order_id),
                        &principal,
                        &req.market_id,
                        req.outcome,
                        req.side,
                        Some(req.price),
                        req.amount,
                    );
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    enforce_submit_order_rate_limits(
                        &ip_rate_limiter,
                        &user_rate_limiter,
                        &ip_key,
                        &principal.subject,
                    )?;
                    if ops::is_draining() {
                        return Err(reject_api(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "system is draining — new orders rejected",
                        ));
                    }
                    if let Err(reason) = sentinel::enforce_order_posture(&sentinel) {
                        return Err(reject_api(StatusCode::SERVICE_UNAVAILABLE, reason));
                    }
                    if instruments.get(&req.market_id).is_none() {
                        return Err(reject_api(StatusCode::NOT_FOUND, "unknown market_id"));
                    }
                    validate_order_fields(&req.market_id, req.amount, Some(req.price), None, None)?;
                    enforce_beta_order_controls(
                        engine.as_ref(),
                        risk.as_ref(),
                        order_projection.as_ref(),
                        instruments.as_ref(),
                        beta_controls.as_ref(),
                        &principal,
                        &req.market_id,
                        req.outcome,
                        req.side,
                        req.amount,
                        Some(req.price),
                        None,
                        None,
                    )
                    .await?;
                    api_trace::emit_new_order_validated(
                        &event_bus,
                        &request_id,
                        Some(&client_order_id),
                        &principal,
                        &req.market_id,
                        req.outcome,
                        req.side,
                        Some(req.price),
                        req.amount,
                    );
                    audit("intent", &request_id, &principal);

                    let command = match sequence_new_order(
                        &sequencer,
                        request_id.clone(),
                        client_order_id.clone(),
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
                        types::StpMode::default(),
                        None,
                        None,
                    ) {
                        Ok(command) => command,
                        Err(error) => {
                            api_trace::emit_api_rejected_unbound(
                                &event_bus,
                                &request_id,
                                Some(&client_order_id),
                                Some(&principal.subject),
                                ApiErrorCode::InternalError,
                                error.clone(),
                            );
                            return Err(reject_api(StatusCode::BAD_REQUEST, error));
                        }
                    };

                    let match_start = Instant::now();
                    let projection_command = command.clone();
                    match engine.submit_new_order(command).await {
                        Ok(result) => {
                            let elapsed_us = match_start.elapsed().as_micros() as u64;
                            observability::METRICS.match_latency.record(elapsed_us);
                            observability::METRICS
                                .queue_wait_latency
                                .record(result.queue_wait_us);
                            observability::METRICS
                                .match_execution_latency
                                .record(result.match_execution_us);
                            observability::METRICS
                                .wal_append_latency
                                .record(result.persist_us);
                            observability::METRICS
                                .risk_latency
                                .record(result.timing.risk_us);
                            observability::METRICS
                                .matching_core_latency
                                .record(result.timing.matching_us);
                            observability::METRICS
                                .settlement_persist_latency
                                .record(result.timing.wal_us);
                            observability::METRICS
                                .post_match_latency
                                .record(result.timing.post_match_us);
                            observability::METRICS
                                .orders_received
                                .fetch_add(1, Ordering::Relaxed);
                            perf::ORDER_THROUGHPUT.record();
                            observability::METRICS
                                .orders_filled
                                .fetch_add(result.fills.len() as u64, Ordering::Relaxed);
                            if !result.fills.is_empty() {
                                perf::FILL_THROUGHPUT.record();
                            }
                            observability::METRICS.record_partition_order(result.partition);
                            observability::METRICS
                                .record_partition_fill(result.partition, result.fills.len() as u64);
                            update_lifecycle_after_submit(&sequencer, &request_id, &result);
                            if let Err(error) = order_projection.record_submit_success(
                                &projection_command,
                                &result,
                                None,
                            ) {
                                tracing::warn!(request_id, error = %error, "order projection write failed");
                            }
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
                                "match_e2e_us": elapsed_us,
                                "queue_wait_us": result.queue_wait_us,
                                "match_execution_us": result.match_execution_us,
                                "persist_us": result.persist_us,
                                "granular_timing": {
                                    "validation_us": result.timing.validation_us,
                                    "risk_us": result.timing.risk_us,
                                    "matching_core_us": result.timing.matching_us,
                                    "settlement_persist_us": result.timing.wal_us,
                                    "post_match_us": result.timing.post_match_us,
                                },
                            })))
                        }
                        Err(error) => {
                            if matches!(&error, SubmissionError::RateLimited { .. }) {
                                observability::METRICS.record_submit_order_engine_rate_limited();
                            }
                            observability::METRICS
                                .orders_rejected
                                .fetch_add(1, Ordering::Relaxed);
                            let _ = sequencer.mark_rejected(&request_id);
                            if let Err(write_error) =
                                order_projection.record_new_order_rejection(&projection_command)
                            {
                                tracing::warn!(request_id, error = %write_error, "order projection reject write failed");
                            }
                            api_trace::emit_api_rejected_unbound(
                                &event_bus,
                                &request_id,
                                Some(&client_order_id),
                                Some(&principal.subject),
                                ApiErrorCode::InternalError,
                                error.to_string(),
                            );
                            Err(reject_submission_error(&error))
                        }
                    }
                }
            },
        )
        .boxed();

    let sequencer_for_order = sequencer.clone();
    let order_projection_for_submit = order_projection.clone();
    let risk_for_submit = risk.clone();
    let ip_rate_limiter_for_submit = ip_rate_limiter.clone();
    let user_rate_limiter_for_submit = user_rate_limiter.clone();
    let partitioned_engine_1 = partitioned_engine.clone();
    let instruments_for_submit = instruments.clone();
    let stop_store_for_submit = stop_order_store.clone();
    let beta_controls_for_submit = beta_controls.clone();
    let sentinel_for_submit = system_sentinel.clone();
    let event_bus_for_submit = event_bus.clone();
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
                let order_projection = order_projection_for_submit.clone();
                let risk = risk_for_submit.clone();
                let user_rate_limiter = user_rate_limiter_for_submit.clone();
                let ip_rate_limiter = ip_rate_limiter_for_submit.clone();
                let instruments = instruments_for_submit.clone();
                let stop_store = stop_store_for_submit.clone();
                let beta_controls = beta_controls_for_submit.clone();
                let sentinel = sentinel_for_submit.clone();
                let event_bus = event_bus_for_submit.clone();
                async move {
                    require_user(&principal)?;
                    let request_id = normalize_request_id(req.request_id);
                    let client_order_id = normalize_client_order_id(req.client_order_id);
                    api_trace::emit_new_order_received(
                        &event_bus,
                        &request_id,
                        Some(&client_order_id),
                        &principal,
                        &req.market_id,
                        req.outcome,
                        req.side,
                        req.price,
                        req.amount,
                    );
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    enforce_submit_order_rate_limits(
                        &ip_rate_limiter,
                        &user_rate_limiter,
                        &ip_key,
                        &principal.subject,
                    )?;
                    if ops::is_draining() {
                        return Err(reject_api(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "system is draining — new orders rejected",
                        ));
                    }
                    if let Err(reason) = sentinel::enforce_order_posture(&sentinel) {
                        return Err(reject_api(StatusCode::SERVICE_UNAVAILABLE, reason));
                    }
                    if instruments.get(&req.market_id).is_none() {
                        return Err(reject_api(StatusCode::NOT_FOUND, "unknown market_id"));
                    }
                    validate_order_fields(
                        &req.market_id,
                        req.amount,
                        req.price,
                        req.leverage,
                        req.order_type,
                    )?;
                    let order_type = req.order_type.unwrap_or(OrderType::Limit);
                    let time_in_force = req.time_in_force.unwrap_or(TimeInForce::Gtc);
                    let post_only = req.post_only.unwrap_or(false);
                    let reduce_only = req.reduce_only.unwrap_or(false);
                    enforce_beta_order_controls(
                        engine.as_ref(),
                        risk.as_ref(),
                        order_projection.as_ref(),
                        instruments.as_ref(),
                        beta_controls.as_ref(),
                        &principal,
                        &req.market_id,
                        req.outcome,
                        req.side,
                        req.amount,
                        req.price,
                        req.leverage,
                        None,
                    )
                    .await?;
                    api_trace::emit_new_order_validated(
                        &event_bus,
                        &request_id,
                        Some(&client_order_id),
                        &principal,
                        &req.market_id,
                        req.outcome,
                        req.side,
                        req.price,
                        req.amount,
                    );
                    if time_in_force == TimeInForce::Gtd {
                        if let Some(expires_at) = req.expires_at {
                            if expires_at <= Utc::now() {
                                return Err(reject_api(
                                    StatusCode::BAD_REQUEST,
                                    "expires_at must be in the future for GTD orders",
                                ));
                            }
                        } else {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "expires_at is required for GTD orders",
                            ));
                        }
                    }
                    audit("submit_order", &request_id, &principal);

                    // Conditional orders (stop/take-profit) are routed to the stop
                    // order store instead of the matching engine.
                    if order_type.is_conditional() {
                        let trigger_price = req.trigger_price.ok_or_else(|| {
                            reject_api(
                                StatusCode::BAD_REQUEST,
                                "trigger_price is required for conditional orders",
                            )
                        })?;
                        if trigger_price <= 0 {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "trigger_price must be positive",
                            ));
                        }
                        let trigger_type =
                            req.trigger_type.unwrap_or(types::TriggerType::LastPrice);
                        if trigger_type != types::TriggerType::LastPrice {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "only last_price trigger type is currently supported",
                            ));
                        }
                        if matches!(
                            order_type,
                            OrderType::StopLimit | OrderType::TakeProfitLimit
                        ) && req.price.is_none()
                        {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "price is required for limit-type conditional orders",
                            ));
                        }
                        let stop_order_id = types::generate_op_id("stop");
                        let record = StopOrderRecord {
                            stop_order_id: stop_order_id.clone(),
                            user_id: principal.subject.clone(),
                            market_id: req.market_id.clone(),
                            outcome: req.outcome,
                            side: req.side,
                            order_type,
                            trigger_price,
                            trigger_type: req.trigger_type.unwrap_or(types::TriggerType::LastPrice),
                            limit_price: req.price,
                            amount: req.amount,
                            time_in_force,
                            post_only,
                            reduce_only,
                            leverage: req.leverage,
                            stp_mode: req.stp_mode.unwrap_or_default(),
                            status: StopOrderStatus::Pending,
                            created_at: Utc::now(),
                            triggered_at: None,
                            cancelled_at: None,
                        };
                        stop_store.insert(record).map_err(|e| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                        })?;
                        return Ok(warp::reply::json(&serde_json::json!({
                            "status": "pending",
                            "stop_order_id": stop_order_id,
                            "order_type": order_type,
                            "trigger_price": trigger_price,
                        })));
                    }

                    let command = match sequence_new_order(
                        &sequencer,
                        request_id.clone(),
                        client_order_id.clone(),
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
                        req.stp_mode.unwrap_or_default(),
                        req.trigger_price,
                        req.trigger_type,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    let match_start = Instant::now();
                    let projection_command = command.clone();
                    match engine.submit_new_order(command).await {
                        Ok(result) => {
                            let elapsed_us = match_start.elapsed().as_micros() as u64;
                            observability::METRICS.match_latency.record(elapsed_us);
                            observability::METRICS
                                .queue_wait_latency
                                .record(result.queue_wait_us);
                            observability::METRICS
                                .match_execution_latency
                                .record(result.match_execution_us);
                            observability::METRICS
                                .wal_append_latency
                                .record(result.persist_us);
                            observability::METRICS
                                .risk_latency
                                .record(result.timing.risk_us);
                            observability::METRICS
                                .matching_core_latency
                                .record(result.timing.matching_us);
                            observability::METRICS
                                .settlement_persist_latency
                                .record(result.timing.wal_us);
                            observability::METRICS
                                .post_match_latency
                                .record(result.timing.post_match_us);
                            observability::METRICS
                                .orders_received
                                .fetch_add(1, Ordering::Relaxed);
                            observability::METRICS
                                .orders_filled
                                .fetch_add(result.fills.len() as u64, Ordering::Relaxed);
                            observability::METRICS.record_partition_order(result.partition);
                            observability::METRICS
                                .record_partition_fill(result.partition, result.fills.len() as u64);
                            update_lifecycle_after_submit(&sequencer, &request_id, &result);
                            if let Err(error) = order_projection.record_submit_success(
                                &projection_command,
                                &result,
                                None,
                            ) {
                                tracing::warn!(request_id, error = %error, "order projection write failed");
                            }
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
                                "match_e2e_us": elapsed_us,
                                "queue_wait_us": result.queue_wait_us,
                                "match_execution_us": result.match_execution_us,
                                "persist_us": result.persist_us,
                                "granular_timing": {
                                    "validation_us": result.timing.validation_us,
                                    "risk_us": result.timing.risk_us,
                                    "matching_core_us": result.timing.matching_us,
                                    "settlement_persist_us": result.timing.wal_us,
                                    "post_match_us": result.timing.post_match_us,
                                },
                            });
                            Ok::<_, warp::Rejection>(warp::reply::json(&resp))
                        }
                        Err(error) => {
                            observability::METRICS
                                .orders_rejected
                                .fetch_add(1, Ordering::Relaxed);
                            let _ = sequencer.mark_rejected(&request_id);
                            if let Err(write_error) =
                                order_projection.record_new_order_rejection(&projection_command)
                            {
                                tracing::warn!(request_id, error = %write_error, "order projection reject write failed");
                            }
                            api_trace::emit_api_rejected_unbound(
                                &event_bus,
                                &request_id,
                                Some(&client_order_id),
                                Some(&principal.subject),
                                ApiErrorCode::InternalError,
                                error.to_string(),
                            );
                            Err(reject_submission_error(&error))
                        }
                    }
                }
            },
        )
        .boxed();

    let partitioned_engine_3 = partitioned_engine.clone();
    let ip_rate_limiter_for_cancel = ip_rate_limiter.clone();
    let user_rate_limiter_for_cancel = user_rate_limiter.clone();
    let sequencer_for_cancel_order = sequencer.clone();
    let order_projection_for_cancel = order_projection.clone();
    let event_bus_for_cancel = event_bus.clone();
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
                let order_projection = order_projection_for_cancel.clone();
                let user_rate_limiter = user_rate_limiter_for_cancel.clone();
                let ip_rate_limiter = ip_rate_limiter_for_cancel.clone();
                let event_bus = event_bus_for_cancel.clone();
                async move {
                    require_user(&principal)?;
                    let request_id = normalize_request_id(req.request_id);
                    let user_id = principal.subject.clone();
                    let order_id = req.order_id.clone();
                    let market_id = req.market_id.clone();
                    api_trace::emit_for_order_received(
                        &event_bus,
                        &order_id,
                        &request_id,
                        &principal,
                        Some(&market_id),
                        req.outcome,
                    );
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    api_trace::emit_for_order_validated(
                        &event_bus,
                        &order_id,
                        &request_id,
                        &principal,
                        Some(&market_id),
                        req.outcome,
                    );
                    audit("cancel_order", &request_id, &principal);
                    let command = match sequence_cancel_order(
                        &sequencer,
                        request_id.clone(),
                        user_id.clone(),
                        req.market_id,
                        req.outcome,
                        req.order_id,
                        req.client_order_id,
                    ) {
                        Ok(command) => command,
                        Err(error) => {
                            api_trace::emit_for_order_rejected(
                                &event_bus,
                                &order_id,
                                &request_id,
                                &user_id,
                                ApiErrorCode::InternalError,
                                error.clone(),
                            );
                            return Err(reject_api(StatusCode::BAD_REQUEST, error));
                        }
                    };

                    match engine.cancel_order(command).await {
                        Ok(result) => {
                            update_lifecycle_after_cancel(&sequencer, &request_id);
                            if let Err(error) = order_projection.record_cancelled_orders(
                                &user_id,
                                &result.cancelled_order_ids,
                                result.metadata.command_seq,
                                "cancel_order",
                            ) {
                                tracing::warn!(request_id, error = %error, "order projection cancel write failed");
                            }
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
                            api_trace::emit_for_order_rejected(
                                &event_bus,
                                &order_id,
                                &request_id,
                                &user_id,
                                ApiErrorCode::InternalError,
                                error.to_string(),
                            );
                            Err(reject_submission_error(&error))
                        }
                    }
                }
            },
        )
        .boxed();

    let partitioned_engine_2b = partitioned_engine.clone();
    let risk_for_replace = risk.clone();
    let ip_rate_limiter_for_replace = ip_rate_limiter.clone();
    let user_rate_limiter_for_replace = user_rate_limiter.clone();
    let sequencer_for_replace_order = sequencer.clone();
    let order_projection_for_replace = order_projection.clone();
    let instruments_for_replace = instruments.clone();
    let beta_controls_for_replace = beta_controls.clone();
    let event_bus_for_replace = event_bus.clone();
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
                let risk = risk_for_replace.clone();
                let sequencer = sequencer_for_replace_order.clone();
                let order_projection = order_projection_for_replace.clone();
                let instruments = instruments_for_replace.clone();
                let beta_controls = beta_controls_for_replace.clone();
                let user_rate_limiter = user_rate_limiter_for_replace.clone();
                let ip_rate_limiter = ip_rate_limiter_for_replace.clone();
                let event_bus = event_bus_for_replace.clone();
                async move {
                    require_user(&principal)?;
                    let request_id = normalize_request_id(req.request_id);
                    let target_order_id = req.order_id.clone();
                    api_trace::emit_for_order_received(
                        &event_bus,
                        &target_order_id,
                        &request_id,
                        &principal,
                        Some(&req.market_id),
                        req.outcome,
                    );
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    if req.market_id.len() > MAX_ID_LEN {
                        return Err(reject_api(StatusCode::BAD_REQUEST, "market_id too long"));
                    }
                    if let Some(p) = req.new_price {
                        if p <= 0 {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "new_price must be positive",
                            ));
                        }
                    }
                    if let Some(a) = req.new_amount {
                        if a <= 0 {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "new_amount must be positive",
                            ));
                        }
                    }
                    if req.new_leverage == Some(0) {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "new_leverage must be >= 1",
                        ));
                    }
                    let existing_projection = order_projection.get(&principal.subject, &req.order_id);
                    enforce_beta_order_controls(
                        engine.as_ref(),
                        risk.as_ref(),
                        order_projection.as_ref(),
                        instruments.as_ref(),
                        beta_controls.as_ref(),
                        &principal,
                        &req.market_id,
                        req.outcome
                            .or_else(|| existing_projection.as_ref().map(|value| value.outcome))
                            .unwrap_or_default(),
                        existing_projection
                            .as_ref()
                            .map(|value| value.side)
                            .unwrap_or(Side::Buy),
                        req.new_amount.unwrap_or_else(|| {
                            existing_projection
                                .as_ref()
                                .map(|value| value.remaining_amount.max(value.original_amount))
                                .unwrap_or(1)
                        }),
                        req.new_price
                            .or_else(|| existing_projection.as_ref().and_then(|value| value.price)),
                        req.new_leverage.or_else(|| {
                            existing_projection.as_ref().and_then(|value| value.leverage)
                        }),
                        Some(&req.order_id),
                    )
                    .await?;
                    let user_id = principal.subject.clone();
                    api_trace::emit_for_order_validated(
                        &event_bus,
                        &target_order_id,
                        &request_id,
                        &principal,
                        Some(&req.market_id),
                        req.outcome,
                    );
                    audit("replace_order", &request_id, &principal);
                    let command = match sequence_replace_order(
                        &sequencer,
                        request_id.clone(),
                        user_id.clone(),
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
                    ) {
                        Ok(command) => command,
                        Err(error) => {
                            api_trace::emit_for_order_rejected(
                                &event_bus,
                                &target_order_id,
                                &request_id,
                                &user_id,
                                ApiErrorCode::InternalError,
                                error.clone(),
                            );
                            return Err(reject_api(StatusCode::BAD_REQUEST, error));
                        }
                    };

                    let projection_command = command.clone();
                    match engine.replace_order(command).await {
                        Ok(result) => {
                            observability::METRICS
                                .orders_received
                                .fetch_add(1, Ordering::Relaxed);
                            observability::METRICS
                                .orders_filled
                                .fetch_add(result.fills.len() as u64, Ordering::Relaxed);
                            if projection_command.new_client_order_id.is_none() {
                                if let Err(error) = sequencer.record_generated_replace_order_id(
                                    &request_id,
                                    &result.order_id,
                                ) {
                                    tracing::warn!(request_id, error = %error, "sequencer replace-order id persistence failed");
                                }
                            }
                            update_lifecycle_after_submit(&sequencer, &request_id, &result);
                            if let Err(error) =
                                order_projection.record_replace_success(&projection_command, &result)
                            {
                                tracing::warn!(request_id, error = %error, "order projection replace write failed");
                            }
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
                            observability::METRICS
                                .orders_rejected
                                .fetch_add(1, Ordering::Relaxed);
                            let _ = sequencer.mark_rejected(&request_id);
                            if let Err(write_error) =
                                order_projection.record_replace_rejection(&projection_command)
                            {
                                tracing::warn!(request_id, error = %write_error, "order projection replace reject write failed");
                            }
                            api_trace::emit_for_order_rejected(
                                &event_bus,
                                &target_order_id,
                                &request_id,
                                &user_id,
                                ApiErrorCode::InternalError,
                                error.to_string(),
                            );
                            Err(reject_submission_error(&error))
                        }
                    }
                }
            },
        )
        .boxed();

    let partitioned_engine_4 = partitioned_engine.clone();
    let ip_rate_limiter_for_mass_cancel_user = ip_rate_limiter.clone();
    let user_rate_limiter_for_mass_cancel_user = user_rate_limiter.clone();
    let sequencer_for_mass_cancel_user = sequencer.clone();
    let order_projection_for_mass_cancel_user = order_projection.clone();
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
                let order_projection = order_projection_for_mass_cancel_user.clone();
                let user_rate_limiter = user_rate_limiter_for_mass_cancel_user.clone();
                let ip_rate_limiter = ip_rate_limiter_for_mass_cancel_user.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    let request_id = normalize_request_id(req.request_id);
                    let user_id = principal.subject.clone();
                    audit("mass_cancel_user", &request_id, &principal);
                    let command = match sequence_mass_cancel_by_user(
                        &sequencer,
                        request_id.clone(),
                        user_id.clone(),
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.mass_cancel_by_user(command).await {
                        Ok(result) => {
                            update_lifecycle_after_cancel(&sequencer, &request_id);
                            if let Err(error) = order_projection.record_cancelled_orders(
                                &user_id,
                                &result.cancelled_order_ids,
                                result.metadata.command_seq,
                                "mass_cancel_user",
                            ) {
                                tracing::warn!(request_id, error = %error, "order projection mass cancel user write failed");
                            }
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
        )
        .boxed();

    let partitioned_engine_5 = partitioned_engine.clone();
    let ip_rate_limiter_for_mass_cancel_session = ip_rate_limiter.clone();
    let user_rate_limiter_for_mass_cancel_session = user_rate_limiter.clone();
    let sequencer_for_mass_cancel_session = sequencer.clone();
    let order_projection_for_mass_cancel_session = order_projection.clone();
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
                let order_projection = order_projection_for_mass_cancel_session.clone();
                let user_rate_limiter = user_rate_limiter_for_mass_cancel_session.clone();
                let ip_rate_limiter = ip_rate_limiter_for_mass_cancel_session.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
                    let request_id = normalize_request_id(req.request_id);
                    let user_id = principal.subject.clone();
                    audit("mass_cancel_session", &request_id, &principal);
                    let command = match sequence_mass_cancel_by_session(
                        &sequencer,
                        request_id.clone(),
                        user_id.clone(),
                        req.session_id,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    match engine.mass_cancel_by_session(command).await {
                        Ok(result) => {
                            update_lifecycle_after_cancel(&sequencer, &request_id);
                            if let Err(error) = order_projection.record_cancelled_orders(
                                &user_id,
                                &result.cancelled_order_ids,
                                result.metadata.command_seq,
                                "mass_cancel_session",
                            ) {
                                tracing::warn!(request_id, error = %error, "order projection mass cancel session write failed");
                            }
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
        )
        .boxed();

    // ── Batch order submission ───────────────────────────────
    let batch_engine = partitioned_engine.clone();
    let batch_sequencer = sequencer.clone();
    let batch_order_projection = order_projection.clone();
    let batch_risk = risk.clone();
    let batch_instruments = instruments.clone();
    let batch_stop_store = stop_order_store.clone();
    let batch_beta_controls = beta_controls.clone();
    let batch_ip_rl = ip_rate_limiter.clone();
    let batch_user_rl = user_rate_limiter.clone();
    let batch_order_route = warp::path("batch-orders")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: BatchOrderRequest| {
                let engine = batch_engine.clone();
                let sequencer = batch_sequencer.clone();
                let order_projection = batch_order_projection.clone();
                let risk = batch_risk.clone();
                let instruments = batch_instruments.clone();
                let stop_store = batch_stop_store.clone();
                let beta_controls = batch_beta_controls.clone();
                let ip_rl = batch_ip_rl.clone();
                let user_rl = batch_user_rl.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-batch:{}", principal.subject), 10)?;
                    if req.orders.is_empty() {
                        return Err(reject_api(StatusCode::BAD_REQUEST, "orders array is empty"));
                    }
                    if req.orders.len() > 20 {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "maximum 20 orders per batch",
                        ));
                    }
                    // Phase 1: Validate ALL orders first — fail entire batch on any error
                    enum ValidatedOrder {
                        Regular {
                            request_id: String,
                            command: types::NewOrderCommand,
                        },
                        Conditional {
                            market_id: String,
                            side: types::Side,
                            order_type: OrderType,
                            trigger_price: i64,
                            trigger_type: types::TriggerType,
                            price: Option<i64>,
                            amount: i64,
                            outcome: i32,
                            time_in_force: TimeInForce,
                            post_only: bool,
                            reduce_only: bool,
                            leverage: Option<u32>,
                            stp_mode: types::StpMode,
                        },
                    }
                    let mut validated = Vec::with_capacity(req.orders.len());
                    for order in &req.orders {
                        if instruments.get(&order.market_id).is_none() {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                &format!("unknown market_id: {}", order.market_id),
                            ));
                        }
                        if let Err(e) = validate_order_fields(
                            &order.market_id,
                            order.amount,
                            order.price,
                            order.leverage,
                            order.order_type,
                        ) {
                            return Err(e);
                        }
                        enforce_beta_order_controls(
                            engine.as_ref(),
                            risk.as_ref(),
                            order_projection.as_ref(),
                            instruments.as_ref(),
                            beta_controls.as_ref(),
                            &principal,
                            &order.market_id,
                            order.outcome,
                            order.side,
                            order.amount,
                            order.price,
                            order.leverage,
                            None,
                        )
                        .await?;
                        let order_type = order.order_type.unwrap_or(OrderType::Limit);
                        if order_type.is_conditional() {
                            let trigger_price = match order.trigger_price {
                                Some(tp) if tp > 0 => tp,
                                _ => {
                                    return Err(reject_api(
                                        StatusCode::BAD_REQUEST,
                                        "trigger_price required and must be > 0 for conditional orders",
                                    ));
                                }
                            };
                            let trigger_type =
                                order.trigger_type.unwrap_or(types::TriggerType::LastPrice);
                            if trigger_type != types::TriggerType::LastPrice {
                                return Err(reject_api(
                                    StatusCode::BAD_REQUEST,
                                    "only last_price trigger type is currently supported",
                                ));
                            }
                            validated.push(ValidatedOrder::Conditional {
                                market_id: order.market_id.clone(),
                                side: order.side,
                                order_type,
                                trigger_price,
                                trigger_type,
                                price: order.price,
                                amount: order.amount,
                                outcome: order.outcome,
                                time_in_force: order.time_in_force.unwrap_or(TimeInForce::Gtc),
                                post_only: order.post_only.unwrap_or(false),
                                reduce_only: order.reduce_only.unwrap_or(false),
                                leverage: order.leverage,
                                stp_mode: order.stp_mode.unwrap_or_default(),
                            });
                        } else {
                            if let Some(limit) = effective_open_order_cap(
                                risk.as_ref(),
                                beta_controls.as_ref(),
                                &principal.subject,
                            ) {
                                let current_open_orders = order_projection
                                    .active_order_count_for_user(&principal.subject, None);
                                let pending_regular_orders = validated
                                    .iter()
                                    .filter(|item| matches!(item, ValidatedOrder::Regular { .. }))
                                    .count();
                                if current_open_orders + pending_regular_orders >= limit as usize {
                                    return Err(reject_api(
                                        StatusCode::BAD_REQUEST,
                                        format!(
                                            "open order cap exceeded for batch: current_open_orders={} pending_regular_orders={} max_open_orders={}",
                                            current_open_orders, pending_regular_orders, limit
                                        ),
                                    ));
                                }
                            }
                            let request_id = normalize_request_id(order.request_id.clone());
                            let client_order_id = normalize_client_order_id(order.client_order_id.clone());
                            let time_in_force = order.time_in_force.unwrap_or(TimeInForce::Gtc);
                            let post_only = order.post_only.unwrap_or(false);
                            let reduce_only = order.reduce_only.unwrap_or(false);
                            let command = match sequence_new_order(
                                &sequencer,
                                request_id.clone(),
                                client_order_id,
                                principal.subject.clone(),
                                principal.session_id.clone().or(order.session_id.clone()),
                                order.market_id.clone(),
                                order.side,
                                order_type,
                                time_in_force,
                                order.price,
                                order.amount,
                                order.outcome,
                                post_only,
                                reduce_only,
                                order.leverage,
                                order.expires_at,
                                order.stp_mode.unwrap_or_default(),
                                order.trigger_price,
                                order.trigger_type,
                            ) {
                                Ok(cmd) => cmd,
                                Err(error) => {
                                    return Err(reject_api(StatusCode::BAD_REQUEST, &error));
                                }
                            };
                            validated.push(ValidatedOrder::Regular {
                                request_id,
                                command,
                            });
                        }
                    }
                    // Phase 2: Execute all validated orders
                    let mut results = Vec::with_capacity(validated.len());
                    for vo in validated {
                        match vo {
                            ValidatedOrder::Conditional {
                                market_id,
                                side,
                                order_type,
                                trigger_price,
                                trigger_type,
                                price,
                                amount,
                                outcome,
                                time_in_force,
                                post_only,
                                reduce_only,
                                leverage,
                                stp_mode,
                            } => {
                                let stop_order_id = types::generate_op_id("stop");
                                let record = StopOrderRecord {
                                    stop_order_id: stop_order_id.clone(),
                                    user_id: principal.subject.clone(),
                                    market_id,
                                    outcome,
                                    side,
                                    order_type,
                                    trigger_price,
                                    trigger_type,
                                    limit_price: price,
                                    amount,
                                    time_in_force,
                                    post_only,
                                    reduce_only,
                                    leverage,
                                    stp_mode,
                                    status: StopOrderStatus::Pending,
                                    created_at: Utc::now(),
                                    triggered_at: None,
                                    cancelled_at: None,
                                };
                                match stop_store.insert(record) {
                                    Ok(()) => {
                                        results.push(serde_json::json!({
                                            "status": "pending",
                                            "stop_order_id": stop_order_id,
                                        }));
                                    }
                                    Err(e) => {
                                        results.push(serde_json::json!({
                                            "status": "error",
                                            "error": e.to_string(),
                                        }));
                                    }
                                }
                            }
                            ValidatedOrder::Regular {
                                request_id,
                                command,
                            } => {
                                let projection_command = command.clone();
                                match engine.submit_new_order(command).await {
                                    Ok(result) => {
                                        observability::METRICS
                                            .orders_received
                                            .fetch_add(1, Ordering::Relaxed);
                                        observability::METRICS
                                            .orders_filled
                                            .fetch_add(result.fills.len() as u64, Ordering::Relaxed);
                                        update_lifecycle_after_submit(&sequencer, &request_id, &result);
                                        if let Err(error) = order_projection.record_submit_success(
                                            &projection_command,
                                            &result,
                                            None,
                                        ) {
                                            tracing::warn!(request_id, error = %error, "order projection batch write failed");
                                        }
                                        results.push(serde_json::json!({
                                            "status": "ok",
                                            "order_id": result.order_id,
                                            "fills": result.fills.len(),
                                            "remaining_amount": result.remaining_amount,
                                        }));
                                    }
                                    Err(error) => {
                                        let _ = sequencer.mark_rejected(&request_id);
                                        if let Err(write_error) = order_projection
                                            .record_new_order_rejection(&projection_command)
                                        {
                                            tracing::warn!(request_id, error = %write_error, "order projection batch reject write failed");
                                        }
                                        results.push(serde_json::json!({
                                            "status": "error",
                                            "error": error.to_string(),
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    Ok::<_, warp::Rejection>(warp::reply::json(
                        &serde_json::json!({ "results": results }),
                    ))
                }
            },
        )
        .boxed();

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
        .or(batch_order_route)
        .unify()
        .boxed()
}
