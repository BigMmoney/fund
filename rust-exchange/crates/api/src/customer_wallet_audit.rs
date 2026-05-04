//! Customer-wallet audit log (H4).
//!
//! Every add/submit/poll attempt against `/v2/wallet/*` lands one
//! line in `data/wallet/customer_audit.jsonl`. Captures success
//! AND failure outcomes (rejected sanctions, velocity, balance,
//! ad-hoc destination) so an investigation has a complete trail
//! independent of the WithdrawalStore (which only records what
//! made it past validation).
//!
//! Mirror of `admin_rbac_audit` but for the customer surface.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use persistence::{JsonlFileWal, WalStore};
use wallet::ChainId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomerWalletAction {
    AddAddress,
    RemoveAddress,
    SubmitWithdraw,
    PollWithdrawal,
    ListAddresses,
    ListWithdrawals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomerWalletOutcome {
    Ok,
    BadRequest,
    AddressNotFound,
    AddressNotActive,
    SanctionsHit,
    SanctionsUnavailable,
    VelocityExceeded,
    InsufficientBalance,
    AmountTooLarge,
    Forbidden,
    DuplicateRequest,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerWalletAuditRow {
    pub schema_version: u32,
    pub at: DateTime<Utc>,
    pub user_id: String,
    pub action: CustomerWalletAction,
    pub outcome: CustomerWalletOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<ChainId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i128>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub const CUSTOMER_WALLET_AUDIT_SCHEMA_VERSION: u32 = 1;

/// Append-only audit store. Writes are serialised behind a single
/// mutex — audit volume is low (one row per request) so contention
/// is not the bottleneck.
pub struct CustomerWalletAuditStore {
    store: Arc<dyn WalStore<CustomerWalletAuditRow>>,
    write_lock: Mutex<()>,
}

impl CustomerWalletAuditStore {
    pub fn new(store: Arc<dyn WalStore<CustomerWalletAuditRow>>) -> Self {
        Self {
            store,
            write_lock: Mutex::new(()),
        }
    }

    pub fn open_jsonl(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn WalStore<CustomerWalletAuditRow>> = Arc::new(JsonlFileWal::new(path)?);
        Ok(Self::new(store))
    }

    pub fn append(&self, mut row: CustomerWalletAuditRow) {
        row.schema_version = CUSTOMER_WALLET_AUDIT_SCHEMA_VERSION;
        let _guard = self.write_lock.lock();
        if let Err(e) = self.store.append(&row) {
            tracing::warn!(error = %e, "customer wallet audit append failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::InMemoryWal;

    #[test]
    fn append_and_replay() {
        let wal: Arc<dyn WalStore<CustomerWalletAuditRow>> = Arc::new(InMemoryWal::new());
        let store = CustomerWalletAuditStore::new(wal.clone());
        store.append(CustomerWalletAuditRow {
            schema_version: 0,
            at: Utc::now(),
            user_id: "alice".into(),
            action: CustomerWalletAction::SubmitWithdraw,
            outcome: CustomerWalletOutcome::Ok,
            chain: Some(ChainId::Eth),
            destination_address: Some("0xclean".into()),
            amount: Some(1_000),
            withdrawal_id: Some("wd-1".into()),
            client_reference: Some("ref-1".into()),
            note: None,
        });
        let entries = wal.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].user_id, "alice");
        assert_eq!(entries[0].outcome, CustomerWalletOutcome::Ok);
        assert_eq!(entries[0].schema_version, CUSTOMER_WALLET_AUDIT_SCHEMA_VERSION);
    }

    #[test]
    fn rejected_outcomes_are_recorded() {
        let wal: Arc<dyn WalStore<CustomerWalletAuditRow>> = Arc::new(InMemoryWal::new());
        let store = CustomerWalletAuditStore::new(wal.clone());
        for outcome in [
            CustomerWalletOutcome::SanctionsHit,
            CustomerWalletOutcome::VelocityExceeded,
            CustomerWalletOutcome::InsufficientBalance,
            CustomerWalletOutcome::AddressNotFound,
        ] {
            store.append(CustomerWalletAuditRow {
                schema_version: 0,
                at: Utc::now(),
                user_id: "alice".into(),
                action: CustomerWalletAction::SubmitWithdraw,
                outcome,
                chain: Some(ChainId::Eth),
                destination_address: Some("0xx".into()),
                amount: Some(1),
                withdrawal_id: None,
                client_reference: None,
                note: None,
            });
        }
        assert_eq!(wal.entries().unwrap().len(), 4);
    }
}
