use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct GovernanceActionRecord {
    pub(crate) action_id: String,
    pub(crate) action_type: String,
    pub(crate) payload: serde_json::Value,
    pub(crate) requested_by: String,
    pub(crate) approved_by: Option<String>,
    pub(crate) rejected_by: Option<String>,
    pub(crate) status: String,
    pub(crate) comment: Option<String>,
    pub(crate) recorded_at: DateTime<Utc>,
    pub(crate) decided_at: Option<DateTime<Utc>>,
}

pub(crate) struct PendingGovernanceActionStore {
    entries: DashMap<String, GovernanceActionRecord>,
    store: Arc<dyn persistence::WalStore<GovernanceActionRecord>>,
}

impl PendingGovernanceActionStore {
    fn new(store: Arc<dyn persistence::WalStore<GovernanceActionRecord>>) -> anyhow::Result<Self> {
        let result = Self {
            entries: DashMap::new(),
            store,
        };
        for record in result.store.entries()? {
            result.entries.insert(record.action_id.clone(), record);
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<GovernanceActionRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub(crate) fn append(&self, record: GovernanceActionRecord) -> anyhow::Result<()> {
        self.store.append(&record)?;
        self.entries.insert(record.action_id.clone(), record);
        Ok(())
    }

    pub(crate) fn get(&self, action_id: &str) -> Option<GovernanceActionRecord> {
        self.entries
            .get(action_id)
            .map(|entry| entry.value().clone())
    }

    pub(crate) fn list_recent(
        &self,
        limit: usize,
        status: Option<&str>,
    ) -> Vec<GovernanceActionRecord> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.value().clone())
            .filter(|item| status.is_none_or(|status| item.status == status))
            .collect();
        items.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));
        items.truncate(limit);
        items
    }
}

pub(crate) fn create_pending_governance_action(
    store: &PendingGovernanceActionStore,
    action_type: &str,
    payload: serde_json::Value,
    requested_by: &str,
    comment: Option<String>,
) -> anyhow::Result<GovernanceActionRecord> {
    let record = GovernanceActionRecord {
        action_id: types::generate_op_id("gov"),
        action_type: action_type.to_string(),
        payload,
        requested_by: requested_by.to_string(),
        approved_by: None,
        rejected_by: None,
        status: "pending".to_string(),
        comment,
        recorded_at: Utc::now(),
        decided_at: None,
    };
    store.append(record.clone())?;
    Ok(record)
}

pub(crate) fn apply_governance_action(
    record: &GovernanceActionRecord,
    adl_governance: &PersistentAdlGovernanceStore,
    liquidation_policy: &PersistentLiquidationPolicyStore,
    index_prices: &PersistentIndexPriceStore,
    liquidation_queue: &LiquidationQueueStore,
) -> Result<serde_json::Value, Rejection> {
    match record.action_type.as_str() {
        "adl_governance_update" => {
            let req: AdlGovernanceUpdateRequest = serde_json::from_value(record.payload.clone())
                .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            let mut current = adl_governance.current();
            current.governance.maintenance_margin_bps = req
                .maintenance_margin_bps
                .unwrap_or(current.governance.maintenance_margin_bps);
            current.governance.leverage_weight_bps = req
                .leverage_weight_bps
                .unwrap_or(current.governance.leverage_weight_bps);
            current.governance.bankruptcy_distance_weight_bps = req
                .bankruptcy_distance_weight_bps
                .unwrap_or(current.governance.bankruptcy_distance_weight_bps);
            current.governance.size_weight_bps = req
                .size_weight_bps
                .unwrap_or(current.governance.size_weight_bps);
            current.governance.buffer_weight_bps = req
                .buffer_weight_bps
                .unwrap_or(current.governance.buffer_weight_bps);
            current.governance.max_candidates = req
                .max_candidates
                .unwrap_or(current.governance.max_candidates)
                .max(1);
            current
                .governance
                .max_socialized_loss_share_bps_per_candidate = req
                .max_socialized_loss_share_bps_per_candidate
                .unwrap_or(
                    current
                        .governance
                        .max_socialized_loss_share_bps_per_candidate,
                )
                .clamp(0, 10_000);
            current.updated_by = record.requested_by.clone();
            current.recorded_at = Utc::now();
            adl_governance.upsert(current.clone()).map_err(|error| {
                reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            })?;
            Ok(serde_json::json!(current))
        }
        "liquidation_policy_update" => {
            let req: LiquidationPolicyUpdateRequest =
                serde_json::from_value(record.payload.clone())
                    .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            let mut current = liquidation_policy.current();
            current.auction_window_secs = req
                .auction_window_secs
                .unwrap_or(current.auction_window_secs)
                .max(1);
            if let Some(retry_backoff_secs) = req.retry_backoff_secs {
                current.retry_backoff_secs = retry_backoff_secs
                    .into_iter()
                    .map(|item| item.max(0))
                    .collect();
                if current.retry_backoff_secs.is_empty() {
                    current.retry_backoff_secs = vec![0, 5, 15];
                }
            }
            current.max_retry_tiers = req
                .max_retry_tiers
                .unwrap_or(current.max_retry_tiers)
                .max(1);
            current.max_auction_rounds = req
                .max_auction_rounds
                .unwrap_or(current.max_auction_rounds)
                .max(1);
            current.updated_by = record.requested_by.clone();
            current.recorded_at = Utc::now();
            liquidation_policy
                .upsert(current.clone())
                .map_err(|error| {
                    reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
            Ok(serde_json::json!(current))
        }
        "index_price_upsert" => {
            let req: IndexPriceUpsertRequest = serde_json::from_value(record.payload.clone())
                .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            if req.index_price <= 0 {
                return Err(reject_api(
                    StatusCode::BAD_REQUEST,
                    "index_price must be positive",
                ));
            }
            let value = IndexPriceRecord {
                market_id: req.market_id,
                outcome: req.outcome.unwrap_or(0),
                index_price: req.index_price,
                source: req.source.unwrap_or_else(|| "admin-manual".to_string()),
                recorded_at: Utc::now(),
            };
            index_prices.upsert(value.clone()).map_err(|error| {
                reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            })?;
            Ok(serde_json::json!(value))
        }
        "liquidation_queue_override" => {
            let req: LiquidationQueueOverrideRequest =
                serde_json::from_value(record.payload.clone())
                    .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            let queue_id = record
                .comment
                .clone()
                .ok_or_else(|| reject_api(StatusCode::BAD_REQUEST, "override queue id missing"))?;
            let next = crate::liquidation::apply_liquidation_queue_override(
                liquidation_queue,
                &queue_id,
                &req,
            )?;
            Ok(serde_json::json!(next))
        }
        _ => Err(reject_api(
            StatusCode::BAD_REQUEST,
            "unsupported governance action type",
        )),
    }
}

pub(crate) fn build_governance_routes(
    adl_governance: Arc<PersistentAdlGovernanceStore>,
    liquidation_policy: Arc<PersistentLiquidationPolicyStore>,
    index_prices: Arc<PersistentIndexPriceStore>,
    liquidation_queue: Arc<LiquidationQueueStore>,
    governance_actions: Arc<PendingGovernanceActionStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let adl_governance_for_get = adl_governance.clone();
    let ip_rate_limiter_for_adl_governance_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_adl_governance_get = admin_rate_limiter.clone();
    let adl_governance_get_route = warp::path!("admin" / "risk" / "adl" / "governance")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let adl_governance = adl_governance_for_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_adl_governance_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_adl_governance_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "item": adl_governance.current(),
                    })))
                }
            },
        );
    let adl_governance_for_post = adl_governance.clone();
    let governance_actions_for_adl_governance_post = governance_actions.clone();
    let ip_rate_limiter_for_adl_governance_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_adl_governance_post = admin_rate_limiter.clone();
    let adl_governance_post_route = warp::path!("admin" / "risk" / "adl" / "governance")
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: AdlGovernanceUpdateRequest| {
                let adl_governance = adl_governance_for_post.clone();
                let governance_actions = governance_actions_for_adl_governance_post.clone();
                let ip_rate_limiter = ip_rate_limiter_for_adl_governance_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_adl_governance_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let pending = create_pending_governance_action(
                        governance_actions.as_ref(),
                        "adl_governance_update",
                        serde_json::to_value(&req).map_err(|error| {
                            reject_api(StatusCode::BAD_REQUEST, error.to_string())
                        })?,
                        &principal.subject,
                        None,
                    )
                    .map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    let _ = adl_governance.current();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "pending",
                        "approval": pending,
                    })))
                }
            },
        );
    let liquidation_policy_for_get = liquidation_policy.clone();
    let ip_rate_limiter_for_liquidation_policy_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_liquidation_policy_get = admin_rate_limiter.clone();
    let liquidation_policy_get_route = warp::path!("admin" / "risk" / "liquidations" / "policy")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let liquidation_policy = liquidation_policy_for_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_liquidation_policy_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_liquidation_policy_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "item": liquidation_policy.current(),
                    })))
                }
            },
        );
    let liquidation_policy_for_post = liquidation_policy.clone();
    let governance_actions_for_liquidation_policy_post = governance_actions.clone();
    let ip_rate_limiter_for_liquidation_policy_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_liquidation_policy_post = admin_rate_limiter.clone();
    let liquidation_policy_post_route = warp::path!("admin" / "risk" / "liquidations" / "policy")
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: LiquidationPolicyUpdateRequest| {
                let liquidation_policy = liquidation_policy_for_post.clone();
                let governance_actions = governance_actions_for_liquidation_policy_post.clone();
                let ip_rate_limiter = ip_rate_limiter_for_liquidation_policy_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_liquidation_policy_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let pending = create_pending_governance_action(
                        governance_actions.as_ref(),
                        "liquidation_policy_update",
                        serde_json::to_value(&req).map_err(|error| {
                            reject_api(StatusCode::BAD_REQUEST, error.to_string())
                        })?,
                        &principal.subject,
                        None,
                    )
                    .map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    let _ = liquidation_policy.current();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "pending",
                        "approval": pending,
                    })))
                }
            },
        );
    let liquidation_queue_for_override = liquidation_queue.clone();
    let governance_actions_for_liquidation_override = governance_actions.clone();
    let ip_rate_limiter_for_liquidation_override = ip_rate_limiter.clone();
    let admin_rate_limiter_for_liquidation_override = admin_rate_limiter.clone();
    let liquidation_queue_override_route =
        warp::path!("admin" / "risk" / "liquidations" / "queue" / String / "override")
            .and(warp::post())
            .and(with_principal())
            .and(remote_ip())
            .and(body_limit())
            .and(verified_json_body())
            .and_then(
                move |queue_id: String,
                      principal: AuthenticatedPrincipal,
                      remote: Option<SocketAddr>,
                      req: LiquidationQueueOverrideRequest| {
                    let queue_store = liquidation_queue_for_override.clone();
                    let governance_actions = governance_actions_for_liquidation_override.clone();
                    let ip_rate_limiter = ip_rate_limiter_for_liquidation_override.clone();
                    let admin_rate_limiter = admin_rate_limiter_for_liquidation_override.clone();
                    async move {
                        require_admin(&principal)?;
                        let ip_key = remote
                            .map(|value| value.ip().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                        admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                        let _ = queue_store.get(&queue_id).ok_or_else(|| {
                            reject_api(StatusCode::NOT_FOUND, "liquidation queue item not found")
                        })?;
                        let pending = create_pending_governance_action(
                            governance_actions.as_ref(),
                            "liquidation_queue_override",
                            serde_json::to_value(&req).map_err(|error| {
                                reject_api(StatusCode::BAD_REQUEST, error.to_string())
                            })?,
                            &principal.subject,
                            Some(queue_id.clone()),
                        )
                        .map_err(|error| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                        })?;
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "status": "pending",
                            "approval": pending,
                        })))
                    }
                },
            );
    let governance_actions_for_list = governance_actions.clone();
    let ip_rate_limiter_for_governance_actions = ip_rate_limiter.clone();
    let admin_rate_limiter_for_governance_actions = admin_rate_limiter.clone();
    let governance_actions_route = warp::path!("admin" / "risk" / "governance" / "actions")
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<LiquidationQueueQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: LiquidationQueueQuery,
                  remote: Option<SocketAddr>| {
                let governance_actions = governance_actions_for_list.clone();
                let ip_rate_limiter = ip_rate_limiter_for_governance_actions.clone();
                let admin_rate_limiter = admin_rate_limiter_for_governance_actions.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let items = governance_actions.list_recent(
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
    let governance_actions_for_approve = governance_actions.clone();
    let adl_governance_for_approve = adl_governance.clone();
    let liquidation_policy_for_approve = liquidation_policy.clone();
    let index_prices_for_approve = index_prices.clone();
    let liquidation_queue_for_approve = liquidation_queue.clone();
    let ip_rate_limiter_for_governance_approve = ip_rate_limiter.clone();
    let admin_rate_limiter_for_governance_approve = admin_rate_limiter.clone();
    let governance_action_approve_route =
        warp::path!("admin" / "risk" / "governance" / "actions" / String / "approve")
            .and(warp::post())
            .and(with_principal())
            .and(remote_ip())
            .and_then(
                move |action_id: String,
                      principal: AuthenticatedPrincipal,
                      remote: Option<SocketAddr>| {
                    let governance_actions = governance_actions_for_approve.clone();
                    let adl_governance = adl_governance_for_approve.clone();
                    let liquidation_policy = liquidation_policy_for_approve.clone();
                    let index_prices = index_prices_for_approve.clone();
                    let liquidation_queue = liquidation_queue_for_approve.clone();
                    let ip_rate_limiter = ip_rate_limiter_for_governance_approve.clone();
                    let admin_rate_limiter = admin_rate_limiter_for_governance_approve.clone();
                    async move {
                        require_admin(&principal)?;
                        let ip_key = remote
                            .map(|value| value.ip().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                        admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                        let current = governance_actions.get(&action_id).ok_or_else(|| {
                            reject_api(StatusCode::NOT_FOUND, "governance action not found")
                        })?;
                        if current.status != "pending" {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "governance action is not pending",
                            ));
                        }
                        if current.requested_by == principal.subject {
                            return Err(reject_api(
                                StatusCode::FORBIDDEN,
                                "dual approval requires a different admin",
                            ));
                        }
                        let result = apply_governance_action(
                            &current,
                            adl_governance.as_ref(),
                            liquidation_policy.as_ref(),
                            index_prices.as_ref(),
                            liquidation_queue.as_ref(),
                        )?;
                        let decided = GovernanceActionRecord {
                            approved_by: Some(principal.subject.clone()),
                            status: "applied".to_string(),
                            decided_at: Some(Utc::now()),
                            ..current
                        };
                        governance_actions
                            .append(decided.clone())
                            .map_err(|error| {
                                reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                            })?;
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "status": "ok",
                            "action": decided,
                            "result": result,
                        })))
                    }
                },
            );
    let governance_actions_for_reject = governance_actions.clone();
    let ip_rate_limiter_for_governance_reject = ip_rate_limiter.clone();
    let admin_rate_limiter_for_governance_reject = admin_rate_limiter.clone();
    let governance_action_reject_route =
        warp::path!("admin" / "risk" / "governance" / "actions" / String / "reject")
            .and(warp::post())
            .and(with_principal())
            .and(remote_ip())
            .and_then(
                move |action_id: String,
                      principal: AuthenticatedPrincipal,
                      remote: Option<SocketAddr>| {
                    let governance_actions = governance_actions_for_reject.clone();
                    let ip_rate_limiter = ip_rate_limiter_for_governance_reject.clone();
                    let admin_rate_limiter = admin_rate_limiter_for_governance_reject.clone();
                    async move {
                        require_admin(&principal)?;
                        let ip_key = remote
                            .map(|value| value.ip().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                        admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                        let current = governance_actions.get(&action_id).ok_or_else(|| {
                            reject_api(StatusCode::NOT_FOUND, "governance action not found")
                        })?;
                        if current.status != "pending" {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "governance action is not pending",
                            ));
                        }
                        if current.requested_by == principal.subject {
                            return Err(reject_api(
                                StatusCode::FORBIDDEN,
                                "dual approval requires a different admin",
                            ));
                        }
                        let decided = GovernanceActionRecord {
                            rejected_by: Some(principal.subject.clone()),
                            status: "rejected".to_string(),
                            decided_at: Some(Utc::now()),
                            ..current
                        };
                        governance_actions
                            .append(decided.clone())
                            .map_err(|error| {
                                reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                            })?;
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "status": "ok",
                            "action": decided,
                        })))
                    }
                },
            );
    adl_governance_get_route
        .or(adl_governance_post_route)
        .unify()
        .or(liquidation_policy_get_route)
        .unify()
        .or(liquidation_policy_post_route)
        .unify()
        .or(liquidation_queue_override_route)
        .unify()
        .or(governance_actions_route)
        .unify()
        .or(governance_action_approve_route)
        .unify()
        .or(governance_action_reject_route)
        .unify()
        .boxed()
}
