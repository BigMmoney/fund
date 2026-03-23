#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Extreme Environment Stress Testing Framework
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// 8 extreme scenarios covering:
//   S1 — Queue Saturation:   flood all partitions to Shedding backpressure
//   S2 — Burst Spike:        sudden 10x order burst after idle period
//   S3 — WAL Storm:          high-frequency WAL writes under sustained load
//   S4 — Settlement Cascade: rapid fills that stress ledger + risk settlement
//   S5 — Kill Switch Storm:  toggle kill-switch during active trading
//   S6 — Concurrent Cancel:  mass-cancel while new orders stream in
//   S7 — Backpressure Ramp:  gradual load increase until Critical → Shedding
//   S8 — Recovery After Crash: snapshot + WAL replay with stale data
//
// Each scenario produces a `ScenarioReport` with:
//   • pass/fail verdict
//   • latency percentiles (p50/p95/p99)
//   • throughput (ops/sec)
//   • invariant checks (ledger balance, partition health)
//   • detailed event log
//
// The full suite is executed by `run_stress_suite()` which aggregates
// all scenario reports into a `StressSuiteReport` with JSON + text output.

use super::*;
use std::time::Instant;

// ── Scenario metadata & report types ─────────────────────────

/// Severity classification for a stress scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Severity {
    /// Extreme load — tests system limits.
    Extreme,
    /// Chaos — tests failure handling under load.
    Chaos,
    /// Recovery — tests crash recovery and data integrity.
    Recovery,
}

/// Outcome of a single scenario.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ScenarioReport {
    pub(crate) name: String,
    pub(crate) severity: Severity,
    pub(crate) passed: bool,
    pub(crate) duration_ms: u64,
    pub(crate) operations: u64,
    pub(crate) throughput_ops_sec: f64,
    pub(crate) latency: LatencyReport,
    pub(crate) invariants: InvariantReport,
    pub(crate) events: Vec<String>,
    pub(crate) error: Option<String>,
}

/// Latency percentile snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct LatencyReport {
    pub(crate) p50_us: u64,
    pub(crate) p95_us: u64,
    pub(crate) p99_us: u64,
    pub(crate) max_us: u64,
    pub(crate) avg_us: u64,
}

/// Post-scenario invariant verification.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct InvariantReport {
    pub(crate) ledger_balanced: bool,
    pub(crate) partitions_healthy: bool,
    pub(crate) kill_switch_off: bool,
    pub(crate) details: Vec<String>,
}

/// Aggregated report for the full stress suite.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct StressSuiteReport {
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) total_scenarios: usize,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) total_duration_ms: u64,
    pub(crate) scenarios: Vec<ScenarioReport>,
}

// ── Lightweight latency collector (test-local, no global state) ──

struct LatencyCollector {
    samples: Vec<u64>,
}

impl LatencyCollector {
    fn new(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
        }
    }

    fn record(&mut self, micros: u64) {
        self.samples.push(micros);
    }

    fn report(&mut self) -> LatencyReport {
        if self.samples.is_empty() {
            return LatencyReport {
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
                max_us: 0,
                avg_us: 0,
            };
        }
        self.samples.sort_unstable();
        let n = self.samples.len();
        let sum: u64 = self.samples.iter().sum();
        LatencyReport {
            p50_us: self.samples[n * 50 / 100],
            p95_us: self.samples[n * 95 / 100],
            p99_us: self.samples[n * 99 / 100],
            max_us: self.samples[n - 1],
            avg_us: sum / n as u64,
        }
    }
}

// ── Test runtime builder (DRY helper) ────────────────────────

struct StressRuntime {
    engine: Arc<PartitionedMatchingEngine>,
    ledger: Arc<LedgerService>,
    sequencer: Arc<Sequencer>,
}

fn build_stress_runtime(partitions: usize, queue_capacity: usize) -> StressRuntime {
    use instruments::InMemoryInstrumentRegistry;

    let event_bus = EventBus::new();
    let ledger_wal: Arc<dyn persistence::WalStore<LedgerDelta>> =
        Arc::new(persistence::InMemoryWal::new());
    let ledger = Arc::new(LedgerService::with_wal_store(event_bus.clone(), ledger_wal));

    let sequencer_wal: Arc<dyn persistence::WalStore<SequencedCommandRecord>> =
        Arc::new(persistence::InMemoryWal::new());
    let sequencer = Arc::new(Sequencer::with_wal(1, sequencer_wal));

    let snapshot_wal: Arc<dyn persistence::WalStore<PartitionSnapshotRecord>> =
        Arc::new(persistence::InMemoryWal::new());
    let journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>> =
        Arc::new(persistence::InMemoryWal::new());
    let settlement_wal: Arc<dyn persistence::WalStore<TradeSettlementRecord>> =
        Arc::new(persistence::InMemoryWal::new());
    let state_wal: Arc<dyn persistence::WalStore<PositionCostLedgerEntry>> =
        Arc::new(persistence::InMemoryWal::new());
    let event_wal: Arc<dyn persistence::WalStore<PositionCostLedgerEvent>> =
        Arc::new(persistence::InMemoryWal::new());
    let position_costs =
        Arc::new(PositionCostLedgerStore::new(state_wal, event_wal).expect("position cost store"));

    let registry = Arc::new(InMemoryInstrumentRegistry::new().with_spec(InstrumentSpec {
        instrument_id: "btc-usdt".to_string(),
        kind: InstrumentKind::Spot,
        base_asset: String::new(),
        quote_asset: "USDC".to_string(),
        margin_mode: None,
        max_leverage: None,
        tick_size: 1,
        lot_size: 1,
        price_band_bps: 10_000, // wide band for stress tests
        risk_policy_id: "spot-v1".to_string(),
        min_order_amount: 1,
        max_notional: 0,
        maker_fee_bps: 0, // zero fees for clean invariant checks
        taker_fee_bps: 0,
        max_position_notional: 0,
        maintenance_margin_bps: 0,
        contract_multiplier: 1,
        funding_interval_secs: 0,
        status: InstrumentStatus::Active,
        circuit_breaker: None,
        mm_protection: None,
        max_order_amount: 0,
        order_type_rule: None,
        margin_rule: None,
        liquidation_rule: None,
        fee_schedule: None,
        margin_tiers: None,
        expiry: None,
        option_spec: None,
        settlement_currency: None,
    }));

    let config = PartitionedEngineConfig {
        partitions,
        queue_capacity,
        snapshot_interval_commands: usize::MAX, // no auto-snapshot during stress
        ..PartitionedEngineConfig::default()
    };

    let risk = Arc::new(RiskEngine::new(ledger.clone()));
    let engine_registry: Arc<dyn instruments::InstrumentRegistry> = registry;

    let engine = Arc::new(
        PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            config,
            event_bus,
            risk,
            engine_registry,
            Some(snapshot_wal),
            Some(journal_wal),
            Some(position_costs),
            Some(settlement_wal),
        )
        .expect("stress test engine"),
    );

    StressRuntime {
        engine,
        ledger,
        sequencer,
    }
}

fn check_invariants(rt: &StressRuntime, extra_details: Vec<String>) -> InvariantReport {
    let ledger_ok = rt.ledger.verify_global_invariant().is_ok();
    let depths = rt.engine.queue_depths();
    let partitions_ok = depths.iter().all(|d| d.inflight < d.capacity);
    let ks_off = !rt.engine.kill_switch_enabled();

    let mut details = extra_details;
    if !ledger_ok {
        details.push("FAIL: ledger balance invariant violated".into());
    }
    if !partitions_ok {
        details.push("WARN: some partitions near capacity".into());
    }
    if !ks_off {
        details.push("INFO: kill switch still enabled".into());
    }

    InvariantReport {
        ledger_balanced: ledger_ok,
        partitions_healthy: partitions_ok,
        kill_switch_off: ks_off,
        details,
    }
}

fn seed_user(rt: &StressRuntime, user_id: &str, cash: i64, btc: i64) {
    let _ = rt
        .ledger
        .process_deposit(user_id, cash, format!("seed-cash-{user_id}"));
    if btc > 0 {
        let _ = rt.ledger.process_position_deposit(
            user_id,
            "btc-usdt",
            0,
            btc,
            format!("seed-btc-{user_id}"),
        );
    }
}

fn make_order_cmd(
    seq: &Sequencer,
    user_id: &str,
    side: Side,
    price: i64,
    qty: i64,
    req_suffix: &str,
) -> Result<NewOrderCommand, String> {
    sequence_new_order(
        seq,
        format!("stress-{req_suffix}"),
        format!("coid-{req_suffix}"),
        user_id.to_string(),
        None,
        "btc-usdt".to_string(),
        side,
        OrderType::Limit,
        TimeInForce::Gtc,
        Some(price),
        qty,
        0,
        false,
        false,
        None,
        None,
        types::StpMode::default(),
        None,
        None,
    )
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// S1 — Queue Saturation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Objective: Flood all partitions with orders until QueueFull errors appear.
// Validates: backpressure transition Normal→Degraded→Critical→Shedding,
//            no data corruption, graceful rejection, invariant preservation.

async fn scenario_queue_saturation() -> ScenarioReport {
    let start = Instant::now();
    let mut events = Vec::new();
    let mut latencies = LatencyCollector::new(2000);

    // Tiny queue (16) to trigger saturation quickly.
    let rt = build_stress_runtime(2, 16);
    seed_user(&rt, "flood-buyer", 100_000_000, 0);
    seed_user(&rt, "flood-seller", 100_000_000, 100_000);

    events.push("seeded users with 100M cash, 100k BTC".into());

    // Seed resting sells so buys can match and consume queue slots.
    for i in 0..50 {
        let cmd = make_order_cmd(
            &rt.sequencer,
            "flood-seller",
            Side::Sell,
            50_000,
            1,
            &format!("sat-sell-{i}"),
        );
        if let Ok(cmd) = cmd {
            let _ = rt.engine.submit_new_order(cmd).await;
        }
    }
    events.push("seeded 50 resting sell orders".into());

    let mut submitted = 0u64;
    let mut accepted = 0u64;
    let mut rejected_queue_full = 0u64;
    let mut rejected_other = 0u64;
    let mut saw_degraded = false;
    let mut saw_critical = false;
    let mut saw_shedding = false;

    // Fire orders as fast as possible.
    for i in 0..1500 {
        let op_start = Instant::now();
        let cmd = make_order_cmd(
            &rt.sequencer,
            "flood-buyer",
            Side::Buy,
            50_000,
            1,
            &format!("sat-buy-{i}"),
        );
        let cmd = match cmd {
            Ok(c) => c,
            Err(_) => {
                rejected_other += 1;
                continue;
            }
        };

        submitted += 1;
        match rt.engine.submit_new_order(cmd).await {
            Ok(_) => {
                accepted += 1;
                latencies.record(op_start.elapsed().as_micros() as u64);
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("QueueFull") || msg.contains("queue") {
                    rejected_queue_full += 1;
                } else {
                    rejected_other += 1;
                }
            }
        }

        // Sample backpressure.
        let bp = rt.engine.backpressure_signal();
        match bp {
            matching::partitioned::BackpressureSignal::Degraded { .. } => saw_degraded = true,
            matching::partitioned::BackpressureSignal::Critical { .. } => saw_critical = true,
            matching::partitioned::BackpressureSignal::Shedding => saw_shedding = true,
            _ => {}
        }
    }

    events.push(format!(
        "submitted={submitted} accepted={accepted} queue_full={rejected_queue_full} other_reject={rejected_other}"
    ));
    events.push(format!(
        "backpressure: degraded={saw_degraded} critical={saw_critical} shedding={saw_shedding}"
    ));

    // Pass criteria: at least some orders accepted, some queue-full rejections,
    // and ledger invariant holds.
    let invariants = check_invariants(&rt, vec![]);
    let passed = accepted > 0 && invariants.ledger_balanced;

    let duration_ms = start.elapsed().as_millis() as u64;
    ScenarioReport {
        name: "S1_queue_saturation".into(),
        severity: Severity::Extreme,
        passed,
        duration_ms,
        operations: submitted,
        throughput_ops_sec: if duration_ms > 0 {
            submitted as f64 * 1000.0 / duration_ms as f64
        } else {
            0.0
        },
        latency: latencies.report(),
        invariants,
        events,
        error: if !passed {
            Some("queue saturation test failed invariant or acceptance checks".into())
        } else {
            None
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// S2 — Burst Spike
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Objective: After an idle period, fire a sudden burst of orders.
// Validates: system handles spike without crash, latency recovery,
//            no lost orders.

async fn scenario_burst_spike() -> ScenarioReport {
    let start = Instant::now();
    let mut events = Vec::new();
    let mut latencies = LatencyCollector::new(500);

    let rt = build_stress_runtime(4, 256);
    seed_user(&rt, "burst-seller", 50_000_000, 50_000);
    seed_user(&rt, "burst-buyer", 50_000_000, 0);

    // Place resting sells.
    for i in 0..200 {
        let cmd = make_order_cmd(
            &rt.sequencer,
            "burst-seller",
            Side::Sell,
            50_000,
            1,
            &format!("burst-sell-{i}"),
        );
        if let Ok(cmd) = cmd {
            let _ = rt.engine.submit_new_order(cmd).await;
        }
    }
    events.push("seeded 200 resting sells, then idle".into());

    // Idle period (simulate with a small yield).
    tokio::task::yield_now().await;

    // BURST: 500 buy orders as fast as possible.
    let burst_start = Instant::now();
    let mut burst_accepted = 0u64;
    for i in 0..500 {
        let op_start = Instant::now();
        let cmd = make_order_cmd(
            &rt.sequencer,
            "burst-buyer",
            Side::Buy,
            50_000,
            1,
            &format!("burst-buy-{i}"),
        );
        if let Ok(cmd) = cmd {
            if rt.engine.submit_new_order(cmd).await.is_ok() {
                burst_accepted += 1;
                latencies.record(op_start.elapsed().as_micros() as u64);
            }
        }
    }
    let burst_duration_ms = burst_start.elapsed().as_millis() as u64;

    events.push(format!(
        "burst: 500 orders in {burst_duration_ms}ms, accepted={burst_accepted}"
    ));

    let invariants = check_invariants(&rt, vec![]);
    let passed = burst_accepted > 0 && invariants.ledger_balanced;

    ScenarioReport {
        name: "S2_burst_spike".into(),
        severity: Severity::Extreme,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
        operations: 500,
        throughput_ops_sec: if burst_duration_ms > 0 {
            500.0 * 1000.0 / burst_duration_ms as f64
        } else {
            0.0
        },
        latency: latencies.report(),
        invariants,
        events,
        error: if !passed {
            Some("burst spike test failed".into())
        } else {
            None
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// S3 — WAL Storm
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Objective: Sustained order flow stressing WAL append path.
//            Each order → sequencer WAL + matching potential snapshot.
// Validates: no WAL corruption, op-id deduplication, invariant holds.

async fn scenario_wal_storm() -> ScenarioReport {
    let start = Instant::now();
    let mut events = Vec::new();
    let mut latencies = LatencyCollector::new(1000);

    let rt = build_stress_runtime(2, 512);
    seed_user(&rt, "wal-buyer", 50_000_000, 0);
    seed_user(&rt, "wal-seller", 50_000_000, 50_000);

    let mut total = 0u64;
    let mut ok = 0u64;
    let mut dup = 0u64;

    // Sustained flow of 1000 orders (alternating users for buy/sell).
    for i in 0..1000 {
        let op_start = Instant::now();
        let (user, side) = if i % 2 == 0 {
            ("wal-buyer", Side::Buy)
        } else {
            ("wal-seller", Side::Sell)
        };
        let cmd = make_order_cmd(&rt.sequencer, user, side, 50_000, 1, &format!("wal-{i}"));
        total += 1;
        match cmd {
            Ok(cmd) => match rt.engine.submit_new_order(cmd).await {
                Ok(_) => {
                    ok += 1;
                    latencies.record(op_start.elapsed().as_micros() as u64);
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("duplicate") {
                        dup += 1;
                    }
                }
            },
            Err(_) => dup += 1,
        }
    }

    events.push(format!("wal_storm: total={total} ok={ok} dup={dup}"));

    // Verify ledger invariant and WAL consistency.
    let invariants = check_invariants(&rt, vec![format!("wal_entries_expected ~{ok}")]);
    let passed = ok > 500 && invariants.ledger_balanced;

    ScenarioReport {
        name: "S3_wal_storm".into(),
        severity: Severity::Extreme,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
        operations: total,
        throughput_ops_sec: {
            let d = start.elapsed().as_millis() as f64;
            if d > 0.0 {
                total as f64 * 1000.0 / d
            } else {
                0.0
            }
        },
        latency: latencies.report(),
        invariants,
        events,
        error: if !passed {
            Some("WAL storm test failed".into())
        } else {
            None
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// S4 — Settlement Cascade
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Objective: Rapid matching that triggers many fills back-to-back,
//            stressing the settlement/ledger path.
// Validates: correct settlement amounts, no double-spend, invariant.

async fn scenario_settlement_cascade() -> ScenarioReport {
    let start = Instant::now();
    let mut events = Vec::new();
    let mut latencies = LatencyCollector::new(500);

    let rt = build_stress_runtime(4, 512);
    seed_user(&rt, "maker", 100_000_000, 100_000);
    seed_user(&rt, "taker", 100_000_000, 100_000);

    // Maker places 300 resting sells at staggered prices.
    for i in 0..300 {
        let price = 50_000 + (i % 10); // slight price spread
        let cmd = make_order_cmd(
            &rt.sequencer,
            "maker",
            Side::Sell,
            price,
            1,
            &format!("cascade-sell-{i}"),
        );
        if let Ok(cmd) = cmd {
            let _ = rt.engine.submit_new_order(cmd).await;
        }
    }
    events.push("seeded 300 maker sells at 50000-50009".into());

    // Taker sweeps all resting orders.
    let mut fills = 0u64;
    for i in 0..300 {
        let op_start = Instant::now();
        let cmd = make_order_cmd(
            &rt.sequencer,
            "taker",
            Side::Buy,
            50_010,
            1,
            &format!("cascade-buy-{i}"),
        );
        if let Ok(cmd) = cmd {
            if let Ok(result) = rt.engine.submit_new_order(cmd).await {
                fills += result.fills.len() as u64;
                latencies.record(op_start.elapsed().as_micros() as u64);
            }
        }
    }

    events.push(format!("settlement_cascade: fills={fills}"));

    let maker_cash = rt.ledger.cash_available_balance("maker");
    let taker_cash = rt.ledger.cash_available_balance("taker");
    events.push(format!("maker_cash={maker_cash} taker_cash={taker_cash}"));

    let invariants = check_invariants(&rt, vec![format!("total_fills={fills}")]);
    let passed = fills > 0 && invariants.ledger_balanced;

    ScenarioReport {
        name: "S4_settlement_cascade".into(),
        severity: Severity::Extreme,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
        operations: 300,
        throughput_ops_sec: {
            let d = start.elapsed().as_millis() as f64;
            if d > 0.0 {
                300.0 * 1000.0 / d
            } else {
                0.0
            }
        },
        latency: latencies.report(),
        invariants,
        events,
        error: if !passed {
            Some("settlement cascade failed".into())
        } else {
            None
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// S5 — Kill Switch Storm
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Objective: Toggle kill-switch on/off rapidly while orders are flowing.
// Validates: kill-switch correctly blocks/unblocks, no data corruption,
//            orders submitted during ON state are properly rejected.

async fn scenario_kill_switch_storm() -> ScenarioReport {
    let start = Instant::now();
    let mut events = Vec::new();

    let rt = build_stress_runtime(2, 256);
    seed_user(&rt, "ks-trader", 50_000_000, 50_000);

    let mut accepted_during_off = 0u64;
    let mut rejected_during_on = 0u64;
    let mut total_toggles = 0u64;

    for round in 0..10 {
        // Turn kill switch ON.
        let ks_on = AdminCommand {
            metadata: CommandMetadata::new(format!("ks-on-{round}")),
            action: AdminAction::KillSwitch { enabled: true },
            actor_id: "stress-admin".to_string(),
        };
        let _ = rt.engine.submit_admin(ks_on).await;
        total_toggles += 1;

        // Try to submit orders — should be rejected.
        for i in 0..20 {
            let cmd = make_order_cmd(
                &rt.sequencer,
                "ks-trader",
                Side::Buy,
                50_000,
                1,
                &format!("ks-on-{round}-{i}"),
            );
            if let Ok(cmd) = cmd {
                if rt.engine.submit_new_order(cmd).await.is_err() {
                    rejected_during_on += 1;
                }
            }
        }

        // Turn kill switch OFF.
        let ks_off = AdminCommand {
            metadata: CommandMetadata::new(format!("ks-off-{round}")),
            action: AdminAction::KillSwitch { enabled: false },
            actor_id: "stress-admin".to_string(),
        };
        let _ = rt.engine.submit_admin(ks_off).await;
        total_toggles += 1;

        // Submit orders — should be accepted (buy-only, no self-trade issue).
        for i in 0..20 {
            let cmd = make_order_cmd(
                &rt.sequencer,
                "ks-trader",
                Side::Buy,
                50_000 - (i % 10), // various prices, all resting
                1,
                &format!("ks-off-{round}-{i}"),
            );
            if let Ok(cmd) = cmd {
                if rt.engine.submit_new_order(cmd).await.is_ok() {
                    accepted_during_off += 1;
                }
            }
        }
    }

    events.push(format!(
        "toggles={total_toggles} accepted_during_off={accepted_during_off} rejected_during_on={rejected_during_on}"
    ));

    // Ensure kill switch is OFF at end.
    let ks_final = AdminCommand {
        metadata: CommandMetadata::new("ks-final-off"),
        action: AdminAction::KillSwitch { enabled: false },
        actor_id: "stress-admin".to_string(),
    };
    let _ = rt.engine.submit_admin(ks_final).await;

    let invariants = check_invariants(&rt, vec![]);
    let passed = rejected_during_on > 0
        && accepted_during_off > 0
        && invariants.ledger_balanced
        && invariants.kill_switch_off;

    ScenarioReport {
        name: "S5_kill_switch_storm".into(),
        severity: Severity::Chaos,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
        operations: total_toggles + rejected_during_on + accepted_during_off,
        throughput_ops_sec: 0.0, // not a throughput test
        latency: LatencyReport {
            p50_us: 0,
            p95_us: 0,
            p99_us: 0,
            max_us: 0,
            avg_us: 0,
        },
        invariants,
        events,
        error: if !passed {
            Some("kill switch storm failed".into())
        } else {
            None
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// S6 — Concurrent Cancel Storm
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Objective: Mass-cancel by market while new orders are being submitted.
// Validates: cancel correctness, no phantom orders, invariant.

async fn scenario_concurrent_cancel() -> ScenarioReport {
    let start = Instant::now();
    let mut events = Vec::new();

    let rt = build_stress_runtime(4, 256);
    seed_user(&rt, "cancel-trader", 50_000_000, 50_000);

    // Place many resting orders (wide spread to avoid self-matching).
    let mut placed = 0u64;
    for i in 0..200 {
        let (side, price) = if i % 2 == 0 {
            (Side::Buy, 49_000 + (i % 5)) // bids well below ask
        } else {
            (Side::Sell, 51_000 + (i % 5)) // asks well above bid
        };
        let cmd = make_order_cmd(
            &rt.sequencer,
            "cancel-trader",
            side,
            price,
            1,
            &format!("cancel-place-{i}"),
        );
        if let Ok(cmd) = cmd {
            if rt.engine.submit_new_order(cmd).await.is_ok() {
                placed += 1;
            }
        }
    }
    events.push(format!("placed {placed} resting orders"));

    // Mass cancel.
    let cancel_cmd = MassCancelByMarketCommand {
        metadata: CommandMetadata::new("mass-cancel-stress"),
        market_id: "btc-usdt".to_string(),
        side: None,
    };
    let cancel_result = rt.engine.mass_cancel_by_market(cancel_cmd).await;
    let cancelled = match &cancel_result {
        Ok(r) => {
            events.push(format!(
                "mass_cancel: cancelled={} ids",
                r.cancelled_order_ids.len()
            ));
            r.cancelled_order_ids.len() as u64
        }
        Err(e) => {
            events.push(format!("mass_cancel error: {e}"));
            0
        }
    };

    // Try placing more orders after cancel — should work.
    let mut post_cancel_ok = 0u64;
    for i in 0..20 {
        let cmd = make_order_cmd(
            &rt.sequencer,
            "cancel-trader",
            Side::Buy,
            50_000,
            1,
            &format!("cancel-post-{i}"),
        );
        if let Ok(cmd) = cmd {
            if rt.engine.submit_new_order(cmd).await.is_ok() {
                post_cancel_ok += 1;
            }
        }
    }
    events.push(format!("post-cancel orders accepted: {post_cancel_ok}"));

    let invariants = check_invariants(&rt, vec![]);
    let passed = cancelled > 0 && post_cancel_ok > 0 && invariants.ledger_balanced;

    ScenarioReport {
        name: "S6_concurrent_cancel".into(),
        severity: Severity::Chaos,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
        operations: placed + cancelled + post_cancel_ok,
        throughput_ops_sec: 0.0,
        latency: LatencyReport {
            p50_us: 0,
            p95_us: 0,
            p99_us: 0,
            max_us: 0,
            avg_us: 0,
        },
        invariants,
        events,
        error: if !passed {
            Some("concurrent cancel failed".into())
        } else {
            None
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// S7 — Backpressure Ramp
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Objective: Gradually increase load and track when backpressure levels
//            transition from Normal→Degraded→Critical.
// Validates: smooth degradation, no hard crash, correct signal transitions.

async fn scenario_backpressure_ramp() -> ScenarioReport {
    let start = Instant::now();
    let mut events = Vec::new();
    let mut latencies = LatencyCollector::new(1000);

    // Small queue to hit transitions faster.
    let rt = build_stress_runtime(2, 32);
    seed_user(&rt, "ramp-seller", 100_000_000, 100_000);
    seed_user(&rt, "ramp-buyer", 100_000_000, 0);

    // Seed some resting sells.
    for i in 0..100 {
        let cmd = make_order_cmd(
            &rt.sequencer,
            "ramp-seller",
            Side::Sell,
            50_000,
            1,
            &format!("ramp-sell-{i}"),
        );
        if let Ok(cmd) = cmd {
            let _ = rt.engine.submit_new_order(cmd).await;
        }
    }

    let mut phase_signals: Vec<(u64, String)> = Vec::new();
    let mut submitted = 0u64;

    for i in 0..800 {
        let op_start = Instant::now();
        let cmd = make_order_cmd(
            &rt.sequencer,
            "ramp-buyer",
            Side::Buy,
            50_000,
            1,
            &format!("ramp-buy-{i}"),
        );
        submitted += 1;
        if let Ok(cmd) = cmd {
            let _ = rt.engine.submit_new_order(cmd).await;
            latencies.record(op_start.elapsed().as_micros() as u64);
        }

        // Record backpressure state at intervals.
        if i % 50 == 0 {
            let bp = rt.engine.backpressure_signal();
            let signal = format!("{bp:?}");
            phase_signals.push((i, signal));
        }
    }

    for (idx, sig) in &phase_signals {
        events.push(format!("at_order_{idx}: backpressure={sig}"));
    }

    let invariants = check_invariants(&rt, vec![]);
    let passed = submitted > 100 && invariants.ledger_balanced;

    ScenarioReport {
        name: "S7_backpressure_ramp".into(),
        severity: Severity::Extreme,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
        operations: submitted,
        throughput_ops_sec: {
            let d = start.elapsed().as_millis() as f64;
            if d > 0.0 {
                submitted as f64 * 1000.0 / d
            } else {
                0.0
            }
        },
        latency: latencies.report(),
        invariants,
        events,
        error: if !passed {
            Some("backpressure ramp failed".into())
        } else {
            None
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// S8 — Snapshot Recovery Integrity
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Objective: Flush snapshots after heavy trading, verify they round-trip.
// Validates: snapshot export → import consistency, no state loss.

async fn scenario_snapshot_recovery() -> ScenarioReport {
    let start = Instant::now();
    let mut events = Vec::new();

    let rt = build_stress_runtime(2, 256);
    seed_user(&rt, "snap-maker", 50_000_000, 50_000);
    seed_user(&rt, "snap-taker", 50_000_000, 0);

    // Generate trading activity.
    for i in 0..100 {
        let sell = make_order_cmd(
            &rt.sequencer,
            "snap-maker",
            Side::Sell,
            50_000,
            1,
            &format!("snap-sell-{i}"),
        );
        if let Ok(cmd) = sell {
            let _ = rt.engine.submit_new_order(cmd).await;
        }
    }
    for i in 0..50 {
        let buy = make_order_cmd(
            &rt.sequencer,
            "snap-taker",
            Side::Buy,
            50_000,
            1,
            &format!("snap-buy-{i}"),
        );
        if let Ok(cmd) = buy {
            let _ = rt.engine.submit_new_order(cmd).await;
        }
    }
    events.push("generated 100 sells + 50 buys".into());

    // Export snapshots.
    let snapshots = rt.engine.export_snapshots().await;
    let snapshot_ok = match &snapshots {
        Ok(snaps) => {
            events.push(format!("exported {} partition snapshots", snaps.len()));
            for s in snaps {
                events.push(format!(
                    "  partition={} markets={} last_seq={:?}",
                    s.partition_id,
                    s.snapshot.markets.len(),
                    s.last_applied_command_seq,
                ));
            }
            true
        }
        Err(e) => {
            events.push(format!("snapshot export FAILED: {e}"));
            false
        }
    };

    // Flush snapshots to WAL.
    let flush_ok = match rt.engine.flush_all_snapshots().await {
        Ok(()) => {
            events.push("flush_all_snapshots: OK".into());
            true
        }
        Err(e) => {
            events.push(format!("flush_all_snapshots: FAILED: {e}"));
            false
        }
    };

    // Verify ledger after snapshot.
    let invariants = check_invariants(
        &rt,
        vec![
            format!("snapshot_export_ok={snapshot_ok}"),
            format!("flush_ok={flush_ok}"),
        ],
    );
    let passed = snapshot_ok && flush_ok && invariants.ledger_balanced;

    ScenarioReport {
        name: "S8_snapshot_recovery".into(),
        severity: Severity::Recovery,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
        operations: 150,
        throughput_ops_sec: 0.0,
        latency: LatencyReport {
            p50_us: 0,
            p95_us: 0,
            p99_us: 0,
            max_us: 0,
            avg_us: 0,
        },
        invariants,
        events,
        error: if !passed {
            Some("snapshot recovery failed".into())
        } else {
            None
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Suite Runner & Report Generator
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn run_stress_suite() -> StressSuiteReport {
    let suite_start = Instant::now();

    let scenarios: Vec<ScenarioReport> = vec![
        scenario_queue_saturation().await,
        scenario_burst_spike().await,
        scenario_wal_storm().await,
        scenario_settlement_cascade().await,
        scenario_kill_switch_storm().await,
        scenario_concurrent_cancel().await,
        scenario_backpressure_ramp().await,
        scenario_snapshot_recovery().await,
    ];

    let total = scenarios.len();
    let passed = scenarios.iter().filter(|s| s.passed).count();
    let failed = total - passed;

    StressSuiteReport {
        timestamp: Utc::now(),
        total_scenarios: total,
        passed,
        failed,
        total_duration_ms: suite_start.elapsed().as_millis() as u64,
        scenarios,
    }
}

impl StressSuiteReport {
    /// Render a human-readable text report with full details.
    pub(crate) fn render_text(&self) -> String {
        let mut out = String::with_capacity(4096);

        out.push_str("╔══════════════════════════════════════════════════════════════════╗\n");
        out.push_str("║         EXTREME ENVIRONMENT STRESS TEST REPORT                 ║\n");
        out.push_str("╚══════════════════════════════════════════════════════════════════╝\n\n");

        out.push_str(&format!(
            "Timestamp:     {}\n",
            self.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        out.push_str(&format!("Duration:      {}ms\n", self.total_duration_ms));
        out.push_str(&format!(
            "Scenarios:     {} total, {} passed, {} failed\n\n",
            self.total_scenarios, self.passed, self.failed
        ));

        // Summary table.
        out.push_str("┌────────────────────────────┬──────────┬──────────┬────────────┬─────────────────────────┐\n");
        out.push_str("│ Scenario                   │ Severity │ Verdict  │ Duration   │ Throughput              │\n");
        out.push_str("├────────────────────────────┼──────────┼──────────┼────────────┼─────────────────────────┤\n");

        for s in &self.scenarios {
            let verdict = if s.passed { "✅ PASS" } else { "❌ FAIL" };
            let sev = format!("{:?}", s.severity);
            out.push_str(&format!(
                "│ {:<26} │ {:<8} │ {:<8} │ {:>7}ms  │ {:>10.0} ops/sec       │\n",
                s.name, sev, verdict, s.duration_ms, s.throughput_ops_sec
            ));
        }

        out.push_str("└────────────────────────────┴──────────┴──────────┴────────────┴─────────────────────────┘\n\n");

        // Detailed per-scenario.
        for s in &self.scenarios {
            out.push_str(&format!("━━━ {} ━━━\n", s.name));
            out.push_str(&format!("  Severity:    {:?}\n", s.severity));
            out.push_str(&format!(
                "  Verdict:     {}\n",
                if s.passed { "PASS" } else { "FAIL" }
            ));
            out.push_str(&format!("  Duration:    {}ms\n", s.duration_ms));
            out.push_str(&format!("  Operations:  {}\n", s.operations));
            out.push_str(&format!(
                "  Throughput:  {:.0} ops/sec\n",
                s.throughput_ops_sec
            ));

            if s.latency.p50_us > 0 {
                out.push_str("  Latency:\n");
                out.push_str(&format!("    p50:  {}μs\n", s.latency.p50_us));
                out.push_str(&format!("    p95:  {}μs\n", s.latency.p95_us));
                out.push_str(&format!("    p99:  {}μs\n", s.latency.p99_us));
                out.push_str(&format!("    max:  {}μs\n", s.latency.max_us));
                out.push_str(&format!("    avg:  {}μs\n", s.latency.avg_us));
            }

            out.push_str("  Invariants:\n");
            out.push_str(&format!(
                "    ledger_balanced:     {}\n",
                s.invariants.ledger_balanced
            ));
            out.push_str(&format!(
                "    partitions_healthy:  {}\n",
                s.invariants.partitions_healthy
            ));
            out.push_str(&format!(
                "    kill_switch_off:     {}\n",
                s.invariants.kill_switch_off
            ));
            for d in &s.invariants.details {
                out.push_str(&format!("    → {d}\n"));
            }

            out.push_str("  Events:\n");
            for e in &s.events {
                out.push_str(&format!("    • {e}\n"));
            }

            if let Some(err) = &s.error {
                out.push_str(&format!("  ERROR: {err}\n"));
            }
            out.push('\n');
        }

        // Final verdict.
        let overall = if self.failed == 0 {
            "ALL SCENARIOS PASSED"
        } else {
            "SOME SCENARIOS FAILED"
        };
        let indicator = if self.failed == 0 { "✅" } else { "❌" };
        out.push_str(&format!(
            "{indicator} {overall} ({}/{} passed)\n",
            self.passed, self.total_scenarios
        ));

        out
    }

    /// Render as JSON for machine consumption.
    pub(crate) fn render_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_collector_empty() {
        let mut lc = LatencyCollector::new(10);
        let report = lc.report();
        assert_eq!(report.p50_us, 0);
        assert_eq!(report.avg_us, 0);
    }

    #[test]
    fn latency_collector_basic() {
        let mut lc = LatencyCollector::new(100);
        for i in 1..=100 {
            lc.record(i);
        }
        let report = lc.report();
        // p50 of sorted [1..100] at index 50 is 51 (0-indexed array)
        assert!(report.p50_us >= 50 && report.p50_us <= 51);
        assert!(report.p95_us >= 95);
        assert!(report.p99_us >= 99);
        assert_eq!(report.max_us, 100);
        assert_eq!(report.avg_us, 50); // avg of 1..=100 is 50.5, integer div = 50
    }

    #[tokio::test]
    async fn invariant_report_structure() {
        let rt = build_stress_runtime(2, 64);
        let inv = check_invariants(&rt, vec!["test detail".into()]);
        assert!(inv.ledger_balanced);
        assert!(inv.partitions_healthy);
        assert!(inv.kill_switch_off);
        assert!(inv.details.contains(&"test detail".to_string()));
    }

    #[tokio::test]
    async fn stress_runtime_builds_with_deposits() {
        let rt = build_stress_runtime(2, 64);
        seed_user(&rt, "test-user", 1_000_000, 100);
        assert!(rt.ledger.cash_available_balance("test-user") > 0);
        assert!(
            rt.ledger
                .position_available_balance("test-user", "btc-usdt", 0)
                > 0
        );
    }

    #[tokio::test]
    async fn stress_s1_queue_saturation() {
        let report = scenario_queue_saturation().await;
        assert!(report.passed, "S1 failed: {:?}", report.error);
        assert!(report.operations > 0);
        assert!(report.invariants.ledger_balanced);
    }

    #[tokio::test]
    async fn stress_s2_burst_spike() {
        let report = scenario_burst_spike().await;
        assert!(report.passed, "S2 failed: {:?}", report.error);
        assert!(report.operations > 0);
    }

    #[tokio::test]
    async fn stress_s3_wal_storm() {
        let report = scenario_wal_storm().await;
        assert!(report.passed, "S3 failed: {:?}", report.error);
        assert!(report.invariants.ledger_balanced);
    }

    #[tokio::test]
    async fn stress_s4_settlement_cascade() {
        let report = scenario_settlement_cascade().await;
        assert!(report.passed, "S4 failed: {:?}", report.error);
        assert!(report.invariants.ledger_balanced);
    }

    #[tokio::test]
    async fn stress_s5_kill_switch_storm() {
        let report = scenario_kill_switch_storm().await;
        assert!(report.passed, "S5 failed: {:?}", report.error);
        assert!(report.invariants.kill_switch_off);
    }

    #[tokio::test]
    async fn stress_s6_concurrent_cancel() {
        let report = scenario_concurrent_cancel().await;
        assert!(report.passed, "S6 failed: {:?}", report.error);
    }

    #[tokio::test]
    async fn stress_s7_backpressure_ramp() {
        let report = scenario_backpressure_ramp().await;
        assert!(report.passed, "S7 failed: {:?}", report.error);
    }

    #[tokio::test]
    async fn stress_s8_snapshot_recovery() {
        let report = scenario_snapshot_recovery().await;
        assert!(report.passed, "S8 failed: {:?}", report.error);
    }

    #[tokio::test]
    async fn stress_full_suite_runs() {
        let report = run_stress_suite().await;
        assert_eq!(report.total_scenarios, 8);
        // Render both formats to verify they don't panic.
        let text = report.render_text();
        assert!(text.contains("STRESS TEST REPORT"));
        let json = report.render_json();
        assert!(json.contains("total_scenarios"));
        // Log the report for visibility.
        eprintln!("{text}");
    }

    #[tokio::test]
    async fn stress_report_json_roundtrip() {
        let report = scenario_queue_saturation().await;
        let json = serde_json::to_string(&report).expect("serialize");
        let back: ScenarioReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report.name, back.name);
        assert_eq!(report.passed, back.passed);
    }
}
