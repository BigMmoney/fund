use chrono::{DateTime, Utc};
use dashmap::DashMap;
use persistence::{JsonlFileWal, WalStore};
use std::sync::Arc;
use types::{InstrumentKind, InstrumentSpec, InstrumentStatus, MarginMode};

pub trait InstrumentRegistry: Send + Sync {
    fn get(&self, market_id: &str) -> Option<InstrumentSpec>;

    fn resolve(&self, market_id: &str) -> InstrumentSpec {
        self.get(market_id)
            .unwrap_or_else(|| fallback_spec_for_market(market_id))
    }
}

#[derive(Default)]
pub struct InMemoryInstrumentRegistry {
    specs: DashMap<String, InstrumentSpec>,
}

impl InMemoryInstrumentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, spec: InstrumentSpec) {
        self.specs.insert(spec.instrument_id.clone(), spec);
    }

    pub fn with_spec(self, spec: InstrumentSpec) -> Self {
        self.register(spec);
        self
    }
}

impl InstrumentRegistry for InMemoryInstrumentRegistry {
    fn get(&self, market_id: &str) -> Option<InstrumentSpec> {
        self.specs.get(market_id).map(|entry| entry.clone())
    }
}

pub fn shared_default_registry() -> Arc<dyn InstrumentRegistry> {
    Arc::new(InMemoryInstrumentRegistry::new())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstrumentRegistryRecord {
    pub spec: InstrumentSpec,
    pub recorded_at: DateTime<Utc>,
}

pub struct PersistentInstrumentRegistry {
    specs: DashMap<String, InstrumentSpec>,
    store: Arc<dyn WalStore<InstrumentRegistryRecord>>,
}

impl PersistentInstrumentRegistry {
    pub fn new(store: Arc<dyn WalStore<InstrumentRegistryRecord>>) -> anyhow::Result<Self> {
        let registry = Self {
            specs: DashMap::new(),
            store,
        };
        for record in registry.store.entries()? {
            registry
                .specs
                .insert(record.spec.instrument_id.clone(), record.spec);
        }
        Ok(registry)
    }

    pub fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn WalStore<InstrumentRegistryRecord>> = Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub fn upsert(&self, spec: InstrumentSpec) -> anyhow::Result<()> {
        if spec.tick_size <= 0 {
            anyhow::bail!("tick_size must be positive, got {}", spec.tick_size);
        }
        if spec.lot_size <= 0 {
            anyhow::bail!("lot_size must be positive, got {}", spec.lot_size);
        }
        if spec.price_band_bps < 0 || spec.price_band_bps > 10_000 {
            anyhow::bail!(
                "price_band_bps must be in [0, 10000], got {}",
                spec.price_band_bps
            );
        }
        if spec.maintenance_margin_bps < 0 || spec.maintenance_margin_bps > 10_000 {
            anyhow::bail!(
                "maintenance_margin_bps must be in [0, 10000], got {}",
                spec.maintenance_margin_bps
            );
        }
        if spec.maker_fee_bps < -500 || spec.maker_fee_bps > 10_000 {
            anyhow::bail!(
                "maker_fee_bps must be in [-500, 10000], got {}",
                spec.maker_fee_bps
            );
        }
        if spec.taker_fee_bps < -500 || spec.taker_fee_bps > 10_000 {
            anyhow::bail!(
                "taker_fee_bps must be in [-500, 10000], got {}",
                spec.taker_fee_bps
            );
        }
        // Insert in-memory first, then persist. If WAL fails, rollback.
        let id = spec.instrument_id.clone();
        let previous = self.specs.insert(id.clone(), spec.clone());
        if let Err(e) = self.store.append(&InstrumentRegistryRecord {
            spec,
            recorded_at: Utc::now(),
        }) {
            // Rollback: restore previous value or remove
            match previous {
                Some(prev) => {
                    self.specs.insert(id, prev);
                }
                None => {
                    self.specs.remove(&id);
                }
            }
            return Err(e);
        }
        Ok(())
    }

    pub fn list(&self) -> Vec<InstrumentSpec> {
        let mut specs: Vec<_> = self
            .specs
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        specs.sort_by(|lhs, rhs| lhs.instrument_id.cmp(&rhs.instrument_id));
        specs
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }
}

impl InstrumentRegistry for PersistentInstrumentRegistry {
    fn get(&self, market_id: &str) -> Option<InstrumentSpec> {
        self.specs.get(market_id).map(|entry| entry.clone())
    }
}

pub fn infer_instrument_kind(market_id: &str) -> InstrumentKind {
    if market_id.starts_with("margin:") {
        InstrumentKind::Margin
    } else if market_id.starts_with("perp:") || market_id.starts_with("perpetual:") {
        InstrumentKind::Perpetual
    } else if market_id.starts_with("future:") {
        InstrumentKind::Future
    } else if market_id.starts_with("option:") {
        InstrumentKind::Option
    } else {
        InstrumentKind::Spot
    }
}

pub fn fallback_spec_for_market(market_id: &str) -> InstrumentSpec {
    let kind = infer_instrument_kind(market_id);
    InstrumentSpec {
        instrument_id: market_id.to_string(),
        kind,
        base_asset: String::new(),
        quote_asset: "USDC".to_string(),
        margin_mode: if kind.is_derivative() {
            Some(MarginMode::Cross)
        } else {
            None
        },
        max_leverage: if kind.is_derivative() { Some(20) } else { None },
        tick_size: 1,
        lot_size: 1,
        price_band_bps: 1_000,
        risk_policy_id: match kind {
            InstrumentKind::Spot => "spot-v1".to_string(),
            InstrumentKind::Margin => "margin-v1".to_string(),
            InstrumentKind::Perpetual => "perpetual-v1".to_string(),
            InstrumentKind::Future => "future-v1".to_string(),
            InstrumentKind::Option => "option-v1".to_string(),
        },
        min_order_amount: 0,
        max_notional: 0,
        maker_fee_bps: 2,
        taker_fee_bps: 5,
        max_position_notional: 0,
        maintenance_margin_bps: 0,
        contract_multiplier: 1,
        funding_interval_secs: if kind == InstrumentKind::Perpetual {
            28800
        } else {
            0
        },
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::InMemoryWal;

    #[test]
    fn fallback_registry_resolves_spot_by_default() {
        let spec = fallback_spec_for_market("btc-usdt");
        assert_eq!(spec.kind, InstrumentKind::Spot);
        assert_eq!(spec.max_leverage, None);
    }

    #[test]
    fn fallback_registry_resolves_margin_and_perpetual() {
        let margin = fallback_spec_for_market("margin:btc-usdt");
        let perp = fallback_spec_for_market("perp:btc-usdt");

        assert_eq!(margin.kind, InstrumentKind::Margin);
        assert_eq!(margin.margin_mode, Some(MarginMode::Cross));
        assert_eq!(perp.kind, InstrumentKind::Perpetual);
        assert_eq!(perp.max_leverage, Some(20));
    }

    #[test]
    fn fallback_registry_resolves_future_and_option() {
        let future = fallback_spec_for_market("future:btc-usdt:202606");
        let option = fallback_spec_for_market("option:btc-usdt:call-70000:202606");

        assert_eq!(future.kind, InstrumentKind::Future);
        assert_eq!(future.margin_mode, Some(MarginMode::Cross));
        assert_eq!(future.max_leverage, Some(20));
        assert_eq!(option.kind, InstrumentKind::Option);
        assert_eq!(option.risk_policy_id, "option-v1");
    }

    #[test]
    fn in_memory_registry_overrides_fallback() {
        let registry = InMemoryInstrumentRegistry::new();
        registry.register(InstrumentSpec {
            instrument_id: "btc-usdt".to_string(),
            kind: InstrumentKind::Margin,
            base_asset: String::new(),
            quote_asset: "USDC".to_string(),
            margin_mode: Some(MarginMode::Cross),
            max_leverage: Some(5),
            tick_size: 5,
            lot_size: 10,
            price_band_bps: 500,
            risk_policy_id: "custom-margin".to_string(),
            min_order_amount: 0,
            max_notional: 0,
            maker_fee_bps: 0,
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
        });

        let spec = registry.resolve("btc-usdt");
        assert_eq!(spec.max_leverage, Some(5));
        assert_eq!(spec.margin_mode, Some(MarginMode::Cross));
    }

    #[test]
    fn persistent_registry_rebuilds_from_store() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store.clone()).unwrap();
        registry
            .upsert(InstrumentSpec {
                instrument_id: "perp:btc-usdt".to_string(),
                kind: InstrumentKind::Perpetual,
                base_asset: String::new(),
                quote_asset: "USDC".to_string(),
                margin_mode: Some(MarginMode::Isolated),
                max_leverage: Some(20),
                tick_size: 1,
                lot_size: 1,
                price_band_bps: 1000,
                risk_policy_id: "perp-v1".to_string(),
                min_order_amount: 0,
                max_notional: 0,
                maker_fee_bps: 0,
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
            })
            .unwrap();

        let reloaded = PersistentInstrumentRegistry::new(store).unwrap();
        let spec = reloaded.resolve("perp:btc-usdt");
        assert_eq!(spec.kind, InstrumentKind::Perpetual);
        assert_eq!(reloaded.list().len(), 1);
    }

    #[test]
    fn upsert_rejects_too_negative_maker_fee_bps() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store).unwrap();
        let mut spec = fallback_spec_for_market("btc-usdt");
        spec.maker_fee_bps = -501;
        assert!(registry.upsert(spec).is_err());
    }

    #[test]
    fn upsert_rejects_excessive_taker_fee_bps() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store).unwrap();
        let mut spec = fallback_spec_for_market("btc-usdt");
        spec.taker_fee_bps = 10_001;
        assert!(registry.upsert(spec).is_err());
    }

    #[test]
    fn upsert_accepts_valid_fee_bps() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store).unwrap();
        let mut spec = fallback_spec_for_market("btc-usdt");
        spec.maker_fee_bps = 0;
        spec.taker_fee_bps = 10_000;
        assert!(registry.upsert(spec).is_ok());
    }

    #[test]
    fn upsert_rejects_zero_tick_size() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store).unwrap();
        let mut spec = fallback_spec_for_market("btc-usdt");
        spec.tick_size = 0;
        assert!(registry.upsert(spec).is_err());
    }

    #[test]
    fn upsert_rejects_negative_lot_size() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store).unwrap();
        let mut spec = fallback_spec_for_market("btc-usdt");
        spec.lot_size = -1;
        assert!(registry.upsert(spec).is_err());
    }

    #[test]
    fn upsert_rejects_excessive_price_band_bps() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store).unwrap();
        let mut spec = fallback_spec_for_market("btc-usdt");
        spec.price_band_bps = 10_001;
        assert!(registry.upsert(spec).is_err());
    }

    #[test]
    fn upsert_rejects_negative_maintenance_margin_bps() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store).unwrap();
        let mut spec = fallback_spec_for_market("btc-usdt");
        spec.maintenance_margin_bps = -1;
        assert!(registry.upsert(spec).is_err());
    }

    #[test]
    fn upsert_accepts_valid_all_fields() {
        let store = Arc::new(InMemoryWal::<InstrumentRegistryRecord>::new());
        let registry = PersistentInstrumentRegistry::new(store).unwrap();
        let mut spec = fallback_spec_for_market("btc-usdt");
        spec.tick_size = 5;
        spec.lot_size = 10;
        spec.price_band_bps = 500;
        spec.maintenance_margin_bps = 200;
        spec.maker_fee_bps = 2;
        spec.taker_fee_bps = 5;
        assert!(registry.upsert(spec).is_ok());
    }
}
