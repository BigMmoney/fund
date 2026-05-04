//! Chainalysis sanctions adapter — feature-gated scaffold (gate
//! **P0-SEC-6**).
//!
//! The full implementation hits the Chainalysis Sanctions API
//! (`https://public.chainalysis.com/api/v1/address/{address}`) per
//! address; on `Hit` we record the matched list and return
//! `SanctionsScreenStatus::Hit`. RPC failures map to `Error` so the
//! customer-wallet handler treats them as a soft block.
//!
//! Like `eth_rpc.rs` this is a scaffold — the API key isn't yet
//! provisioned and adding `reqwest` etc. for one HTTP call inflates
//! the dependency surface. The trait surface is final and the
//! integration point in `main.rs` is one line:
//!
//! ```ignore
//! #[cfg(feature = "chainalysis")]
//! let customer_wallet_sanctions = Arc::new(
//!     wallet::ChainalysisProvider::from_env()
//! ) as Arc<dyn wallet::SanctionsProvider>;
//! ```

#![cfg(feature = "chainalysis")]

use chrono::Utc;

use crate::sanctions::SanctionsProvider;
use crate::types::{ChainId, SanctionsCheckResult, SanctionsScreenStatus};

#[derive(Debug, Clone)]
pub struct ChainalysisConfig {
    pub api_url: String,
    pub api_key: String,
    pub timeout_ms: u64,
    pub max_retries: u32,
}

impl ChainalysisConfig {
    pub fn from_env() -> Self {
        Self {
            api_url: std::env::var("CHAINALYSIS_API_URL")
                .unwrap_or_else(|_| "https://public.chainalysis.com/api/v1/address".to_string()),
            api_key: std::env::var("CHAINALYSIS_API_KEY")
                .expect("CHAINALYSIS_API_KEY must be set when --features chainalysis"),
            timeout_ms: std::env::var("CHAINALYSIS_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2_000),
            max_retries: std::env::var("CHAINALYSIS_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
        }
    }
}

pub struct ChainalysisProvider {
    #[allow(dead_code)]
    config: ChainalysisConfig,
}

impl ChainalysisProvider {
    pub fn new(config: ChainalysisConfig) -> Self {
        eprintln!(
            "[wallet::chainalysis] ChainalysisProvider scaffold loaded — \
             real HTTP calls are not yet wired (gate P0-SEC-6); \
             every screen() will return Error"
        );
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(ChainalysisConfig::from_env())
    }
}

impl SanctionsProvider for ChainalysisProvider {
    fn provider_id(&self) -> &str {
        "chainalysis"
    }

    fn screen(&self, _chain: ChainId, address: &str) -> SanctionsCheckResult {
        // Soft-block until the real adapter ships. The customer-wallet
        // handler treats Error as `SanctionsUnavailable` (HTTP 503) so
        // a misconfigured environment fails closed, not open.
        SanctionsCheckResult {
            status: SanctionsScreenStatus::Error,
            provider: self.provider_id().to_string(),
            checked_at: Utc::now(),
            hit: None,
            correlation_id: Some(format!("chainalysis-scaffold-{address}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_returns_error_status_so_handler_soft_blocks() {
        let cfg = ChainalysisConfig {
            api_url: "http://localhost:0".into(),
            api_key: "stub".into(),
            timeout_ms: 1_000,
            max_retries: 1,
        };
        let provider = ChainalysisProvider::new(cfg);
        let result = provider.screen(ChainId::Eth, "0xanything");
        assert_eq!(result.status, SanctionsScreenStatus::Error);
        assert_eq!(result.provider, "chainalysis");
    }
}
