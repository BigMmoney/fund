//! Real ETH chain adapter — feature-gated scaffold (gate **P0-FUND-1**).
//!
//! This module is the integration point that REPLACES
//! `InMemoryChainAdapter` for production. The full implementation
//! (ethers-rs / alloy + multi-RPC failover + nonce management +
//! EIP-1559 fee bump) is documented in
//! `docs/REAL_CHAIN_ADAPTER_SPEC.md`. To keep CI green without the
//! dependency surface, this file ships ONLY the trait scaffold
//! behind `--features eth-rpc`.
//!
//! Why a scaffold and not the full impl:
//! - `ethers-rs` / `alloy` add ~80 transitive crates and ~5 minutes
//!   of cold build.
//! - The HOT WALLET PRIVATE KEY belongs in a KMS-sealed env var that
//!   isn't yet provisioned.
//! - The primary/secondary/tertiary RPC URLs aren't yet provisioned
//!   for any environment.
//!
//! When all three are ready, the bodies of `EthRpcAdapter`'s methods
//! are filled in per the spec; the trait surface here is final and
//! stable so downstream wiring (`SettlementWorker`, `HotWalletWorker`)
//! needs no further change.

#![cfg(feature = "eth-rpc")]

use std::sync::Arc;

use crate::chain::{ChainAdapter, ChainError, UnsignedTx};
use crate::types::{ChainId, FeeUrgency};

/// Configuration for the ETH adapter. All fields are loaded once at
/// startup from env (`WALLET_ETH_*`) per
/// `docs/REAL_CHAIN_ADAPTER_SPEC.md` §3.
#[derive(Debug, Clone)]
pub struct EthAdapterConfig {
    pub primary_rpc: String,
    pub secondary_rpc: Option<String>,
    pub tertiary_rpc: Option<String>,
    pub private_rpc: Option<String>,
    pub hot_address: String,
    /// KMS-sealed private key. Held as `Arc<str>` so the value can
    /// be loaded once and shared across worker tasks; zeroize on
    /// drop is a follow-up (gate P2-SEC-1 for the rotation runbook).
    pub hot_private_key: Arc<str>,
    pub confirmations_required: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas_gwei: u64,
    pub priority_fee_gwei_normal: f64,
    pub priority_fee_gwei_fast: f64,
    pub rpc_timeout_ms: u64,
    pub rpc_max_retries: u32,
    pub reorg_tolerance: u64,
}

impl EthAdapterConfig {
    /// Load every field from env. Missing required fields panic at
    /// startup — that's the right behaviour for a KMS-sealed value.
    pub fn from_env() -> Self {
        Self {
            primary_rpc: std::env::var("WALLET_ETH_RPC_PRIMARY")
                .expect("WALLET_ETH_RPC_PRIMARY must be set when --features eth-rpc"),
            secondary_rpc: std::env::var("WALLET_ETH_RPC_SECONDARY").ok(),
            tertiary_rpc: std::env::var("WALLET_ETH_RPC_TERTIARY").ok(),
            private_rpc: std::env::var("WALLET_ETH_RPC_PRIVATE").ok(),
            hot_address: std::env::var("WALLET_ETH_HOT_ADDRESS")
                .expect("WALLET_ETH_HOT_ADDRESS must be set when --features eth-rpc"),
            hot_private_key: Arc::from(
                std::env::var("WALLET_ETH_HOT_PRIVATE_KEY")
                    .expect("WALLET_ETH_HOT_PRIVATE_KEY must be set when --features eth-rpc")
                    .as_str(),
            ),
            confirmations_required: std::env::var("WALLET_ETH_CONFIRMATIONS_REQUIRED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(25),
            gas_limit: std::env::var("WALLET_ETH_GAS_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(21_000),
            max_fee_per_gas_gwei: std::env::var("WALLET_ETH_MAX_FEE_PER_GAS_GWEI")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
            priority_fee_gwei_normal: std::env::var("WALLET_ETH_PRIORITY_FEE_GWEI_NORMAL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.5),
            priority_fee_gwei_fast: std::env::var("WALLET_ETH_PRIORITY_FEE_GWEI_FAST")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3.0),
            rpc_timeout_ms: std::env::var("WALLET_ETH_RPC_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5_000),
            rpc_max_retries: std::env::var("WALLET_ETH_RPC_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            reorg_tolerance: std::env::var("WALLET_ETH_REORG_TOLERANCE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
        }
    }
}

/// Real ETH chain adapter. Stub bodies — full implementation tracked
/// in `docs/REAL_CHAIN_ADAPTER_SPEC.md`. Returns `ChainError::Rpc`
/// from every fallible method; production builds will unstub these
/// once the deps and secrets are provisioned.
pub struct EthRpcAdapter {
    #[allow(dead_code)]
    config: EthAdapterConfig,
}

impl EthRpcAdapter {
    pub fn new(config: EthAdapterConfig) -> Self {
        eprintln!(
            "[wallet::eth_rpc] EthRpcAdapter scaffold loaded — primary_rpc={}; \
             real RPC calls are not yet wired (gate P0-FUND-1)",
            config.primary_rpc
        );
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(EthAdapterConfig::from_env())
    }
}

impl ChainAdapter for EthRpcAdapter {
    fn chain(&self) -> ChainId {
        ChainId::Eth
    }

    fn confirmations(&self, _tx_hash: &str) -> Result<u32, ChainError> {
        Err(ChainError::Rpc(
            "eth-rpc adapter is a scaffold; see docs/REAL_CHAIN_ADAPTER_SPEC.md".to_string(),
        ))
    }

    fn balance(&self, _address: &str) -> Result<i128, ChainError> {
        Err(ChainError::Rpc(
            "eth-rpc adapter is a scaffold; see docs/REAL_CHAIN_ADAPTER_SPEC.md".to_string(),
        ))
    }

    fn fee_estimate(&self, _urgency: FeeUrgency) -> Result<i128, ChainError> {
        Err(ChainError::Rpc(
            "eth-rpc adapter is a scaffold; see docs/REAL_CHAIN_ADAPTER_SPEC.md".to_string(),
        ))
    }

    fn build_withdrawal(
        &self,
        _from: &str,
        _to: &str,
        _amount: i128,
        _fee: i128,
    ) -> Result<UnsignedTx, ChainError> {
        Err(ChainError::Rpc(
            "eth-rpc adapter is a scaffold; see docs/REAL_CHAIN_ADAPTER_SPEC.md".to_string(),
        ))
    }

    fn sign_and_broadcast(&self, _tx: &UnsignedTx) -> Result<String, ChainError> {
        Err(ChainError::Rpc(
            "eth-rpc adapter is a scaffold; see docs/REAL_CHAIN_ADAPTER_SPEC.md".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> EthAdapterConfig {
        EthAdapterConfig {
            primary_rpc: "http://localhost:0".into(),
            secondary_rpc: None,
            tertiary_rpc: None,
            private_rpc: None,
            hot_address: "0x0".into(),
            hot_private_key: Arc::from("dummy"),
            confirmations_required: 25,
            gas_limit: 21_000,
            max_fee_per_gas_gwei: 300,
            priority_fee_gwei_normal: 1.5,
            priority_fee_gwei_fast: 3.0,
            rpc_timeout_ms: 5_000,
            rpc_max_retries: 3,
            reorg_tolerance: 5,
        }
    }

    #[test]
    fn scaffold_chain_id_is_eth() {
        let adapter = EthRpcAdapter::new(cfg());
        assert_eq!(adapter.chain(), ChainId::Eth);
    }

    #[test]
    fn scaffold_methods_return_rpc_error_until_implemented() {
        let adapter = EthRpcAdapter::new(cfg());
        assert!(matches!(adapter.balance("0xdeadbeef"), Err(ChainError::Rpc(_))));
        assert!(matches!(adapter.confirmations("0xnone"), Err(ChainError::Rpc(_))));
        assert!(matches!(adapter.fee_estimate(FeeUrgency::Standard), Err(ChainError::Rpc(_))));
    }
}
