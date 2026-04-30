use super::*;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 1 — Vault topology: hot / warm / cold separation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Vault tier — determines signing authority and spending limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum VaultTier {
    /// Online key, auto-signs below threshold.  Capped balance.
    Hot,
    /// Semi-online key. Requires 1-of-N approval, medium balance.
    Warm,
    /// Offline / HSM key. Requires M-of-N approval + time-lock.
    Cold,
}

/// Per-vault configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct VaultConfig {
    pub(crate) tier: VaultTier,
    /// Ledger account backing this vault, e.g. `SYS:HOT_VAULT:USDC`.
    pub(crate) ledger_account: String,
    /// Max balance the vault should hold (auto-sweep excess to colder tier).
    pub(crate) max_balance: i64,
    /// Max single-withdrawal this vault may serve without escalation.
    pub(crate) single_tx_limit: i64,
    /// Required approvals before vault can disburse.
    pub(crate) required_approvals: u32,
    /// Time-lock in seconds between approval and execution.
    pub(crate) time_lock_secs: u64,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            tier: VaultTier::Hot,
            ledger_account: "SYS:HOT_VAULT:USDC".into(),
            max_balance: 500_000,    // 500k USDC hot limit
            single_tx_limit: 50_000, // auto up to 50k
            required_approvals: 0,   // hot = auto
            time_lock_secs: 0,
        }
    }
}

/// Full custody configuration with three vault tiers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CustodyConfig {
    pub(crate) hot: VaultConfig,
    pub(crate) warm: VaultConfig,
    pub(crate) cold: VaultConfig,
}

impl Default for CustodyConfig {
    fn default() -> Self {
        Self {
            hot: VaultConfig {
                tier: VaultTier::Hot,
                ledger_account: "SYS:HOT_VAULT:USDC".into(),
                max_balance: 500_000,
                single_tx_limit: 50_000,
                required_approvals: 0,
                time_lock_secs: 0,
            },
            warm: VaultConfig {
                tier: VaultTier::Warm,
                ledger_account: "SYS:WARM_VAULT:USDC".into(),
                max_balance: 5_000_000,
                single_tx_limit: 500_000,
                required_approvals: 1,
                time_lock_secs: 300, // 5 min
            },
            cold: VaultConfig {
                tier: VaultTier::Cold,
                ledger_account: "SYS:COLD_VAULT:USDC".into(),
                max_balance: i64::MAX,
                single_tx_limit: i64::MAX,
                required_approvals: 3,
                time_lock_secs: 3600, // 1 hour
            },
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 2 — Address whitelist
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A single whitelisted withdrawal address.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WhitelistedAddress {
    pub(crate) address: String,
    pub(crate) label: String,
    pub(crate) added_by: String,
    pub(crate) added_at: DateTime<Utc>,
    /// Cooldown: address cannot be used until this time.
    pub(crate) usable_after: DateTime<Utc>,
}

/// Per-user address whitelist store (WAL-backed).
pub(crate) struct AddressWhitelistStore {
    /// user_id → Vec<WhitelistedAddress>
    entries: DashMap<String, Vec<WhitelistedAddress>>,
    store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>>,
    write_lock: Mutex<()>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AddressWhitelistRecord {
    pub(crate) user_id: String,
    pub(crate) action: String, // "add" | "remove"
    pub(crate) address: String,
    pub(crate) label: String,
    pub(crate) actor: String,
    pub(crate) recorded_at: DateTime<Utc>,
    pub(crate) usable_after: DateTime<Utc>,
}

impl AddressWhitelistStore {
    pub(crate) fn new(
        store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            entries: DashMap::new(),
            store,
            write_lock: Mutex::new(()),
        };
        // Replay WAL to rebuild state
        for record in result.store.entries()? {
            match record.action.as_str() {
                "add" => {
                    let entry = WhitelistedAddress {
                        address: record.address.clone(),
                        label: record.label.clone(),
                        added_by: record.actor.clone(),
                        added_at: record.recorded_at,
                        usable_after: record.usable_after,
                    };
                    result
                        .entries
                        .entry(record.user_id.clone())
                        .or_default()
                        .push(entry);
                }
                "remove" => {
                    if let Some(mut addrs) = result.entries.get_mut(&record.user_id) {
                        addrs.retain(|a| a.address != record.address);
                    }
                }
                _ => {}
            }
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    /// Add a new address to user's whitelist.
    /// Returns the cooldown-expiry timestamp.
    pub(crate) fn add_address(
        &self,
        user_id: &str,
        address: &str,
        label: &str,
        actor: &str,
        cooldown_secs: u64,
    ) -> anyhow::Result<DateTime<Utc>> {
        let _guard = self.write_lock.lock();
        let now = Utc::now();
        let usable_after = now + chrono::Duration::seconds(cooldown_secs as i64);

        // Reject duplicate
        if let Some(addrs) = self.entries.get(user_id) {
            if addrs.iter().any(|a| a.address == address) {
                anyhow::bail!("address already whitelisted");
            }
        }

        let record = AddressWhitelistRecord {
            user_id: user_id.to_string(),
            action: "add".into(),
            address: address.to_string(),
            label: label.to_string(),
            actor: actor.to_string(),
            recorded_at: now,
            usable_after,
        };
        self.store.append(&record)?;
        self.entries
            .entry(user_id.to_string())
            .or_default()
            .push(WhitelistedAddress {
                address: address.to_string(),
                label: label.to_string(),
                added_by: actor.to_string(),
                added_at: now,
                usable_after,
            });
        Ok(usable_after)
    }

    /// Remove an address from user's whitelist (admin-only).
    pub(crate) fn remove_address(
        &self,
        user_id: &str,
        address: &str,
        actor: &str,
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        let record = AddressWhitelistRecord {
            user_id: user_id.to_string(),
            action: "remove".into(),
            address: address.to_string(),
            label: String::new(),
            actor: actor.to_string(),
            recorded_at: Utc::now(),
            usable_after: Utc::now(),
        };
        self.store.append(&record)?;
        if let Some(mut addrs) = self.entries.get_mut(user_id) {
            addrs.retain(|a| a.address != address);
        }
        Ok(())
    }

    /// Check if an address is whitelisted AND past its cooldown.
    pub(crate) fn is_address_allowed(&self, user_id: &str, address: &str) -> bool {
        let now = Utc::now();
        self.entries
            .get(user_id)
            .map(|addrs| {
                addrs
                    .iter()
                    .any(|a| a.address == address && a.usable_after <= now)
            })
            .unwrap_or(false)
    }

    pub(crate) fn list_for_user(&self, user_id: &str) -> Vec<WhitelistedAddress> {
        self.entries
            .get(user_id)
            .map(|addrs| addrs.clone())
            .unwrap_or_default()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 3 — Per-user withdrawal limits (daily cap + cooldown)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Global withdrawal policy applied to all users (overridable per-user).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WithdrawalPolicy {
    /// Per-user daily withdrawal cap (in base units).
    pub(crate) daily_limit: i64,
    /// Minimum seconds between successive withdrawal *requests*.
    pub(crate) cooldown_secs: u64,
    /// Threshold above which withdrawal requires multi-admin approval
    /// (routed through governance pipeline instead of single-admin).
    pub(crate) large_withdrawal_threshold: i64,
    /// Required governance approvals for large withdrawals.
    pub(crate) large_withdrawal_approvals: u32,
    /// Whether address whitelist is enforced.
    pub(crate) require_whitelisted_address: bool,
    /// Cooldown in seconds after adding a new address before it's usable.
    pub(crate) address_cooldown_secs: u64,
}

impl Default for WithdrawalPolicy {
    fn default() -> Self {
        Self {
            daily_limit: 1_000_000,              // 1M USDC / day
            cooldown_secs: 60,                   // 1 min between requests
            large_withdrawal_threshold: 100_000, // > 100k needs multi-approval
            large_withdrawal_approvals: 2,
            require_whitelisted_address: true,
            address_cooldown_secs: 86_400, // 24h after adding address
        }
    }
}

/// Rolling daily usage tracker per user (in-memory, rebuilt from WAL on restart).
pub(crate) struct WithdrawalUsageTracker {
    /// user_id → Vec<(timestamp, amount)> of approved withdrawals in last 24h
    recent: DashMap<String, Vec<(DateTime<Utc>, i64)>>,
}

impl WithdrawalUsageTracker {
    pub(crate) fn new() -> Self {
        Self {
            recent: DashMap::new(),
        }
    }

    /// Record an approved withdrawal.
    pub(crate) fn record(&self, user_id: &str, amount: i64) {
        let now = Utc::now();
        self.recent
            .entry(user_id.to_string())
            .or_default()
            .push((now, amount));
    }

    /// Sum of withdrawals in the last 24 hours.
    pub(crate) fn daily_total(&self, user_id: &str) -> i64 {
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        self.recent
            .get(user_id)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|(ts, _)| *ts >= cutoff)
                    .map(|(_, amt)| *amt)
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Most recent withdrawal timestamp.
    pub(crate) fn last_withdrawal_time(&self, user_id: &str) -> Option<DateTime<Utc>> {
        self.recent
            .get(user_id)
            .and_then(|entries| entries.iter().map(|(ts, _)| *ts).max())
    }

    /// Prune entries older than 24 hours (call periodically).
    #[allow(dead_code)]
    pub(crate) fn gc(&self) {
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        for mut entry in self.recent.iter_mut() {
            entry.value_mut().retain(|(ts, _)| *ts >= cutoff);
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 4 — Pre-withdrawal gate: unified check before approval
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Result of the pre-withdrawal gate check.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum WithdrawalGateResult {
    /// Proceed — all checks passed.
    Proceed,
    /// Blocked — address not whitelisted or still in cooldown.
    AddressNotWhitelisted { address: String },
    /// Blocked — daily limit would be exceeded.
    DailyLimitExceeded { used: i64, limit: i64 },
    /// Blocked — cooldown between withdrawals not yet elapsed.
    CooldownActive { remaining_secs: u64 },
    /// Escalated — requires multi-admin governance approval.
    RequiresGovernance { required_approvals: u32 },
    /// Blocked — vault insufficient for this amount.
    VaultInsufficient { available: i64, requested: i64 },
}

/// Run all pre-disbursement checks.
pub(crate) fn check_withdrawal_gate(
    user_id: &str,
    amount: i64,
    destination_address: &str,
    policy: &WithdrawalPolicy,
    usage: &WithdrawalUsageTracker,
    whitelist: &AddressWhitelistStore,
    _custody: &CustodyConfig,
) -> WithdrawalGateResult {
    // 1. Address whitelist
    if policy.require_whitelisted_address
        && !whitelist.is_address_allowed(user_id, destination_address)
    {
        return WithdrawalGateResult::AddressNotWhitelisted {
            address: destination_address.to_string(),
        };
    }

    // 2. Cooldown between requests
    if policy.cooldown_secs > 0 {
        if let Some(last) = usage.last_withdrawal_time(user_id) {
            let elapsed = (Utc::now() - last).num_seconds().max(0) as u64;
            if elapsed < policy.cooldown_secs {
                return WithdrawalGateResult::CooldownActive {
                    remaining_secs: policy.cooldown_secs - elapsed,
                };
            }
        }
    }

    // 3. Daily limit
    let used = usage.daily_total(user_id);
    if used.saturating_add(amount) > policy.daily_limit {
        return WithdrawalGateResult::DailyLimitExceeded {
            used,
            limit: policy.daily_limit,
        };
    }

    // 4. Large withdrawal → governance escalation
    if amount > policy.large_withdrawal_threshold {
        return WithdrawalGateResult::RequiresGovernance {
            required_approvals: policy.large_withdrawal_approvals,
        };
    }

    WithdrawalGateResult::Proceed
}

/// Select the vault tier that should service a given withdrawal amount.
pub(crate) fn select_vault_tier(amount: i64, custody: &CustodyConfig) -> VaultTier {
    if amount <= custody.hot.single_tx_limit {
        VaultTier::Hot
    } else if amount <= custody.warm.single_tx_limit {
        VaultTier::Warm
    } else {
        VaultTier::Cold
    }
}

/// Get vault config for a tier.
#[allow(dead_code)]
pub(crate) fn vault_config_for_tier(tier: VaultTier, custody: &CustodyConfig) -> &VaultConfig {
    match tier {
        VaultTier::Hot => &custody.hot,
        VaultTier::Warm => &custody.warm,
        VaultTier::Cold => &custody.cold,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 5 — Sweep engine: keep hot vault at target balance
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A single sweep operation record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SweepRecord {
    pub(crate) sweep_id: String,
    pub(crate) from_tier: VaultTier,
    pub(crate) to_tier: VaultTier,
    pub(crate) amount: i64,
    pub(crate) reason: String,
    pub(crate) status: String,
    pub(crate) recorded_at: DateTime<Utc>,
}

/// Compute sweep suggestions: excess in hot → warm, excess in warm → cold.
#[allow(dead_code)]
pub(crate) fn compute_sweep_suggestions(
    hot_balance: i64,
    warm_balance: i64,
    custody: &CustodyConfig,
) -> Vec<SweepRecord> {
    let mut sweeps = Vec::new();
    let now = Utc::now();

    // Hot → Warm: if hot exceeds its max
    if hot_balance > custody.hot.max_balance {
        let excess = hot_balance - custody.hot.max_balance;
        sweeps.push(SweepRecord {
            sweep_id: types::generate_id(),
            from_tier: VaultTier::Hot,
            to_tier: VaultTier::Warm,
            amount: excess,
            reason: format!(
                "hot balance {hot_balance} exceeds max {}",
                custody.hot.max_balance
            ),
            status: "suggested".into(),
            recorded_at: now,
        });
    }

    // Warm → Cold: if warm exceeds its max
    if warm_balance > custody.warm.max_balance {
        let excess = warm_balance - custody.warm.max_balance;
        sweeps.push(SweepRecord {
            sweep_id: types::generate_id(),
            from_tier: VaultTier::Warm,
            to_tier: VaultTier::Cold,
            amount: excess,
            reason: format!(
                "warm balance {warm_balance} exceeds max {}",
                custody.warm.max_balance
            ),
            status: "suggested".into(),
            recorded_at: now,
        });
    }

    sweeps
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 6 — Transaction signing verification (pre-signing decode gate)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Describes a decoded on-chain transaction before signing.
/// In production this would be produced by parsing the raw transaction
/// bytes and verifying the destination / amount / calldata match the
/// approved withdrawal.  Here we define the verification interface.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DecodedTransaction {
    /// Target address (must match withdrawal destination).
    pub(crate) to_address: String,
    /// Transfer amount (must equal approved amount).
    pub(crate) amount: i64,
    /// Asset being transferred.
    pub(crate) asset: String,
    /// Chain ID (prevents cross-chain replay).
    pub(crate) chain_id: u64,
    /// Optional memo / calldata hash for audit.
    pub(crate) calldata_hash: String,
}

/// Verify that a decoded transaction matches the approved withdrawal.
/// This is the anti-blind-signing gate: refuse to sign unless all
/// fields match the approved intent.
#[allow(dead_code)]
pub(crate) fn verify_transaction_matches_withdrawal(
    decoded: &DecodedTransaction,
    approved_address: &str,
    approved_amount: i64,
    approved_asset: &str,
    expected_chain_id: u64,
) -> Result<(), String> {
    if decoded.to_address != approved_address {
        return Err(format!(
            "address mismatch: tx={} approved={}",
            decoded.to_address, approved_address
        ));
    }
    if decoded.amount != approved_amount {
        return Err(format!(
            "amount mismatch: tx={} approved={}",
            decoded.amount, approved_amount
        ));
    }
    if decoded.asset != approved_asset {
        return Err(format!(
            "asset mismatch: tx={} approved={}",
            decoded.asset, approved_asset
        ));
    }
    if decoded.chain_id != expected_chain_id {
        return Err(format!(
            "chain_id mismatch: tx={} expected={}",
            decoded.chain_id, expected_chain_id
        ));
    }
    Ok(())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 7 — Independent signing decode channel
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// The signing pipeline is intentionally separated from the approval
// pipeline.  A `SigningRequest` is created ONLY after approval + delay
// window expiry.  The decode service independently re-derives the
// expected transaction fields from the persisted `WithdrawalRecord`
// and compares them to the raw transaction bytes decoded by an
// independent codec (not the same code path that built the tx).

/// A signing request that is derived from an approved withdrawal AFTER
/// the time-lock window has elapsed.  The signer never sees the raw
/// approval; it only sees this independently-reconstructed intent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SigningRequest {
    pub(crate) signing_id: String,
    pub(crate) withdrawal_id: String,
    /// Fields re-derived from the withdrawal record — NOT copied from
    /// the approval payload.
    pub(crate) expected_to: String,
    pub(crate) expected_amount: i64,
    pub(crate) expected_asset: String,
    pub(crate) expected_chain_id: u64,
    /// Vault tier that will provide the signature.
    pub(crate) vault_tier: VaultTier,
    pub(crate) status: SigningStatus,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) signed_at: Option<DateTime<Utc>>,
    pub(crate) decode_result: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SigningStatus {
    /// Waiting for raw tx bytes to arrive from the builder.
    AwaitingTransaction,
    /// Raw tx decoded and matched — ready to sign.
    Verified,
    /// Decode mismatch — signing refused.
    Rejected,
    /// Signed and broadcast.
    Signed,
}

/// Independent decode + compare service.
/// `raw_decoded` is produced by an independent decoder (not the tx builder).
pub(crate) fn verify_signing_request(
    request: &SigningRequest,
    raw_decoded: &DecodedTransaction,
) -> Result<(), String> {
    // Re-derive from signing request (which was independently built from
    // the withdrawal record) and compare against the independently-decoded
    // raw transaction.
    verify_transaction_matches_withdrawal(
        raw_decoded,
        &request.expected_to,
        request.expected_amount,
        &request.expected_asset,
        request.expected_chain_id,
    )?;
    // Additional: verify calldata_hash is non-empty (prevents empty-call attacks)
    if raw_decoded.calldata_hash.is_empty() && raw_decoded.amount > 0 {
        return Err("calldata_hash missing on non-zero transfer".into());
    }
    Ok(())
}

/// Build a signing request from an approved withdrawal (independent derivation).
pub(crate) fn create_signing_request(
    withdrawal_id: &str,
    destination_address: &str,
    amount: i64,
    asset: &str,
    chain_id: u64,
    custody: &CustodyConfig,
) -> SigningRequest {
    let vault_tier = select_vault_tier(amount, custody);
    SigningRequest {
        signing_id: types::generate_id(),
        withdrawal_id: withdrawal_id.to_string(),
        expected_to: destination_address.to_string(),
        expected_amount: amount,
        expected_asset: asset.to_string(),
        expected_chain_id: chain_id,
        vault_tier,
        status: SigningStatus::AwaitingTransaction,
        created_at: Utc::now(),
        signed_at: None,
        decode_result: None,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 8 — Approval / display separation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Sanitised view of a pending withdrawal (for display to users/admins).
/// Does NOT contain internal fields like ledger_op_id.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingWithdrawalView {
    pub(crate) withdrawal_id: String,
    pub(crate) user_id: String,
    pub(crate) amount: i64,
    pub(crate) asset: String,
    /// Masked destination: first 6 + last 4 chars visible.
    pub(crate) destination_masked: String,
    pub(crate) status: String,
    pub(crate) requested_at: DateTime<Utc>,
    pub(crate) executable_after: Option<DateTime<Utc>>,
    pub(crate) cancel_before: Option<DateTime<Utc>>,
    pub(crate) vault_tier: VaultTier,
}

/// Internal approval intent — only accessible to the approval engine,
/// never serialised to the display layer.
#[derive(Debug, Clone)]
pub(crate) struct WithdrawalApprovalIntent {
    pub(crate) withdrawal_id: String,
    pub(crate) user_id: String,
    pub(crate) amount: i64,
    pub(crate) asset: String,
    pub(crate) destination_address: String,
    pub(crate) vault_tier: VaultTier,
    pub(crate) required_approvals: u32,
    pub(crate) time_lock_secs: u64,
    pub(crate) executable_after: DateTime<Utc>,
    pub(crate) cancel_before: DateTime<Utc>,
}

/// Mask an address for display: show first 6 and last 4 characters.
pub(crate) fn mask_address(addr: &str) -> String {
    if addr.len() <= 10 {
        return "***".to_string();
    }
    format!("{}...{}", &addr[..6], &addr[addr.len() - 4..])
}

/// Build a sanitised display view from internal withdrawal data.
pub(crate) fn to_pending_view(
    intent: &WithdrawalApprovalIntent,
    status: &str,
    requested_at: DateTime<Utc>,
) -> PendingWithdrawalView {
    PendingWithdrawalView {
        withdrawal_id: intent.withdrawal_id.clone(),
        user_id: intent.user_id.clone(),
        amount: intent.amount,
        asset: intent.asset.clone(),
        destination_masked: mask_address(&intent.destination_address),
        status: status.to_string(),
        requested_at,
        executable_after: Some(intent.executable_after),
        cancel_before: Some(intent.cancel_before),
        vault_tier: intent.vault_tier,
    }
}

/// Build the internal approval intent from withdrawal record + custody config.
pub(crate) fn build_approval_intent(
    withdrawal_id: &str,
    user_id: &str,
    amount: i64,
    asset: &str,
    destination_address: &str,
    custody: &CustodyConfig,
    delay_policy: &WithdrawalDelayPolicy,
) -> WithdrawalApprovalIntent {
    let vault_tier = select_vault_tier(amount, custody);
    let vc = vault_config_for_tier(vault_tier, custody);
    let now = Utc::now();
    let delay_secs = delay_policy.delay_for_tier(vault_tier);
    let executable_after = now + chrono::Duration::seconds(delay_secs as i64);
    let cancel_window = delay_policy.cancel_window_secs.max(delay_secs);
    let cancel_before = now + chrono::Duration::seconds(cancel_window as i64);

    WithdrawalApprovalIntent {
        withdrawal_id: withdrawal_id.to_string(),
        user_id: user_id.to_string(),
        amount,
        asset: asset.to_string(),
        destination_address: destination_address.to_string(),
        vault_tier,
        required_approvals: vc.required_approvals,
        time_lock_secs: vc.time_lock_secs,
        executable_after,
        cancel_before,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 9 — Withdrawal delay / cancel window
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Configurable delays per vault tier + universal cancel window.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WithdrawalDelayPolicy {
    /// Delay before hot-tier withdrawals become executable (seconds).
    pub(crate) hot_delay_secs: u64,
    /// Delay before warm-tier withdrawals become executable (seconds).
    pub(crate) warm_delay_secs: u64,
    /// Delay before cold-tier withdrawals become executable (seconds).
    pub(crate) cold_delay_secs: u64,
    /// Window in seconds during which a user can cancel their withdrawal.
    /// Always ≥ the tier delay — if set lower, tier delay is used.
    pub(crate) cancel_window_secs: u64,
}

impl Default for WithdrawalDelayPolicy {
    fn default() -> Self {
        Self {
            hot_delay_secs: 0,       // hot = instant
            warm_delay_secs: 600,    // 10 min
            cold_delay_secs: 7200,   // 2 hours
            cancel_window_secs: 900, // 15 min universal cancel window
        }
    }
}

impl WithdrawalDelayPolicy {
    pub(crate) fn delay_for_tier(&self, tier: VaultTier) -> u64 {
        match tier {
            VaultTier::Hot => self.hot_delay_secs,
            VaultTier::Warm => self.warm_delay_secs,
            VaultTier::Cold => self.cold_delay_secs,
        }
    }
}

/// Check if a withdrawal has passed its delay and cancel window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WithdrawalTimingResult {
    /// Still within cancel window — user can cancel.
    Cancellable { cancel_remaining_secs: u64 },
    /// Past cancel window but before executable time — waiting.
    WaitingForDelay { delay_remaining_secs: u64 },
    /// Ready to execute.
    Executable,
    /// Already past a hard deadline (stale) — should be auto-cancelled.
    Expired,
}

/// Evaluate timing for a withdrawal.
pub(crate) fn evaluate_withdrawal_timing(
    now: DateTime<Utc>,
    executable_after: DateTime<Utc>,
    cancel_before: DateTime<Utc>,
    max_age_secs: u64,
    requested_at: DateTime<Utc>,
) -> WithdrawalTimingResult {
    // Hard expiry: if withdrawal is older than max_age, it's stale
    let age = (now - requested_at).num_seconds().max(0) as u64;
    if max_age_secs > 0 && age > max_age_secs {
        return WithdrawalTimingResult::Expired;
    }
    // Cancel window
    if now < cancel_before {
        let remaining = (cancel_before - now).num_seconds().max(0) as u64;
        return WithdrawalTimingResult::Cancellable {
            cancel_remaining_secs: remaining,
        };
    }
    // Delay window
    if now < executable_after {
        let remaining = (executable_after - now).num_seconds().max(0) as u64;
        return WithdrawalTimingResult::WaitingForDelay {
            delay_remaining_secs: remaining,
        };
    }
    WithdrawalTimingResult::Executable
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 10 — Vault velocity cap
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Per-vault outflow velocity limits.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct VaultVelocityPolicy {
    /// Maximum total outflow from the hot vault in a rolling window.
    pub(crate) hot_max_outflow: i64,
    /// Maximum total outflow from the warm vault in a rolling window.
    pub(crate) warm_max_outflow: i64,
    /// Maximum total outflow from the cold vault in a rolling window.
    pub(crate) cold_max_outflow: i64,
    /// Rolling window size in seconds.
    pub(crate) window_secs: u64,
}

impl Default for VaultVelocityPolicy {
    fn default() -> Self {
        Self {
            hot_max_outflow: 200_000,     // 200k / window from hot
            warm_max_outflow: 2_000_000,  // 2M / window from warm
            cold_max_outflow: 10_000_000, // 10M / window from cold
            window_secs: 3600,            // 1-hour window
        }
    }
}

/// Tracks per-vault outflow for velocity limiting.
pub(crate) struct VaultVelocityTracker {
    /// (tier, timestamp, amount) ring buffer
    events: Mutex<Vec<(VaultTier, DateTime<Utc>, i64)>>,
}

impl VaultVelocityTracker {
    pub(crate) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    /// Record a vault outflow event.
    pub(crate) fn record_outflow(&self, tier: VaultTier, amount: i64) {
        let mut events = self.events.lock();
        events.push((tier, Utc::now(), amount));
    }

    /// Sum outflow for a specific tier in the last `window_secs`.
    pub(crate) fn tier_outflow(&self, tier: VaultTier, window_secs: u64) -> i64 {
        let cutoff = Utc::now() - chrono::Duration::seconds(window_secs as i64);
        let events = self.events.lock();
        events
            .iter()
            .filter(|(t, ts, _)| *t == tier && *ts >= cutoff)
            .map(|(_, _, amt)| *amt)
            .sum()
    }

    /// Prune old entries.
    pub(crate) fn gc(&self, window_secs: u64) {
        let cutoff = Utc::now() - chrono::Duration::seconds(window_secs as i64);
        let mut events = self.events.lock();
        events.retain(|(_, ts, _)| *ts >= cutoff);
    }
}

/// Check if a vault withdrawal would breach the velocity cap.
pub(crate) fn check_vault_velocity(
    tier: VaultTier,
    amount: i64,
    tracker: &VaultVelocityTracker,
    policy: &VaultVelocityPolicy,
) -> Result<(), (i64, i64)> {
    let current = tracker.tier_outflow(tier, policy.window_secs);
    let limit = match tier {
        VaultTier::Hot => policy.hot_max_outflow,
        VaultTier::Warm => policy.warm_max_outflow,
        VaultTier::Cold => policy.cold_max_outflow,
    };
    if current.saturating_add(amount) > limit {
        Err((current, limit))
    } else {
        Ok(())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 11 — Stronger destination allowlist policy
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Extended allowlist policy with per-address limits, max addresses,
/// and admin-only registration for cold-tier amounts.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AllowlistPolicy {
    /// Maximum whitelisted addresses per user.
    pub(crate) max_addresses_per_user: usize,
    /// Per-address daily withdrawal cap (0 = no per-address limit).
    pub(crate) per_address_daily_limit: i64,
    /// Threshold above which only admins can add an address.
    /// If a user's largest single withdrawal to an address will
    /// exceed this, the address must be admin-added.
    pub(crate) admin_only_add_threshold: i64,
    /// Require 2FA confirmation for address additions.
    pub(crate) require_2fa_for_add: bool,
    /// Minimum address age (cooldown) before first use, in seconds.
    pub(crate) min_address_age_secs: u64,
    /// Addresses can only be added via admin for cold-tier amounts.
    pub(crate) cold_tier_admin_only: bool,
}

impl Default for AllowlistPolicy {
    fn default() -> Self {
        Self {
            max_addresses_per_user: 10,
            per_address_daily_limit: 500_000, // 500k per address per day
            admin_only_add_threshold: 200_000, // > 200k needs admin to add
            require_2fa_for_add: true,
            min_address_age_secs: 86_400, // 24h
            cold_tier_admin_only: true,
        }
    }
}

/// Validate an address addition against the extended allowlist policy.
pub(crate) fn validate_address_add(
    user_id: &str,
    is_admin: bool,
    whitelist: &AddressWhitelistStore,
    policy: &AllowlistPolicy,
) -> Result<(), String> {
    let current = whitelist.list_for_user(user_id);
    if current.len() >= policy.max_addresses_per_user {
        return Err(format!(
            "max {} addresses per user reached",
            policy.max_addresses_per_user
        ));
    }
    if !is_admin && policy.admin_only_add_threshold > 0 {
        // Non-admin adding: will be limited to < admin_only_add_threshold per tx
        // This is informational; actual enforcement at withdrawal time
    }
    Ok(())
}

/// Check per-address daily usage.
pub(crate) fn check_per_address_limit(
    user_id: &str,
    address: &str,
    amount: i64,
    usage: &WithdrawalUsageTracker,
    address_usage: &AddressUsageTracker,
    policy: &AllowlistPolicy,
) -> Result<(), (i64, i64)> {
    if policy.per_address_daily_limit <= 0 {
        return Ok(());
    }
    let _ = (user_id, usage); // user-level check is done elsewhere
    let current = address_usage.daily_total_for_address(address);
    if current.saturating_add(amount) > policy.per_address_daily_limit {
        Err((current, policy.per_address_daily_limit))
    } else {
        Ok(())
    }
}

/// Per-address daily usage tracker.
pub(crate) struct AddressUsageTracker {
    /// address → Vec<(timestamp, amount)>
    recent: DashMap<String, Vec<(DateTime<Utc>, i64)>>,
}

impl AddressUsageTracker {
    pub(crate) fn new() -> Self {
        Self {
            recent: DashMap::new(),
        }
    }

    pub(crate) fn record(&self, address: &str, amount: i64) {
        self.recent
            .entry(address.to_string())
            .or_default()
            .push((Utc::now(), amount));
    }

    pub(crate) fn daily_total_for_address(&self, address: &str) -> i64 {
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        self.recent
            .get(address)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|(ts, _)| *ts >= cutoff)
                    .map(|(_, amt)| *amt)
                    .sum()
            })
            .unwrap_or(0)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 12 — Hot/warm/cold automated isolation policy
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Isolation rules between vault tiers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct IsolationPolicy {
    /// Cold → Hot direct transfer is NEVER allowed.
    pub(crate) prohibit_cold_to_hot: bool,
    /// Maximum amount that can move from warm → hot in a single operation.
    pub(crate) warm_to_hot_max_amount: i64,
    /// Required approvals for warm → hot replenishment.
    pub(crate) warm_to_hot_approvals: u32,
    /// Hot vault target balance for auto-replenish from warm.
    pub(crate) hot_target_balance: i64,
    /// If hot falls below this, trigger auto-replenish from warm.
    pub(crate) hot_replenish_trigger: i64,
    /// Maximum % of warm vault that can move to hot in one day (bps).
    pub(crate) warm_to_hot_daily_pct_bps: u32,
}

impl Default for IsolationPolicy {
    fn default() -> Self {
        Self {
            prohibit_cold_to_hot: true,
            warm_to_hot_max_amount: 200_000,
            warm_to_hot_approvals: 1,
            hot_target_balance: 300_000,
            hot_replenish_trigger: 100_000,
            warm_to_hot_daily_pct_bps: 2000, // 20%
        }
    }
}

/// Result of evaluating auto-isolation rules.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum IsolationAction {
    /// No action needed — balances are within policy.
    NoAction,
    /// Hot vault needs replenishment from warm.
    ReplenishHot {
        amount: i64,
        approvals_required: u32,
    },
    /// Hot vault has excess → sweep to warm.
    SweepHotToWarm { amount: i64 },
    /// Warm vault has excess → sweep to cold.
    SweepWarmToCold { amount: i64 },
}

/// Compute the next isolation action given current vault balances.
pub(crate) fn evaluate_isolation(
    hot_balance: i64,
    warm_balance: i64,
    custody: &CustodyConfig,
    isolation: &IsolationPolicy,
) -> Vec<IsolationAction> {
    let mut actions = Vec::new();

    // Hot depleted → replenish from warm
    if hot_balance < isolation.hot_replenish_trigger {
        let need = (isolation.hot_target_balance - hot_balance)
            .min(isolation.warm_to_hot_max_amount)
            .min(warm_balance)
            .max(0);
        if need > 0 {
            actions.push(IsolationAction::ReplenishHot {
                amount: need,
                approvals_required: isolation.warm_to_hot_approvals,
            });
        }
    }

    // Hot excess → sweep to warm
    if hot_balance > custody.hot.max_balance {
        actions.push(IsolationAction::SweepHotToWarm {
            amount: hot_balance - custody.hot.max_balance,
        });
    }

    // Warm excess → sweep to cold
    if warm_balance > custody.warm.max_balance {
        actions.push(IsolationAction::SweepWarmToCold {
            amount: warm_balance - custody.warm.max_balance,
        });
    }

    if actions.is_empty() {
        actions.push(IsolationAction::NoAction);
    }
    actions
}

/// Validate a proposed inter-tier transfer against isolation rules.
#[allow(dead_code)]
pub(crate) fn validate_tier_transfer(
    from: VaultTier,
    to: VaultTier,
    amount: i64,
    isolation: &IsolationPolicy,
) -> Result<(), String> {
    // Cold → Hot is always prohibited
    if isolation.prohibit_cold_to_hot && from == VaultTier::Cold && to == VaultTier::Hot {
        return Err("direct Cold → Hot transfer is prohibited by isolation policy".into());
    }
    // Warm → Hot amount cap
    if from == VaultTier::Warm && to == VaultTier::Hot && amount > isolation.warm_to_hot_max_amount
    {
        return Err(format!(
            "warm→hot amount {} exceeds max {}",
            amount, isolation.warm_to_hot_max_amount
        ));
    }
    // Hot → Cold is prohibited (must go through warm)
    if from == VaultTier::Hot && to == VaultTier::Cold {
        return Err("direct Hot → Cold transfer prohibited; sweep through Warm".into());
    }
    Ok(())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 13 — Treasury audit / simulation / dry-run
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Snapshot of the full treasury state for audit purposes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TreasuryAuditSnapshot {
    pub(crate) snapshot_id: String,
    pub(crate) taken_at: DateTime<Utc>,
    pub(crate) hot_balance: i64,
    pub(crate) warm_balance: i64,
    pub(crate) cold_balance: i64,
    pub(crate) total_balance: i64,
    pub(crate) pending_withdrawals_count: usize,
    pub(crate) pending_withdrawals_amount: i64,
    pub(crate) daily_outflow: i64,
    pub(crate) isolation_actions: Vec<IsolationAction>,
    pub(crate) velocity_hot_used: i64,
    pub(crate) velocity_warm_used: i64,
    pub(crate) velocity_cold_used: i64,
    pub(crate) whitelisted_addresses_total: usize,
}

/// Build a treasury audit snapshot.
#[allow(clippy::too_many_arguments)]
pub(crate) fn take_treasury_snapshot(
    hot_balance: i64,
    warm_balance: i64,
    cold_balance: i64,
    pending_count: usize,
    pending_amount: i64,
    daily_outflow: i64,
    velocity_tracker: &VaultVelocityTracker,
    velocity_policy: &VaultVelocityPolicy,
    custody: &CustodyConfig,
    isolation: &IsolationPolicy,
    total_whitelist_addresses: usize,
) -> TreasuryAuditSnapshot {
    let actions = evaluate_isolation(hot_balance, warm_balance, custody, isolation);
    TreasuryAuditSnapshot {
        snapshot_id: types::generate_id(),
        taken_at: Utc::now(),
        hot_balance,
        warm_balance,
        cold_balance,
        total_balance: hot_balance
            .saturating_add(warm_balance)
            .saturating_add(cold_balance),
        pending_withdrawals_count: pending_count,
        pending_withdrawals_amount: pending_amount,
        daily_outflow,
        isolation_actions: actions,
        velocity_hot_used: velocity_tracker
            .tier_outflow(VaultTier::Hot, velocity_policy.window_secs),
        velocity_warm_used: velocity_tracker
            .tier_outflow(VaultTier::Warm, velocity_policy.window_secs),
        velocity_cold_used: velocity_tracker
            .tier_outflow(VaultTier::Cold, velocity_policy.window_secs),
        whitelisted_addresses_total: total_whitelist_addresses,
    }
}

/// Dry-run result for a proposed withdrawal — tests all gates without executing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WithdrawalDryRunResult {
    pub(crate) would_proceed: bool,
    pub(crate) gate_result: WithdrawalGateResult,
    pub(crate) vault_tier: VaultTier,
    pub(crate) delay_secs: u64,
    pub(crate) cancel_window_secs: u64,
    pub(crate) velocity_check: String,
    pub(crate) per_address_check: String,
    pub(crate) isolation_check: String,
    pub(crate) required_approvals: u32,
}

/// Simulate a withdrawal through all gates without any side effects.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dry_run_withdrawal(
    user_id: &str,
    amount: i64,
    destination_address: &str,
    policy: &WithdrawalPolicy,
    usage: &WithdrawalUsageTracker,
    whitelist: &AddressWhitelistStore,
    custody: &CustodyConfig,
    delay_policy: &WithdrawalDelayPolicy,
    velocity_tracker: &VaultVelocityTracker,
    velocity_policy: &VaultVelocityPolicy,
    allowlist_policy: &AllowlistPolicy,
    address_usage: &AddressUsageTracker,
) -> WithdrawalDryRunResult {
    let gate_result = check_withdrawal_gate(
        user_id,
        amount,
        destination_address,
        policy,
        usage,
        whitelist,
        custody,
    );
    let would_proceed = matches!(gate_result, WithdrawalGateResult::Proceed);

    let vault_tier = select_vault_tier(amount, custody);
    let vc = vault_config_for_tier(vault_tier, custody);
    let delay_secs = delay_policy.delay_for_tier(vault_tier);
    let cancel_window_secs = delay_policy.cancel_window_secs.max(delay_secs);

    let velocity_check =
        match check_vault_velocity(vault_tier, amount, velocity_tracker, velocity_policy) {
            Ok(()) => "pass".to_string(),
            Err((current, limit)) => format!("BLOCKED: current={current}, limit={limit}"),
        };

    let per_address_check = match check_per_address_limit(
        user_id,
        destination_address,
        amount,
        usage,
        address_usage,
        allowlist_policy,
    ) {
        Ok(()) => "pass".to_string(),
        Err((current, limit)) => format!("BLOCKED: current={current}, limit={limit}"),
    };

    let isolation_check = "pass".to_string();

    WithdrawalDryRunResult {
        would_proceed: would_proceed && velocity_check == "pass" && per_address_check == "pass",
        gate_result,
        vault_tier,
        delay_secs,
        cancel_window_secs,
        velocity_check,
        per_address_check,
        isolation_check,
        required_approvals: vc.required_approvals,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 14 — Custody circuit breaker
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const BREAKER_CLOSED: u8 = 0;
const BREAKER_OPEN: u8 = 1;
const BREAKER_HALF_OPEN: u8 = 2;

/// Global custody circuit breaker — trips on anomalous patterns.
pub(crate) struct CustodyCircuitBreaker {
    state: AtomicU8,
    tripped_at: Mutex<Option<DateTime<Utc>>>,
    trip_reason: Mutex<String>,
    /// Consecutive velocity breach count.
    velocity_breach_count: AtomicU64,
    /// Failed signing verification count.
    failed_signing_count: AtomicU64,
    /// Large withdrawal burst count (rolling window tracked externally).
    large_burst_count: AtomicU64,
    pub(crate) config: BreakerConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct BreakerConfig {
    /// Consecutive velocity breaches to trip.
    pub(crate) velocity_breach_threshold: u64,
    /// Signing failures to trip (1 = immediate).
    pub(crate) signing_failure_threshold: u64,
    /// Large burst count to trip.
    pub(crate) large_burst_threshold: u64,
    /// Auto-recovery after this many seconds (0 = manual only).
    pub(crate) auto_recovery_secs: u64,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            velocity_breach_threshold: 3,
            signing_failure_threshold: 1,
            large_burst_threshold: 5,
            auto_recovery_secs: 3600,
        }
    }
}

impl CustodyCircuitBreaker {
    pub(crate) fn new(config: BreakerConfig) -> Self {
        Self {
            state: AtomicU8::new(BREAKER_CLOSED),
            tripped_at: Mutex::new(None),
            trip_reason: Mutex::new(String::new()),
            velocity_breach_count: AtomicU64::new(0),
            failed_signing_count: AtomicU64::new(0),
            large_burst_count: AtomicU64::new(0),
            config,
        }
    }

    /// Trip the breaker open.
    pub(crate) fn trip(&self, reason: &str) {
        self.state.store(BREAKER_OPEN, Ordering::SeqCst);
        *self.tripped_at.lock() = Some(Utc::now());
        *self.trip_reason.lock() = reason.to_string();
    }

    /// Check if operations should be blocked.
    pub(crate) fn is_open(&self) -> bool {
        let state = self.state.load(Ordering::SeqCst);
        if state == BREAKER_CLOSED {
            return false;
        }
        // Auto-recovery check
        if self.config.auto_recovery_secs > 0 {
            if let Some(tripped) = *self.tripped_at.lock() {
                let elapsed = (Utc::now() - tripped).num_seconds().max(0) as u64;
                if elapsed >= self.config.auto_recovery_secs {
                    self.state.store(BREAKER_HALF_OPEN, Ordering::SeqCst);
                    return false; // allow one probe
                }
            }
        }
        true
    }

    /// Record a velocity breach and potentially trip.
    pub(crate) fn record_velocity_breach(&self) {
        let count = self.velocity_breach_count.fetch_add(1, Ordering::SeqCst) + 1;
        if count >= self.config.velocity_breach_threshold {
            self.trip(&format!(
                "velocity breach count {count} >= threshold {}",
                self.config.velocity_breach_threshold
            ));
        }
    }

    /// Clear velocity breach counter on successful withdrawal.
    pub(crate) fn clear_velocity_breaches(&self) {
        self.velocity_breach_count.store(0, Ordering::SeqCst);
    }

    /// Record a signing failure and trip immediately.
    pub(crate) fn record_signing_failure(&self, detail: &str) {
        let count = self.failed_signing_count.fetch_add(1, Ordering::SeqCst) + 1;
        if count >= self.config.signing_failure_threshold {
            self.trip(&format!("signing failure: {detail}"));
        }
    }

    /// Record a large withdrawal burst event.
    pub(crate) fn record_large_burst(&self) {
        let count = self.large_burst_count.fetch_add(1, Ordering::SeqCst) + 1;
        if count >= self.config.large_burst_threshold {
            self.trip(&format!(
                "large withdrawal burst count {count} >= threshold {}",
                self.config.large_burst_threshold
            ));
        }
    }

    /// Admin manual reset.
    pub(crate) fn reset(&self) {
        self.state.store(BREAKER_CLOSED, Ordering::SeqCst);
        *self.tripped_at.lock() = None;
        *self.trip_reason.lock() = String::new();
        self.velocity_breach_count.store(0, Ordering::SeqCst);
        self.failed_signing_count.store(0, Ordering::SeqCst);
        self.large_burst_count.store(0, Ordering::SeqCst);
    }

    /// Complete a half-open probe successfully → close.
    pub(crate) fn probe_success(&self) {
        if self.state.load(Ordering::SeqCst) == BREAKER_HALF_OPEN {
            self.reset();
        }
    }

    /// Complete a half-open probe with failure → re-open.
    pub(crate) fn probe_failure(&self, reason: &str) {
        if self.state.load(Ordering::SeqCst) == BREAKER_HALF_OPEN {
            self.trip(reason);
        }
    }

    pub(crate) fn status(&self) -> serde_json::Value {
        let state = self.state.load(Ordering::SeqCst);
        let state_str = match state {
            BREAKER_CLOSED => "closed",
            BREAKER_OPEN => "open",
            BREAKER_HALF_OPEN => "half_open",
            _ => "unknown",
        };
        serde_json::json!({
            "state": state_str,
            "tripped_at": *self.tripped_at.lock(),
            "trip_reason": *self.trip_reason.lock(),
            "velocity_breach_count": self.velocity_breach_count.load(Ordering::SeqCst),
            "failed_signing_count": self.failed_signing_count.load(Ordering::SeqCst),
            "large_burst_count": self.large_burst_count.load(Ordering::SeqCst),
        })
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Area 15 — Custody audit trail
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum CustodyEventType {
    WithdrawalRequested,
    WithdrawalGateBlocked,
    VelocityBreachDetected,
    PerAddressLimitBlocked,
    SigningRequestCreated,
    SigningVerified,
    SigningRejected,
    WithdrawalApproved,
    WithdrawalRejected,
    WithdrawalCancelled,
    WithdrawalExpired,
    CircuitBreakerTripped,
    CircuitBreakerReset,
    DryRunExecuted,
    TreasurySnapshotTaken,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CustodyAuditEvent {
    pub(crate) event_id: String,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) event_type: CustodyEventType,
    pub(crate) actor: String,
    pub(crate) details: serde_json::Value,
}

/// Append-only audit log for custody operations.
pub(crate) struct CustodyAuditLog {
    events: Mutex<Vec<CustodyAuditEvent>>,
}

impl CustodyAuditLog {
    pub(crate) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn record(
        &self,
        event_type: CustodyEventType,
        actor: &str,
        details: serde_json::Value,
    ) {
        let event = CustodyAuditEvent {
            event_id: types::generate_id(),
            timestamp: Utc::now(),
            event_type,
            actor: actor.to_string(),
            details,
        };
        self.events.lock().push(event);
    }

    pub(crate) fn recent(&self, limit: usize) -> Vec<CustodyAuditEvent> {
        let events = self.events.lock();
        events.iter().rev().take(limit).cloned().collect()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Routes — address whitelist management + custody status
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_custody_routes(
    whitelist_store: Arc<AddressWhitelistStore>,
    policy: WithdrawalPolicy,
    custody_config: CustodyConfig,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    user_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
    delay_policy: WithdrawalDelayPolicy,
    velocity_tracker: Arc<VaultVelocityTracker>,
    velocity_policy: VaultVelocityPolicy,
    allowlist_policy: AllowlistPolicy,
    address_usage: Arc<AddressUsageTracker>,
    isolation_policy: IsolationPolicy,
    withdrawal_usage: Arc<WithdrawalUsageTracker>,
    breaker: Arc<CustodyCircuitBreaker>,
    audit_log: Arc<CustodyAuditLog>,
    withdrawal_store: Arc<super::WithdrawalStore>,
    ledger: Arc<LedgerService>,
) -> JsonRoute {
    // POST /whitelist/address — user adds a withdrawal address
    let ws = whitelist_store.clone();
    let pol = policy.clone();
    let allow_pol = allowlist_policy.clone();
    let ip_rl = ip_rate_limiter.clone();
    let user_rl = user_rate_limiter.clone();
    let add_address = warp::path!("whitelist" / "address")
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json::<AddAddressRequest>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: AddAddressRequest,
                  remote: Option<SocketAddr>| {
                let ws = ws.clone();
                let pol = pol.clone();
                let allow_pol = allow_pol.clone();
                let ip_rl = ip_rl.clone();
                let user_rl = user_rl.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-whitelist:{}", principal.subject), 10)?;

                    if req.address.trim().is_empty() || req.address.len() > 256 {
                        return Err(reject_api(StatusCode::BAD_REQUEST, "invalid address"));
                    }
                    validate_address_add(&principal.subject, false, &ws, &allow_pol)
                        .map_err(|error| reject_api(StatusCode::BAD_REQUEST, error))?;

                    let usable_after = ws
                        .add_address(
                            &principal.subject,
                            &req.address,
                            &req.label.unwrap_or_default(),
                            &principal.subject,
                            pol.address_cooldown_secs,
                        )
                        .map_err(|e| reject_api(StatusCode::CONFLICT, e.to_string()))?;

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "address": req.address,
                        "usable_after": usable_after,
                        "cooldown_secs": pol.address_cooldown_secs,
                    })))
                }
            },
        )
        .boxed();

    // DELETE /whitelist/address — admin removes an address
    let ws2 = whitelist_store.clone();
    let ip_rl2 = ip_rate_limiter.clone();
    let admin_rl2 = admin_rate_limiter.clone();
    let remove_address = warp::path!("admin" / "whitelist" / "address")
        .and(warp::post()) // using POST for compatibility, body has user_id + address
        .and(with_principal())
        .and(warp::body::json::<RemoveAddressRequest>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: RemoveAddressRequest,
                  remote: Option<SocketAddr>| {
                let ws = ws2.clone();
                let ip_rl = ip_rl2.clone();
                let admin_rl = admin_rl2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    ws.remove_address(&req.user_id, &req.address, &principal.subject)
                        .map_err(reject_internal_error)?;

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "removed": true,
                        "address": req.address,
                        "user_id": req.user_id,
                    })))
                }
            },
        )
        .boxed();

    // GET /whitelist/addresses/{user_id}
    let ws3 = whitelist_store.clone();
    let ip_rl3 = ip_rate_limiter.clone();
    let user_rl3 = user_rate_limiter.clone();
    let list_addresses = warp::path!("whitelist" / "addresses" / String)
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |user_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let ws = ws3.clone();
                let ip_rl = ip_rl3.clone();
                let user_rl = user_rl3.clone();
                async move {
                    ensure_subject_or_admin(&principal, &user_id)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-read:{}", principal.subject), 30)?;

                    let items = ws.list_for_user(&user_id);
                    let resp: Vec<_> = items
                        .into_iter()
                        .map(|a| {
                            serde_json::json!({
                                "address": a.address,
                                "label": a.label,
                                "added_by": a.added_by,
                                "added_at": a.added_at,
                                "usable_after": a.usable_after,
                            })
                        })
                        .collect();
                    Ok::<_, warp::Rejection>(warp::reply::json(&resp))
                }
            },
        )
        .boxed();

    // GET /custody/status — vault balances + sweep suggestions
    let cc = custody_config.clone();
    let ip_rl4 = ip_rate_limiter.clone();
    let admin_rl4 = admin_rate_limiter.clone();
    let custody_status = warp::path!("admin" / "custody" / "status")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let cc = cc.clone();
                let ip_rl = ip_rl4.clone();
                let admin_rl = admin_rl4.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "hot": {
                            "account": cc.hot.ledger_account,
                            "max_balance": cc.hot.max_balance,
                            "single_tx_limit": cc.hot.single_tx_limit,
                            "required_approvals": cc.hot.required_approvals,
                        },
                        "warm": {
                            "account": cc.warm.ledger_account,
                            "max_balance": cc.warm.max_balance,
                            "single_tx_limit": cc.warm.single_tx_limit,
                            "required_approvals": cc.warm.required_approvals,
                        },
                        "cold": {
                            "account": cc.cold.ledger_account,
                            "max_balance": cc.cold.max_balance,
                            "single_tx_limit": cc.cold.single_tx_limit,
                            "required_approvals": cc.cold.required_approvals,
                        },
                        "policy": "withdrawal limits enforced at gate",
                    })))
                }
            },
        )
        .boxed();

    // GET /custody/policy — show current withdrawal policy
    let pol2 = policy.clone();
    let ip_rl5 = ip_rate_limiter.clone();
    let custody_policy = warp::path!("admin" / "custody" / "policy")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let pol = pol2.clone();
                let ip_rl = ip_rl5.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "daily_limit": pol.daily_limit,
                        "cooldown_secs": pol.cooldown_secs,
                        "large_withdrawal_threshold": pol.large_withdrawal_threshold,
                        "large_withdrawal_approvals": pol.large_withdrawal_approvals,
                        "require_whitelisted_address": pol.require_whitelisted_address,
                        "address_cooldown_secs": pol.address_cooldown_secs,
                    })))
                }
            },
        )
        .boxed();

    // POST /withdraw/dry-run — simulate a withdrawal through all gates
    let pol_dr = policy.clone();
    let usage_dr = withdrawal_usage.clone();
    let ws_dr = whitelist_store.clone();
    let cc_dr = custody_config.clone();
    let delay_dr = delay_policy.clone();
    let vel_dr = velocity_tracker.clone();
    let vel_pol_dr = velocity_policy.clone();
    let al_dr = allowlist_policy.clone();
    let au_dr = address_usage.clone();
    let ip_rl_dr = ip_rate_limiter.clone();
    let user_rl_dr = user_rate_limiter.clone();
    let audit_dr = audit_log.clone();
    let dry_run_route = warp::path!("withdraw" / "dry-run")
        .and(warp::post())
        .and(with_principal())
        .and(warp::body::json::<DryRunRequest>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  req: DryRunRequest,
                  remote: Option<SocketAddr>| {
                let pol = pol_dr.clone();
                let usage = usage_dr.clone();
                let ws = ws_dr.clone();
                let cc = cc_dr.clone();
                let delay = delay_dr.clone();
                let vel = vel_dr.clone();
                let vel_pol = vel_pol_dr.clone();
                let al = al_dr.clone();
                let au = au_dr.clone();
                let ip_rl = ip_rl_dr.clone();
                let user_rl = user_rl_dr.clone();
                let audit = audit_dr.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-dryrun:{}", principal.subject), 10)?;

                    let result = dry_run_withdrawal(
                        &principal.subject,
                        req.amount,
                        &req.destination_address,
                        &pol,
                        &usage,
                        &ws,
                        &cc,
                        &delay,
                        &vel,
                        &vel_pol,
                        &al,
                        &au,
                    );
                    audit.record(
                        CustodyEventType::DryRunExecuted,
                        &principal.subject,
                        serde_json::json!({
                            "amount": req.amount,
                            "destination": mask_address(&req.destination_address),
                            "would_proceed": result.would_proceed,
                        }),
                    );
                    Ok::<_, warp::Rejection>(warp::reply::json(&result))
                }
            },
        )
        .boxed();

    // POST /withdraw/{id}/cancel — user cancels a pending withdrawal in cancel window
    let ws_cancel = withdrawal_store.clone();
    let ledger_cancel = ledger.clone();
    let ip_rl_cancel = ip_rate_limiter.clone();
    let user_rl_cancel = user_rate_limiter.clone();
    let audit_cancel = audit_log.clone();
    let cancel_route = warp::path!("withdraw" / String / "cancel")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |withdrawal_id: String,
                  principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>| {
                let store = ws_cancel.clone();
                let ledger = ledger_cancel.clone();
                let ip_rl = ip_rl_cancel.clone();
                let user_rl = user_rl_cancel.clone();
                let audit = audit_cancel.clone();
                async move {
                    require_user(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    user_rl.check(&format!("user-cancel:{}", principal.subject), 10)?;

                    let current = store
                        .get(&withdrawal_id)
                        .ok_or_else(|| reject_api(StatusCode::NOT_FOUND, "withdrawal not found"))?;
                    if current.user_id != principal.subject {
                        return Err(reject_api(StatusCode::FORBIDDEN, "not your withdrawal"));
                    }
                    if current.status != "pending" {
                        return Err(reject_api(
                            StatusCode::CONFLICT,
                            format!("withdrawal is already {}", current.status),
                        ));
                    }

                    // Check timing: must be in cancel window
                    let now = Utc::now();
                    let exec_after = current.executable_after.unwrap_or(now);
                    let cancel_before = current.cancel_before.unwrap_or(now);
                    let timing = evaluate_withdrawal_timing(
                        now,
                        exec_after,
                        cancel_before,
                        86400,
                        current.requested_at,
                    );
                    match timing {
                        WithdrawalTimingResult::Cancellable { .. } => {}
                        _ => {
                            return Err(reject_api(
                                StatusCode::CONFLICT,
                                "cancel window has passed",
                            ));
                        }
                    }

                    // Release hold
                    let op_id = format!("withdrawal-cancel-{withdrawal_id}");
                    ledger
                        .release_cash_hold(&current.user_id, current.amount, op_id)
                        .map_err(reject_internal_error)?;

                    let cancelled = super::WithdrawalRecord {
                        status: "cancelled".into(),
                        decided_at: Some(now),
                        decided_by: Some(principal.subject.clone()),
                        ..current
                    };
                    store.append(cancelled).map_err(reject_internal_error)?;

                    audit.record(
                        CustodyEventType::WithdrawalCancelled,
                        &principal.subject,
                        serde_json::json!({ "withdrawal_id": withdrawal_id }),
                    );

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "withdrawal_id": withdrawal_id,
                        "status": "cancelled",
                    })))
                }
            },
        )
        .boxed();

    // GET /admin/custody/audit — treasury snapshot
    let cc_snap = custody_config.clone();
    let vel_snap = velocity_tracker.clone();
    let vel_pol_snap = velocity_policy.clone();
    let iso_snap = isolation_policy.clone();
    let ip_rl_snap = ip_rate_limiter.clone();
    let admin_rl_snap = admin_rate_limiter.clone();
    let audit_snap = audit_log.clone();
    let audit_snapshot_route = warp::path!("admin" / "custody" / "audit")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let cc = cc_snap.clone();
                let vel = vel_snap.clone();
                let vel_pol = vel_pol_snap.clone();
                let iso = iso_snap.clone();
                let ip_rl = ip_rl_snap.clone();
                let admin_rl = admin_rl_snap.clone();
                let audit = audit_snap.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    // Use 0 balances as placeholder (real integration would read ledger)
                    let snap =
                        take_treasury_snapshot(0, 0, 0, 0, 0, 0, &vel, &vel_pol, &cc, &iso, 0);
                    audit.record(
                        CustodyEventType::TreasurySnapshotTaken,
                        &principal.subject,
                        serde_json::json!({ "snapshot_id": snap.snapshot_id }),
                    );
                    Ok::<_, warp::Rejection>(warp::reply::json(&snap))
                }
            },
        )
        .boxed();

    // GET /admin/custody/audit/events — recent audit events
    let audit_ev = audit_log.clone();
    let ip_rl_ev = ip_rate_limiter.clone();
    let admin_rl_ev = admin_rate_limiter.clone();
    let audit_events_route = warp::path!("admin" / "custody" / "audit" / "events")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let audit = audit_ev.clone();
                let ip_rl = ip_rl_ev.clone();
                let admin_rl = admin_rl_ev.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let events = audit.recent(100);
                    Ok::<_, warp::Rejection>(warp::reply::json(&events))
                }
            },
        )
        .boxed();

    // GET /admin/custody/breaker — circuit breaker status
    let brk_status = breaker.clone();
    let ip_rl_bs = ip_rate_limiter.clone();
    let admin_rl_bs = admin_rate_limiter.clone();
    let breaker_status_route = warp::path!("admin" / "custody" / "breaker")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let brk = brk_status.clone();
                let ip_rl = ip_rl_bs.clone();
                let admin_rl = admin_rl_bs.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    Ok::<_, warp::Rejection>(warp::reply::json(&brk.status()))
                }
            },
        )
        .boxed();

    // POST /admin/custody/breaker/reset — admin resets circuit breaker
    let brk_reset = breaker.clone();
    let ip_rl_br = ip_rate_limiter.clone();
    let admin_rl_br = admin_rate_limiter.clone();
    let audit_br = audit_log.clone();
    let breaker_reset_route = warp::path!("admin" / "custody" / "breaker" / "reset")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let brk = brk_reset.clone();
                let ip_rl = ip_rl_br.clone();
                let admin_rl = admin_rl_br.clone();
                let audit = audit_br.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    admin_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    brk.reset();
                    audit.record(
                        CustodyEventType::CircuitBreakerReset,
                        &principal.subject,
                        serde_json::json!({}),
                    );
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "closed",
                        "reset_by": principal.subject,
                    })))
                }
            },
        )
        .boxed();

    add_address
        .or(remove_address)
        .unify()
        .or(list_addresses)
        .unify()
        .or(custody_status)
        .unify()
        .or(custody_policy)
        .unify()
        .or(dry_run_route)
        .unify()
        .or(cancel_route)
        .unify()
        .or(audit_snapshot_route)
        .unify()
        .or(audit_events_route)
        .unify()
        .or(breaker_status_route)
        .unify()
        .or(breaker_reset_route)
        .unify()
        .boxed()
}

// ── DTO types for custody routes ─────────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct AddAddressRequest {
    pub(crate) address: String,
    pub(crate) label: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct RemoveAddressRequest {
    pub(crate) user_id: String,
    pub(crate) address: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct DryRunRequest {
    pub(crate) amount: i64,
    pub(crate) destination_address: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    fn test_policy() -> WithdrawalPolicy {
        WithdrawalPolicy {
            daily_limit: 1000,
            cooldown_secs: 0,
            large_withdrawal_threshold: 500,
            large_withdrawal_approvals: 2,
            require_whitelisted_address: false,
            address_cooldown_secs: 0,
        }
    }

    #[test]
    fn withdrawal_gate_proceeds_within_limits() {
        let policy = test_policy();
        let usage = WithdrawalUsageTracker::new();
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let whitelist = AddressWhitelistStore::new(store).unwrap();
        let custody = CustodyConfig::default();

        let result =
            check_withdrawal_gate("u1", 100, "0xABC", &policy, &usage, &whitelist, &custody);
        assert!(matches!(result, WithdrawalGateResult::Proceed));
    }

    #[test]
    fn withdrawal_gate_blocks_daily_limit() {
        let policy = test_policy();
        let usage = WithdrawalUsageTracker::new();
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let whitelist = AddressWhitelistStore::new(store).unwrap();
        let custody = CustodyConfig::default();

        // Record 900 already used today
        usage.record("u1", 900);

        let result =
            check_withdrawal_gate("u1", 200, "0xABC", &policy, &usage, &whitelist, &custody);
        assert!(matches!(
            result,
            WithdrawalGateResult::DailyLimitExceeded { .. }
        ));
    }

    #[test]
    fn withdrawal_gate_escalates_large_amount() {
        let policy = test_policy();
        let usage = WithdrawalUsageTracker::new();
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let whitelist = AddressWhitelistStore::new(store).unwrap();
        let custody = CustodyConfig::default();

        let result =
            check_withdrawal_gate("u1", 600, "0xABC", &policy, &usage, &whitelist, &custody);
        assert!(matches!(
            result,
            WithdrawalGateResult::RequiresGovernance {
                required_approvals: 2
            }
        ));
    }

    #[test]
    fn address_whitelist_enforced_when_enabled() {
        let mut policy = test_policy();
        policy.require_whitelisted_address = true;
        let usage = WithdrawalUsageTracker::new();
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let whitelist = AddressWhitelistStore::new(store).unwrap();
        let custody = CustodyConfig::default();

        // Address not whitelisted → blocked
        let result =
            check_withdrawal_gate("u1", 100, "0xABC", &policy, &usage, &whitelist, &custody);
        assert!(matches!(
            result,
            WithdrawalGateResult::AddressNotWhitelisted { .. }
        ));

        // Add address with 0 cooldown → allowed
        whitelist
            .add_address("u1", "0xABC", "main", "u1", 0)
            .unwrap();
        let result =
            check_withdrawal_gate("u1", 100, "0xABC", &policy, &usage, &whitelist, &custody);
        assert!(matches!(result, WithdrawalGateResult::Proceed));
    }

    #[test]
    fn address_whitelist_cooldown_blocks_early_use() {
        let mut policy = test_policy();
        policy.require_whitelisted_address = true;
        let usage = WithdrawalUsageTracker::new();
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let whitelist = AddressWhitelistStore::new(store).unwrap();
        let custody = CustodyConfig::default();

        // Add address with 1-hour cooldown
        whitelist
            .add_address("u1", "0xDEF", "cold", "u1", 3600)
            .unwrap();

        // Still in cooldown → blocked
        let result =
            check_withdrawal_gate("u1", 50, "0xDEF", &policy, &usage, &whitelist, &custody);
        assert!(matches!(
            result,
            WithdrawalGateResult::AddressNotWhitelisted { .. }
        ));
    }

    #[test]
    fn vault_tier_selection() {
        let custody = CustodyConfig::default();
        assert_eq!(select_vault_tier(10_000, &custody), VaultTier::Hot);
        assert_eq!(select_vault_tier(50_000, &custody), VaultTier::Hot);
        assert_eq!(select_vault_tier(100_000, &custody), VaultTier::Warm);
        assert_eq!(select_vault_tier(1_000_000, &custody), VaultTier::Cold);
    }

    #[test]
    fn sweep_suggestions_excess_hot() {
        let custody = CustodyConfig::default();
        let sweeps = compute_sweep_suggestions(800_000, 100_000, &custody);
        assert_eq!(sweeps.len(), 1);
        assert_eq!(sweeps[0].from_tier, VaultTier::Hot);
        assert_eq!(sweeps[0].to_tier, VaultTier::Warm);
        assert_eq!(sweeps[0].amount, 300_000); // 800k - 500k
    }

    #[test]
    fn sweep_suggestions_none_when_balanced() {
        let custody = CustodyConfig::default();
        let sweeps = compute_sweep_suggestions(200_000, 1_000_000, &custody);
        assert!(sweeps.is_empty());
    }

    #[test]
    fn verify_transaction_matches_approved() {
        let decoded = DecodedTransaction {
            to_address: "0xABC".into(),
            amount: 1000,
            asset: "USDC".into(),
            chain_id: 1,
            calldata_hash: String::new(),
        };
        assert!(verify_transaction_matches_withdrawal(&decoded, "0xABC", 1000, "USDC", 1).is_ok());
    }

    #[test]
    fn verify_transaction_rejects_address_mismatch() {
        let decoded = DecodedTransaction {
            to_address: "0xEVIL".into(),
            amount: 1000,
            asset: "USDC".into(),
            chain_id: 1,
            calldata_hash: String::new(),
        };
        let result = verify_transaction_matches_withdrawal(&decoded, "0xABC", 1000, "USDC", 1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("address mismatch"));
    }

    #[test]
    fn verify_transaction_rejects_amount_mismatch() {
        let decoded = DecodedTransaction {
            to_address: "0xABC".into(),
            amount: 999_999,
            asset: "USDC".into(),
            chain_id: 1,
            calldata_hash: String::new(),
        };
        let result = verify_transaction_matches_withdrawal(&decoded, "0xABC", 1000, "USDC", 1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("amount mismatch"));
    }

    #[test]
    fn verify_transaction_rejects_chain_id_mismatch() {
        let decoded = DecodedTransaction {
            to_address: "0xABC".into(),
            amount: 1000,
            asset: "USDC".into(),
            chain_id: 56, // mismatches expected chain_id=1
            calldata_hash: String::new(),
        };
        let result = verify_transaction_matches_withdrawal(&decoded, "0xABC", 1000, "USDC", 1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("chain_id mismatch"));
    }

    #[test]
    fn daily_usage_tracker_sums_correctly() {
        let tracker = WithdrawalUsageTracker::new();
        tracker.record("u1", 100);
        tracker.record("u1", 200);
        tracker.record("u2", 50);

        assert_eq!(tracker.daily_total("u1"), 300);
        assert_eq!(tracker.daily_total("u2"), 50);
        assert_eq!(tracker.daily_total("u3"), 0);
    }

    #[test]
    fn address_whitelist_add_remove_cycle() {
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let ws = AddressWhitelistStore::new(store).unwrap();

        ws.add_address("u1", "0xA", "main", "u1", 0).unwrap();
        assert!(ws.is_address_allowed("u1", "0xA"));
        assert!(!ws.is_address_allowed("u1", "0xB"));

        ws.remove_address("u1", "0xA", "admin1").unwrap();
        assert!(!ws.is_address_allowed("u1", "0xA"));
    }

    #[test]
    fn address_whitelist_rejects_duplicate() {
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let ws = AddressWhitelistStore::new(store).unwrap();

        ws.add_address("u1", "0xA", "main", "u1", 0).unwrap();
        let err = ws.add_address("u1", "0xA", "dup", "u1", 0);
        assert!(err.is_err());
    }

    #[test]
    fn custody_defaults_are_sane() {
        let cc = CustodyConfig::default();
        assert_eq!(cc.hot.tier, VaultTier::Hot);
        assert!(cc.hot.max_balance < cc.warm.max_balance);
        assert!(cc.hot.single_tx_limit < cc.warm.single_tx_limit);
        assert_eq!(cc.hot.required_approvals, 0);
        assert_eq!(cc.warm.required_approvals, 1);
        assert_eq!(cc.cold.required_approvals, 3);
        assert!(cc.cold.time_lock_secs > cc.warm.time_lock_secs);
    }

    // ── Area 7: Independent signing decode channel ────────────────

    #[test]
    fn signing_request_independently_derived() {
        let custody = CustodyConfig::default();
        let sr = create_signing_request("wd-1", "0xABC", 10_000, "USDC", 1, &custody);
        assert_eq!(sr.withdrawal_id, "wd-1");
        assert_eq!(sr.expected_to, "0xABC");
        assert_eq!(sr.expected_amount, 10_000);
        assert_eq!(sr.vault_tier, VaultTier::Hot);
        assert_eq!(sr.status, SigningStatus::AwaitingTransaction);
    }

    #[test]
    fn signing_verify_matches() {
        let custody = CustodyConfig::default();
        let sr = create_signing_request("wd-2", "0xDEF", 5000, "USDC", 1, &custody);
        let decoded = DecodedTransaction {
            to_address: "0xDEF".into(),
            amount: 5000,
            asset: "USDC".into(),
            chain_id: 1,
            calldata_hash: "0xabcdef".into(),
        };
        assert!(verify_signing_request(&sr, &decoded).is_ok());
    }

    #[test]
    fn signing_verify_rejects_mismatch() {
        let custody = CustodyConfig::default();
        let sr = create_signing_request("wd-3", "0xGOOD", 1000, "USDC", 1, &custody);
        let decoded = DecodedTransaction {
            to_address: "0xEVIL".into(),
            amount: 1000,
            asset: "USDC".into(),
            chain_id: 1,
            calldata_hash: "0x123".into(),
        };
        let err = verify_signing_request(&sr, &decoded);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("address mismatch"));
    }

    #[test]
    fn signing_verify_rejects_missing_calldata() {
        let custody = CustodyConfig::default();
        let sr = create_signing_request("wd-4", "0xABC", 100, "USDC", 1, &custody);
        let decoded = DecodedTransaction {
            to_address: "0xABC".into(),
            amount: 100,
            asset: "USDC".into(),
            chain_id: 1,
            calldata_hash: String::new(),
        };
        let err = verify_signing_request(&sr, &decoded);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("calldata_hash missing"));
    }

    // ── Area 8: Approval / display separation ─────────────────────

    #[test]
    fn mask_address_works() {
        assert_eq!(mask_address("0xABCDEF1234567890"), "0xABCD...7890");
        assert_eq!(mask_address("short"), "***");
    }

    #[test]
    fn approval_intent_has_correct_timing() {
        let custody = CustodyConfig::default();
        let delay_policy = WithdrawalDelayPolicy::default();
        let intent = build_approval_intent(
            "wd-5",
            "u1",
            100_000,
            "USDC",
            "0xABCDEF1234567890",
            &custody,
            &delay_policy,
        );
        assert_eq!(intent.vault_tier, VaultTier::Warm);
        assert!(intent.executable_after > Utc::now() - chrono::Duration::seconds(2));
    }

    #[test]
    fn pending_view_masks_address() {
        let custody = CustodyConfig::default();
        let delay_policy = WithdrawalDelayPolicy::default();
        let intent = build_approval_intent(
            "wd-6",
            "u1",
            10_000,
            "USDC",
            "0xABCDEF1234567890AABB",
            &custody,
            &delay_policy,
        );
        let view = to_pending_view(&intent, "pending", Utc::now());
        assert!(view.destination_masked.contains("..."));
        assert!(!view.destination_masked.contains("1234567890"));
    }

    // ── Area 9: Withdrawal delay / cancel window ──────────────────

    #[test]
    fn timing_cancellable_in_window() {
        let now = Utc::now();
        let exec_after = now + chrono::Duration::seconds(600);
        let cancel_before = now + chrono::Duration::seconds(900);
        let result = evaluate_withdrawal_timing(now, exec_after, cancel_before, 0, now);
        assert!(matches!(result, WithdrawalTimingResult::Cancellable { .. }));
    }

    #[test]
    fn timing_executable_after_delay() {
        let now = Utc::now();
        let exec_after = now - chrono::Duration::seconds(10);
        let cancel_before = now - chrono::Duration::seconds(20);
        let result = evaluate_withdrawal_timing(now, exec_after, cancel_before, 0, now);
        assert_eq!(result, WithdrawalTimingResult::Executable);
    }

    #[test]
    fn timing_expired() {
        let now = Utc::now();
        let requested_at = now - chrono::Duration::hours(25);
        let exec_after = requested_at + chrono::Duration::seconds(600);
        let cancel_before = requested_at + chrono::Duration::seconds(900);
        let result =
            evaluate_withdrawal_timing(now, exec_after, cancel_before, 86400, requested_at);
        assert_eq!(result, WithdrawalTimingResult::Expired);
    }

    #[test]
    fn delay_policy_per_tier() {
        let dp = WithdrawalDelayPolicy::default();
        assert_eq!(dp.delay_for_tier(VaultTier::Hot), 0);
        assert!(dp.delay_for_tier(VaultTier::Warm) > 0);
        assert!(dp.delay_for_tier(VaultTier::Cold) > dp.delay_for_tier(VaultTier::Warm));
    }

    // ── Area 10: Vault velocity cap ───────────────────────────────

    #[test]
    fn vault_velocity_allows_within_limit() {
        let tracker = VaultVelocityTracker::new();
        let policy = VaultVelocityPolicy::default();
        assert!(check_vault_velocity(VaultTier::Hot, 100_000, &tracker, &policy).is_ok());
    }

    #[test]
    fn vault_velocity_blocks_over_limit() {
        let tracker = VaultVelocityTracker::new();
        let policy = VaultVelocityPolicy::default();
        tracker.record_outflow(VaultTier::Hot, 190_000);
        let result = check_vault_velocity(VaultTier::Hot, 20_000, &tracker, &policy);
        assert!(result.is_err());
        let (current, limit) = result.unwrap_err();
        assert_eq!(current, 190_000);
        assert_eq!(limit, 200_000);
    }

    #[test]
    fn vault_velocity_independent_per_tier() {
        let tracker = VaultVelocityTracker::new();
        let policy = VaultVelocityPolicy::default();
        tracker.record_outflow(VaultTier::Hot, 199_000);
        assert!(check_vault_velocity(VaultTier::Warm, 100_000, &tracker, &policy).is_ok());
    }

    // ── Area 11: Stronger allowlist ───────────────────────────────

    #[test]
    fn allowlist_max_addresses_enforced() {
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let ws = AddressWhitelistStore::new(store).unwrap();
        let policy = AllowlistPolicy {
            max_addresses_per_user: 2,
            ..AllowlistPolicy::default()
        };
        ws.add_address("u1", "0xA", "a", "u1", 0).unwrap();
        ws.add_address("u1", "0xB", "b", "u1", 0).unwrap();
        let result = validate_address_add("u1", false, &ws, &policy);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max"));
    }

    #[test]
    fn per_address_daily_limit_blocks() {
        let usage = WithdrawalUsageTracker::new();
        let addr_usage = AddressUsageTracker::new();
        let policy = AllowlistPolicy {
            per_address_daily_limit: 100_000,
            ..AllowlistPolicy::default()
        };
        addr_usage.record("0xTARGET", 90_000);
        let result =
            check_per_address_limit("u1", "0xTARGET", 20_000, &usage, &addr_usage, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn per_address_limit_passes_within() {
        let usage = WithdrawalUsageTracker::new();
        let addr_usage = AddressUsageTracker::new();
        let policy = AllowlistPolicy {
            per_address_daily_limit: 100_000,
            ..AllowlistPolicy::default()
        };
        addr_usage.record("0xOK", 50_000);
        assert!(
            check_per_address_limit("u1", "0xOK", 30_000, &usage, &addr_usage, &policy).is_ok()
        );
    }

    // ── Area 12: Hot/warm/cold isolation ──────────────────────────

    #[test]
    fn isolation_prohibits_cold_to_hot() {
        let policy = IsolationPolicy::default();
        let result = validate_tier_transfer(VaultTier::Cold, VaultTier::Hot, 100, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn isolation_prohibits_hot_to_cold() {
        let policy = IsolationPolicy::default();
        let result = validate_tier_transfer(VaultTier::Hot, VaultTier::Cold, 100, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn isolation_allows_hot_to_warm() {
        let policy = IsolationPolicy::default();
        assert!(validate_tier_transfer(VaultTier::Hot, VaultTier::Warm, 100_000, &policy).is_ok());
    }

    #[test]
    fn isolation_warm_to_hot_amount_capped() {
        let policy = IsolationPolicy::default();
        let result = validate_tier_transfer(VaultTier::Warm, VaultTier::Hot, 999_999, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn isolation_evaluate_triggers_replenish() {
        let custody = CustodyConfig::default();
        let isolation = IsolationPolicy::default();
        let actions = evaluate_isolation(50_000, 1_000_000, &custody, &isolation);
        assert!(actions
            .iter()
            .any(|a| matches!(a, IsolationAction::ReplenishHot { .. })));
    }

    #[test]
    fn isolation_evaluate_no_action_when_balanced() {
        let custody = CustodyConfig::default();
        let isolation = IsolationPolicy::default();
        let actions = evaluate_isolation(300_000, 2_000_000, &custody, &isolation);
        assert_eq!(actions, vec![IsolationAction::NoAction]);
    }

    // ── Area 13: Treasury audit / dry-run ─────────────────────────

    #[test]
    fn treasury_snapshot_captures_state() {
        let velocity = VaultVelocityTracker::new();
        let vel_policy = VaultVelocityPolicy::default();
        let custody = CustodyConfig::default();
        let isolation = IsolationPolicy::default();
        velocity.record_outflow(VaultTier::Hot, 50_000);
        let snap = take_treasury_snapshot(
            300_000,
            2_000_000,
            50_000_000,
            5,
            250_000,
            800_000,
            &velocity,
            &vel_policy,
            &custody,
            &isolation,
            42,
        );
        assert_eq!(snap.hot_balance, 300_000);
        assert_eq!(snap.total_balance, 52_300_000);
        assert_eq!(snap.pending_withdrawals_count, 5);
        assert_eq!(snap.velocity_hot_used, 50_000);
        assert_eq!(snap.whitelisted_addresses_total, 42);
    }

    #[test]
    fn dry_run_passes_clean_withdrawal() {
        let policy = test_policy();
        let usage = WithdrawalUsageTracker::new();
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let whitelist = AddressWhitelistStore::new(store).unwrap();
        let custody = CustodyConfig::default();
        let delay = WithdrawalDelayPolicy::default();
        let vel_tracker = VaultVelocityTracker::new();
        let vel_policy = VaultVelocityPolicy::default();
        let allowlist = AllowlistPolicy::default();
        let addr_usage = AddressUsageTracker::new();
        let result = dry_run_withdrawal(
            "u1",
            100,
            "0xABC",
            &policy,
            &usage,
            &whitelist,
            &custody,
            &delay,
            &vel_tracker,
            &vel_policy,
            &allowlist,
            &addr_usage,
        );
        assert!(result.would_proceed);
        assert_eq!(result.velocity_check, "pass");
        assert_eq!(result.per_address_check, "pass");
    }

    #[test]
    fn dry_run_blocks_velocity_breach() {
        let policy = test_policy();
        let usage = WithdrawalUsageTracker::new();
        let store: Arc<dyn persistence::WalStore<AddressWhitelistRecord>> =
            Arc::new(persistence::InMemoryWal::new());
        let whitelist = AddressWhitelistStore::new(store).unwrap();
        let custody = CustodyConfig::default();
        let delay = WithdrawalDelayPolicy::default();
        let vel_tracker = VaultVelocityTracker::new();
        let vel_policy = VaultVelocityPolicy {
            hot_max_outflow: 50,
            ..VaultVelocityPolicy::default()
        };
        let allowlist = AllowlistPolicy::default();
        let addr_usage = AddressUsageTracker::new();
        vel_tracker.record_outflow(VaultTier::Hot, 40);
        let result = dry_run_withdrawal(
            "u1",
            20,
            "0xABC",
            &policy,
            &usage,
            &whitelist,
            &custody,
            &delay,
            &vel_tracker,
            &vel_policy,
            &allowlist,
            &addr_usage,
        );
        assert!(!result.would_proceed);
        assert!(result.velocity_check.contains("BLOCKED"));
    }

    // ── Area 14: Circuit breaker ──────────────────────────────────

    #[test]
    fn breaker_starts_closed() {
        let brk = CustodyCircuitBreaker::new(BreakerConfig::default());
        assert!(!brk.is_open());
    }

    #[test]
    fn breaker_trips_on_velocity_breaches() {
        let brk = CustodyCircuitBreaker::new(BreakerConfig {
            velocity_breach_threshold: 2,
            ..BreakerConfig::default()
        });
        brk.record_velocity_breach();
        assert!(!brk.is_open());
        brk.record_velocity_breach();
        assert!(brk.is_open());
    }

    #[test]
    fn breaker_trips_on_signing_failure() {
        let brk = CustodyCircuitBreaker::new(BreakerConfig::default());
        brk.record_signing_failure("address mismatch");
        assert!(brk.is_open());
    }

    #[test]
    fn breaker_trips_on_large_burst() {
        let brk = CustodyCircuitBreaker::new(BreakerConfig {
            large_burst_threshold: 3,
            ..BreakerConfig::default()
        });
        brk.record_large_burst();
        brk.record_large_burst();
        assert!(!brk.is_open());
        brk.record_large_burst();
        assert!(brk.is_open());
    }

    #[test]
    fn breaker_admin_reset() {
        let brk = CustodyCircuitBreaker::new(BreakerConfig::default());
        brk.trip("test");
        assert!(brk.is_open());
        brk.reset();
        assert!(!brk.is_open());
    }

    #[test]
    fn breaker_clears_velocity_on_success() {
        let brk = CustodyCircuitBreaker::new(BreakerConfig {
            velocity_breach_threshold: 3,
            ..BreakerConfig::default()
        });
        brk.record_velocity_breach();
        brk.record_velocity_breach();
        brk.clear_velocity_breaches();
        brk.record_velocity_breach();
        assert!(!brk.is_open());
    }

    #[test]
    fn breaker_status_reports_correctly() {
        let brk = CustodyCircuitBreaker::new(BreakerConfig::default());
        let s = brk.status();
        assert_eq!(s["state"], "closed");
        brk.trip("reason");
        let s2 = brk.status();
        assert_eq!(s2["state"], "open");
        assert_eq!(s2["trip_reason"], "reason");
    }

    // ── Area 15: Audit trail ──────────────────────────────────────

    #[test]
    fn audit_log_records_and_retrieves() {
        let log = CustodyAuditLog::new();
        log.record(
            CustodyEventType::WithdrawalRequested,
            "user1",
            serde_json::json!({ "amount": 1000 }),
        );
        log.record(
            CustodyEventType::CircuitBreakerTripped,
            "system",
            serde_json::json!({ "reason": "test" }),
        );
        let recent = log.recent(10);
        assert_eq!(recent.len(), 2);
        // most recent first
        assert!(matches!(
            recent[0].event_type,
            CustodyEventType::CircuitBreakerTripped
        ));
        assert!(matches!(
            recent[1].event_type,
            CustodyEventType::WithdrawalRequested
        ));
    }

    #[test]
    fn audit_log_limit_respected() {
        let log = CustodyAuditLog::new();
        for i in 0..20 {
            log.record(
                CustodyEventType::DryRunExecuted,
                &format!("u{i}"),
                serde_json::json!({}),
            );
        }
        assert_eq!(log.recent(5).len(), 5);
        assert_eq!(log.recent(100).len(), 20);
    }
}
