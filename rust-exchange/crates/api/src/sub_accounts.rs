// Sub-account registry — institutional firm hierarchy on the
// read side. Maps `user_id → firm_id` so an admin can:
//
//   * Aggregate balances across the firm's sub-accounts
//   * Aggregate risk (margin, position) across the firm
//   * Auto-derive `stp_group_id` from `firm_id` for unified self-
//     trade prevention (the order submit path stays unchanged in this
//     commit — that integration is a follow-up that would touch every
//     trading route; here we only build the registry + read-side
//     aggregation)
//
// What this commit does NOT do:
//
//   * Mutate the existing `user_id` identity model in matching /
//     ledger / risk / wallet. Sub-accounts remain independent
//     identities; the firm relationship is metadata, not a change of
//     identity.
//   * Auto-inject firm_id into orders. That's a separate change to
//     `trading.rs` that the operator owns the rollout of.
//
// The registry is WAL-persisted (file at
// `data/sub_account_registry.jsonl` by default) so the firm mapping
// survives restarts. Single global registry per instance; admin
// endpoints under `/admin/firms/*` mutate via the existing maker-
// checker governance flow.

use std::sync::Arc;

use dashmap::DashMap;
use persistence::WalStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SubAccountMembership {
    pub(crate) user_id: String,
    pub(crate) firm_id: String,
    pub(crate) recorded_at: chrono::DateTime<chrono::Utc>,
    pub(crate) recorded_by: String,
}

pub(crate) struct SubAccountRegistry {
    membership: DashMap<String, SubAccountMembership>, // user_id → record
    by_firm: DashMap<String, Vec<String>>,             // firm_id → user_ids
    wal: Arc<dyn WalStore<SubAccountMembership>>,
}

impl SubAccountRegistry {
    pub(crate) fn new(wal: Arc<dyn WalStore<SubAccountMembership>>) -> anyhow::Result<Self> {
        let registry = Self {
            membership: DashMap::new(),
            by_firm: DashMap::new(),
            wal,
        };
        for record in registry.wal.entries()? {
            // Latest record wins on the user_id key — append-only log
            // with last-write-wins semantics.
            registry.apply_locally(record);
        }
        Ok(registry)
    }

    fn apply_locally(&self, record: SubAccountMembership) {
        // Remove prior firm membership (if any) before installing the
        // new one, so the inverted index stays consistent.
        if let Some(prev) = self.membership.get(&record.user_id) {
            let prev_firm = prev.firm_id.clone();
            drop(prev);
            if prev_firm != record.firm_id {
                if let Some(mut entry) = self.by_firm.get_mut(&prev_firm) {
                    entry.retain(|uid| uid != &record.user_id);
                }
            }
        }
        self.by_firm
            .entry(record.firm_id.clone())
            .or_default()
            .push(record.user_id.clone());
        self.membership.insert(record.user_id.clone(), record);
    }

    /// Register or update a user's firm membership. Persists to WAL
    /// then updates the in-memory indexes.
    pub(crate) fn upsert(&self, record: SubAccountMembership) -> anyhow::Result<()> {
        self.wal.append(&record)?;
        self.apply_locally(record);
        Ok(())
    }

    pub(crate) fn firm_for(&self, user_id: &str) -> Option<String> {
        self.membership
            .get(user_id)
            .map(|entry| entry.value().firm_id.clone())
    }

    /// All user_ids belonging to a firm. Returned in insertion order,
    /// deduplicated.
    pub(crate) fn members_of(&self, firm_id: &str) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        let mut ordered = Vec::new();
        if let Some(entry) = self.by_firm.get(firm_id) {
            for uid in entry.value().iter() {
                if seen.insert(uid.clone()) {
                    ordered.push(uid.clone());
                }
            }
        }
        ordered
    }

    pub(crate) fn list_firms(&self) -> Vec<String> {
        let mut firms: Vec<String> = self
            .by_firm
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        firms.sort();
        firms
    }
}

/// Aggregate read-side view: sum a per-user metric across all members
/// of a firm. Caller supplies the lookup closure (e.g. ledger balance,
/// position size). Returns `(firm_id, member_ids, aggregated_value)`.
pub(crate) fn aggregate_firm<F>(
    registry: &SubAccountRegistry,
    firm_id: &str,
    per_user_metric: F,
) -> (String, Vec<String>, i128)
where
    F: Fn(&str) -> i64,
{
    let members = registry.members_of(firm_id);
    let total: i128 = members
        .iter()
        .map(|uid| per_user_metric(uid) as i128)
        .sum();
    (firm_id.to_string(), members, total)
}

// ── HTTP routes ────────────────────────────────────────────────

#[allow(unused_imports)]
use super::*;

#[derive(Debug, serde::Deserialize)]
struct UpsertMembershipRequest {
    user_id: String,
    firm_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct MembersQuery {
    firm_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct FirmAggregateQuery {
    firm_id: String,
}

/// Routes:
///   POST /admin/firms/membership { user_id, firm_id }
///   GET  /admin/firms/members?firm_id=X
///   GET  /admin/firms
///   GET  /admin/firms/balance?firm_id=X  — aggregate cash balance
pub(crate) fn build_routes(
    registry: Arc<SubAccountRegistry>,
    ledger: Arc<LedgerService>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let reg1 = registry.clone();
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let upsert_route = warp::path!("admin" / "firms" / "membership")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: UpsertMembershipRequest| {
                let reg = reg1.clone();
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;
                    let record = SubAccountMembership {
                        user_id: req.user_id.clone(),
                        firm_id: req.firm_id.clone(),
                        recorded_at: chrono::Utc::now(),
                        recorded_by: principal.subject.clone(),
                    };
                    reg.upsert(record)
                        .map_err(|e| reject_internal_error(e))?;
                    tracing::info!(
                        admin = %principal.subject,
                        user_id = %req.user_id,
                        firm_id = %req.firm_id,
                        "firm membership upsert"
                    );
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "user_id": req.user_id,
                        "firm_id": req.firm_id,
                    })))
                }
            },
        );

    let reg2 = registry.clone();
    let ip2 = ip_rate_limiter.clone();
    let members_route = warp::path!("admin" / "firms" / "members")
        .and(warp::get())
        .and(with_principal())
        .and(warp::query::<MembersQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: MembersQuery,
                  remote: Option<SocketAddr>| {
                let reg = reg2.clone();
                let ip_rl = ip2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    let members = reg.members_of(&query.firm_id);
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "firm_id": query.firm_id,
                        "members": members,
                    })))
                }
            },
        );

    let reg3 = registry.clone();
    let ip3 = ip_rate_limiter.clone();
    let list_firms_route = warp::path!("admin" / "firms")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let reg = reg3.clone();
                let ip_rl = ip3.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    let firms = reg.list_firms();
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "firms": firms,
                    })))
                }
            },
        );

    let reg4 = registry;
    let ledger4 = ledger;
    let ip4 = ip_rate_limiter;
    let balance_route = warp::path!("admin" / "firms" / "balance")
        .and(warp::get())
        .and(with_principal())
        .and(warp::query::<FirmAggregateQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: FirmAggregateQuery,
                  remote: Option<SocketAddr>| {
                let reg = reg4.clone();
                let ledger = ledger4.clone();
                let ip_rl = ip4.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    let (firm_id, members, total) = aggregate_firm(
                        reg.as_ref(),
                        &query.firm_id,
                        |uid| ledger.cash_available_balance(uid),
                    );
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "firm_id": firm_id,
                        "members": members,
                        "aggregate_cash_balance": total.to_string(),
                    })))
                }
            },
        );

    upsert_route
        .or(members_route)
        .unify()
        .or(list_firms_route)
        .unify()
        .or(balance_route)
        .unify()
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::InMemoryWal;

    fn record(user: &str, firm: &str, by: &str) -> SubAccountMembership {
        SubAccountMembership {
            user_id: user.to_string(),
            firm_id: firm.to_string(),
            recorded_at: chrono::Utc::now(),
            recorded_by: by.to_string(),
        }
    }

    fn registry() -> SubAccountRegistry {
        let wal = Arc::new(InMemoryWal::<SubAccountMembership>::new());
        SubAccountRegistry::new(wal).unwrap()
    }

    #[test]
    fn upsert_records_membership_and_inverted_index() {
        let r = registry();
        r.upsert(record("u1", "firm-a", "admin")).unwrap();
        r.upsert(record("u2", "firm-a", "admin")).unwrap();
        assert_eq!(r.firm_for("u1"), Some("firm-a".into()));
        assert_eq!(r.firm_for("u2"), Some("firm-a".into()));
        let members = r.members_of("firm-a");
        assert!(members.contains(&"u1".to_string()));
        assert!(members.contains(&"u2".to_string()));
    }

    #[test]
    fn moving_user_between_firms_updates_inverted_index() {
        let r = registry();
        r.upsert(record("u1", "firm-a", "admin")).unwrap();
        r.upsert(record("u1", "firm-b", "admin")).unwrap();
        assert_eq!(r.firm_for("u1"), Some("firm-b".into()));
        assert!(!r.members_of("firm-a").contains(&"u1".to_string()));
        assert!(r.members_of("firm-b").contains(&"u1".to_string()));
    }

    #[test]
    fn unknown_user_returns_none() {
        let r = registry();
        assert!(r.firm_for("ghost").is_none());
        assert!(r.members_of("ghost-firm").is_empty());
    }

    #[test]
    fn recovery_from_wal_rebuilds_state() {
        let wal: Arc<dyn WalStore<SubAccountMembership>> =
            Arc::new(InMemoryWal::<SubAccountMembership>::new());
        let r1 = SubAccountRegistry::new(wal.clone()).unwrap();
        r1.upsert(record("u1", "firm-a", "admin")).unwrap();
        r1.upsert(record("u2", "firm-a", "admin")).unwrap();
        // Move u1 to firm-b.
        r1.upsert(record("u1", "firm-b", "admin")).unwrap();

        // Construct a fresh registry from the same WAL — should arrive
        // at the same final state.
        let r2 = SubAccountRegistry::new(wal).unwrap();
        assert_eq!(r2.firm_for("u1"), Some("firm-b".into()));
        assert_eq!(r2.firm_for("u2"), Some("firm-a".into()));
        assert!(!r2.members_of("firm-a").contains(&"u1".to_string()));
        assert!(r2.members_of("firm-b").contains(&"u1".to_string()));
    }

    #[test]
    fn aggregate_sums_per_user_metric() {
        let r = registry();
        r.upsert(record("u1", "firm-a", "admin")).unwrap();
        r.upsert(record("u2", "firm-a", "admin")).unwrap();
        r.upsert(record("u3", "firm-b", "admin")).unwrap();
        let balances = |uid: &str| -> i64 {
            match uid {
                "u1" => 100,
                "u2" => 250,
                "u3" => 999, // not in firm-a
                _ => 0,
            }
        };
        let (firm, members, total) = aggregate_firm(&r, "firm-a", balances);
        assert_eq!(firm, "firm-a");
        assert_eq!(members.len(), 2);
        assert_eq!(total, 350);
    }

    #[test]
    fn list_firms_is_sorted_and_deduped() {
        let r = registry();
        r.upsert(record("u1", "zeta", "admin")).unwrap();
        r.upsert(record("u2", "alpha", "admin")).unwrap();
        r.upsert(record("u3", "alpha", "admin")).unwrap();
        let firms = r.list_firms();
        assert_eq!(firms, vec!["alpha".to_string(), "zeta".to_string()]);
    }
}
