//! Wallet, custody, and withdrawal type model.
//!
//! Step 7A of the wallet implementation track (per docs/WALLET_DESIGN.md
//! §10.5). Type model only — no storage, no chain adapter, no REST
//! handlers, no behaviour change. Subsequent sub-steps:
//!
//!   7B  AddressBookStore (JSONL-backed, mirrors AdminEmployeeStore).
//!   7C  WithdrawalStore + state machine (Submitted -> Settled).
//!   7D  ChainAdapter trait + in-memory stub adapter for tests.
//!   7E  Real ETH adapter (live RPC behind a feature flag).
//!   7F  Operator REST handlers under /admin/wallet/*.
//!   7G  Runtime activation + wallet_smoke_test.ps1.
//!
//! All monetary amounts use the smallest indivisible unit per chain
//! (wei for ETH, satoshi for BTC, lamports for SOL). The `i128` width
//! covers Bitcoin's 21M-coin × 1e8 satoshi supply with room to spare
//! for arithmetic without overflow concerns.

pub mod address_book;
pub mod types;
pub mod withdrawal_store;

pub use address_book::AddressBookStore;
pub use types::{
    AddressStatus, ChainId, FeeUrgency, SanctionsCheckResult, SanctionsHit, SanctionsScreenStatus,
    WalletTier, WithdrawalAddress, WithdrawalAddressId, WithdrawalRecord, WithdrawalRejectReason,
    WithdrawalStatus, WALLET_SCHEMA_VERSION,
};
pub use withdrawal_store::{is_valid_transition, WithdrawalStore};
