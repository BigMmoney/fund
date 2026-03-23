//! OpenAPI 3.0 specification and embedded Swagger UI.
//!
//! Routes:
//!   GET /openapi.json  — OpenAPI 3.0 JSON specification
//!   GET /swagger-ui    — Single-page Swagger UI (CDN-hosted assets)

use warp::Filter;

/// Build routes for OpenAPI spec and Swagger UI.
pub fn build_openapi_routes(
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let spec_route = warp::path("openapi.json").and(warp::get()).map(|| {
        warp::reply::with_header(
            warp::reply::json(&spec()),
            "Access-Control-Allow-Origin",
            "*",
        )
    });

    let swagger_route = warp::path("swagger-ui")
        .and(warp::get())
        .map(|| warp::reply::html(SWAGGER_HTML));

    spec_route.or(swagger_route)
}

fn spec() -> serde_json::Value {
    serde_json::json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Rust Exchange API",
            "version": "0.1.0",
            "description": "Prediction market / derivatives exchange — REST & WebSocket API"
        },
        "servers": [
            { "url": "http://localhost:3030", "description": "Local dev" }
        ],
        "tags": [
            { "name": "Trading", "description": "Order management" },
            { "name": "Accounts", "description": "Balances, positions, P&L" },
            { "name": "Markets", "description": "Orderbook, trades, stats" },
            { "name": "Admin", "description": "Instruments, funding, risk" },
            { "name": "Governance", "description": "ADL, liquidation policy, dual-auth" },
            { "name": "Pricing", "description": "Index & fair price management" },
            { "name": "System", "description": "Health, readiness, metrics" },
            { "name": "WebSocket", "description": "Real-time feeds" }
        ],
        "paths": {
            // ── Trading ──────────────────────────────────
            "/intent": {
                "post": route("Trading", "Submit intent", "Submit prediction market limit order with optional outcome",
                    req_body("IntentRequest"), resp_json("IntentResponse"))
            },
            "/submit-order": {
                "post": route("Trading", "Submit order", "Full order submission with type, leverage, GTD support",
                    req_body("OrderRequest"), resp_json("OrderResponse"))
            },
            "/cancel-order": {
                "post": route("Trading", "Cancel order", "Cancel a specific order by ID",
                    req_body("CancelRequest"), resp_json("CancelResponse"))
            },
            "/replace-order": {
                "post": route("Trading", "Replace order", "Cancel and re-submit order atomically",
                    req_body("ReplaceOrderRequest"), resp_json("OrderResponse"))
            },
            "/mass-cancel/user": {
                "post": route("Trading", "Mass cancel user", "Cancel all orders for authenticated user",
                    req_body("MassCancelUserRequest"), resp_json("MassCancelResponse"))
            },
            "/mass-cancel/session": {
                "post": route("Trading", "Mass cancel session", "Cancel all orders for session",
                    req_body("MassCancelSessionRequest"), resp_json("MassCancelResponse"))
            },
            // ── Accounts ─────────────────────────────────
            "/balances/{user_id}": {
                "get": route_get("Accounts", "Get balances", "Cash balance (available + hold)", path_param("user_id"))
            },
            "/positions/{user_id}": {
                "get": route_get("Accounts", "Get positions", "All positions across markets", path_param("user_id"))
            },
            "/margin/{user_id}": {
                "get": route_get("Accounts", "Get margin", "Margin projection with mark price", path_param("user_id"))
            },
            "/pnl/{user_id}": {
                "get": route_get("Accounts", "Get PnL", "Unrealized P&L for positions", path_param("user_id"))
            },
            "/orders/{user_id}": {
                "get": route_get("Accounts", "Get orders", "All open orders with filtering", path_param("user_id"))
            },
            "/fills/{user_id}": {
                "get": route_get("Accounts", "Get fills", "Trade fills/settlement history (max 500)", path_param("user_id"))
            },
            "/deposits/{user_id}": {
                "get": route_get("Accounts", "Get deposits", "On-chain deposit records", path_param("user_id"))
            },
            "/position-costs/{user_id}": {
                "get": route_get("Accounts", "Get position costs", "Average entry prices", path_param("user_id"))
            },
            // ── Markets ──────────────────────────────────
            "/markets": {
                "get": route_get_simple("Markets", "List markets", "All markets with aggregated state")
            },
            "/markets/{market_id}": {
                "get": route_get("Markets", "Get market detail", "Single market detail", path_param("market_id"))
            },
            "/markets/{market_id}/book": {
                "get": route_get("Markets", "Get orderbook", "L1/L2 orderbook snapshot with ?depth=N (default 20, max 200)", path_param("market_id"))
            },
            "/trades": {
                "get": route_get_simple("Markets", "Get trades", "Recent trades (filterable by market/outcome/user)")
            },
            "/markets/{market_id}/history": {
                "get": route_get("Markets", "Get OHLCV", "1-hour candle data", path_param("market_id"))
            },
            "/stats": {
                "get": route_get_simple("Markets", "Platform stats", "Volume, trades, users, liquidity")
            },
            "/matching-status": {
                "get": route_get_simple("Markets", "Matching status", "Partition queue depths + kill-switch")
            },
            "/rules": {
                "get": route_get_simple("Markets", "Trading rules", "Standardised trading rules, order types, risk params, and per-market constraints")
            },
            "/markets/{market_id}/microstructure": {
                "get": route_get("Markets", "Microstructure", "Tick/lot size, fees, price bands, depth statistics, matching model", path_param("market_id"))
            },
            // ── Admin: Control ───────────────────────────
            "/deposit": {
                "post": route("Admin", "Deposit", "Deposit USDC for user (admin-only)",
                    req_body("DepositRequest"), resp_json("DepositResponse"))
            },
            "/mass-cancel/market": {
                "post": route("Admin", "Mass cancel market", "Cancel all orders in a market",
                    req_body("MassCancelMarketRequest"), resp_json("MassCancelResponse"))
            },
            "/admin/kill-switch": {
                "post": route("Admin", "Kill switch", "Toggle emergency kill switch (pending approval)",
                    req_body("KillSwitchRequest"), resp_json("GovernanceActionResponse"))
            },
            "/admin/market-state": {
                "post": route("Admin", "Set market state", "Normal/Stress/AuctionCall/CancelOnly/Halted",
                    req_body("MarketStateRequest"), resp_json("GovernanceActionResponse"))
            },
            "/admin/reference-price": {
                "post": route("Admin", "Set reference price", "Manually set reference price for market",
                    req_body("ReferencePriceRequest"), resp_json("GovernanceActionResponse"))
            },
            // ── Admin: Instruments & Funding ─────────────
            "/admin/instruments": {
                "get": route_get_simple("Admin", "List instruments", "All configured instrument specs"),
                "post": route("Admin", "Upsert instrument", "Create or update instrument spec",
                    req_body("InstrumentSpec"), resp_json("InstrumentSpec"))
            },
            "/admin/risk/funding-rates": {
                "get": route_get_simple("Admin", "Get funding rates", "All funding rates (filterable)"),
                "post": route("Admin", "Set funding rate", "Set funding rate for market/outcome",
                    req_body("FundingRateRequest"), resp_json("FundingRateResponse"))
            },
            "/admin/risk/funding/settle": {
                "post": route("Admin", "Settle funding", "Settle funding manually between users",
                    req_body("SettleFundingRequest"), resp_json("SettleFundingResponse"))
            },
            "/admin/risk/events": {
                "get": route_get_simple("Admin", "Risk events", "Recent risk automation audit events")
            },
            // ── Pricing ──────────────────────────────────
            "/admin/risk/pricing/index": {
                "post": route("Pricing", "Upsert index price", "Set index price (pending approval)",
                    req_body("IndexPriceRequest"), resp_json("GovernanceActionResponse"))
            },
            "/admin/risk/pricing/sources": {
                "get": route_get_simple("Pricing", "Get price sources", "Index price policies"),
                "post": route("Pricing", "Update price source", "Update index source policy (pending approval)",
                    req_body("IndexSourcePolicyRequest"), resp_json("GovernanceActionResponse"))
            },
            "/admin/risk/pricing/fair": {
                "get": route_get_simple("Pricing", "Get fair price", "Fair price calculation with detail")
            },
            // ── Governance ───────────────────────────────
            "/admin/risk/adl/governance": {
                "get": route_get_simple("Governance", "Get ADL governance", "ADL ranking parameters"),
                "post": route("Governance", "Update ADL governance", "Update ADL params (pending approval)",
                    req_body("AdlGovernanceRequest"), resp_json("GovernanceActionResponse"))
            },
            "/admin/risk/liquidations/policy": {
                "get": route_get_simple("Governance", "Get liquidation policy", "Auction policy settings"),
                "post": route("Governance", "Update liquidation policy", "Update policy (pending approval)",
                    req_body("LiquidationPolicyRequest"), resp_json("GovernanceActionResponse"))
            },
            "/admin/risk/governance/actions": {
                "get": route_get_simple("Governance", "List governance actions", "Pending governance actions")
            },
            "/admin/risk/governance/actions/{action_id}/approve": {
                "post": route_get("Governance", "Approve action", "Approve governance action (dual-auth)", path_param("action_id"))
            },
            "/admin/risk/governance/actions/{action_id}/reject": {
                "post": route_get("Governance", "Reject action", "Reject governance action", path_param("action_id"))
            },
            // ── System ───────────────────────────────────
            "/health": {
                "get": route_get_simple("System", "Health check", "Uptime, account count, kill-switch status")
            },
            "/ready": {
                "get": route_get_simple("System", "Readiness", "Balance invariant verification")
            },
            "/health/partitions": {
                "get": route_get_simple("System", "Partition health", "Per-partition queue depth & utilization")
            },
            "/metrics": {
                "get": route_get_simple("System", "Metrics (JSON)", "Custom JSON metrics snapshot")
            },
            "/metrics/prometheus": {
                "get": route_get_simple("System", "Metrics (Prometheus)", "Prometheus text format metrics")
            },
            "/version": {
                "get": route_get_simple("System", "Version", "Build name, version, and build date")
            },
            // ── WebSocket ────────────────────────────────
            "/ws/trades/{market_id}": {
                "get": route_get("WebSocket", "Trade stream", "Real-time trade feed via WebSocket", path_param("market_id"))
            },
            "/ws/orderbook/{market_id}": {
                "get": route_get("WebSocket", "Orderbook stream", "Periodic orderbook snapshots via WebSocket (default 200ms)", path_param("market_id"))
            },
            // ── Liquidation History ──────────────────────
            "/liquidations/{user_id}": {
                "get": route_get("Accounts", "Liquidation history", "User's liquidation history (own records only, or admin)", path_param("user_id"))
            },
            // ── Funding Rate ─────────────────────────────
            "/markets/{market_id}/funding-rate": {
                "get": route_get("Markets", "Funding rate", "Current and predicted funding rate, countdown to next settlement", path_param("market_id"))
            },
            // ── Fee Tiers ────────────────────────────────
            "/admin/fee-tiers": {
                "get": route_get_simple("Admin", "Get fee tiers", "Current global fee tier schedule"),
                "put": route("Admin", "Update fee tiers", "Replace global fee tier schedule",
                    req_body("FeeTierSchedule"), resp_json("FeeTierSchedule"))
            },
            "/admin/fee-tier/{user_id}": {
                "get": route_get("Admin", "Get user fee tier", "Resolved fee tier for user based on 30-day volume", path_param("user_id"))
            }
        },
        "components": {
            "securitySchemes": {
                "InternalAuth": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "x-internal-auth-signature",
                    "description": "HMAC-SHA256 internal service authentication"
                }
            },
            "schemas": {
                "OrderRequest": {
                    "type": "object",
                    "required": ["market_id", "user_id", "side", "amount", "order_type", "time_in_force"],
                    "properties": {
                        "market_id": { "type": "string", "example": "BTC-USDC-PERP" },
                        "user_id": { "type": "string" },
                        "side": { "type": "string", "enum": ["buy", "sell"] },
                        "amount": { "type": "integer", "format": "int64", "description": "Quantity in lot units" },
                        "price": { "type": "integer", "format": "int64", "description": "Limit price in tick units (required for limit orders)" },
                        "order_type": { "type": "string", "enum": ["limit", "market", "stop_market", "stop_limit", "take_profit_market", "take_profit_limit"] },
                        "time_in_force": { "type": "string", "enum": ["gtc", "ioc", "fok", "gtd"] },
                        "client_order_id": { "type": "string", "description": "Unique client-assigned order ID" },
                        "post_only": { "type": "boolean", "default": false },
                        "reduce_only": { "type": "boolean", "default": false },
                        "display_qty": { "type": "integer", "format": "int64", "description": "Iceberg visible tranche size; 0 or omit for fully visible" },
                        "trigger_price": { "type": "integer", "format": "int64", "description": "Trigger price for conditional orders" },
                        "trigger_type": { "type": "string", "enum": ["last_price", "mark_price", "index_price"], "default": "last_price" },
                        "stp_mode": { "type": "string", "enum": ["cancel_taker", "cancel_maker", "cancel_both"], "default": "cancel_taker" },
                        "stp_group_id": { "type": "string", "description": "STP group identifier for cross-account self-trade prevention" },
                        "leverage": { "type": "integer", "format": "int32" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "expires_at": { "type": "integer", "format": "int64", "description": "Unix epoch seconds (GTD orders only)" },
                        "min_fill_qty": { "type": "integer", "format": "int64", "description": "Minimum first-fill quantity" }
                    }
                },
                "OrderResponse": {
                    "type": "object",
                    "properties": {
                        "order_id": { "type": "string" },
                        "status": { "type": "string", "enum": ["accepted", "rejected", "filled", "partially_filled"] },
                        "fills": { "type": "array", "items": { "$ref": "#/components/schemas/Fill" } },
                        "error": { "$ref": "#/components/schemas/ApiError" }
                    }
                },
                "Fill": {
                    "type": "object",
                    "properties": {
                        "price": { "type": "integer", "format": "int64" },
                        "quantity": { "type": "integer", "format": "int64" },
                        "side": { "type": "string", "enum": ["buy", "sell"] },
                        "fee": { "type": "integer", "format": "int64" },
                        "timestamp": { "type": "integer", "format": "int64" }
                    }
                },
                "CancelRequest": {
                    "type": "object",
                    "required": ["market_id", "order_id", "user_id"],
                    "properties": {
                        "market_id": { "type": "string" },
                        "order_id": { "type": "string" },
                        "user_id": { "type": "string" }
                    }
                },
                "CancelResponse": {
                    "type": "object",
                    "properties": {
                        "cancelled_order_ids": { "type": "array", "items": { "type": "string" } },
                        "error": { "$ref": "#/components/schemas/ApiError" }
                    }
                },
                "ReplaceOrderRequest": {
                    "type": "object",
                    "required": ["market_id", "user_id", "existing_order_id"],
                    "properties": {
                        "market_id": { "type": "string" },
                        "user_id": { "type": "string" },
                        "existing_order_id": { "type": "string" },
                        "new_price": { "type": "integer", "format": "int64" },
                        "new_amount": { "type": "integer", "format": "int64" },
                        "new_display_qty": { "type": "integer", "format": "int64" }
                    }
                },
                "MassCancelResponse": {
                    "type": "object",
                    "properties": {
                        "cancelled_count": { "type": "integer" },
                        "cancelled_order_ids": { "type": "array", "items": { "type": "string" } }
                    }
                },
                "DepositRequest": {
                    "type": "object",
                    "required": ["user_id", "amount"],
                    "properties": {
                        "user_id": { "type": "string" },
                        "amount": { "type": "integer", "format": "int64", "description": "Amount in smallest currency unit" }
                    }
                },
                "ApiError": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "string", "description": "Machine-readable error code (SCREAMING_SNAKE_CASE)",
                            "enum": [
                                "INVALID_ORDER", "DUPLICATE_ORDER_ID", "ORDER_NOT_FOUND",
                                "MARKET_CLOSED", "QUEUE_FULL", "KILL_SWITCH_ACTIVE",
                                "PRICE_BAND_BREACHED", "INSUFFICIENT_LIQUIDITY",
                                "SELF_TRADE_PREVENTED", "LEDGER_ERROR", "PERSISTENCE_ERROR",
                                "RATE_LIMITED", "UNAUTHORIZED", "TICK_SIZE_VIOLATION",
                                "LOT_SIZE_VIOLATION", "BELOW_MIN_AMOUNT", "EXCEEDS_MAX_NOTIONAL",
                                "ACCOUNT_FROZEN", "INVALID_STATE_TRANSITION", "INTERNAL_ERROR",
                                "FAT_FINGER_REJECTED", "MARKET_KILL_SWITCH_ACTIVE",
                                "CIRCUIT_BREAKER_TRIGGERED", "MARKET_MAKER_PROTECTION_TRIGGERED",
                                "INSUFFICIENT_MARGIN", "EXCEEDS_MAX_LEVERAGE",
                                "EXCEEDS_POSITION_LIMIT", "REDUCE_ONLY_VIOLATION",
                                "POST_ONLY_WOULD_TRADE", "INVALID_TIME_IN_FORCE",
                                "INVALID_TRIGGER_PRICE", "ORDER_EXPIRED", "MARKET_NOT_FOUND",
                                "INSTRUMENT_HALTED", "INSTRUMENT_DELISTED", "INVALID_AMENDMENT",
                                "SESSION_EXPIRED", "IP_RATE_LIMITED", "MAINTENANCE_MODE",
                                "INVALID_ACCOUNT_MODE", "COLLATERAL_INELIGIBLE",
                                "LIQUIDATION_IN_PROGRESS"
                            ]
                        },
                        "message": { "type": "string", "description": "Human-readable error description" }
                    }
                },
                "MarginSnapshot": {
                    "type": "object",
                    "properties": {
                        "user_id": { "type": "string" },
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "collateral_total": { "type": "integer", "format": "int64" },
                        "position_qty": { "type": "integer", "format": "int64" },
                        "mark_price": { "type": "integer", "format": "int64" },
                        "notional": { "type": "integer", "format": "int64" },
                        "initial_margin_required": { "type": "integer", "format": "int64" },
                        "maintenance_margin_required": { "type": "integer", "format": "int64" },
                        "margin_ratio_bps": { "type": "integer", "format": "int64", "nullable": true },
                        "liquidation_required": { "type": "boolean" }
                    }
                },
                "FundingRateResponse": {
                    "type": "object",
                    "properties": {
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "mark_price": { "type": "integer", "format": "int64" },
                        "index_price": { "type": "integer", "format": "int64" },
                        "premium_bps": { "type": "integer", "format": "int64" },
                        "clamped_premium_bps": { "type": "integer", "format": "int64" },
                        "interest_bps": { "type": "integer", "format": "int64" },
                        "funding_rate_ppm": { "type": "integer", "format": "int64" },
                        "predicted_funding_rate_ppm": { "type": "integer", "format": "int64" },
                        "funding_interval_secs": { "type": "integer", "format": "int64" },
                        "next_funding_at": { "type": "integer", "format": "int64" },
                        "seconds_until_funding": { "type": "integer", "format": "int64" },
                        "degraded_mode": { "type": "boolean" },
                        "timestamp": { "type": "integer", "format": "int64" }
                    }
                },
                "FeeTier": {
                    "type": "object",
                    "properties": {
                        "tier_name": { "type": "string", "example": "VIP0" },
                        "min_volume_30d": { "type": "integer", "format": "int64" },
                        "maker_fee_bps": { "type": "integer", "format": "int32" },
                        "taker_fee_bps": { "type": "integer", "format": "int32" }
                    }
                },
                "InstrumentSpec": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "kind": { "type": "string", "enum": ["spot", "perpetual", "future", "option", "prediction"] },
                        "base_currency": { "type": "string" },
                        "quote_currency": { "type": "string" },
                        "tick_size": { "type": "integer", "format": "int64" },
                        "lot_size": { "type": "integer", "format": "int64" },
                        "min_amount": { "type": "integer", "format": "int64" },
                        "max_notional": { "type": "integer", "format": "int64" },
                        "max_leverage": { "type": "integer", "format": "int32" },
                        "maintenance_margin_bps": { "type": "integer", "format": "int64" },
                        "margin_mode": { "type": "string", "enum": ["cross", "isolated"], "nullable": true }
                    }
                },
                "IntentRequest": {
                    "type": "object",
                    "required": ["market_id", "side", "price", "amount", "outcome"],
                    "properties": {
                        "request_id": { "type": "string" },
                        "client_order_id": { "type": "string" },
                        "market_id": { "type": "string", "example": "BTC-USDC-PERP" },
                        "side": { "type": "string", "enum": ["buy", "sell"] },
                        "price": { "type": "integer", "format": "int64" },
                        "amount": { "type": "integer", "format": "int64" },
                        "outcome": { "type": "integer", "format": "int32" }
                    }
                },
                "IntentResponse": {
                    "type": "object",
                    "properties": {
                        "status": { "type": "string", "example": "ok" },
                        "order_id": { "type": "string" },
                        "request_id": { "type": "string" },
                        "command_seq": { "type": "integer", "format": "int64" },
                        "lifecycle": { "type": "string" },
                        "market_state": { "type": "string" },
                        "order_state": { "type": "string" },
                        "remaining_amount": { "type": "integer", "format": "int64" },
                        "fills": { "type": "integer", "format": "int32" }
                    }
                },
                "MassCancelUserRequest": {
                    "type": "object",
                    "properties": {
                        "request_id": { "type": "string" }
                    }
                },
                "MassCancelSessionRequest": {
                    "type": "object",
                    "required": ["session_id"],
                    "properties": {
                        "request_id": { "type": "string" },
                        "session_id": { "type": "string" }
                    }
                },
                "MassCancelMarketRequest": {
                    "type": "object",
                    "required": ["market_id"],
                    "properties": {
                        "request_id": { "type": "string" },
                        "market_id": { "type": "string" }
                    }
                },
                "DepositResponse": {
                    "type": "object",
                    "properties": {
                        "status": { "type": "string", "enum": ["ok", "error"] },
                        "error": { "type": "string", "nullable": true }
                    }
                },
                "KillSwitchRequest": {
                    "type": "object",
                    "required": ["enabled"],
                    "properties": {
                        "request_id": { "type": "string" },
                        "enabled": { "type": "boolean" }
                    }
                },
                "MarketStateRequest": {
                    "type": "object",
                    "required": ["market_id", "state"],
                    "properties": {
                        "request_id": { "type": "string" },
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "state": { "type": "string", "enum": ["pre_open", "normal", "stress", "auction_call", "cancel_only", "halted", "maintenance", "closed"] }
                    }
                },
                "ReferencePriceRequest": {
                    "type": "object",
                    "required": ["market_id", "outcome", "reference_price"],
                    "properties": {
                        "request_id": { "type": "string" },
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "source": { "type": "string" },
                        "reference_price": { "type": "integer", "format": "int64" }
                    }
                },
                "GovernanceActionResponse": {
                    "type": "object",
                    "properties": {
                        "status": { "type": "string", "enum": ["pending", "ok"] },
                        "approval": { "$ref": "#/components/schemas/GovernanceActionRecord" },
                        "action": { "type": "object" },
                        "result": { "type": "object" }
                    }
                },
                "GovernanceActionRecord": {
                    "type": "object",
                    "properties": {
                        "action_id": { "type": "string" },
                        "action_type": { "type": "string" },
                        "payload": { "type": "object" },
                        "requested_by": { "type": "string" },
                        "required_approvals": { "type": "integer", "format": "int32", "default": 2 },
                        "approvers": { "type": "array", "items": { "type": "string" } },
                        "approved_by": { "type": "string", "nullable": true },
                        "rejected_by": { "type": "string", "nullable": true },
                        "status": { "type": "string", "enum": ["pending", "applied", "rejected", "apply_failed"] },
                        "comment": { "type": "string", "nullable": true },
                        "recorded_at": { "type": "string", "format": "date-time" },
                        "decided_at": { "type": "string", "format": "date-time", "nullable": true }
                    }
                },
                "FundingRateRequest": {
                    "type": "object",
                    "required": ["market_id", "funding_rate_ppm"],
                    "properties": {
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "funding_rate_ppm": { "type": "integer", "format": "int64" }
                    }
                },
                "SettleFundingRequest": {
                    "type": "object",
                    "required": ["long_user_id", "short_user_id", "market_id", "mark_price", "funding_rate_ppm"],
                    "properties": {
                        "request_id": { "type": "string" },
                        "long_user_id": { "type": "string" },
                        "short_user_id": { "type": "string" },
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32", "default": 0 },
                        "mark_price": { "type": "integer", "format": "int64" },
                        "funding_rate_ppm": { "type": "integer", "format": "int64" }
                    }
                },
                "SettleFundingResponse": {
                    "type": "object",
                    "properties": {
                        "status": { "type": "string" },
                        "request_id": { "type": "string" },
                        "settlement": { "type": "object" }
                    }
                },
                "IndexPriceRequest": {
                    "type": "object",
                    "required": ["market_id", "index_price"],
                    "properties": {
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "index_price": { "type": "integer", "format": "int64" },
                        "source": { "type": "string" }
                    }
                },
                "IndexSourcePolicyRequest": {
                    "type": "object",
                    "required": ["market_id", "source", "status"],
                    "properties": {
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "source": { "type": "string" },
                        "status": { "type": "string", "enum": ["active", "degraded", "quarantined"] },
                        "weight_bps": { "type": "integer", "format": "int64", "default": 10000 }
                    }
                },
                "AdlGovernanceRequest": {
                    "type": "object",
                    "properties": {
                        "maintenance_margin_bps": { "type": "integer", "format": "int64" },
                        "leverage_weight_bps": { "type": "integer", "format": "int64" },
                        "bankruptcy_distance_weight_bps": { "type": "integer", "format": "int64" },
                        "size_weight_bps": { "type": "integer", "format": "int64" },
                        "buffer_weight_bps": { "type": "integer", "format": "int64" },
                        "max_candidates": { "type": "integer", "format": "int32" },
                        "max_socialized_loss_share_bps_per_candidate": { "type": "integer", "format": "int64" }
                    },
                    "description": "All fields optional — partial update of ADL governance parameters"
                },
                "LiquidationPolicyRequest": {
                    "type": "object",
                    "properties": {
                        "auction_window_secs": { "type": "integer", "format": "int64" },
                        "retry_backoff_secs": { "type": "array", "items": { "type": "integer", "format": "int64" } },
                        "max_retry_tiers": { "type": "integer", "format": "int32" },
                        "max_auction_rounds": { "type": "integer", "format": "int32" },
                        "auction_reserve_step_bps": { "type": "integer", "format": "int64" }
                    },
                    "description": "All fields optional — partial update of liquidation auction policy"
                },
                "FeeTierSchedule": {
                    "type": "object",
                    "properties": {
                        "tiers": { "type": "array", "items": { "$ref": "#/components/schemas/FeeTier" } }
                    }
                },
                "LiquidationRecord": {
                    "type": "object",
                    "properties": {
                        "queue_id": { "type": "string" },
                        "market_id": { "type": "string" },
                        "outcome": { "type": "integer", "format": "int32" },
                        "status": { "type": "string", "enum": ["queued", "in_auction", "filled", "failed", "cancelled"] },
                        "strategy": { "type": "string" },
                        "mark_price": { "type": "integer", "format": "int64" },
                        "position_qty": { "type": "integer", "format": "int64" },
                        "remaining_qty": { "type": "integer", "format": "int64" },
                        "filled_qty": { "type": "integer", "format": "int64" },
                        "margin_ratio_bps": { "type": "integer", "format": "int64", "nullable": true },
                        "auction_round": { "type": "integer", "format": "int32" },
                        "retry_tier": { "type": "integer", "format": "int32" },
                        "recorded_at": { "type": "string", "format": "date-time" }
                    }
                }
            },
            "headers": {
                "X-RateLimit-Limit": {
                    "description": "Maximum requests allowed in the current window",
                    "schema": { "type": "integer" }
                },
                "X-RateLimit-Remaining": {
                    "description": "Remaining requests in the current window",
                    "schema": { "type": "integer" }
                },
                "X-RateLimit-Reset": {
                    "description": "Unix timestamp when the rate limit window resets",
                    "schema": { "type": "integer" }
                },
                "Retry-After": {
                    "description": "Seconds until the rate limit window resets (429 responses only)",
                    "schema": { "type": "integer" }
                }
            }
        },
        "security": [{ "InternalAuth": [] }]
    })
}

// ── JSON builder helpers ─────────────────────────────────────

fn route(
    tag: &str,
    summary: &str,
    desc: &str,
    request_body: serde_json::Value,
    response: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "tags": [tag],
        "summary": summary,
        "description": desc,
        "requestBody": request_body,
        "responses": { "200": response, "400": error_resp(), "401": unauth_resp(), "429": rate_limit_resp() }
    })
}

fn route_get(tag: &str, summary: &str, desc: &str, params: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "tags": [tag],
        "summary": summary,
        "description": desc,
        "parameters": [params],
        "responses": { "200": { "description": "Success" }, "401": unauth_resp(), "429": rate_limit_resp() }
    })
}

fn route_get_simple(tag: &str, summary: &str, desc: &str) -> serde_json::Value {
    serde_json::json!({
        "tags": [tag],
        "summary": summary,
        "description": desc,
        "responses": { "200": { "description": "Success" }, "401": unauth_resp(), "429": rate_limit_resp() }
    })
}

fn req_body(schema_name: &str) -> serde_json::Value {
    serde_json::json!({
        "required": true,
        "content": { "application/json": { "schema": { "$ref": format!("#/components/schemas/{schema_name}") } } }
    })
}

fn resp_json(schema_name: &str) -> serde_json::Value {
    serde_json::json!({
        "description": "Success",
        "content": { "application/json": { "schema": { "$ref": format!("#/components/schemas/{schema_name}") } } }
    })
}

fn path_param(name: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "in": "path", "required": true, "schema": { "type": "string" } })
}

fn error_resp() -> serde_json::Value {
    serde_json::json!({
        "description": "Bad request",
        "content": {
            "application/json": {
                "schema": { "$ref": "#/components/schemas/ApiError" }
            }
        }
    })
}

fn unauth_resp() -> serde_json::Value {
    serde_json::json!({
        "description": "Unauthorized",
        "content": {
            "application/json": {
                "schema": { "$ref": "#/components/schemas/ApiError" }
            }
        }
    })
}

fn rate_limit_resp() -> serde_json::Value {
    serde_json::json!({
        "description": "Rate limited",
        "headers": {
            "X-RateLimit-Limit": { "$ref": "#/components/headers/X-RateLimit-Limit" },
            "X-RateLimit-Remaining": { "$ref": "#/components/headers/X-RateLimit-Remaining" },
            "X-RateLimit-Reset": { "$ref": "#/components/headers/X-RateLimit-Reset" },
            "Retry-After": { "$ref": "#/components/headers/Retry-After" }
        },
        "content": {
            "application/json": {
                "schema": { "$ref": "#/components/schemas/ApiError" }
            }
        }
    })
}

/// Embedded Swagger UI HTML — loads from CDN, points to /openapi.json.
const SWAGGER_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Rust Exchange — API Docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    SwaggerUIBundle({ url: '/openapi.json', dom_id: '#swagger-ui', deepLinking: true });
  </script>
</body>
</html>"#;
