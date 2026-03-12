use super::*;

pub(crate) fn build_admin_routes(
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    funding_rates: Arc<PersistentFundingRateStore>,
    risk_automation_audit: Arc<RiskAutomationAuditStore>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let risk_for_funding_settlement = risk.clone();
    let ip_rate_limiter_for_funding_settlement = ip_rate_limiter.clone();
    let admin_rate_limiter_for_funding_settlement = admin_rate_limiter.clone();
    let funding_settlement_route = warp::path!("admin" / "risk" / "funding" / "settle")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: FundingSettlementRequest| {
                let risk = risk_for_funding_settlement.clone();
                let ip_rate_limiter = ip_rate_limiter_for_funding_settlement.clone();
                let admin_rate_limiter = admin_rate_limiter_for_funding_settlement.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = normalize_request_id(req.request_id);
                    audit("funding_settlement", &request_id, &principal);
                    let settlement = risk
                        .settle_funding_between_users(
                            &req.long_user_id,
                            &req.short_user_id,
                            &req.market_id,
                            req.outcome.unwrap_or(0),
                            req.mark_price,
                            req.funding_rate_ppm,
                            &request_id,
                        )
                        .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error.to_string()))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "settlement": settlement,
                    })))
                }
            },
        );
    let instruments_for_admin_list = instruments.clone();
    let ip_rate_limiter_for_admin_instruments_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_admin_instruments_get = admin_rate_limiter.clone();
    let admin_instruments_route = warp::path!("admin" / "instruments")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let instruments = instruments_for_admin_list.clone();
                let ip_rate_limiter = ip_rate_limiter_for_admin_instruments_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_admin_instruments_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": instruments.list(),
                    })))
                }
            },
        );
    let instruments_for_admin_upsert = instruments.clone();
    let ip_rate_limiter_for_admin_instruments_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_admin_instruments_post = admin_rate_limiter.clone();
    let admin_instruments_upsert_route = warp::path!("admin" / "instruments")
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  spec: InstrumentSpec| {
                let instruments = instruments_for_admin_upsert.clone();
                let ip_rate_limiter = ip_rate_limiter_for_admin_instruments_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_admin_instruments_post.clone();
                async move {
                    require_admin(&principal)?;
                    validate_instrument_spec(&spec)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = types::generate_op_id("instrument");
                    audit("instrument_upsert", &request_id, &principal);
                    instruments.upsert(spec.clone()).map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "instrument": spec,
                    })))
                }
            },
        );
    let funding_rates_for_get = funding_rates.clone();
    let ip_rate_limiter_for_funding_rates_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_funding_rates_get = admin_rate_limiter.clone();
    let funding_rates_route = warp::path!("admin" / "risk" / "funding-rates")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<FundingRatesQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: FundingRatesQuery,
                  remote: Option<SocketAddr>| {
                let funding_rates = funding_rates_for_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_funding_rates_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_funding_rates_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let items: Vec<_> = funding_rates
                        .list()
                        .into_iter()
                        .filter(|item| {
                            query
                                .market_id
                                .as_deref()
                                .is_none_or(|market_id| item.market_id == market_id)
                                && query.outcome.is_none_or(|outcome| item.outcome == outcome)
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": items,
                    })))
                }
            },
        );
    let funding_rates_for_post = funding_rates.clone();
    let ip_rate_limiter_for_funding_rates_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_funding_rates_post = admin_rate_limiter.clone();
    let funding_rates_upsert_route = warp::path!("admin" / "risk" / "funding-rates")
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: FundingRateUpsertRequest| {
                let funding_rates = funding_rates_for_post.clone();
                let ip_rate_limiter = ip_rate_limiter_for_funding_rates_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_funding_rates_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let record = FundingRateRecord {
                        market_id: req.market_id,
                        outcome: req.outcome.unwrap_or(0),
                        funding_rate_ppm: req.funding_rate_ppm,
                        updated_by: principal.subject.clone(),
                        recorded_at: Utc::now(),
                    };
                    let request_id = types::generate_op_id("funding-rate");
                    audit("funding_rate_upsert", &request_id, &principal);
                    funding_rates.upsert(record.clone()).map_err(|error| {
                        reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "item": record,
                    })))
                }
            },
        );
    let risk_automation_audit_for_events = risk_automation_audit.clone();
    let ip_rate_limiter_for_risk_events = ip_rate_limiter.clone();
    let admin_rate_limiter_for_risk_events = admin_rate_limiter.clone();
    let risk_events_route = warp::path!("admin" / "risk" / "events")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<RiskEventsQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: RiskEventsQuery,
                  remote: Option<SocketAddr>| {
                let audit_store = risk_automation_audit_for_events.clone();
                let ip_rate_limiter = ip_rate_limiter_for_risk_events.clone();
                let admin_rate_limiter = admin_rate_limiter_for_risk_events.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let items = audit_store
                        .list_recent(query.limit.unwrap_or(100).clamp(1, 1000))
                        .map_err(|error| {
                            reject_api(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                        })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": items,
                    })))
                }
            },
        );
    funding_settlement_route
        .or(admin_instruments_route)
        .unify()
        .or(admin_instruments_upsert_route)
        .unify()
        .or(funding_rates_route)
        .unify()
        .or(funding_rates_upsert_route)
        .unify()
        .or(risk_events_route)
        .unify()
        .boxed()
}
