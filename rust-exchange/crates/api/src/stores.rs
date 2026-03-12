use super::*;

pub(crate) fn build_funding_rate_store() -> Arc<PersistentFundingRateStore> {
    Arc::new(
        PersistentFundingRateStore::open_jsonl(funding_rates_wal_path())
            .expect("failed to initialize persistent funding rate store"),
    )
}

pub(crate) fn build_risk_automation_audit_store() -> Arc<RiskAutomationAuditStore> {
    Arc::new(
        RiskAutomationAuditStore::open_jsonl(risk_automation_audit_wal_path())
            .expect("failed to initialize risk automation audit store"),
    )
}

pub(crate) fn build_liquidation_queue_store() -> Arc<LiquidationQueueStore> {
    Arc::new(
        LiquidationQueueStore::open_jsonl(liquidation_queue_wal_path())
            .expect("failed to initialize liquidation queue store"),
    )
}

pub(crate) fn build_liquidation_auction_store() -> Arc<LiquidationAuctionStore> {
    Arc::new(
        LiquidationAuctionStore::open_jsonl(liquidation_auction_wal_path())
            .expect("failed to initialize liquidation auction store"),
    )
}

pub(crate) fn build_adl_governance_store() -> Arc<PersistentAdlGovernanceStore> {
    Arc::new(
        PersistentAdlGovernanceStore::open_jsonl(adl_governance_wal_path())
            .expect("failed to initialize ADL governance store"),
    )
}

pub(crate) fn build_liquidation_policy_store() -> Arc<PersistentLiquidationPolicyStore> {
    Arc::new(
        PersistentLiquidationPolicyStore::open_jsonl(liquidation_policy_wal_path())
            .expect("failed to initialize liquidation policy store"),
    )
}

pub(crate) fn build_index_price_store() -> Arc<PersistentIndexPriceStore> {
    Arc::new(
        PersistentIndexPriceStore::open_jsonl(index_price_wal_path())
            .expect("failed to initialize index price store"),
    )
}

pub(crate) fn build_governance_action_store() -> Arc<PendingGovernanceActionStore> {
    Arc::new(
        PendingGovernanceActionStore::open_jsonl(governance_action_wal_path())
            .expect("failed to initialize governance action store"),
    )
}

pub(crate) fn build_instrument_registry() -> Arc<PersistentInstrumentRegistry> {
    let registry = Arc::new(
        PersistentInstrumentRegistry::open_jsonl(instruments_registry_wal_path())
            .expect("failed to initialize persistent instrument registry"),
    );
    if registry.is_empty() {
        seed_default_instruments(&registry);
    }
    registry
}
