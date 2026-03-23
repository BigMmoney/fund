use super::*;

/// Maximum allowed string length for user-supplied identifiers (market_id, order_id, etc.).
const MAX_ID_LEN: usize = 256;

/// Validate common order input fields that are present in both intent and submit-order.
fn validate_order_fields(
    market_id: &str,
    amount: i64,
    price: Option<i64>,
    leverage: Option<u32>,
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
    if let Some(p) = price {
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

pub(crate) fn build_trading_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    instruments: Arc<dyn InstrumentRegistry>,
    stop_order_store: Arc<StopOrderStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
    system_sentinel: Arc<sentinel::SystemSentinel>,
) -> JsonRoute {
    let sequencer_for_intent = sequencer.clone();
    let ip_rate_limiter_for_intent = ip_rate_limiter.clone();
    let user_rate_limiter_for_intent = user_rate_limiter.clone();
    let partitioned_engine_for_intent = partitioned_engine.clone();
    let instruments_for_intent = instruments.clone();
    let sentinel_for_intent = system_sentinel.clone();
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
                let instruments = instruments_for_intent.clone();
                let sentinel = sentinel_for_intent.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
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
                        return Err(reject_api(StatusCode::BAD_REQUEST, "unknown market_id"));
                    }
                    validate_order_fields(&req.market_id, req.amount, Some(req.price), None)?;
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
                        types::StpMode::default(),
                        None,
                        None,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    let match_start = Instant::now();
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
                            Err(reject_submission_error(&error))
                        }
                    }
                }
            },
        );

    let sequencer_for_order = sequencer.clone();
    let ip_rate_limiter_for_submit = ip_rate_limiter.clone();
    let user_rate_limiter_for_submit = user_rate_limiter.clone();
    let partitioned_engine_1 = partitioned_engine.clone();
    let instruments_for_submit = instruments.clone();
    let stop_store_for_submit = stop_order_store.clone();
    let sentinel_for_submit = system_sentinel.clone();
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
                let instruments = instruments_for_submit.clone();
                let stop_store = stop_store_for_submit.clone();
                let sentinel = sentinel_for_submit.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user:{}", principal.subject), 30)?;
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
                        return Err(reject_api(StatusCode::BAD_REQUEST, "unknown market_id"));
                    }
                    validate_order_fields(&req.market_id, req.amount, req.price, req.leverage)?;
                    let request_id = normalize_request_id(req.request_id);
                    let client_order_id = normalize_client_order_id(req.client_order_id);
                    let order_type = req.order_type.unwrap_or(OrderType::Limit);
                    let time_in_force = req.time_in_force.unwrap_or(TimeInForce::Gtc);
                    let post_only = req.post_only.unwrap_or(false);
                    let reduce_only = req.reduce_only.unwrap_or(false);
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
                        req.stp_mode.unwrap_or_default(),
                        req.trigger_price,
                        req.trigger_type,
                    ) {
                        Ok(command) => command,
                        Err(error) => return Err(reject_api(StatusCode::BAD_REQUEST, error)),
                    };

                    let match_start = Instant::now();
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
                                .orders_received
                                .fetch_add(1, Ordering::Relaxed);
                            observability::METRICS
                                .orders_filled
                                .fetch_add(result.fills.len() as u64, Ordering::Relaxed);
                            observability::METRICS.record_partition_order(result.partition);
                            observability::METRICS
                                .record_partition_fill(result.partition, result.fills.len() as u64);
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
                            observability::METRICS
                                .orders_rejected
                                .fetch_add(1, Ordering::Relaxed);
                            let _ = sequencer.mark_rejected(&request_id);
                            Err(reject_submission_error(&error))
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
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
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
                            Err(reject_submission_error(&error))
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
                            observability::METRICS
                                .orders_received
                                .fetch_add(1, Ordering::Relaxed);
                            observability::METRICS
                                .orders_filled
                                .fetch_add(result.fills.len() as u64, Ordering::Relaxed);
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
                            observability::METRICS
                                .orders_rejected
                                .fetch_add(1, Ordering::Relaxed);
                            let _ = sequencer.mark_rejected(&request_id);
                            Err(reject_submission_error(&error))
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
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
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
                            Err(reject_submission_error(&error))
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
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
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
                            Err(reject_submission_error(&error))
                        }
                    }
                }
            },
        );

    // ── Batch order submission ───────────────────────────────
    let batch_engine = partitioned_engine.clone();
    let batch_sequencer = sequencer.clone();
    let batch_instruments = instruments.clone();
    let batch_stop_store = stop_order_store.clone();
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
                let instruments = batch_instruments.clone();
                let stop_store = batch_stop_store.clone();
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
                    let mut results = Vec::with_capacity(req.orders.len());
                    for order in req.orders {
                        if instruments.get(&order.market_id).is_none() {
                            results.push(serde_json::json!({
                                "status": "error",
                                "error": "unknown market_id",
                            }));
                            continue;
                        }
                        if let Err(e) = validate_order_fields(
                            &order.market_id,
                            order.amount,
                            order.price,
                            order.leverage,
                        ) {
                            results.push(serde_json::json!({
                                "status": "error",
                                "error": format!("{e:?}"),
                            }));
                            continue;
                        }
                        let request_id = normalize_request_id(order.request_id);
                        let client_order_id = normalize_client_order_id(order.client_order_id);
                        let order_type = order.order_type.unwrap_or(OrderType::Limit);
                        let time_in_force = order.time_in_force.unwrap_or(TimeInForce::Gtc);
                        let post_only = order.post_only.unwrap_or(false);
                        let reduce_only = order.reduce_only.unwrap_or(false);

                        // Conditional orders → stop store
                        if order_type.is_conditional() {
                            let trigger_price = match order.trigger_price {
                                Some(tp) if tp > 0 => tp,
                                _ => {
                                    results.push(serde_json::json!({
                                        "status": "error",
                                        "error": "trigger_price required and must be > 0",
                                    }));
                                    continue;
                                }
                            };
                            let trigger_type =
                                order.trigger_type.unwrap_or(types::TriggerType::LastPrice);
                            if trigger_type != types::TriggerType::LastPrice {
                                results.push(serde_json::json!({
                                    "status": "error",
                                    "error": "only last_price trigger type is currently supported",
                                }));
                                continue;
                            }
                            let stop_order_id = types::generate_op_id("stop");
                            let record = StopOrderRecord {
                                stop_order_id: stop_order_id.clone(),
                                user_id: principal.subject.clone(),
                                market_id: order.market_id.clone(),
                                outcome: order.outcome,
                                side: order.side,
                                order_type,
                                trigger_price,
                                trigger_type,
                                limit_price: order.price,
                                amount: order.amount,
                                time_in_force,
                                post_only,
                                reduce_only,
                                leverage: order.leverage,
                                stp_mode: order.stp_mode.unwrap_or_default(),
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
                            continue;
                        }

                        let command = match sequence_new_order(
                            &sequencer,
                            request_id.clone(),
                            client_order_id,
                            principal.subject.clone(),
                            principal.session_id.clone().or(order.session_id),
                            order.market_id,
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
                                results.push(serde_json::json!({
                                    "status": "error",
                                    "error": error,
                                }));
                                continue;
                            }
                        };

                        match engine.submit_new_order(command).await {
                            Ok(result) => {
                                observability::METRICS
                                    .orders_received
                                    .fetch_add(1, Ordering::Relaxed);
                                observability::METRICS
                                    .orders_filled
                                    .fetch_add(result.fills.len() as u64, Ordering::Relaxed);
                                update_lifecycle_after_submit(&sequencer, &request_id, &result);
                                results.push(serde_json::json!({
                                    "status": "ok",
                                    "order_id": result.order_id,
                                    "fills": result.fills.len(),
                                    "remaining_amount": result.remaining_amount,
                                }));
                            }
                            Err(error) => {
                                let _ = sequencer.mark_rejected(&request_id);
                                results.push(serde_json::json!({
                                    "status": "error",
                                    "error": format!("{error}"),
                                }));
                            }
                        }
                    }
                    Ok::<_, warp::Rejection>(warp::reply::json(
                        &serde_json::json!({ "results": results }),
                    ))
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
        .or(batch_order_route)
        .unify()
        .boxed()
}
