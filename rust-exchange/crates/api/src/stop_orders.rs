use super::*;

// ── Stop Order Record ────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct StopOrderRecord {
    pub(crate) stop_order_id: String,
    pub(crate) user_id: String,
    pub(crate) market_id: String,
    pub(crate) outcome: i32,
    pub(crate) side: Side,
    pub(crate) order_type: OrderType,
    pub(crate) trigger_price: i64,
    pub(crate) trigger_type: types::TriggerType,
    /// Limit price for StopLimit/TakeProfitLimit orders.
    pub(crate) limit_price: Option<i64>,
    pub(crate) amount: i64,
    pub(crate) time_in_force: TimeInForce,
    pub(crate) post_only: bool,
    pub(crate) reduce_only: bool,
    pub(crate) leverage: Option<u32>,
    pub(crate) stp_mode: types::StpMode,
    pub(crate) status: StopOrderStatus,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) triggered_at: Option<DateTime<Utc>>,
    pub(crate) cancelled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StopOrderStatus {
    Pending,
    Triggered,
    Cancelled,
    Failed,
}

// ── In-memory store backed by WAL ────────────────────────────

pub(crate) struct StopOrderStore {
    orders: DashMap<String, StopOrderRecord>,
    store: Arc<dyn persistence::WalStore<StopOrderRecord>>,
    write_lock: Mutex<()>,
}

impl StopOrderStore {
    pub(crate) fn new(
        store: Arc<dyn persistence::WalStore<StopOrderRecord>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            orders: DashMap::new(),
            store,
            write_lock: Mutex::new(()),
        };
        for record in result.store.entries()? {
            result.orders.insert(record.stop_order_id.clone(), record);
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<StopOrderRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub(crate) fn insert(&self, record: StopOrderRecord) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store.append(&record)?;
        self.orders.insert(record.stop_order_id.clone(), record);
        Ok(())
    }

    pub(crate) fn cancel(
        &self,
        stop_order_id: &str,
        user_id: &str,
    ) -> anyhow::Result<StopOrderRecord> {
        let _guard = self.write_lock.lock();
        let record = self
            .orders
            .get(stop_order_id)
            .map(|r| r.clone())
            .ok_or_else(|| anyhow::anyhow!("stop order not found"))?;
        if record.user_id != user_id {
            anyhow::bail!("not authorized to cancel this stop order");
        }
        if record.status != StopOrderStatus::Pending {
            anyhow::bail!("stop order is not pending (status: {:?})", record.status);
        }
        let mut updated = record;
        updated.status = StopOrderStatus::Cancelled;
        updated.cancelled_at = Some(Utc::now());
        self.store.append(&updated)?;
        self.orders
            .insert(stop_order_id.to_string(), updated.clone());
        Ok(updated)
    }

    pub(crate) fn list_for_user(&self, user_id: &str, limit: usize) -> Vec<StopOrderRecord> {
        let mut items: Vec<StopOrderRecord> = self
            .orders
            .iter()
            .filter(|entry| entry.value().user_id == user_id)
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        items.truncate(limit);
        items
    }

    /// Find all pending stop orders for a market+outcome that should trigger
    /// given the current price.
    pub(crate) fn check_triggers(
        &self,
        market_id: &str,
        outcome: i32,
        trigger_type_filter: types::TriggerType,
        current_price: i64,
    ) -> Vec<StopOrderRecord> {
        self.orders
            .iter()
            .filter_map(|entry| {
                let record = entry.value();
                if record.status != StopOrderStatus::Pending {
                    return None;
                }
                if record.market_id != market_id || record.outcome != outcome {
                    return None;
                }
                if record.trigger_type != trigger_type_filter {
                    return None;
                }
                let triggered = match record.order_type {
                    // Stop orders trigger when price moves AGAINST position:
                    // - StopMarket/StopLimit Buy: trigger when price >= trigger_price
                    // - StopMarket/StopLimit Sell: trigger when price <= trigger_price
                    OrderType::StopMarket | OrderType::StopLimit => match record.side {
                        Side::Buy => current_price >= record.trigger_price,
                        Side::Sell => current_price <= record.trigger_price,
                    },
                    // Take-profit: trigger when price moves IN FAVOR:
                    // - TakeProfitMarket/TakeProfitLimit Buy: trigger when price <= trigger_price
                    // - TakeProfitMarket/TakeProfitLimit Sell: trigger when price >= trigger_price
                    OrderType::TakeProfitMarket | OrderType::TakeProfitLimit => match record.side {
                        Side::Buy => current_price <= record.trigger_price,
                        Side::Sell => current_price >= record.trigger_price,
                    },
                    _ => false,
                };
                if triggered {
                    Some(record.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Mark a stop order as triggered. Returns Err if not in Pending state.
    pub(crate) fn mark_triggered(&self, stop_order_id: &str) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        let record = self
            .orders
            .get(stop_order_id)
            .map(|r| r.clone())
            .ok_or_else(|| anyhow::anyhow!("stop order not found"))?;
        if record.status != StopOrderStatus::Pending {
            anyhow::bail!("cannot trigger stop order in {:?} state", record.status);
        }
        let mut updated = record;
        updated.status = StopOrderStatus::Triggered;
        updated.triggered_at = Some(Utc::now());
        self.store.append(&updated)?;
        self.orders.insert(stop_order_id.to_string(), updated);
        Ok(())
    }

    /// Mark a stop order as failed (trigger attempted but submission failed).
    /// Returns Err if not in Pending state.
    pub(crate) fn mark_failed(&self, stop_order_id: &str) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        let record = self
            .orders
            .get(stop_order_id)
            .map(|r| r.clone())
            .ok_or_else(|| anyhow::anyhow!("stop order not found"))?;
        if record.status != StopOrderStatus::Pending {
            anyhow::bail!("cannot mark failed stop order in {:?} state", record.status);
        }
        let mut updated = record;
        updated.status = StopOrderStatus::Failed;
        self.store.append(&updated)?;
        self.orders.insert(stop_order_id.to_string(), updated);
        Ok(())
    }
}

// ── Routes ───────────────────────────────────────────────────

pub(crate) fn build_stop_order_routes(
    stop_store: Arc<StopOrderStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let store_for_list = stop_store.clone();
    let ip_rl_list = ip_rate_limiter.clone();
    let user_rl_list = user_rate_limiter.clone();

    let list_route = warp::path!("stop-orders" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let store = store_for_list.clone();
                let ip_rl = ip_rl_list.clone();
                let user_rl = user_rl_list.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-read:{}", principal.subject), 30)?;
                    let items: Vec<serde_json::Value> = store
                        .list_for_user(&user_id, 200)
                        .into_iter()
                        .map(|r| {
                            serde_json::json!({
                                "stop_order_id": r.stop_order_id,
                                "market_id": r.market_id,
                                "outcome": r.outcome,
                                "side": r.side,
                                "order_type": r.order_type,
                                "trigger_price": r.trigger_price,
                                "trigger_type": r.trigger_type,
                                "limit_price": r.limit_price,
                                "amount": r.amount,
                                "time_in_force": r.time_in_force,
                                "status": r.status,
                                "created_at": r.created_at,
                                "triggered_at": r.triggered_at,
                                "cancelled_at": r.cancelled_at,
                            })
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&items))
                }
            },
        );

    let store_for_cancel = stop_store.clone();
    let ip_rl_cancel = ip_rate_limiter.clone();
    let user_rl_cancel = user_rate_limiter.clone();

    let cancel_route = warp::path!("cancel-stop-order" / String)
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |stop_order_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let store = store_for_cancel.clone();
                let ip_rl = ip_rl_cancel.clone();
                let user_rl = user_rl_cancel.clone();
                async move {
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rl.check(&format!("ip:{ip_key}"), 30)?;
                    user_rl.check(&format!("user-write:{}", principal.subject), 10)?;
                    let record = store
                        .cancel(&stop_order_id, &principal.subject)
                        .map_err(|e| reject_api(StatusCode::BAD_REQUEST, e.to_string()))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "cancelled",
                        "stop_order_id": record.stop_order_id,
                    })))
                }
            },
        );

    list_route.or(cancel_route).unify().boxed()
}

// ── Trigger monitor ──────────────────────────────────────────

/// Bridge: listens for trade events and checks stop order triggers.
pub(crate) async fn bridge_trades_to_stop_triggers(
    eventbus: eventbus::EventBus,
    stop_store: Arc<StopOrderStore>,
    engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    hub: Arc<WsHub>,
) {
    let mut rx = eventbus.subscribe("fill.created");
    loop {
        match rx.recv().await {
            Ok(types::Event::FillCreated(fill)) => {
                // Check stop orders triggered by last trade price.
                let triggered = stop_store.check_triggers(
                    &fill.market_id,
                    fill.outcome,
                    types::TriggerType::LastPrice,
                    fill.price,
                );
                for record in triggered {
                    let execution_type = record.order_type.triggered_type();
                    let price = match execution_type {
                        OrderType::Limit => record.limit_price,
                        _ => None, // Market orders have no price
                    };
                    let request_id = types::generate_op_id("stop-trigger");
                    let client_order_id = format!("stop:{}", record.stop_order_id);

                    let command_result = sequence_new_order(
                        &sequencer,
                        request_id.clone(),
                        client_order_id,
                        record.user_id.clone(),
                        None,
                        record.market_id.clone(),
                        record.side,
                        execution_type,
                        record.time_in_force,
                        price,
                        record.amount,
                        record.outcome,
                        record.post_only,
                        record.reduce_only,
                        record.leverage,
                        None, // no expires_at on triggered order
                        record.stp_mode,
                        None, // no further trigger
                        None,
                    );

                    match command_result {
                        Ok(command) => match engine.submit_new_order(command).await {
                            Ok(_result) => {
                                let _ = stop_store.mark_triggered(&record.stop_order_id);
                                hub.publish_user_event(
                                    &record.user_id,
                                    websocket::WsFeedEvent {
                                        event_type: "stop_triggered".into(),
                                        market_id: record.market_id.clone(),
                                        data: serde_json::json!({
                                            "stop_order_id": record.stop_order_id,
                                            "trigger_price": record.trigger_price,
                                            "order_type": record.order_type,
                                            "status": "triggered",
                                        }),
                                    },
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    stop_order_id = %record.stop_order_id,
                                    error = %e,
                                    "stop order trigger failed"
                                );
                                let _ = stop_store.mark_failed(&record.stop_order_id);
                            }
                        },
                        Err(e) => {
                            tracing::warn!(
                                stop_order_id = %record.stop_order_id,
                                error = %e,
                                "stop order sequencing failed"
                            );
                            let _ = stop_store.mark_failed(&record.stop_order_id);
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(lagged = n, "stop trigger bridge lagged");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
