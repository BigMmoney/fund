use super::*;

struct ArbitratedIndexPrice {
    selected_price: i64,
    quorum_met: bool,
    degraded: bool,
    total_sources: usize,
    active_sources: usize,
    degraded_sources: usize,
    quarantined_sources: usize,
    stale_sources: usize,
    outlier_sources: usize,
}

pub(crate) struct PersistentIndexPriceStore {
    prices: DashMap<String, IndexPriceRecord>,
    policies: DashMap<String, IndexSourcePolicyRecord>,
    store: Arc<dyn persistence::WalStore<IndexPriceRecord>>,
    policy_store: Arc<dyn persistence::WalStore<IndexSourcePolicyRecord>>,
}

impl PersistentIndexPriceStore {
    fn new(
        store: Arc<dyn persistence::WalStore<IndexPriceRecord>>,
        policy_store: Arc<dyn persistence::WalStore<IndexSourcePolicyRecord>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            prices: DashMap::new(),
            policies: DashMap::new(),
            store,
            policy_store,
        };
        for record in result.store.entries()? {
            result.prices.insert(
                index_price_source_key(&record.market_id, record.outcome, &record.source),
                record,
            );
        }
        for record in result.policy_store.entries()? {
            result.policies.insert(
                index_price_source_key(&record.market_id, record.outcome, &record.source),
                record,
            );
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(
        path: impl Into<std::path::PathBuf>,
        policy_path: impl Into<std::path::PathBuf>,
    ) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<IndexPriceRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        let policy_store: Arc<dyn persistence::WalStore<IndexSourcePolicyRecord>> =
            Arc::new(JsonlFileWal::new(policy_path)?);
        Self::new(store, policy_store)
    }

    pub(crate) fn upsert(&self, record: IndexPriceRecord) -> anyhow::Result<()> {
        self.store.append(&record)?;
        self.prices.insert(
            index_price_source_key(&record.market_id, record.outcome, &record.source),
            record,
        );
        Ok(())
    }

    pub(crate) fn upsert_policy(&self, record: IndexSourcePolicyRecord) -> anyhow::Result<()> {
        self.policy_store.append(&record)?;
        self.policies.insert(
            index_price_source_key(&record.market_id, record.outcome, &record.source),
            record,
        );
        Ok(())
    }

    fn policy_for(
        &self,
        market_id: &str,
        outcome: i32,
        source: &str,
    ) -> Option<IndexSourcePolicyRecord> {
        self.policies
            .get(&index_price_source_key(market_id, outcome, source))
            .map(|entry| entry.value().clone())
    }

    pub(crate) fn list_source_policies(
        &self,
        market_id: &str,
        outcome: i32,
    ) -> Vec<IndexSourcePolicyRecord> {
        let mut items: Vec<_> = self
            .policies
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
        let active: Vec<_> = all_sources
            .iter()
            .filter(|record| now - record.recorded_at <= stale_after)
            .filter(|record| {
                self.policy_for(market_id, outcome, &record.source)
                    .map(|policy| policy.status != "quarantined")
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        if active.is_empty() {
            return None;
        }
        let mut baseline_prices: Vec<_> = active.iter().map(|record| record.index_price).collect();
        baseline_prices.sort();
        let baseline = baseline_prices[baseline_prices.len() / 2];
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
        let selected_price = weighted_median_price(selected_sources, |record| {
            self.policy_for(market_id, outcome, &record.source)
                .map(|policy| policy.weight_bps.clamp(1, 10_000))
                .unwrap_or(default_index_source_weight_bps())
        });
        let degraded_sources = active
            .iter()
            .filter(|record| {
                self.policy_for(market_id, outcome, &record.source)
                    .map(|policy| policy.status == "degraded")
                    .unwrap_or(false)
            })
            .count();
        let quarantined_sources = all_sources
            .len()
            .saturating_sub(active.len())
            .saturating_sub(
                all_sources
                    .iter()
                    .filter(|record| now - record.recorded_at > stale_after)
                    .count(),
            );
        Some(ArbitratedIndexPrice {
            selected_price,
            quorum_met: inliers.len() >= quorum,
            degraded: inliers.len() < quorum || degraded_sources > 0 || quarantined_sources > 0,
            total_sources: all_sources.len(),
            active_sources: active.len(),
            degraded_sources,
            quarantined_sources,
            stale_sources: all_sources.len().saturating_sub(active.len()),
            outlier_sources: active.len().saturating_sub(inliers.len()),
        })
    }
}

fn weighted_median_price<F>(records: &[IndexPriceRecord], weight_of: F) -> i64
where
    F: Fn(&IndexPriceRecord) -> i64,
{
    let mut weighted: Vec<_> = records
        .iter()
        .map(|record| (record.index_price, weight_of(record).max(1)))
        .collect();
    weighted.sort_by_key(|(price, _)| *price);
    let total_weight: i64 = weighted.iter().map(|(_, weight)| *weight).sum();
    let midpoint = (total_weight.max(1) + 1) / 2;
    let mut acc = 0i64;
    for (price, weight) in weighted {
        acc = acc.saturating_add(weight);
        if acc >= midpoint {
            return price;
        }
    }
    records.last().map(|record| record.index_price).unwrap_or(0)
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
    index_degraded_sources: usize,
    index_quarantined_sources: usize,
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

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct DerivedFundingRateQuote {
    pub(crate) market_id: String,
    pub(crate) outcome: i32,
    pub(crate) mark_price: i64,
    pub(crate) fair_price: i64,
    pub(crate) index_price: i64,
    pub(crate) premium_bps: i64,
    pub(crate) clamped_premium_bps: i64,
    pub(crate) interest_bps: i64,
    pub(crate) funding_rate_ppm: i64,
    pub(crate) degraded_mode: bool,
}

pub(crate) fn rate_key(market_id: &str, outcome: i32) -> String {
    format!("{market_id}:{outcome}")
}

pub(crate) fn index_price_source_key(market_id: &str, outcome: i32, source: &str) -> String {
    format!("{market_id}:{outcome}:{source}")
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

pub(crate) fn funding_premium_cap_bps() -> i64 {
    env::var("FUNDING_PREMIUM_CAP_BPS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(50)
        .max(1)
}

pub(crate) fn funding_interest_bps_per_interval() -> i64 {
    env::var("FUNDING_INTEREST_BPS_PER_INTERVAL")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0)
}

pub(crate) fn funding_rate_max_abs_ppm() -> i64 {
    env::var("FUNDING_RATE_MAX_ABS_PPM")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(5_000)
        .max(100)
}

pub(crate) fn derive_funding_rate_quote(
    snapshot: &MarketRuntimeSnapshot,
    index_prices: &PersistentIndexPriceStore,
) -> Option<DerivedFundingRateQuote> {
    let fair_quote = fair_price_quote_for_snapshot(snapshot, index_prices)?;
    let index_price = fair_quote.index_price?;
    let premium_bps = ((fair_quote.fair_price - index_price).saturating_mul(10_000))
        .saturating_div(index_price.abs().max(1));
    let clamped_premium_bps =
        premium_bps.clamp(-funding_premium_cap_bps(), funding_premium_cap_bps());
    let interest_bps = match clamped_premium_bps.cmp(&0) {
        std::cmp::Ordering::Greater => funding_interest_bps_per_interval(),
        std::cmp::Ordering::Less => -funding_interest_bps_per_interval(),
        std::cmp::Ordering::Equal => 0,
    };
    let funding_rate_ppm = clamped_premium_bps
        .saturating_add(interest_bps)
        .saturating_mul(100)
        .clamp(-funding_rate_max_abs_ppm(), funding_rate_max_abs_ppm());
    Some(DerivedFundingRateQuote {
        market_id: snapshot.market_id.clone(),
        outcome: snapshot.outcome,
        mark_price: fair_quote.fair_price,
        fair_price: fair_quote.fair_price,
        index_price,
        premium_bps,
        clamped_premium_bps,
        interest_bps,
        funding_rate_ppm,
        degraded_mode: fair_quote.degraded_mode,
    })
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
        index_degraded_sources: arbitrated_index
            .as_ref()
            .map(|value| value.degraded_sources)
            .unwrap_or(0),
        index_quarantined_sources: arbitrated_index
            .as_ref()
            .map(|value| value.quarantined_sources)
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
                    .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "pending",
                        "approval": pending,
                    })))
                }
            },
        );

    let governance_actions_for_source_policy = governance_actions.clone();
    let ip_rate_limiter_for_source_policy_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_source_policy_post = admin_rate_limiter.clone();
    let source_policy_post_route = warp::path!("admin" / "risk" / "pricing" / "sources")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: IndexSourcePolicyUpdateRequest| {
                let governance_actions = governance_actions_for_source_policy.clone();
                let ip_rate_limiter = ip_rate_limiter_for_source_policy_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_source_policy_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let normalized = req.status.trim().to_ascii_lowercase();
                    if !matches!(normalized.as_str(), "active" | "degraded" | "quarantined") {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "status must be active, degraded, or quarantined",
                        ));
                    }
                    let pending = create_pending_governance_action(
                        governance_actions.as_ref(),
                        "index_source_policy_update",
                        serde_json::json!({
                            "market_id": req.market_id,
                            "outcome": req.outcome,
                            "source": req.source,
                            "status": normalized,
                            "weight_bps": req.weight_bps,
                        }),
                        &principal.subject,
                        None,
                    )
                    .map_err(reject_internal_error)?;
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
                    let records = engine
                        .export_snapshots()
                        .await
                        .map_err(reject_internal_error)?;
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

    let index_prices_for_source_policy_get = index_prices.clone();
    let ip_rate_limiter_for_source_policy_get = ip_rate_limiter.clone();
    let source_policy_get_route = warp::path!("admin" / "risk" / "pricing" / "sources")
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<IndexSourcePolicyQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: IndexSourcePolicyQuery,
                  remote: Option<SocketAddr>| {
                let index_prices = index_prices_for_source_policy_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_source_policy_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    let outcome = query.outcome.unwrap_or(0);
                    let items = index_prices.list_source_policies(&query.market_id, outcome);
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": items,
                    })))
                }
            },
        );

    index_price_upsert_route
        .or(source_policy_post_route)
        .unify()
        .or(fair_price_route)
        .unify()
        .or(source_policy_get_route)
        .unify()
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use matching::partitioned::ReferencePriceSourceSnapshot;
    use persistence::InMemoryWal;
    use types::{MarketState, OrderType, Side, TimeInForce};

    fn sample_snapshot(last_trade_price: i64) -> MarketRuntimeSnapshot {
        MarketRuntimeSnapshot {
            market_id: "perp:btc-usdt".to_string(),
            outcome: 0,
            state: MarketState::Normal,
            reference_price: Some(last_trade_price),
            last_trade_price: Some(last_trade_price),
            reference_sources: vec![ReferencePriceSourceSnapshot {
                source: "index".to_string(),
                price: 100,
                updated_at: Utc::now(),
            }],
            orders: vec![
                RestingOrderSnapshot {
                    order_id: "bid-1".to_string(),
                    request_id: "req-bid-1".to_string(),
                    command_seq: Some(1),
                    user_id: "u1".to_string(),
                    session_id: None,
                    market_id: "perp:btc-usdt".to_string(),
                    outcome: 0,
                    side: Side::Buy,
                    price: last_trade_price.saturating_sub(1),
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    post_only: false,
                    reduce_only: false,
                    leverage: Some(5),
                    original_amount: 1,
                    remaining_amount: 1,
                    expires_at: None,
                    stp_mode: types::StpMode::default(),
                    display_qty: None,
                    min_fill_qty: None,
                    stp_group_id: None,
                    is_market_maker: false,
                },
                RestingOrderSnapshot {
                    order_id: "ask-1".to_string(),
                    request_id: "req-ask-1".to_string(),
                    command_seq: Some(2),
                    user_id: "u2".to_string(),
                    session_id: None,
                    market_id: "perp:btc-usdt".to_string(),
                    outcome: 0,
                    side: Side::Sell,
                    price: last_trade_price.saturating_add(1),
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    post_only: false,
                    reduce_only: false,
                    leverage: Some(5),
                    original_amount: 1,
                    remaining_amount: 1,
                    expires_at: None,
                    stp_mode: types::StpMode::default(),
                    display_qty: None,
                    min_fill_qty: None,
                    stp_group_id: None,
                    is_market_maker: false,
                },
            ],
            trade_stats: Default::default(),
            trigger_orders: vec![],
        }
    }

    fn sample_index_store() -> PersistentIndexPriceStore {
        let store: Arc<dyn persistence::WalStore<IndexPriceRecord>> =
            Arc::new(InMemoryWal::<IndexPriceRecord>::new());
        let policy_store: Arc<dyn persistence::WalStore<IndexSourcePolicyRecord>> =
            Arc::new(InMemoryWal::<IndexSourcePolicyRecord>::new());
        let result = PersistentIndexPriceStore::new(store, policy_store).expect("index store");
        result
            .upsert(IndexPriceRecord {
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                source: "oracle-a".to_string(),
                index_price: 100,
                recorded_at: Utc::now(),
            })
            .expect("index upsert a");
        result
            .upsert(IndexPriceRecord {
                market_id: "perp:btc-usdt".to_string(),
                outcome: 0,
                source: "oracle-b".to_string(),
                index_price: 100,
                recorded_at: Utc::now(),
            })
            .expect("index upsert b");
        result
    }

    #[test]
    fn derive_funding_rate_quote_uses_fair_minus_index_premium() {
        let snapshot = sample_snapshot(104);
        let store = sample_index_store();
        let quote = derive_funding_rate_quote(&snapshot, &store).expect("funding quote");
        assert_eq!(quote.index_price, 100);
        assert!(quote.fair_price > quote.index_price);
        assert!(quote.premium_bps > 0);
        assert!(quote.funding_rate_ppm > 0);
    }

    #[test]
    fn derive_funding_rate_quote_is_capped() {
        let snapshot = sample_snapshot(400);
        let store = sample_index_store();
        let quote = derive_funding_rate_quote(&snapshot, &store).expect("funding quote");
        assert_eq!(quote.clamped_premium_bps, funding_premium_cap_bps());
        assert_eq!(
            quote.funding_rate_ppm,
            funding_rate_max_abs_ppm().min(funding_premium_cap_bps() * 100)
        );
    }
}
