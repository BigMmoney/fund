use super::*;

struct ArbitratedIndexPrice {
    selected_price: i64,
    quorum_met: bool,
    degraded: bool,
    total_sources: usize,
    active_sources: usize,
    stale_sources: usize,
    outlier_sources: usize,
}

pub(crate) struct PersistentIndexPriceStore {
    prices: DashMap<String, IndexPriceRecord>,
    store: Arc<dyn persistence::WalStore<IndexPriceRecord>>,
}

impl PersistentIndexPriceStore {
    fn new(store: Arc<dyn persistence::WalStore<IndexPriceRecord>>) -> anyhow::Result<Self> {
        let result = Self {
            prices: DashMap::new(),
            store,
        };
        for record in result.store.entries()? {
            result.prices.insert(
                index_price_source_key(&record.market_id, record.outcome, &record.source),
                record,
            );
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<IndexPriceRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub(crate) fn upsert(&self, record: IndexPriceRecord) -> anyhow::Result<()> {
        self.store.append(&record)?;
        self.prices.insert(
            index_price_source_key(&record.market_id, record.outcome, &record.source),
            record,
        );
        Ok(())
    }

    pub(crate) fn list_sources(&self, market_id: &str, outcome: i32) -> Vec<IndexPriceRecord> {
        let mut items: Vec<_> = self
            .prices
            .iter()
            .filter(|entry| {
                let value = entry.value();
                value.market_id == market_id && value.outcome == outcome
            })
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|lhs, rhs| lhs.source.cmp(&rhs.source));
        items
    }

    fn arbitrated(&self, market_id: &str, outcome: i32) -> Option<ArbitratedIndexPrice> {
        let now = Utc::now();
        let all_sources = self.list_sources(market_id, outcome);
        if all_sources.is_empty() {
            return None;
        }
        let stale_after = chrono::Duration::seconds(index_source_stale_after_secs());
        let mut active: Vec<_> = all_sources
            .iter()
            .filter(|record| now - record.recorded_at <= stale_after)
            .cloned()
            .collect();
        if active.is_empty() {
            return None;
        }
        active.sort_by_key(|record| record.index_price);
        let baseline = active[active.len() / 2].index_price;
        let max_deviation_bps = index_source_max_deviation_bps();
        let inliers: Vec<_> = active
            .iter()
            .filter(|record| deviation_bps_i64(baseline, record.index_price) <= max_deviation_bps)
            .cloned()
            .collect();
        let quorum = index_source_quorum();
        let selected_sources = if inliers.len() >= quorum {
            &inliers
        } else {
            &active
        };
        let mut prices: Vec<_> = selected_sources
            .iter()
            .map(|record| record.index_price)
            .collect();
        prices.sort();
        let selected_price = prices[prices.len() / 2];
        Some(ArbitratedIndexPrice {
            selected_price,
            quorum_met: inliers.len() >= quorum,
            degraded: inliers.len() < quorum,
            total_sources: all_sources.len(),
            active_sources: active.len(),
            stale_sources: all_sources.len().saturating_sub(active.len()),
            outlier_sources: active.len().saturating_sub(inliers.len()),
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct FairPriceQuote {
    market_id: String,
    outcome: i32,
    index_price: Option<i64>,
    index_quorum_met: bool,
    degraded_mode: bool,
    index_total_sources: usize,
    index_active_sources: usize,
    index_stale_sources: usize,
    index_outlier_sources: usize,
    reference_price: Option<i64>,
    last_trade_price: Option<i64>,
    best_bid: Option<i64>,
    best_ask: Option<i64>,
    book_mid_price: Option<i64>,
    pub(crate) fair_price: i64,
    clamp_low: Option<i64>,
    clamp_high: Option<i64>,
}

pub(crate) fn rate_key(market_id: &str, outcome: i32) -> String {
    format!("{}:{}", market_id, outcome)
}

pub(crate) fn index_price_source_key(market_id: &str, outcome: i32, source: &str) -> String {
    format!("{}:{}:{}", market_id, outcome, source)
}

pub(crate) fn index_source_quorum() -> usize {
    env::var("INDEX_SOURCE_QUORUM")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2)
        .max(1)
}

pub(crate) fn index_source_stale_after_secs() -> i64 {
    env::var("INDEX_SOURCE_STALE_AFTER_SECS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(60)
        .max(1)
}

pub(crate) fn index_source_max_deviation_bps() -> i64 {
    env::var("INDEX_SOURCE_MAX_DEVIATION_BPS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(200)
        .max(1)
}

pub(crate) fn deviation_bps_i64(reference_price: i64, attempted_price: i64) -> i64 {
    ((attempted_price - reference_price).abs() * 10_000) / reference_price.max(1)
}

pub(crate) fn fair_price_quote_for_snapshot(
    snapshot: &MarketRuntimeSnapshot,
    index_prices: &PersistentIndexPriceStore,
) -> Option<FairPriceQuote> {
    let (best_bid, best_ask) = market_best_levels(snapshot);
    let book_mid_price = match (best_bid, best_ask) {
        (Some(bid), Some(ask)) if ask >= bid => Some((bid + ask) / 2),
        _ => None,
    };
    let arbitrated_index = index_prices.arbitrated(&snapshot.market_id, snapshot.outcome);
    let index_price = arbitrated_index
        .as_ref()
        .map(|record| record.selected_price)
        .or_else(|| {
            snapshot
                .reference_sources
                .iter()
                .find(|source| source.source == "index")
                .map(|source| source.price)
        });
    let mut weighted_sum = 0i128;
    let mut weight_sum = 0i128;
    for (value, weight) in [
        (index_price, 5_000i64),
        (book_mid_price, 2_000i64),
        (snapshot.reference_price, 2_000i64),
        (snapshot.last_trade_price, 1_000i64),
    ] {
        if let Some(value) = value {
            weighted_sum =
                weighted_sum.saturating_add((value as i128).saturating_mul(weight as i128));
            weight_sum = weight_sum.saturating_add(weight as i128);
        }
    }
    if weight_sum == 0 {
        return None;
    }
    let mut fair_price = weighted_sum
        .saturating_div(weight_sum)
        .clamp(0, i64::MAX as i128) as i64;
    let clamp_anchor = index_price.or(snapshot.reference_price);
    let (clamp_low, clamp_high) = if let Some(anchor) = clamp_anchor {
        let width = anchor.abs().saturating_mul(500).saturating_div(10_000);
        let low = anchor.saturating_sub(width).max(0);
        let high = anchor.saturating_add(width);
        fair_price = fair_price.clamp(low, high);
        (Some(low), Some(high))
    } else {
        (None, None)
    };
    Some(FairPriceQuote {
        market_id: snapshot.market_id.clone(),
        outcome: snapshot.outcome,
        index_price,
        index_quorum_met: arbitrated_index
            .as_ref()
            .map(|value| value.quorum_met)
            .unwrap_or(false),
        degraded_mode: arbitrated_index
            .as_ref()
            .map(|value| value.degraded)
            .unwrap_or(false),
        index_total_sources: arbitrated_index
            .as_ref()
            .map(|value| value.total_sources)
            .unwrap_or(0),
        index_active_sources: arbitrated_index
            .as_ref()
            .map(|value| value.active_sources)
            .unwrap_or(0),
        index_stale_sources: arbitrated_index
            .as_ref()
            .map(|value| value.stale_sources)
            .unwrap_or(0),
        index_outlier_sources: arbitrated_index
            .as_ref()
            .map(|value| value.outlier_sources)
            .unwrap_or(0),
        reference_price: snapshot.reference_price,
        last_trade_price: snapshot.last_trade_price,
        best_bid,
        best_ask,
        book_mid_price,
        fair_price,
        clamp_low,
        clamp_high,
    })
}

pub(crate) fn resolve_mark_price_for_market(
    snapshots: &[MarketRuntimeSnapshot],
    index_prices: &PersistentIndexPriceStore,
    market_id: &str,
    outcome: i32,
) -> Option<i64> {
    snapshots
        .iter()
        .find(|snapshot| snapshot.market_id == market_id && snapshot.outcome == outcome)
        .and_then(|snapshot| fair_price_quote_for_snapshot(snapshot, index_prices))
        .map(|quote| quote.fair_price)
}

pub(crate) fn build_pricing_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    index_prices: Arc<PersistentIndexPriceStore>,
    governance_actions: Arc<PendingGovernanceActionStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let index_prices_for_upsert = index_prices.clone();
    let governance_actions_for_index_price = governance_actions.clone();
    let ip_rate_limiter_for_index_price = ip_rate_limiter.clone();
    let admin_rate_limiter_for_index_price = admin_rate_limiter.clone();
    let index_price_upsert_route = warp::path!("admin" / "risk" / "pricing" / "index")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: IndexPriceUpsertRequest| {
                let governance_actions = governance_actions_for_index_price.clone();
                let ip_rate_limiter = ip_rate_limiter_for_index_price.clone();
                let admin_rate_limiter = admin_rate_limiter_for_index_price.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    if req.index_price <= 0 {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "index_price must be positive",
                        ));
                    }
                    let pending = create_pending_governance_action(
                        governance_actions.as_ref(),
                        "index_price_upsert",
                        serde_json::to_value(&req).map_err(|error| {
                            reject_api(StatusCode::BAD_REQUEST, error.to_string())
                        })?,
                        &principal.subject,
                        None,
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

    let partitioned_engine_for_fair_price = partitioned_engine.clone();
    let index_prices_for_fair_price = index_prices_for_upsert.clone();
    let ip_rate_limiter_for_fair_price = ip_rate_limiter.clone();
    let fair_price_route = warp::path!("admin" / "risk" / "pricing" / "fair")
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<FairPriceQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: FairPriceQuery,
                  remote: Option<SocketAddr>| {
                let engine = partitioned_engine_for_fair_price.clone();
                let index_prices = index_prices_for_fair_price.clone();
                let ip_rate_limiter = ip_rate_limiter_for_fair_price.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    let records = engine.export_snapshots().await.map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    let outcome = query.outcome.unwrap_or(0);
                    let snapshot = flatten_market_snapshots(&records)
                        .into_iter()
                        .find(|snapshot| {
                            snapshot.market_id == query.market_id && snapshot.outcome == outcome
                        })
                        .ok_or_else(|| {
                            reject_api(StatusCode::NOT_FOUND, "market snapshot not found")
                        })?;
                    let quote = fair_price_quote_for_snapshot(&snapshot, index_prices.as_ref())
                        .ok_or_else(|| {
                            reject_api(StatusCode::BAD_REQUEST, "fair price unavailable")
                        })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "quote": quote,
                    })))
                }
            },
        );

    index_price_upsert_route
        .or(fair_price_route)
        .unify()
        .boxed()
}
