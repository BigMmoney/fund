use super::*;

pub(crate) struct AppBootstrap {
    pub(crate) ledger: Arc<LedgerService>,
    pub(crate) sequencer: Arc<Sequencer>,
    pub(crate) risk: Arc<RiskEngine>,
    pub(crate) instruments: Arc<PersistentInstrumentRegistry>,
    pub(crate) funding_rates: Arc<PersistentFundingRateStore>,
    pub(crate) risk_automation_audit: Arc<RiskAutomationAuditStore>,
    pub(crate) liquidation_queue: Arc<LiquidationQueueStore>,
    pub(crate) liquidation_auction: Arc<LiquidationAuctionStore>,
    pub(crate) adl_governance: Arc<PersistentAdlGovernanceStore>,
    pub(crate) liquidation_policy: Arc<PersistentLiquidationPolicyStore>,
    pub(crate) index_prices: Arc<PersistentIndexPriceStore>,
    pub(crate) position_costs: Arc<PositionCostLedgerStore>,
    pub(crate) governance_actions: Arc<PendingGovernanceActionStore>,
    pub(crate) partitioned_engine: Arc<PartitionedMatchingEngine>,
    pub(crate) trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
    pub(crate) trade_settlement_wal: Arc<dyn persistence::WalStore<TradeSettlementRecord>>,
}

pub(crate) struct AutomationRuntime {
    pub(crate) ledger: Arc<LedgerService>,
    pub(crate) partitioned_engine: Arc<PartitionedMatchingEngine>,
    pub(crate) risk: Arc<RiskEngine>,
    pub(crate) instruments: Arc<PersistentInstrumentRegistry>,
    pub(crate) funding_rates: Arc<PersistentFundingRateStore>,
    pub(crate) risk_automation_audit: Arc<RiskAutomationAuditStore>,
    pub(crate) liquidation_queue: Arc<LiquidationQueueStore>,
    pub(crate) liquidation_auction: Arc<LiquidationAuctionStore>,
    pub(crate) adl_governance: Arc<PersistentAdlGovernanceStore>,
    pub(crate) liquidation_policy: Arc<PersistentLiquidationPolicyStore>,
    pub(crate) index_prices: Arc<PersistentIndexPriceStore>,
    pub(crate) position_costs: Arc<PositionCostLedgerStore>,
    pub(crate) trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
    pub(crate) ws_hub: Arc<WsHub>,
    pub(crate) system_sentinel: Arc<sentinel::SystemSentinel>,
}

pub(crate) async fn bootstrap_runtime(event_bus: EventBus) -> AppBootstrap {
    let wal_rotation_max: u64 = cfg().wal.rotation_max_entries;
    let wal_group_commit: u64 = cfg().wal.group_commit_size;

    // Ensure the data directory exists.
    let data_dir = &cfg().wal.data_dir;
    if !std::path::Path::new(data_dir).exists() {
        std::fs::create_dir_all(data_dir)
            .unwrap_or_else(|e| panic!("failed to create data directory '{data_dir}': {e}"));
    }

    let ledger_wal_path = ledger_wal_path();
    let ledger_wal = Arc::new(
        JsonlFileWal::<LedgerDelta>::with_rotation(&ledger_wal_path, wal_rotation_max)
            .unwrap_or_else(|e| {
                panic!("failed to initialize ledger WAL at '{ledger_wal_path}': {e}")
            })
            .with_group_commit(wal_group_commit),
    );
    let ledger = Arc::new(LedgerService::with_wal_store(event_bus.clone(), ledger_wal));
    if let Err(e) = ledger.recover_from_wal() {
        panic!("FATAL: ledger WAL recovery failed — refusing to start with empty state (wal={ledger_wal_path}): {e}");
    }
    seed_demo_balances(&ledger);
    seed_demo_inventory(&ledger);

    let sequencer_wal_path = sequencer_wal_path();
    let sequencer_wal = Arc::new(
        JsonlFileWal::<SequencedCommandRecord>::with_rotation(
            &sequencer_wal_path,
            wal_rotation_max,
        )
        .unwrap_or_else(|e| {
            panic!("failed to initialize sequencer WAL at '{sequencer_wal_path}': {e}")
        })
        .with_group_commit(wal_group_commit),
    );
    // Order Flow Monitor: wire an observer-only trace emitter into the
    // sequencer so `sequencer_accepted` and `sequencer_persisted` flow to
    // the eventbus `order.trace` channel. Failures emit nothing.
    let trace_emitter: Arc<dyn types::TraceEmitter> =
        Arc::new(monitor::EventBusTraceEmitter::new(event_bus.clone()));
    let sequencer = Arc::new(Sequencer::with_wal_and_emitter(
        1,
        sequencer_wal,
        Some(trace_emitter),
    ));
    if let Err(e) = sequencer.recover_from_wal() {
        panic!("FATAL: sequencer WAL recovery failed — refusing to start with empty state (wal={sequencer_wal_path}): {e}");
    }

    let matching_snapshot_wal_path = matching_snapshot_wal_path();
    let matching_snapshot_wal: Arc<dyn persistence::WalStore<PartitionSnapshotRecord>> = Arc::new(
        JsonlFileWal::<PartitionSnapshotRecord>::with_rotation(
            &matching_snapshot_wal_path,
            wal_rotation_max,
        )
        .unwrap_or_else(|e| {
            panic!(
                "failed to initialize matching snapshot WAL at '{matching_snapshot_wal_path}': {e}"
            )
        })
        .with_group_commit(wal_group_commit),
    );
    let trade_journal_wal_path = trade_journal_wal_path();
    let trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>> = Arc::new(
        JsonlFileWal::<TradeJournalRecord>::with_rotation(
            &trade_journal_wal_path,
            wal_rotation_max,
        )
        .unwrap_or_else(|e| {
            panic!("failed to initialize trade journal WAL at '{trade_journal_wal_path}': {e}")
        })
        .with_group_commit(wal_group_commit),
    );
    let trade_settlement_wal_path = trade_settlement_wal_path();
    let trade_settlement_wal: Arc<dyn persistence::WalStore<TradeSettlementRecord>> = Arc::new(
        JsonlFileWal::<TradeSettlementRecord>::with_rotation(
            &trade_settlement_wal_path,
            wal_rotation_max,
        )
        .unwrap_or_else(|e| {
            panic!(
                "failed to initialize trade settlement WAL at '{trade_settlement_wal_path}': {e}"
            )
        })
        .with_group_commit(wal_group_commit),
    );

    tracing::info!(
        "WAL initialized: ledger={}, sequencer={}, matching_snapshot={}, trade_journal={}, trade_settlement={}, rotation_max={}, group_commit={}",
        ledger_wal_path,
        sequencer_wal_path,
        matching_snapshot_wal_path,
        trade_journal_wal_path,
        trade_settlement_wal_path,
        if wal_rotation_max == 0 { "disabled".to_string() } else { wal_rotation_max.to_string() },
        if wal_group_commit == 0 { "off (sync every append)".to_string() } else { format!("every {wal_group_commit} appends") },
    );

    let risk = Arc::new(RiskEngine::new(ledger.clone()));
    let instruments = build_instrument_registry();
    let funding_rates = build_funding_rate_store();
    let risk_automation_audit = build_risk_automation_audit_store();
    let liquidation_queue = build_liquidation_queue_store();
    let liquidation_auction = build_liquidation_auction_store();
    let adl_governance = build_adl_governance_store();
    let liquidation_policy = build_liquidation_policy_store();
    let index_prices = build_index_price_store();
    let position_costs = build_position_cost_store();
    let governance_actions = build_governance_action_store();
    let engine_instrument_registry: Arc<dyn InstrumentRegistry> = instruments.clone();

    // Configure matching engine with tuned snapshot interval from config
    let engine_config = PartitionedEngineConfig {
        snapshot_interval_commands: cfg().wal.snapshot_interval_commands as usize,
        ..PartitionedEngineConfig::default()
    };

    // Clone the event_bus before handing ownership to the engine, so the
    // recovery emit (Step 9 — recovery_completed aggregate event) can
    // publish onto the same channel.
    let event_bus_for_replay = event_bus.clone();
    let partitioned_engine = Arc::new(
        PartitionedMatchingEngine::with_stores_registry_costs_and_settlements(
            engine_config,
            event_bus,
            risk.clone(),
            engine_instrument_registry,
            Some(matching_snapshot_wal),
            Some(trade_journal_wal.clone()),
            Some(position_costs.clone()),
            Some(trade_settlement_wal.clone()),
        )
        .unwrap_or_else(|e| panic!("failed to initialize partitioned matching engine: {e}")),
    );

    replay_commands_after_snapshot(
        partitioned_engine.as_ref(),
        sequencer.as_ref(),
        &event_bus_for_replay,
    )
    .await
    .unwrap_or_else(|e| panic!("FATAL: command replay after snapshot failed — cannot guarantee matching engine consistency: {e}"));
    if let Err(e) = position_costs.sync_from_trade_journal(trade_journal_wal.as_ref()) {
        tracing::error!(error = %e, "failed to recover position cost ledger from trade journal — costs may be stale");
    }

    // Startup self-test: verify critical invariants after recovery.
    match ledger.verify_global_invariant() {
        Ok(()) => tracing::info!("startup self-test passed: ledger balance invariant OK"),
        Err(e) => {
            tracing::error!(error = %e, "CRITICAL: ledger balance invariant violated at startup — proceeding with caution")
        }
    }

    AppBootstrap {
        ledger,
        sequencer,
        risk,
        instruments,
        funding_rates,
        risk_automation_audit,
        liquidation_queue,
        liquidation_auction,
        adl_governance,
        liquidation_policy,
        index_prices,
        position_costs,
        governance_actions,
        partitioned_engine,
        trade_journal_wal,
        trade_settlement_wal,
    }
}

pub(crate) fn spawn_automation_tasks(runtime: AutomationRuntime) {
    tokio::spawn(run_position_cost_resync_scheduler(
        runtime.position_costs.clone(),
        runtime.trade_journal_wal.clone(),
    ));
    tokio::spawn(run_invariant_check_scheduler(
        runtime.ledger.clone(),
        runtime.system_sentinel.clone(),
    ));

    if !automation_enabled() {
        tracing::info!(
            "risk automation disabled; position-cost resync stays enabled, set RISK_AUTOMATION_ENABLED=true to enable liquidation/funding schedulers"
        );
        return;
    }

    tracing::info!("risk automation enabled; starting liquidation and funding schedulers");
    tokio::spawn(run_liquidation_scheduler(
        runtime.partitioned_engine.clone(),
        runtime.risk.clone(),
        runtime.instruments.clone(),
        runtime.index_prices.clone(),
        runtime.risk_automation_audit.clone(),
        runtime.liquidation_queue.clone(),
        runtime.adl_governance.clone(),
        runtime.position_costs.clone(),
        runtime.trade_journal_wal.clone(),
        runtime.ws_hub.clone(),
        runtime.system_sentinel.clone(),
    ));
    tokio::spawn(run_liquidation_worker_scheduler(
        runtime.partitioned_engine.clone(),
        runtime.risk.clone(),
        runtime.instruments.clone(),
        runtime.index_prices.clone(),
        runtime.risk_automation_audit.clone(),
        runtime.liquidation_queue.clone(),
        runtime.liquidation_auction.clone(),
        runtime.adl_governance.clone(),
        runtime.liquidation_policy.clone(),
        runtime.position_costs.clone(),
        runtime.trade_journal_wal.clone(),
    ));
    tokio::spawn(run_funding_scheduler(
        runtime.partitioned_engine,
        runtime.risk,
        runtime.instruments,
        runtime.funding_rates,
        runtime.index_prices,
        runtime.risk_automation_audit,
    ));
}

fn seed_demo_balances(ledger: &Arc<LedgerService>) {
    // Demo balances are always seeded on first boot for dev/test environments.
    // The env var is kept for backwards compatibility but defaults to ON.
    if std::env::var("SKIP_DEMO_DATA_SEEDING").as_deref() == Ok("1") {
        tracing::info!("demo balance seeding skipped via SKIP_DEMO_DATA_SEEDING=1");
        return;
    }
    let demo_accounts = [
        ("trader", 1_000_000_i64, "seed-demo-trader-usdc"),
        ("admin", 5_000_000_i64, "seed-demo-admin-usdc"),
        ("viewer", 250_000_i64, "seed-demo-viewer-usdc"),
    ];

    for (user_id, amount, op_id) in demo_accounts {
        if ledger.cash_available_balance(user_id) > 0 {
            continue;
        }

        let _ = ledger.process_deposit(user_id, amount, op_id.to_string());
    }
}

fn seed_demo_inventory(ledger: &Arc<LedgerService>) {
    // Demo inventory is always seeded on first boot for dev/test environments.
    if std::env::var("SKIP_DEMO_DATA_SEEDING").as_deref() == Ok("1") {
        return;
    }
    let demo_positions = [
        ("trader", "btc-usdt", 0, 25_i64, "seed-demo-trader-btc-spot"),
        ("viewer", "btc-usdt", 0, 25_i64, "seed-demo-viewer-btc-spot"),
        (
            "trader",
            "eth-usdt",
            0,
            120_i64,
            "seed-demo-trader-eth-spot",
        ),
        (
            "viewer",
            "eth-usdt",
            0,
            120_i64,
            "seed-demo-viewer-eth-spot",
        ),
    ];

    for (user_id, market_id, outcome, amount, op_id) in demo_positions {
        if ledger.position_available_balance(user_id, market_id, outcome) > 0 {
            continue;
        }

        let _ =
            ledger.process_position_deposit(user_id, market_id, outcome, amount, op_id.to_string());
    }
}

async fn run_position_cost_resync_scheduler(
    position_costs: Arc<PositionCostLedgerStore>,
    trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
) {
    let interval_ms = cfg().risk.position_cost_resync_interval_ms;
    if interval_ms == 0 {
        tracing::info!("position cost resync scheduler disabled");
        return;
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        if let Err(error) = position_costs.sync_from_trade_journal(trade_journal_wal.as_ref()) {
            tracing::error!(error = %error, "position cost resync failed");
        }
    }
}

async fn replay_commands_after_snapshot(
    partitioned_engine: &PartitionedMatchingEngine,
    sequencer: &Sequencer,
    event_bus: &EventBus,
) -> anyhow::Result<()> {
    let mut partition_snapshot_seqs: HashMap<usize, u64> = partitioned_engine
        .export_snapshots()
        .await?
        .into_iter()
        .map(|record| {
            (
                record.partition_id,
                record.last_applied_command_seq.unwrap_or(0),
            )
        })
        .collect();

    // Per-record recovery events are gated on MONITOR_TRACE_RECOVERY_DETAIL=1
    // (design §3.6). Evaluated once up front so the per-record cost when
    // the flag is unset is a single boolean test, not an env_var lookup.
    let detail_emit = std::env::var("MONITOR_TRACE_RECOVERY_DETAIL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let started_at = std::time::Instant::now();
    let mut replayed_count: u64 = 0;
    let mut skipped_terminal_count: u64 = 0;
    let mut total_seen: u64 = 0;
    let mut highest_command_seq: u64 = 0;

    for record in sequencer.latest_records().into_iter() {
        total_seen += 1;
        if record.command_seq > highest_command_seq {
            highest_command_seq = record.command_seq;
        }

        if should_skip_replay_record(&record) {
            tracing::info!(
                command_seq = record.command_seq,
                request_id = %record.request_id,
                lifecycle = ?record.command.metadata().lifecycle,
                "skipping replay for terminal non-applying command"
            );
            skipped_terminal_count += 1;
            if detail_emit {
                emit_recovery_per_record(
                    event_bus,
                    types::OrderTraceStage::RecoverySkippedTerminal,
                    &record,
                );
            }
            continue;
        }

        let replay_partitions = partitioned_engine
            .partitions_for_command(&record.command)
            .into_iter()
            .filter(|partition| {
                record.command_seq > partition_snapshot_seqs.get(partition).copied().unwrap_or(0)
            })
            .collect::<Vec<_>>();
        if replay_partitions.is_empty() {
            continue;
        }

        tracing::info!(
            command_seq = record.command_seq,
            request_id = %record.request_id,
            "replaying sequenced command after snapshot"
        );
        if detail_emit {
            emit_recovery_per_record(
                event_bus,
                types::OrderTraceStage::RecoveryReplayed,
                &record,
            );
        }
        if let Err(error) = partitioned_engine
            .replay_command(record.command.clone())
            .await
        {
            anyhow::bail!(
                "failed to replay sequenced command after snapshot: seq={} request_id={} lifecycle={:?} error={}",
                record.command_seq,
                record.request_id,
                record.command.metadata().lifecycle,
                error
            );
        }
        replayed_count += 1;
        for partition in replay_partitions {
            partition_snapshot_seqs.insert(partition, record.command_seq);
        }
    }

    // Always emit recovery_completed once on the success path. Aggregate
    // counts go in `detail`; per-record events do not (design §3.6).
    let duration_ms = started_at.elapsed().as_millis() as u64;
    let mut ev = types::OrderTraceEvent::new_unbound(types::OrderTraceStage::RecoveryCompleted);
    ev.detail = serde_json::json!({
        "replayed_count": replayed_count,
        "skipped_terminal_count": skipped_terminal_count,
        "total_seen": total_seen,
        "duration_ms": duration_ms,
        "highest_command_seq": highest_command_seq,
        "detail_emit_enabled": detail_emit,
    });
    event_bus.publish(types::Event::OrderTrace(ev));

    tracing::info!(
        replayed_count,
        skipped_terminal_count,
        total_seen,
        duration_ms,
        highest_command_seq,
        detail_emit,
        "command replay after snapshot complete"
    );

    Ok(())
}

/// Build and publish a per-record recovery event. Called only when the
/// MONITOR_TRACE_RECOVERY_DETAIL env var is set; the JSONL writer drops
/// these events regardless (design §3.6), so they live on the broadcast
/// channel only and are safe to drop under lag.
fn emit_recovery_per_record(
    event_bus: &EventBus,
    stage: types::OrderTraceStage,
    record: &SequencedCommandRecord,
) {
    let mut ev = types::OrderTraceEvent::new_unbound(stage);
    ev.request_id = Some(record.request_id.clone());
    ev.command_seq = Some(record.command_seq);
    ev.lifecycle = Some(record.command.metadata().lifecycle);
    event_bus.publish(types::Event::OrderTrace(ev));
}

fn should_skip_replay_record(record: &SequencedCommandRecord) -> bool {
    // Skip lifecycles whose effects are already reflected in the ledger WAL
    // (recovered via `LedgerService::recover_from_wal` before replay starts).
    // Re-running these through `submit_new_order` would either double-debit
    // the ledger or fail preflight reservation because available cash already
    // reflects the post-settlement state.
    matches!(
        record.command.metadata().lifecycle,
        types::CommandLifecycle::Rejected
            | types::CommandLifecycle::Cancelled
            | types::CommandLifecycle::Settled
            | types::CommandLifecycle::Completed
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use types::{
        Command, CommandLifecycle, CommandMetadata, NewOrderCommand, OrderType, Side, TimeInForce,
    };

    fn sequenced_record(lifecycle: CommandLifecycle) -> SequencedCommandRecord {
        let mut metadata = CommandMetadata::new("bootstrap-replay-test");
        metadata.command_seq = Some(42);
        metadata.lifecycle = lifecycle;
        let command = Command::NewOrder(NewOrderCommand {
            metadata,
            client_order_id: "coid-bootstrap".to_string(),
            user_id: "user-1".to_string(),
            session_id: Some("session-1".to_string()),
            market_id: "btc-usdt".to_string(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: Some(100),
            amount: 1,
            outcome: 0,
            post_only: false,
            reduce_only: false,
            leverage: None,
            expires_at: None,
            stp_mode: types::StpMode::default(),
            trigger_price: None,
            trigger_type: None,
            display_qty: None,
            min_fill_qty: None,
            stp_group_id: None,
            is_market_maker: false,
        });

        SequencedCommandRecord {
            request_id: "bootstrap-replay-test".to_string(),
            command_seq: 42,
            command,
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn replay_skips_terminal_settled_records() {
        for lifecycle in [
            CommandLifecycle::Rejected,
            CommandLifecycle::Cancelled,
            CommandLifecycle::Settled,
            CommandLifecycle::Completed,
        ] {
            assert!(
                should_skip_replay_record(&sequenced_record(lifecycle)),
                "expected {lifecycle:?} to be skipped during replay"
            );
        }
        for lifecycle in [
            CommandLifecycle::Received,
            CommandLifecycle::Sequenced,
            CommandLifecycle::WalAppended,
            CommandLifecycle::Routed,
            CommandLifecycle::PartitionAccepted,
            CommandLifecycle::RiskReserved,
            CommandLifecycle::Executed,
        ] {
            assert!(
                !should_skip_replay_record(&sequenced_record(lifecycle)),
                "expected {lifecycle:?} to be replayed"
            );
        }
    }
}
