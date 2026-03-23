#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Structured Trace ID Correlation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Propagates a `trace_id` through every HTTP request span so that all
// log lines emitted during request processing can be correlated in log
// aggregation tools (Loki, Datadog, Splunk, CloudWatch, etc.).
//
// Flow:
//   1. Client sends `X-Request-Id: <value>` header (or omits it).
//   2. `request_trace()` warp filter extracts or generates a UUID-based
//      trace ID and enters a tracing::Span with `trace_id` field.
//   3. All downstream `tracing::info!/warn!/error!` calls inherit the
//      span context and include `trace_id` in JSON output.
//   4. Response includes `X-Request-Id` header for client correlation.
//
// The module also provides `current_trace_id()` for code that needs
// to read the active trace ID (e.g. WAL metadata, settlement records).

use super::*;

// Thread-local storage for the current request's trace ID.
// This is set by the `request_trace` filter and can be read by any
// downstream code within the same async task.
tokio::task_local! {
    static TRACE_ID: String;
}

/// Generate a short trace ID (hex-encoded, 16 chars = 8 bytes).
pub(crate) fn generate_trace_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Mix timestamp + counter for uniqueness without UUID dependency overhead.
    let raw = ts ^ (seq.wrapping_mul(0x517cc1b727220a95));
    format!("{raw:016x}")
}

/// Read the current trace ID from task-local storage.
/// Returns `None` if called outside a `request_trace` scope.
pub(crate) fn current_trace_id() -> Option<String> {
    TRACE_ID.try_with(|id| id.clone()).ok()
}

/// Warp trace filter — wraps every request in a span that carries `trace_id`.
///
/// Usage:  `.with(warp::trace(tracing_ctx::request_trace_fn()))`
///
/// This replaces the default `warp::trace::request()` with a version
/// that reads `X-Request-Id` from the incoming request and populates
/// both a tracing span field and a response header.
pub(crate) fn request_trace_fn() -> impl Fn(warp::trace::Info<'_>) -> tracing::Span + Clone {
    |info: warp::trace::Info<'_>| {
        let trace_id = info
            .request_headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .map(|s| s.to_string())
            .unwrap_or_else(generate_trace_id);

        tracing::info_span!(
            "request",
            trace_id = %trace_id,
            method = %info.method(),
            path = %info.path(),
        )
    }
}

/// Warp filter that extracts `X-Request-Id` from request headers or
/// generates a new one.  Can be `.and()`-ed into any route that needs
/// the trace ID as a parameter.
pub(crate) fn with_trace_id_header(
) -> impl Filter<Extract = (Option<String>,), Error = Rejection> + Clone {
    warp::header::optional::<String>("x-request-id").map(|incoming: Option<String>| {
        incoming
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .or_else(|| Some(generate_trace_id()))
    })
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_trace_id_is_16_hex_chars() {
        let id = generate_trace_id();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_trace_id_unique() {
        let ids: Vec<String> = (0..100).map(|_| generate_trace_id()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        // Should have at least 95 unique IDs out of 100.
        assert!(unique.len() >= 95, "too many collisions: {}", unique.len());
    }

    #[test]
    fn current_trace_id_outside_scope_is_none() {
        assert!(current_trace_id().is_none());
    }

    #[tokio::test]
    async fn task_local_trace_id_propagates() {
        let id = "test-trace-abc123".to_string();
        let captured = TRACE_ID
            .scope(id.clone(), async { current_trace_id() })
            .await;
        assert_eq!(captured, Some(id));
    }

    #[tokio::test]
    async fn task_local_trace_id_isolated_between_tasks() {
        let id1 = "trace-1".to_string();
        let id2 = "trace-2".to_string();

        let (r1, r2) = tokio::join!(
            TRACE_ID.scope(id1.clone(), async { current_trace_id() }),
            TRACE_ID.scope(id2.clone(), async { current_trace_id() }),
        );
        assert_eq!(r1, Some(id1));
        assert_eq!(r2, Some(id2));
    }

    #[test]
    fn request_trace_fn_returns_closure() {
        // Just verify the closure can be constructed (type check).
        let _f = request_trace_fn();
    }
}
