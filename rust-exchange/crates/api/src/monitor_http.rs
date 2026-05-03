//! Order Flow Monitor — REST handlers.
//!
//! Step 3C of `docs/MONITOR_DESIGN.md` §7. Three GET endpoints over the
//! in-memory `OrderTraceProjector`:
//!
//! - `GET /monitor/orders` — list summaries with filter / pagination
//!   (design §5.1)
//! - `GET /monitor/orders/{order_id}` — single summary, no timeline
//!   (design §5.2)
//! - `GET /monitor/orders/{order_id}/timeline` — windowed timeline
//!   (design §5.3)
//!
//! Authorization (design §5):
//! - All endpoints require an authenticated principal (`with_principal`).
//! - Admin sees every order. Non-admin can only see their own:
//!   `list` is force-filtered to `principal.subject`; `get_order` and
//!   `get_timeline` reject with 404 when the principal is not the owner
//!   (404 instead of 403 to avoid leaking the existence of other users'
//!   orders to a non-admin).
//!
//! WebSocket endpoint (`/ws/order-trace`, design §5.4) is Step 10 — not
//! in scope here.

use std::convert::Infallible;
use std::sync::Arc;

use serde::Deserialize;
use types::{
    AuthenticatedPrincipal, BackofficeAction, BackofficeActionVerdict, GrantScope, OrderTraceStage,
    PrincipalRole,
};
use warp::{filters::BoxedFilter, Filter, Rejection};

use crate::admin_authz::AuthzService;
use crate::monitor::{OrderFilter, OrderTraceProjector};

pub(crate) const LIST_LIMIT_DEFAULT: usize = 100;
pub(crate) const LIST_LIMIT_MAX: usize = 500;
pub(crate) const TIMELINE_LIMIT_DEFAULT: usize = 200;
pub(crate) const TIMELINE_LIMIT_MAX: usize = 1000;

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct ListOrdersQuery {
    pub(crate) user_id: Option<String>,
    pub(crate) market_id: Option<String>,
    pub(crate) stage: Option<OrderTraceStage>,
    pub(crate) terminal: Option<bool>,
    pub(crate) since_ms: Option<i64>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct TimelineQuery {
    pub(crate) since_event_id: Option<String>,
    pub(crate) limit: Option<usize>,
}

/// Compose a filter that injects `Arc<OrderTraceProjector>` into a handler.
fn with_projector(
    p: Arc<OrderTraceProjector>,
) -> impl Filter<Extract = (Arc<OrderTraceProjector>,), Error = Infallible> + Clone {
    warp::any().map(move || p.clone())
}

/// Build the three monitor REST routes wrapped under `auth`. The caller
/// supplies the principal-extracting filter (in production that is
/// `crate::security::with_principal()`; in tests, a stub).
///
/// Returns a `BoxedFilter` so the type collapses early — see Group 2c
/// `feat(api): harden order validation and box warp routes` for the
/// motivation (long warp chains otherwise blow up cargo type-check).
/// Build the monitor REST routes. `authz` is optional: when `Some`, an
/// admin principal must additionally satisfy `BackofficeAction::
/// MonitorAccess` per the RBAC matrix (Step 4 integration). When
/// `None`, falls back to the legacy admin-sees-all behaviour for
/// existing test scaffolds and pre-RBAC deployments.
///
/// Customer (`PrincipalRole::User`) access is unaffected: customer
/// principals always see only their own subject's orders, regardless
/// of `authz` presence.
pub(crate) fn build_monitor_routes<F>(
    projector: Arc<OrderTraceProjector>,
    auth: F,
    authz: Option<Arc<AuthzService>>,
) -> BoxedFilter<(warp::reply::Json,)>
where
    F: Filter<Extract = (AuthenticatedPrincipal,), Error = Rejection>
        + Clone
        + Send
        + Sync
        + 'static,
{
    let list = warp::path!("monitor" / "orders")
        .and(warp::get())
        .and(with_projector(projector.clone()))
        .and(auth.clone())
        .and(warp::query::<ListOrdersQuery>())
        .and(with_optional_authz(authz.clone()))
        .and_then(handle_list_orders)
        .boxed();

    let get = warp::path!("monitor" / "orders" / String)
        .and(warp::get())
        .and(with_projector(projector.clone()))
        .and(auth.clone())
        .and(with_optional_authz(authz.clone()))
        .and_then(handle_get_order)
        .boxed();

    let timeline = warp::path!("monitor" / "orders" / String / "timeline")
        .and(warp::get())
        .and(with_projector(projector))
        .and(auth)
        .and(warp::query::<TimelineQuery>())
        .and(with_optional_authz(authz))
        .and_then(handle_get_timeline)
        .boxed();

    timeline.or(get).unify().or(list).unify().boxed()
}

fn with_optional_authz(
    authz: Option<Arc<AuthzService>>,
) -> impl Filter<Extract = (Option<Arc<AuthzService>>,), Error = Infallible> + Clone {
    warp::any().map(move || authz.clone())
}

/// Returns true when an admin principal is permitted to use the
/// monitor admin surface. Without an `authz` service (legacy mode),
/// any admin role grants access. With `authz`, the admin must also
/// satisfy `BackofficeAction::MonitorAccess`. Unknown employees in
/// authz get `Deny`, which falls through to "no access" — operators
/// must be seeded as `Active` employees with a MonitorAccess-bearing
/// role before the monitor admin surface unlocks for them.
fn admin_monitor_access(
    principal: &AuthenticatedPrincipal,
    authz: Option<&Arc<AuthzService>>,
) -> bool {
    if !matches!(principal.role, PrincipalRole::Admin) {
        return false;
    }
    match authz {
        None => true,
        Some(svc) => {
            svc.is_allowed(
                &principal.subject,
                BackofficeAction::MonitorAccess,
                &GrantScope::Global,
            ) != BackofficeActionVerdict::Deny
        }
    }
}

pub(crate) async fn handle_list_orders(
    projector: Arc<OrderTraceProjector>,
    principal: AuthenticatedPrincipal,
    q: ListOrdersQuery,
    authz: Option<Arc<AuthzService>>,
) -> Result<warp::reply::Json, Rejection> {
    // Non-admin: force the user_id filter to the principal regardless of
    // any value the client supplied. Admin: pass through the filter
    // *iff* RBAC allows MonitorAccess (or RBAC is disabled).
    let user_id = match principal.role {
        PrincipalRole::Admin => {
            if !admin_monitor_access(&principal, authz.as_ref()) {
                // RBAC denies: behave as if the admin were a non-employee
                // user — fall back to forcing user_id=subject. The subject
                // is unlikely to own any orders, so the response is
                // effectively empty without leaking the existence of
                // others' orders.
                Some(principal.subject.clone())
            } else {
                q.user_id
            }
        }
        PrincipalRole::User => Some(principal.subject.clone()),
    };
    let updated_since = q
        .since_ms
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis);

    let limit = clamp_limit(q.limit, LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX);
    let filter = OrderFilter {
        user_id,
        market_id: q.market_id,
        stage: q.stage,
        terminal: q.terminal,
        updated_since,
        limit: Some(limit),
    };
    let orders = projector.list_orders(&filter);
    let body = serde_json::json!({
        "orders": orders,
        "total_returned": orders.len(),
    });
    Ok(warp::reply::json(&body))
}

pub(crate) async fn handle_get_order(
    order_id: String,
    projector: Arc<OrderTraceProjector>,
    principal: AuthenticatedPrincipal,
    authz: Option<Arc<AuthzService>>,
) -> Result<warp::reply::Json, Rejection> {
    let summary = projector
        .get_order(&order_id)
        .ok_or_else(warp::reject::not_found)?;
    if !is_visible(&principal, summary.user_id.as_deref(), authz.as_ref()) {
        // Mask existence to non-owner non-admin principals.
        return Err(warp::reject::not_found());
    }
    Ok(warp::reply::json(&summary))
}

pub(crate) async fn handle_get_timeline(
    order_id: String,
    projector: Arc<OrderTraceProjector>,
    principal: AuthenticatedPrincipal,
    q: TimelineQuery,
    authz: Option<Arc<AuthzService>>,
) -> Result<warp::reply::Json, Rejection> {
    let limit = Some(clamp_limit(q.limit, TIMELINE_LIMIT_DEFAULT, TIMELINE_LIMIT_MAX));
    let summary = projector
        .get_order(&order_id)
        .ok_or_else(warp::reject::not_found)?;
    if !is_visible(&principal, summary.user_id.as_deref(), authz.as_ref()) {
        return Err(warp::reject::not_found());
    }
    let page = projector
        .get_timeline(&order_id, q.since_event_id.as_deref(), limit)
        .ok_or_else(warp::reject::not_found)?;
    Ok(warp::reply::json(&page))
}

fn is_visible(
    principal: &AuthenticatedPrincipal,
    owner: Option<&str>,
    authz: Option<&Arc<AuthzService>>,
) -> bool {
    if matches!(principal.role, PrincipalRole::Admin)
        && admin_monitor_access(principal, authz)
    {
        return true;
    }
    owner == Some(principal.subject.as_str())
}

fn clamp_limit(raw: Option<usize>, default: usize, max: usize) -> usize {
    raw.unwrap_or(default).min(max).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::OrderTraceProjector;
    use chrono::TimeZone;
    use types::{OrderTraceEvent, OrderTraceStage, PrincipalRole};
    use warp::http::StatusCode;
    use warp::Reply;

    fn principal(subject: &str, role: PrincipalRole) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            subject: subject.into(),
            role,
            session_id: None,
        }
    }

    fn ev(stage: OrderTraceStage, order_id: &str, user_id: &str, secs: i64) -> OrderTraceEvent {
        let mut e = OrderTraceEvent::new(stage, order_id);
        e.event_id = format!("evt-{order_id}-{:?}-{secs}", stage);
        e.recorded_at = chrono::Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap();
        e.user_id = Some(user_id.into());
        e.market_id = Some("btc-usdt".into());
        e
    }

    fn seed_projector() -> Arc<OrderTraceProjector> {
        let p = OrderTraceProjector::new();
        p.apply_event(ev(OrderTraceStage::SequencerAccepted, "ord-a", "alice", 0));
        p.apply_event(ev(OrderTraceStage::MatchingResting, "ord-a", "alice", 1));
        p.apply_event(ev(OrderTraceStage::SequencerAccepted, "ord-b", "bob", 2));
        p.apply_event(ev(OrderTraceStage::MatchingFilled, "ord-c", "alice", 3));
        p
    }

    /// Convert a `warp::reply::Json` into a `serde_json::Value` by going
    /// through the http response body. `Json` itself is not `Debug` so we
    /// can't use `.unwrap()` on a `Result<Json, Rejection>`; this helper
    /// also avoids that.
    async fn json_body(reply: warp::reply::Json) -> serde_json::Value {
        let bytes = warp::hyper::body::to_bytes(reply.into_response().into_body())
            .await
            .expect("body to_bytes");
        serde_json::from_slice(&bytes).expect("valid json body")
    }

    fn expect_not_found(r: Result<warp::reply::Json, Rejection>) {
        match r {
            Ok(_) => panic!("expected Rejection::NotFound, got Ok"),
            Err(rej) => assert!(rej.is_not_found(), "expected NotFound, got {:?}", rej),
        }
    }

    #[tokio::test]
    async fn list_orders_admin_can_filter_any_user() {
        let p = seed_projector();
        let admin = principal("root", PrincipalRole::Admin);
        let q = ListOrdersQuery {
            user_id: Some("alice".into()),
            ..Default::default()
        };
        let reply = handle_list_orders(p, admin, q, None).await.expect("ok");
        let json = json_body(reply).await;
        let orders = json["orders"].as_array().unwrap();
        assert_eq!(orders.len(), 2, "alice has two orders (ord-a, ord-c)");
        for o in orders {
            assert_eq!(o["user_id"].as_str().unwrap(), "alice");
        }
    }

    #[tokio::test]
    async fn list_orders_admin_no_filter_sees_all() {
        let p = seed_projector();
        let admin = principal("root", PrincipalRole::Admin);
        let reply = handle_list_orders(p, admin, ListOrdersQuery::default(), None)
            .await
            .expect("ok");
        let json = json_body(reply).await;
        assert_eq!(json["orders"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn list_orders_non_admin_query_user_id_is_ignored() {
        let p = seed_projector();
        let alice = principal("alice", PrincipalRole::User);
        let q = ListOrdersQuery {
            user_id: Some("bob".into()), // attempted spoof
            ..Default::default()
        };
        let reply = handle_list_orders(p, alice, q, None).await.expect("ok");
        let json = json_body(reply).await;
        let orders = json["orders"].as_array().unwrap();
        assert_eq!(orders.len(), 2);
        for o in orders {
            assert_eq!(o["user_id"].as_str().unwrap(), "alice");
        }
    }

    #[tokio::test]
    async fn list_orders_filters_by_stage() {
        let p = seed_projector();
        let admin = principal("root", PrincipalRole::Admin);
        let q = ListOrdersQuery {
            stage: Some(OrderTraceStage::MatchingResting),
            ..Default::default()
        };
        let reply = handle_list_orders(p, admin, q, None).await.expect("ok");
        let json = json_body(reply).await;
        let orders = json["orders"].as_array().unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0]["order_id"].as_str().unwrap(), "ord-a");
    }

    #[test]
    fn clamp_limit_obeys_default_max_and_min() {
        assert_eq!(clamp_limit(Some(10_000), LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX), LIST_LIMIT_MAX);
        assert_eq!(clamp_limit(None, LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX), LIST_LIMIT_DEFAULT);
        assert_eq!(clamp_limit(Some(0), LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX), 1);
        assert_eq!(clamp_limit(Some(50), LIST_LIMIT_DEFAULT, LIST_LIMIT_MAX), 50);
    }

    #[tokio::test]
    async fn get_order_owner_can_read() {
        let p = seed_projector();
        let alice = principal("alice", PrincipalRole::User);
        let reply = handle_get_order("ord-a".into(), p, alice, None).await.expect("ok");
        let json = json_body(reply).await;
        assert_eq!(json["order_id"].as_str().unwrap(), "ord-a");
        assert_eq!(json["user_id"].as_str().unwrap(), "alice");
    }

    #[tokio::test]
    async fn get_order_non_owner_sees_404_not_403() {
        let p = seed_projector();
        let bob = principal("bob", PrincipalRole::User);
        expect_not_found(handle_get_order("ord-a".into(), p, bob, None).await);
    }

    #[tokio::test]
    async fn get_order_admin_sees_other_users() {
        let p = seed_projector();
        let admin = principal("root", PrincipalRole::Admin);
        let reply = handle_get_order("ord-b".into(), p, admin, None)
            .await
            .expect("ok");
        let json = json_body(reply).await;
        assert_eq!(json["order_id"].as_str().unwrap(), "ord-b");
        assert_eq!(json["user_id"].as_str().unwrap(), "bob");
    }

    #[tokio::test]
    async fn get_order_unknown_id_404() {
        let p = seed_projector();
        let admin = principal("root", PrincipalRole::Admin);
        expect_not_found(handle_get_order("nope".into(), p, admin, None).await);
    }

    #[tokio::test]
    async fn get_timeline_owner_returns_events() {
        let p = seed_projector();
        let alice = principal("alice", PrincipalRole::User);
        let reply = handle_get_timeline("ord-a".into(), p, alice, TimelineQuery::default(), None)
            .await
            .expect("ok");
        let json = json_body(reply).await;
        assert_eq!(json["order_id"].as_str().unwrap(), "ord-a");
        let timeline = json["timeline"].as_array().unwrap();
        assert_eq!(timeline.len(), 2);
    }

    #[tokio::test]
    async fn get_timeline_non_owner_404() {
        let p = seed_projector();
        let bob = principal("bob", PrincipalRole::User);
        expect_not_found(
            handle_get_timeline("ord-a".into(), p, bob, TimelineQuery::default(), None).await,
        );
    }

    // ── RBAC integration (Step 4) ────────────────────────────────

    fn make_authz_with(
        employee_id: &str,
        role: types::BackofficeRole,
    ) -> Arc<crate::admin_authz::AuthzService> {
        use crate::admin_rbac_store::{AdminEmployeeStore, AdminGrantStore};
        use persistence::InMemoryWal;
        use types::{
            Employee, EmployeeStatus, Grant, GrantScope, GrantStatus, MfaMethod, RoleLevel,
            BACKOFFICE_SCHEMA_VERSION,
        };
        let employees = Arc::new(AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let grants = Arc::new(AdminGrantStore::new(Arc::new(InMemoryWal::new())).unwrap());
        employees
            .create(Employee {
                schema_version: BACKOFFICE_SCHEMA_VERSION,
                employee_id: employee_id.into(),
                display_name: employee_id.into(),
                status: EmployeeStatus::Active,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_mfa_method: Some(MfaMethod::Webauthn),
                last_login_at: Some(chrono::Utc::now()),
            })
            .unwrap();
        grants
            .create(Grant {
                schema_version: BACKOFFICE_SCHEMA_VERSION,
                grant_id: "g-1".into(),
                employee_id: employee_id.into(),
                role,
                level: RoleLevel::Read,
                scope: GrantScope::Global,
                status: GrantStatus::Active,
                granted_by: "secadmin".into(),
                granted_at: chrono::Utc::now(),
                expires_at: chrono::Utc::now() + chrono::Duration::days(30),
                reason: "test grant".into(),
                approval_request_id: None,
            })
            .unwrap();
        Arc::new(crate::admin_authz::AuthzService::new(employees, grants))
    }

    fn make_empty_authz() -> Arc<crate::admin_authz::AuthzService> {
        use crate::admin_rbac_store::{AdminEmployeeStore, AdminGrantStore};
        use persistence::InMemoryWal;
        let employees = Arc::new(AdminEmployeeStore::new(Arc::new(InMemoryWal::new())).unwrap());
        let grants = Arc::new(AdminGrantStore::new(Arc::new(InMemoryWal::new())).unwrap());
        Arc::new(crate::admin_authz::AuthzService::new(employees, grants))
    }

    #[tokio::test]
    async fn list_orders_admin_with_rbac_grant_sees_all() {
        let p = seed_projector();
        let admin = principal("aud", PrincipalRole::Admin);
        let authz = make_authz_with("aud", types::BackofficeRole::AuditorReadonly);
        let reply = handle_list_orders(p, admin, ListOrdersQuery::default(), Some(authz))
            .await
            .expect("ok");
        let json = json_body(reply).await;
        assert_eq!(json["orders"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn list_orders_admin_without_rbac_grant_falls_back_to_self() {
        let p = seed_projector();
        let admin = principal("ghost-admin", PrincipalRole::Admin);
        let authz = make_empty_authz();
        let reply = handle_list_orders(
            p,
            admin,
            ListOrdersQuery {
                user_id: Some("alice".into()), // attempted spoof
                ..Default::default()
            },
            Some(authz),
        )
        .await
        .expect("ok");
        let json = json_body(reply).await;
        // user_id forced to "ghost-admin"; no orders match.
        assert_eq!(json["orders"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn get_order_admin_without_rbac_grant_cannot_see_others() {
        let p = seed_projector();
        let admin = principal("ghost-admin", PrincipalRole::Admin);
        let authz = make_empty_authz();
        // ord-a is alice's; ghost-admin is not alice and lacks
        // MonitorAccess.
        expect_not_found(handle_get_order("ord-a".into(), p, admin, Some(authz)).await);
    }

    #[tokio::test]
    async fn build_monitor_routes_compiles_and_serves() {
        // Smoke test through warp::test: a stub auth filter that lifts
        // Infallible to Rejection by going through `and_then`.
        let p = seed_projector();
        let auth = warp::any().and_then(|| async {
            Ok::<AuthenticatedPrincipal, Rejection>(AuthenticatedPrincipal {
                subject: "root".into(),
                role: PrincipalRole::Admin,
                session_id: None,
            })
        });
        let routes = build_monitor_routes(p, auth, None);

        let resp = warp::test::request()
            .method("GET")
            .path("/monitor/orders")
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["orders"].as_array().unwrap().len(), 3);

        let resp = warp::test::request()
            .method("GET")
            .path("/monitor/orders/ord-a")
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["order_id"].as_str().unwrap(), "ord-a");

        let resp = warp::test::request()
            .method("GET")
            .path("/monitor/orders/ord-a/timeline")
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(body["timeline"].as_array().unwrap().len(), 2);
    }
}
