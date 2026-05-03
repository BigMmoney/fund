//! Chain adapter trait + in-memory stub.
//!
//! Step 7D of the wallet implementation track. The `ChainAdapter`
//! trait abstracts per-chain operations so the withdrawal pipeline,
//! reconciliation job, and hot-wallet daemon can be written once and
//! parameterised over chain (ETH / BTC / SOL).
//!
//! v1 ships:
//! - The trait itself.
//! - `InMemoryChainAdapter` — a deterministic stub for tests + the
//!   wallet smoke test. Tracks balances, fee suggestions, and the
//!   set of broadcast tx hashes purely in-process. No network I/O.
//!
//! Real adapters (Eth in 7E, Btc + Sol later) live alongside this
//! module under `crates/wallet/src/chains/`.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use thiserror::Error;

use crate::types::{ChainId, FeeUrgency};

#[derive(Debug, Error)]
pub enum ChainError {
    #[error("address invalid for chain {chain}: {detail}")]
    InvalidAddress { chain: ChainId, detail: String },
    #[error("insufficient hot-wallet balance for chain {chain}: have {have}, need {need}")]
    InsufficientBalance { chain: ChainId, have: i128, need: i128 },
    #[error("rpc error: {0}")]
    Rpc(String),
    #[error("broadcast failed: {0}")]
    Broadcast(String),
    #[error("tx not found: {0}")]
    TxNotFound(String),
    #[error("amount must be > 0, got {0}")]
    InvalidAmount(i128),
}

/// Built (but unsigned) tx body. Opaque to the pipeline; the chain
/// adapter knows how to sign and broadcast it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsignedTx {
    pub chain: ChainId,
    pub from: String,
    pub to: String,
    pub amount: i128,
    pub fee: i128,
    /// Chain-specific opaque payload — e.g. RLP-encoded ETH tx body
    /// or a serialised PSBT for BTC. The in-memory stub uses a
    /// deterministic hex-of-(from, to, amount) string.
    pub payload: String,
}

/// Signed tx. Same shape as `UnsignedTx` plus a signature blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedTx {
    pub unsigned: UnsignedTx,
    /// Hex-encoded signature blob; chain-specific format.
    pub signature: String,
}

/// Subscription event surfaced by `watch_deposits`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositEvent {
    pub chain: ChainId,
    pub address: String,
    pub amount: i128,
    pub tx_hash: String,
    pub observed_at: DateTime<Utc>,
}

/// Per-chain abstraction. Implementations live under
/// `crates/wallet/src/chains/{eth,btc,sol}.rs`. This commit ships
/// the in-memory stub only; real adapters land in 7E+.
pub trait ChainAdapter: Send + Sync {
    fn chain(&self) -> ChainId;

    /// Confirmation depth observed on-chain for `tx_hash`. Returns 0
    /// for unknown hashes (the caller polls until depth >= the
    /// per-withdrawal `confirmations_required`).
    fn confirmations(&self, tx_hash: &str) -> Result<u32, ChainError>;

    /// On-chain balance for `address` in smallest indivisible unit.
    fn balance(&self, address: &str) -> Result<i128, ChainError>;

    /// Estimated total fee for a one-output transfer at the given
    /// urgency tier. Smallest indivisible unit.
    fn fee_estimate(&self, urgency: FeeUrgency) -> Result<i128, ChainError>;

    /// Build (but do not sign) a withdrawal tx.
    fn build_withdrawal(
        &self,
        from: &str,
        to: &str,
        amount: i128,
        fee: i128,
    ) -> Result<UnsignedTx, ChainError>;

    /// Sign + broadcast a built tx. Returns the resulting tx hash.
    /// In production the signing step lives in the hot-wallet daemon
    /// (a separate process with key material in memory only) and the
    /// adapter is split into `Builder` and `Broadcaster` halves; v1
    /// keeps both in the trait for the in-memory smoke path.
    fn sign_and_broadcast(&self, tx: &UnsignedTx) -> Result<String, ChainError>;
}

/// Address-bookkeeping rec for the stub.
#[derive(Debug, Clone)]
struct StubBalance {
    balance: i128,
}

#[derive(Debug, Clone)]
struct StubTx {
    confirmations: u32,
}

/// Deterministic in-process chain adapter for tests + the wallet
/// smoke test. Each instance is per-chain. Methods are non-async and
/// non-blocking; balance / fee mutations are atomic under a single
/// RwLock.
pub struct InMemoryChainAdapter {
    chain: ChainId,
    fee_per_urgency: i128,
    state: Arc<RwLock<InMemoryChainState>>,
}

#[derive(Debug, Default)]
struct InMemoryChainState {
    balances: std::collections::HashMap<String, StubBalance>,
    txs: std::collections::HashMap<String, StubTx>,
    next_tx_seq: u64,
}

impl InMemoryChainAdapter {
    pub fn new(chain: ChainId) -> Self {
        Self {
            chain,
            fee_per_urgency: default_fee_for(chain),
            state: Arc::new(RwLock::new(InMemoryChainState::default())),
        }
    }

    /// Seed an address with a starting balance. Used by tests / the
    /// smoke harness to fund the hot wallet.
    pub fn seed_balance(&self, address: &str, balance: i128) {
        let mut s = self.state.write();
        s.balances.insert(address.into(), StubBalance { balance });
    }

    /// Bump the recorded confirmations for a previously-broadcast tx.
    /// Used by the smoke harness to drive the Broadcast → Confirmed
    /// transition deterministically.
    pub fn set_confirmations(&self, tx_hash: &str, depth: u32) -> Result<(), ChainError> {
        let mut s = self.state.write();
        let entry = s
            .txs
            .get_mut(tx_hash)
            .ok_or_else(|| ChainError::TxNotFound(tx_hash.into()))?;
        entry.confirmations = depth;
        Ok(())
    }
}

fn default_fee_for(chain: ChainId) -> i128 {
    match chain {
        ChainId::Eth => 21_000_i128 * 30_000_000_000_i128, // 21k gas × 30 gwei
        ChainId::Btc => 1_000_i128, // 1k sat — placeholder
        ChainId::Sol => 5_000_i128, // 5k lamports — placeholder
    }
}

impl ChainAdapter for InMemoryChainAdapter {
    fn chain(&self) -> ChainId {
        self.chain
    }

    fn confirmations(&self, tx_hash: &str) -> Result<u32, ChainError> {
        let s = self.state.read();
        Ok(s.txs.get(tx_hash).map(|t| t.confirmations).unwrap_or(0))
    }

    fn balance(&self, address: &str) -> Result<i128, ChainError> {
        let s = self.state.read();
        Ok(s.balances.get(address).map(|b| b.balance).unwrap_or(0))
    }

    fn fee_estimate(&self, urgency: FeeUrgency) -> Result<i128, ChainError> {
        // Stub: linear bump per urgency tier.
        let multiplier = match urgency {
            FeeUrgency::Economy => 1,
            FeeUrgency::Standard => 2,
            FeeUrgency::Urgent => 4,
        };
        Ok(self.fee_per_urgency * multiplier)
    }

    fn build_withdrawal(
        &self,
        from: &str,
        to: &str,
        amount: i128,
        fee: i128,
    ) -> Result<UnsignedTx, ChainError> {
        if from.is_empty() {
            return Err(ChainError::InvalidAddress {
                chain: self.chain,
                detail: "from is empty".into(),
            });
        }
        if to.is_empty() {
            return Err(ChainError::InvalidAddress {
                chain: self.chain,
                detail: "to is empty".into(),
            });
        }
        if amount <= 0 {
            return Err(ChainError::InvalidAmount(amount));
        }
        let s = self.state.read();
        let have = s.balances.get(from).map(|b| b.balance).unwrap_or(0);
        let need = amount + fee;
        if have < need {
            return Err(ChainError::InsufficientBalance {
                chain: self.chain,
                have,
                need,
            });
        }
        Ok(UnsignedTx {
            chain: self.chain,
            from: from.into(),
            to: to.into(),
            amount,
            fee,
            payload: format!("stub-payload:{from}->{to}:{amount}"),
        })
    }

    fn sign_and_broadcast(&self, tx: &UnsignedTx) -> Result<String, ChainError> {
        let mut s = self.state.write();
        let from_bal = s.balances.get(&tx.from).map(|b| b.balance).unwrap_or(0);
        let need = tx.amount + tx.fee;
        if from_bal < need {
            return Err(ChainError::InsufficientBalance {
                chain: self.chain,
                have: from_bal,
                need,
            });
        }
        // Debit `from`, credit `to`. (Real adapters publish to RPC and
        // the on-chain state catches up async; the stub is synchronous.)
        s.balances.insert(
            tx.from.clone(),
            StubBalance {
                balance: from_bal - need,
            },
        );
        let to_bal = s.balances.get(&tx.to).map(|b| b.balance).unwrap_or(0);
        s.balances.insert(
            tx.to.clone(),
            StubBalance {
                balance: to_bal + tx.amount,
            },
        );
        s.next_tx_seq += 1;
        let tx_hash = format!("stub-tx-{}-{:016x}", self.chain, s.next_tx_seq);
        s.txs.insert(tx_hash.clone(), StubTx { confirmations: 0 });
        Ok(tx_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_estimate_scales_with_urgency() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        let economy = adapter.fee_estimate(FeeUrgency::Economy).unwrap();
        let standard = adapter.fee_estimate(FeeUrgency::Standard).unwrap();
        let urgent = adapter.fee_estimate(FeeUrgency::Urgent).unwrap();
        assert!(standard > economy);
        assert!(urgent > standard);
    }

    #[test]
    fn balance_returns_zero_for_unknown_address() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        assert_eq!(adapter.balance("0xunknown").unwrap(), 0);
    }

    #[test]
    fn seed_balance_is_observable() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        adapter.seed_balance("0xhot", 1_000_000_i128);
        assert_eq!(adapter.balance("0xhot").unwrap(), 1_000_000_i128);
    }

    #[test]
    fn build_withdrawal_rejects_insufficient_balance() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        adapter.seed_balance("0xhot", 100_i128);
        let err = adapter
            .build_withdrawal("0xhot", "0xdest", 1000, 10)
            .unwrap_err();
        match err {
            ChainError::InsufficientBalance { have, need, .. } => {
                assert_eq!(have, 100);
                assert_eq!(need, 1010);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn build_withdrawal_rejects_invalid_amount() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        adapter.seed_balance("0xhot", 1_000_000_i128);
        let err = adapter
            .build_withdrawal("0xhot", "0xdest", -1, 10)
            .unwrap_err();
        assert!(matches!(err, ChainError::InvalidAmount(-1)));
    }

    #[test]
    fn build_withdrawal_rejects_empty_addresses() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        adapter.seed_balance("0xhot", 1_000_000_i128);
        assert!(matches!(
            adapter.build_withdrawal("", "0xdest", 100, 1),
            Err(ChainError::InvalidAddress { .. })
        ));
        assert!(matches!(
            adapter.build_withdrawal("0xhot", "", 100, 1),
            Err(ChainError::InvalidAddress { .. })
        ));
    }

    #[test]
    fn broadcast_debits_from_credits_to_and_returns_tx_hash() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        adapter.seed_balance("0xhot", 1_000_i128);
        let tx = adapter
            .build_withdrawal("0xhot", "0xdest", 500, 10)
            .unwrap();
        let hash = adapter.sign_and_broadcast(&tx).unwrap();
        assert!(hash.starts_with("stub-tx-eth-"));
        assert_eq!(adapter.balance("0xhot").unwrap(), 1_000 - 500 - 10);
        assert_eq!(adapter.balance("0xdest").unwrap(), 500);
        // Newly-broadcast tx has 0 confirmations.
        assert_eq!(adapter.confirmations(&hash).unwrap(), 0);
    }

    #[test]
    fn set_confirmations_drives_status_polling() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        adapter.seed_balance("0xhot", 1_000_i128);
        let tx = adapter.build_withdrawal("0xhot", "0xdest", 500, 10).unwrap();
        let hash = adapter.sign_and_broadcast(&tx).unwrap();
        adapter.set_confirmations(&hash, 25).unwrap();
        assert_eq!(adapter.confirmations(&hash).unwrap(), 25);
    }

    #[test]
    fn set_confirmations_for_unknown_tx_errors() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        let err = adapter.set_confirmations("not-broadcast", 1).unwrap_err();
        assert!(matches!(err, ChainError::TxNotFound(_)));
    }

    #[test]
    fn confirmations_returns_zero_for_unknown_hash() {
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        assert_eq!(adapter.confirmations("never-seen").unwrap(), 0);
    }

    #[test]
    fn double_broadcast_continues_to_debit() {
        // Each broadcast is independent — sending twice debits twice.
        // Caller is responsible for idempotency (the WithdrawalStore
        // tracks tx_hash and refuses to re-broadcast for a given
        // withdrawal_id).
        let adapter = InMemoryChainAdapter::new(ChainId::Eth);
        adapter.seed_balance("0xhot", 1_000_i128);
        let tx1 = adapter.build_withdrawal("0xhot", "0xdest", 100, 1).unwrap();
        let h1 = adapter.sign_and_broadcast(&tx1).unwrap();
        let tx2 = adapter.build_withdrawal("0xhot", "0xdest", 100, 1).unwrap();
        let h2 = adapter.sign_and_broadcast(&tx2).unwrap();
        assert_ne!(h1, h2);
        assert_eq!(adapter.balance("0xdest").unwrap(), 200);
    }

    #[test]
    fn chain_id_observable() {
        let eth = InMemoryChainAdapter::new(ChainId::Eth);
        let btc = InMemoryChainAdapter::new(ChainId::Btc);
        assert_eq!(eth.chain(), ChainId::Eth);
        assert_eq!(btc.chain(), ChainId::Btc);
    }

    #[test]
    fn adapter_is_send_sync_for_arc_dyn_use() {
        // Compile-time property: ChainAdapter trait objects can be
        // wrapped in Arc and shared across tasks.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<dyn ChainAdapter>>();
    }
}
