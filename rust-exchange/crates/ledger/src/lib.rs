use anyhow::{bail, Result};
use dashmap::DashMap;
use eventbus::EventBus;
use parking_lot::{Mutex, RwLock};
use persistence::{InMemoryWal, WalStore};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use types::{Account, Event, LedgerDelta, LedgerEntry, RejectReason};

const LOCK_SHARDS: usize = 64;

#[derive(Clone)]
pub struct LedgerService {
    accounts: Arc<DashMap<String, Account>>,
    seen_op_ids: Arc<RwLock<HashSet<String>>>,
    pruned_command_seq_floor: Arc<RwLock<Option<u64>>>,
    event_bus: EventBus,
    wal_store: Arc<dyn WalStore<LedgerDelta>>,
    lifecycle_lock: Arc<RwLock<()>>,
    account_locks: Arc<Vec<Mutex<()>>>,
    op_id_locks: Arc<Vec<Mutex<()>>>,
}

pub struct SpotTradeSettlement<'a> {
    pub buy_user_id: &'a str,
    pub sell_user_id: &'a str,
    pub market_id: &'a str,
    pub outcome: i32,
    pub price: i64,
    pub amount: i64,
    pub op_id: String,
}

impl LedgerService {
    pub fn new(event_bus: EventBus) -> Self {
        Self::with_wal_store(event_bus, Arc::new(InMemoryWal::new()))
    }

    pub fn with_wal_store(event_bus: EventBus, wal_store: Arc<dyn WalStore<LedgerDelta>>) -> Self {
        Self {
            accounts: Arc::new(DashMap::new()),
            seen_op_ids: Arc::new(RwLock::new(HashSet::new())),
            pruned_command_seq_floor: Arc::new(RwLock::new(None)),
            event_bus,
            wal_store,
            lifecycle_lock: Arc::new(RwLock::new(())),
            account_locks: Arc::new((0..LOCK_SHARDS).map(|_| Mutex::new(())).collect()),
            op_id_locks: Arc::new((0..LOCK_SHARDS).map(|_| Mutex::new(())).collect()),
        }
    }

    pub fn recover_from_wal(&self) -> Result<usize> {
        let _lifecycle_guard = self.lifecycle_lock.write();
        let deltas = self.wal_store.entries()?;

        self.accounts.clear();
        self.seen_op_ids.write().clear();
        *self.pruned_command_seq_floor.write() = None;

        for delta in &deltas {
            self.apply_delta_from_wal(delta)?;
        }

        Ok(deltas.len())
    }

    pub fn commit_delta(&self, delta: LedgerDelta) -> Result<()> {
        if self.commit_delta_if_absent(delta.clone())? {
            return Ok(());
        }

        self.publish_rejection(&delta.op_id, RejectReason::DuplicateOp);
        bail!("duplicate op_id: {}", delta.op_id);
    }

    pub fn commit_delta_if_absent(&self, delta: LedgerDelta) -> Result<bool> {
        let _lifecycle_guard = self.lifecycle_lock.read();

        if delta.op_id.trim().is_empty() {
            self.publish_rejection(&delta.op_id, RejectReason::InvalidEntry);
            bail!("invalid op_id: empty");
        }

        self.validate_balance(&delta.entries)?;
        let affected_accounts = self.get_affected_accounts(&delta.entries);
        let _op_guard = self.op_id_locks[lock_shard(&delta.op_id)].lock();
        let _account_guards = self.lock_account_shards(affected_accounts.keys());

        {
            let seen = self.seen_op_ids.read();
            if seen.contains(&delta.op_id) {
                return Ok(false);
            }
        }
        if self.should_check_pruned_wal(&delta.op_id) && self.wal_contains_op_id(&delta.op_id)? {
            return Ok(false);
        }

        self.verify_sufficient_balance(&delta.entries, &affected_accounts)?;
        self.append_wal(&delta)?;
        self.apply_entries(&delta.entries, &affected_accounts);
        self.bump_versions(&affected_accounts);

        {
            let mut seen = self.seen_op_ids.write();
            seen.insert(delta.op_id.clone());
        }

        self.event_bus.publish(Event::LedgerCommitted(delta));

        Ok(true)
    }

    pub fn wal_entries(&self) -> Result<Vec<LedgerDelta>> {
        self.wal_store.entries()
    }

    fn append_wal(&self, delta: &LedgerDelta) -> Result<()> {
        self.wal_store.append(delta)
    }

    pub fn prune_seen_op_ids_up_to(&self, command_seq: u64) -> usize {
        let _lifecycle_guard = self.lifecycle_lock.write();
        let mut seen = self.seen_op_ids.write();
        let before = seen.len();
        seen.retain(|op_id| parse_command_seq(op_id).is_none_or(|seq| seq > command_seq));
        let mut floor = self.pruned_command_seq_floor.write();
        *floor = Some(floor.map_or(command_seq, |current| current.max(command_seq)));
        before.saturating_sub(seen.len())
    }

    fn wal_contains_op_id(&self, op_id: &str) -> Result<bool> {
        Ok(self
            .wal_store
            .entries()?
            .into_iter()
            .any(|entry| entry.op_id == op_id))
    }

    fn should_check_pruned_wal(&self, op_id: &str) -> bool {
        let Some(command_seq) = parse_command_seq(op_id) else {
            return false;
        };
        self.pruned_command_seq_floor
            .read()
            .is_some_and(|floor| command_seq <= floor)
    }

    fn apply_delta_from_wal(&self, delta: &LedgerDelta) -> Result<()> {
        if self.seen_op_ids.read().contains(&delta.op_id) {
            bail!("duplicate op_id in wal replay: {}", delta.op_id);
        }

        self.validate_balance(&delta.entries)?;
        let affected_accounts = self.get_affected_accounts(&delta.entries);
        self.verify_sufficient_balance(&delta.entries, &affected_accounts)?;
        self.apply_entries(&delta.entries, &affected_accounts);
        self.bump_versions(&affected_accounts);
        self.seen_op_ids.write().insert(delta.op_id.clone());
        Ok(())
    }

    fn bump_versions(&self, affected_accounts: &HashMap<String, Account>) {
        for account_id in affected_accounts.keys() {
            if let Some(mut acc) = self.accounts.get_mut(account_id) {
                acc.version += 1;
            }
        }
    }

    fn validate_balance(&self, entries: &[LedgerEntry]) -> Result<()> {
        if entries.is_empty() {
            bail!("ledger delta has no entries");
        }

        let mut sum_debits = 0i64;
        let mut sum_credits = 0i64;

        for entry in entries {
            if entry.amount <= 0 {
                bail!("invalid amount for op {}: {}", entry.op_id, entry.amount);
            }
            if entry.debit_account.trim().is_empty() || entry.credit_account.trim().is_empty() {
                bail!("invalid entry: empty account");
            }
            if entry.debit_account == entry.credit_account {
                bail!(
                    "invalid entry: debit and credit account are the same: {}",
                    entry.debit_account
                );
            }
            sum_debits += entry.amount;
            sum_credits += entry.amount;
        }

        if sum_debits != sum_credits {
            bail!("debits and credits not balanced: debits={sum_debits}, credits={sum_credits}");
        }

        Ok(())
    }

    fn get_affected_accounts(&self, entries: &[LedgerEntry]) -> HashMap<String, Account> {
        let mut accounts = HashMap::new();

        for entry in entries {
            if !self.accounts.contains_key(&entry.debit_account) {
                self.accounts.insert(
                    entry.debit_account.clone(),
                    Account {
                        id: entry.debit_account.clone(),
                        balance: 0,
                        version: 0,
                        account_type: String::new(),
                    },
                );
            }
            if !self.accounts.contains_key(&entry.credit_account) {
                self.accounts.insert(
                    entry.credit_account.clone(),
                    Account {
                        id: entry.credit_account.clone(),
                        balance: 0,
                        version: 0,
                        account_type: String::new(),
                    },
                );
            }

            if let Some(acc) = self.accounts.get(&entry.debit_account) {
                accounts.insert(entry.debit_account.clone(), acc.clone());
            }
            if let Some(acc) = self.accounts.get(&entry.credit_account) {
                accounts.insert(entry.credit_account.clone(), acc.clone());
            }
        }

        accounts
    }

    fn verify_sufficient_balance(
        &self,
        entries: &[LedgerEntry],
        accounts: &HashMap<String, Account>,
    ) -> Result<()> {
        let mut balance_changes: HashMap<String, i64> = HashMap::new();

        for entry in entries {
            *balance_changes
                .entry(entry.debit_account.clone())
                .or_insert(0) -= entry.amount;
            *balance_changes
                .entry(entry.credit_account.clone())
                .or_insert(0) += entry.amount;
        }

        for (account_id, change) in balance_changes {
            if account_id.starts_with("SYS:") {
                continue;
            }

            let current_balance = accounts.get(&account_id).map(|a| a.balance).unwrap_or(0);
            let new_balance = current_balance + change;

            if new_balance < 0 && !allows_negative_balance(&account_id) {
                self.publish_rejection(&account_id, RejectReason::InsufficientFunds);
                bail!(
                    "insufficient balance: account={account_id}, balance={current_balance}, change={change}"
                );
            }
        }

        Ok(())
    }

    fn apply_entries(&self, entries: &[LedgerEntry], _accounts: &HashMap<String, Account>) {
        for entry in entries {
            if let Some(mut acc) = self.accounts.get_mut(&entry.debit_account) {
                acc.balance -= entry.amount;
            }
            if let Some(mut acc) = self.accounts.get_mut(&entry.credit_account) {
                acc.balance += entry.amount;
            }
        }
    }

    fn publish_rejection(&self, op_id: &str, reason: RejectReason) {
        self.event_bus.publish(Event::LedgerRejected {
            op_id: op_id.to_string(),
            reason,
        });
    }

    pub fn get_balance(&self, account_id: &str) -> i64 {
        self.accounts
            .get(account_id)
            .map(|acc| acc.balance)
            .unwrap_or(0)
    }

    pub fn has_seen_op_id(&self, op_id: &str) -> bool {
        self.seen_op_ids.read().contains(op_id)
    }

    fn lock_account_shards<'a, I>(&'a self, account_ids: I) -> Vec<parking_lot::MutexGuard<'a, ()>>
    where
        I: Iterator<Item = &'a String>,
    {
        let mut shards = account_ids
            .map(|account_id| lock_shard(account_id))
            .collect::<Vec<_>>();
        shards.sort_unstable();
        shards.dedup();
        shards
            .into_iter()
            .map(|shard| self.account_locks[shard].lock())
            .collect()
    }

    pub fn cash_account(user_id: &str) -> String {
        format!("U:{user_id}:USDC")
    }

    pub fn cash_hold_account(user_id: &str) -> String {
        format!("U:{user_id}:USDC:HOLD")
    }

    pub fn position_account(user_id: &str, market_id: &str, outcome: i32) -> String {
        format!("U:{user_id}:{market_id}:{outcome}")
    }

    pub fn position_hold_account(user_id: &str, market_id: &str, outcome: i32) -> String {
        format!("U:{user_id}:{market_id}:{outcome}:HOLD")
    }

    pub fn derivative_position_account(user_id: &str, market_id: &str, outcome: i32) -> String {
        format!("U:{user_id}:DERIV:{market_id}:{outcome}")
    }

    pub fn insurance_fund_account() -> String {
        "SYS:INSURANCE_FUND:USDC".to_string()
    }

    pub fn cash_available_balance(&self, user_id: &str) -> i64 {
        self.get_balance(&Self::cash_account(user_id))
    }

    pub fn cash_hold_balance(&self, user_id: &str) -> i64 {
        self.get_balance(&Self::cash_hold_account(user_id))
    }

    pub fn position_available_balance(&self, user_id: &str, market_id: &str, outcome: i32) -> i64 {
        self.get_balance(&Self::position_account(user_id, market_id, outcome))
    }

    pub fn position_hold_balance(&self, user_id: &str, market_id: &str, outcome: i32) -> i64 {
        self.get_balance(&Self::position_hold_account(user_id, market_id, outcome))
    }

    pub fn derivative_position_balance(&self, user_id: &str, market_id: &str, outcome: i32) -> i64 {
        self.get_balance(&Self::derivative_position_account(
            user_id, market_id, outcome,
        ))
    }

    pub fn balances_for_user(&self, user_id: &str) -> HashMap<String, i64> {
        let prefix = format!("U:{user_id}:");
        self.accounts
            .iter()
            .filter_map(|entry| {
                if entry.key().starts_with(&prefix) {
                    Some((entry.key().clone(), entry.value().balance))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn user_ids(&self) -> Vec<String> {
        let mut users = HashSet::new();
        for entry in self.accounts.iter() {
            let account_id = entry.key();
            if let Some(rest) = account_id.strip_prefix("U:") {
                if let Some((user_id, _)) = rest.split_once(':') {
                    if !user_id.trim().is_empty() {
                        users.insert(user_id.to_string());
                    }
                }
            }
        }
        let mut items: Vec<_> = users.into_iter().collect();
        items.sort();
        items
    }

    pub fn create_cash_hold(&self, user_id: &str, amount: i64, op_id: String) -> Result<()> {
        let delta = LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![LedgerEntry {
                debit_account: Self::cash_account(user_id),
                credit_account: Self::cash_hold_account(user_id),
                amount,
                op_id,
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };
        self.commit_delta(delta)
    }

    pub fn release_cash_hold(&self, user_id: &str, amount: i64, op_id: String) -> Result<()> {
        let delta = LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![LedgerEntry {
                debit_account: Self::cash_hold_account(user_id),
                credit_account: Self::cash_account(user_id),
                amount,
                op_id,
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };
        self.commit_delta(delta)
    }

    pub fn create_position_hold(
        &self,
        user_id: &str,
        market_id: &str,
        outcome: i32,
        amount: i64,
        op_id: String,
    ) -> Result<()> {
        let delta = LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![LedgerEntry {
                debit_account: Self::position_account(user_id, market_id, outcome),
                credit_account: Self::position_hold_account(user_id, market_id, outcome),
                amount,
                op_id,
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };
        self.commit_delta(delta)
    }

    pub fn release_position_hold(
        &self,
        user_id: &str,
        market_id: &str,
        outcome: i32,
        amount: i64,
        op_id: String,
    ) -> Result<()> {
        let delta = LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![LedgerEntry {
                debit_account: Self::position_hold_account(user_id, market_id, outcome),
                credit_account: Self::position_account(user_id, market_id, outcome),
                amount,
                op_id,
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };
        self.commit_delta(delta)
    }

    pub fn process_position_deposit(
        &self,
        user_id: &str,
        market_id: &str,
        outcome: i32,
        amount: i64,
        op_id: String,
    ) -> Result<()> {
        let asset_account = Self::position_account(user_id, market_id, outcome);
        let delta = LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![LedgerEntry {
                debit_account: format!("SYS:POSITION_VAULT:{market_id}:{outcome}"),
                credit_account: asset_account,
                amount,
                op_id,
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };

        self.commit_delta(delta)
    }

    pub fn settle_trade(&self, trade: SpotTradeSettlement<'_>) -> Result<()> {
        let SpotTradeSettlement {
            buy_user_id,
            sell_user_id,
            market_id,
            outcome,
            price,
            amount,
            op_id,
        } = trade;
        let notional = price.checked_mul(amount).ok_or_else(|| {
            anyhow::anyhow!("trade notional overflow: price={price} amount={amount}")
        })?;
        let delta = LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![
                LedgerEntry {
                    debit_account: Self::cash_hold_account(buy_user_id),
                    credit_account: Self::cash_account(sell_user_id),
                    amount: notional,
                    op_id: format!("{op_id}:cash"),
                    timestamp: chrono::Utc::now(),
                },
                LedgerEntry {
                    debit_account: Self::position_hold_account(sell_user_id, market_id, outcome),
                    credit_account: Self::position_account(buy_user_id, market_id, outcome),
                    amount,
                    op_id: format!("{op_id}:position"),
                    timestamp: chrono::Utc::now(),
                },
            ],
            timestamp: chrono::Utc::now(),
        };

        self.commit_delta(delta)
    }

    pub fn settle_derivative_trade(
        &self,
        buy_user_id: &str,
        sell_user_id: &str,
        market_id: &str,
        outcome: i32,
        amount: i64,
        op_id: String,
    ) -> Result<()> {
        let delta = LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![LedgerEntry {
                debit_account: Self::derivative_position_account(sell_user_id, market_id, outcome),
                credit_account: Self::derivative_position_account(buy_user_id, market_id, outcome),
                amount,
                op_id,
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };

        self.commit_delta(delta)
    }

    pub fn process_deposit(&self, user_id: &str, amount: i64, op_id: String) -> Result<()> {
        let delta = LedgerDelta {
            op_id,
            entries: vec![LedgerEntry {
                debit_account: "SYS:ONCHAIN_VAULT:USDC".to_string(),
                credit_account: format!("U:{user_id}:USDC"),
                amount,
                op_id: format!("deposit_{user_id}"),
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };

        self.commit_delta(delta)
    }

    pub fn transfer_cash(
        &self,
        from_user_id: &str,
        to_user_id: &str,
        amount: i64,
        op_id: String,
    ) -> Result<()> {
        if from_user_id.trim().is_empty() || to_user_id.trim().is_empty() {
            bail!("cash transfer users must be non-empty");
        }
        if from_user_id == to_user_id {
            bail!("cash transfer users must differ");
        }
        self.transfer_cash_between_accounts(
            &Self::cash_account(from_user_id),
            &Self::cash_account(to_user_id),
            amount,
            op_id,
        )
    }

    pub fn transfer_cash_between_accounts(
        &self,
        from_account_id: &str,
        to_account_id: &str,
        amount: i64,
        op_id: String,
    ) -> Result<()> {
        if amount <= 0 {
            bail!("cash transfer amount must be positive: {amount}");
        }
        if from_account_id.trim().is_empty() || to_account_id.trim().is_empty() {
            bail!("cash transfer accounts must be non-empty");
        }
        if from_account_id == to_account_id {
            bail!("cash transfer accounts must differ");
        }

        let delta = LedgerDelta {
            op_id: op_id.clone(),
            entries: vec![LedgerEntry {
                debit_account: from_account_id.to_string(),
                credit_account: to_account_id.to_string(),
                amount,
                op_id,
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };

        self.commit_delta(delta)
    }

    pub fn deposit_insurance_fund(&self, amount: i64, op_id: String) -> Result<()> {
        self.transfer_cash_between_accounts(
            "SYS:ONCHAIN_VAULT:USDC",
            &Self::insurance_fund_account(),
            amount,
            op_id,
        )
    }

    pub fn insurance_fund_balance(&self) -> i64 {
        self.get_balance(&Self::insurance_fund_account())
    }
}

fn lock_shard(value: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    (hasher.finish() as usize) % LOCK_SHARDS
}

fn parse_command_seq(value: &str) -> Option<u64> {
    let marker = "seq-";
    let start = value.find(marker)? + marker.len();
    let digits = value[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn allows_negative_balance(account_id: &str) -> bool {
    account_id.contains(":DERIV:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use persistence::InMemoryWal;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn deposit_delta(user_id: &str, amount: i64, op_id: &str) -> LedgerDelta {
        LedgerDelta {
            op_id: op_id.to_string(),
            entries: vec![LedgerEntry {
                debit_account: "SYS:ONCHAIN_VAULT:USDC".to_string(),
                credit_account: format!("U:{user_id}:USDC"),
                amount,
                op_id: format!("entry_{op_id}"),
                timestamp: Utc::now(),
            }],
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn transfer_cash_moves_balance_between_users() {
        let ledger = LedgerService::new(EventBus::new());
        ledger
            .process_deposit("payer", 100, "dep-payer".to_string())
            .unwrap();

        ledger
            .transfer_cash("payer", "receiver", 40, "transfer-1".to_string())
            .unwrap();

        assert_eq!(ledger.get_balance("U:payer:USDC"), 60);
        assert_eq!(ledger.get_balance("U:receiver:USDC"), 40);
    }

    #[test]
    fn transfer_cash_rejects_insufficient_balance() {
        let ledger = LedgerService::new(EventBus::new());
        let err = ledger
            .transfer_cash("payer", "receiver", 40, "transfer-2".to_string())
            .unwrap_err();

        assert!(err.to_string().contains("insufficient balance"));
        assert_eq!(ledger.get_balance("U:payer:USDC"), 0);
        assert_eq!(ledger.get_balance("U:receiver:USDC"), 0);
    }

    #[test]
    fn insurance_fund_deposit_and_transfer_work() {
        let ledger = LedgerService::new(EventBus::new());
        ledger
            .deposit_insurance_fund(250, "if-dep-1".to_string())
            .unwrap();
        ledger
            .transfer_cash_between_accounts(
                &LedgerService::insurance_fund_account(),
                &LedgerService::cash_account("liquidator"),
                40,
                "if-pay-1".to_string(),
            )
            .unwrap();

        assert_eq!(ledger.insurance_fund_balance(), 210);
        assert_eq!(ledger.cash_available_balance("liquidator"), 40);
    }

    #[test]
    fn duplicate_op_id_is_rejected_without_double_credit() {
        let ledger = LedgerService::new(EventBus::new());

        ledger
            .commit_delta(deposit_delta("user1", 100, "dup-op"))
            .unwrap();
        let err = ledger
            .commit_delta(deposit_delta("user1", 100, "dup-op"))
            .unwrap_err();

        assert!(err.to_string().contains("duplicate op_id"));
        assert_eq!(ledger.get_balance("U:user1:USDC"), 100);
        assert_eq!(ledger.wal_entries().unwrap().len(), 1);
        assert!(ledger.seen_op_ids.read().contains("dup-op"));
    }

    #[test]
    fn failed_commit_does_not_consume_op_id_or_write_wal() {
        let ledger = LedgerService::new(EventBus::new());

        let err = ledger
            .commit_delta(deposit_delta("user2", 0, "retryable-op"))
            .unwrap_err();

        assert!(err.to_string().contains("invalid amount"));
        assert!(!ledger.seen_op_ids.read().contains("retryable-op"));
        assert_eq!(ledger.wal_entries().unwrap().len(), 0);

        ledger
            .commit_delta(deposit_delta("user2", 250, "retryable-op"))
            .unwrap();

        assert_eq!(ledger.get_balance("U:user2:USDC"), 250);
        assert_eq!(ledger.wal_entries().unwrap().len(), 1);
    }

    #[test]
    fn recover_from_wal_rebuilds_balances_and_seen_ops() {
        let wal = Arc::new(InMemoryWal::<LedgerDelta>::new());
        let ledger = LedgerService::with_wal_store(EventBus::new(), wal.clone());
        ledger
            .commit_delta(deposit_delta("user4", 200, "replay-op-1"))
            .unwrap();
        ledger
            .commit_delta(deposit_delta("user4", 50, "replay-op-2"))
            .unwrap();

        let recovered = LedgerService::with_wal_store(EventBus::new(), wal);
        assert_eq!(recovered.recover_from_wal().unwrap(), 2);
        assert_eq!(recovered.get_balance("U:user4:USDC"), 250);
        assert!(recovered.seen_op_ids.read().contains("replay-op-1"));
        assert!(recovered.seen_op_ids.read().contains("replay-op-2"));
    }

    #[test]
    fn concurrent_duplicate_op_id_commits_only_once() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        let barrier = Arc::new(Barrier::new(8));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let ledger = ledger.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    ledger
                        .commit_delta(deposit_delta("user3", 75, "shared-op"))
                        .is_ok()
                })
            })
            .collect();

        let successes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|ok| *ok)
            .count();

        assert_eq!(successes, 1);
        assert_eq!(ledger.get_balance("U:user3:USDC"), 75);
        assert_eq!(ledger.wal_entries().unwrap().len(), 1);
        assert!(ledger.seen_op_ids.read().contains("shared-op"));
    }
}
