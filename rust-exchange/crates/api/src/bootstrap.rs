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
    pub(crate) governance_actions: Arc<PendingGovernanceActionStore>,
    pub(crate) partitioned_engine: Arc<PartitionedMatchingEngine>,
    pub(crate) trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>>,
}

pub(crate) struct AutomationRuntime {
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
}

pub(crate) async fn bootstrap_runtime(event_bus: EventBus) -> AppBootstrap {
    let ledger_wal_path = ledger_wal_path();
    let ledger_wal = Arc::new(
        JsonlFileWal::<LedgerDelta>::new(&ledger_wal_path)
            .expect("failed to initialize ledger WAL file"),
    );
    let ledger = Arc::new(LedgerService::with_wal_store(event_bus.clone(), ledger_wal));
    ledger
        .recover_from_wal()
        .expect("failed to recover ledger state from WAL");

    let sequencer_wal_path = sequencer_wal_path();
    let sequencer_wal = Arc::new(
        JsonlFileWal::<SequencedCommandRecord>::new(&sequencer_wal_path)
            .expect("failed to initialize sequencer WAL file"),
    );
    let sequencer = Arc::new(Sequencer::with_wal(1, sequencer_wal));
    sequencer
        .recover_from_wal()
        .expect("failed to recover sequencer state from WAL");

    let matching_snapshot_wal_path = matching_snapshot_wal_path();
    let matching_snapshot_wal: Arc<dyn persistence::WalStore<PartitionSnapshotRecord>> = Arc::new(
        JsonlFileWal::<PartitionSnapshotRecord>::new(&matching_snapshot_wal_path)
            .expect("failed to initialize matching snapshot WAL file"),
    );
    let trade_journal_wal_path = trade_journal_wal_path();
    let trade_journal_wal: Arc<dyn persistence::WalStore<TradeJournalRecord>> = Arc::new(
        JsonlFileWal::<TradeJournalRecord>::new(&trade_journal_wal_path)
            .expect("failed to initialize trade journal WAL file"),
    );

    tracing::info!(
        "WAL initialized: ledger={}, sequencer={}, matching_snapshot={}, trade_journal={}",
        ledger_wal_path,
        sequencer_wal_path,
        matching_snapshot_wal_path,
        trade_journal_wal_path,
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
    let governance_actions = build_governance_action_store();
    let engine_instrument_registry: Arc<dyn InstrumentRegistry> = instruments.clone();

    let partitioned_engine = Arc::new(
        PartitionedMatchingEngine::with_stores_and_registry(
            PartitionedEngineConfig::default(),
            event_bus,
            risk.clone(),
            engine_instrument_registry,
            Some(matching_snapshot_wal),
            Some(trade_journal_wal.clone()),
        )
        .expect("failed to initialize partitioned matching engine with snapshot recovery"),
    );

    replay_commands_after_snapshot(partitioned_engine.as_ref(), sequencer.as_ref()).await;

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
        governance_actions,
        partitioned_engine,
        trade_journal_wal,
    }
}

pub(crate) fn spawn_automation_tasks(runtime: AutomationRuntime) {
    if !automation_enabled() {
        tracing::info!(
            "risk automation disabled; set RISK_AUTOMATION_ENABLED=true to enable schedulers"
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
    ));
    tokio::spawn(run_liquidation_worker_scheduler(
        runtime.risk.clone(),
        runtime.instruments.clone(),
        runtime.risk_automation_audit.clone(),
        runtime.liquidation_queue.clone(),
        runtime.liquidation_auction.clone(),
        runtime.adl_governance.clone(),
        runtime.liquidation_policy.clone(),
    ));
    tokio::spawn(run_funding_scheduler(
        runtime.partitioned_engine,
        runtime.risk,
        runtime.funding_rates,
        runtime.index_prices,
        runtime.risk_automation_audit,
    ));
}

async fn replay_commands_after_snapshot(
    partitioned_engine: &PartitionedMatchingEngine,
    sequencer: &Sequencer,
) {
    let mut partition_snapshot_seqs: HashMap<usize, u64> = partitioned_engine
        .export_snapshots()
        .await
        .expect("failed to export partition snapshots for replay boundary")
        .into_iter()
        .map(|record| {
            (
                record.partition_id,
                record.last_applied_command_seq.unwrap_or(0),
            )
        })
        .collect();

    for record in sequencer.latest_records().into_iter() {
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
        partitioned_engine
            .replay_command(record.command)
            .await
            .expect("failed to replay sequenced command after snapshot");
        for partition in replay_partitions {
            partition_snapshot_seqs.insert(partition, record.command_seq);
        }
    }
}
