//! Prometheus text format exporter.
//!
//! Converts `observability::METRICS` into `text/plain; version=0.4.0` lines
//! that Prometheus can scrape.  Zero external crate dependencies — the text
//! wire format is trivial enough to emit directly.

use std::fmt::Write;
use std::sync::atomic::Ordering;

use super::observability::{
    HISTOGRAM_BOUNDARIES_US, HTTP_PATH_COUNTERS, METRICS, WALLET_HOT_BALANCES,
};

/// Render all exchange metrics in Prometheus exposition format.
pub fn render_prometheus() -> String {
    let mut out = String::with_capacity(4096);

    // ── Counters ─────────────────────────────────────────
    counter(
        &mut out,
        "exchange_orders_received_total",
        "Total orders received",
        METRICS.orders_received.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_orders_filled_total",
        "Total orders filled",
        METRICS.orders_filled.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_orders_rejected_total",
        "Total orders rejected",
        METRICS.orders_rejected.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_orders_cancelled_total",
        "Total orders cancelled",
        METRICS.orders_cancelled.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_settlements_committed_total",
        "Total settlements committed",
        METRICS.settlements_committed.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_wal_appends_total",
        "Total WAL append operations",
        METRICS.wal_appends.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_wal_errors_total",
        "Total WAL errors",
        METRICS.wal_errors.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_snapshot_writes_total",
        "Total snapshot writes",
        METRICS.snapshot_writes.load(Ordering::Relaxed),
    );

    // ── WebSocket gauges / counters ──────────────────────
    gauge(
        &mut out,
        "exchange_ws_connections_active",
        "Active WebSocket connections",
        METRICS.ws_connections_active.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_ws_connections_total",
        "Total WebSocket connections opened",
        METRICS.ws_connections_total.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_ws_messages_sent_total",
        "Total WebSocket messages sent",
        METRICS.ws_messages_sent.load(Ordering::Relaxed),
    );

    // ── HTTP counters ────────────────────────────────────
    counter(
        &mut out,
        "exchange_http_requests_total",
        "Total HTTP requests",
        METRICS.http_requests_total.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_http_errors_total",
        "Total HTTP error responses",
        METRICS.http_errors_total.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_submit_order_ip_rate_limited_total",
        "Total /submit-order requests rejected by IP rate limiting",
        METRICS.submit_order_ip_rate_limited.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_submit_order_user_rate_limited_total",
        "Total /submit-order requests rejected by user write rate limiting",
        METRICS
            .submit_order_user_rate_limited
            .load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "exchange_submit_order_engine_rate_limited_total",
        "Total /submit-order requests rejected by matching-engine rate limiting",
        METRICS
            .submit_order_engine_rate_limited
            .load(Ordering::Relaxed),
    );

    // ── Bridge health ────────────────────────────────────
    gauge(
        &mut out,
        "exchange_bridge_alive",
        "EventBus to WS bridge alive (1=up, 0=down)",
        if METRICS.bridge_alive.load(Ordering::Relaxed) {
            1
        } else {
            0
        },
    );

    // ── Per-path HTTP request counters ───────────────────
    {
        let _ = writeln!(
            out,
            "# HELP exchange_http_requests_by_path HTTP requests per normalized path"
        );
        let _ = writeln!(out, "# TYPE exchange_http_requests_by_path counter");
        for entry in HTTP_PATH_COUNTERS.iter() {
            let _ = writeln!(
                out,
                "exchange_http_requests_by_path{{path=\"{}\"}} {}",
                entry.key(),
                entry.value()
            );
        }
    }

    // ── Histograms ───────────────────────────────────────
    histogram(
        &mut out,
        "exchange_match_latency_us",
        "Match end-to-end latency in microseconds",
        &METRICS.match_latency,
    );
    histogram(
        &mut out,
        "exchange_queue_wait_latency_us",
        "Queue wait latency in microseconds",
        &METRICS.queue_wait_latency,
    );
    histogram(
        &mut out,
        "exchange_match_execution_latency_us",
        "Pure match execution latency in microseconds",
        &METRICS.match_execution_latency,
    );
    histogram(
        &mut out,
        "exchange_wal_append_latency_us",
        "WAL append latency in microseconds",
        &METRICS.wal_append_latency,
    );
    histogram(
        &mut out,
        "exchange_http_request_latency_us",
        "HTTP request latency in microseconds",
        &METRICS.http_request_latency,
    );

    // ── Granular matching-engine stage latencies ─────────
    histogram(
        &mut out,
        "exchange_risk_latency_us",
        "Risk check + reservation latency in microseconds",
        &METRICS.risk_latency,
    );
    histogram(
        &mut out,
        "exchange_matching_core_latency_us",
        "Core order-book matching latency in microseconds",
        &METRICS.matching_core_latency,
    );
    histogram(
        &mut out,
        "exchange_settlement_persist_latency_us",
        "Trade settlement + WAL persistence latency in microseconds",
        &METRICS.settlement_persist_latency,
    );
    histogram(
        &mut out,
        "exchange_post_match_latency_us",
        "Post-match processing latency in microseconds",
        &METRICS.post_match_latency,
    );

    // ── Per-partition gauges ─────────────────────────────
    let _ = writeln!(
        out,
        "# HELP exchange_partition_fills_total Total fills per partition"
    );
    let _ = writeln!(out, "# TYPE exchange_partition_fills_total counter");
    for i in 0..16 {
        let v = METRICS.partition_fills[i].load(Ordering::Relaxed);
        if v > 0 {
            let _ = writeln!(
                out,
                "exchange_partition_fills_total{{partition=\"{i}\"}} {v}"
            );
        }
    }

    let _ = writeln!(
        out,
        "# HELP exchange_partition_orders_total Total orders per partition"
    );
    let _ = writeln!(out, "# TYPE exchange_partition_orders_total counter");
    for i in 0..16 {
        let v = METRICS.partition_orders[i].load(Ordering::Relaxed);
        if v > 0 {
            let _ = writeln!(
                out,
                "exchange_partition_orders_total{{partition=\"{i}\"}} {v}"
            );
        }
    }

    // ── Wallet (P1-OPS-1) ────────────────────────────────
    counter(
        &mut out,
        "wallet_settlements_settled_total",
        "Wallet settlement worker — withdrawals flipped Confirmed -> Settled",
        METRICS.wallet_settlements_settled.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "wallet_settlements_failed_total",
        "Wallet settlement worker — transient failures (record-not-found, store-update-failed)",
        METRICS.wallet_settlements_failed.load(Ordering::Relaxed),
    );
    // The headline alert metric: every increment of this counter is a
    // record where the on-chain broadcast already happened but the
    // customer-side ledger debit could not be applied. PagerDuty.
    counter(
        &mut out,
        "wallet_settlements_stuck_total",
        "Wallet settlement worker — withdrawals flipped Confirmed -> SettlementStuck (operator action required)",
        METRICS.wallet_settlements_stuck.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "wallet_sanctions_errors_total",
        "Sanctions provider returned Error (request hard-blocked with 503 SanctionsUnavailable)",
        METRICS.wallet_sanctions_errors.load(Ordering::Relaxed),
    );

    // Per-chain hot-wallet balance gauge.
    let _ = writeln!(
        out,
        "# HELP wallet_hot_wallet_balance Hot-wallet on-chain balance (ledger units, post-divisor)"
    );
    let _ = writeln!(out, "# TYPE wallet_hot_wallet_balance gauge");
    for entry in WALLET_HOT_BALANCES.iter() {
        let _ = writeln!(
            out,
            "wallet_hot_wallet_balance{{chain=\"{}\"}} {}",
            entry.key(),
            entry.value().load(Ordering::Relaxed)
        );
    }

    out
}

// ── Helpers ──────────────────────────────────────────────────

fn counter(out: &mut String, name: &str, help: &str, value: u64) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} counter");
    let _ = writeln!(out, "{name} {value}");
}

fn gauge(out: &mut String, name: &str, help: &str, value: u64) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} gauge");
    let _ = writeln!(out, "{name} {value}");
}

fn histogram(
    out: &mut String,
    name: &str,
    help: &str,
    tracker: &super::observability::HistogramTracker,
) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} histogram");

    let mut cumulative: u64 = 0;
    for (i, bucket) in tracker.buckets.iter().enumerate() {
        cumulative += bucket.load(Ordering::Relaxed);
        let le = if i < HISTOGRAM_BOUNDARIES_US.len() {
            format!("{}", HISTOGRAM_BOUNDARIES_US[i])
        } else {
            "+Inf".to_string()
        };
        let _ = writeln!(out, "{name}_bucket{{le=\"{le}\"}} {cumulative}");
    }

    let count = tracker.count.load(Ordering::Relaxed);
    let sum = tracker.sum_us.load(Ordering::Relaxed);
    let _ = writeln!(out, "{name}_sum {sum}");
    let _ = writeln!(out, "{name}_count {count}");
}
