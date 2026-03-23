use super::*;
use types::LedgerEntry;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OtcQuoteRequest {
    quote_id: String,
    request_id: String,
    requester_user_id: String,
    counterparty_user_id: Option<String>,
    market_id: String,
    settlement_market_id: String,
    side: Side,
    price: i64,
    amount: i64,
    outcome: i32,
    status: String,
    created_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct EarnPosition {
    position_id: String,
    user_id: String,
    product_id: String,
    asset: String,
    principal_amount: i64,
    apr_bps: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OtcQuoteCreateRequest {
    market_id: String,
    side: Side,
    price: i64,
    amount: i64,
    #[serde(default)]
    outcome: i32,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct EarnSubscribeRequest {
    product_id: String,
    amount: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct EarnRedeemRequest {
    product_id: String,
    amount: i64,
}

#[derive(Default)]
pub(crate) struct ProductFlowStore {
    otc_quotes: DashMap<String, OtcQuoteRequest>,
    earn_positions: DashMap<String, EarnPosition>,
}

impl ProductFlowStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn earn_position_key(user_id: &str, product_id: &str) -> String {
        format!("{user_id}:{product_id}")
    }

    fn list_otc_quotes(&self, principal: &AuthenticatedPrincipal) -> Vec<OtcQuoteRequest> {
        let mut items: Vec<_> = self
            .otc_quotes
            .iter()
            .filter_map(|entry| {
                let value = entry.value();
                if principal.role == PrincipalRole::Admin
                    || value.requester_user_id == principal.subject
                    || value.counterparty_user_id.as_deref() == Some(principal.subject.as_str())
                {
                    Some(value.clone())
                } else {
                    None
                }
            })
            .collect();
        items.sort_by(|lhs, rhs| rhs.created_at.cmp(&lhs.created_at));
        items
    }

    fn create_otc_quote(
        &self,
        principal: &AuthenticatedPrincipal,
        request: OtcQuoteCreateRequest,
    ) -> anyhow::Result<OtcQuoteRequest> {
        if !request.market_id.starts_with("otc:") {
            anyhow::bail!("market_id must use otc: prefix");
        }
        if request.price <= 0 {
            anyhow::bail!("price must be positive");
        }
        if request.amount <= 0 {
            anyhow::bail!("amount must be positive");
        }
        let quote = OtcQuoteRequest {
            quote_id: types::generate_op_id("otc-quote"),
            request_id: types::generate_op_id("otc-req"),
            requester_user_id: principal.subject.clone(),
            counterparty_user_id: None,
            settlement_market_id: otc_settlement_market(&request.market_id),
            market_id: request.market_id,
            side: request.side,
            price: request.price,
            amount: request.amount,
            outcome: request.outcome,
            status: "open".to_string(),
            created_at: Utc::now(),
            accepted_at: None,
        };
        self.otc_quotes
            .insert(quote.quote_id.clone(), quote.clone());
        Ok(quote)
    }

    fn accept_otc_quote(
        &self,
        quote_id: &str,
        principal: &AuthenticatedPrincipal,
        ledger: &LedgerService,
    ) -> anyhow::Result<OtcQuoteRequest> {
        let mut quote = self
            .otc_quotes
            .get_mut(quote_id)
            .ok_or_else(|| anyhow::anyhow!("otc quote not found"))?;
        if quote.status != "open" {
            anyhow::bail!("otc quote is not open");
        }
        if quote.requester_user_id == principal.subject {
            anyhow::bail!("requester cannot self-accept otc quote");
        }
        let (buy_user_id, sell_user_id) = match quote.side {
            Side::Buy => (quote.requester_user_id.clone(), principal.subject.clone()),
            Side::Sell => (principal.subject.clone(), quote.requester_user_id.clone()),
        };
        let op_id = format!("otc_settle_{}", quote.quote_id);
        let notional = quote
            .price
            .checked_mul(quote.amount)
            .ok_or_else(|| anyhow::anyhow!("price*amount overflow"))?;
        ledger.commit_delta(LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![
                LedgerEntry {
                    debit_account: LedgerService::cash_account(&buy_user_id),
                    credit_account: LedgerService::cash_account(&sell_user_id),
                    amount: notional,
                    op_id: format!("{op_id}:cash"),
                    timestamp: Utc::now(),
                },
                LedgerEntry {
                    debit_account: LedgerService::position_account(
                        &sell_user_id,
                        &quote.settlement_market_id,
                        quote.outcome,
                    ),
                    credit_account: LedgerService::position_account(
                        &buy_user_id,
                        &quote.settlement_market_id,
                        quote.outcome,
                    ),
                    amount: quote.amount,
                    op_id: format!("{op_id}:position"),
                    timestamp: Utc::now(),
                },
            ],
            timestamp: Utc::now(),
        })?;
        quote.status = "accepted".to_string();
        quote.counterparty_user_id = Some(principal.subject.clone());
        quote.accepted_at = Some(Utc::now());
        Ok(quote.clone())
    }

    fn list_earn_positions(&self, user_id: &str) -> Vec<EarnPosition> {
        let mut items: Vec<_> = self
            .earn_positions
            .iter()
            .filter(|entry| entry.value().user_id == user_id)
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|lhs, rhs| lhs.product_id.cmp(&rhs.product_id));
        items
    }

    fn subscribe_earn(
        &self,
        principal: &AuthenticatedPrincipal,
        request: EarnSubscribeRequest,
        ledger: &LedgerService,
    ) -> anyhow::Result<EarnPosition> {
        validate_earn_product_id(&request.product_id)?;
        if request.amount <= 0 {
            anyhow::bail!("amount must be positive");
        }
        let op_id = format!(
            "earn_subscribe_{}_{}",
            principal.subject,
            types::generate_op_id("op")
        );
        ledger.transfer_cash(&principal.subject, "earn_pool", request.amount, op_id)?;
        let key = Self::earn_position_key(&principal.subject, &request.product_id);
        let asset = earn_asset_for_product(&request.product_id);
        let now = Utc::now();
        let updated = if let Some(mut existing) = self.earn_positions.get_mut(&key) {
            existing.principal_amount = existing.principal_amount.saturating_add(request.amount);
            existing.updated_at = now;
            existing.clone()
        } else {
            let created = EarnPosition {
                position_id: types::generate_op_id("earn-pos"),
                user_id: principal.subject.clone(),
                product_id: request.product_id.clone(),
                asset,
                principal_amount: request.amount,
                apr_bps: 350,
                created_at: now,
                updated_at: now,
            };
            self.earn_positions.insert(key, created.clone());
            created
        };
        Ok(updated)
    }

    fn redeem_earn(
        &self,
        principal: &AuthenticatedPrincipal,
        request: EarnRedeemRequest,
        ledger: &LedgerService,
    ) -> anyhow::Result<EarnPosition> {
        validate_earn_product_id(&request.product_id)?;
        if request.amount <= 0 {
            anyhow::bail!("amount must be positive");
        }
        let key = Self::earn_position_key(&principal.subject, &request.product_id);
        let mut existing = self
            .earn_positions
            .get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!("earn position not found"))?;
        if request.amount > existing.principal_amount {
            anyhow::bail!("redeem amount exceeds subscribed principal");
        }
        let op_id = format!(
            "earn_redeem_{}_{}",
            principal.subject,
            types::generate_op_id("op")
        );
        ledger.transfer_cash("earn_pool", &principal.subject, request.amount, op_id)?;
        existing.principal_amount -= request.amount;
        existing.updated_at = Utc::now();
        let updated = existing.clone();
        if updated.principal_amount == 0 {
            drop(existing);
            self.earn_positions.remove(&key);
            return Ok(EarnPosition {
                principal_amount: 0,
                ..updated
            });
        }
        Ok(updated)
    }
}

fn otc_settlement_market(market_id: &str) -> String {
    market_id
        .strip_prefix("otc:")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("btc-usdt")
        .to_string()
}

fn earn_asset_for_product(product_id: &str) -> String {
    product_id
        .strip_prefix("earn:")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("usdc")
        .to_ascii_uppercase()
}

fn validate_earn_product_id(product_id: &str) -> anyhow::Result<()> {
    if !product_id.starts_with("earn:") {
        anyhow::bail!("product_id must use earn: prefix");
    }
    Ok(())
}

pub(crate) fn supplemental_product_markets() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "id": "otc:btc-usdt:block",
            "market_id": "otc:btc-usdt:block",
            "name": "BTC/USDT OTC Block",
            "kind": "otc",
            "state": "normal",
            "outcomes": [0],
            "open_orders": 0,
            "markets": [],
            "trading_enabled": true,
        }),
        serde_json::json!({
            "id": "earn:usdc:flex",
            "market_id": "earn:usdc:flex",
            "name": "USDC Flexible Earn",
            "kind": "earn",
            "state": "normal",
            "outcomes": [0],
            "open_orders": 0,
            "markets": [],
            "trading_enabled": true,
        }),
    ]
}

pub(crate) fn build_product_flow_routes(
    store: Arc<ProductFlowStore>,
    ledger: Arc<LedgerService>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let otc_create_store = store.clone();
    let otc_create_ip = ip_rate_limiter.clone();
    let otc_create_user = user_rate_limiter.clone();
    let otc_create_route = warp::path!("otc" / "quotes")
        .and(warp::post())
        .and(with_principal())
        .and(body_limit())
        .and(warp::body::json())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: OtcQuoteCreateRequest,
                  remote: Option<SocketAddr>| {
                let store = otc_create_store.clone();
                let ip_rate_limiter = otc_create_ip.clone();
                let user_rate_limiter = otc_create_user.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-write:{}", principal.subject), 20)?;
                    let quote = store
                        .create_otc_quote(&principal, req)
                        .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&quote))
                }
            },
        );

    let otc_list_store = store.clone();
    let otc_list_ip = ip_rate_limiter.clone();
    let otc_list_user = user_rate_limiter.clone();
    let otc_list_route = warp::path!("otc" / "quotes")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let store = otc_list_store.clone();
                let ip_rate_limiter = otc_list_ip.clone();
                let user_rate_limiter = otc_list_user.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&store.list_otc_quotes(&principal)))
                }
            },
        );

    let otc_accept_store = store.clone();
    let otc_accept_ledger = ledger.clone();
    let otc_accept_ip = ip_rate_limiter.clone();
    let otc_accept_user = user_rate_limiter.clone();
    let otc_accept_route = warp::path!("otc" / "quotes" / String / "accept")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |quote_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let store = otc_accept_store.clone();
                let ledger = otc_accept_ledger.clone();
                let ip_rate_limiter = otc_accept_ip.clone();
                let user_rate_limiter = otc_accept_user.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-write:{}", principal.subject), 20)?;
                    let quote = store
                        .accept_otc_quote(&quote_id, &principal, ledger.as_ref())
                        .map_err(|error| {
                            let message = error.to_string();
                            if message.contains("not found") {
                                reject_api(StatusCode::NOT_FOUND, message)
                            } else if message.contains("not open")
                                || message.contains("self-accept")
                            {
                                reject_api(StatusCode::CONFLICT, message)
                            } else {
                                reject_api(StatusCode::BAD_REQUEST, message)
                            }
                        })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&quote))
                }
            },
        );

    let earn_positions_store = store.clone();
    let earn_positions_ip = ip_rate_limiter.clone();
    let earn_positions_user = user_rate_limiter.clone();
    let earn_positions_route = warp::path!("earn" / "positions" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let store = earn_positions_store.clone();
                let ip_rate_limiter = earn_positions_ip.clone();
                let user_rate_limiter = earn_positions_user.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-read:{}", principal.subject), 30)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(
                        &store.list_earn_positions(&user_id),
                    ))
                }
            },
        );

    let earn_subscribe_store = store.clone();
    let earn_subscribe_ledger = ledger.clone();
    let earn_subscribe_ip = ip_rate_limiter.clone();
    let earn_subscribe_user = user_rate_limiter.clone();
    let earn_subscribe_route = warp::path!("earn" / "subscribe")
        .and(warp::post())
        .and(with_principal())
        .and(body_limit())
        .and(warp::body::json())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: EarnSubscribeRequest,
                  remote: Option<SocketAddr>| {
                let store = earn_subscribe_store.clone();
                let ledger = earn_subscribe_ledger.clone();
                let ip_rate_limiter = earn_subscribe_ip.clone();
                let user_rate_limiter = earn_subscribe_user.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-write:{}", principal.subject), 20)?;
                    let position = store
                        .subscribe_earn(&principal, req, ledger.as_ref())
                        .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&position))
                }
            },
        );

    let earn_redeem_store = store.clone();
    let earn_redeem_ledger = ledger.clone();
    let earn_redeem_ip = ip_rate_limiter.clone();
    let earn_redeem_user = user_rate_limiter.clone();
    let earn_redeem_route = warp::path!("earn" / "redeem")
        .and(warp::post())
        .and(with_principal())
        .and(body_limit())
        .and(warp::body::json())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: EarnRedeemRequest,
                  remote: Option<SocketAddr>| {
                let store = earn_redeem_store.clone();
                let ledger = earn_redeem_ledger.clone();
                let ip_rate_limiter = earn_redeem_ip.clone();
                let user_rate_limiter = earn_redeem_user.clone();
                async move {
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    user_rate_limiter.check(&format!("user-write:{}", principal.subject), 20)?;
                    let position = store
                        .redeem_earn(&principal, req, ledger.as_ref())
                        .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&position))
                }
            },
        );

    otc_create_route
        .or(otc_list_route)
        .unify()
        .or(otc_accept_route)
        .unify()
        .or(earn_positions_route)
        .unify()
        .or(earn_subscribe_route)
        .unify()
        .or(earn_redeem_route)
        .unify()
        .boxed()
}
