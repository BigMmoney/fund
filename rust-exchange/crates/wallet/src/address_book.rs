//! Address book store — whitelisted withdrawal destinations.
//!
//! Step 7B of the wallet implementation track. JSONL-backed,
//! in-memory cached, append-only. Latest record per `address_id`
//! wins on recovery (status / sanctions_check / last_used_at can
//! change). Mirrors the api crate's `AdminEmployeeStore` pattern.
//!
//! Out of scope for this commit:
//! - Sanctions provider integration (caller passes the result in).
//! - Cool-down auto-promotion job (1G activation will spawn one).
//! - REST handlers under `/admin/wallet/addresses/*` (Step 7F).

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use parking_lot::Mutex;

use persistence::{JsonlFileWal, WalStore};

use crate::types::{AddressStatus, ChainId, WithdrawalAddress, WithdrawalAddressId};

const ADDRESS_LOCK_SHARDS: usize = 32;

fn lock_shard(key: &str) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % ADDRESS_LOCK_SHARDS
}

pub struct AddressBookStore {
    by_id: DashMap<WithdrawalAddressId, WithdrawalAddress>,
    store: Arc<dyn WalStore<WithdrawalAddress>>,
    write_locks: Vec<Mutex<()>>,
}

impl AddressBookStore {
    pub fn new(store: Arc<dyn WalStore<WithdrawalAddress>>) -> anyhow::Result<Self> {
        let result = Self {
            by_id: DashMap::new(),
            store,
            write_locks: (0..ADDRESS_LOCK_SHARDS).map(|_| Mutex::new(())).collect(),
        };
        for entry in result.store.entries()? {
            result.by_id.insert(entry.address_id.clone(), entry);
        }
        Ok(result)
    }

    pub fn open_jsonl(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn WalStore<WithdrawalAddress>> = Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub fn get(&self, address_id: &str) -> Option<WithdrawalAddress> {
        self.by_id.get(address_id).map(|e| e.value().clone())
    }

    /// All addresses owned by `user_id`, sorted by `added_at` desc.
    pub fn for_user(&self, user_id: &str) -> Vec<WithdrawalAddress> {
        let mut out: Vec<WithdrawalAddress> = self
            .by_id
            .iter()
            .filter(|e| e.value().user_id == user_id)
            .map(|e| e.value().clone())
            .collect();
        out.sort_by(|a, b| b.added_at.cmp(&a.added_at));
        out
    }

    /// Resolve the literal on-chain address (chain-specific encoding)
    /// to an address record for `user_id`. Returns None if the address
    /// isn't whitelisted for that user (regardless of status). Callers
    /// should additionally check `status == Active` and
    /// `cooldown_until <= now` before treating the result as
    /// withdrawal-eligible.
    pub fn resolve(
        &self,
        user_id: &str,
        chain: ChainId,
        on_chain_address: &str,
    ) -> Option<WithdrawalAddress> {
        self.by_id
            .iter()
            .find(|e| {
                let v = e.value();
                v.user_id == user_id && v.chain == chain && v.address == on_chain_address
            })
            .map(|e| e.value().clone())
    }

    pub fn count(&self) -> usize {
        self.by_id.len()
    }

    /// Insert a fresh address. Returns `Err` if `address_id` is
    /// already present; use `update` for status / sanctions / last-used
    /// changes.
    pub fn create(&self, address: WithdrawalAddress) -> anyhow::Result<()> {
        let key = address.address_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if self.by_id.contains_key(&key) {
            anyhow::bail!("address already exists: {key}");
        }
        self.store.append(&address)?;
        self.by_id.insert(key, address);
        Ok(())
    }

    /// Append an updated record for an existing address. Used for
    /// status flips (PendingCooldown → Active when the cool-down
    /// passes; Active → Suspended on sanctions hit; Active → Removed
    /// on customer remove) and for `last_used_at` bumps.
    pub fn update(&self, address: WithdrawalAddress) -> anyhow::Result<()> {
        let key = address.address_id.clone();
        let _guard = self.write_locks[lock_shard(&key)].lock();
        if !self.by_id.contains_key(&key) {
            anyhow::bail!("address not found: {key}");
        }
        self.store.append(&address)?;
        self.by_id.insert(key, address);
        Ok(())
    }

    /// Convenience: flip an address's status. Returns the post-update
    /// record.
    pub fn set_status(
        &self,
        address_id: &str,
        status: AddressStatus,
    ) -> anyhow::Result<WithdrawalAddress> {
        let mut current = self
            .get(address_id)
            .ok_or_else(|| anyhow::anyhow!("address not found: {address_id}"))?;
        current.status = status;
        self.update(current.clone())?;
        Ok(current)
    }

    /// Promote `PendingCooldown` addresses past their cool-down
    /// window to `Active`. Returns the count promoted. Callers run
    /// this periodically from a tokio interval task.
    pub fn sweep_cooldowns(&self) -> anyhow::Result<usize> {
        let now = Utc::now();
        let to_promote: Vec<WithdrawalAddress> = self
            .by_id
            .iter()
            .filter(|e| {
                let v = e.value();
                v.status == AddressStatus::PendingCooldown && v.cooldown_until <= now
            })
            .map(|e| e.value().clone())
            .collect();
        let n = to_promote.len();
        for mut addr in to_promote {
            addr.status = AddressStatus::Active;
            self.update(addr)?;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        SanctionsCheckResult, SanctionsScreenStatus, WithdrawalAddress, WALLET_SCHEMA_VERSION,
    };
    use chrono::Duration;
    use persistence::InMemoryWal;

    fn ts(secs: i64) -> chrono::DateTime<Utc> {
        chrono::TimeZone::timestamp_opt(&Utc, 1_700_000_000 + secs, 0).unwrap()
    }

    fn sample_check() -> SanctionsCheckResult {
        SanctionsCheckResult {
            status: SanctionsScreenStatus::Clear,
            provider: "chainalysis".into(),
            checked_at: ts(0),
            hit: None,
            correlation_id: None,
        }
    }

    fn make_addr(
        id: &str,
        user: &str,
        chain: ChainId,
        on_chain: &str,
        status: AddressStatus,
        cooldown_offset_secs: i64,
    ) -> WithdrawalAddress {
        WithdrawalAddress {
            schema_version: WALLET_SCHEMA_VERSION,
            address_id: id.into(),
            user_id: user.into(),
            chain,
            address: on_chain.into(),
            label: format!("label-{id}"),
            status,
            added_at: Utc::now(),
            cooldown_until: Utc::now() + Duration::seconds(cooldown_offset_secs),
            last_used_at: None,
            sanctions_check: sample_check(),
            added_by: user.into(),
        }
    }

    #[test]
    fn create_then_get() {
        let s = AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_addr("a-1", "alice", ChainId::Eth, "0xabc", AddressStatus::Active, -10))
            .unwrap();
        assert_eq!(s.count(), 1);
        assert_eq!(s.get("a-1").unwrap().address, "0xabc");
    }

    #[test]
    fn create_duplicate_rejected() {
        let s = AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_addr("a-1", "alice", ChainId::Eth, "0xabc", AddressStatus::Active, -10))
            .unwrap();
        let err = s
            .create(make_addr("a-1", "alice", ChainId::Eth, "0xdef", AddressStatus::Active, -10))
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn for_user_filters_by_owner_and_sorts_newest_first() {
        let s = AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap();
        let mut a = make_addr("a-old", "alice", ChainId::Eth, "0xa", AddressStatus::Active, -10);
        a.added_at = ts(0);
        let mut b = make_addr("a-new", "alice", ChainId::Eth, "0xb", AddressStatus::Active, -10);
        b.added_at = ts(100);
        let c = make_addr("a-bob", "bob", ChainId::Eth, "0xc", AddressStatus::Active, -10);
        s.create(a).unwrap();
        s.create(b).unwrap();
        s.create(c).unwrap();
        let listed = s.for_user("alice");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].address_id, "a-new");
        assert_eq!(listed[1].address_id, "a-old");
    }

    #[test]
    fn resolve_matches_user_chain_and_address() {
        let s = AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap();
        s.create(make_addr("a-1", "alice", ChainId::Eth, "0xabc", AddressStatus::Active, -10))
            .unwrap();
        // Match
        assert_eq!(s.resolve("alice", ChainId::Eth, "0xabc").unwrap().address_id, "a-1");
        // Wrong user
        assert!(s.resolve("bob", ChainId::Eth, "0xabc").is_none());
        // Wrong chain
        assert!(s.resolve("alice", ChainId::Btc, "0xabc").is_none());
        // Wrong on-chain address
        assert!(s.resolve("alice", ChainId::Eth, "0xdef").is_none());
    }

    #[test]
    fn set_status_appends_and_recovery_replays_latest() {
        let wal: Arc<dyn WalStore<WithdrawalAddress>> = Arc::new(InMemoryWal::new());
        {
            let s = AddressBookStore::new(wal.clone()).unwrap();
            s.create(make_addr(
                "a-1",
                "alice",
                ChainId::Eth,
                "0xabc",
                AddressStatus::Active,
                -10,
            ))
            .unwrap();
            s.set_status("a-1", AddressStatus::Suspended).unwrap();
        }
        // Re-open: latest status survives.
        let s2 = AddressBookStore::new(wal).unwrap();
        assert_eq!(s2.get("a-1").unwrap().status, AddressStatus::Suspended);
    }

    #[test]
    fn sweep_cooldowns_promotes_only_past_window() {
        let s = AddressBookStore::new(Arc::new(InMemoryWal::new())).unwrap();
        // a-past: cool-down already elapsed.
        s.create(make_addr(
            "a-past",
            "alice",
            ChainId::Eth,
            "0x1",
            AddressStatus::PendingCooldown,
            -60,
        ))
        .unwrap();
        // a-future: cool-down still in the future.
        s.create(make_addr(
            "a-future",
            "alice",
            ChainId::Eth,
            "0x2",
            AddressStatus::PendingCooldown,
            3600,
        ))
        .unwrap();
        // a-active: already past the gate, not eligible to be re-promoted.
        s.create(make_addr(
            "a-active",
            "alice",
            ChainId::Eth,
            "0x3",
            AddressStatus::Active,
            -100,
        ))
        .unwrap();
        let n = s.sweep_cooldowns().unwrap();
        assert_eq!(n, 1);
        assert_eq!(s.get("a-past").unwrap().status, AddressStatus::Active);
        assert_eq!(s.get("a-future").unwrap().status, AddressStatus::PendingCooldown);
        assert_eq!(s.get("a-active").unwrap().status, AddressStatus::Active);
        // Idempotent: a second sweep finds nothing to do.
        assert_eq!(s.sweep_cooldowns().unwrap(), 0);
    }
}
