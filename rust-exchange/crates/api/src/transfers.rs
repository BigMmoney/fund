use super::*;

// ── Transfer record persisted in WAL ─────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TransferRecord {
    pub(crate) transfer_id: String,
    pub(crate) from_user_id: String,
    pub(crate) to_user_id: String,
    pub(crate) amount: i64,
    pub(crate) asset: String,
    pub(crate) memo: String,
    pub(crate) ledger_op_id: String,
    pub(crate) recorded_at: DateTime<Utc>,
}

// ── In-memory store backed by append-only WAL ────────────────

pub(crate) struct TransferStore {
    entries: Vec<TransferRecord>,
    store: Arc<dyn persistence::WalStore<TransferRecord>>,
    write_lock: Mutex<()>,
}

impl TransferStore {
    pub(crate) fn new(
        store: Arc<dyn persistence::WalStore<TransferRecord>>,
    ) -> anyhow::Result<Self> {
        let entries = store.entries().unwrap_or_default();
        Ok(Self {
            entries,
            store,
            write_lock: Mutex::new(()),
        })
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<TransferRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    fn append(&mut self, record: TransferRecord) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store.append(&record)?;
        self.entries.push(record);
        Ok(())
    }

    fn list_for_user(&self, user_id: &str, limit: usize) -> Vec<&TransferRecord> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .filter(|r| r.from_user_id == user_id || r.to_user_id == user_id)
            .collect();
        items.sort_by(|a, b| b.recorded_at.cmp(&a.recorded_at));
        items.truncate(limit);
        items
    }
}

// ── Routes ───────────────────────────────────────────────────

pub(crate) fn build_transfer_routes(
    transfer_store: Arc<parking_lot::RwLock<TransferStore>>,
    ledger: Arc<LedgerService>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let store_for_create = transfer_store.clone();
    let ledger_for_create = ledger.clone();
    let ip_rl_create = ip_rate_limiter.clone();
    let user_rl_create = user_rate_limiter.clone();

    let create_route = warp::path("transfer")
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: TransferRequest,
                  remote: Option<SocketAddr>| {
                let store = store_for_create.clone();
                let ledger = ledger_for_create.clone();
                let ip_rl = ip_rl_create.clone();
                let user_rl = user_rl_create.clone();
                async move {
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rl.check(&format!("ip:{ip_key}"), 30)?;
                    user_rl.check(&format!("user-write:{}", principal.subject), 10)?;

                    let from_user_id = principal.subject.clone();
                    let to_user_id = req.to_user_id.trim().to_string();

                    if to_user_id.is_empty() {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "to_user_id must be non-empty",
                        ));
                    }
                    if from_user_id == to_user_id {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "cannot transfer to yourself",
                        ));
                    }
                    if req.amount <= 0 {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "amount must be positive",
                        ));
                    }

                    let transfer_id = types::generate_op_id("transfer");
                    let op_id = format!("transfer:{transfer_id}");

                    ledger
                        .transfer_cash(&from_user_id, &to_user_id, req.amount, op_id.clone())
                        .map_err(|e| reject_api(StatusCode::BAD_REQUEST, e.to_string()))?;

                    let record = TransferRecord {
                        transfer_id: transfer_id.clone(),
                        from_user_id: from_user_id.clone(),
                        to_user_id: to_user_id.clone(),
                        amount: req.amount,
                        asset: req.asset.unwrap_or_else(|| "USDC".into()),
                        memo: req.memo.unwrap_or_default(),
                        ledger_op_id: op_id,
                        recorded_at: Utc::now(),
                    };

                    {
                        let mut s = store.write();
                        if let Err(e) = s.append(record.clone()) {
                            tracing::error!(error = %e, "failed to persist transfer record");
                        }
                    }

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "transfer_id": transfer_id,
                        "from_user_id": from_user_id,
                        "to_user_id": to_user_id,
                        "amount": req.amount,
                        "asset": record.asset,
                        "memo": record.memo,
                        "recorded_at": record.recorded_at,
                    })))
                }
            },
        );

    let store_for_list = transfer_store.clone();
    let ip_rl_list = ip_rate_limiter.clone();
    let user_rl_list = user_rate_limiter.clone();

    let list_route = warp::path!("transfers" / String)
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<TransferQuery>())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  query: TransferQuery,
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

                    let limit = query.limit.unwrap_or(100).min(1000);
                    let s = store.read();
                    let items: Vec<serde_json::Value> = s
                        .list_for_user(&user_id, limit)
                        .into_iter()
                        .map(|r| {
                            let direction = if r.from_user_id == user_id {
                                "outgoing"
                            } else {
                                "incoming"
                            };
                            serde_json::json!({
                                "transfer_id": r.transfer_id,
                                "direction": direction,
                                "from_user_id": r.from_user_id,
                                "to_user_id": r.to_user_id,
                                "amount": r.amount,
                                "asset": r.asset,
                                "memo": r.memo,
                                "recorded_at": r.recorded_at,
                            })
                        })
                        .collect();

                    Ok::<_, warp::Rejection>(warp::reply::json(&items))
                }
            },
        );

    create_route.or(list_route).unify().boxed()
}
