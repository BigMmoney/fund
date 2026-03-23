use super::*;

// ── Withdrawal record persisted in WAL ───────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WithdrawalRecord {
    pub(crate) withdrawal_id: String,
    pub(crate) user_id: String,
    pub(crate) amount: i64,
    pub(crate) asset: String,
    pub(crate) destination_address: String,
    pub(crate) status: String, // pending, approved, completed, rejected, cancelled, expired
    pub(crate) requested_at: DateTime<Utc>,
    pub(crate) decided_at: Option<DateTime<Utc>>,
    pub(crate) decided_by: Option<String>,
    pub(crate) ledger_op_id: Option<String>,
    /// Time after which the withdrawal can be executed.
    #[serde(default)]
    pub(crate) executable_after: Option<DateTime<Utc>>,
    /// Deadline by which the user can cancel the withdrawal.
    #[serde(default)]
    pub(crate) cancel_before: Option<DateTime<Utc>>,
    /// Vault tier selected for this withdrawal.
    #[serde(default)]
    pub(crate) vault_tier: Option<custody::VaultTier>,
    /// Number of distinct admin approvals required before execution.
    #[serde(default)]
    pub(crate) required_approvals: u32,
    /// Distinct admins who have already approved this withdrawal.
    #[serde(default)]
    pub(crate) approvers: Vec<String>,
}

// ── In-memory store backed by append-only WAL ────────────────

pub(crate) struct WithdrawalStore {
    entries: DashMap<String, WithdrawalRecord>,
    store: Arc<dyn persistence::WalStore<WithdrawalRecord>>,
    write_lock: Mutex<()>,
}

impl WithdrawalStore {
    pub(crate) fn new(
        store: Arc<dyn persistence::WalStore<WithdrawalRecord>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            entries: DashMap::new(),
            store,
            write_lock: Mutex::new(()),
        };
        for record in result.store.entries()? {
            result.entries.insert(record.withdrawal_id.clone(), record);
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<WithdrawalRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub(crate) fn append(&self, record: WithdrawalRecord) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store.append(&record)?;
        self.entries.insert(record.withdrawal_id.clone(), record);
        Ok(())
    }

    pub(crate) fn get(&self, withdrawal_id: &str) -> Option<WithdrawalRecord> {
        self.entries
            .get(withdrawal_id)
            .map(|entry| entry.value().clone())
    }

    pub(crate) fn list_for_user(
        &self,
        user_id: &str,
        status_filter: Option<&str>,
        limit: usize,
    ) -> Vec<WithdrawalRecord> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.value().user_id == user_id)
            .filter(|entry| status_filter.is_none_or(|status| entry.value().status == status))
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|a, b| b.requested_at.cmp(&a.requested_at));
        items.truncate(limit);
        items
    }

    fn list_pending(&self, limit: usize) -> Vec<WithdrawalRecord> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.value().status == "pending")
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|a, b| a.requested_at.cmp(&b.requested_at));
        items.truncate(limit);
        items
    }
}

// ── Routes ───────────────────────────────────────────────────

const MAX_DESTINATION_LEN: usize = 256;
const SUPPORTED_WITHDRAWAL_ASSET: &str = "USDC";

fn normalize_withdrawal_asset(asset: Option<String>) -> Result<String, Rejection> {
    match asset {
        Some(value) if !value.trim().eq_ignore_ascii_case(SUPPORTED_WITHDRAWAL_ASSET) => {
            Err(reject_api(
                StatusCode::BAD_REQUEST,
                format!(
                    "unsupported withdrawal asset: only {SUPPORTED_WITHDRAWAL_ASSET} is currently enabled"
                ),
            ))
        }
        _ => Ok(SUPPORTED_WITHDRAWAL_ASSET.to_string()),
    }
}

fn approval_threshold(record: &WithdrawalRecord) -> u32 {
    record.required_approvals.max(1)
}

fn approvals_remaining(record: &WithdrawalRecord) -> u32 {
    approval_threshold(record).saturating_sub(record.approvers.len() as u32)
}

fn build_pending_view(record: &WithdrawalRecord) -> custody::PendingWithdrawalView {
    let executable_after = record.executable_after.unwrap_or(record.requested_at);
    let cancel_before = record.cancel_before.unwrap_or(record.requested_at);
    let time_lock_secs = (executable_after - record.requested_at)
        .num_seconds()
        .max(0) as u64;
    let intent = custody::WithdrawalApprovalIntent {
        withdrawal_id: record.withdrawal_id.clone(),
        user_id: record.user_id.clone(),
        amount: record.amount,
        asset: record.asset.clone(),
        destination_address: record.destination_address.clone(),
        vault_tier: record.vault_tier.unwrap_or(custody::VaultTier::Hot),
        required_approvals: record.required_approvals,
        time_lock_secs,
        executable_after,
        cancel_before,
    };
    custody::to_pending_view(&intent, &record.status, record.requested_at)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_withdrawal_routes(
    withdrawal_store: Arc<WithdrawalStore>,
    ledger: Arc<LedgerService>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
    address_whitelist: Arc<custody::AddressWhitelistStore>,
    withdrawal_policy: custody::WithdrawalPolicy,
    custody_config: custody::CustodyConfig,
    withdrawal_usage: Arc<custody::WithdrawalUsageTracker>,
    velocity_tracker: Arc<custody::VaultVelocityTracker>,
    velocity_policy: custody::VaultVelocityPolicy,
    delay_policy: custody::WithdrawalDelayPolicy,
    allowlist_policy: custody::AllowlistPolicy,
    address_usage: Arc<custody::AddressUsageTracker>,
    breaker: Arc<custody::CustodyCircuitBreaker>,
    audit_log: Arc<custody::CustodyAuditLog>,
    sentinel: Arc<sentinel::SystemSentinel>,
) -> JsonRoute {
    // POST /withdraw — user requests a withdrawal
    let store_for_request = withdrawal_store.clone();
    let ledger_for_request = ledger.clone();
    let ip_rl_request = ip_rate_limiter.clone();
    let user_rl_request = user_rate_limiter.clone();
    let whitelist_for_request = address_whitelist.clone();
    let policy_for_request = withdrawal_policy.clone();
    let custody_for_request = custody_config.clone();
    let usage_for_request = withdrawal_usage.clone();
    let vel_for_request = velocity_tracker.clone();
    let vel_pol_for_request = velocity_policy.clone();
    let delay_for_request = delay_policy.clone();
    let al_for_request = allowlist_policy.clone();
    let au_for_request = address_usage.clone();
    let brk_for_request = breaker.clone();
    let audit_for_request = audit_log.clone();
    let sentinel_for_request = sentinel.clone();
    let request_route = warp::path!("withdraw")
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json::<WithdrawalRequest>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: WithdrawalRequest,
                  remote: Option<SocketAddr>| {
                let store = store_for_request.clone();
                let ledger = ledger_for_request.clone();
                let ip_rl = ip_rl_request.clone();
                let user_rl = user_rl_request.clone();
                let whitelist = whitelist_for_request.clone();
                let policy = policy_for_request.clone();
                let custody = custody_for_request.clone();
                let usage = usage_for_request.clone();
                let vel = vel_for_request.clone();
                let vel_pol = vel_pol_for_request.clone();
                let delay = delay_for_request.clone();
                let al_pol = al_for_request.clone();
                let addr_usage = au_for_request.clone();
                let brk = brk_for_request.clone();
                let audit = audit_for_request.clone();
                let sentinel = sentinel_for_request.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-withdraw:{}", principal.subject), 10)?;
                    let asset = normalize_withdrawal_asset(req.asset)?;

                    if req.amount <= 0 {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "amount must be positive",
                        ));
                    }
                    if req.destination_address.trim().is_empty()
                        || req.destination_address.len() > MAX_DESTINATION_LEN
                    {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "invalid destination_address",
                        ));
                    }

                    // Check available balance
                    let available = ledger.cash_available_balance(&principal.subject);
                    if available < req.amount {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            format!(
                                "insufficient balance: available={available}, requested={}",
                                req.amount
                            ),
                        ));
                    }

                    // ── Custody gate: whitelist, daily limit, cooldown, escalation ──
                    let gate_result = custody::check_withdrawal_gate(
                        &principal.subject,
                        req.amount,
                        &req.destination_address,
                        &policy,
                        &usage,
                        &whitelist,
                        &custody,
                    );
                    let governance_required = match gate_result {
                        custody::WithdrawalGateResult::Proceed => 0,
                        custody::WithdrawalGateResult::RequiresGovernance {
                            required_approvals,
                        } => required_approvals,
                        custody::WithdrawalGateResult::AddressNotWhitelisted { address } => {
                            return Err(reject_api(
                                StatusCode::FORBIDDEN,
                                format!("address not whitelisted or still in cooldown: {address}"),
                            ));
                        }
                        custody::WithdrawalGateResult::DailyLimitExceeded { used, limit } => {
                            return Err(reject_api(
                                StatusCode::TOO_MANY_REQUESTS,
                                format!("daily withdrawal limit exceeded: used={used}, limit={limit}"),
                            ));
                        }
                        custody::WithdrawalGateResult::CooldownActive { remaining_secs } => {
                            return Err(reject_api(
                                StatusCode::TOO_MANY_REQUESTS,
                                format!("withdrawal cooldown active: {remaining_secs}s remaining"),
                            ));
                        }
                        custody::WithdrawalGateResult::VaultInsufficient {
                            available: avail,
                            requested,
                        } => {
                            return Err(reject_api(
                                StatusCode::SERVICE_UNAVAILABLE,
                                format!("vault insufficient: available={avail}, requested={requested}"),
                            ));
                        }
                    };

                    // ── Circuit breaker check ──
                    if brk.is_open() {
                        audit.record(
                            custody::CustodyEventType::WithdrawalGateBlocked,
                            &principal.subject,
                            serde_json::json!({ "reason": "circuit_breaker_open" }),
                        );
                        return Err(reject_api(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "custody circuit breaker is open — withdrawals suspended",
                        ));
                    }

                    // ── System sentinel posture check ──
                    let vault_tier = custody::select_vault_tier(req.amount, &custody);
                    if let Err(reason) = sentinel::enforce_withdrawal_posture(
                        &sentinel, req.amount, vault_tier,
                    ) {
                        audit.record(
                            custody::CustodyEventType::WithdrawalGateBlocked,
                            &principal.subject,
                            serde_json::json!({ "reason": &reason, "sentinel": true }),
                        );
                        return Err(reject_api(StatusCode::SERVICE_UNAVAILABLE, reason));
                    }

                    // ── Vault velocity check ──
                    vel.gc(vel_pol.window_secs);
                    if let Err((current, limit)) = custody::check_vault_velocity(
                        vault_tier, req.amount, &vel, &vel_pol,
                    ) {
                        brk.record_velocity_breach();
                        sentinel.report_custody_breaker_trip("velocity_breach");
                        sentinel.report_velocity_breach(
                            &format!("{vault_tier:?}"), current, limit,
                        );
                        audit.record(
                            custody::CustodyEventType::VelocityBreachDetected,
                            &principal.subject,
                            serde_json::json!({
                                "tier": format!("{:?}", vault_tier),
                                "current": current, "limit": limit,
                            }),
                        );
                        return Err(reject_api(
                            StatusCode::TOO_MANY_REQUESTS,
                            format!("vault velocity exceeded: current={current}, limit={limit}"),
                        ));
                    }
                    brk.clear_velocity_breaches();

                    // ── Per-address daily limit check ──
                    if let Err((current, limit)) = custody::check_per_address_limit(
                        &principal.subject, &req.destination_address, req.amount,
                        &usage, &addr_usage, &al_pol,
                    ) {
                        audit.record(
                            custody::CustodyEventType::PerAddressLimitBlocked,
                            &principal.subject,
                            serde_json::json!({
                                "address": custody::mask_address(&req.destination_address),
                                "current": current, "limit": limit,
                            }),
                        );
                        return Err(reject_api(
                            StatusCode::TOO_MANY_REQUESTS,
                            format!("per-address daily limit exceeded: current={current}, limit={limit}"),
                        ));
                    }

                    // ── Large burst tracking ──
                    if req.amount > policy.large_withdrawal_threshold {
                        brk.record_large_burst();
                        sentinel.report_custody_breaker_trip("large_burst");
                    }

                    // ── Compute delay / cancel window ──
                    let now = Utc::now();
                    let withdrawal_id = types::generate_id();
                    let approval_intent = custody::build_approval_intent(
                        &withdrawal_id,
                        &principal.subject,
                        req.amount,
                        &asset,
                        &req.destination_address,
                        &custody,
                        &delay,
                    );
                    let required_approvals =
                        approval_intent.required_approvals.max(governance_required).max(1);

                    // Place a hold on the withdrawal amount
                    let hold_op_id = format!("withdrawal-hold-{withdrawal_id}");
                    ledger
                        .create_cash_hold(&principal.subject, req.amount, hold_op_id.clone())
                        .map_err(|e| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                        })?;

                    let record = WithdrawalRecord {
                        withdrawal_id: withdrawal_id.clone(),
                        user_id: principal.subject.clone(),
                        amount: req.amount,
                        asset,
                        destination_address: req.destination_address,
                        status: "pending".into(),
                        requested_at: now,
                        decided_at: None,
                        decided_by: None,
                        ledger_op_id: Some(hold_op_id),
                        executable_after: Some(approval_intent.executable_after),
                        cancel_before: Some(approval_intent.cancel_before),
                        vault_tier: Some(approval_intent.vault_tier),
                        required_approvals,
                        approvers: Vec::new(),
                    };

                    store.append(record.clone()).map_err(|e| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                    })?;

                    audit.record(
                        custody::CustodyEventType::WithdrawalRequested,
                        &principal.subject,
                        serde_json::json!({
                            "withdrawal_id": withdrawal_id,
                            "amount": req.amount,
                            "vault_tier": format!("{:?}", approval_intent.vault_tier),
                            "required_approvals": required_approvals,
                            "governance_escalated": governance_required > 0,
                        }),
                    );

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "withdrawal_id": withdrawal_id,
                        "user_id": principal.subject,
                        "amount": req.amount,
                        "status": "pending",
                        "requested_at": record.requested_at,
                        "vault_tier": format!("{:?}", approval_intent.vault_tier),
                        "executable_after": approval_intent.executable_after,
                        "cancel_before": approval_intent.cancel_before,
                        "required_approvals": required_approvals,
                        "approvals_collected": 0,
                        "remaining_approvals": approval_threshold(&record),
                        "time_lock_secs": approval_intent.time_lock_secs,
                        "governance_escalated": governance_required > 0,
                    })))
                }
            },
        );

    // GET /withdrawals/{user_id} — list user's withdrawals
    let store_for_list = withdrawal_store.clone();
    let ip_rl_list = ip_rate_limiter.clone();
    let user_rl_list = user_rate_limiter.clone();
    let list_route = warp::path!("withdrawals" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<WithdrawalQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: WithdrawalQuery,
                  remote: Option<SocketAddr>| {
                let store = store_for_list.clone();
                let ip_rl = ip_rl_list.clone();
                let user_rl = user_rl_list.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-read:{}", principal.subject), 30)?;

                    let limit = query.limit.unwrap_or(50).clamp(1, 500);
                    let items = store.list_for_user(&user_id, query.status.as_deref(), limit);
                    let resp: Vec<_> = items
                        .into_iter()
                        .map(|w| {
                            let pending_view = build_pending_view(&w);
                            serde_json::json!({
                                "withdrawal_id": pending_view.withdrawal_id,
                                "amount": pending_view.amount,
                                "asset": pending_view.asset,
                                "destination_masked": pending_view.destination_masked,
                                "status": pending_view.status,
                                "requested_at": pending_view.requested_at,
                                "decided_at": w.decided_at,
                                "decided_by": w.decided_by,
                                "executable_after": pending_view.executable_after,
                                "cancel_before": pending_view.cancel_before,
                                "vault_tier": pending_view.vault_tier,
                                "required_approvals": w.required_approvals,
                                "approvers": w.approvers,
                                "remaining_approvals": approvals_remaining(&w),
                            })
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&resp))
                }
            },
        );

    // POST /admin/withdrawal/approve — admin approves a pending withdrawal
    let store_for_approve = withdrawal_store.clone();
    let ledger_for_approve = ledger.clone();
    let ip_rl_approve = ip_rate_limiter.clone();
    let admin_rl_approve = admin_rate_limiter.clone();
    let custody_for_approve = custody_config.clone();
    let usage_for_approve = withdrawal_usage.clone();
    let vel_for_approve = velocity_tracker.clone();
    let addr_usage_for_approve = address_usage.clone();
    let brk_for_approve = breaker.clone();
    let audit_for_approve = audit_log.clone();
    let sentinel_for_approve = sentinel.clone();
    let approve_route = warp::path!("admin" / "withdrawal" / "approve")
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json::<WithdrawalApproveRequest>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: WithdrawalApproveRequest,
                  remote: Option<SocketAddr>| {
                let store = store_for_approve.clone();
                let ledger = ledger_for_approve.clone();
                let ip_rl = ip_rl_approve.clone();
                let admin_rl = admin_rl_approve.clone();
                let custody = custody_for_approve.clone();
                let usage = usage_for_approve.clone();
                let vel = vel_for_approve.clone();
                let addr_usage = addr_usage_for_approve.clone();
                let brk = brk_for_approve.clone();
                let audit = audit_for_approve.clone();
                let sentinel = sentinel_for_approve.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    // Circuit breaker gate
                    if brk.is_open() {
                        return Err(reject_api(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "custody circuit breaker is open — approvals suspended",
                        ));
                    }

                    let current = store
                        .get(&req.withdrawal_id)
                        .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "withdrawal not found"))?;
                    if current.status != "pending" {
                        return Err(reject_api(
                            StatusCode::CONFLICT,
                            format!("withdrawal is already {}", current.status),
                        ));
                    }
                    if current
                        .approvers
                        .iter()
                        .any(|approver| approver == &principal.subject)
                    {
                        return Err(reject_api(
                            StatusCode::CONFLICT,
                            "admin has already approved this withdrawal",
                        ));
                    }

                    // ── Time-lock / delay check ──
                    let now = Utc::now();
                    let exec_after = current.executable_after.unwrap_or(current.requested_at);
                    let cancel_before = current.cancel_before.unwrap_or(current.requested_at);
                    let timing = custody::evaluate_withdrawal_timing(
                        now,
                        exec_after,
                        cancel_before,
                        86400,
                        current.requested_at,
                    );
                    match timing {
                        custody::WithdrawalTimingResult::Cancellable {
                            cancel_remaining_secs,
                        } => {
                            return Err(reject_api(
                                StatusCode::CONFLICT,
                                format!(
                                    "still in cancel window: {cancel_remaining_secs}s remaining"
                                ),
                            ));
                        }
                        custody::WithdrawalTimingResult::WaitingForDelay {
                            delay_remaining_secs,
                        } => {
                            return Err(reject_api(
                                StatusCode::CONFLICT,
                                format!("delay not elapsed: {delay_remaining_secs}s remaining"),
                            ));
                        }
                        custody::WithdrawalTimingResult::Expired => {
                            // Auto-reject expired withdrawal
                            let op_id = format!("withdrawal-expire-{}", current.withdrawal_id);
                            let _ =
                                ledger.release_cash_hold(&current.user_id, current.amount, op_id);
                            let expired = WithdrawalRecord {
                                status: "expired".into(),
                                decided_at: Some(now),
                                decided_by: Some("system".into()),
                                ..current
                            };
                            let _ = store.append(expired);
                            audit.record(
                                custody::CustodyEventType::WithdrawalExpired,
                                &principal.subject,
                                serde_json::json!({ "withdrawal_id": req.withdrawal_id }),
                            );
                            return Err(reject_api(
                                StatusCode::GONE,
                                "withdrawal expired — hold released",
                            ));
                        }
                        custody::WithdrawalTimingResult::Executable => {}
                    }

                    // ── Independent signing request creation & verification ──
                    if !current
                        .asset
                        .eq_ignore_ascii_case(SUPPORTED_WITHDRAWAL_ASSET)
                    {
                        return Err(reject_api(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!(
                                "withdrawal {} has unsupported persisted asset {}",
                                current.withdrawal_id, current.asset
                            ),
                        ));
                    }
                    let signing_req = custody::create_signing_request(
                        &current.withdrawal_id,
                        &current.destination_address,
                        current.amount,
                        &current.asset,
                        1, // chain_id
                        &custody,
                    );
                    audit.record(
                        custody::CustodyEventType::SigningRequestCreated,
                        &principal.subject,
                        serde_json::json!({
                            "signing_id": signing_req.signing_id,
                            "withdrawal_id": current.withdrawal_id,
                        }),
                    );

                    // Build a DecodedTransaction from the independently-derived signing request
                    // (In production, this would come from an independent TX decoder service)
                    let decoded = custody::DecodedTransaction {
                        to_address: current.destination_address.clone(),
                        amount: current.amount,
                        asset: current.asset.clone(),
                        chain_id: 1,
                        calldata_hash: format!("0x{}", current.withdrawal_id),
                    };
                    if let Err(mismatch) = custody::verify_signing_request(&signing_req, &decoded) {
                        brk.record_signing_failure(&mismatch);
                        brk.probe_failure(&mismatch);
                        sentinel
                            .report_custody_breaker_trip(&format!("signing_failure: {mismatch}"));
                        sentinel.report_signing_failure(&mismatch);
                        audit.record(
                            custody::CustodyEventType::SigningRejected,
                            &principal.subject,
                            serde_json::json!({
                                "withdrawal_id": current.withdrawal_id,
                                "mismatch": mismatch,
                            }),
                        );
                        return Err(reject_api(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("signing verification failed: {mismatch}"),
                        ));
                    }
                    audit.record(
                        custody::CustodyEventType::SigningVerified,
                        &principal.subject,
                        serde_json::json!({ "withdrawal_id": current.withdrawal_id }),
                    );

                    // Transfer from HOLD → SYS:WITHDRAWAL_VAULT
                    let mut approvers = current.approvers.clone();
                    approvers.push(principal.subject.clone());
                    let required_approvals = approval_threshold(&current);
                    if (approvers.len() as u32) < required_approvals {
                        let updated = WithdrawalRecord {
                            approvers: approvers.clone(),
                            ..current
                        };
                        store.append(updated.clone()).map_err(|e| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                        })?;
                        return Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "withdrawal_id": updated.withdrawal_id,
                            "status": "pending",
                            "approvals_collected": approvers.len(),
                            "approvals_required": required_approvals,
                            "remaining_approvals": approvals_remaining(&updated),
                        })));
                    }

                    let op_id = format!("withdrawal-exec-{}", current.withdrawal_id);
                    ledger
                        .transfer_cash_between_accounts(
                            &LedgerService::cash_hold_account(&current.user_id),
                            &format!("SYS:WITHDRAWAL_VAULT:{SUPPORTED_WITHDRAWAL_ASSET}"),
                            current.amount,
                            op_id,
                        )
                        .map_err(|e| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                        })?;
                    usage.record(&current.user_id, current.amount);
                    addr_usage.record(&current.destination_address, current.amount);
                    if let Some(vault_tier) = current.vault_tier {
                        vel.record_outflow(vault_tier, current.amount);
                    }

                    // On half-open breaker, mark probe success
                    brk.probe_success();

                    let approved = WithdrawalRecord {
                        status: "approved".into(),
                        decided_at: Some(now),
                        decided_by: Some(principal.subject.clone()),
                        approvers,
                        ..current
                    };
                    store.append(approved.clone()).map_err(|e| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                    })?;

                    audit.record(
                        custody::CustodyEventType::WithdrawalApproved,
                        &principal.subject,
                        serde_json::json!({
                            "withdrawal_id": approved.withdrawal_id,
                        }),
                    );

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "withdrawal_id": approved.withdrawal_id,
                        "status": "approved",
                        "decided_by": principal.subject,
                        "decided_at": approved.decided_at,
                        "approvals_collected": approved.approvers.len(),
                        "approvals_required": required_approvals,
                        "remaining_approvals": 0,
                    })))
                }
            },
        );

    // POST /admin/withdrawal/reject — admin rejects, releasing hold
    let store_for_reject = withdrawal_store.clone();
    let ledger_for_reject = ledger.clone();
    let ip_rl_reject = ip_rate_limiter.clone();
    let admin_rl_reject = admin_rate_limiter.clone();
    let reject_route = warp::path!("admin" / "withdrawal" / "reject")
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json::<WithdrawalApproveRequest>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: WithdrawalApproveRequest,
                  remote: Option<SocketAddr>| {
                let store = store_for_reject.clone();
                let ledger = ledger_for_reject.clone();
                let ip_rl = ip_rl_reject.clone();
                let admin_rl = admin_rl_reject.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let current = store
                        .get(&req.withdrawal_id)
                        .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "withdrawal not found"))?;
                    if current.status != "pending" {
                        return Err(reject_api(
                            StatusCode::CONFLICT,
                            format!("withdrawal is already {}", current.status),
                        ));
                    }

                    // Release the hold back to user's available balance
                    let op_id = format!("withdrawal-reject-{}", current.withdrawal_id);
                    ledger
                        .release_cash_hold(&current.user_id, current.amount, op_id)
                        .map_err(|e| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                        })?;

                    let rejected = WithdrawalRecord {
                        status: "rejected".into(),
                        decided_at: Some(Utc::now()),
                        decided_by: Some(principal.subject.clone()),
                        ..current
                    };
                    store.append(rejected.clone()).map_err(|e| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                    })?;

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "withdrawal_id": rejected.withdrawal_id,
                        "status": "rejected",
                        "decided_by": principal.subject,
                        "decided_at": rejected.decided_at,
                    })))
                }
            },
        );

    // GET /admin/withdrawals/pending — admin lists pending withdrawals
    let store_for_pending = withdrawal_store.clone();
    let ip_rl_pending = ip_rate_limiter.clone();
    let admin_rl_pending = admin_rate_limiter.clone();
    let pending_route = warp::path!("admin" / "withdrawals" / "pending")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let store = store_for_pending.clone();
                let ip_rl = ip_rl_pending.clone();
                let admin_rl = admin_rl_pending.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let items = store.list_pending(100);
                    let resp: Vec<_> = items
                        .into_iter()
                        .map(|w| {
                            let now = Utc::now();
                            let exec_after = w.executable_after.unwrap_or(w.requested_at);
                            let cancel_before_ts = w.cancel_before.unwrap_or(w.requested_at);
                            let timing = custody::evaluate_withdrawal_timing(
                                now,
                                exec_after,
                                cancel_before_ts,
                                86400,
                                w.requested_at,
                            );
                            let timing_str = match timing {
                                custody::WithdrawalTimingResult::Cancellable { .. } => {
                                    "cancellable"
                                }
                                custody::WithdrawalTimingResult::WaitingForDelay { .. } => {
                                    "waiting"
                                }
                                custody::WithdrawalTimingResult::Executable => "executable",
                                custody::WithdrawalTimingResult::Expired => "expired",
                            };
                            let pending_view = build_pending_view(&w);
                            serde_json::json!({
                                "withdrawal_id": pending_view.withdrawal_id,
                                "user_id": pending_view.user_id,
                                "amount": pending_view.amount,
                                "asset": pending_view.asset,
                                "destination_masked": pending_view.destination_masked,
                                "status": pending_view.status,
                                "timing_status": timing_str,
                                "requested_at": pending_view.requested_at,
                                "executable_after": pending_view.executable_after,
                                "cancel_before": pending_view.cancel_before,
                                "vault_tier": pending_view.vault_tier,
                                "required_approvals": w.required_approvals,
                                "approvers": w.approvers,
                                "remaining_approvals": approvals_remaining(&w),
                            })
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&resp))
                }
            },
        );

    request_route
        .or(list_route)
        .unify()
        .or(approve_route)
        .unify()
        .or(reject_route)
        .unify()
        .or(pending_route)
        .unify()
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_withdrawal_asset_defaults_to_usdc() {
        assert_eq!(
            normalize_withdrawal_asset(None).expect("default asset"),
            SUPPORTED_WITHDRAWAL_ASSET
        );
    }

    #[test]
    fn normalize_withdrawal_asset_rejects_non_usdc() {
        assert!(normalize_withdrawal_asset(Some("BTC".to_string())).is_err());
    }

    #[test]
    fn pending_view_masks_destination_and_tracks_remaining_approvals() {
        let record = WithdrawalRecord {
            withdrawal_id: "wd-1".into(),
            user_id: "alice".into(),
            amount: 100,
            asset: SUPPORTED_WITHDRAWAL_ASSET.into(),
            destination_address: "0xABCDEF1234567890".into(),
            status: "pending".into(),
            requested_at: Utc::now(),
            decided_at: None,
            decided_by: None,
            ledger_op_id: Some("hold-1".into()),
            executable_after: None,
            cancel_before: None,
            vault_tier: Some(custody::VaultTier::Warm),
            required_approvals: 2,
            approvers: vec!["admin-1".into()],
        };

        let view = build_pending_view(&record);
        assert_eq!(view.destination_masked, "0xABCD...7890");
        assert_eq!(approvals_remaining(&record), 1);
    }
}
