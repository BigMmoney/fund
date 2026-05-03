//! Integration tests for the order flow monitor.
//!
//! Unit tests in `monitor.rs`, `monitor_http.rs`, and `monitor_jsonl.rs`
//! cover individual components in isolation. This module wires them
//! together (producer → eventbus → consumer task → projector → REST
//! handler) and asserts the full chain behaves correctly end-to-end.
//!
//! These tests build the same plumbing that `main.rs::spawn_monitor_consumer`
//! constructs at startup, minus the JSONL writer (which has its own
//! dedicated test surface in `monitor_jsonl.rs`).

#![cfg(test)]

use std::sync::Arc;
use std::time::Duration;

use eventbus::EventBus;
use types::{
    AuthenticatedPrincipal, Event, OrderTraceEvent, OrderTraceStage, PrincipalRole, TraceEmitter,
};
use warp::Filter;

use crate::monitor::{OrderTraceProjector, EventBusTraceEmitter};
use crate::monitor_http;

/// Spawn a minimal consumer task that mirrors the production
/// `spawn_monitor_consumer` in `main.rs` but skips the JSONL writer.
/// Returns once the task has subscribed to the channel so callers can
/// publish without racing the subscription.
async fn spawn_test_consumer(
    event_bus: EventBus,
    projector: Arc<OrderTraceProjector>,
) {
    // Pre-create the channel so the first publish can't outrun the
    // task's subscribe.
    let _initial_rx = event_bus.subscribe("order.trace");
    let projector_clone = projector.clone();
    let bus_clone = event_bus.clone();
    tokio::spawn(async move {
        let mut rx = bus_clone.subscribe("order.trace");
        // Drop the bootstrap receiver only after the task has its own.
        drop(_initial_rx);
        loop {
            match rx.recv().await {
                Ok(Event::OrderTrace(ev)) => projector_clone.apply_event(ev),
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    // Let tokio actually schedule the task before the test continues.
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(20)).await;
}

/// Poll `predicate` every 10 ms up to `timeout_ms` total. Returns the
/// final value of the predicate.
async fn wait_until<F>(mut predicate: F, timeout_ms: u64) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    while tokio::time::Instant::now() < deadline {
        if predicate() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    predicate()
}

fn make_event(
    stage: OrderTraceStage,
    order_id: Option<&str>,
    request_id: &str,
    user_id: &str,
) -> OrderTraceEvent {
    let mut ev = match order_id {
        Some(id) => OrderTraceEvent::new(stage, id),
        None => OrderTraceEvent::new_unbound(stage),
    };
    ev.request_id = Some(request_id.into());
    ev.user_id = Some(user_id.into());
    ev.market_id = Some("btc-usdt".into());
    ev
}

#[tokio::test]
async fn end_to_end_pipeline_pre_sequencer_to_matching_filled() {
    let event_bus = EventBus::new();
    let projector = OrderTraceProjector::new();
    spawn_test_consumer(event_bus.clone(), projector.clone()).await;

    // Producer side: publish via the same EventBusTraceEmitter that
    // sequencer/matching/projection/api use in production.
    let emitter: Arc<dyn TraceEmitter> =
        Arc::new(EventBusTraceEmitter::new(event_bus.clone()));

    // 1. Pre-sequencer api_received (no order_id yet).
    emitter.emit(make_event(
        OrderTraceStage::ApiReceived,
        None,
        "req-pipeline-1",
        "alice",
    ));
    // 2. sequencer_accepted (still no order_id for new orders).
    emitter.emit(make_event(
        OrderTraceStage::SequencerAccepted,
        None,
        "req-pipeline-1",
        "alice",
    ));
    // 3. matching_resting — assigns the canonical order_id, flushes
    //    the trace_key bucket. This is the binding moment per design §3.3.1.
    emitter.emit(make_event(
        OrderTraceStage::MatchingResting,
        Some("ord-pipeline-1"),
        "req-pipeline-1",
        "alice",
    ));

    let arrived = wait_until(
        || projector.get_order("ord-pipeline-1").is_some(),
        500,
    )
    .await;
    assert!(arrived, "order should be visible to the projector");

    let timeline = projector
        .get_timeline("ord-pipeline-1", None, None)
        .expect("timeline present");
    // All three buffered + bound events flushed into the timeline.
    assert_eq!(timeline.timeline.len(), 3);
    assert_eq!(timeline.timeline[0].stage, OrderTraceStage::ApiReceived);
    assert_eq!(
        timeline.timeline[1].stage,
        OrderTraceStage::SequencerAccepted
    );
    assert_eq!(
        timeline.timeline[2].stage,
        OrderTraceStage::MatchingResting
    );
}

#[tokio::test]
async fn end_to_end_pipeline_through_rest_handler_admin_view() {
    use warp::http::StatusCode;

    let event_bus = EventBus::new();
    let projector = OrderTraceProjector::new();
    spawn_test_consumer(event_bus.clone(), projector.clone()).await;

    let emitter: Arc<dyn TraceEmitter> =
        Arc::new(EventBusTraceEmitter::new(event_bus.clone()));

    // Two orders, two users. Admin sees both.
    emitter.emit(make_event(
        OrderTraceStage::SequencerAccepted,
        Some("ord-A"),
        "req-A",
        "alice",
    ));
    emitter.emit(make_event(
        OrderTraceStage::SequencerAccepted,
        Some("ord-B"),
        "req-B",
        "bob",
    ));

    // Wait for both to be visible.
    assert!(
        wait_until(
            || projector.get_order("ord-A").is_some() && projector.get_order("ord-B").is_some(),
            500,
        )
        .await
    );

    // Build the same routes that production wires.
    let admin = warp::any().and_then(|| async {
        Ok::<AuthenticatedPrincipal, warp::Rejection>(AuthenticatedPrincipal {
            subject: "root".into(),
            role: PrincipalRole::Admin,
            session_id: None,
        })
    });
    let routes = monitor_http::build_monitor_routes(projector.clone(), admin, None);

    // Admin: list returns both orders.
    let resp = warp::test::request()
        .method("GET")
        .path("/monitor/orders")
        .reply(&routes)
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    let orders = body["orders"].as_array().unwrap();
    let returned_ids: std::collections::HashSet<&str> =
        orders.iter().map(|o| o["order_id"].as_str().unwrap()).collect();
    assert!(returned_ids.contains("ord-A"));
    assert!(returned_ids.contains("ord-B"));

    // Admin: get_order works for any user.
    let resp = warp::test::request()
        .method("GET")
        .path("/monitor/orders/ord-B")
        .reply(&routes)
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(body["user_id"].as_str().unwrap(), "bob");
}

#[tokio::test]
async fn end_to_end_pipeline_through_rest_handler_non_admin_cannot_see_others() {
    use warp::http::StatusCode;

    let event_bus = EventBus::new();
    let projector = OrderTraceProjector::new();
    spawn_test_consumer(event_bus.clone(), projector.clone()).await;

    let emitter: Arc<dyn TraceEmitter> =
        Arc::new(EventBusTraceEmitter::new(event_bus.clone()));

    emitter.emit(make_event(
        OrderTraceStage::SequencerAccepted,
        Some("ord-alice"),
        "req-alice",
        "alice",
    ));
    emitter.emit(make_event(
        OrderTraceStage::SequencerAccepted,
        Some("ord-bob"),
        "req-bob",
        "bob",
    ));

    assert!(
        wait_until(
            || projector.get_order("ord-alice").is_some()
                && projector.get_order("ord-bob").is_some(),
            500,
        )
        .await
    );

    // Bob's principal: list_orders is force-filtered to bob.
    let bob_principal = warp::any().and_then(|| async {
        Ok::<AuthenticatedPrincipal, warp::Rejection>(AuthenticatedPrincipal {
            subject: "bob".into(),
            role: PrincipalRole::User,
            session_id: None,
        })
    });
    let routes = monitor_http::build_monitor_routes(projector.clone(), bob_principal, None);

    // Bob lists orders — should only see ord-bob, not ord-alice.
    let resp = warp::test::request()
        .method("GET")
        .path("/monitor/orders")
        .reply(&routes)
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    let orders = body["orders"].as_array().unwrap();
    assert_eq!(orders.len(), 1, "bob should only see his own order");
    assert_eq!(orders[0]["order_id"].as_str().unwrap(), "ord-bob");

    // Bob tries to read alice's order directly — must get 404 (not 403)
    // to avoid leaking existence.
    let resp = warp::test::request()
        .method("GET")
        .path("/monitor/orders/ord-alice")
        .reply(&routes)
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Bob can read his own order.
    let resp = warp::test::request()
        .method("GET")
        .path("/monitor/orders/ord-bob")
        .reply(&routes)
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn lagged_consumer_does_not_block_producer() {
    // Producer publishes faster than a hypothetical slow consumer can
    // drain. Since publish is fire-and-forget on a tokio broadcast
    // channel, producers must never observe blocking even with no
    // active subscriber. This test covers the property directly.
    let event_bus = EventBus::new();
    // Pre-subscribe so the channel exists, then drop and simulate a
    // dead consumer (no one reading).
    let rx = event_bus.subscribe("order.trace");
    drop(rx);

    let emitter: Arc<dyn TraceEmitter> =
        Arc::new(EventBusTraceEmitter::new(event_bus.clone()));

    let started = std::time::Instant::now();
    for i in 0..10_000 {
        emitter.emit(make_event(
            OrderTraceStage::ApiReceived,
            None,
            &format!("req-flood-{i}"),
            "alice",
        ));
    }
    let elapsed = started.elapsed();
    // 10k publishes with no consumer should complete in well under
    // a second. If the publish path is blocking, this would either
    // hang or take >> 1 s.
    assert!(
        elapsed < Duration::from_secs(2),
        "10k publishes took {elapsed:?}; producer must be non-blocking"
    );
}

#[tokio::test]
async fn missing_request_id_drops_pre_sequencer_event_silently() {
    let event_bus = EventBus::new();
    let projector = OrderTraceProjector::new();
    spawn_test_consumer(event_bus.clone(), projector.clone()).await;

    let emitter: Arc<dyn TraceEmitter> =
        Arc::new(EventBusTraceEmitter::new(event_bus.clone()));

    // Pre-sequencer event with neither order_id nor request_id has
    // nothing to bind to. The projector drops it; we expect no orders
    // and no orphan trace-key buckets.
    let mut ev = OrderTraceEvent::new_unbound(OrderTraceStage::ApiReceived);
    ev.user_id = Some("alice".into());
    emitter.emit(ev);

    // Give the consumer a moment.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(projector.order_count(), 0);
    assert_eq!(projector.trace_key_bucket_count(), 0);
}
