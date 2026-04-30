use super::*;

// ── Fee tier definitions ─────────────────────────────────────

/// A single fee tier level.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FeeTier {
    pub(crate) tier: String,
    pub(crate) min_volume_30d: i64,
    pub(crate) maker_fee_bps: i64,
    pub(crate) taker_fee_bps: i64,
}

/// Persistent store of fee tiers, backed by WAL.
pub(crate) struct FeeTierStore {
    tiers: parking_lot::RwLock<Vec<FeeTier>>,
    store: Arc<dyn persistence::WalStore<FeeTier>>,
}

impl FeeTierStore {
    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<FeeTier>> = Arc::new(JsonlFileWal::new(path)?);
        let mut tiers = store.entries()?;
        if tiers.is_empty() {
            tiers = default_tiers();
            for tier in &tiers {
                store.append(tier)?;
            }
        }
        Ok(Self {
            tiers: parking_lot::RwLock::new(tiers),
            store,
        })
    }

    pub(crate) fn list(&self) -> Vec<FeeTier> {
        self.tiers.read().clone()
    }

    /// Resolve the tier for a given 30-day trading volume.
    pub(crate) fn resolve(&self, volume_30d: i64) -> FeeTier {
        let tiers = self.tiers.read();
        let mut best = tiers.first().cloned().unwrap_or_else(|| FeeTier {
            tier: "VIP0".into(),
            min_volume_30d: 0,
            maker_fee_bps: 10,
            taker_fee_bps: 10,
        });
        for t in tiers.iter() {
            if volume_30d >= t.min_volume_30d && t.min_volume_30d >= best.min_volume_30d {
                best = t.clone();
            }
        }
        best
    }

    /// Replace all tiers with a new set (admin operation).
    pub(crate) fn replace_all(&self, new_tiers: Vec<FeeTier>) -> anyhow::Result<()> {
        for tier in &new_tiers {
            self.store.append(tier)?;
        }
        *self.tiers.write() = new_tiers;
        Ok(())
    }
}

fn default_tiers() -> Vec<FeeTier> {
    vec![
        FeeTier {
            tier: "VIP0".into(),
            min_volume_30d: 0,
            maker_fee_bps: 10,
            taker_fee_bps: 10,
        },
        FeeTier {
            tier: "VIP1".into(),
            min_volume_30d: 100_000,
            maker_fee_bps: 8,
            taker_fee_bps: 8,
        },
        FeeTier {
            tier: "VIP2".into(),
            min_volume_30d: 1_000_000,
            maker_fee_bps: 5,
            taker_fee_bps: 6,
        },
        FeeTier {
            tier: "VIP3".into(),
            min_volume_30d: 10_000_000,
            maker_fee_bps: 2,
            taker_fee_bps: 4,
        },
    ]
}

// ── Routes ───────────────────────────────────────────────────

pub(crate) fn build_fee_tier_routes(
    fee_tier_store: Arc<FeeTierStore>,
    trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    // GET /fee-tiers — list all tiers (public)
    let store_for_list = fee_tier_store.clone();
    let ip_rl_list = ip_rate_limiter.clone();
    let list_route = warp::path!("fee-tiers")
        .and(warp::get())
        .and(remote_ip())
        .and_then(move |remote: Option<SocketAddr>| {
            let store = store_for_list.clone();
            let ip_rl = ip_rl_list.clone();
            async move {
                let ip_key = remote
                    .map(|v| v.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                Ok::<_, warp::Rejection>(warp::reply::json(&store.list()))
            }
        });

    // GET /fee-tier/{user_id} — resolve user's tier from 30d volume
    let store_for_resolve = fee_tier_store.clone();
    let trade_journal_for_resolve = trade_journal_wal.clone();
    let ip_rl_resolve = ip_rate_limiter.clone();
    let user_rl_resolve = user_rate_limiter.clone();
    let resolve_route = warp::path!("fee-tier" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let store = store_for_resolve.clone();
                let trade_journal = trade_journal_for_resolve.clone();
                let ip_rl = ip_rl_resolve.clone();
                let user_rl = user_rl_resolve.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-read:{}", principal.subject), 30)?;

                    let cutoff = Utc::now() - chrono::Duration::days(30);
                    let trades = wal_entries_or_empty(trade_journal.as_ref())
                        .map_err(reject_internal_error)?;
                    let volume_30d: i64 = trades
                        .iter()
                        .filter(|t| t.recorded_at >= cutoff)
                        .filter(|t| t.buy_user_id == user_id || t.sell_user_id == user_id)
                        .map(|t| t.price.saturating_mul(t.amount))
                        .sum();
                    let tier = store.resolve(volume_30d);
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "user_id": user_id,
                        "volume_30d": volume_30d,
                        "tier": tier.tier,
                        "maker_fee_bps": tier.maker_fee_bps,
                        "taker_fee_bps": tier.taker_fee_bps,
                    })))
                }
            },
        );

    // PUT /admin/fee-tiers — replace all tiers (admin)
    let store_for_update = fee_tier_store.clone();
    let ip_rl_update = ip_rate_limiter.clone();
    let admin_rl_update = admin_rate_limiter.clone();
    let update_route = warp::path!("admin" / "fee-tiers")
        .and(warp::put())
        .and(with_principal())
        .and(warp::body::json::<Vec<FeeTier>>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  tiers: Vec<FeeTier>,
                  remote: Option<SocketAddr>| {
                let store = store_for_update.clone();
                let ip_rl = ip_rl_update.clone();
                let admin_rl = admin_rl_update.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    if tiers.is_empty() {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "at least one fee tier is required",
                        ));
                    }
                    for t in &tiers {
                        if t.maker_fee_bps < 0 || t.taker_fee_bps < 0 {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                "fee_bps must be non-negative",
                            ));
                        }
                    }

                    store
                        .replace_all(tiers.clone())
                        .map_err(reject_internal_error)?;

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "tiers": tiers.len(),
                    })))
                }
            },
        );

    list_route
        .or(resolve_route)
        .unify()
        .or(update_route)
        .unify()
        .boxed()
}
