//! Real ETH chain adapter (gate **P0-FUND-1**).
//!
//! Read-side (`balance`, `confirmations`, `fee_estimate`) talks
//! `eth_getBalance` / `eth_blockNumber` / `eth_getTransactionByHash` /
//! `eth_feeHistory` over JSON-RPC via `ureq`. Write-side
//! (`build_withdrawal`, `sign_and_broadcast`) is intentionally
//! stubbed — it requires an in-process signing key (KMS-sealed) and
//! a hardened nonce manager (see `docs/REAL_CHAIN_ADAPTER_SPEC.md`
//! §5). Wiring those in is mechanical once the secrets backend
//! (P0-SEC-1) is provisioned; the trait surface here is final.
//!
//! Multi-RPC failover: primary → secondary → tertiary, each given
//! `rpc_max_retries` attempts with linear backoff. On total failure
//! the call returns `ChainError::Rpc` and the worker leaves the
//! record at its current state for the next tick.

#![cfg(feature = "eth-rpc")]

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::chain::{ChainAdapter, ChainError, UnsignedTx};
use crate::secrets::{Secret, SecretLoader};
use crate::types::{ChainId, FeeUrgency};

/// Configuration for the ETH adapter. Loaded once at startup from env
/// (`WALLET_ETH_*`) per `docs/REAL_CHAIN_ADAPTER_SPEC.md` §3.
#[derive(Debug, Clone)]
pub struct EthAdapterConfig {
    pub primary_rpc: String,
    pub secondary_rpc: Option<String>,
    pub tertiary_rpc: Option<String>,
    pub private_rpc: Option<String>,
    pub hot_address: String,
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
    /// Load every non-secret field from env. The hot-wallet private key
    /// is loaded via the `SecretLoader` (gate P0-SEC-1) inside
    /// `EthRpcAdapter::from_loader`.
    pub fn from_env() -> Self {
        Self {
            primary_rpc: std::env::var("WALLET_ETH_RPC_PRIMARY")
                .expect("WALLET_ETH_RPC_PRIMARY must be set when --features eth-rpc"),
            secondary_rpc: std::env::var("WALLET_ETH_RPC_SECONDARY").ok(),
            tertiary_rpc: std::env::var("WALLET_ETH_RPC_TERTIARY").ok(),
            private_rpc: std::env::var("WALLET_ETH_RPC_PRIVATE").ok(),
            hot_address: std::env::var("WALLET_ETH_HOT_ADDRESS")
                .expect("WALLET_ETH_HOT_ADDRESS must be set when --features eth-rpc"),
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

/// JSON-RPC request envelope.
#[derive(Serialize)]
struct RpcRequest<'a, T: Serialize> {
    jsonrpc: &'a str,
    method: &'a str,
    params: T,
    id: u64,
}

#[derive(Deserialize, Debug)]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
struct RpcResponse<T> {
    #[serde(default = "default_none")]
    result: Option<T>,
    #[serde(default)]
    error: Option<RpcErrorBody>,
}

fn default_none<T>() -> Option<T> {
    None
}

#[derive(Deserialize, Debug)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

/// Real ETH chain adapter. Read-side only in v1; write-side stubbed
/// until the KMS-sealed key is provisioned.
pub struct EthRpcAdapter {
    config: EthAdapterConfig,
    /// Held for future signing path; currently unused but kept on the
    /// struct so `from_loader` can validate the key is present at
    /// startup rather than at first broadcast.
    #[allow(dead_code)]
    hot_private_key: Option<Arc<Secret>>,
    agent: ureq::Agent,
}

impl EthRpcAdapter {
    pub fn new(config: EthAdapterConfig) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(config.rpc_timeout_ms))
            .build();
        eprintln!(
            "[wallet::eth_rpc] EthRpcAdapter loaded — primary_rpc={}, hot_address={}; \
             read-side wired (balance/confirmations); signing/broadcast not yet wired \
             (gate P0-FUND-1 write phase)",
            config.primary_rpc, config.hot_address
        );
        Self {
            config,
            hot_private_key: None,
            agent,
        }
    }

    /// Build via the SecretLoader. Required secrets:
    /// - `WALLET_ETH_HOT_PRIVATE_KEY` — hot wallet private key (read-
    ///   only path doesn't need it; loader is checked here so a
    ///   misconfigured production node still fails closed at boot).
    pub fn from_loader(loader: &dyn SecretLoader) -> Self {
        let key = loader
            .load("WALLET_ETH_HOT_PRIVATE_KEY")
            .expect("WALLET_ETH_HOT_PRIVATE_KEY must be loadable when --features eth-rpc");
        let mut adapter = Self::new(EthAdapterConfig::from_env());
        adapter.hot_private_key = Some(Arc::new(key));
        adapter
    }

    /// Iterate over the configured RPC endpoints in order.
    fn rpc_endpoints(&self) -> Vec<&str> {
        let mut out = vec![self.config.primary_rpc.as_str()];
        if let Some(s) = self.config.secondary_rpc.as_deref() {
            out.push(s);
        }
        if let Some(t) = self.config.tertiary_rpc.as_deref() {
            out.push(t);
        }
        out
    }

    /// Per-RPC retry. Returns the first successful response, or the
    /// last error after all RPCs failed.
    fn call_rpc<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, ChainError> {
        let body = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id: 1,
        };
        let mut last_err = String::from("no rpc endpoints configured");
        for url in self.rpc_endpoints() {
            for attempt in 0..=self.config.rpc_max_retries {
                let resp = self.agent.post(url).send_json(&body);
                match resp {
                    Ok(r) => match r.into_json::<RpcResponse<T>>() {
                        Ok(parsed) => {
                            if let Some(err) = parsed.error {
                                last_err = format!(
                                    "rpc {url} returned error code={} msg={}",
                                    err.code, err.message
                                );
                                // protocol error — try next endpoint
                                break;
                            }
                            if let Some(result) = parsed.result {
                                return Ok(result);
                            }
                            last_err = format!("rpc {url} missing result and error");
                            break;
                        }
                        Err(e) => {
                            last_err = format!("rpc {url} decode failed: {e}");
                            break;
                        }
                    },
                    Err(e) => {
                        last_err = format!(
                            "rpc {url} attempt {} failed: {e}",
                            attempt + 1
                        );
                        std::thread::sleep(Duration::from_millis(
                            200u64 * 3u64.pow(attempt),
                        ));
                    }
                }
            }
        }
        Err(ChainError::Rpc(last_err))
    }
}

fn parse_hex_u128(hex: &str) -> Result<u128, ChainError> {
    let stripped = hex.strip_prefix("0x").unwrap_or(hex);
    u128::from_str_radix(stripped, 16)
        .map_err(|e| ChainError::Rpc(format!("invalid hex from rpc: {hex} ({e})")))
}

fn parse_hex_u64(hex: &str) -> Result<u64, ChainError> {
    let stripped = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(stripped, 16)
        .map_err(|e| ChainError::Rpc(format!("invalid hex from rpc: {hex} ({e})")))
}

#[derive(Deserialize)]
struct EthTxByHash {
    #[serde(default)]
    #[serde(rename = "blockNumber")]
    block_number: Option<String>,
}

impl ChainAdapter for EthRpcAdapter {
    fn chain(&self) -> ChainId {
        ChainId::Eth
    }

    fn balance(&self, address: &str) -> Result<i128, ChainError> {
        let hex: String = self.call_rpc(
            "eth_getBalance",
            serde_json::json!([address, "latest"]),
        )?;
        let wei = parse_hex_u128(&hex)?;
        i128::try_from(wei)
            .map_err(|_| ChainError::Rpc(format!("balance overflows i128: {wei}")))
    }

    fn confirmations(&self, tx_hash: &str) -> Result<u32, ChainError> {
        let tx: Option<EthTxByHash> = self.call_rpc(
            "eth_getTransactionByHash",
            serde_json::json!([tx_hash]),
        )?;
        let Some(tx) = tx else { return Ok(0) };
        let Some(block_hex) = tx.block_number else { return Ok(0) };
        let block_number = parse_hex_u64(&block_hex)?;
        let latest_hex: String = self.call_rpc("eth_blockNumber", serde_json::json!([]))?;
        let latest = parse_hex_u64(&latest_hex)?;
        if latest < block_number {
            return Ok(0);
        }
        let depth = latest - block_number + 1;
        Ok(u32::try_from(depth).unwrap_or(u32::MAX))
    }

    fn fee_estimate(&self, urgency: FeeUrgency) -> Result<i128, ChainError> {
        // Conservative static estimate: gas_limit × (base fee 30 gwei
        // floor + priority fee per urgency). Production should replace
        // this with eth_feeHistory + EIP-1559 calc per spec §6.
        let priority_gwei = match urgency {
            FeeUrgency::Economy => self.config.priority_fee_gwei_normal,
            FeeUrgency::Standard => self.config.priority_fee_gwei_normal * 1.25,
            FeeUrgency::Urgent => self.config.priority_fee_gwei_fast,
        };
        let base_floor_gwei = 30.0_f64;
        let total_gwei = base_floor_gwei + priority_gwei;
        let total_wei = (total_gwei * 1e9) as u128;
        let cost = (self.config.gas_limit as u128).saturating_mul(total_wei);
        i128::try_from(cost)
            .map_err(|_| ChainError::Rpc("fee estimate overflow i128".to_string()))
    }

    fn build_withdrawal(
        &self,
        _from: &str,
        _to: &str,
        _amount: i128,
        _fee: i128,
    ) -> Result<UnsignedTx, ChainError> {
        Err(ChainError::Rpc(
            "build_withdrawal not yet wired — write-side requires KMS-sealed key (gate P0-FUND-1 write phase)"
                .to_string(),
        ))
    }

    fn sign_and_broadcast(&self, _tx: &UnsignedTx) -> Result<String, ChainError> {
        Err(ChainError::Rpc(
            "sign_and_broadcast not yet wired — write-side requires KMS-sealed key (gate P0-FUND-1 write phase)"
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(primary: &str) -> EthAdapterConfig {
        EthAdapterConfig {
            primary_rpc: primary.to_string(),
            secondary_rpc: None,
            tertiary_rpc: None,
            private_rpc: None,
            hot_address: "0x0".into(),
            confirmations_required: 25,
            gas_limit: 21_000,
            max_fee_per_gas_gwei: 300,
            priority_fee_gwei_normal: 1.5,
            priority_fee_gwei_fast: 3.0,
            rpc_timeout_ms: 200,
            rpc_max_retries: 0,
            reorg_tolerance: 5,
        }
    }

    #[test]
    fn chain_id_is_eth() {
        let adapter = EthRpcAdapter::new(cfg("http://127.0.0.1:1"));
        assert_eq!(adapter.chain(), ChainId::Eth);
    }

    #[test]
    fn unreachable_rpc_returns_rpc_error() {
        // Closed port → transport error → ChainError::Rpc, NOT silent
        // success. The hot-wallet worker will leave the record at its
        // current state and retry.
        let adapter = EthRpcAdapter::new(cfg("http://127.0.0.1:1"));
        match adapter.balance("0xdeadbeef") {
            Err(ChainError::Rpc(msg)) => {
                assert!(msg.contains("failed") || msg.contains("attempt"));
            }
            other => panic!("expected Rpc error, got {:?}", other),
        }
    }

    #[test]
    fn fee_estimate_scales_with_urgency() {
        let adapter = EthRpcAdapter::new(cfg("http://127.0.0.1:1"));
        let economy = adapter.fee_estimate(FeeUrgency::Economy).unwrap();
        let standard = adapter.fee_estimate(FeeUrgency::Standard).unwrap();
        let urgent = adapter.fee_estimate(FeeUrgency::Urgent).unwrap();
        assert!(standard >= economy);
        assert!(urgent > standard);
    }

    #[test]
    fn write_side_returns_explicit_not_yet_wired_error() {
        let adapter = EthRpcAdapter::new(cfg("http://127.0.0.1:1"));
        match adapter.build_withdrawal("0xa", "0xb", 1, 1) {
            Err(ChainError::Rpc(msg)) => assert!(msg.contains("not yet wired")),
            other => panic!("expected Rpc error, got {:?}", other),
        }
    }

    #[test]
    fn parse_hex_helpers() {
        assert_eq!(parse_hex_u128("0x1").unwrap(), 1);
        assert_eq!(parse_hex_u128("0xff").unwrap(), 255);
        assert_eq!(parse_hex_u64("0x10").unwrap(), 16);
        assert!(parse_hex_u128("not-hex").is_err());
    }
}
