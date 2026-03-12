use chrono::{DateTime, Utc};
use dashmap::DashMap;
use persistence::{JsonlFileWal, WalStore};
use std::sync::Arc;
use types::{InstrumentKind, InstrumentSpec, MarginMode};

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
        self.store.append(&InstrumentRegistryRecord {
            spec: spec.clone(),
            recorded_at: Utc::now(),
        })?;
        self.specs.insert(spec.instrument_id.clone(), spec);
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
    } else {
        InstrumentKind::Spot
    }
}

pub fn fallback_spec_for_market(market_id: &str) -> InstrumentSpec {
    let kind = infer_instrument_kind(market_id);
    InstrumentSpec {
        instrument_id: market_id.to_string(),
        kind,
        quote_asset: "USDC".to_string(),
        margin_mode: match kind {
            InstrumentKind::Spot => None,
            InstrumentKind::Margin | InstrumentKind::Perpetual => Some(MarginMode::Isolated),
        },
        max_leverage: match kind {
            InstrumentKind::Spot => None,
            InstrumentKind::Margin | InstrumentKind::Perpetual => Some(20),
        },
        tick_size: 1,
        lot_size: 1,
        price_band_bps: 1_000,
        risk_policy_id: match kind {
            InstrumentKind::Spot => "spot-v1".to_string(),
            InstrumentKind::Margin => "margin-v1".to_string(),
            InstrumentKind::Perpetual => "perpetual-v1".to_string(),
        },
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
        assert_eq!(margin.margin_mode, Some(MarginMode::Isolated));
        assert_eq!(perp.kind, InstrumentKind::Perpetual);
        assert_eq!(perp.max_leverage, Some(20));
    }

    #[test]
    fn in_memory_registry_overrides_fallback() {
        let registry = InMemoryInstrumentRegistry::new();
        registry.register(InstrumentSpec {
            instrument_id: "btc-usdt".to_string(),
            kind: InstrumentKind::Margin,
            quote_asset: "USDC".to_string(),
            margin_mode: Some(MarginMode::Cross),
            max_leverage: Some(5),
            tick_size: 5,
            lot_size: 10,
            price_band_bps: 500,
            risk_policy_id: "custom-margin".to_string(),
        });

        let spec = registry.resolve("btc-usdt");
        assert_eq!(spec.kind, InstrumentKind::Margin);
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
                quote_asset: "USDC".to_string(),
                margin_mode: Some(MarginMode::Isolated),
                max_leverage: Some(20),
                tick_size: 1,
                lot_size: 1,
                price_band_bps: 1000,
                risk_policy_id: "perp-v1".to_string(),
            })
            .unwrap();

        let reloaded = PersistentInstrumentRegistry::new(store).unwrap();
        let spec = reloaded.resolve("perp:btc-usdt");
        assert_eq!(spec.kind, InstrumentKind::Perpetual);
        assert_eq!(reloaded.list().len(), 1);
    }
}
