use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::LazyLock;

/// Maximum number of partitions tracked for per-partition metrics.
const MAX_PARTITIONS: usize = 16;

/// Log-scale histogram bucket boundaries in microseconds.
/// Covers 1μs → 1s with ~4 points per decade for smooth percentiles.
pub(crate) const HISTOGRAM_BOUNDARIES_US: &[u64] = &[
    1, 2, 5, 10, 20, 50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000, 50_000, 100_000,
    200_000, 500_000, 1_000_000,
];
/// Number of histogram buckets (boundaries + 1 overflow bucket).
pub(crate) const HISTOGRAM_BUCKETS: usize = 20; // HISTOGRAM_BOUNDARIES_US.len() + 1

/// Lightweight process-level counters — zero external dependencies.
pub(crate) struct ExchangeMetrics {
    pub orders_received: AtomicU64,
    pub orders_filled: AtomicU64,
    pub orders_rejected: AtomicU64,
    pub orders_cancelled: AtomicU64,
    pub settlements_committed: AtomicU64,
    pub wal_appends: AtomicU64,
    pub wal_errors: AtomicU64,
    pub snapshot_writes: AtomicU64,
    // WebSocket connection tracking.
    pub ws_connections_active: AtomicU64,
    pub ws_connections_total: AtomicU64,
    pub ws_messages_sent: AtomicU64,
    // HTTP request tracking.
    pub http_requests_total: AtomicU64,
    pub http_errors_total: AtomicU64,
    pub submit_order_ip_rate_limited: AtomicU64,
    pub submit_order_user_rate_limited: AtomicU64,
    pub submit_order_engine_rate_limited: AtomicU64,
    // Batch order tracking.
    pub batch_orders_submitted: AtomicU64,
    pub batch_orders_success: AtomicU64,
    pub batch_latency: HistogramTracker,
    // EventBus → WS bridge health.
    pub bridge_alive: AtomicBool,
    // Histogram latency trackers (microseconds) with p50/p95/p99.
    pub match_latency: HistogramTracker,
    pub wal_append_latency: HistogramTracker,
    pub queue_wait_latency: HistogramTracker,
    pub match_execution_latency: HistogramTracker,
    pub http_request_latency: HistogramTracker,
    // Granular matching-engine stage latencies.
    pub risk_latency: HistogramTracker,
    pub matching_core_latency: HistogramTracker,
    pub settlement_persist_latency: HistogramTracker,
    pub post_match_latency: HistogramTracker,
    // Per-partition fill counters.
    pub partition_fills: [AtomicU64; MAX_PARTITIONS],
    pub partition_orders: [AtomicU64; MAX_PARTITIONS],
}

/// Lock-free histogram latency tracker (microseconds) with percentiles.
///
/// Uses log-scale buckets with atomic counters.  Percentiles (p50/p95/p99) are
/// computed from the bucket distribution at snapshot time.  Also tracks
/// min/max/sum/count for backward-compatible aggregate stats.
pub(crate) struct HistogramTracker {
    pub(crate) count: AtomicU64,
    pub(crate) sum_us: AtomicU64,
    min_us: AtomicI64,
    max_us: AtomicI64,
    /// Bucket counters.  Index i counts samples where
    /// HISTOGRAM_BOUNDARIES_US[i-1] < value <= HISTOGRAM_BOUNDARIES_US[i].
    /// The last bucket is the overflow (> last boundary).
    pub(crate) buckets: [AtomicU64; HISTOGRAM_BUCKETS],
}

impl HistogramTracker {
    pub const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            sum_us: AtomicU64::new(0),
            min_us: AtomicI64::new(i64::MAX),
            max_us: AtomicI64::new(0),
            buckets: [const { AtomicU64::new(0) }; HISTOGRAM_BUCKETS],
        }
    }

    pub fn record(&self, micros: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_us.fetch_add(micros, Ordering::Relaxed);

        let val = micros as i64;
        // CAS loop for min.
        let mut cur = self.min_us.load(Ordering::Relaxed);
        while val < cur {
            match self
                .min_us
                .compare_exchange_weak(cur, val, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
        // CAS loop for max.
        cur = self.max_us.load(Ordering::Relaxed);
        while val > cur {
            match self
                .max_us
                .compare_exchange_weak(cur, val, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }

        // Bucket assignment — find first boundary >= micros.
        let bucket_idx = HISTOGRAM_BOUNDARIES_US
            .iter()
            .position(|&b| micros <= b)
            .unwrap_or(HISTOGRAM_BOUNDARIES_US.len()); // overflow bucket
        self.buckets[bucket_idx].fetch_add(1, Ordering::Relaxed);
    }

    /// Compute the approximate value at a given percentile (0.0–1.0) from
    /// the bucket distribution.  Returns the upper boundary of the bucket
    /// that contains the percentile rank, or 0 if no samples.
    fn percentile(&self, p: f64) -> u64 {
        self.percentile_pub(p)
    }

    /// Public accessor for percentile computation (used by perf module).
    pub(crate) fn percentile_pub(&self, p: f64) -> u64 {
        let total = self.count.load(Ordering::Relaxed);
        if total == 0 {
            return 0;
        }
        let threshold = ((total as f64) * p).ceil() as u64;
        let mut cumulative: u64 = 0;
        for (i, bucket) in self.buckets.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if cumulative >= threshold {
                return if i < HISTOGRAM_BOUNDARIES_US.len() {
                    HISTOGRAM_BOUNDARIES_US[i]
                } else {
                    // Overflow bucket — return the max observed value.
                    let max = self.max_us.load(Ordering::Relaxed);
                    if max > 0 {
                        max as u64
                    } else {
                        *HISTOGRAM_BOUNDARIES_US.last().unwrap()
                    }
                };
            }
        }
        0
    }

    fn snapshot_json(&self) -> serde_json::Value {
        let count = self.count.load(Ordering::Relaxed);
        let sum = self.sum_us.load(Ordering::Relaxed);
        let min = self.min_us.load(Ordering::Relaxed);
        let max = self.max_us.load(Ordering::Relaxed);
        let avg = if count > 0 { sum / count } else { 0 };
        serde_json::json!({
            "count": count,
            "avg_us": avg,
            "min_us": if min == i64::MAX { 0 } else { min },
            "max_us": max,
            "p50_us": self.percentile(0.50),
            "p95_us": self.percentile(0.95),
            "p99_us": self.percentile(0.99),
        })
    }
}

impl ExchangeMetrics {
    pub const fn new() -> Self {
        Self {
            orders_received: AtomicU64::new(0),
            orders_filled: AtomicU64::new(0),
            orders_rejected: AtomicU64::new(0),
            orders_cancelled: AtomicU64::new(0),
            settlements_committed: AtomicU64::new(0),
            wal_appends: AtomicU64::new(0),
            wal_errors: AtomicU64::new(0),
            snapshot_writes: AtomicU64::new(0),
            ws_connections_active: AtomicU64::new(0),
            ws_connections_total: AtomicU64::new(0),
            ws_messages_sent: AtomicU64::new(0),
            http_requests_total: AtomicU64::new(0),
            http_errors_total: AtomicU64::new(0),
            submit_order_ip_rate_limited: AtomicU64::new(0),
            submit_order_user_rate_limited: AtomicU64::new(0),
            submit_order_engine_rate_limited: AtomicU64::new(0),
            batch_orders_submitted: AtomicU64::new(0),
            batch_orders_success: AtomicU64::new(0),
            batch_latency: HistogramTracker::new(),
            bridge_alive: AtomicBool::new(false),
            match_latency: HistogramTracker::new(),
            wal_append_latency: HistogramTracker::new(),
            queue_wait_latency: HistogramTracker::new(),
            match_execution_latency: HistogramTracker::new(),
            http_request_latency: HistogramTracker::new(),
            risk_latency: HistogramTracker::new(),
            matching_core_latency: HistogramTracker::new(),
            settlement_persist_latency: HistogramTracker::new(),
            post_match_latency: HistogramTracker::new(),
            partition_fills: [const { AtomicU64::new(0) }; MAX_PARTITIONS],
            partition_orders: [const { AtomicU64::new(0) }; MAX_PARTITIONS],
        }
    }

    pub fn record_partition_fill(&self, partition_id: usize, count: u64) {
        if partition_id < MAX_PARTITIONS {
            self.partition_fills[partition_id].fetch_add(count, Ordering::Relaxed);
        }
    }

    pub fn record_partition_order(&self, partition_id: usize) {
        if partition_id < MAX_PARTITIONS {
            self.partition_orders[partition_id].fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_cancel(&self) {
        self.orders_cancelled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_order_received(&self) {
        self.orders_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_order_filled(&self) {
        self.orders_filled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_order_rejected(&self) {
        self.orders_rejected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_submit_order_ip_rate_limited(&self) {
        self.submit_order_ip_rate_limited
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_submit_order_user_rate_limited(&self) {
        self.submit_order_user_rate_limited
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_submit_order_engine_rate_limited(&self) {
        self.submit_order_engine_rate_limited
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> serde_json::Value {
        let mut partition_detail = Vec::new();
        for i in 0..MAX_PARTITIONS {
            let fills = self.partition_fills[i].load(Ordering::Relaxed);
            let orders = self.partition_orders[i].load(Ordering::Relaxed);
            if fills > 0 || orders > 0 {
                partition_detail.push(serde_json::json!({
                    "partition": i,
                    "fills": fills,
                    "orders": orders,
                }));
            }
        }
        serde_json::json!({
            "orders_received": self.orders_received.load(Ordering::Relaxed),
            "orders_filled": self.orders_filled.load(Ordering::Relaxed),
            "orders_rejected": self.orders_rejected.load(Ordering::Relaxed),
            "orders_cancelled": self.orders_cancelled.load(Ordering::Relaxed),
            "settlements_committed": self.settlements_committed.load(Ordering::Relaxed),
            "wal_appends": self.wal_appends.load(Ordering::Relaxed),
            "wal_errors": self.wal_errors.load(Ordering::Relaxed),
            "snapshot_writes": self.snapshot_writes.load(Ordering::Relaxed),
            "ws_connections_active": self.ws_connections_active.load(Ordering::Relaxed),
            "ws_connections_total": self.ws_connections_total.load(Ordering::Relaxed),
            "ws_messages_sent": self.ws_messages_sent.load(Ordering::Relaxed),
            "http_requests_total": self.http_requests_total.load(Ordering::Relaxed),
            "http_errors_total": self.http_errors_total.load(Ordering::Relaxed),
            "submit_order_ip_rate_limited": self.submit_order_ip_rate_limited.load(Ordering::Relaxed),
            "submit_order_user_rate_limited": self.submit_order_user_rate_limited.load(Ordering::Relaxed),
            "submit_order_engine_rate_limited": self.submit_order_engine_rate_limited.load(Ordering::Relaxed),
            "bridge_alive": self.bridge_alive.load(Ordering::Relaxed),
            "latency": {
                "match_e2e_us": self.match_latency.snapshot_json(),
                "queue_wait_us": self.queue_wait_latency.snapshot_json(),
                "match_execution_us": self.match_execution_latency.snapshot_json(),
                "wal_append_us": self.wal_append_latency.snapshot_json(),
                "http_request_us": self.http_request_latency.snapshot_json(),
                "granular": {
                    "risk_us": self.risk_latency.snapshot_json(),
                    "matching_core_us": self.matching_core_latency.snapshot_json(),
                    "settlement_persist_us": self.settlement_persist_latency.snapshot_json(),
                    "post_match_us": self.post_match_latency.snapshot_json(),
                },
            },
            "partitions": partition_detail,
        })
    }
}

/// Global metrics singleton.
pub(crate) static METRICS: ExchangeMetrics = ExchangeMetrics::new();

/// Per-path HTTP request counters (path → count).
pub(crate) static HTTP_PATH_COUNTERS: LazyLock<DashMap<String, u64>> = LazyLock::new(DashMap::new);

/// Record an HTTP request for a normalised path.
pub(crate) fn record_http_path(path: &str) {
    // Normalise: strip query, collapse path params to placeholders.
    let normalised = normalise_path(path);
    HTTP_PATH_COUNTERS
        .entry(normalised)
        .and_modify(|c| *c += 1)
        .or_insert(1);
}

/// Collapse numeric / UUID-like segments to `:id` for stable cardinality.
fn normalise_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('?').next().unwrap_or(path).split('/').collect();
    let normalised: Vec<&str> = parts
        .iter()
        .map(|seg| {
            if seg.is_empty() {
                return *seg;
            }
            // Pure numeric or UUID-like → `:id`
            if seg.chars().all(|c| c.is_ascii_digit())
                || (seg.len() > 8
                    && seg.contains('-')
                    && seg.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
            {
                ":id"
            } else {
                seg
            }
        })
        .collect();
    normalised.join("/")
}
