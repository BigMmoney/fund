use super::*;

pub(crate) fn build_admin_routes(
    risk: Arc<RiskEngine>,
    instruments: Arc<PersistentInstrumentRegistry>,
    ledger: Arc<LedgerService>,
    funding_rates: Arc<PersistentFundingRateStore>,
    risk_automation_audit: Arc<RiskAutomationAuditStore>,
    beta_controls: Arc<BetaControlStore>,
    admin_action_audit: Arc<AdminActionAuditStore>,
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
                        .map_err(|error| {
                            reject_api(
                                StatusCode::BAD_REQUEST,
                                super::helpers::sanitize_internal_error(&error.to_string()),
                            )
                        })?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "settlement": settlement,
                    })))
                }
            },
        )
        .boxed();
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
        )
        .boxed();
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
                    instruments
                        .upsert(spec.clone())
                        .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "instrument": spec,
                    })))
                }
            },
        )
        .boxed();
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
        )
        .boxed();
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
                    funding_rates
                        .upsert(record.clone())
                        .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "item": record,
                    })))
                }
            },
        )
        .boxed();
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
                        .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": items,
                    })))
                }
            },
        )
        .boxed();
    let risk_for_user_limits_get = risk.clone();
    let ip_rate_limiter_for_user_limits_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_user_limits_get = admin_rate_limiter.clone();
    let user_risk_limits_route = warp::path!("admin" / "risk" / "users" / String / "limits")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let risk = risk_for_user_limits_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_user_limits_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_user_limits_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let limits = risk.user_risk_limits(&user_id).unwrap_or_default();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "user_id": user_id,
                        "limits": limits,
                    })))
                }
            },
        )
        .boxed();
    let risk_for_user_limits_post = risk.clone();
    let ip_rate_limiter_for_user_limits_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_user_limits_post = admin_rate_limiter.clone();
    let user_risk_limits_upsert_route = warp::path!("admin" / "risk" / "users" / String / "limits")
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  limits: types::UserRiskLimits| {
                let risk = risk_for_user_limits_post.clone();
                let ip_rate_limiter = ip_rate_limiter_for_user_limits_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_user_limits_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = types::generate_op_id("risk-limits");
                    audit("user_risk_limits_upsert", &request_id, &principal);
                    risk.set_user_risk_limits(&user_id, limits.clone());
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "user_id": user_id,
                        "limits": limits,
                    })))
                }
            },
        )
        .boxed();
    let ledger_for_fee_collector = ledger.clone();
    let ip_rate_limiter_for_fee_collector = ip_rate_limiter.clone();
    let admin_rate_limiter_for_fee_collector = admin_rate_limiter.clone();
    let fee_collector_route = warp::path!("admin" / "treasury" / "fee-collector")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ledger = ledger_for_fee_collector.clone();
                let ip_rate_limiter = ip_rate_limiter_for_fee_collector.clone();
                let admin_rate_limiter = admin_rate_limiter_for_fee_collector.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "account_id": "SYS:FEE_COLLECTOR:USDC",
                        "balance": ledger.fee_collector_balance(),
                    })))
                }
            },
        )
        .boxed();
    let ledger_for_insurance_funds = ledger.clone();
    let instruments_for_insurance_funds = instruments.clone();
    let ip_rate_limiter_for_insurance_funds = ip_rate_limiter.clone();
    let admin_rate_limiter_for_insurance_funds = admin_rate_limiter.clone();
    let insurance_funds_route = warp::path!("admin" / "treasury" / "insurance-funds")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ledger = ledger_for_insurance_funds.clone();
                let instruments = instruments_for_insurance_funds.clone();
                let ip_rate_limiter = ip_rate_limiter_for_insurance_funds.clone();
                let admin_rate_limiter = admin_rate_limiter_for_insurance_funds.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let mut per_market: Vec<_> = instruments
                        .list()
                        .into_iter()
                        .map(|instrument| {
                            serde_json::json!({
                                "market_id": instrument.instrument_id,
                                "account_id": LedgerService::insurance_fund_account_for(&instrument.instrument_id),
                                "balance": ledger.insurance_fund_balance_for(&instrument.instrument_id),
                            })
                        })
                        .collect();
                    per_market.sort_by(|lhs, rhs| lhs["market_id"].as_str().cmp(&rhs["market_id"].as_str()));
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "global": {
                            "account_id": LedgerService::insurance_fund_account(),
                            "balance": ledger.insurance_fund_balance(),
                        },
                        "per_market": per_market,
                    })))
                }
            },
        )
        .boxed();
    let ledger_for_market_treasury = ledger.clone();
    let instruments_for_market_treasury = instruments.clone();
    let ip_rate_limiter_for_market_treasury = ip_rate_limiter.clone();
    let admin_rate_limiter_for_market_treasury = admin_rate_limiter.clone();
    let market_treasury_route = warp::path!("admin" / "treasury" / "markets" / String)
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |market_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let ledger = ledger_for_market_treasury.clone();
                let instruments = instruments_for_market_treasury.clone();
                let ip_rate_limiter = ip_rate_limiter_for_market_treasury.clone();
                let admin_rate_limiter = admin_rate_limiter_for_market_treasury.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let instrument = instruments
                        .get(&market_id)
                        .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "market not found"))?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "market_id": market_id,
                        "instrument_kind": instrument.kind,
                        "insurance_fund": {
                            "account_id": LedgerService::insurance_fund_account_for(&instrument.instrument_id),
                            "balance": ledger.insurance_fund_balance_for(&instrument.instrument_id),
                        },
                        "fee_collector": {
                            "account_id": "SYS:FEE_COLLECTOR:USDC",
                            "balance": ledger.fee_collector_balance(),
                            "scope": "global",
                        },
                    })))
                }
            },
        )
        .boxed();
    let beta_controls_for_get = beta_controls.clone();
    let ip_rate_limiter_for_beta_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_beta_get = admin_rate_limiter.clone();
    let beta_control_plane_route = warp::path!("admin" / "beta" / "control-plane")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let beta_controls = beta_controls_for_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_beta_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_beta_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "control_plane": beta_controls.control_plane(),
                        "counts": {
                            "users": beta_controls.list_users().len(),
                            "markets": beta_controls.list_markets().len(),
                        }
                    })))
                }
            },
        )
        .boxed();
    let beta_controls_for_post = beta_controls.clone();
    let ip_rate_limiter_for_beta_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_beta_post = admin_rate_limiter.clone();
    let beta_control_plane_upsert_route = warp::path!("admin" / "beta" / "control-plane")
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: AdminBetaControlPlaneUpdateRequest| {
                let beta_controls = beta_controls_for_post.clone();
                let ip_rate_limiter = ip_rate_limiter_for_beta_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_beta_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let request_id = types::generate_op_id("beta-control");
                    let current = beta_controls.control_plane();
                    let next = BetaControlPlaneConfig {
                        enabled: req.enabled.unwrap_or(current.enabled),
                        require_whitelist: req
                            .require_whitelist
                            .unwrap_or(current.require_whitelist),
                        updated_by: principal.subject.clone(),
                        recorded_at: Utc::now(),
                    };
                    audit("beta_control_plane_upsert", &request_id, &principal);
                    beta_controls
                        .upsert_control_plane(next.clone())
                        .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "control_plane": next,
                    })))
                }
            },
        )
        .boxed();
    let beta_controls_for_users_list = beta_controls.clone();
    let ip_rate_limiter_for_beta_users_list = ip_rate_limiter.clone();
    let admin_rate_limiter_for_beta_users_list = admin_rate_limiter.clone();
    let beta_users_route = warp::path!("admin" / "beta" / "users")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let beta_controls = beta_controls_for_users_list.clone();
                let ip_rate_limiter = ip_rate_limiter_for_beta_users_list.clone();
                let admin_rate_limiter = admin_rate_limiter_for_beta_users_list.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": beta_controls.list_users(),
                    })))
                }
            },
        )
        .boxed();
    let beta_controls_for_user_get = beta_controls.clone();
    let ip_rate_limiter_for_beta_user_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_beta_user_get = admin_rate_limiter.clone();
    let beta_user_route = warp::path!("admin" / "beta" / "users" / String)
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let beta_controls = beta_controls_for_user_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_beta_user_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_beta_user_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let control = beta_controls.user(&user_id).unwrap_or(BetaUserControl {
                        user_id: user_id.clone(),
                        whitelisted: false,
                        max_cash_balance: None,
                        max_open_orders: None,
                        updated_by: String::new(),
                        recorded_at: Utc::now(),
                    });
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "user": control,
                    })))
                }
            },
        )
        .boxed();
    let beta_controls_for_user_post = beta_controls.clone();
    let ip_rate_limiter_for_beta_user_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_beta_user_post = admin_rate_limiter.clone();
    let beta_user_upsert_route = warp::path!("admin" / "beta" / "users" / String)
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: AdminBetaUserControlUpdateRequest| {
                let beta_controls = beta_controls_for_user_post.clone();
                let ip_rate_limiter = ip_rate_limiter_for_beta_user_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_beta_user_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    if req.max_cash_balance.is_some_and(|value| value <= 0) {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "max_cash_balance must be positive when provided",
                        ));
                    }
                    if req.max_open_orders == Some(0) {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "max_open_orders must be >= 1 when provided",
                        ));
                    }
                    let request_id = types::generate_op_id("beta-user");
                    let current = beta_controls.user(&user_id).unwrap_or(BetaUserControl {
                        user_id: user_id.clone(),
                        whitelisted: false,
                        max_cash_balance: None,
                        max_open_orders: None,
                        updated_by: String::new(),
                        recorded_at: Utc::now(),
                    });
                    let next = BetaUserControl {
                        user_id: user_id.clone(),
                        whitelisted: req.whitelisted.unwrap_or(current.whitelisted),
                        max_cash_balance: req.max_cash_balance.or(current.max_cash_balance),
                        max_open_orders: req.max_open_orders.or(current.max_open_orders),
                        updated_by: principal.subject.clone(),
                        recorded_at: Utc::now(),
                    };
                    audit("beta_user_upsert", &request_id, &principal);
                    beta_controls
                        .upsert_user(next.clone())
                        .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "user": next,
                    })))
                }
            },
        )
        .boxed();
    let beta_controls_for_markets_list = beta_controls.clone();
    let ip_rate_limiter_for_beta_markets_list = ip_rate_limiter.clone();
    let admin_rate_limiter_for_beta_markets_list = admin_rate_limiter.clone();
    let beta_markets_route = warp::path!("admin" / "beta" / "markets")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let beta_controls = beta_controls_for_markets_list.clone();
                let ip_rate_limiter = ip_rate_limiter_for_beta_markets_list.clone();
                let admin_rate_limiter = admin_rate_limiter_for_beta_markets_list.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": beta_controls.list_markets(),
                    })))
                }
            },
        )
        .boxed();
    let beta_controls_for_market_get = beta_controls.clone();
    let ip_rate_limiter_for_beta_market_get = ip_rate_limiter.clone();
    let admin_rate_limiter_for_beta_market_get = admin_rate_limiter.clone();
    let beta_market_route = warp::path!("admin" / "beta" / "markets" / String)
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |market_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let beta_controls = beta_controls_for_market_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_beta_market_get.clone();
                let admin_rate_limiter = admin_rate_limiter_for_beta_market_get.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let control = beta_controls
                        .market(&market_id)
                        .unwrap_or(BetaMarketControl {
                            market_id: market_id.clone(),
                            max_order_notional: None,
                            max_leverage: None,
                            updated_by: String::new(),
                            recorded_at: Utc::now(),
                        });
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "market": control,
                    })))
                }
            },
        )
        .boxed();
    let beta_controls_for_market_post = beta_controls.clone();
    let instruments_for_beta_market_post = instruments.clone();
    let ip_rate_limiter_for_beta_market_post = ip_rate_limiter.clone();
    let admin_rate_limiter_for_beta_market_post = admin_rate_limiter.clone();
    let beta_market_upsert_route = warp::path!("admin" / "beta" / "markets" / String)
        .and(warp::path::end())
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |market_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: AdminBetaMarketControlUpdateRequest| {
                let beta_controls = beta_controls_for_market_post.clone();
                let instruments = instruments_for_beta_market_post.clone();
                let ip_rate_limiter = ip_rate_limiter_for_beta_market_post.clone();
                let admin_rate_limiter = admin_rate_limiter_for_beta_market_post.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let instrument = instruments
                        .get(&market_id)
                        .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "market not found"))?;
                    if req.max_order_notional.is_some_and(|value| value <= 0) {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "max_order_notional must be positive when provided",
                        ));
                    }
                    if req.max_leverage == Some(0) {
                        return Err(reject_api(
                            StatusCode::BAD_REQUEST,
                            "max_leverage must be >= 1 when provided",
                        ));
                    }
                    if let (Some(requested), Some(instrument_max)) =
                        (req.max_leverage, instrument.max_leverage)
                    {
                        if requested > instrument_max {
                            return Err(reject_api(
                                StatusCode::BAD_REQUEST,
                                format!(
                                    "max_leverage exceeds instrument max {} for {}",
                                    instrument_max, market_id
                                ),
                            ));
                        }
                    }
                    let request_id = types::generate_op_id("beta-market");
                    let current = beta_controls
                        .market(&market_id)
                        .unwrap_or(BetaMarketControl {
                            market_id: market_id.clone(),
                            max_order_notional: None,
                            max_leverage: None,
                            updated_by: String::new(),
                            recorded_at: Utc::now(),
                        });
                    let next = BetaMarketControl {
                        market_id: market_id.clone(),
                        max_order_notional: req.max_order_notional.or(current.max_order_notional),
                        max_leverage: req.max_leverage.or(current.max_leverage),
                        updated_by: principal.subject.clone(),
                        recorded_at: Utc::now(),
                    };
                    audit("beta_market_upsert", &request_id, &principal);
                    beta_controls
                        .upsert_market(next.clone())
                        .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "request_id": request_id,
                        "market": next,
                    })))
                }
            },
        )
        .boxed();
    let admin_action_audit_for_get = admin_action_audit.clone();
    let ip_rate_limiter_for_admin_audit = ip_rate_limiter.clone();
    let admin_rate_limiter_for_admin_audit = admin_rate_limiter.clone();
    let admin_audit_route = warp::path!("admin" / "audit" / "actions")
        .and(warp::path::end())
        .and(warp::get())
        .and(with_principal())
        .and(optional_query::<AdminActionAuditQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: AdminActionAuditQuery,
                  remote: Option<SocketAddr>| {
                let admin_action_audit = admin_action_audit_for_get.clone();
                let ip_rate_limiter = ip_rate_limiter_for_admin_audit.clone();
                let admin_rate_limiter = admin_rate_limiter_for_admin_audit.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|value| value.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    ip_rate_limiter.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rate_limiter.check(&format!("admin:{}", principal.subject), 10)?;
                    let items = admin_action_audit
                        .list_recent(
                            query.limit.unwrap_or(100).clamp(1, 1000),
                            query.action.as_deref(),
                            query.subject.as_deref(),
                            true,
                        )
                        .map_err(reject_internal_error)?;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "items": items,
                    })))
                }
            },
        )
        .boxed();
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
        .or(user_risk_limits_route)
        .unify()
        .or(user_risk_limits_upsert_route)
        .unify()
        .or(fee_collector_route)
        .unify()
        .or(insurance_funds_route)
        .unify()
        .or(market_treasury_route)
        .unify()
        .or(beta_control_plane_route)
        .unify()
        .or(beta_control_plane_upsert_route)
        .unify()
        .or(beta_users_route)
        .unify()
        .or(beta_user_route)
        .unify()
        .or(beta_user_upsert_route)
        .unify()
        .or(beta_markets_route)
        .unify()
        .or(beta_market_route)
        .unify()
        .or(beta_market_upsert_route)
        .unify()
        .or(admin_audit_route)
        .unify()
        .boxed()
}
