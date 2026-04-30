use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct GovernanceActionRecord {
    pub(crate) action_id: String,
    pub(crate) action_type: String,
    pub(crate) payload: serde_json::Value,
    pub(crate) requested_by: String,
    #[serde(default = "default_required_approvals")]
    pub(crate) required_approvals: u32,
    #[serde(default)]
    pub(crate) approvers: Vec<String>,
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
    action_locks: DashMap<String, Arc<tokio::sync::Mutex<()>>>,
}

impl PendingGovernanceActionStore {
    pub(crate) fn new(
        store: Arc<dyn persistence::WalStore<GovernanceActionRecord>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            entries: DashMap::new(),
            store,
            action_locks: DashMap::new(),
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

    pub(crate) fn action_lock(&self, action_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.action_locks
            .entry(action_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

fn default_required_approvals() -> u32 {
    2
}

fn required_approvals_for_action(action_type: &str) -> u32 {
    match action_type {
        "liquidation_queue_override" => 3,
        "liquidation_execute" => 2,
        "index_price_upsert"
        | "index_source_policy_update"
        | "adl_governance_update"
        | "liquidation_policy_update" => 2,
        _ => default_required_approvals(),
    }
}

// ─── Area 5: Financial-grade Control Plane ─────────────────────────────────

/// RBAC permission flags for governance and admin operations.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Permission {
    ReadMarkets,
    WriteOrders,
    ManageInstruments,
    ExecuteLiquidation,
    ManageRiskParams,
    ManageGovernance,
    ApproveGovernance,
    ViewAuditLog,
    ManageUsers,
    SystemAdmin,
}

/// RBAC role with a set of permissions.
#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Role {
    pub(crate) name: String,
    pub(crate) permissions: Vec<Permission>,
    /// If true, this role requires 4-eyes (dual approval) for destructive actions.
    pub(crate) requires_dual_approval: bool,
}

impl Role {
    pub(crate) fn has_permission(&self, perm: Permission) -> bool {
        self.permissions.contains(&perm) || self.permissions.contains(&Permission::SystemAdmin)
    }
}

/// Built-in roles for the control plane.
#[allow(dead_code)]
pub(crate) fn builtin_roles() -> Vec<Role> {
    vec![
        Role {
            name: "viewer".to_string(),
            permissions: vec![Permission::ReadMarkets, Permission::ViewAuditLog],
            requires_dual_approval: false,
        },
        Role {
            name: "trader".to_string(),
            permissions: vec![Permission::ReadMarkets, Permission::WriteOrders],
            requires_dual_approval: false,
        },
        Role {
            name: "risk_manager".to_string(),
            permissions: vec![
                Permission::ReadMarkets,
                Permission::ManageRiskParams,
                Permission::ExecuteLiquidation,
                Permission::ViewAuditLog,
            ],
            requires_dual_approval: true,
        },
        Role {
            name: "admin".to_string(),
            permissions: vec![
                Permission::ReadMarkets,
                Permission::WriteOrders,
                Permission::ManageInstruments,
                Permission::ExecuteLiquidation,
                Permission::ManageRiskParams,
                Permission::ManageGovernance,
                Permission::ApproveGovernance,
                Permission::ViewAuditLog,
                Permission::ManageUsers,
            ],
            requires_dual_approval: true,
        },
        Role {
            name: "super_admin".to_string(),
            permissions: vec![Permission::SystemAdmin],
            requires_dual_approval: true,
        },
    ]
}

/// Resolve the required permission for a governance action type.
#[allow(dead_code)]
pub(crate) fn required_permission_for_action(action_type: &str) -> Permission {
    match action_type {
        "kill_switch" | "set_market_state" => Permission::SystemAdmin,
        "liquidation_execute" | "liquidation_queue_override" => Permission::ExecuteLiquidation,
        "adl_governance_update" | "liquidation_policy_update" => Permission::ManageRiskParams,
        "index_price_upsert" | "index_source_policy_update" => Permission::ManageRiskParams,
        "reference_price" => Permission::ManageRiskParams,
        _ => Permission::ManageGovernance,
    }
}

/// Check if a role is permitted to request a given governance action.
#[allow(dead_code)]
pub(crate) fn check_role_permission(role: &Role, action_type: &str) -> Result<(), String> {
    let required = required_permission_for_action(action_type);
    if role.has_permission(required) {
        Ok(())
    } else {
        Err(format!(
            "role '{}' lacks permission {:?} required for action '{}'",
            role.name, required, action_type
        ))
    }
}

/// Policy-as-code rule for automated compliance checks.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PolicyRule {
    pub(crate) rule_id: String,
    pub(crate) name: String,
    /// Condition expression (simplified DSL: "action_type == X", "approvers >= N").
    pub(crate) condition: String,
    /// Action to take when condition matches.
    pub(crate) action: PolicyAction,
    pub(crate) enabled: bool,
    pub(crate) version: u32,
}

/// Actions taken by policy-as-code rules.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum PolicyAction {
    Allow,
    Deny { reason: String },
    RequireApproval { min_approvers: u32 },
    Alert { message: String },
}

/// Evaluate policy rules against a governance action.
/// Returns the list of policy violations (empty = all clear).
#[allow(dead_code)]
pub(crate) fn evaluate_policy_rules(
    rules: &[PolicyRule],
    action_type: &str,
    _payload: &serde_json::Value,
) -> Vec<String> {
    let mut violations = Vec::new();
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        // Simplified DSL evaluation
        let condition = rule.condition.trim();
        let matches = if condition.starts_with("action_type == ") {
            let target = condition
                .trim_start_matches("action_type == ")
                .trim_matches('"');
            action_type == target
        } else {
            condition == "*" || condition == "all"
        };

        if matches {
            match &rule.action {
                PolicyAction::Deny { reason } => {
                    violations.push(format!("[{}] denied: {}", rule.name, reason));
                }
                PolicyAction::Alert { message } => {
                    // Alerts are logged but don't block
                    tracing::warn!(rule = %rule.name, %message, "policy alert triggered");
                }
                PolicyAction::RequireApproval { .. } | PolicyAction::Allow => {}
            }
        }
    }
    violations
}

/// Simulate the impact of a governance action without applying it.
#[allow(dead_code)]
pub(crate) fn simulate_governance_action(
    action_type: &str,
    payload: &serde_json::Value,
    policy_rules: &[PolicyRule],
) -> SimulationResult {
    let policy_violations = evaluate_policy_rules(policy_rules, action_type, payload);

    let (affected_users, affected_markets) = match action_type {
        "kill_switch" => (0, u32::MAX), // affects all markets
        "set_market_state" => (0, 1),
        "reference_price" => (0, 1),
        "liquidation_execute" => (1, 1),
        "adl_governance_update" | "liquidation_policy_update" => (0, 0), // policy-level
        "index_price_upsert" | "index_source_policy_update" => (0, 1),
        "liquidation_queue_override" => (1, 1),
        _ => (0, 0),
    };

    let impact = match action_type {
        "kill_switch" => "halts all trading immediately".to_string(),
        "set_market_state" => format!("changes market state to {:?}", payload.get("state")),
        "liquidation_execute" => "forces liquidation of a user position".to_string(),
        "adl_governance_update" => "updates ADL scoring weights".to_string(),
        "liquidation_policy_update" => "updates liquidation pipeline parameters".to_string(),
        _ => "modifies system configuration".to_string(),
    };

    SimulationResult {
        action_type: action_type.to_string(),
        would_affect_users: affected_users,
        would_affect_markets: affected_markets,
        estimated_impact: impact,
        policy_violations,
        rollback_possible: !matches!(action_type, "kill_switch" | "liquidation_execute"),
    }
}

/// Result of simulating a governance action before applying.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SimulationResult {
    pub(crate) action_type: String,
    pub(crate) would_affect_users: u32,
    pub(crate) would_affect_markets: u32,
    pub(crate) estimated_impact: String,
    pub(crate) policy_violations: Vec<String>,
    pub(crate) rollback_possible: bool,
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
        required_approvals: required_approvals_for_action(action_type),
        approvers: Vec::new(),
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

#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_governance_action(
    record: &GovernanceActionRecord,
    adl_governance: &PersistentAdlGovernanceStore,
    liquidation_policy: &PersistentLiquidationPolicyStore,
    index_prices: &PersistentIndexPriceStore,
    liquidation_queue: &LiquidationQueueStore,
    risk: Option<&RiskEngine>,
    instruments: Option<&PersistentInstrumentRegistry>,
    engine: Option<&PartitionedMatchingEngine>,
    sequencer: Option<&Sequencer>,
    system_sentinel: Option<&sentinel::SystemSentinel>,
) -> Result<serde_json::Value, Rejection> {
    match record.action_type.as_str() {
        "kill_switch" => {
            let engine = engine.ok_or_else(|| {
                reject_api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "engine not available for kill_switch",
                )
            })?;
            let sequencer = sequencer.ok_or_else(|| {
                reject_api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "sequencer not available for kill_switch",
                )
            })?;
            let req: KillSwitchRequest = serde_json::from_value(record.payload.clone())
                .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            let request_id = record
                .comment
                .clone()
                .unwrap_or_else(|| types::generate_op_id("ks"));
            let command = sequence_admin(
                sequencer,
                request_id.clone(),
                record.requested_by.clone(),
                AdminAction::KillSwitch {
                    enabled: req.enabled,
                },
            )
            .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error))?;
            engine
                .submit_admin(command)
                .await
                .map_err(reject_internal_error)?;
            update_lifecycle_after_admin(sequencer, &request_id);
            if req.enabled {
                if let Some(s) = system_sentinel {
                    s.report_market_restricted("*", "kill_switch_enabled");
                }
            }
            Ok(serde_json::json!({
                "status": "ok",
                "request_id": request_id,
            }))
        }
        "set_market_state" => {
            let engine = engine.ok_or_else(|| {
                reject_api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "engine not available for set_market_state",
                )
            })?;
            let sequencer = sequencer.ok_or_else(|| {
                reject_api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "sequencer not available for set_market_state",
                )
            })?;
            let req: SetMarketStateRequest = serde_json::from_value(record.payload.clone())
                .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            let request_id = record
                .comment
                .clone()
                .unwrap_or_else(|| types::generate_op_id("ms"));
            let market_id_for_sentinel = req.market_id.clone();
            let state_for_sentinel = req.state;
            let command = sequence_admin(
                sequencer,
                request_id.clone(),
                record.requested_by.clone(),
                AdminAction::SetMarketState {
                    market_id: req.market_id,
                    outcome: req.outcome,
                    state: req.state,
                },
            )
            .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error))?;
            engine
                .submit_admin(command)
                .await
                .map_err(reject_internal_error)?;
            update_lifecycle_after_admin(sequencer, &request_id);
            if matches!(
                state_for_sentinel,
                MarketState::Halted
                    | MarketState::CancelOnly
                    | MarketState::Maintenance
                    | MarketState::Closed
            ) {
                if let Some(s) = system_sentinel {
                    s.report_market_restricted(
                        &market_id_for_sentinel,
                        &format!("set_market_state:{state_for_sentinel:?}"),
                    );
                }
            }
            Ok(serde_json::json!({
                "status": "ok",
                "request_id": request_id,
            }))
        }
        "reference_price" => {
            let engine = engine.ok_or_else(|| {
                reject_api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "engine not available for reference_price",
                )
            })?;
            let req: ReferencePriceRequest = serde_json::from_value(record.payload.clone())
                .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            let snapshot = engine
                .update_reference_price(
                    req.market_id,
                    req.outcome,
                    req.source.unwrap_or_else(|| "manual".to_string()),
                    req.reference_price,
                )
                .await
                .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            Ok(serde_json::json!({
                "status": "ok",
                "market_id": snapshot.market_id,
                "outcome": snapshot.outcome,
                "market_state": snapshot.state,
                "reference_price": snapshot.reference_price,
                "best_bid": snapshot.best_bid,
                "best_ask": snapshot.best_ask,
            }))
        }
        "liquidation_execute" => {
            let risk = risk.ok_or_else(|| {
                reject_api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "risk engine not available for liquidation_execute",
                )
            })?;
            let instruments = instruments.ok_or_else(|| {
                reject_api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "instruments not available for liquidation_execute",
                )
            })?;
            let req: LiquidationExecuteRequest = serde_json::from_value(record.payload.clone())
                .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            let request_id = record
                .comment
                .clone()
                .unwrap_or_else(|| types::generate_op_id("liq"));
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
            Ok(serde_json::json!({
                "request_id": request_id,
                "execution": execution,
            }))
        }
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
            adl_governance
                .upsert(current.clone())
                .map_err(reject_internal_error)?;
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
            current.auction_reserve_step_bps = req
                .auction_reserve_step_bps
                .unwrap_or(current.auction_reserve_step_bps)
                .max(0);
            current.updated_by = record.requested_by.clone();
            current.recorded_at = Utc::now();
            liquidation_policy
                .upsert(current.clone())
                .map_err(reject_internal_error)?;
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
            index_prices
                .upsert(value.clone())
                .map_err(reject_internal_error)?;
            Ok(serde_json::json!(value))
        }
        "index_source_policy_update" => {
            let req: IndexSourcePolicyUpdateRequest =
                serde_json::from_value(record.payload.clone())
                    .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
            let outcome = req.outcome.unwrap_or(0);
            let status = req.status.trim().to_ascii_lowercase();
            if !matches!(status.as_str(), "active" | "degraded" | "quarantined") {
                return Err(reject_api(
                    StatusCode::BAD_REQUEST,
                    "status must be active, degraded, or quarantined",
                ));
            }
            let value = IndexSourcePolicyRecord {
                market_id: req.market_id,
                outcome,
                source: req.source,
                status,
                weight_bps: req
                    .weight_bps
                    .unwrap_or(default_index_source_weight_bps())
                    .clamp(1, 10_000),
                updated_by: record.requested_by.clone(),
                recorded_at: Utc::now(),
            };
            index_prices
                .upsert_policy(value.clone())
                .map_err(reject_internal_error)?;
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_governance_routes(
    adl_governance: Arc<PersistentAdlGovernanceStore>,
    liquidation_policy: Arc<PersistentLiquidationPolicyStore>,
    index_prices: Arc<PersistentIndexPriceStore>,
    liquidation_queue: Arc<LiquidationQueueStore>,
    governance_actions: Arc<PendingGovernanceActionStore>,
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    engine: Arc<PartitionedMatchingEngine>,
    sequencer: Arc<Sequencer>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
    system_sentinel: Arc<sentinel::SystemSentinel>,
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
        )
        .boxed();
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
                    .map_err(reject_internal_error)?;
                    let _ = adl_governance.current();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "pending",
                        "approval": pending,
                    })))
                }
            },
        )
        .boxed();
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
        )
        .boxed();
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
                    .map_err(reject_internal_error)?;
                    let _ = liquidation_policy.current();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "pending",
                        "approval": pending,
                    })))
                }
            },
        )
        .boxed();
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
                        .map_err(reject_internal_error)?;
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
        )
        .boxed();
    let governance_actions_for_approve = governance_actions.clone();
    let adl_governance_for_approve = adl_governance.clone();
    let liquidation_policy_for_approve = liquidation_policy.clone();
    let index_prices_for_approve = index_prices.clone();
    let liquidation_queue_for_approve = liquidation_queue.clone();
    let risk_for_approve = risk.clone();
    let instruments_for_approve = instruments.clone();
    let engine_for_approve = engine.clone();
    let sequencer_for_approve = sequencer.clone();
    let sentinel_for_approve = system_sentinel.clone();
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
                    let risk = risk_for_approve.clone();
                    let instruments = instruments_for_approve.clone();
                    let engine = engine_for_approve.clone();
                    let sequencer = sequencer_for_approve.clone();
                    let sentinel = sentinel_for_approve.clone();
                    let ip_rate_limiter = ip_rate_limiter_for_governance_approve.clone();
                    let admin_rate_limiter = admin_rate_limiter_for_governance_approve.clone();
                    async move {
                        require_admin(&principal)?;
                        let ip_key = remote
                            .map(|value| value.ip().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                        admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                        let lock = governance_actions.action_lock(&action_id);
                        let _guard = lock.lock().await;
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
                        if current.approvers.iter().any(|item| item == &principal.subject) {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "admin already approved this action",
                            ));
                        }
                        let mut approvers = current.approvers.clone();
                        approvers.push(principal.subject.clone());
                        if (approvers.len() as u32) < current.required_approvals {
                            let pending = GovernanceActionRecord {
                                approvers,
                                approved_by: Some(principal.subject.clone()),
                                decided_at: None,
                                ..current
                            };
                            governance_actions.append(pending.clone()).map_err(reject_internal_error)?;
                            return Ok::<_, warp::Rejection>(warp::reply::json(
                                &serde_json::json!({
                                    "status": "pending",
                                    "action": pending,
                                    "remaining_approvals": pending.required_approvals.saturating_sub(pending.approvers.len() as u32),
                                }),
                            ));
                        }
                        let result = {
                            let decided = GovernanceActionRecord {
                                approvers: approvers.clone(),
                                approved_by: Some(principal.subject.clone()),
                                status: "applied".to_string(),
                                decided_at: Some(Utc::now()),
                                ..current.clone()
                            };
                            governance_actions
                                .append(decided.clone())
                                .map_err(|error| {
                                    reject_api(
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        error.to_string(),
                                    )
                                })?;
                            match apply_governance_action(
                                &current,
                                adl_governance.as_ref(),
                                liquidation_policy.as_ref(),
                                index_prices.as_ref(),
                                liquidation_queue.as_ref(),
                                Some(risk.as_ref()),
                                Some(instruments.as_ref()),
                                Some(engine.as_ref()),
                                Some(sequencer.as_ref()),
                                Some(sentinel.as_ref()),
                            )
                            .await
                            {
                                Ok(result) => result,
                                Err(rejection) => {
                                    let failed = GovernanceActionRecord {
                                        status: "apply_failed".to_string(),
                                        ..decided
                                    };
                                    let _ = governance_actions.append(failed);
                                    return Err(rejection);
                                }
                            }
                        };
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "status": "ok",
                            "action": {
                                "action_id": current.action_id,
                                "status": "applied",
                                "approvers": approvers,
                            },
                            "result": result,
                        })))
                    }
                },
            )
            .boxed();
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
                        let lock = governance_actions.action_lock(&action_id);
                        let _guard = lock.lock().await;
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
                        if current
                            .approvers
                            .iter()
                            .any(|item| item == &principal.subject)
                        {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "admin already approved this action",
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
                            .map_err(reject_internal_error)?;
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "status": "ok",
                            "action": decided,
                        })))
                    }
                },
            )
            .boxed();
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
