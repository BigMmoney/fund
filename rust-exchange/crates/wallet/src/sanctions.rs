//! Sanctions screening — provider trait + in-memory stub.
//!
//! Step 8 of the wallet implementation track. The `SanctionsProvider`
//! trait abstracts the on-chain analytics provider (Chainalysis / TRM
//! / Elliptic) so the address book + withdrawal validate-time check
//! can be parameterised. v1 ships:
//!
//! - The trait itself.
//! - `StubSanctionsProvider` — an in-process deterministic stub that
//!   matches addresses against a configurable deny-list. Used by
//!   tests + the wallet smoke harness; the real Chainalysis adapter
//!   lands behind a feature flag when the api keys are provisioned.
//!
//! Per design §11 rule 9: a sanctions hit is BLOCKING, not advisory.
//! The address book layer interprets `SanctionsScreenStatus::Hit` as
//! an immediate `Suspended` flip; the withdrawal validate-time path
//! aborts with `WithdrawalRejectReason::SanctionsHit`.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use parking_lot::RwLock;
use thiserror::Error;

use crate::types::{
    ChainId, SanctionsCheckResult, SanctionsHit, SanctionsScreenStatus,
};

#[derive(Debug, Error)]
pub enum SanctionsError {
    #[error("sanctions provider rpc error: {0}")]
    Rpc(String),
    #[error("sanctions provider timeout")]
    Timeout,
}

/// Trait implementations live alongside this module: the stub here,
/// the future Chainalysis adapter under `crates/wallet/src/sanctions/
/// chainalysis.rs` (not in v1).
///
/// Implementations MUST NOT block. Real providers wrap an HTTP call;
/// callers run the screen in a separate task. The stub returns
/// synchronously.
pub trait SanctionsProvider: Send + Sync {
    /// Provider identifier surfaced on the audit row. Stable string
    /// (e.g. "chainalysis", "trm", "stub").
    fn provider_id(&self) -> &str;

    /// Screen one address. Returns the structured result; never
    /// panics. Errors at the RPC layer surface as `Status::Error`
    /// inside the result rather than a returned `Err` so the caller
    /// can treat them as soft-block-and-retry uniformly.
    fn screen(&self, chain: ChainId, address: &str) -> SanctionsCheckResult;
}

/// In-process deterministic stub. Tracks a single deny-list keyed on
/// `(chain, address)`. `add_hit` registers a hit; otherwise returns
/// `Clear`. Mutable behind an internal RwLock so tests can flip
/// addresses between clear and hit dynamically.
pub struct StubSanctionsProvider {
    provider_id: String,
    state: Arc<RwLock<StubState>>,
}

#[derive(Default)]
struct StubState {
    deny_list: HashSet<(ChainId, String)>,
    /// Forced-error addresses — when present in this set the
    /// `screen` method returns `SanctionsScreenStatus::Error`.
    /// Used by tests of soft-block behaviour.
    error_list: HashSet<(ChainId, String)>,
}

impl StubSanctionsProvider {
    pub fn new() -> Self {
        Self::with_provider_id("stub")
    }

    pub fn with_provider_id(id: impl Into<String>) -> Self {
        Self {
            provider_id: id.into(),
            state: Arc::new(RwLock::new(StubState::default())),
        }
    }

    /// Register a `(chain, address)` as sanctioned. Subsequent
    /// `screen` calls return `Hit`.
    pub fn add_hit(&self, chain: ChainId, address: impl Into<String>) {
        let mut s = self.state.write();
        s.deny_list.insert((chain, address.into()));
    }

    /// Register a `(chain, address)` as currently failing — `screen`
    /// returns `SanctionsScreenStatus::Error` for these. Used to test
    /// soft-block behaviour.
    pub fn add_error(&self, chain: ChainId, address: impl Into<String>) {
        let mut s = self.state.write();
        s.error_list.insert((chain, address.into()));
    }

    /// Remove a previously-added hit / error.
    pub fn clear(&self, chain: ChainId, address: &str) {
        let mut s = self.state.write();
        s.deny_list.remove(&(chain, address.to_string()));
        s.error_list.remove(&(chain, address.to_string()));
    }
}

impl Default for StubSanctionsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SanctionsProvider for StubSanctionsProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn screen(&self, chain: ChainId, address: &str) -> SanctionsCheckResult {
        let now = Utc::now();
        let s = self.state.read();
        let key = (chain, address.to_string());
        if s.error_list.contains(&key) {
            return SanctionsCheckResult {
                status: SanctionsScreenStatus::Error,
                provider: self.provider_id.clone(),
                checked_at: now,
                hit: None,
                correlation_id: Some(format!("stub-error-{address}")),
            };
        }
        if s.deny_list.contains(&key) {
            return SanctionsCheckResult {
                status: SanctionsScreenStatus::Hit,
                provider: self.provider_id.clone(),
                checked_at: now,
                hit: Some(SanctionsHit {
                    list_name: "STUB DENY LIST".into(),
                    matched_entity: address.to_string(),
                    score: 100,
                    matched_at: now,
                }),
                correlation_id: Some(format!("stub-hit-{address}")),
            };
        }
        SanctionsCheckResult {
            status: SanctionsScreenStatus::Clear,
            provider: self.provider_id.clone(),
            checked_at: now,
            hit: None,
            correlation_id: Some(format!("stub-clear-{address}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_address_returns_clear_status() {
        let p = StubSanctionsProvider::new();
        let r = p.screen(ChainId::Eth, "0xclean");
        assert_eq!(r.status, SanctionsScreenStatus::Clear);
        assert!(r.hit.is_none());
        assert_eq!(r.provider, "stub");
    }

    #[test]
    fn deny_listed_address_returns_hit() {
        let p = StubSanctionsProvider::new();
        p.add_hit(ChainId::Eth, "0xbad");
        let r = p.screen(ChainId::Eth, "0xbad");
        assert_eq!(r.status, SanctionsScreenStatus::Hit);
        let hit = r.hit.expect("hit block populated");
        assert_eq!(hit.list_name, "STUB DENY LIST");
        assert_eq!(hit.matched_entity, "0xbad");
        assert_eq!(hit.score, 100);
    }

    #[test]
    fn deny_listed_other_chain_does_not_match() {
        let p = StubSanctionsProvider::new();
        p.add_hit(ChainId::Eth, "0xbad");
        // Same string on BTC should be Clear.
        let r = p.screen(ChainId::Btc, "0xbad");
        assert_eq!(r.status, SanctionsScreenStatus::Clear);
    }

    #[test]
    fn error_list_returns_error_status_for_soft_block() {
        let p = StubSanctionsProvider::new();
        p.add_error(ChainId::Eth, "0xprovider-down");
        let r = p.screen(ChainId::Eth, "0xprovider-down");
        assert_eq!(r.status, SanctionsScreenStatus::Error);
        assert!(r.hit.is_none());
    }

    #[test]
    fn clear_removes_existing_entry() {
        let p = StubSanctionsProvider::new();
        p.add_hit(ChainId::Eth, "0xbad");
        assert_eq!(p.screen(ChainId::Eth, "0xbad").status, SanctionsScreenStatus::Hit);
        p.clear(ChainId::Eth, "0xbad");
        assert_eq!(p.screen(ChainId::Eth, "0xbad").status, SanctionsScreenStatus::Clear);
    }

    #[test]
    fn provider_id_overridable() {
        let p = StubSanctionsProvider::with_provider_id("test-provider");
        assert_eq!(p.provider_id(), "test-provider");
        let r = p.screen(ChainId::Eth, "0x1");
        assert_eq!(r.provider, "test-provider");
    }

    #[test]
    fn provider_is_send_sync_for_arc_dyn_use() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<dyn SanctionsProvider>>();
    }
}
